import invariant from "invariant";
import pRetry, { AbortError } from "p-retry";
import type { AnyActorDefinition } from "@/actor/definition";
import { PATH_CONNECT } from "@/common/actor-router-consts";
import type * as protocol from "@/common/client-protocol";
import {
	CURRENT_VERSION as CLIENT_PROTOCOL_CURRENT_VERSION,
	CLIENT_PROTOCOL_TO_CLIENT,
	CLIENT_PROTOCOL_TO_SERVER,
} from "@/common/client-protocol-versioned";
import {
	type ToClient as ToClientJson,
	ToClientSchema,
	type ToServer as ToServerJson,
	ToServerSchema,
} from "@/common/client-protocol-zod";
import type { JsonCompatValue } from "@/common/encoding";
import {
	type Encoding,
	inputDataToBuffer,
	jsonStringifyCompat,
} from "@/common/encoding";
import { assertUnreachable, stringifyError } from "@/common/utils";
import type { UniversalWebSocket } from "@/common/websocket-interface";
import type { EngineControlClient } from "@/engine-client/driver";
import {
	decodeCborCompat,
	deserializeWithEncoding,
	encodeCborCompat,
	serializeWithEncoding,
} from "@/serde";
import { bufferToArrayBuffer, promiseWithResolvers } from "@/utils";
import { getLogMessage } from "@/utils/env-vars";
import type {
	ActorDefinitionActions,
	ActorDefinitionEventSubscriptions,
	ActorDefinitionQueueSend,
	ActorGatewayOptions,
	ResolvedActorGatewayOptions,
} from "./actor-common";
import { resolveActorGatewayOptions } from "./actor-common";
import {
	type ActorResolutionState,
	checkForSchedulingError,
	getGatewayTarget,
	isDynamicActorQuery,
	isStaleResolvedActorError,
} from "./actor-query";
import { ACTOR_CONNS_SYMBOL, type ClientRaw } from "./client";
import * as errors from "./errors";
import { isRetryableLifecycleReconnectSignal } from "./lifecycle-errors";
import { logger } from "./log";
import {
	createQueueSender,
	type QueueSendNoWaitOptions,
	type QueueSendOptions,
	type QueueSendResult,
	type QueueSendWaitOptions,
} from "./queue";
import {
	type WebSocketMessage as ConnMessage,
	messageLength,
	parseWebSocketCloseReason,
} from "./utils";

interface CloseEventLike {
	code?: number;
	reason?: string;
	wasClean?: boolean;
}

/**
 * Connection status for an actor connection.
 *
 * - `"idle"`: Not connected, no auto-reconnect (initial state, after dispose, or disabled)
 * - `"connecting"`: Attempting to establish connection
 * - `"connected"`: Connection is active
 * - `"disconnected"`: Connection was lost, will auto-reconnect
 */
export type ActorConnStatus =
	| "idle"
	| "connecting"
	| "connected"
	| "disconnected";

interface ActionInFlight {
	name: string;
	resolve: (response: { id: bigint; output: unknown }) => void;
	reject: (error: Error) => void;
}

interface EventSubscriptions<Args extends Array<unknown>> {
	callback: (...args: Args) => void;
	once: boolean;
}

/**
 * A function that unsubscribes from an event.
 *
 * @typedef {Function} EventUnsubscribe
 */
export type EventUnsubscribe = () => void;

/**
 * A function that handles connection errors.
 *
 * @typedef {Function} ActorErrorCallback
 */
export type ActorErrorCallback = (error: errors.ActorError) => void;

/**
 * A callback for connection state changes.
 *
 * @typedef {Function} ConnectionStateCallback
 */
export type ConnectionStateCallback = () => void;

/**
 * A callback for connection status changes.
 *
 * @typedef {Function} StatusChangeCallback
 */
export type StatusChangeCallback = (status: ActorConnStatus) => void;

export interface SendHttpMessageOpts {
	ephemeral: boolean;
	signal?: AbortSignal;
}

export const CONNECT_SYMBOL = Symbol("connect");

/**
 * Provides underlying functions for {@link ActorConn}. See {@link ActorConn} for using type-safe remote procedure calls.
 *
 * @see {@link ActorConn}
 */
export class ActorConnRaw {
	#disposed = false;

	/* Will be aborted on dispose. */
	#abortController = new AbortController();

	#connStatus: ActorConnStatus = "idle";

	#actorId?: string;
	#connId?: string;

