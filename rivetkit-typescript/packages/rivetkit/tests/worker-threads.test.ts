import { availableParallelism } from "node:os";
import { afterEach, describe, expect, test, vi } from "vitest";
import { actor } from "@/actor/mod";
import { RegistryConfigSchema } from "@/registry/config";
import { buildRegistryWithRuntime } from "@/registry/native";
import {
	claimActorWorkerBootstrap,
	setActorWorkerAttachPromise,
} from "@/registry/node-worker-pool";
import type { CoreRuntime } from "@/registry/runtime";

const bootstrapSymbol = Symbol.for("rivetkit.actorWorkerThread.bootstrap");

afterEach(() => {
	delete (
		globalThis as typeof globalThis & {
			[bootstrapSymbol]?: unknown;
		}
	)[bootstrapSymbol];
});

describe("Node actor worker threads", () => {
	test("validates actorsPerThread as a positive safe integer", () => {
		const input = { use: {}, startEngine: false };
		expect(
			RegistryConfigSchema.parse({ ...input, actorsPerThread: 2 })
				.actorsPerThread,
		).toBe(2);
		for (const actorsPerThread of [
			0,
			-1,
			1.5,
			Number.MAX_SAFE_INTEGER + 1,
		]) {
			expect(
				RegistryConfigSchema.safeParse({ ...input, actorsPerThread })
					.success,
			).toBe(false);
		}
	});

	test("registers metadata on the main registry and configures the hybrid pool", async () => {
		const registerActor = vi.fn();
		const registerActorConfig = vi.fn();
		const configureWorkerPool = vi.fn(() => "pool-1");
		const runtime = {
			kind: "napi",
			createRegistry: () => ({ registry: true }),
			registerActor,
			registerActorConfig,
			configureWorkerPool,
			workerSpawnFailed: vi.fn(),
			workerExited: vi.fn(),
		} as unknown as CoreRuntime;
		const definition = actor({ state: {}, actions: {} });
		const config = RegistryConfigSchema.parse({
			use: { counter: definition },
			startEngine: false,
			actorsPerThread: 3,
		});

		const result = await buildRegistryWithRuntime(config, runtime);

		expect(registerActor).not.toHaveBeenCalled();
		expect(registerActorConfig).toHaveBeenCalledOnce();
		expect(registerActorConfig).toHaveBeenCalledWith(
			result.registry,
			"counter",
			expect.objectContaining({ actions: [] }),
		);
		expect(configureWorkerPool).toHaveBeenCalledWith(
			result.registry,
			3,
			availableParallelism(),
			expect.any(Function),
			expect.any(Function),
		);
		expect(result.workerPoolId).toBe("pool-1");
	});

	test("worker bootstrap can only be claimed once", () => {
		const state = {
			poolId: "pool-1",
			workerId: 2,
			spawnToken: "token",
			class: "baseline" as const,
			entrypoint: "file:///app.js",
			claimed: false,
		};
		(
			globalThis as typeof globalThis & {
				[bootstrapSymbol]?: typeof state;
			}
		)[bootstrapSymbol] = state;

		const claimed = claimActorWorkerBootstrap();
		expect(claimed).toBe(state);
		const attached = Promise.resolve();
		setActorWorkerAttachPromise(claimed!, attached);
		expect(state).toMatchObject({ claimed: true, attachPromise: attached });
		expect(() => claimActorWorkerBootstrap()).toThrow(/more than one/);
	});
});
