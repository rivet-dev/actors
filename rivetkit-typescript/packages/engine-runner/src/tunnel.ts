import type * as protocol from "@rivetkit/engine-runner-protocol";
import type { GatewayId, RequestId } from "@rivetkit/engine-runner-protocol";
import type { Logger } from "pino";
import { type Runner, type RunnerActor, RunnerShutdownError } from "./mod";
import {
	stringifyToClientTunnelMessageKind,
	stringifyToServerTunnelMessageKind,
} from "./stringify";
import {
	arraysEqual,
	idToStr,
	MAX_PAYLOAD_SIZE,
	stringifyError,
	unreachable,
} from "./utils";
import {
	HIBERNATABLE_SYMBOL,
	WebSocketTunnelAdapter,
} from "./websocket-tunnel-adapter";

export interface PendingRequest {
	resolve: (response: Response) => void;
	reject: (error: Error) => void;
	streamController?: ReadableStreamDefaultController<Uint8Array>;
	actorId?: string;
	gatewayId?: GatewayId;
	requestId?: RequestId;
	clientMessageIndex: number;
}

export interface HibernatingWebSocketMetadata {
	gatewayId: GatewayId;
	requestId: RequestId;
	clientMessageIndex: number;
	serverMessageIndex: number;

	path: string;
	headers: Record<string, string>;
}

export class Tunnel {
	#runner: Runner;

