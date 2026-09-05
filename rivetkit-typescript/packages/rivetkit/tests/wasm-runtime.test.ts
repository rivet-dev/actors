import { AsyncLocalStorage } from "node:async_hooks";
import { describe, expect, test, vi } from "vitest";
import { BRIDGE_RIVET_ERROR_PREFIX, RivetError } from "@/actor/errors";
import { actor } from "@/actor/mod";
import { RegistryConfigSchema } from "@/registry/config";
import { NapiCoreRuntime } from "@/registry/napi-runtime";
import { buildNativeFactory } from "@/registry/native";
import type {
	ActorContextHandle,
	CoreRuntime,
	RuntimeServeConfig,
} from "@/registry/runtime";
import {
	loadWasmRuntime,
	type WasmBindings,
	WasmCoreRuntime,
} from "@/registry/wasm-runtime";
import { decodeCborJsonCompat, encodeCborCompat } from "@/serde";

const serveConfig: RuntimeServeConfig = {
	version: 4,
	endpoint: "https://api.rivet.dev",
	namespace: "default",
	poolName: "default",
	serverlessPackageVersion: "0.0.0",
	serverlessValidateEndpoint: true,
	serverlessMaxStartPayloadBytes: 1024,
};

class Deferred<T = void> {
	promise: Promise<T>;
	resolve!: (value: T | PromiseLike<T>) => void;
	reject!: (reason?: unknown) => void;

	constructor() {
		this.promise = new Promise<T>((resolve, reject) => {
			this.resolve = resolve;
			this.reject = reject;
		});
	}
}

function structuredBridgeError(reason: string): Error {
	return new Error(
		`${BRIDGE_RIVET_ERROR_PREFIX}${JSON.stringify({
			group: "wasm",
			code: "invalid_state",
			message: `Invalid wasm state 'core registry': ${reason}`,
			metadata: {
				state: "core registry",
				reason,
			},
		})}`,
	);
}

class FakeCoreRegistry {
	registered: Array<{ name: string; factory: FakeActorFactory }> = [];
	serveError?: Error;
	state:
		| "registering"
		| "buildingServerless"
		| "serving"
		| "serverless"
		| "shutdown" = "registering";
	serverlessBuilds = 0;
	serverlessRequests = 0;
	serverlessShutdowns = 0;
	buildStarted = new Deferred<void>();
	#buildRelease?: Deferred<void>;
	#buildWaiters: Array<Deferred<void>> = [];

	blockNextServerlessBuild(): void {
		this.#buildRelease = new Deferred<void>();
	}

	releaseServerlessBuild(): void {
		this.#buildRelease?.resolve();
	}

	#notifyBuildWaiters(): void {
		const waiters = this.#buildWaiters.splice(0);
		for (const waiter of waiters) {
			waiter.resolve();
		}
	}

	register(name: string, factory: FakeActorFactory): void {
		if (this.state !== "registering") {
			throw structuredBridgeError("already serving or shut down");
		}
		this.registered.push({ name, factory });
	}

	async serve(_config: RuntimeServeConfig): Promise<void> {
		if (
			this.state === "buildingServerless" ||
			this.state === "serverless"
		) {
			throw structuredBridgeError(
				"mode conflict: another run mode is already active",
			);
		}
		if (this.state === "shutdown") {
			throw structuredBridgeError("shut down");
		}
		this.state = "serving";
		if (this.serveError) {
			throw this.serveError;
		}
	}

	async shutdown(): Promise<void> {
		if (this.state === "serverless") {
			this.serverlessShutdowns += 1;
		}
		this.state = "shutdown";
		this.#notifyBuildWaiters();
	}

	async handleServerlessRequest(
		_req: unknown,
		onStreamEvent: (error: unknown, event?: unknown) => unknown,
		_cancelToken: unknown,
		_config: RuntimeServeConfig,
	): Promise<{ status: number; headers: Record<string, string> }> {
		await this.#ensureServerlessRuntime();
		this.serverlessRequests += 1;
		const requestCount = this.serverlessRequests;
		await onStreamEvent(null, { kind: "end" });
		return {
			status: 200,
			headers: { "x-request-count": String(requestCount) },
		};
	}

	async #ensureServerlessRuntime(): Promise<void> {
		for (;;) {
			switch (this.state) {
				case "serverless":
					return;
				case "shutdown":
					throw structuredBridgeError("shut down");
				case "serving":
					throw structuredBridgeError(
						"mode conflict: another run mode is already active",
					);
				case "buildingServerless": {
					const waiter = new Deferred<void>();
					this.#buildWaiters.push(waiter);
					await waiter.promise;
					continue;
				}
				case "registering":
					this.state = "buildingServerless";
					this.serverlessBuilds += 1;
					this.buildStarted.resolve();
					await this.#buildRelease?.promise;
					if (this.state === "shutdown") {
						this.serverlessShutdowns += 1;
						this.#notifyBuildWaiters();
						throw structuredBridgeError("shut down");
					}
					this.state = "serverless";
					this.#notifyBuildWaiters();
					return;
			}
		}
	}
}

