import { AsyncLocalStorage } from "node:async_hooks";
import { describe, expect, test } from "vitest";
import {
	BRIDGE_RIVET_ERROR_PREFIX,
	decodeBridgeRivetError,
	type RivetError,
} from "@/actor/errors";
import { actor, defineRunHandler, type RunControl } from "@/actor/mod";
import { type RegistryConfig, RegistryConfigSchema } from "@/registry/config";
import { NapiCoreRuntime } from "@/registry/napi-runtime";
import { buildNativeFactory } from "@/registry/native";
import type {
	ActorContextHandle,
	CoreRuntime,
	RuntimeServeConfig,
} from "@/registry/runtime";
import { type WasmBindings, WasmCoreRuntime } from "@/registry/wasm-runtime";
import { decodeCborCompat, encodeCborCompat } from "@/serde";

const serveConfig: RuntimeServeConfig = {
	version: 4,
	endpoint: "https://api.rivet.dev",
	token: "parity-token",
	namespace: "parity-namespace",
	poolName: "parity-pool",
	serverlessPackageVersion: "0.0.0",
	serverlessValidateEndpoint: true,
	serverlessMaxStartPayloadBytes: 1024,
};

type NativeCallbacks = {
	createState?: (
		error: unknown,
		payload: {
			ctx: ActorContextHandle;
			input?: Uint8Array;
		},
	) => Promise<Uint8Array>;
	run?: (
		error: unknown,
		payload: { ctx: ActorContextHandle },
	) => Promise<void>;
	actions: Record<
		string,
		(
			error: unknown,
			payload: {
				ctx: ActorContextHandle;
				conn: null;
				name: string;
				args: Uint8Array;
				scheduledFire?: {
					kind: "at" | "cron" | "every";
					id: string;
					name?: string;
					scheduledAt: number;
					firedAt: number;
				};
				cancelToken: FakeCancellationToken;
			},
		) => Promise<Uint8Array>
	>;
};

type RuntimeCase = {
	kind: CoreRuntime["kind"];
	runtime: CoreRuntime;
	scenario: ParityScenario;
};

type PromotedStatusCase = {
	group: string;
	code: string;
	statusCode: number;
};

class Gate {
	#started!: () => void;
	#released!: () => void;

	readonly started = new Promise<void>((resolve) => {
		this.#started = resolve;
	});
	readonly released = new Promise<void>((resolve) => {
		this.#released = resolve;
	});

	markStarted(): void {
		this.#started();
	}

	release(): void {
		this.#released();
	}
}

class ParityScenario {
	readonly save = new Gate();
	readonly registerTask = new Gate();
	readonly saves: unknown[] = [];
	readonly runWakeAt: Array<number | null> = [];
	registerTaskCompleted = false;
	runRestarts = 0;
}

class FakeActorContext {
	stateBytes = Buffer.alloc(0);
	readonly runtimeBag = {};
	readonly registeredTasks: Array<Promise<void>> = [];
	readonly abortController = new AbortController();
	queueMessages: Array<{
		id(): bigint;
		name(): string;
		body(): Uint8Array;
		createdAt(): number;
		isCompletable(): boolean;
		complete(): Promise<void>;
	}> = [];

	constructor(
		private readonly scenario: ParityScenario,
		private readonly saveError?: PromotedStatusCase,
	) {}

	state(): Buffer {
		return this.stateBytes;
	}

	beginOnStateChange(): void {}

	endOnStateChange(): void {}

	requestSave(opts?: unknown): void {
		this.scenario.saves.push(opts);
	}

	async requestSaveAndWait(opts?: unknown): Promise<void> {
		this.scenario.saves.push(opts);
		if (this.saveError) {
			throw bridgeError(this.saveError);
		}
		this.scenario.save.markStarted();
		await this.scenario.save.released;
	}

	registerTask(promise: Promise<unknown>): void {
		this.registeredTasks.push(Promise.resolve(promise).then(() => {}));
	}

	async drainRegisteredTasks(): Promise<void> {
		while (this.registeredTasks.length > 0) {
			const tasks = this.registeredTasks.splice(0);
			await Promise.all(tasks);
		}
	}

	runtimeState(): object {
		return this.runtimeBag;
	}

	invocationTraceContext(): undefined {
		return undefined;
	}

	sameActorInstance(other: FakeActorContext): boolean {
		return this === other;
	}

	actorId(): string {
		return "parity-actor";
	}

	name(): string {
		return "parity";
	}

	key(): Array<{ kind: string; stringValue: string }> {
		return [{ kind: "string", stringValue: "key" }];
	}

