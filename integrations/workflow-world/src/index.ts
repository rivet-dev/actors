import {
	MessageId,
	QueuePayloadSchema,
	ValidQueueName as ValidQueueNameSchema,
	SPEC_VERSION_CURRENT,
	getQueueTopicPrefix,
	resolveQueueNamespace,
	type CreateEventParams,
	type CreateEventRequest,
	type EventResult,
	type GetChunksOptions,
	type GetEventParams,
	type GetHookParams,
	type GetStepParams,
	type GetWorkflowRunParams,
	type ListEventsByCorrelationIdParams,
	type ListEventsParams,
	type ListHooksParams,
	type ListWorkflowRunStepsParams,
	type ListWorkflowRunsParams,
	type QueueOptions,
	type QueuePayload,
	type QueuePrefix,
	type RunCreatedEventRequest,
	type StreamChunksResponse,
	type StreamInfoResponse,
	type ValidQueueName,
	type WorkflowRun,
	type World,
} from "@workflow/world";
import { HookNotFoundError, WorkflowWorldError } from "@workflow/errors";
import { setup, type Registry } from "rivetkit";
import { createClient, type Client } from "rivetkit/client";
import { monotonicFactory } from "ulid";
import { registry, workflowWorldActors } from "./actors.js";

export type RivetWorldConfig = {
	/** The application's combined registry, including `workflowWorldActors`. */
	registry?: RegistryWithReadiness;
	runtimeUrl?: string;
	/**
	 * Advanced: inject a pre-built RivetKit client (for example, a `setupTest`
	 * client) instead of constructing one with RivetKit's standard defaults.
	 */
	client?: Client<typeof registry>;
};

type HookConflictEventRequest = {
	eventType: "hook_conflict";
	specVersion?: number;
	correlationId: string;
	eventData: {
		token: string;
		conflictingRunId?: string;
	};
};

type EventCreateInput =
	| CreateEventRequest
	| RunCreatedEventRequest
	| HookConflictEventRequest;

function env(name: string) {
	return process.env[name];
}

function required(value: string | undefined, name: string) {
	if (!value) {
		throw new Error(
			`${name} is required for @rivet-dev/workflow-world. Set it in the app process environment.`,
		);
	}
	return value;
}

async function resolveDerivedRunId(
	lookup: () => Promise<string | null>,
): Promise<string | null> {
	// Canonical state commits before its coordinator index. Briefly tolerate that
	// propagation window so create-then-get stays consistent without making writes
	// depend on the coordinator.
	for (const delayMs of [0, 10, 25, 50, 100]) {
		if (delayMs > 0) {
			await new Promise((resolve) => setTimeout(resolve, delayMs));
		}
		const runId = await lookup();
		if (runId) return runId;
	}
	return null;
}

function jsonReplacer(_key: string, value: unknown) {
	if (value instanceof Uint8Array) {
		return {
			__type: "Uint8Array",
			data: Buffer.from(value).toString("base64"),
		};
	}
	return value;
}

function jsonReviver(_key: string, value: unknown) {
	if (
		value !== null &&
		typeof value === "object" &&
		"__type" in value &&
		(value as { __type?: unknown }).__type === "Uint8Array" &&
		"data" in value &&
		typeof (value as { data?: unknown }).data === "string"
	) {
		return new Uint8Array(
			Buffer.from((value as { data: string }).data, "base64"),
		);
	}
	return value;
}

function serializeQueuePayload(value: QueuePayload) {
	return JSON.stringify(value, jsonReplacer);
}

function streamChunkToBase64(chunk: string | Uint8Array) {
	const bytes =
		typeof chunk === "string" ? Buffer.from(chunk) : Buffer.from(chunk);
	return bytes.toString("base64");
}

function encodeStreamCursor(index: number) {
	return Buffer.from(JSON.stringify({ i: Math.max(0, index) })).toString(
		"base64",
	);
}

function decodeStreamCursor(cursor: string | null | undefined) {
	if (!cursor) return 0;
	try {
		const decoded = JSON.parse(Buffer.from(cursor, "base64").toString("utf8"));
		return typeof decoded.i === "number" ? Math.max(0, decoded.i) : 0;
	} catch {
		return 0;
	}
}

