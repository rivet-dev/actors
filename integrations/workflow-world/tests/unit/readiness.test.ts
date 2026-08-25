import type { Registry } from "rivetkit";
import type { Client } from "rivetkit/client";
import { describe, expect, test, vi } from "vitest";
import { registry } from "../../src/actors.js";
import { RivetClientWorld } from "../../src/index.js";

function testWorld(startAndWait: () => Promise<void>) {
	const getRun = vi.fn(async (runId: string) => ({ runId }));
	const client = {
		workflowRun: {
			getOrCreate: () => ({ getRun }),
		},
		dispose: vi.fn(async () => {}),
	} as unknown as Client<typeof registry>;
	const applicationRegistry = {
		startAndWait,
		parseConfig: () => ({
			namespace: "test",
			envoy: { poolName: "test" },
		}),
	} as unknown as Registry<any> & { startAndWait(): Promise<void> };

	return {
		getRun,
		world: new RivetClientWorld({ client, registry: applicationRegistry }),
	};
}

describe("World registry readiness", () => {
	test("waits for readiness before the first actor operation", async () => {
		let resolveReady!: () => void;
		const ready = new Promise<void>((resolve) => {
			resolveReady = resolve;
		});
		const startAndWait = vi.fn(() => ready);
		const { getRun, world } = testWorld(startAndWait);

		const result = world.runs.get("run-1");
		await Promise.resolve();
		expect(startAndWait).toHaveBeenCalledOnce();
		expect(getRun).not.toHaveBeenCalled();

		resolveReady();
		await expect(result).resolves.toEqual({ runId: "run-1" });
		expect(getRun).toHaveBeenCalledOnce();
	});

	test("intentionally checks the shared readiness promise for every operation", async () => {
		const startAndWait = vi.fn(async () => {});
		const { getRun, world } = testWorld(startAndWait);

		await Promise.all([world.runs.get("run-1"), world.runs.get("run-2")]);

		expect(startAndWait).toHaveBeenCalledTimes(2);
		expect(getRun).toHaveBeenCalledTimes(2);
	});

	test("waits for readiness before opening a stream connection", async () => {
		let resolveReady!: () => void;
		const ready = new Promise<void>((resolve) => {
			resolveReady = resolve;
		});
		const startAndWait = vi.fn(() => ready);
		const connect = vi.fn(() => ({
			on: vi.fn(() => vi.fn()),
			onOpen: vi.fn(() => vi.fn()),
			dispose: vi.fn(async () => {}),
		}));
		const store = {
			connect,
			getStreamChunks: vi.fn(async () => ({
				data: [],
				done: true,
				hasMore: false,
			})),
		};
		const client = {
			workflowRun: { getOrCreate: () => store },
			dispose: vi.fn(async () => {}),
		} as unknown as Client<typeof registry>;
		const applicationRegistry = {
			startAndWait,
			parseConfig: () => ({
				namespace: "test",
				envoy: { poolName: "test" },
			}),
		} as unknown as Registry<any> & { startAndWait(): Promise<void> };
		const world = new RivetClientWorld({
			client,
			registry: applicationRegistry,
		});

		const streamPromise = world.readFromStream("run-1", "output");
		await Promise.resolve();
		expect(connect).not.toHaveBeenCalled();

		resolveReady();
		const stream = await streamPromise;
		await stream.getReader().read();
		expect(connect).toHaveBeenCalledOnce();
	});
});
