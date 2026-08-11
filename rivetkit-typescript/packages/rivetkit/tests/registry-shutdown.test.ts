import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { actor } from "@/actor/mod";
import { type RegistryDeps, Registry } from "@/registry";
import type { RegistryConfigInput } from "@/registry/config";
import type {
	CoreRuntime,
	RegistryHandle,
	RuntimeServeConfig,
} from "@/registry/runtime";

const testActor = actor({
	state: {},
	actions: {},
});

interface Gate {
	promise: Promise<void>;
	release: () => void;
}

function makeGate(): Gate {
	let release!: () => void;
	const promise = new Promise<void>((resolve) => {
		release = resolve;
	});
	return { promise, release };
}

interface FakeState {
	/** Number of times the injected registry builder was invoked. */
	builderCalls: number;
	/** Registry handles passed to `shutdownRegistry`, in call order. */
	shutdownRegistries: RegistryHandle[];
	/** Grace periods passed to `shutdownRegistry`, in call order. */
	shutdownGracePeriods: number[];
	/** Value returned by `registryActorStopThresholdMs`. */
	stopThresholdMs: number | undefined;
	/** When true, `shutdownRegistry` never resolves (forces the grace race). */
	hangShutdown: boolean;
	/** When set, `shutdownRegistry` blocks on this gate before resolving. */
	gate: Gate | null;
}

interface Fake {
	deps: Partial<RegistryDeps>;
	state: FakeState;
}

/**
 * Builds an injectable registry builder backed by a fake `CoreRuntime`. The
 * fake records lifecycle calls so the suite can assert orchestration behavior
 * (fan-out, idempotency, grace ceiling) without an engine.
 */
function createFake(): Fake {
	const state: FakeState = {
		builderCalls: 0,
		shutdownRegistries: [],
		shutdownGracePeriods: [],
		stopThresholdMs: undefined,
		hangShutdown: false,
		gate: null,
	};

	const runtime = {
		kind: "napi",
		serveRegistry: async () => {},
		shutdownRegistry: async (
			registry: RegistryHandle,
			gracePeriodMs: number,
		) => {
			state.shutdownRegistries.push(registry);
			state.shutdownGracePeriods.push(gracePeriodMs);
			if (state.hangShutdown) {
				await new Promise<void>(() => {});
			}
			if (state.gate) {
				await state.gate.promise;
			}
		},
		registryActorStopThresholdMs: async () => state.stopThresholdMs,
		createCancellationToken: () => ({}),
		cancelCancellationToken: () => {},
		handleServerlessRequest: async (
			_registry: unknown,
			_req: unknown,
			onStreamEvent: (
				error: unknown,
				event?: { kind: string },
			) => unknown,
		) => {
			await onStreamEvent(null, { kind: "end" });
			return { status: 200, headers: {} };
		},
	} as unknown as CoreRuntime;

	const buildConfiguredRegistry = async () => {
		state.builderCalls += 1;
		// A distinct handle per build so fan-out can prove both modes were
		// torn down (Mode A and Mode B build separate registries).
		const registry = {
			id: state.builderCalls,
		} as unknown as RegistryHandle;
		const serveConfig = {
			serverlessBasePath: "/api/rivet",
			serverlessMaxStartPayloadBytes: 1024,
		} as unknown as RuntimeServeConfig;
		return { runtime, registry, serveConfig };
	};

	return {
		deps: {
			buildConfiguredRegistry:
				buildConfiguredRegistry as RegistryDeps["buildConfiguredRegistry"],
		},
		state,
	};
}

function makeRegistry(
	deps: Fake["deps"],
	overrides: Partial<RegistryConfigInput<{ test: typeof testActor }>> = {},
): Registry<{ test: typeof testActor }> {
	return new Registry(
		{
			use: { test: testActor },
			startEngine: false,
			noWelcome: true,
			...overrides,
		},
		deps,
	);
}