async function parseQueueRequestBody(req: Request) {
	return JSON.parse(await req.text(), jsonReviver);
}

function partitionForQueuePayload(message: QueuePayload) {
	const data = QueuePayloadSchema.parse(message);
	const runId = "runId" in data ? data.runId : "workflowRunId" in data ? data.workflowRunId : undefined;
	if (typeof runId !== "string" || runId.length === 0) {
		throw new Error("Workflow World queue payload requires a non-empty runId");
	}
	return runId;
}

function resolveRuntimeUrl(configured?: string) {
	const value =
		configured ??
		env("WORKFLOW_RUNTIME_URL") ??
		env("WORKFLOW_LOCAL_BASE_URL") ??
		(env("PORT") ? `http://localhost:${env("PORT")}` : undefined);
	return required(value, "WORKFLOW_RUNTIME_URL");
}

function queueHeaders(opts?: QueueOptions) {
	const headers = { ...(opts?.headers ?? {}) };
	const secret = env("RIVET_WORKFLOW_SECRET");
	if (secret && !("authorization" in headers) && !("Authorization" in headers)) {
		headers.authorization = `Bearer ${secret}`;
	}
	return headers;
}

function verifyWorkflowBearer(req: Request) {
	const secret = env("RIVET_WORKFLOW_SECRET");
	if (!secret) return null;
	const expected = `Bearer ${secret}`;
	if (req.headers.get("authorization") === expected) return null;
	return Response.json({ error: "Unauthorized" }, { status: 401 });
}

function runActor(client: Client<typeof registry>, runId: string) {
	return client.workflowRun.getOrCreate([runId]);
}

type RegistryWithReadiness = Registry<any> & {
	startAndWait(): Promise<void>;
};

function withReadyHandle<T extends object>(
	handle: T,
	applicationRegistry: RegistryWithReadiness,
): T {
	return new Proxy(handle, {
		get(target, prop, receiver) {
			if (prop === "then") return undefined;
			const value = Reflect.get(target, prop, receiver);
			if (typeof value !== "function") return value;
			// ActorHandle.connect() is intentionally synchronous. Stream setup waits
			// for registry readiness before calling it, so preserve that contract.
			if (prop === "connect") return value.bind(target);
			return async (...args: unknown[]) => {
				// Every outgoing World operation intentionally waits here. Registry
				// readiness is shared, so repeated/concurrent calls are cheap and cannot
				// start duplicate runtimes or let a cold request race registry startup.
				await applicationRegistry.startAndWait();
				return Reflect.apply(value, target, args);
			};
		},
	}) as T;
}

function withRegistryReadiness(
	client: Client<typeof registry>,
	applicationRegistry: RegistryWithReadiness | undefined,
): Client<typeof registry> {
	if (!applicationRegistry) return client;
	const actorNames = new Set(["workflowRun", "coordinator", "hookToken"]);
	return new Proxy(client, {
		get(target, prop, receiver) {
			if (typeof prop !== "string" || !actorNames.has(prop)) {
				const value = Reflect.get(target, prop, receiver);
				return typeof value === "function" ? value.bind(target) : value;
			}

			const accessor = Reflect.get(target, prop, receiver) as {
				get: (...args: unknown[]) => object;
				getOrCreate: (...args: unknown[]) => object;
				getForId: (...args: unknown[]) => object;
				create: (...args: unknown[]) => Promise<object>;
			};
			return {
				get: (...args: unknown[]) =>
					withReadyHandle(accessor.get(...args), applicationRegistry),
				getOrCreate: (...args: unknown[]) =>
					withReadyHandle(accessor.getOrCreate(...args), applicationRegistry),
				getForId: (...args: unknown[]) =>
					withReadyHandle(accessor.getForId(...args), applicationRegistry),
				create: async (...args: unknown[]) => {
					await applicationRegistry.startAndWait();
					return withReadyHandle(await accessor.create(...args), applicationRegistry);
				},
			};
		},
	}) as Client<typeof registry>;
}

const ulid = monotonicFactory();

export class RivetClientWorld {
	readonly specVersion = SPEC_VERSION_CURRENT;
	readonly #client: Client<typeof registry>;
	readonly #registry: RegistryWithReadiness | undefined;
	readonly #ownsRegistry: boolean;
	readonly #runtimeUrl?: string;
	readonly #injectedClient: boolean;
	#startPromise: Promise<void> | undefined;