class FakeActorFactory {
	constructor(
		readonly callbacks: object,
		readonly config: object | null | undefined,
	) {}
}

type NativeActorCallbacks = {
	createState?: (
		error: unknown,
		payload: {
			ctx: ActorContextHandle;
			input?: Uint8Array;
		},
	) => Promise<Uint8Array>;
	actions: Record<
		string,
		(
			error: unknown,
			payload: {
				ctx: ActorContextHandle;
				conn: null;
				name: string;
				args: Uint8Array;
			},
		) => Promise<Uint8Array>
	>;
};

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

function fakeWasmBindings(
	defaultFn: WasmBindings["default"] = async () => {},
): WasmBindings {
	return {
		CoreRegistry: FakeCoreRegistry,
		ActorFactory: FakeActorFactory,
		CancellationToken: FakeCancellationToken,
		ActorContext: class {},
		ConnHandle: class {},
		WebSocketHandle: class {},
		bridgeRivetErrorPrefix: () => BRIDGE_RIVET_ERROR_PREFIX,
		roundTripBytes: (bytes: Uint8Array) => bytes,
		uint8ArrayFromBytes: (bytes: Uint8Array) => bytes,
		awaitPromise: async <T>(promise: Promise<T>) => await promise,
		default: defaultFn,
	} as unknown as WasmBindings;
}