	region(): string {
		return "local";
	}

	conns(): unknown[] {
		return [];
	}

	abortSignal(): AbortSignal {
		return this.abortController.signal;
	}

	queue() {
		return {
			nextBatch: async () => this.queueMessages.splice(0),
		};
	}

	restartRunHandler(): void {
		this.scenario.runRestarts += 1;
	}

	async setRunWakeAt(timestamp: number | null): Promise<void> {
		this.scenario.runWakeAt.push(timestamp);
	}
}

class FakeCancellationToken {
	#cancelled = false;
	#callbacks: Array<() => void> = [];

	aborted(): boolean {
		return this.#cancelled;
	}

	cancel(): void {
		this.#cancelled = true;
		for (const callback of this.#callbacks) {
			callback();
		}
	}

	onCancelled(callback: () => void): void {
		this.#callbacks.push(callback);
	}
}

class FakeActorFactory {
	constructor(
		readonly callbacks: NativeCallbacks,
		readonly config: Record<string, unknown> | null | undefined,
	) {}
}

class FakeCoreRegistry {
	readonly registered = new Map<string, FakeActorFactory>();
	activeCtx?: FakeActorContext;

	constructor(private readonly scenario: ParityScenario) {}

	register(name: string, factory: FakeActorFactory): void {
		this.registered.set(name, factory);
	}

	async serve(): Promise<void> {
		const factory = this.registered.get("parity");
		if (!factory) {
			throw new Error("parity actor was not registered");
		}

		const ctx = new FakeActorContext(this.scenario);
		this.activeCtx = ctx;
		const stateBytes = await factory.callbacks.createState?.(null, {
			ctx,
			input: encodeValue(null),
		});
		ctx.stateBytes = Buffer.from(stateBytes ?? encodeValue({ count: 0 }));

		let actionSettled = false;
		const actionPromise = factory.callbacks.actions.lifecycle(null, {
			ctx,
			conn: null,
			name: "lifecycle",
			args: encodeValue([]),
			cancelToken: new FakeCancellationToken(),
		});
		void actionPromise.finally(() => {
			actionSettled = true;
		});

		await this.scenario.save.started;
		await Promise.resolve();
		expect(actionSettled).toBe(false);
		this.scenario.save.release();

		expect(decodeValue<{ count: number }>(await actionPromise)).toEqual({
			count: 1,
		});
	}

	async shutdown(): Promise<void> {
		await this.activeCtx?.drainRegisteredTasks();
	}
}

function bridgeError(error: PromotedStatusCase): Error {
	return new Error(
		`${BRIDGE_RIVET_ERROR_PREFIX}${JSON.stringify({
			group: error.group,
			code: error.code,
			message: `${error.group}.${error.code}`,
			metadata: null,
			public: true,
			statusCode: error.statusCode,
		})}`,
	);
}

function encodeValue(value: unknown): Uint8Array {
	return encodeCborCompat(value);
}

function decodeValue<T>(value: Uint8Array): T {
	return decodeCborCompat<T>(value);
}

function fakeNapiBindings(scenario: ParityScenario) {
	return {
		CoreRegistry: class extends FakeCoreRegistry {
			constructor() {
				super(scenario);
			}
		},
		NapiActorFactory: FakeActorFactory,
		CancellationToken: FakeCancellationToken,
		ActorContext: class {},
		// The native module exports this, and `createRegistry` calls it on
		// every registry, so the fake has to carry it too.
		setTelemetryLogSink: () => {},
	};
}

function fakeWasmBindings(scenario: ParityScenario): WasmBindings {
	return {
		CoreRegistry: class extends FakeCoreRegistry {
			constructor() {
				super(scenario);
			}
		},
		ActorFactory: FakeActorFactory,
		CancellationToken: FakeCancellationToken,
		ActorContext: class {},
		ConnHandle: class {},
		WebSocketHandle: class {},
		bridgeRivetErrorPrefix: () => BRIDGE_RIVET_ERROR_PREFIX,
		roundTripBytes: (bytes: Uint8Array) => bytes,
		uint8ArrayFromBytes: (bytes: Uint8Array) => bytes,
		awaitPromise: async <T>(promise: Promise<T>) => await promise,
		default: async () => {},
	} as unknown as WasmBindings;
}

function createRuntimeCase(kind: CoreRuntime["kind"]): RuntimeCase {
	const scenario = new ParityScenario();
	return {
		kind,
		scenario,
		runtime:
			kind === "napi"
				? new NapiCoreRuntime(
						fakeNapiBindings(scenario) as never,
						new AsyncLocalStorage(),
					)
				: new WasmCoreRuntime(fakeWasmBindings(scenario)),
	};
}