	#messageQueue: Array<{
		body:
			| {
					tag: "ActionRequest";
					val: { id: bigint; name: string; args: unknown };
			  }
			| {
					tag: "SubscriptionRequest";
					val: { eventName: string; subscribe: boolean };
			  };
	}> = [];
	#actionsInFlight = new Map<number, ActionInFlight>();

	#eventSubscriptions = new Map<string, Set<EventSubscriptions<any[]>>>();

	#errorHandlers = new Set<ActorErrorCallback>();
	#openHandlers = new Set<ConnectionStateCallback>();
	#openScheduled = false;
	#closeHandlers = new Set<ConnectionStateCallback>();
	#statusChangeHandlers = new Set<StatusChangeCallback>();

	#actionIdCounter = 0;
	#queueSender: ReturnType<typeof createQueueSender>;
	#readyPromise: ReturnType<typeof promiseWithResolvers<void>>;

	/**
	 * Interval that keeps the NodeJS process alive if this is the only thing running.
	 *
	 * See ttps://github.com/nodejs/node/issues/22088
	 */
	#keepNodeAliveInterval: NodeJS.Timeout;

	/** Promise used to indicate the socket has connected successfully. This will be rejected if the connection fails. */
	#onOpenPromise?: ReturnType<typeof promiseWithResolvers<undefined>>;

	#websocket?: UniversalWebSocket;

	#client: ClientRaw;
	#driver: EngineControlClient;
	#params: unknown;
	#getParams?: () => Promise<unknown>;
	#encoding: Encoding;
	#actorResolutionState: ActorResolutionState;
	#gatewayOptions: ResolvedActorGatewayOptions;

	// TODO: ws message queue

	/**
	 * Do not call this directly.
	 *
	 * Creates an instance of ActorConnRaw.
	 *
	 * @protected
	 */
	public constructor(
		client: ClientRaw,
		driver: EngineControlClient,
		params: unknown,
		getParams: (() => Promise<unknown>) | undefined,
		encoding: Encoding,
		actorResolutionState: ActorResolutionState,
		gatewayOptions: ActorGatewayOptions = {},
	) {
		this.#client = client;
		this.#driver = driver;
		this.#params = params;
		this.#getParams = getParams;
		this.#encoding = encoding;
		this.#actorResolutionState = actorResolutionState;
		this.#gatewayOptions = resolveActorGatewayOptions(gatewayOptions);
		if ("getForId" in actorResolutionState) {
			this.#actorId = actorResolutionState.getForId.actorId;
		}
		this.#readyPromise = promiseWithResolvers((reason) =>
			logger().warn({
				msg: "unhandled ready promise rejection",
				reason,
			}),
		);
		// Resolve the actor ID for each queue send so key-based connections do
		// not pin themselves to an earlier resolution.
		this.#queueSender = createQueueSender({
			encoding: this.#encoding,
			params: this.#params,
			customFetch: async (request: Request) => {
				return await this.#driver.sendRequest(
					getGatewayTarget(this.#actorResolutionState),
					request,
					this.#gatewayOptions,
				);
			},
		});

		this.#keepNodeAliveInterval = setInterval(() => {}, 60_000);
	}

	#clearResolvedActorIdentity() {
		this.#actorId = undefined;
		this.#connId = undefined;
	}

	/**
	 * If the query is dynamic (getForKey or getOrCreateForKey) and the error
	 * indicates the previously resolved actor is stale (not_found, starting,
	 * stopping, or destroyed),
	 * clear the cached actor ID and connection ID so the next operation
	 * re-resolves to a fresh actor. Returns true if the identity was
	 * invalidated.
	 */
	#invalidateActorIfStale(group: string, code: string): boolean {
		if (
			!isDynamicActorQuery(this.#actorResolutionState) ||
			!isStaleResolvedActorError(group, code)
		) {
			return false;
		}

		this.#clearResolvedActorIdentity();
		return true;
	}

	#shouldReconnectForStaleActor(group: string, code: string): boolean {
		return (
			isDynamicActorQuery(this.#actorResolutionState) &&
			isStaleResolvedActorError(group, code) &&
			this.#onOpenPromise !== undefined &&
			this.#connStatus !== "connected"
		);
	}

	send(
		name: string,
		body: unknown,
		options: QueueSendWaitOptions,
	): Promise<QueueSendResult>;
	send(
		name: string,
		body: unknown,
		options?: QueueSendNoWaitOptions,
	): Promise<void>;
	send(
		name: string,
		body: unknown,
		options?: QueueSendOptions,
	): Promise<QueueSendResult | void> {
		return this.#sendQueueMessage(name, body, options as any);
	}

	async #sendQueueMessage(
		name: string,
		body: unknown,
		options?: QueueSendOptions,
	): Promise<QueueSendResult | void> {
		return await this.#queueSender.send(name, body, options as any);
	}

	/**
	 * Call a raw action connection. See {@link ActorConn} for type-safe action calls.
	 *
	 * @see {@link ActorConn}
	 * @template Args - The type of arguments to pass to the action function.
	 * @template Response - The type of the response returned by the action function.
	 * @param {string} name - The name of the action function to call.
	 * @param {...Args} args - The arguments to pass to the action function.
	 * @returns {Promise<Response>} - A promise that resolves to the response of the action function.
	 */
	async action<
		Args extends Array<unknown> = unknown[],
		Response = unknown,
	>(opts: {
		name: string;
		args: Args;
		signal?: AbortSignal;
	}): Promise<Response> {
		if (
			typeof opts === "string" ||
			typeof opts !== "object" ||
			opts === null ||
			!("name" in opts)
		) {
			throw new Error(
				`Invalid action call: expected an options object { name, args }, got ${typeof opts}. Use conn.actionName(...args) for the shorthand API.`,
			);
		}
		logger().debug({ msg: "action", name: opts.name, args: opts.args });

		// If we have an active connection, use the websockactionId
		const actionId = this.#actionIdCounter;
		this.#actionIdCounter += 1;

		const { promise, resolve, reject } = promiseWithResolvers<{
			id: bigint;
			output: unknown;
		}>((reason) =>
			logger().warn({
				msg: "unhandled action promise rejection",
				reason,
			}),
		);
		this.#actionsInFlight.set(actionId, {
			name: opts.name,
			resolve,
			reject,
		});
		logger().debug({
			msg: "added action to in-flight map",
			actionId,
			actionName: opts.name,
			inFlightCount: this.#actionsInFlight.size,
		});

		this.#sendMessage({
			body: {
				tag: "ActionRequest",
				val: {
					id: BigInt(actionId),
					name: opts.name,
					args: opts.args,
				},
			},
		});

		// TODO: Throw error if disconnect is called

		const { id: responseId, output } = await promise;
		if (responseId !== BigInt(actionId))
			throw new Error(
				`Request ID ${actionId} does not match response ID ${responseId}`,
			);

		return output as Response;
	}

	/**
	 * Do not call this directly.
	 * Establishes a connection to the server using the specified endpoint & encoding & driver.
	 *
	 * @protected
	 */
	public [CONNECT_SYMBOL]() {
		this.#connectWithRetry();
	}

	#setConnStatus(status: ActorConnStatus) {
		const prevStatus = this.#connStatus;
		if (prevStatus === status) return;
		this.#connStatus = status;

		// Notify status change handlers
		for (const handler of [...this.#statusChangeHandlers]) {
			try {
				handler(status);
			} catch (err) {
				logger().error({
					msg: "error in status change handler",
					error: stringifyError(err),
				});
			}
		}

		// Notify open handlers
		if (status === "connected") {
			this.#readyPromise.resolve(undefined);
			for (const handler of [...this.#openHandlers]) {
				try {
					handler();
				} catch (err) {
					logger().error({
						msg: "error in open handler",
						error: stringifyError(err),
					});
				}
			}
		}

		// Notify close handlers (only if transitioning from Connected to Disconnected or Idle)
		if (
			(status === "disconnected" || status === "idle") &&
			prevStatus === "connected"
		) {
			for (const handler of [...this.#closeHandlers]) {
				try {
					handler();
				} catch (err) {
					logger().error({
						msg: "error in close handler",
						error: stringifyError(err),
					});
				}
			}
		}
	}

	#connectWithRetry() {
		this.#setConnStatus("connecting");

		// Attempt to reconnect indefinitely
		// This is intentionally not awaited - connection happens in background
		pRetry(this.#connectAndWait.bind(this), {
			forever: true,
			minTimeout: 250,
			maxTimeout: 30_000,

			onFailedAttempt: (error) => {
				logger().warn({
					msg: "failed to reconnect",
					attempt: error.attemptNumber,
					error: stringifyError(error),
				});
			},

			// Cancel retry if aborted
			signal: this.#abortController.signal,
		}).catch((err) => {
			if (
				err instanceof AbortError ||
				(err as Error).name === "AbortError" ||
				!this.#shouldRetryConnectionOpenError(err)
			) {
				logger().info({ msg: "connection retry aborted" });
			} else {
				logger().error({
					msg: "unexpected error in connection retry",
					error: stringifyError(err),
				});
			}
		});
	}

	async #connectAndWait() {
		try {
			// Create promise for open
			if (this.#onOpenPromise)
				throw new Error("#onOpenPromise already defined");
			this.#onOpenPromise = promiseWithResolvers((reason) =>
				logger().warn({
					msg: "unhandled open promise rejection",
					reason,
				}),
			);

			await this.#connectWebSocket();

			// Wait for result
			await this.#onOpenPromise.promise;
		} catch (error) {
			if (this.#shouldRetryConnectionOpenError(error)) {
				throw error;
			}

			const actorError =
				error instanceof errors.ActorError
					? error
					: new errors.ActorError(
							"client",
							"connection_open_failed",
							`Failed to open connection: ${stringifyError(error)}`,
							{ error: stringifyError(error) },
						);

			this.#clearQueuedMessages();
			this.#rejectPendingPromises(actorError, false);
			this.#dispatchActorError(actorError);
			this.#setConnStatus("idle");

			throw new AbortError(
				error instanceof Error
					? error
					: new Error(stringifyError(error)),
			);
		} finally {
			this.#onOpenPromise = undefined;
		}
	}

	#shouldRetryConnectionOpenError(error: unknown): boolean {
		if (error instanceof errors.ActorConnDisposed) {
			return false;
		}

		if (
			error instanceof errors.ActorError &&
			this.#shouldReconnectForStaleActor(error.group, error.code)
		) {
			return true;
		}

		if (
			error instanceof errors.ActorError &&
			error.group === "client" &&
			error.code === "get_params_failed"
		) {
			return true;
		}

		return isRetryableLifecycleReconnectSignal(error);
	}

	#clearQueuedMessages() {
		if (this.#messageQueue.length === 0) return;

		logger().debug({
			msg: "clearing queued connection messages",
			queueLength: this.#messageQueue.length,
		});
		this.#messageQueue = [];
	}

	async #resolveConnectionParams(): Promise<unknown> {
		if (!this.#getParams) {
			return this.#params;
		}

		try {
			return await this.#getParams();
		} catch (err) {
			const errorMessage = stringifyError(err);
			const error = new errors.ActorError(
				"client",
				"get_params_failed",
				`Failed to resolve connection params: ${errorMessage}`,
				{ error: errorMessage },
			);

			this.#clearQueuedMessages();
			this.#rejectPendingPromises(error, false);
			this.#dispatchActorError(error);

			throw error;
		}
	}

	async #connectWebSocket() {
		const params = await this.#resolveConnectionParams();
		const target = getGatewayTarget(this.#actorResolutionState);
		const ws = await this.#driver.openWebSocket(
			PATH_CONNECT,
			target,
			this.#encoding,
			params,
			this.#gatewayOptions,
		);
		invariant(ws, "websocket should have been created");
		logger().debug({
			msg: "opened websocket",
			connId: this.#connId,
			readyState: ws.readyState,
			messageQueueLength: this.#messageQueue.length,
		});
		this.#websocket = ws;
		ws.addEventListener("open", () => {
			logger().debug({
				msg: "client websocket open",
				connId: this.#connId,
			});
		});
		ws.addEventListener("message", async (ev: { data: ConnMessage }) => {
			try {
				await this.#handleOnMessage(ev.data);
			} catch (err) {
				logger().error({
					msg: "error in websocket message handler",
					error: stringifyError(err),
				});
			}
		});
		ws.addEventListener("close", async (ev: Event | CloseEventLike) => {
			try {
				await this.#handleOnClose(ev);
			} catch (err) {
				logger().error({
					msg: "error in websocket close handler",
					error: stringifyError(err),
				});
			}
		});
		ws.addEventListener("error", () => {
			try {
				this.#handleOnError();
			} catch (err) {
				logger().error({
					msg: "error in websocket error handler",
					error: stringifyError(err),
				});
			}
		});
	}

	/** Called by the onopen event from drivers. */
	#handleOnOpen() {
		// Connection was disposed before Init message arrived - close the websocket to avoid leak
		if (this.#disposed) {
			logger().debug({
				msg: "handleOnOpen called after dispose, closing websocket",
			});
			if (this.#websocket) {
				this.#websocket.close(1000, "Disposed");
				this.#websocket = undefined;
			}
			return;
		}

		if (this.#connStatus === "connected" || this.#openScheduled) {
			return;
		}
		this.#openScheduled = true;

		queueMicrotask(() => {
			this.#openScheduled = false;
			if (this.#disposed) {
				logger().debug({
					msg: "handleOnOpen scheduled after dispose, closing websocket",
				});
				if (this.#websocket) {
					this.#websocket.close(1000, "Disposed");
					this.#websocket = undefined;
				}
				return;
			}

			logger().debug({
				msg: "socket open",
				messageQueueLength: this.#messageQueue.length,
				connId: this.#connId,
			});

			// Update connection state (this also notifies handlers)
			this.#setConnStatus("connected");

			// Resolve open promise
			if (this.#onOpenPromise) {
				this.#onOpenPromise.resolve(undefined);
			} else {
				logger().warn({ msg: "#onOpenPromise is undefined" });
			}

			// Resubscribe to all active events
			for (const eventName of this.#eventSubscriptions.keys()) {
				this.#sendSubscription(eventName, true);
			}

			// Flush queue
			//
			// If the message fails to send, the message will be re-queued
			const queue = this.#messageQueue;
			this.#messageQueue = [];
			logger().debug({
				msg: "flushing message queue",
				queueLength: queue.length,
			});
			for (const msg of queue) {
				this.#sendMessage(msg);
			}
		});
	}

	/** Called by the onmessage event from drivers. */
	async #handleOnMessage(data: any) {
		logger().trace({
			msg: "received message",
			dataType: typeof data,
			isBlob: data instanceof Blob,
			isArrayBuffer: data instanceof ArrayBuffer,
		});

		const response = await this.#parseMessage(data as ConnMessage);
		logger().trace(
			getLogMessage()
				? {
						msg: "parsed message",
						message:
							jsonStringifyCompat(response).substring(0, 100) +
							"...",
					}
				: { msg: "parsed message" },
		);

		if (response.body.tag === "Init") {
			// Store connection info
			this.#actorId = response.body.val.actorId;
			this.#connId = response.body.val.connectionId;
			logger().trace({
				msg: "received init message",
				actorId: this.#actorId,
				connId: this.#connId,
			});
			this.#handleOnOpen();
		} else if (response.body.tag === "Error") {
			// Connection error
			const { group, code, message, metadata, actionId, actor } =
				response.body.val;

			if (actionId !== null) {
				const inFlight = this.#takeActionInFlight(Number(actionId));
				this.#invalidateActorIfStale(group, code);

				logger().warn({
					msg: "action error",
					actionId: actionId,
					actionName: inFlight?.name,
					group,
					code,
					message,
					metadata,
					actorId: actor?.actorId,
					generation: actor?.generation,
					actorKey: actor?.key,
				});

				inFlight.reject(
					new errors.ActorError(group, code, message, {
						metadata,
						actor,
					}),
				);
			} else {
				logger().warn({
					msg: "connection error",
					group,
					code,
					message,
					metadata,
					actorId: actor?.actorId,
					generation: actor?.generation,
					actorKey: actor?.key,
				});

				if (this.#shouldReconnectForStaleActor(group, code)) {
					this.#clearResolvedActorIdentity();
					this.#onOpenPromise?.reject(
						new errors.ActorError(group, code, message, {
							metadata,
							actor,
						}),
					);
					return;
				}

				// Check if this is an actor scheduling error and try to get more details
				let errorToThrow = new errors.ActorError(group, code, message, {
					metadata,
					actor,
				});
				if (errors.isSchedulingError(group, code) && this.#actorId) {
					const schedulingError = await checkForSchedulingError(
						group,
						code,
						this.#actorId,
						this.#actorResolutionState,
						this.#driver,
					);
					if (schedulingError) {
						errorToThrow = schedulingError;
					}
				}

				// If we have an onOpenPromise, reject it with the error
				if (this.#onOpenPromise) {
					this.#onOpenPromise.reject(errorToThrow);
				}

				this.#invalidateActorIfStale(group, code);

				// Reject any in-flight requests
				for (const [id, inFlight] of this.#actionsInFlight.entries()) {
					inFlight.reject(errorToThrow);
					this.#actionsInFlight.delete(id);
				}

				this.#dispatchActorError(errorToThrow);
			}
		} else if (response.body.tag === "ActionResponse") {
			// Action response OK
			const { id: actionId } = response.body.val;
			logger().debug({
				msg: "received action response",
				actionId: Number(actionId),
				inFlightCount: this.#actionsInFlight.size,
				inFlightIds: Array.from(this.#actionsInFlight.keys()),
			});

			const inFlight = this.#takeActionInFlight(Number(actionId));
			logger().trace({
				msg: "resolving action promise",
				actionId,
				actionName: inFlight?.name,
			});
			inFlight.resolve(response.body.val);
		} else if (response.body.tag === "Event") {
			logger().trace({
				msg: "received event",
				name: response.body.val.name,
			});
			this.#dispatchEvent(response.body.val);
		} else {
			assertUnreachable(response.body);
		}
	}

	/** Called by the onclose event from drivers. */
	async #handleOnClose(event: Event | CloseEventLike) {
		// We can't use `event instanceof CloseEvent` because it's not defined in NodeJS
		const closeEvent = event as CloseEventLike;
		const wasClean = closeEvent.wasClean;
		const wasConnected = this.#connStatus === "connected";

		logger().info({
			msg: "socket closed",
			code: closeEvent.code,
			reason: closeEvent.reason,
			wasClean,
			disposed: this.#disposed,
			connId: this.#connId,
		});

		this.#websocket = undefined;

		if (this.#disposed) {
			// Use ActorConnDisposed error and prevent unhandled rejection
			this.#rejectPendingPromises(new errors.ActorConnDisposed(), true);
		} else {
			this.#setConnStatus("disconnected");

			// Build error from close event
			let error: Error;
			const reason = closeEvent.reason || "";
			const parsed = parseWebSocketCloseReason(reason);

			if (parsed) {
				const { group, code, rayId } = parsed;

				if (this.#shouldReconnectForStaleActor(group, code)) {
					this.#clearResolvedActorIdentity();
					this.#onOpenPromise?.reject(
						new errors.ActorError(
							group,
							code,
							`Connection closed: ${reason}`,
							{ rayId },
						),
					);
					return;
				}

				// Check if this is a scheduling error
				if (errors.isSchedulingError(group, code) && this.#actorId) {
					const schedulingError = await checkForSchedulingError(
						group,
						code,
						this.#actorId,
						this.#actorResolutionState,
						this.#driver,
						rayId,
					);
					if (schedulingError) {
						error = schedulingError;
					} else {
						error = new errors.ActorError(
							group,
							code,
							`Connection closed: ${reason}`,
							{ rayId },
						);
					}
				} else {
					error = new errors.ActorError(
						group,
						code,
						`Connection closed: ${reason}`,
						{ rayId },
					);
				}

				this.#invalidateActorIfStale(group, code);
			} else {
				// Default error for non-structured close reasons
				error = new Error(
					`${wasClean ? "Connection closed" : "Connection lost"} (code: ${closeEvent.code}, reason: ${reason})`,
				);
			}

			this.#rejectPendingPromises(error, false);

			// Dispatch to error handler if it's an ActorError
			if (error instanceof errors.ActorError) {
				this.#dispatchActorError(error);
			}

			// Automatically reconnect if we were connected
			if (wasConnected) {
				logger().debug({
					msg: "triggering reconnect",
					connId: this.#connId,
				});
				this.#connectWithRetry();
			}
		}
	}

	#rejectPendingPromises(error: Error, suppressUnhandled: boolean) {
		if (this.#onOpenPromise) {
			if (suppressUnhandled) {
				this.#onOpenPromise.promise.catch(() => {});
			}
			this.#onOpenPromise.reject(error);
		}

		for (const actionInfo of this.#actionsInFlight.values()) {
			actionInfo.reject(error);
		}
		this.#actionsInFlight.clear();
	}

	/** Called by the onerror event from drivers. */
	#handleOnError() {
		if (this.#disposed) return;

		// More detailed information will be logged in onclose
		logger().warn("socket error");
	}

	#takeActionInFlight(id: number): ActionInFlight {
		const inFlight = this.#actionsInFlight.get(id);
		if (!inFlight) {
			logger().error({
				msg: "action not found in in-flight map",
				lookupId: id,
				inFlightCount: this.#actionsInFlight.size,
				inFlightIds: Array.from(this.#actionsInFlight.keys()),
				inFlightActions: Array.from(
					this.#actionsInFlight.entries(),
				).map(([id, action]) => ({
					id,
					name: action.name,
				})),
			});
			throw errors.internalClientError(`No in flight response for ${id}`);
		}
		this.#actionsInFlight.delete(id);
		logger().debug({
			msg: "removed action from in-flight map",
			actionId: id,
			actionName: inFlight.name,
			inFlightCount: this.#actionsInFlight.size,
		});
		return inFlight;
	}

	#dispatchEvent(event: { name: string; args: unknown }) {
		const { name, args } = event;

		const listeners = this.#eventSubscriptions.get(name);
		if (!listeners) return;

		// Create a new array to avoid issues with listeners being removed during iteration
		for (const listener of [...listeners]) {
			listener.callback(...(args as unknown[]));

			// Remove if this was a one-time listener
			if (listener.once) {
				listeners.delete(listener);
			}
		}

		// Clean up empty listener sets
		if (listeners.size === 0) {
			this.#eventSubscriptions.delete(name);
		}
	}

	#dispatchActorError(error: errors.ActorError) {
		// Call all registered error handlers
		for (const handler of [...this.#errorHandlers]) {
			try {
				handler(error);
			} catch (err) {
				logger().error({
					msg: "error in connection error handler",
					error: stringifyError(err),
				});
			}
		}
	}

	#addEventSubscription<Args extends Array<unknown>>(
		eventName: string,
		callback: (...args: Args) => void,
		once: boolean,
	): EventUnsubscribe {
		const listener: EventSubscriptions<Args> = {
			callback,
			once,
		};

		let subscriptionSet = this.#eventSubscriptions.get(eventName);
		if (subscriptionSet === undefined) {
			subscriptionSet = new Set();
			this.#eventSubscriptions.set(eventName, subscriptionSet);
			this.#sendSubscription(eventName, true);
		}
		subscriptionSet.add(listener);

		// Return unsubscribe function
		return () => {
			const listeners = this.#eventSubscriptions.get(eventName);
			if (listeners) {
				listeners.delete(listener);
				if (listeners.size === 0) {
					this.#eventSubscriptions.delete(eventName);
					this.#sendSubscription(eventName, false);
				}
			}
		};
	}

	/**
	 * Subscribes to an event that will happen repeatedly.
	 *
	 * @template Args - The type of arguments the event callback will receive.
	 * @param {string} eventName - The name of the event to subscribe to.
	 * @param {(...args: Args) => void} callback - The callback function to execute when the event is triggered.
	 * @returns {EventUnsubscribe} - A function to unsubscribe from the event.
	 * @see {@link https://rivet.dev/docs/events|Events Documentation}
	 */
	on<Args extends Array<unknown> = unknown[]>(
		eventName: string,
		callback: (...args: Args) => void,
	): EventUnsubscribe {
		return this.#addEventSubscription<Args>(eventName, callback, false);
	}

	/**
	 * Subscribes to an event that will be triggered only once.
	 *
	 * @template Args - The type of arguments the event callback will receive.
	 * @param {string} eventName - The name of the event to subscribe to.
	 * @param {(...args: Args) => void} callback - The callback function to execute when the event is triggered.
	 * @returns {EventUnsubscribe} - A function to unsubscribe from the event.
	 * @see {@link https://rivet.dev/docs/events|Events Documentation}
	 */
	once<Args extends Array<unknown> = unknown[]>(
		eventName: string,
		callback: (...args: Args) => void,
	): EventUnsubscribe {
		return this.#addEventSubscription<Args>(eventName, callback, true);
	}

	/**
	 * Subscribes to connection errors.
	 *
	 * @param {ActorErrorCallback} callback - The callback function to execute when a connection error occurs.
	 * @returns {() => void} - A function to unsubscribe from the error handler.
	 */
	onError(callback: ActorErrorCallback): () => void {
		this.#errorHandlers.add(callback);

		// Return unsubscribe function
		return () => {
			this.#errorHandlers.delete(callback);
		};
	}

	/**
	 * Returns the current connection status.
	 *
	 * @returns {ActorConnStatus} - The current connection status.
	 */
	get connStatus(): ActorConnStatus {
		return this.#connStatus;
	}

	/**
	 * Returns whether the connection is currently open.
	 *
	 * @deprecated Use `connStatus` instead.
	 * @returns {boolean} - True if the connection is open, false otherwise.
	 */
	get isConnected(): boolean {
		return this.#connStatus === "connected";
	}

	/**
	 * Resolves when this connection first reaches the connected state.
	 */
	get ready(): Promise<void> {
		return this.#readyPromise.promise;
	}

	/**
	 * Subscribes to connection open events.
	 *
	 * This is called when the WebSocket connection is established and the Init message is received.
	 *
	 * @param {ConnectionStateCallback} callback - The callback function to execute when the connection opens.
	 * @returns {() => void} - A function to unsubscribe from the open handler.
	 */
	onOpen(callback: ConnectionStateCallback): () => void {
		this.#openHandlers.add(callback);

		if (this.#connStatus === "connected") {
			queueMicrotask(() => {
				if (!this.#openHandlers.has(callback)) {
					return;
				}

				try {
					callback();
				} catch (err) {
					logger().error({
						msg: "error in open handler",
						error: stringifyError(err),
					});
				}
			});
		}

		// Return unsubscribe function
		return () => {
			this.#openHandlers.delete(callback);
		};
	}

	/**
	 * Subscribes to connection close events.
	 *
	 * This is called when the WebSocket connection is closed. The connection will automatically
	 * attempt to reconnect unless disposed.
	 *
	 * @param {ConnectionStateCallback} callback - The callback function to execute when the connection closes.
	 * @returns {() => void} - A function to unsubscribe from the close handler.
	 */
	onClose(callback: ConnectionStateCallback): () => void {
		this.#closeHandlers.add(callback);

		// Return unsubscribe function
		return () => {
			this.#closeHandlers.delete(callback);
		};
	}

	/**
	 * Subscribes to connection status changes.
	 *
	 * This is called whenever the connection status changes between Disconnected, Connecting, and Connected.
	 *
	 * @param {StatusChangeCallback} callback - The callback function to execute when the status changes.
	 * @returns {() => void} - A function to unsubscribe from the status change handler.
	 */
	onStatusChange(callback: StatusChangeCallback): () => void {
		this.#statusChangeHandlers.add(callback);

		// Return unsubscribe function
		return () => {
			this.#statusChangeHandlers.delete(callback);
		};
	}

	#sendMessage(
		message: {
			body:
				| {
						tag: "ActionRequest";
						val: { id: bigint; name: string; args: unknown };
				  }
				| {
						tag: "SubscriptionRequest";
						val: { eventName: string; subscribe: boolean };
				  };
		},
		opts?: SendHttpMessageOpts,
	) {
		if (this.#disposed) {
			if (opts?.ephemeral) {
				return;
			} else {
				throw new errors.ActorConnDisposed();
			}
		}

		let queueMessage = false;
		if (this.#websocket) {
			const readyState = this.#websocket.readyState;
			logger().debug({
				msg: "websocket send attempt",
				readyState,
				readyStateString:
					readyState === 0
						? "CONNECTING"
						: readyState === 1
							? "OPEN"
							: readyState === 2
								? "CLOSING"
								: "CLOSED",
				connId: this.#connId,
				messageType: (message.body as any).tag,
				actionName: (message.body as any).val?.name,
			});
			if (this.#connStatus !== "connected") {
				logger().debug({
					msg: "websocket init pending, queueing message",
					connStatus: this.#connStatus,
					messageType: (message.body as any).tag,
				});
				queueMessage = true;
			} else if (readyState === 1) {
				try {
					const messageSerialized = serializeWithEncoding(
						this.#encoding,
						message,
						CLIENT_PROTOCOL_TO_SERVER,
						CLIENT_PROTOCOL_CURRENT_VERSION,
						ToServerSchema,
						// JSON: args is the raw value
						(msg): ToServerJson => msg as ToServerJson,
						// BARE: args needs to be CBOR-encoded to ArrayBuffer
						(msg): protocol.ToServer => {
							if (msg.body.tag === "ActionRequest") {
								return {
									body: {
										tag: "ActionRequest",
										val: {
											id: msg.body.val.id,
											name: msg.body.val.name,
											args: bufferToArrayBuffer(
												encodeCborCompat(
													msg.body.val
														.args as JsonCompatValue,
												),
											),
										},
									},
								};
							} else {
								return msg as protocol.ToServer;
							}
						},
					);
					this.#websocket.send(messageSerialized);
					const serializedLength = messageLength(messageSerialized);
					logger().trace({
						msg: "sent websocket message",
						len: serializedLength,
					});
				} catch (error) {
					logger().warn({
						msg: "failed to send message, added to queue",
						error,
						connId: this.#connId,
					});

					// Assuming the socket is disconnected and will be reconnected soon
					queueMessage = true;
				}
			} else {
				logger().debug({
					msg: "websocket not open, queueing message",
					readyState,
				});
				queueMessage = true;
			}
		} else {
			// No websocket connected yet
			logger().debug({ msg: "no websocket, queueing message" });
			queueMessage = true;
		}

		if (!opts?.ephemeral && queueMessage) {
			this.#messageQueue.push(message);
			logger().debug({
				msg: "queued connection message",
				queueLength: this.#messageQueue.length,
				connId: this.#connId,
				messageType: (message.body as any).tag,
				actionName: (message.body as any).val?.name,
			});
		}
	}

	async #parseMessage(data: ConnMessage): Promise<{
		body:
			| { tag: "Init"; val: { actorId: string; connectionId: string } }
			| {
					tag: "Error";
					val: {
						group: string;
						code: string;
						message: string;
						metadata: unknown;
						actionId: bigint | null;
						actor?: errors.ActorSpecifier;
					};
			  }
			| { tag: "ActionResponse"; val: { id: bigint; output: unknown } }
			| { tag: "Event"; val: { name: string; args: unknown } };
	}> {
		invariant(this.#websocket, "websocket must be defined");

		const buffer = await inputDataToBuffer(data);

		return deserializeWithEncoding(
			this.#encoding,
			buffer,
			CLIENT_PROTOCOL_TO_CLIENT,
			ToClientSchema,
			// JSON/CBOR: normalize actor generation to the public number shape.
			(msg): any => {
				if (msg.body.tag !== "Error" || !msg.body.val.actor) {
					return msg as ToClientJson;
				}
				return {
					body: {
						tag: "Error",
						val: {
							...msg.body.val,
							actor: {
								...msg.body.val.actor,
								generation: Number(
									msg.body.val.actor.generation,
								),
							},
						},
					},
				};
			},
			// BARE: need to decode ArrayBuffer fields back to unknown
			(msg): any => {
				if (msg.body.tag === "Error") {
					return {
						body: {
							tag: "Error",
							val: {
								group: msg.body.val.group,
								code: msg.body.val.code,
								message: msg.body.val.message,
								metadata: msg.body.val.metadata
									? decodeCborCompat(
											new Uint8Array(
												msg.body.val.metadata,
											),
										)
									: null,
								actionId: msg.body.val.actionId,
								actor: msg.body.val.actor
									? {
											actorId: msg.body.val.actor.actorId,
											generation: Number(
												msg.body.val.actor.generation,
											),
											key:
												msg.body.val.actor.key ??
												undefined,
										}
									: undefined,
							},
						},
					};
				} else if (msg.body.tag === "ActionResponse") {
					return {
						body: {
							tag: "ActionResponse",
							val: {
								id: msg.body.val.id,
								output: decodeCborCompat(
									new Uint8Array(msg.body.val.output),
								),
							},
						},
					};
				} else if (msg.body.tag === "Event") {
					return {
						body: {
							tag: "Event",
							val: {
								name: msg.body.val.name,
								args: decodeCborCompat(
									new Uint8Array(msg.body.val.args),
								),
							},
						},
					};
				} else {
					// Init has no ArrayBuffer fields
					return msg;
				}
			},
		);
	}

	/**
	 * Get the actor ID (for testing purposes).
	 * @internal
	 */
	get actorId(): string | undefined {
		return this.#actorId;
	}

	/**
	 * Get the connection ID (for testing purposes).
	 * @internal
	 */
	get connId(): string | undefined {
		return this.#connId;
	}

	/**
	 * Get the connection ID (for testing purposes).
	 * @internal
	 * @deprecated Use `connId` instead.
	 */
	get connectionId(): string | undefined {
		return this.#connId;
	}

	/**
	 * Disconnects from the actor.
	 *
	 * @returns {Promise<void>} A promise that resolves when the socket is gracefully closed.
	 */
	async dispose(): Promise<void> {
		// Internally, this "disposes" the connection

		if (this.#disposed) {
			logger().warn({ msg: "connection already disconnected" });
			return;
		}
		this.#disposed = true;

		logger().debug({ msg: "disposing actor conn" });

		// Set status to Idle (intentionally closed, no auto-reconnect)
		this.#setConnStatus("idle");

		// Clear interval so NodeJS process can exit
		clearInterval(this.#keepNodeAliveInterval);

		// Abort retry loop
		this.#abortController.abort();

		// Remove from registry
		this.#client[ACTOR_CONNS_SYMBOL].delete(this);

		// Close websocket (#handleOnClose will reject pending promises)
		if (this.#websocket) {
			const ws = this.#websocket;
			if (
				ws.readyState !== 2 /* CLOSING */ &&
				ws.readyState !== 3 /* CLOSED */
			) {
				const { promise, resolve } = promiseWithResolvers((reason) =>
					logger().warn({
						msg: "unhandled websocket close promise rejection",
						reason,
					}),
				);
				ws.addEventListener("close", () => resolve(undefined));
				ws.close(1000, "Disposed");
				await promise;
			}
		} else {
			this.#rejectPendingPromises(new errors.ActorConnDisposed(), true);
		}
		this.#websocket = undefined;
	}

	#sendSubscription(eventName: string, subscribe: boolean) {
		this.#sendMessage(
			{
				body: {
					tag: "SubscriptionRequest",
					val: {
						eventName,
						subscribe,
					},
				},
			},
			{ ephemeral: true },
		);
	}
}

/**
 * Connection to a actor. Allows calling actor's remote procedure calls with inferred types. See {@link ActorConnRaw} for underlying methods.
 *
 * @example
 * ```
 * const room = client.connect<ChatRoom>(...etc...);
 * // This calls the action named `sendMessage` on the `ChatRoom` actor.
 * await room.sendMessage('Hello, world!');
 * ```
 *
 * Private methods (e.g. those starting with `_`) are automatically excluded.
 *
 * @template AD The actor class that this connection is for.
 * @see {@link ActorConnRaw}
 */
export type ActorConn<AD extends AnyActorDefinition> = Omit<
	ActorConnRaw,
	"send" | "on" | "once"
> &
	ActorDefinitionQueueSend<AD> &
	ActorDefinitionEventSubscriptions<AD> &
	ActorDefinitionActions<AD>;