describe("WasmCoreRuntime", () => {
	test("satisfies the same shared runtime interface as the NAPI adapter", () => {
		const acceptRuntime = (_runtime: CoreRuntime) => {};

		acceptRuntime(new WasmCoreRuntime(fakeWasmBindings()));
		acceptRuntime(
			new NapiCoreRuntime({} as never, new AsyncLocalStorage()),
		);
	});

	test("maps raw wasm registry, factory, and cancellation handles", () => {
		const runtime = new WasmCoreRuntime(fakeWasmBindings());
		const registry = runtime.createRegistry();
		const factory = runtime.createActorFactory(
			{ run: vi.fn() },
			{ name: "actor" },
		);
		const token = runtime.createCancellationToken();
		const onCancel = vi.fn();

		runtime.registerActor(registry, "actor", factory);
		runtime.onCancellationTokenCancelled(token, onCancel);
		expect(runtime.cancellationTokenAborted(token)).toBe(false);
		runtime.cancelCancellationToken(token);

		expect((registry as unknown as FakeCoreRegistry).registered).toEqual([
			{ name: "actor", factory },
		]);
		expect(runtime.cancellationTokenAborted(token)).toBe(true);
		expect(onCancel).toHaveBeenCalledOnce();
	});

	test("runs shared actor callbacks without a Buffer global", async () => {
		const portableActor = actor({
			state: { ready: true },
			actions: {
				echo: (_ctx, value: string) => ({ value }),
			},
		});
		const config = RegistryConfigSchema.parse({
			use: { portable: portableActor },
			runtime: "wasm",
			sqlite: "remote",
			startEngine: false,
		});
		const runtime = new WasmCoreRuntime(fakeWasmBindings());
		const factory = buildNativeFactory(
			runtime,
			config,
			portableActor,
		) as unknown as FakeActorFactory;
		const callbacks = factory.callbacks as NativeActorCallbacks;
		const globalWithBuffer = globalThis as typeof globalThis & {
			Buffer?: unknown;
		};
		const previousBuffer = globalWithBuffer.Buffer;
		const runtimeState = {};
		const ctx = {
			actorId: () => "actor-1",
			runtimeState: () => runtimeState,
		} as unknown as ActorContextHandle;

		try {
			globalWithBuffer.Buffer = undefined;

			const stateBytes = await callbacks.createState?.(null, {
				ctx,
			});
			const outputBytes = await callbacks.actions.echo(null, {
				ctx,
				conn: null,
				name: "echo",
				args: encodeCborCompat(["ok"]),
			});

			expect(globalWithBuffer.Buffer).toBeUndefined();
			expect(
				decodeCborJsonCompat(stateBytes ?? new Uint8Array()),
			).toEqual({ ready: true });
			expect(decodeCborJsonCompat(outputBytes)).toEqual({ value: "ok" });
		} finally {
			globalWithBuffer.Buffer = previousBuffer;
		}
	});

	test("decodes structured wasm bridge errors", async () => {
		const runtime = new WasmCoreRuntime(fakeWasmBindings());
		const registry = runtime.createRegistry();
		(registry as unknown as FakeCoreRegistry).serveError = new Error(
			`${BRIDGE_RIVET_ERROR_PREFIX}${JSON.stringify({
				group: "sqlite",
				code: "remote_unavailable",
				message: "remote sqlite is unavailable",
				metadata: { backend: "remote" },
			})}`,
		);

		await expect(
			runtime.serveRegistry(registry, serveConfig),
		).rejects.toMatchObject({
			group: "sqlite",
			code: "remote_unavailable",
			message: "remote sqlite is unavailable",
			metadata: { backend: "remote" },
		});
	});

	test("fails explicitly when the wasm binding has not exported a runtime method", () => {
		const runtime = new WasmCoreRuntime(fakeWasmBindings());

		let error: unknown;
		try {
			runtime.actorId({} as ActorContextHandle);
		} catch (err) {
			error = err;
		}

		expect(error).toBeInstanceOf(RivetError);
		expect(error).toMatchObject({
			group: "runtime",
			code: "unsupported",
			metadata: {
				runtime: "wasm",
				method: "actorId",
			},
		});
	});

	test("returns queue max size through NAPI and wasm adapters", () => {
		const maxSize = 37;
		const context = {
			queue: () => ({
				maxSize: () => maxSize,
			}),
		} as unknown as ActorContextHandle;

		expect(
			new NapiCoreRuntime(
				{} as never,
				new AsyncLocalStorage(),
			).actorQueueMaxSize(context),
		).toBe(maxSize);
		expect(
			new WasmCoreRuntime(fakeWasmBindings()).actorQueueMaxSize(context),
		).toBe(maxSize);
	});

	test("shares a concurrent first serverless build", async () => {
		const runtime = new WasmCoreRuntime(fakeWasmBindings());
		const registry = runtime.createRegistry();
		const fakeRegistry = registry as unknown as FakeCoreRegistry;
		fakeRegistry.blockNextServerlessBuild();
		const token = runtime.createCancellationToken();
		const request = {
			method: "POST",
			url: "https://api.rivet.dev/api/rivet/start",
			headers: {},
			body: new Uint8Array(),
		};

		const first = runtime.handleServerlessRequest(
			registry,
			request,
			vi.fn(),
			token,
			serveConfig,
		);
		await fakeRegistry.buildStarted.promise;
		const second = runtime.handleServerlessRequest(
			registry,
			request,
			vi.fn(),
			token,
			serveConfig,
		);

		expect(fakeRegistry.serverlessBuilds).toBe(1);
		fakeRegistry.releaseServerlessBuild();

		await expect(Promise.all([first, second])).resolves.toEqual([
			{ status: 200, headers: { "x-request-count": "1" } },
			{ status: 200, headers: { "x-request-count": "2" } },
		]);
		expect(fakeRegistry.serverlessBuilds).toBe(1);
		expect(fakeRegistry.serverlessRequests).toBe(2);
	});

	test("drains a serverless runtime built during shutdown", async () => {
		const runtime = new WasmCoreRuntime(fakeWasmBindings());
		const registry = runtime.createRegistry();
		const fakeRegistry = registry as unknown as FakeCoreRegistry;
		fakeRegistry.blockNextServerlessBuild();
		const token = runtime.createCancellationToken();
		const request = {
			method: "POST",
			url: "https://api.rivet.dev/api/rivet/start",
			headers: {},
			body: new Uint8Array(),
		};

		const first = runtime.handleServerlessRequest(
			registry,
			request,
			vi.fn(),
			token,
			serveConfig,
		);
		await fakeRegistry.buildStarted.promise;
		await runtime.shutdownRegistry(registry);
		fakeRegistry.releaseServerlessBuild();

		await expect(first).rejects.toMatchObject({
			group: "wasm",
			code: "invalid_state",
			message: "Invalid wasm state 'core registry': shut down",
		});
		expect(fakeRegistry.serverlessShutdowns).toBe(1);
		expect(fakeRegistry.state).toBe("shutdown");
	});

	test("returns a structured wrong-mode error for serverless after serve", async () => {
		const runtime = new WasmCoreRuntime(fakeWasmBindings());
		const registry = runtime.createRegistry();
		const token = runtime.createCancellationToken();

		await runtime.serveRegistry(registry, serveConfig);

		await expect(
			runtime.handleServerlessRequest(
				registry,
				{
					method: "POST",
					url: "https://api.rivet.dev/api/rivet/start",
					headers: {},
					body: new Uint8Array(),
				},
				vi.fn(),
				token,
				serveConfig,
			),
		).rejects.toMatchObject({
			group: "wasm",
			code: "invalid_state",
			message:
				"Invalid wasm state 'core registry': mode conflict: another run mode is already active",
		});
	});

	test("loads configured bindings instead of hidden globals", async () => {
		const initInput = new Uint8Array([3, 2, 1]);
		const configuredDefault = vi.fn(async () => {});
		const hiddenDefault = vi.fn(async () => {});
		const configuredBindings = fakeWasmBindings(configuredDefault);
		const hiddenBindings = fakeWasmBindings(hiddenDefault);
		const globalScope = globalThis as typeof globalThis & {
			__rivetkitWasmBindings?: WasmBindings;
		};
		globalScope.__rivetkitWasmBindings = hiddenBindings;

		try {
			const loaded = await loadWasmRuntime({
				bindings: configuredBindings,
				initInput,
			});

			expect(loaded.bindings).toBe(configuredBindings);
			expect(configuredDefault).toHaveBeenCalledWith(initInput);
			expect(hiddenDefault).not.toHaveBeenCalled();
		} finally {
			delete globalScope.__rivetkitWasmBindings;
		}
	});
});