function registryConfig(definition: ReturnType<typeof actor>): RegistryConfig {
	return RegistryConfigSchema.parse({
		use: { parity: definition },
		endpoint: serveConfig.endpoint,
		token: serveConfig.token,
		namespace: serveConfig.namespace,
		noWelcome: true,
		startEngine: false,
		test: {
			enabled: true,
			sqliteBackend: "remote",
		},
	});
}

async function runLifecycleScenario(runtimeCase: RuntimeCase): Promise<void> {
	const { runtime, scenario } = runtimeCase;
	const registry = runtime.createRegistry();
	const definition = actor({
		state: { count: 0 },
		actions: {
			lifecycle: async (c) => {
				c.state.count += 1;
				await c.saveState({ immediate: true });
				void (
					c as unknown as {
						internalKeepAwake<T>(run: () => Promise<T>): Promise<T>;
					}
				).internalKeepAwake(async () => {
					scenario.registerTask.markStarted();
					await scenario.registerTask.released;
					scenario.registerTaskCompleted = true;
				});
				return { count: c.state.count };
			},
		},
	});

	runtime.registerActor(
		registry,
		"parity",
		buildNativeFactory(runtime, registryConfig(definition), definition),
	);
	await runtime.serveRegistry(registry, serveConfig);
	await scenario.registerTask.started;
	expect(scenario.saves).toContainEqual({ immediate: true });

	let shutdownSettled = false;
	const shutdownPromise = runtime.shutdownRegistry(registry).then(() => {
		shutdownSettled = true;
	});
	await Promise.resolve();
	expect(shutdownSettled).toBe(false);
	expect(scenario.registerTaskCompleted).toBe(false);

	scenario.registerTask.release();
	await shutdownPromise;
	expect(shutdownSettled).toBe(true);
	expect(scenario.registerTaskCompleted).toBe(true);
}

async function invokePromotedStatus(
	runtimeCase: RuntimeCase,
	promoted: PromotedStatusCase,
): Promise<RivetError> {
	const { runtime, scenario } = runtimeCase;
	const definition = actor({
		state: {},
		actions: {
			status: async (c) => {
				await c.saveState({ immediate: true });
			},
		},
	});
	const factory = buildNativeFactory(
		runtime,
		registryConfig(definition),
		definition,
	) as unknown as FakeActorFactory;
	const ctx = new FakeActorContext(scenario, promoted);
	const stateBytes = await factory.callbacks.createState?.(null, {
		ctx,
		input: encodeValue(null),
	});
	ctx.stateBytes = Buffer.from(stateBytes ?? encodeValue({}));

	try {
		await factory.callbacks.actions.status(null, {
			ctx,
			conn: null,
			name: "status",
			args: encodeValue([]),
			cancelToken: new FakeCancellationToken(),
		});
		throw new Error("expected status action to fail");
	} catch (error) {
		if (!(error instanceof Error)) {
			throw error;
		}
		const decoded = decodeBridgeRivetError(error.message);
		if (!decoded) {
			throw error;
		}
		return decoded;
	}
}

