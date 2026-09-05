import type { AnyActorDefinition } from "@/actor/definition";
import type { ActorSpecifier } from "@/actor/errors";
import {
	HEADER_CONN_PARAMS,
	HEADER_ENCODING,
	HEADER_RIVETKIT_RAY_ID,
	HEADER_TRACEPARENT,
	HEADER_TRACESTATE,
} from "@/common/actor-router-consts";
import { isRequestLike } from "@/common/fetch-like";
import type * as protocol from "@/common/client-protocol";
import {
	CURRENT_VERSION as CLIENT_PROTOCOL_CURRENT_VERSION,
	HTTP_ACTION_REQUEST_VERSIONED,
	HTTP_ACTION_RESPONSE_VERSIONED,
	HTTP_RESPONSE_ERROR_VERSIONED,
} from "@/common/client-protocol-versioned";
import {
	type HttpActionRequest as HttpActionRequestJson,
	HttpActionRequestSchema,
	type HttpActionResponse as HttpActionResponseJson,
	HttpActionResponseSchema,
	type HttpResponseError as HttpResponseErrorJson,
	HttpResponseErrorSchema,
} from "@/common/client-protocol-zod";
import { AsyncMutex } from "@/common/database/shared";
import type { Encoding, JsonCompatValue } from "@/common/encoding";
import { deconstructError } from "@/common/utils";
import type { EngineControlClient } from "@/engine-client/driver";
import type { CurrentActorInvocation } from "@/registry/runtime";
import {
	decodeCborCompat,
	deserializeWithEncoding,
	encodeCborCompat,
} from "@/serde";
import { bufferToArrayBuffer } from "@/utils";
import type {
	ActorActionOptions,
	ActorConnectOptions,
	ActorDefinitionActions,
	ActorDefinitionQueueSend,
	ActorFetchInit,
	ActorGatewayOptions,
	ActorWebSocketOptions,
} from "./actor-common";
import { resolveActorGatewayOptions } from "./actor-common";
import { type ActorConn, ActorConnRaw } from "./actor-conn";
import {
	type ActorResolutionState,
	checkForSchedulingError,
	getActorNameFromQuery,
	getGatewayTarget,
	isDynamicActorQuery,
	isStaleResolvedActorError,
} from "./actor-query";
import { type ClientRaw, CREATE_ACTOR_CONN_PROXY } from "./client";
import { ActorError, isSchedulingError } from "./errors";
import { retryOnLifecycleBoundary } from "./lifecycle-errors";
import { logger } from "./log";
import { readActiveTraceHeaders } from "./otel-context";
import {
	createQueueSender,
	type QueueSendNoWaitOptions,
	type QueueSendOptions,
	type QueueSendResult,
	type QueueSendWaitOptions,
} from "./queue";
import { prepareRetryableInit, rawHttpFetch, rawWebSocket } from "./raw-utils";
import { resolveGatewayTarget } from "./resolve-gateway-target";
import { sendHttpRequest } from "./utils";

/**
 * Provides underlying functions for stateless {@link ActorHandle} for action calls.
 * Similar to ActorConnRaw but doesn't maintain a connection.
 *
 * @see {@link ActorHandle}
 */
export class ActorHandleRaw {
	#client: ClientRaw;
	#driver: EngineControlClient;
	#encoding: Encoding;
	#actorResolutionState: ActorResolutionState;
	#gatewayOptions: ActorGatewayOptions;
	#params: unknown;
	#getParams?: () => Promise<unknown>;
	#resolvedActorId?: string;
	#resolvingActorId?: Promise<string>;
	#queueSendMutex = new AsyncMutex();
	#currentActorInvocation?: CurrentActorInvocation;