	/** Maps request IDs to actor IDs for lookup */
	#requestToActor: Array<{
		gatewayId: GatewayId;
		requestId: RequestId;
		actorId: string;
	}> = [];

	/** Buffer for messages when not connected */
	#bufferedMessages: Array<{
		gatewayId: GatewayId;
		requestId: RequestId;
		messageKind: protocol.ToServerTunnelMessageKind;
	}> = [];

	get log(): Logger | undefined {
		return this.#runner.log;
	}

	constructor(runner: Runner) {
		this.#runner = runner;
	}

	start(): void {
		// No-op - kept for compatibility
	}

	resendBufferedEvents(): void {
		if (this.#bufferedMessages.length === 0) {
			return;
		}

		this.log?.info({
			msg: "resending buffered tunnel messages",
			count: this.#bufferedMessages.length,
		});

		const messages = this.#bufferedMessages;
		this.#bufferedMessages = [];

		for (const { gatewayId, requestId, messageKind } of messages) {
			this.#sendMessage(gatewayId, requestId, messageKind);
		}
	}

	shutdown() {
		// NOTE: Pegboard WS already closed at this point, cannot send
		// anything. All teardown logic is handled by pegboard-runner.

		// Reject all pending requests and close all WebSockets for all actors
		// RunnerShutdownError will be explicitly ignored
		for (const [_actorId, actor] of this.#runner.actors) {
			// Reject all pending requests for this actor
			for (const entry of actor.pendingRequests) {
				entry.request.reject(new RunnerShutdownError());
			}
			actor.pendingRequests = [];

			// Close all WebSockets for this actor
			// The WebSocket close event with retry is automatically sent when the
			// runner WS closes, so we only need to notify the client that the WS
			// closed:
			// https://github.com/rivet-dev/rivet/blob/00d4f6a22da178a6f8115e5db50d96c6f8387c2e/engine/packages/pegboard-runner/src/lib.rs#L157
			for (const entry of actor.webSockets) {
				// Only close non-hibernatable websockets to prevent sending
				// unnecessary close messages for websockets that will be hibernated
				if (!entry.ws[HIBERNATABLE_SYMBOL]) {
					entry.ws._closeWithoutCallback(1000, "ws.tunnel_shutdown");
				}
			}
			actor.webSockets = [];
		}

		// Clear the request-to-actor mapping
		this.#requestToActor = [];
	}

	async restoreHibernatingRequests(
		actorId: string,
		metaEntries: HibernatingWebSocketMetadata[],
	) {
		const actor = this.#runner.getActor(actorId);
		if (!actor) {
			throw new Error(
				`Actor ${actorId} not found for restoring hibernating requests`,
			);
		}

		if (actor.hibernationRestored) {
			throw new Error(
				`Actor ${actorId} already restored hibernating requests`,
			);
		}

		this.log?.debug({
			msg: "restoring hibernating requests",
			actorId,
			requests: actor.hibernatingRequests.length,
		});

		// Track all background operations
		const backgroundOperations: Promise<void>[] = [];

		// Process connected WebSockets
		let connectedButNotLoadedCount = 0;
		let restoredCount = 0;
		for (const { gatewayId, requestId } of actor.hibernatingRequests) {
			const requestIdStr = idToStr(requestId);
			const meta = metaEntries.find(
				(entry) =>
					arraysEqual(entry.gatewayId, gatewayId) &&
					arraysEqual(entry.requestId, requestId),
			);

			if (!meta) {
				// Connected but not loaded (not persisted) - close it
				//
				// This may happen if the metadata was not successfully persisted
				this.log?.warn({
					msg: "closing websocket that is not persisted",
					requestId: requestIdStr,
				});

				this.#sendMessage(gatewayId, requestId, {
					tag: "ToServerWebSocketClose",
					val: {
						code: 1000,
						reason: "ws.meta_not_found_during_restore",
						hibernate: false,
					},
				});

				connectedButNotLoadedCount++;
			} else {
				// Both connected and persisted - restore it
				const request = buildRequestForWebSocket(
					meta.path,
					meta.headers,
				);

				// This will call `runner.config.websocket` under the hood to
				// attach the event listeners to the WebSocket.
				// Track this operation to ensure it completes
				const restoreOperation = this.#createWebSocket(
					actorId,
					gatewayId,
					requestId,
					requestIdStr,
					meta.serverMessageIndex,
					true,
					true,
					request,
					meta.path,
					meta.headers,
					false,
				)
					.then(() => {
						// Create a PendingRequest entry to track the message index
						const actor = this.#runner.getActor(actorId);
						if (actor) {
							actor.createPendingRequest(
								gatewayId,
								requestId,
								meta.clientMessageIndex,
							);
						}

						this.log?.info({
							msg: "connection successfully restored",
							actorId,
							requestId: requestIdStr,
						});
					})
					.catch((err) => {
						this.log?.error({
							msg: "error creating websocket during restore",
							requestId: requestIdStr,
							error: stringifyError(err),
						});

						// Close the WebSocket on error
						this.#sendMessage(gatewayId, requestId, {
							tag: "ToServerWebSocketClose",
							val: {
								code: 1011,
								reason: "ws.restore_error",
								hibernate: false,
							},
						});
					});

				backgroundOperations.push(restoreOperation);
				restoredCount++;
			}
		}

		// Process loaded but not connected (stale) - remove them
		let loadedButNotConnectedCount = 0;
		for (const meta of metaEntries) {
			const requestIdStr = idToStr(meta.requestId);
			const isConnected = actor.hibernatingRequests.some(
				(req) =>
					arraysEqual(req.gatewayId, meta.gatewayId) &&
					arraysEqual(req.requestId, meta.requestId),
			);
			if (!isConnected) {
				this.log?.warn({
					msg: "removing stale persisted websocket",
					requestId: requestIdStr,
				});

				const request = buildRequestForWebSocket(
					meta.path,
					meta.headers,
				);

				// Create adapter to register user's event listeners.
				// Pass engineAlreadyClosed=true so close callback won't send tunnel message.
				// Track this operation to ensure it completes
				const cleanupOperation = this.#createWebSocket(
					actorId,
					meta.gatewayId,
					meta.requestId,
					requestIdStr,
					meta.serverMessageIndex,
					true,
					true,
					request,
					meta.path,
					meta.headers,
					true,
				)
					.then((adapter) => {
						// Close the adapter normally - this will fire user's close event handler
						// (which should clean up persistence) and trigger the close callback
						// (which will clean up maps but skip sending tunnel message)
						adapter.close(1000, "ws.stale_metadata");
					})
					.catch((err) => {
						this.log?.error({
							msg: "error creating stale websocket during restore",
							requestId: requestIdStr,
							error: stringifyError(err),
						});
					});

				backgroundOperations.push(cleanupOperation);
				loadedButNotConnectedCount++;
			}
		}

		// Wait for all background operations to complete before finishing
		await Promise.allSettled(backgroundOperations);

		// Mark restoration as complete
		actor.hibernationRestored = true;

		this.log?.info({
			msg: "restored hibernatable websockets",
			actorId,
			restoredCount,
			connectedButNotLoadedCount,
			loadedButNotConnectedCount,
		});
	}

	/**
	 * Called from WebSocketOpen message and when restoring hibernatable WebSockets.
	 *
	 * engineAlreadyClosed will be true if this is only being called to trigger
	 * the close callback and not to send a close message to the server. This
	 * is used specifically to clean up zombie WebSocket connections.
	 */
	async #createWebSocket(
		actorId: string,
		gatewayId: GatewayId,
		requestId: RequestId,
		requestIdStr: string,
		serverMessageIndex: number,
		isHibernatable: boolean,
		isRestoringHibernatable: boolean,
		request: Request,
		path: string,
		headers: Record<string, string>,
		engineAlreadyClosed: boolean,
	): Promise<WebSocketTunnelAdapter> {
		this.log?.debug({
			msg: "createWebSocket creating adapter",
			actorId,
			requestIdStr,
			isHibernatable,
			path,
		});
		// Create WebSocket adapter
		const adapter = new WebSocketTunnelAdapter(
			this,
			actorId,
			requestIdStr,
			serverMessageIndex,
			isHibernatable,
			isRestoringHibernatable,
			request,
			(data: ArrayBuffer | string, isBinary: boolean) => {
				// Send message through tunnel
				const dataBuffer =
					typeof data === "string"
						? (new TextEncoder().encode(data).buffer as ArrayBuffer)
						: data;

				this.#sendMessage(gatewayId, requestId, {
					tag: "ToServerWebSocketMessage",
					val: {
						data: dataBuffer,
						binary: isBinary,
					},
				});
			},
			(code?: number, reason?: string) => {
				// Send close through tunnel if engine doesn't already know it's closed
				if (!engineAlreadyClosed) {
					this.#sendMessage(gatewayId, requestId, {
						tag: "ToServerWebSocketClose",
						val: {
							code: code || null,
							reason: reason || null,
							hibernate: false,
						},
					});
				}

				// Clean up actor tracking
				const actor = this.#runner.getActor(actorId);
				if (actor) {
					actor.deleteWebSocket(gatewayId, requestId);
					actor.deletePendingRequest(gatewayId, requestId);
				}

				// Clean up request-to-actor mapping
				this.#removeRequestToActor(gatewayId, requestId);
			},
		);

		// Get actor and add websocket to it
		const actor = this.#runner.getActor(actorId);
		if (!actor) {
			throw new Error(`Actor ${actorId} not found`);
		}

		actor.setWebSocket(gatewayId, requestId, adapter);
		this.addRequestToActor(gatewayId, requestId, actorId);

		// Call WebSocket handler. This handler will add event listeners
		// for `open`, etc. Pass the VirtualWebSocket (not the adapter) to the actor.
		await this.#runner.config.websocket(
			this.#runner,
			actorId,
			adapter.websocket,
			gatewayId,
			requestId,
			request,
			path,
			headers,
			isHibernatable,
			isRestoringHibernatable,
		);

		return adapter;
	}

	addRequestToActor(
		gatewayId: GatewayId,
		requestId: RequestId,
		actorId: string,
	) {
		this.#requestToActor.push({ gatewayId, requestId, actorId });
	}

	#removeRequestToActor(gatewayId: GatewayId, requestId: RequestId) {
		const index = this.#requestToActor.findIndex(
			(entry) =>
				arraysEqual(entry.gatewayId, gatewayId) &&
				arraysEqual(entry.requestId, requestId),
		);
		if (index !== -1) {
			this.#requestToActor.splice(index, 1);
		}
	}

	getRequestActor(
		gatewayId: GatewayId,
		requestId: RequestId,
	): RunnerActor | undefined {
		const entry = this.#requestToActor.find(
			(entry) =>
				arraysEqual(entry.gatewayId, gatewayId) &&
				arraysEqual(entry.requestId, requestId),
		);

		if (!entry) {
			this.log?.warn({
				msg: "missing requestToActor entry",
				requestId: idToStr(requestId),
			});
			return undefined;
		}

		const actor = this.#runner.getActor(entry.actorId);
		if (!actor) {
			this.log?.warn({
				msg: "missing actor for requestToActor lookup",
				requestId: idToStr(requestId),
				actorId: entry.actorId,
			});
			return undefined;
		}

		return actor;
	}

	async getAndWaitForRequestActor(
		gatewayId: GatewayId,
		requestId: RequestId,
	): Promise<RunnerActor | undefined> {
		const actor = this.getRequestActor(gatewayId, requestId);
		if (!actor) return;
		await actor.actorStartPromise.promise;
		return actor;
	}

	#sendMessage(
		gatewayId: GatewayId,
		requestId: RequestId,
		messageKind: protocol.ToServerTunnelMessageKind,
	) {
		// Buffer message if not connected
		if (!this.#runner.getPegboardWebSocketIfReady()) {
			this.log?.debug({
				msg: "buffering tunnel message, socket not connected to engine",
				requestId: idToStr(requestId),
				message: stringifyToServerTunnelMessageKind(messageKind),
			});
			this.#bufferedMessages.push({ gatewayId, requestId, messageKind });
			return;
		}

		// Get or initialize message index for this request
		//
		// We don't have to wait for the actor to start since we're not calling
		// any callbacks on the actor
		const gatewayIdStr = idToStr(gatewayId);
		const requestIdStr = idToStr(requestId);
		const actor = this.getRequestActor(gatewayId, requestId);
		if (!actor) {
			this.log?.warn({
				msg: "cannot send tunnel message, actor not found",
				gatewayId: gatewayIdStr,
				requestId: requestIdStr,
			});
			return;
		}

		// Get message index from pending request
		let clientMessageIndex: number;
		const pending = actor.getPendingRequest(gatewayId, requestId);
		if (pending) {
			clientMessageIndex = pending.clientMessageIndex;
			pending.clientMessageIndex++;
		} else {
			// No pending request
			this.log?.warn({
				msg: "missing pending request for send message, defaulting to message index 0",
				gatewayId: gatewayIdStr,
				requestId: requestIdStr,
			});
			clientMessageIndex = 0;
		}

		// Build message ID from gatewayId + requestId + messageIndex
		const messageId: protocol.MessageId = {
			gatewayId,
			requestId,
			messageIndex: clientMessageIndex,
		};
		const messageIdStr = `${idToStr(messageId.gatewayId)}-${idToStr(messageId.requestId)}-${messageId.messageIndex}`;

		this.log?.debug({
			msg: "sending tunnel msg",
			messageId: messageIdStr,
			gatewayId: gatewayIdStr,
			requestId: requestIdStr,
			messageIndex: clientMessageIndex,
			message: stringifyToServerTunnelMessageKind(messageKind),
		});

		// Send message
		const message: protocol.ToServer = {
			tag: "ToServerTunnelMessage",
			val: {
				messageId,
				messageKind,
			},
		};
		this.#runner.__sendToServer(message);
	}

	closeActiveRequests(actor: RunnerActor) {
		const actorId = actor.actorId;

		// Terminate all requests for this actor. This will no send a
		// ToServerResponse* message since the actor will no longer be loaded.
		// The gateway is responsible for closing the request.
		for (const entry of actor.pendingRequests) {
			entry.request.reject(new Error(`Actor ${actorId} stopped`));
			if (entry.gatewayId && entry.requestId) {
				this.#removeRequestToActor(entry.gatewayId, entry.requestId);
			}
		}

		// Close all WebSockets. Only send close event to non-HWS. The gateway is
		// responsible for hibernating HWS and closing regular WS.
		for (const entry of actor.webSockets) {
			const isHibernatable = entry.ws[HIBERNATABLE_SYMBOL];
			if (!isHibernatable) {
				entry.ws._closeWithoutCallback(1000, "actor.stopped");
			}
			// Note: request-to-actor mapping is cleaned up in the close callback
		}
	}

	async #fetch(
		actorId: string,
		gatewayId: protocol.GatewayId,
		requestId: protocol.RequestId,
		request: Request,
	): Promise<Response> {
		// Validate actor exists
		if (!this.#runner.hasActor(actorId)) {
			this.log?.warn({
				msg: "ignoring request for unknown actor",
				actorId,
			});

			// NOTE: This is a special response that will cause Guard to retry the request
			//
			// See should_retry_request_inner
			// https://github.com/rivet-dev/rivet/blob/222dae87e3efccaffa2b503de40ecf8afd4e31eb/engine/packages/guard-core/src/proxy_service.rs#L2458
			return new Response("Actor not found", {
				status: 503,
				headers: { "x-rivet-error": "runner.actor_not_found" },
			});
		}

		const fetchHandler = this.#runner.config.fetch(
			this.#runner,
			actorId,
			gatewayId,
			requestId,
			request,
		);

		if (!fetchHandler) {
			return new Response("Not Implemented", { status: 501 });
		}

		return fetchHandler;
	}

	async handleTunnelMessage(message: protocol.ToClientTunnelMessage) {
		// Parse the gateway ID, request ID, and message index from the messageId
		const { gatewayId, requestId, messageIndex } = message.messageId;

		const gatewayIdStr = idToStr(gatewayId);
		const requestIdStr = idToStr(requestId);
		this.log?.debug({
			msg: "receive tunnel msg",
			gatewayId: gatewayIdStr,
			requestId: requestIdStr,
			messageIndex: message.messageId.messageIndex,
			message: stringifyToClientTunnelMessageKind(message.messageKind),
		});

		switch (message.messageKind.tag) {
			case "ToClientRequestStart":
				await this.#handleRequestStart(
					gatewayId,
					requestId,
					message.messageKind.val,
				);
				break;
			case "ToClientRequestChunk":
				await this.#handleRequestChunk(
					gatewayId,
					requestId,
					message.messageKind.val,
				);
				break;
			case "ToClientRequestAbort":
				await this.#handleRequestAbort(gatewayId, requestId);
				break;
			case "ToClientWebSocketOpen":
				await this.#handleWebSocketOpen(
					gatewayId,
					requestId,
					message.messageKind.val,
				);
				break;
			case "ToClientWebSocketMessage": {
				await this.#handleWebSocketMessage(
					gatewayId,
					requestId,
					messageIndex,
					message.messageKind.val,
				);
				break;
			}
			case "ToClientWebSocketClose":
				await this.#handleWebSocketClose(
					gatewayId,
					requestId,
					message.messageKind.val,
				);
				break;
			default:
				unreachable(message.messageKind);
		}
	}

	async #handleRequestStart(
		gatewayId: GatewayId,
		requestId: RequestId,
		req: protocol.ToClientRequestStart,
	) {
		// Track this request for the actor
		const requestIdStr = idToStr(requestId);
		const actor = await this.#runner.getAndWaitForActor(req.actorId);
		if (!actor) {
			this.log?.warn({
				msg: "ignoring request for unknown actor",
				actorId: req.actorId,
				requestId: requestIdStr,
			});

			const body = new TextEncoder().encode("Actor not found");

			this.#runner.__sendToServer({
				tag: "ToServerTunnelMessage",
				val: {
					messageId: {
						gatewayId,
						requestId,
						messageIndex: 0,
					},
					messageKind: {
						tag: "ToServerResponseStart",
						val: {
							status: 503,
							headers: new Map([
								["x-rivet-error", "runner.actor_not_found"],
								["content-length", String(body.byteLength)],
							]),
							body: body.buffer,
							stream: false,
						},
					},
				},
			});
			return;
		}

		// Add to request-to-actor mapping
		this.addRequestToActor(gatewayId, requestId, req.actorId);

		try {
			// Convert headers map to Headers object
			const headers = new Headers();
			for (const [key, value] of req.headers) {
				headers.append(key, value);
			}

			// Create Request object
			const request = new Request(`http://localhost${req.path}`, {
				method: req.method,
				headers,
				body: req.body ? new Uint8Array(req.body) : undefined,
			});

			// Handle streaming request
			if (req.stream) {
				// Create a stream for the request body
				const stream = new ReadableStream<Uint8Array>({
					start: (controller) => {
						// Store controller for chunks
						const existing = actor.getPendingRequest(
							gatewayId,
							requestId,
						);
						if (existing) {
							existing.streamController = controller;
							existing.actorId = req.actorId;
							existing.gatewayId = gatewayId;
							existing.requestId = requestId;
						} else {
							actor.createPendingRequestWithStreamController(
								gatewayId,
								requestId,
								0,
								controller,
							);
						}
					},
				});

				// Create request with streaming body
				const streamingRequest = new Request(request, {
					body: stream,
					duplex: "half",
				} as any);

				// Call fetch handler with validation
				const response = await this.#fetch(
					req.actorId,
					gatewayId,
					requestId,
					streamingRequest,
				);
				await this.#sendResponse(
					actor.actorId,
					actor.generation,
					gatewayId,
					requestId,
					response,
				);
			} else {
				// Non-streaming request
				// Create a pending request entry to track messageIndex for the response
				actor.createPendingRequest(gatewayId, requestId, 0);

				const response = await this.#fetch(
					req.actorId,
					gatewayId,
					requestId,
					request,
				);
				await this.#sendResponse(
					actor.actorId,
					actor.generation,
					gatewayId,
					requestId,
					response,
				);
			}
		} catch (error) {
			if (error instanceof RunnerShutdownError) {
				this.log?.debug({ msg: "catught runner shutdown error" });
			} else {
				this.log?.error({ msg: "error handling request", error });
				this.#sendResponseError(
					actor.actorId,
					actor.generation,
					gatewayId,
					requestId,
					500,
					"Internal Server Error",
				);
			}
		} finally {
			// Clean up request tracking
			if (this.#runner.hasActor(req.actorId, actor.generation)) {
				actor.deletePendingRequest(gatewayId, requestId);
				this.#removeRequestToActor(gatewayId, requestId);
			}
		}
	}

	async #handleRequestChunk(
		gatewayId: GatewayId,
		requestId: RequestId,
		chunk: protocol.ToClientRequestChunk,
	) {
		const actor = await this.getAndWaitForRequestActor(
			gatewayId,
			requestId,
		);
		if (actor) {
			const pending = actor.getPendingRequest(gatewayId, requestId);
			if (pending?.streamController) {
				pending.streamController.enqueue(new Uint8Array(chunk.body));
				if (chunk.finish) {
					pending.streamController.close();
					actor.deletePendingRequest(gatewayId, requestId);
					this.#removeRequestToActor(gatewayId, requestId);
				}
			}
		}
	}

	async #handleRequestAbort(gatewayId: GatewayId, requestId: RequestId) {
		const actor = await this.getAndWaitForRequestActor(
			gatewayId,
			requestId,
		);
		if (actor) {
			const pending = actor.getPendingRequest(gatewayId, requestId);
			if (pending?.streamController) {
				pending.streamController.error(new Error("Request aborted"));
			}
			actor.deletePendingRequest(gatewayId, requestId);
			this.#removeRequestToActor(gatewayId, requestId);
		}
	}

	async #sendResponse(
		actorId: string,
		generation: number,
		gatewayId: GatewayId,
		requestId: ArrayBuffer,
		response: Response,
	) {
		if (!this.#runner.hasActor(actorId, generation)) {
			this.log?.warn({
				msg: "actor not loaded to send response, assuming gateway has closed request",
				actorId,
				generation,
				requestId,
			});
			return;
		}

		// Always treat responses as non-streaming for now
		// In the future, we could detect streaming responses based on:
		// - Transfer-Encoding: chunked
		// - Content-Type: text/event-stream
		// - Explicit stream flag from the handler

		// Read the body first to get the actual content
		const body = response.body ? await response.arrayBuffer() : null;

		if (body && body.byteLength > MAX_PAYLOAD_SIZE) {
			throw new Error("Response body too large");
		}

		// Convert headers to map and add Content-Length if not present
		const headers = new Map<string, string>();
		response.headers.forEach((value, key) => {
			headers.set(key, value);
		});

		// Add Content-Length header if we have a body and it's not already set
		if (body && !headers.has("content-length")) {
			headers.set("content-length", String(body.byteLength));
		}

		// Send as non-streaming response if actor has not stopped
		this.#sendMessage(gatewayId, requestId, {
			tag: "ToServerResponseStart",
			val: {
				status: response.status as protocol.u16,
				headers,
				body: body || null,
				stream: false,
			},
		});
	}

	#sendResponseError(
		actorId: string,
		generation: number,
		gatewayId: GatewayId,
		requestId: ArrayBuffer,
		status: number,
		message: string,
	) {
		if (!this.#runner.hasActor(actorId, generation)) {
			this.log?.warn({
				msg: "actor not loaded to send response, assuming gateway has closed request",
				actorId,
				generation,
				requestId,
			});
			return;
		}

		const headers = new Map<string, string>();
		headers.set("content-type", "text/plain");

		this.#sendMessage(gatewayId, requestId, {
			tag: "ToServerResponseStart",
			val: {
				status: status as protocol.u16,
				headers,
				body: new TextEncoder().encode(message).buffer as ArrayBuffer,
				stream: false,
			},
		});
	}

	async #handleWebSocketOpen(
		gatewayId: GatewayId,
		requestId: RequestId,
		open: protocol.ToClientWebSocketOpen,
	) {
		// NOTE: This method is safe to be async since we will not receive any
		// further WebSocket events until we send a ToServerWebSocketOpen
		// tunnel message. We can do any async logic we need to between those two events.
		//
		// Sending a ToServerWebSocketClose will terminate the WebSocket early.

		const requestIdStr = idToStr(requestId);

		// Validate actor exists
		const actor = await this.#runner.getAndWaitForActor(open.actorId);
		if (!actor) {
			this.log?.warn({
				msg: "ignoring websocket for unknown actor",
				actorId: open.actorId,
			});

			// NOTE: Closing a WebSocket before open is equivalent to a Service
			// Unavailable error and will cause Guard to retry the request
			//
			// See
			// https://github.com/rivet-dev/rivet/blob/222dae87e3efccaffa2b503de40ecf8afd4e31eb/engine/packages/pegboard-gateway/src/lib.rs#L238
			this.#sendMessage(gatewayId, requestId, {
				tag: "ToServerWebSocketClose",
				val: {
					code: 1011,
					reason: "Actor not found",
					hibernate: false,
				},
			});
			return;
		}

		// Close existing WebSocket if one already exists for this request ID.
		// This should never happen, but prevents any potential duplicate
		// WebSockets from retransmits.
		const existingAdapter = actor.getWebSocket(gatewayId, requestId);
		if (existingAdapter) {
			this.log?.warn({
				msg: "closing existing websocket for duplicate open event for the same request id",
				requestId: requestIdStr,
			});
			// Close without sending a message through the tunnel since the server
			// already knows about the new connection
			existingAdapter._closeWithoutCallback(1000, "ws.duplicate_open");
		}

		// Create WebSocket
		try {
			const request = buildRequestForWebSocket(
				open.path,
				Object.fromEntries(open.headers),
			);

			const canHibernate =
				this.#runner.config.hibernatableWebSocket.canHibernate(
					actor.actorId,
					gatewayId,
					requestId,
					request,
				);

			// #createWebSocket will call `runner.config.websocket` under the
			// hood to add the event listeners for open, etc. If this handler
			// throws, then the WebSocket will be closed before sending the
			// open event.
			const adapter = await this.#createWebSocket(
				actor.actorId,
				gatewayId,
				requestId,
				requestIdStr,
				0,
				canHibernate,
				false,
				request,
				open.path,
				Object.fromEntries(open.headers),
				false,
			);

			// Create a PendingRequest entry to track the message index
			actor.createPendingRequest(gatewayId, requestId, 0);

			// Open the WebSocket after `config.socket` so (a) the event
			// handlers can be added and (b) any errors in `config.websocket`
			// will cause the WebSocket to terminate before the open event.
			this.#sendMessage(gatewayId, requestId, {
				tag: "ToServerWebSocketOpen",
				val: {
					canHibernate,
				},
			});

			// Dispatch open event
			adapter._handleOpen(requestId);
		} catch (error) {
			this.log?.error({ msg: "error handling websocket open", error });

			// TODO: Call close event on adapter if needed

			// Send close on error
			this.#sendMessage(gatewayId, requestId, {
				tag: "ToServerWebSocketClose",
				val: {
					code: 1011,
					reason: "Server Error",
					hibernate: false,
				},
			});

			// Clean up actor tracking
			actor.deleteWebSocket(gatewayId, requestId);
			actor.deletePendingRequest(gatewayId, requestId);
			this.#removeRequestToActor(gatewayId, requestId);
		}
	}

	async #handleWebSocketMessage(
		gatewayId: GatewayId,
		requestId: RequestId,
		serverMessageIndex: number,
		msg: protocol.ToClientWebSocketMessage,
	) {
		const actor = await this.getAndWaitForRequestActor(
			gatewayId,
			requestId,
		);
		if (actor) {
			const adapter = actor.getWebSocket(gatewayId, requestId);
			if (adapter) {
				const data = msg.binary
					? new Uint8Array(msg.data)
					: new TextDecoder().decode(new Uint8Array(msg.data));

				adapter._handleMessage(
					requestId,
					data,
					serverMessageIndex,
					msg.binary,
				);
				return;
			}
		}

		// TODO: This will never retransmit the socket and the socket will close
		this.log?.warn({
			msg: "missing websocket for incoming websocket message, this may indicate the actor stopped before processing a message",
			requestId,
		});
	}

	sendHibernatableWebSocketMessageAck(
		gatewayId: ArrayBuffer,
		requestId: ArrayBuffer,
		clientMessageIndex: number,
	) {
		const requestIdStr = idToStr(requestId);

		this.log?.debug({
			msg: "ack ws msg",
			requestId: requestIdStr,
			index: clientMessageIndex,
		});

		if (clientMessageIndex < 0 || clientMessageIndex > 65535)
			throw new Error("Invalid websocket ack index");

		// Get the actor to find the gatewayId
		//
		// We don't have to wait for the actor to start since we're not calling
		// any callbacks on the actor
		const actor = this.getRequestActor(gatewayId, requestId);
		if (!actor) {
			this.log?.warn({
				msg: "cannot send websocket ack, actor not found",
				requestId: requestIdStr,
			});
			return;
		}

		// Get gatewayId from the pending request
		const pending = actor.getPendingRequest(gatewayId, requestId);
		if (!pending?.gatewayId) {
			this.log?.warn({
				msg: "cannot send websocket ack, gatewayId not found in pending request",
				requestId: requestIdStr,
			});
			return;
		}

		// Send the ack message
		this.#sendMessage(pending.gatewayId, requestId, {
			tag: "ToServerWebSocketMessageAck",
			val: {
				index: clientMessageIndex,
			},
		});
	}

	async #handleWebSocketClose(
		gatewayId: GatewayId,
		requestId: RequestId,
		close: protocol.ToClientWebSocketClose,
	) {
		const actor = await this.getAndWaitForRequestActor(
			gatewayId,
			requestId,
		);
		if (actor) {
			const adapter = actor.getWebSocket(gatewayId, requestId);
			if (adapter) {
				// We don't need to send a close response
				adapter._handleClose(
					requestId,
					close.code || undefined,
					close.reason || undefined,
				);
				actor.deleteWebSocket(gatewayId, requestId);
				actor.deletePendingRequest(gatewayId, requestId);
				this.#removeRequestToActor(gatewayId, requestId);
			}
		}
	}
}

/**
 * Builds a request that represents the incoming request for a given WebSocket.
 *
 * This request is not a real request and will never be sent. It's used to be passed to the actor to behave like a real incoming request.
 */
function buildRequestForWebSocket(
	path: string,
	headers: Record<string, string>,
): Request {
	// We need to manually ensure the original Upgrade/Connection WS
	// headers are present
	const fullHeaders = {
		...headers,
		Upgrade: "websocket",
		Connection: "Upgrade",
	};

	if (!path.startsWith("/")) {
		throw new Error("Path must start with leading slash");
	}

	const request = new Request(`http://actor${path}`, {
		method: "GET",
		headers: fullHeaders,
	});

	return request;
}