describe("CoreRuntime NAPI and wasm parity", () => {
	test.each([
		["napi", 42n, 42],
		["wasm", 42n, 42],
		["napi", 2n ** 53n + 1n, 2n ** 53n + 1n],
		["wasm", 2n ** 53n + 1n, 2n ** 53n + 1n],
	] as const)("%s exposes queue message id %s without losing precision", async (kind, messageId, expectedId) => {
		const runtimeCase = createRuntimeCase(kind);
		let receivedId: number | bigint | undefined;
		const definition = actor({
			state: {},
			actions: {
				receive: async (c) => {
					receivedId = (await c.queue.next()).id;
				},
			},
		});
		const factory = buildNativeFactory(
			runtimeCase.runtime,
			registryConfig(definition),
			definition,
		) as unknown as FakeActorFactory;
		const ctx = new FakeActorContext(runtimeCase.scenario);
		ctx.stateBytes = Buffer.from(
			(await factory.callbacks.createState?.(null, {
				ctx,
				input: encodeValue(null),
			})) ?? encodeValue({}),
		);
		ctx.queueMessages.push({
			id: () => messageId,
			name: () => "work",
			body: () => encodeValue({}),
			createdAt: () => 0,
			isCompletable: () => false,
			complete: async () => {},
		});

		await factory.callbacks.actions.receive(null, {
			ctx,
			conn: null,
			name: "receive",
			args: encodeValue([]),
			cancelToken: new FakeCancellationToken(),
		});

		expect(receivedId).toBe(expectedId);
	});

	test.each([
		"napi",
		"wasm",
	] as const)("%s rejects run wake deadlines without a run handler", async (kind) => {
		const runtimeCase = createRuntimeCase(kind);
		const definition = actor({
			actions: {
				setWake: async (c) => {
					await c.run.setWakeAt(123);
				},
			},
		});
		const factory = buildNativeFactory(
			runtimeCase.runtime,
			registryConfig(definition),
			definition,
		) as unknown as FakeActorFactory;
		const ctx = new FakeActorContext(runtimeCase.scenario);

		await expect(
			factory.callbacks.actions.setWake(null, {
				ctx,
				conn: null,
				name: "setWake",
				args: encodeValue([]),
				cancelToken: new FakeCancellationToken(),
			}),
		).rejects.toMatchObject({
			group: "actor",
			code: "run_handler_unavailable",
		});
		expect(runtimeCase.scenario.runWakeAt).toEqual([]);
	});

	test.each([
		"napi",
		"wasm",
	] as const)("%s shares the public run-control gate with runtime starts", async (kind) => {
		const runtimeCase = createRuntimeCase(kind);
		const started = new Gate();
		let control: RunControl | undefined;
		const definition = actor({
			run: defineRunHandler(
				async () => {
					started.markStarted();
					await started.released;
				},
				{
					inspectorKind: "workflow",
					createInspector: (context) => {
						control = context.control;
						return {
							inspector: {
								workflow: {
									getHistory: () => null,
									getState: async () => null,
									onHistoryUpdated: () => () => {},
									replayFromStep: async () => null,
								},
							},
						};
					},
				},
			),
		});
		const factory = buildNativeFactory(
			runtimeCase.runtime,
			registryConfig(definition),
			definition,
		) as unknown as FakeActorFactory;
		const callbacks = factory.callbacks as NativeCallbacks & {
			getWorkflowHistory?: NativeCallbacks["run"];
		};
		const ctx = new FakeActorContext(runtimeCase.scenario);

		await callbacks.getWorkflowHistory?.(null, { ctx });
		expect(control).toBeDefined();
		const running = callbacks.run?.(null, { ctx });
		await started.started;

		await expect(
			control?.run.withInactive({}, async () => {}),
		).rejects.toMatchObject({
			group: "actor",
			code: "run_handler_unavailable",
		});

		started.release();
		await running;
		await control?.run.withInactive(
			{ restartOnSuccess: true },
			async () => {},
		);
		expect(runtimeCase.scenario.runRestarts).toBe(1);
	});
	test("scheduled fire metadata is appended after validated action args", async () => {
		const runtimeCase = createRuntimeCase("napi");
		let received: unknown[] = [];
		const definition = actor({
			state: {},
			actions: {
				report: async (_c, team: string, fire?: unknown) => {
					received = [team, fire];
				},
			},
		});
		const factory = buildNativeFactory(
			runtimeCase.runtime,
			registryConfig(definition),
			definition,
		) as unknown as FakeActorFactory;
		const ctx = new FakeActorContext(runtimeCase.scenario);
		const fire = {
			kind: "cron" as const,
			id: "daily-report",
			name: "daily-report",
			scheduledAt: 100,
			firedAt: 125,
		};

		await factory.callbacks.actions.report(null, {
			ctx,
			conn: null,
			name: "report",
			args: encodeValue(["operations"]),
			scheduledFire: fire,
			cancelToken: new FakeCancellationToken(),
		});

		expect(received).toEqual(["operations", fire]);
	});

	test.each([
		"napi",
		"wasm",
	] as const)("%s waits for durable saves and drains registered tasks", async (kind) => {
		await runLifecycleScenario(createRuntimeCase(kind));
	});

	test.each([
		{ group: "auth", code: "forbidden", statusCode: 403 },
		{ group: "actor", code: "action_not_found", statusCode: 404 },
		{ group: "actor", code: "action_timed_out", statusCode: 408 },
	])("preserves promoted $group.$code statusCode across NAPI and wasm", async (promoted) => {
		const nativeError = await invokePromotedStatus(
			createRuntimeCase("napi"),
			promoted,
		);
		const wasmError = await invokePromotedStatus(
			createRuntimeCase("wasm"),
			promoted,
		);

		expect(nativeError).toMatchObject(promoted);
		expect(wasmError).toMatchObject(promoted);
	});
});