	/**
	 * Do not call this directly.
	 *
	 * Creates an instance of ActorHandleRaw.
	 *
	 * @protected
	 */
	public constructor(
		client: any,
		driver: EngineControlClient,
		params: unknown,
		getParams: (() => Promise<unknown>) | undefined,
		encoding: Encoding,
		actorResolutionState: ActorResolutionState,
		gatewayOptions: ActorGatewayOptions = {},
		currentActorInvocation?: CurrentActorInvocation,
	) {
		this.#client = client;
		this.#driver = driver;
		this.#encoding = encoding;
		this.#actorResolutionState = actorResolutionState;
		this.#gatewayOptions = gatewayOptions;
		this.#params = params;
		this.#getParams = getParams;
		this.#currentActorInvocation = currentActorInvocation;
	}

	async #resolveConnectionParams(): Promise<unknown> {
		if (this.#getParams) {
			return await this.#getParams();
		}

		return this.#params;
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
		return await this.#queueSendMutex.run(async () => {
			const maxAttempts = this.#getDynamicQueryMaxAttempts();
			// Dynamic (get/getOrCreate) handles resolve at the gateway per
			// request so cross-datacenter queries forward to the reserved dc,
			// rather than pre-resolving via the non-forwarding PUT /actors path.
			let useQueryTarget = isDynamicActorQuery(
				this.#actorResolutionState,
			);