	constructor(config: RivetWorldConfig = {}) {
		this.#runtimeUrl = config.runtimeUrl;
		this.#injectedClient = config.client != null;
		this.#ownsRegistry = config.registry == null && config.client == null;
		this.#registry =
			config.registry ??
			(config.client
				? undefined
				: (setup({ use: workflowWorldActors }) as RegistryWithReadiness));
		const registryConfig = this.#registry?.parseConfig();
		this.#client = withRegistryReadiness(
			config.client ??
				createClient<typeof registry>(
					registryConfig
						? {
								endpoint: registryConfig.endpoint,
								headers: registryConfig.headers,
								namespace: registryConfig.namespace,
								poolName: registryConfig.envoy.poolName,
								token: registryConfig.token,
							}
						: undefined,
				),
			this.#registry,
		);
	}

	async start() {
		this.#startPromise ??= this.#startOnce().catch((error) => {
			this.#startPromise = undefined;
			throw error;
		});
		await this.#startPromise;
	}

	async #startOnce() {
		// Per-run alarms and the transactionally persisted initial dispatch recover
		// their own work. World startup intentionally performs no global scan.
	}

	#workflowQueueName(workflowName: string) {
		return ValidQueueNameSchema.parse(
			`${getQueueTopicPrefix("workflow", resolveQueueNamespace())}${workflowName}`,
		);
	}

	runs = {
		get: async (id: string, params?: GetWorkflowRunParams) =>
			runActor(this.#client, id).getRun(id, params),
		list: async (params?: ListWorkflowRunsParams) =>
			this.#client.coordinator.getOrCreate(["coordinator"]).listRuns(params),
	};

	steps = {
		get: async (
			runId: string | undefined,
			stepId: string,
			params?: GetStepParams,
		) => {
			const effectiveRunId =
				runId ??
				(await resolveDerivedRunId(() =>
					this.#client.coordinator
						.getOrCreate(["coordinator"])
						.getRunIdByCorrelation(stepId),
				));
			if (!effectiveRunId) {
				throw new WorkflowWorldError(`Step not found: ${stepId}`);
			}
			return runActor(this.#client, effectiveRunId).getStep(
				effectiveRunId,
				stepId,
				params,
			);
		},
		list: async (params: ListWorkflowRunStepsParams) =>
			runActor(this.#client, params.runId).listSteps(params),
	};

	events = {
		create: async (
			runId: string | null,
			data: CreateEventRequest | RunCreatedEventRequest,
			params?: CreateEventParams,
		) => {
			let effectiveRunId =
				data.eventType === "run_created" && !runId ? `wrun_${ulid()}` : runId;
			if (data.eventType !== "run_created" && !effectiveRunId) {
				throw new Error("runId is required for non-run_created events");
			}
			let dataToStore: EventCreateInput = data;
			// Token reserved by THIS call that must be released if the append below
			// never lands (P0 #3): otherwise a thrown/crashed append leaves the
			// token claimed with no backing `hook_created` event, leaking it past
			// the 60s reconciliation grace.
			let reservationToConfirm:
				| { token: string; hookId: string; generation: number }
				| undefined;

			if (data.eventType === "hook_created") {
				if (!effectiveRunId) {
					throw new Error("runId is required for hook_created events");
				}
				const reservation = await this.#client.hookToken
					.getOrCreate([data.eventData.token])
					.reserve(data.eventData.token, effectiveRunId, data.correlationId);
				if (reservation.ok) {
					reservationToConfirm = {
						token: data.eventData.token,
						hookId: data.correlationId,
						generation: reservation.generation,
					};
				} else {
					dataToStore = {
						eventType: "hook_conflict",
						specVersion: data.specVersion,
						correlationId: data.correlationId,
						eventData: {
							token: data.eventData.token,
							...(reservation.runId
								? { conflictingRunId: reservation.runId }
								: {}),
						},
					};
				}
			}

			if (!effectiveRunId) {
				throw new Error("runId is required");
			}
			const actorRunId = effectiveRunId;
			let initialDispatch:
				| {
						runtimeUrl: string;
						queueName: string;
						headers: Record<string, string>;
						initial: {
							messageId: string;
							body: string;
							idempotencyKey: string;
						};
				  }
				| undefined;
			if (dataToStore.eventType === "run_created") {
				const queueName = this.#workflowQueueName(dataToStore.eventData.workflowName);
				const executionContext = dataToStore.eventData.executionContext;
				const traceCarrier = executionContext?.traceCarrier;
				const payload = {
					runId: actorRunId,
					...(traceCarrier == null ? {} : { traceCarrier }),
					runInput: {
						input: dataToStore.eventData.input,
						deploymentId: dataToStore.eventData.deploymentId,
						workflowName: dataToStore.eventData.workflowName,
						specVersion: dataToStore.specVersion ?? SPEC_VERSION_CURRENT,
						...(executionContext == null ? {} : { executionContext }),
						...(dataToStore.eventData.attributes == null
							? {}
							: { attributes: dataToStore.eventData.attributes }),
						...(dataToStore.eventData.allowReservedAttributes == null
							? {}
							: { allowReservedAttributes: true as const }),
					},
				};
				initialDispatch = {
					runtimeUrl: resolveRuntimeUrl(this.#runtimeUrl),
					queueName,
					headers: queueHeaders(),
					initial: {
						messageId: MessageId.parse(`msg_${ulid()}`),
						body: serializeQueuePayload(QueuePayloadSchema.parse(payload)),
						idempotencyKey: `workflow-start:${actorRunId}`,
					},
				};
			}
			let result: EventResult;
			try {
				result = await runActor(this.#client, actorRunId).appendEvent(
					actorRunId,
					dataToStore,
					params,
					initialDispatch,
					{
						...(reservationToConfirm == null
							? {}
							: { confirm: reservationToConfirm }),
					},
				);
			} catch (error) {
				if (reservationToConfirm) {
					// An action error is ambiguous: postcommit work can fail after the
					// canonical hook landed. Verify canonical ownership before deciding
					// whether to confirm or release this generation.
					try {
						const owns = await runActor(this.#client, actorRunId).ownsHook(
							reservationToConfirm.hookId,
							reservationToConfirm.token,
						);
						const tokenActor = this.#client.hookToken.getOrCreate([
							reservationToConfirm.token,
						]);
						if (owns) {
							await tokenActor.confirm(
								reservationToConfirm.token,
								reservationToConfirm.generation,
								actorRunId,
								reservationToConfirm.hookId,
							);
						} else {
							await tokenActor.release(
								reservationToConfirm.token,
								reservationToConfirm.generation,
								actorRunId,
								reservationToConfirm.hookId,
							);
						}
					} catch {}
				}
				throw error;
			}
			effectiveRunId = result.run?.runId ?? result.event?.runId ?? effectiveRunId;
			return result;
		},
		get: async (runId: string, eventId: string, params?: GetEventParams) =>
			runActor(this.#client, runId).getEvent(runId, eventId, params),
		list: async (params: ListEventsParams) =>
			runActor(this.#client, params.runId).listEvents(params),
		listByCorrelationId: async (params: ListEventsByCorrelationIdParams) => {
			const runId = await this.#client.coordinator
				.getOrCreate(["coordinator"])
				.getRunIdByCorrelation(params.correlationId);
			if (!runId) return { data: [], cursor: null, hasMore: false };
			return runActor(this.#client, runId).listEventsByCorrelationId(params);
		},
	};

	hooks = {
		get: async (hookId: string, params?: GetHookParams) => {
			const runId = await resolveDerivedRunId(() =>
				this.#client.coordinator
					.getOrCreate(["coordinator"])
					.getRunIdByHook(hookId),
			);
			if (!runId) throw new HookNotFoundError(hookId);
			return runActor(this.#client, runId).getHook(hookId, params);
		},
		getByToken: async (token: string, params?: GetHookParams) => {
			const owner = await this.#client.hookToken.getOrCreate([token]).get(token);
			if (!owner) throw new HookNotFoundError(token);
			return runActor(this.#client, owner.runId).getHook(owner.hookId, params);
		},
		list: async (params: ListHooksParams) => {
			if (!params.runId) {
				const page = await this.#client.coordinator
					.getOrCreate(["coordinator"])
					.listHookIndex(params);
				const data = [];
				for (const item of page.data) {
					try {
						data.push(
							await runActor(this.#client, item.runId).getHook(
								item.hookId,
								params,
							),
						);
					} catch (error) {
						if (!(error instanceof HookNotFoundError)) throw error;
					}
				}
				return { data, cursor: page.cursor, hasMore: page.hasMore };
			}
			return runActor(this.#client, params.runId).listHooks(params);
		},
	};

	async getDeploymentId() {
		// Rivet currently dispatches to WORKFLOW_RUNTIME_URL, so it cannot route an
		// unfinished run to immutable historical code. True deployment pinning
		// requires a retained deployment URL or versioned workflow bundle loader.
		return "rivet";
	}

	async queue(
		queueName: ValidQueueName,
		message: QueuePayload,
		opts?: QueueOptions,
	): Promise<{ messageId: MessageId }> {
		const messageId = MessageId.parse(`msg_${ulid()}`);
		const runId = partitionForQueuePayload(message);
		const result = await this.#client.workflowRun
			.getOrCreate([runId])
			.enqueue(
				runId,
				messageId,
				queueName,
				resolveRuntimeUrl(this.#runtimeUrl),
				serializeQueuePayload(message),
				queueHeaders(opts),
				opts?.delaySeconds,
				opts?.idempotencyKey ??
					("runInput" in message ? `workflow-start:${runId}` : undefined),
			);
		return { messageId: MessageId.parse(result.messageId) };
	}

	async inspectDispatcherQueue(runId: string) {
		return runActor(this.#client, runId).inspectQueue();
	}

	async forceStaleDispatcherMessageForTesting(
		runId: string,
		messageId: string,
		ageMs?: number,
	) {
		await this.#client.workflowRun
			.getOrCreate([runId])
			.forceStaleInflightForTesting(runId, messageId, ageMs);
	}

	async expireClosedStreamForTesting(runId: string, name: string) {
		return runActor(this.#client, runId).expireClosedStreamForTesting(name, runId);
	}

	createQueueHandler(
		queueNamePrefix: QueuePrefix,
		handler: (message: unknown, meta: any) => Promise<void | { timeoutSeconds: number }>,
	) {
		return async (req: Request) => {
			const unauthorized = verifyWorkflowBearer(req);
			if (unauthorized) return unauthorized;
			if (!req.body) {
				return Response.json({ error: "Missing request body" }, { status: 400 });
			}
			const queueName = req.headers.get("x-vqs-queue-name");
			const messageId = req.headers.get("x-vqs-message-id");
			const attemptHeader = req.headers.get("x-vqs-message-attempt");
			const attempt = Number(attemptHeader);
			if (
				!queueName ||
				!messageId ||
				attemptHeader == null ||
				!Number.isSafeInteger(attempt) ||
				attempt < 1
			) {
				return Response.json(
					{ error: "Missing required headers" },
					{ status: 400 },
				);
			}
			ValidQueueNameSchema.parse(queueName);
			const parsedMessageId = MessageId.parse(messageId);
			if (!queueName.startsWith(queueNamePrefix)) {
				return Response.json({ error: "Unhandled queue" }, { status: 400 });
			}
			const message = await parseQueueRequestBody(req);
			try {
				// Stable across delivery attempts. Workflow uses the message id as a
				// crash-recovery ownership token, not as an ingress deduplication key.
				const result = await handler(message, {
					attempt,
					queueName,
					messageId: parsedMessageId,
				});
				if (typeof result?.timeoutSeconds === "number") {
					return Response.json({ timeoutSeconds: result.timeoutSeconds });
				}
				return Response.json({ ok: true });
			} catch (error) {
				return Response.json(String(error), { status: 500 });
			}
		};
	}

	async writeToStream(name: string, runId: string, chunk: string | Uint8Array) {
		await runActor(this.#client, runId).writeStream(
			name,
			runId,
			streamChunkToBase64(chunk),
			false,
		);
	}

	async closeStream(name: string, runId: string) {
		await runActor(this.#client, runId).writeStream(name, runId, "", true);
	}

	async readFromStream(runId: string, name: string, startIndex = 0) {
		const store = runActor(this.#client, runId);
		let nextIndex = startIndex;
		if (nextIndex < 0) {
			const info = await store.getStreamInfo(name, runId);
			nextIndex = Math.max(0, info.tailIndex + 1 + nextIndex);
		}
		let cancelled = false;
		let disposeConnection: (() => Promise<void>) | undefined;
		const applicationRegistry = this.#registry;

		return new ReadableStream<Uint8Array>({
			async start(controller) {
				await applicationRegistry?.startAndWait();
				// `nextIndex` is the last-consumed cursor and is the single source of
				// truth for resume position: every drain reads forward from it, so a
				// later (surviving) `streamAppended` re-drain catches up any chunks
				// whose own broadcast was lost (best-effort delivery, SPEC §7).
				let draining = false;
				let pending = false;
				const drainOnce = async () => {
					for (;;) {
						const page = await store.getStreamChunks(name, runId, nextIndex, 100);
						for (const chunk of page.data) {
							controller.enqueue(chunk.data);
						}
						nextIndex += page.data.length;
						if (page.done) {
							cancelled = true;
							controller.close();
							await disposeConnection?.();
							return;
						}
						if (!page.hasMore) return;
					}
				};
				// Re-drain if signaled while a drain is in flight: a naive "one drain
				// at a time" guard would coalesce away the trailing notification and
				// strand the chunk that arrived mid-drain. The pending flag forces
				// another pass so no signal is lost.
				const scheduleDrain = () => {
					if (cancelled) return;
					pending = true;
					if (draining) return;
					draining = true;
					void (async () => {
						try {
							while (pending && !cancelled) {
								pending = false;
								await drainOnce();
							}
						} catch (error) {
							cancelled = true;
							controller.error(error);
							await disposeConnection?.();
						} finally {
							draining = false;
						}
					})();
				};

				const conn = store.connect();
				const unsubscribe = conn.on("streamAppended", (event) => {
					if (
						!cancelled &&
						(event as { streamId?: string; runId?: string }).streamId === name &&
						(event as { streamId?: string; runId?: string }).runId === runId
					) {
						scheduleDrain();
					}
				});
				// The actor connection may still be opening when the initial drain runs.
				// Re-drain once it is live so an append that raced connection setup is
				// recovered from persisted chunks even if its broadcast was missed.
				const unsubscribeOpen = conn.onOpen(scheduleDrain);
				disposeConnection = async () => {
					unsubscribe();
					unsubscribeOpen();
					await conn.dispose();
				};

				scheduleDrain();
			},
			cancel() {
				cancelled = true;
				void disposeConnection?.();
			},
		});
	}

	async listStreamsByRunId(runId: string) {
		return runActor(this.#client, runId).listStreams(runId);
	}

	async getStreamChunks(
		name: string,
		runId: string,
		options?: GetChunksOptions,
	): Promise<StreamChunksResponse> {
		const limit = Math.min(Math.max(options?.limit ?? 100, 1), 1000);
		return runActor(this.#client, runId).getStreamChunks(
			name,
			runId,
			decodeStreamCursor(options?.cursor),
			limit,
		);
	}

	async getStreamInfo(
		name: string,
		runId: string,
	): Promise<StreamInfoResponse> {
		return runActor(this.#client, runId).getStreamInfo(name, runId);
	}

	readonly streams = {
		write: (runId: string, name: string, chunk: string | Uint8Array) =>
			this.writeToStream(name, runId, chunk),
		close: (runId: string, name: string) => this.closeStream(name, runId),
		get: (runId: string, name: string, startIndex?: number) =>
			this.readFromStream(runId, name, startIndex),
		list: (runId: string) => this.listStreamsByRunId(runId),
		getChunks: (runId: string, name: string, options?: GetChunksOptions) =>
			this.getStreamChunks(name, runId, options),
		getInfo: (runId: string, name: string) =>
			this.getStreamInfo(name, runId),
	};

	async close() {
		if (this.#injectedClient) return;
		await this.#client.dispose();
		if (this.#ownsRegistry) await this.#registry?.shutdown();
	}
}

export function createWorld(config?: RivetWorldConfig): World {
	// The SDK's data-resolution overloads cannot be expressed on object-literal
	// namespaces, but the actor methods honor the same runtime contract.
	return new RivetClientWorld(config) as unknown as World;
}

export default createWorld;
export { registry };