describe("Registry.shutdown", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
		vi.restoreAllMocks();
	});

	test("tears down both serverful and serverless modes", async () => {
		const { deps, state } = createFake();
		const registry = makeRegistry(deps);

		// Mode A (`start()`) and Mode B (`handler()`) build separate registries.
		registry.start();
		await registry.handler(
			new Request("http://localhost/api/rivet/x", { method: "GET" }),
		);

		await registry.shutdown();

		expect(state.shutdownRegistries).toHaveLength(2);
		expect(state.shutdownRegistries[0]).not.toBe(
			state.shutdownRegistries[1],
		);
	});

	test("concurrent and repeated calls share a single drain", async () => {
		const { deps, state } = createFake();
		const registry = makeRegistry(deps);
		registry.start();

		const first = registry.shutdown();
		const second = registry.shutdown();
		await Promise.all([first, second]);
		await registry.shutdown();

		// One Mode A registry, torn down exactly once across three calls.
		expect(state.shutdownRegistries).toHaveLength(1);
		expect(state.builderCalls).toBe(1);
	});

	test("is a no-op when nothing has started", async () => {
		const { deps, state } = createFake();
		const registry = makeRegistry(deps);

		await registry.shutdown();

		expect(state.builderCalls).toBe(0);
		expect(state.shutdownRegistries).toHaveLength(0);
	});

	test("does not exit the process", async () => {
		const killSpy = vi
			.spyOn(process, "kill")
			.mockImplementation(() => true);
		const exitSpy = vi
			.spyOn(process, "exit")
			.mockImplementation((() => undefined) as never);

		const { deps } = createFake();
		const registry = makeRegistry(deps);
		registry.start();

		await registry.shutdown();

		expect(killSpy).not.toHaveBeenCalled();
		expect(exitSpy).not.toHaveBeenCalled();
	});

	test("removes installed signal handlers", async () => {
		const beforeSigint = process.listeners("SIGINT").length;
		const beforeSigterm = process.listeners("SIGTERM").length;

		const { deps } = createFake();
		const registry = makeRegistry(deps);
		registry.start();

		expect(process.listeners("SIGINT").length).toBe(beforeSigint + 1);
		expect(process.listeners("SIGTERM").length).toBe(beforeSigterm + 1);

		await registry.shutdown();

		// Handlers gone, so a later signal cannot re-trigger a drain on the
		// already-torn-down registry.
		expect(process.listeners("SIGINT").length).toBe(beforeSigint);
		expect(process.listeners("SIGTERM").length).toBe(beforeSigterm);
	});

	test("waits for in-flight shutdown work before resolving", async () => {
		const { deps, state } = createFake();
		const gate = makeGate();
		state.gate = gate;

		const registry = makeRegistry(deps, {
			shutdown: { gracePeriodMs: 60_000 },
		});
		registry.start();

		let settled = false;
		const drained = registry.shutdown().then(() => {
			settled = true;
		});

		await vi.advanceTimersByTimeAsync(0);
		expect(state.shutdownRegistries).toHaveLength(1);
		expect(state.shutdownGracePeriods).toEqual([60_000]);
		expect(settled).toBe(false);

		gate.release();
		await vi.advanceTimersByTimeAsync(0);
		await drained;
		expect(settled).toBe(true);
	});

	test("resolves on the grace ceiling when a drain hangs", async () => {
		const { deps, state } = createFake();
		state.hangShutdown = true;

		const registry = makeRegistry(deps, {
			shutdown: { gracePeriodMs: 5_000 },
		});
		registry.start();

		let settled = false;
		const drained = registry.shutdown().then(() => {
			settled = true;
		});

		await vi.advanceTimersByTimeAsync(0);
		expect(state.shutdownRegistries).toHaveLength(1);
		expect(settled).toBe(false);

		await vi.advanceTimersByTimeAsync(5_000);
		await drained;
		expect(settled).toBe(true);
	});
});