			for (let attempt = 0; attempt < maxAttempts; attempt++) {
				let actorId: string | undefined;
				try {
					const gatewayOptions = resolveActorGatewayOptions(
						this.#gatewayOptions,
					);
					const target = await this.#resolveGatewayRequestTarget(
						useQueryTarget,
						gatewayOptions,
					);
					actorId =
						"directId" in target ? target.directId : undefined;

					return await createQueueSender({
						encoding: this.#encoding,
						params: this.#params,
						customFetch: async (request: Request) => {
							return await this.#driver.sendRequest(
								target,
								request,
								gatewayOptions,
							);
						},
					}).send(name, body, options as any);
				} catch (err) {
					const { group, code, message, metadata, rayId, actor } =
						deconstructError(err, true);

					if (
						this.#shouldRetryQueueDispatchOverload(
							group,
							code,
							metadata,
							attempt,
							maxAttempts,
						)
					) {
						await this.#waitForRetryWindow();
						continue;
					}

					if (
						await this.#shouldRetrySchedulingError(
							group,
							code,
							actorId,
							attempt,
							maxAttempts,
							rayId,
						)
					) {
						useQueryTarget = true;
						await this.#waitForRetryWindow();
						continue;
					}

					if (
						this.#shouldRetryDynamicLifecycleError(
							group,
							code,
							attempt,
							maxAttempts,
						)
					) {
						this.#clearResolvedActorId();
						useQueryTarget = true;
						await this.#waitForRetryWindow();
						continue;
					}

					const invalidated = this.#invalidateResolvedActorId(
						group,
						code,
					);
					if (invalidated && attempt < maxAttempts - 1) {
						const waitForReady =
							code === "starting" ||
							code === "stopping" ||
							code.startsWith("destroyed_");
						useQueryTarget = useQueryTarget || waitForReady;
						if (waitForReady) {
							await this.#waitForRetryWindow();
						}
						continue;
					}

					throw new ActorError(group, code, message, {
						metadata,
						rayId,
						actor,
					});
				}
			}

			throw new Error("unreachable queue retry state");
		});
	}

	/**
	 * Call a raw action. This method sends an HTTP request to invoke the named action.
	 *
	 * @see {@link ActorHandle}
	 * @template Args - The type of arguments to pass to the action function.
	 * @template Response - The type of the response returned by the action function.
	 */
	async action<Args extends Array<unknown> = unknown[], Response = unknown>(
		opts: {
			name: string;
			args: Args;
		} & ActorActionOptions,
	): Promise<Response> {
		if (
			typeof opts === "string" ||
			typeof opts !== "object" ||
			opts === null ||
			!("name" in opts)
		) {
			throw new Error(
				`Invalid action call: expected an options object { name, args }, got ${typeof opts}. Use handle.actionName(...args) for the shorthand API.`,
			);
		}
		const run = async () => (await this.#sendActionNow(opts)) as Response;
		if (opts.name === "destroy") {
			return await run();
		}

		return await retryOnLifecycleBoundary(run, { signal: opts.signal });
	}

	async #sendActionNow(
		opts: {
			name: string;
			args: unknown[];
		} & ActorActionOptions,
	): Promise<unknown> {
		const maxAttempts = this.#getDynamicQueryMaxAttempts();
		let useQueryTarget = isDynamicActorQuery(this.#actorResolutionState);
		const gatewayOptions = resolveActorGatewayOptions(
			this.#gatewayOptions,
			opts,
		);

		for (let attempt = 0; attempt < maxAttempts; attempt++) {
			let actorId: string | undefined;
			try {
				const target = await this.#resolveGatewayRequestTarget(
					useQueryTarget,
					gatewayOptions,
				);
				actorId = "directId" in target ? target.directId : undefined;

				logger().debug(
					actorId
						? { msg: "using direct actor gateway target", actorId }
						: {
								msg: "using query gateway target for action",
								query: this.#actorResolutionState,
							},
				);

				logger().debug({
					msg: "handling action",
					name: opts.name,
					encoding: this.#encoding,
				});
				const invocation = this.#currentActorInvocation?.();
				const headers: Record<string, string> = {
					[HEADER_ENCODING]: this.#encoding,
				};
				if (this.#params !== undefined) {
					headers[HEADER_CONN_PARAMS] = JSON.stringify(this.#params);
				}
				if (invocation) {
					headers[HEADER_RIVETKIT_RAY_ID] = invocation.rayId;
				}
				// An application span active in this JavaScript context wins,
				// then the calling actor's own Core invocation span.
				const traceHeaders =
					readActiveTraceHeaders() ?? invocation?.span;
				if (traceHeaders) {
					headers[HEADER_TRACEPARENT] = traceHeaders.traceparent;
					if (traceHeaders.tracestate) {
						headers[HEADER_TRACESTATE] = traceHeaders.tracestate;
					}
				}
				const output = await sendHttpRequest<
					protocol.HttpActionRequest,
					protocol.HttpActionResponse,
					HttpActionRequestJson,
					HttpActionResponseJson,
					unknown[],
					Response
				>({
					url: `http://actor/action/${encodeURIComponent(opts.name)}`,
					method: "POST",
					headers,
					body: opts.args,
					encoding: this.#encoding,
					customFetch: async (request) =>
						await this.#driver.sendRequest(
							target,
							request,
							gatewayOptions,
						),
					signal: opts?.signal,
					requestVersion: CLIENT_PROTOCOL_CURRENT_VERSION,
					requestVersionedDataHandler: HTTP_ACTION_REQUEST_VERSIONED,
					responseVersion: CLIENT_PROTOCOL_CURRENT_VERSION,
					responseVersionedDataHandler:
						HTTP_ACTION_RESPONSE_VERSIONED,
					requestZodSchema: HttpActionRequestSchema,
					responseZodSchema: HttpActionResponseSchema,
					requestToJson: (args): HttpActionRequestJson => ({
						args,
					}),
					requestToBare: (args): protocol.HttpActionRequest => ({
						args: bufferToArrayBuffer(
							encodeCborCompat(args as JsonCompatValue),
						),
					}),
					responseFromJson: (json): Response =>
						json.output as Response,
					responseFromBare: (bare): Response =>
						decodeCborCompat(new Uint8Array(bare.output)),
				});
				if (opts.name === "destroy" && actorId) {
					await this.#waitForDestroyActionToSettle(actorId);
				}
				return output;
			} catch (err) {
				const { group, code, message, metadata, rayId, actor } =
					deconstructError(err, true);

				if (
					await this.#shouldRetrySchedulingError(
						group,
						code,
						actorId,
						attempt,
						maxAttempts,
						rayId,
					)
				) {
					useQueryTarget = true;
					await this.#waitForRetryWindow();
					continue;
				}

				if (
					opts.name !== "destroy" &&
					this.#shouldRetryDynamicLifecycleError(
						group,
						code,
						attempt,
						maxAttempts,
					)
				) {
					this.#clearResolvedActorId();
					useQueryTarget = true;
					await this.#waitForRetryWindow();
					continue;
				}

				if (
					group === "actor" &&
					code === "destroyed_while_waiting_for_ready" &&
					"getForId" in this.#actorResolutionState
				) {
					throw new ActorError(
						"actor",
						"not_found",
						"The actor does not exist or was destroyed.",
						{ metadata, rayId, actor },
					);
				}

				const invalidated = this.#invalidateResolvedActorId(
					group,
					code,
				);
				if (invalidated && attempt < maxAttempts - 1) {
					if (
						group === "actor" &&
						(code === "starting" || code === "stopping")
					) {
						useQueryTarget = true;
						await new Promise((resolve) =>
							setTimeout(resolve, 100),
						);
					}
					continue;
				}

				throw new ActorError(group, code, message, {
					metadata,
					rayId,
					actor,
				});
			}
		}

		throw new Error("unreachable action retry state");
	}

	async #waitForDestroyActionToSettle(actorId: string): Promise<void> {
		const name = getActorNameFromQuery(this.#actorResolutionState);
		const deadline = Date.now() + 1_000;
		while (Date.now() < deadline) {
			const actor = await this.#driver.getForId({ name, actorId });
			if (!actor) {
				return;
			}
			await new Promise((resolve) => setTimeout(resolve, 25));
		}
	}

	async #waitForRetryWindow(): Promise<void> {
		await new Promise((resolve) => setTimeout(resolve, 100));
	}

	#getDynamicQueryMaxAttempts(): number {
		if (!isDynamicActorQuery(this.#actorResolutionState)) {
			return 1;
		}

		return "getOrCreateForKey" in this.#actorResolutionState ? 60 : 24;
	}

	#shouldRetryDynamicLifecycleError(
		group: string,
		code: string,
		attempt: number,
		maxAttempts: number,
	): boolean {
		if (
			!isDynamicActorQuery(this.#actorResolutionState) ||
			attempt >= maxAttempts - 1 ||
			group !== "actor"
		) {
			return false;
		}

		return (
			code === "not_found" ||
			code === "starting" ||
			code === "stopping" ||
			code === "not_configured" ||
			code === "dropped_reply" ||
			code === "destroying" ||
			code.startsWith("destroyed_")
		);
	}

	#shouldRetryQueueDispatchOverload(
		group: string,
		code: string,
		metadata: unknown,
		attempt: number,
		maxAttempts: number,
	): boolean {
		if (
			!isDynamicActorQuery(this.#actorResolutionState) ||
			attempt >= maxAttempts - 1 ||
			group !== "actor" ||
			code !== "overloaded" ||
			metadata === null ||
			typeof metadata !== "object"
		) {
			return false;
		}

		const overload = metadata as {
			channel?: unknown;
			operation?: unknown;
		};
		return (
			overload.channel === "dispatch_inbox" &&
			overload.operation === "dispatch_queue_send"
		);
	}

	#clearResolvedActorId(): void {
		this.#resolvedActorId = undefined;
		this.#resolvingActorId = undefined;
	}

	async #shouldRetrySchedulingError(
		group: string,
		code: string,
		actorId: string | undefined,
		attempt: number,
		maxAttempts: number,
		rayId?: string,
	): Promise<boolean> {
		if (
			!isDynamicActorQuery(this.#actorResolutionState) ||
			!isSchedulingError(group, code) ||
			attempt >= maxAttempts - 1
		) {
			return false;
		}

		if (actorId) {
			const schedulingError = await checkForSchedulingError(
				group,
				code,
				actorId,
				this.#actorResolutionState,
				this.#driver,
				rayId,
			);
			if (schedulingError) {
				throw schedulingError;
			}
		}

		this.#clearResolvedActorId();
		return true;
	}

	#invalidateResolvedActorId(group: string, code: string): boolean {
		if (
			!isDynamicActorQuery(this.#actorResolutionState) ||
			!isStaleResolvedActorError(group, code)
		) {
			return false;
		}

		this.#clearResolvedActorId();
		return true;
	}

	async #resolveActionTarget(useQueryTarget: boolean) {
		if ("getForId" in this.#actorResolutionState) {
			return getGatewayTarget(this.#actorResolutionState);
		}

		if (useQueryTarget) {
			return getGatewayTarget(this.#actorResolutionState);
		}

		if (this.#resolvedActorId) {
			return { directId: this.#resolvedActorId } as const;
		}

		if (!this.#resolvingActorId) {
			this.#resolvingActorId = resolveGatewayTarget(
				this.#driver,
				this.#actorResolutionState,
			).then((actorId) => {
				this.#resolvedActorId = actorId;
				return actorId;
			});
		}

		try {
			return { directId: await this.#resolvingActorId } as const;
		} finally {
			this.#resolvingActorId = undefined;
		}
	}

	async #resolveGatewayRequestTarget(
		useQueryTarget: boolean,
		gatewayOptions: ActorGatewayOptions,
	) {
		if (gatewayOptions.skipReadyWait) {
			return getGatewayTarget(this.#actorResolutionState);
		}

		return await this.#resolveActionTarget(useQueryTarget);
	}

	/**
	 * Establishes a persistent connection to the actor.
	 *
	 * @template AD The actor class that this connection is for.
	 * @returns {ActorConn<AD>} A connection to the actor.
	 */
	connect(
		params?: unknown,
		options: ActorConnectOptions = {},
	): ActorConn<AnyActorDefinition> {
		logger().debug({
			msg: "establishing connection from handle",
			query: this.#actorResolutionState,
		});

		const connParams = params === undefined ? this.#params : params;
		const getParams = params === undefined ? this.#getParams : undefined;

		const conn = new ActorConnRaw(
			this.#client,
			this.#driver,
			connParams,
			getParams,
			this.#encoding,
			this.#actorResolutionState,
			resolveActorGatewayOptions(this.#gatewayOptions, options),
		);

		return this.#client[CREATE_ACTOR_CONN_PROXY](
			conn,
		) as ActorConn<AnyActorDefinition>;
	}

	/**
	 * Fetches a resource from this actor via the /request endpoint. This is a
	 * convenience wrapper around the raw HTTP API.
	 */
	fetch(input: string | URL | Request, init?: ActorFetchInit) {
		return this.#fetchWithResolvedActor(input, init);
	}

	async #fetchWithResolvedActor(
		input: string | URL | Request,
		init?: ActorFetchInit,
	) {
		const { skipReadyWait, ...restInit } = init ?? {};
		const maxAttempts = this.#getDynamicQueryMaxAttempts();
		// Buffer a streaming init body before the loop, but only when retries are possible.
		const requestInit =
			maxAttempts > 1 ? await prepareRetryableInit(restInit) : restInit;
		// Clone the input Request only when its own body is the one being sent.
		// If init replaces the body, cloning a consumed input would throw.
		const clonesInputBody =
			requestInit.body === undefined &&
			isRequestLike(input) &&
			input.body !== null;
		let useQueryTarget = isDynamicActorQuery(this.#actorResolutionState);
		const gatewayOptions = resolveActorGatewayOptions(
			this.#gatewayOptions,
			{
				skipReadyWait,
			},
		);

		for (let attempt = 0; attempt < maxAttempts; attempt++) {
			let actorId: string | undefined;
			try {
				const target = await this.#resolveGatewayRequestTarget(
					useQueryTarget,
					gatewayOptions,
				);
				actorId = "directId" in target ? target.directId : undefined;
				const response = await rawHttpFetch(
					this.#driver,
					target,
					this.#params,
					// Clone a body-bearing Request so retries don't reuse a consumed stream.
					// NOTE: This holds the body in memory until GC (unavoidable).
					clonesInputBody ? input.clone() : input,
					requestInit,
					gatewayOptions,
				);
				const retry = await this.#shouldRetryRawFetchResponse(
					response,
					actorId,
					attempt,
					maxAttempts,
				);
				if (retry) {
					useQueryTarget = retry.useQueryTarget;
					if (retry.waitForRetryWindow) {
						await this.#waitForRetryWindow();
					}
					continue;
				}
				return response;
			} catch (err) {
				const { group, code, message, metadata, rayId, actor } =
					deconstructError(err, true);

				if (
					await this.#shouldRetrySchedulingError(
						group,
						code,
						actorId,
						attempt,
						maxAttempts,
						rayId,
					)
				) {
					useQueryTarget = true;
					await this.#waitForRetryWindow();
					continue;
				}

				if (
					this.#shouldRetryDynamicLifecycleError(
						group,
						code,
						attempt,
						maxAttempts,
					)
				) {
					this.#clearResolvedActorId();
					useQueryTarget = true;
					await this.#waitForRetryWindow();
					continue;
				}

				const invalidated = this.#invalidateResolvedActorId(
					group,
					code,
				);
				if (invalidated && attempt < maxAttempts - 1) {
					const waitForReady =
						code === "starting" ||
						code === "stopping" ||
						code.startsWith("destroyed_");
					useQueryTarget = useQueryTarget || waitForReady;
					if (waitForReady) {
						await this.#waitForRetryWindow();
					}
					continue;
				}

				throw new ActorError(group, code, message, {
					metadata,
					rayId,
					actor,
				});
			}
		}

		throw new Error("unreachable fetch retry state");
	}

	async #shouldRetryRawFetchResponse(
		response: Response,
		actorId: string | undefined,
		attempt: number,
		maxAttempts: number,
	): Promise<{
		useQueryTarget: boolean;
		waitForRetryWindow: boolean;
	} | null> {
		if (response.ok || !isDynamicActorQuery(this.#actorResolutionState)) {
			return null;
		}

		const error = await this.#parseRawFetchErrorResponse(response);
		if (!error) {
			return null;
		}

		const { group, code, rayId } = error;

		if (
			await this.#shouldRetrySchedulingError(
				group,
				code,
				actorId,
				attempt,
				maxAttempts,
				rayId,
			)
		) {
			return {
				useQueryTarget: true,
				waitForRetryWindow: true,
			};
		}

		if (
			this.#shouldRetryDynamicLifecycleError(
				group,
				code,
				attempt,
				maxAttempts,
			)
		) {
			this.#clearResolvedActorId();
			return {
				useQueryTarget: true,
				waitForRetryWindow: true,
			};
		}

		const invalidated = this.#invalidateResolvedActorId(group, code);
		if (invalidated && attempt < maxAttempts - 1) {
			const waitForReady =
				code === "starting" ||
				code === "stopping" ||
				code.startsWith("destroyed_");
			return {
				useQueryTarget: true,
				waitForRetryWindow: waitForReady,
			};
		}

		return null;
	}

	async #parseRawFetchErrorResponse(response: Response): Promise<{
		group: string;
		code: string;
		message: string;
		metadata?: unknown;
		rayId?: string;
		actor?: ActorSpecifier;
	} | null> {
		if (response.ok) {
			return null;
		}

		const contentType = response.headers.get("content-type");
		const encoding: Encoding = contentType?.includes("application/json")
			? "json"
			: this.#encoding;

		try {
			const error = deserializeWithEncoding<
				protocol.HttpResponseError,
				HttpResponseErrorJson,
				{
					group: string;
					code: string;
					message: string;
					metadata?: unknown;
					rayId?: string;
					actor?: ActorSpecifier;
				}
			>(
				encoding,
				new Uint8Array(await response.clone().arrayBuffer()),
				HTTP_RESPONSE_ERROR_VERSIONED,
				HttpResponseErrorSchema,
				(json) => ({
					...json,
					actor: json.actor
						? {
								...json.actor,
								generation: Number(json.actor.generation),
							}
						: undefined,
				}),
				(bare) => ({
					group: bare.group,
					code: bare.code,
					message: bare.message,
					metadata: bare.metadata
						? decodeCborCompat(new Uint8Array(bare.metadata))
						: undefined,
					actor: bare.actor
						? {
								actorId: bare.actor.actorId,
								generation: Number(bare.actor.generation),
								key: bare.actor.key ?? undefined,
							}
						: undefined,
				}),
			);
			return {
				...error,
				rayId: response.headers.get("x-rivet-ray-id") ?? undefined,
			};
		} catch {
			return null;
		}
	}

	/**
	 * Opens a raw WebSocket connection to this actor.
	 */
	async webSocket(
		path?: string,
		protocols?: string | string[],
		options: ActorWebSocketOptions = {},
	) {
		const params = await this.#resolveConnectionParams();
		const gatewayOptions = resolveActorGatewayOptions(
			this.#gatewayOptions,
			options,
		);
		const useQueryTarget = isDynamicActorQuery(this.#actorResolutionState);
		const target = await this.#resolveGatewayRequestTarget(
			useQueryTarget,
			gatewayOptions,
		);
		return await rawWebSocket(
			this.#driver,
			target,
			params,
			path,
			protocols,
			gatewayOptions,
		);
	}

	/**
	 * Resolves the actor to get its unique actor ID.
	 */
	async resolve(): Promise<string> {
		if ("getForId" in this.#actorResolutionState) {
			return this.#actorResolutionState.getForId.actorId;
		}

		const target = await this.#resolveActionTarget(false);
		if ("directId" in target) {
			return target.directId;
		}

		throw new Error(
			"dynamic actor resolution did not produce a direct actor id",
		);
	}

	/**
	 * Returns the raw URL for routing traffic to the actor.
	 */
	async getGatewayUrl(): Promise<string> {
		return await this.#driver.buildGatewayUrl(
			getGatewayTarget(this.#actorResolutionState),
			this.#gatewayOptions,
		);
	}

	async reload(): Promise<void> {
		const target = getGatewayTarget(this.#actorResolutionState);
		const request = new Request("http://actor/dynamic/reload", {
			method: "PUT",
		});
		const response = await this.#driver.sendRequest(target, request);
		if (!response.ok) {
			const body = await response.text().catch(() => "");
			throw new ActorError(
				"actor",
				"reload_failed",
				`reload failed with status ${response.status}: ${body}`,
				{
					rayId: response.headers.get("x-rivet-ray-id") ?? undefined,
				},
			);
		}
	}
}

/**
 * Stateless handle to a actor. Allows calling actor's remote procedure calls with inferred types
 * without establishing a persistent connection.
 *
 * @example
 * ```
 * const room = client.get<ChatRoom>(...etc...);
 * // This calls the action named `sendMessage` on the `ChatRoom` actor without a connection.
 * await room.sendMessage('Hello, world!');
 * ```
 *
 * Private methods (e.g. those starting with `_`) are automatically excluded.
 *
 * @template AD The actor class that this handle is for.
 * @see {@link ActorHandleRaw}
 */
export type ActorHandle<AD extends AnyActorDefinition> = Omit<
	ActorHandleRaw,
	"connect" | "send"
> & {
	// Add typed version of ActorConn (instead of using AnyActorDefinition)
	connect(params?: unknown, options?: ActorConnectOptions): ActorConn<AD>;
	// Resolve method returns the actor ID
	resolve(): Promise<string>;
} & ActorDefinitionQueueSend<AD> &
	ActorDefinitionActions<AD>;
