import { once } from "node:events";
import { createServer } from "node:net";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { afterEach, expect, test, vi } from "vitest";
import { configureNodeActorWorkerPool } from "@/registry/node-worker-pool";
import type {
	CoreRuntime,
	RuntimeWorkerRetireRequest,
	RuntimeWorkerSpawnRequest,
} from "@/registry/runtime";

const originalArgv = process.argv;
const originalExecArgv = process.execArgv;

afterEach(() => {
	process.argv = originalArgv;
	process.execArgv = originalExecArgv;
	vi.unstubAllEnvs();
	vi.useRealTimers();
});

async function startWorker() {
	// Workers inherit this preload and run the TypeScript fixture through tsx.
	process.execArgv = [...originalExecArgv, "--import", "tsx"];
	// Worker I/O remains real while the parent bootstrap deadline is controlled.
	vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
	const server = createServer();
	server.listen(0, "127.0.0.1");
	await once(server, "listening");
	const address = server.address();
	if (!address || typeof address === "string")
		throw new Error("missing port");
	vi.stubEnv("RIVETKIT_TEST_WORKER_PORT", String(address.port));
	process.argv = [
		process.execPath,
		fileURLToPath(
			new URL("./fixtures/worker-bootstrap.ts", import.meta.url),
		),
		"--config",
		"configuration with spaces.json",
		"--actors=2",
	];
	const expectedArgv = [...process.argv];
	let requestSpawns!: (requests: RuntimeWorkerSpawnRequest[]) => void;
	let retireWorker!: (request: RuntimeWorkerRetireRequest) => void;
	let resolveExited!: () => void;
	const exited = new Promise<void>((resolve) => {
		resolveExited = resolve;
	});
	const workerSpawnFailed = vi.fn(() => true);
	const workerExited = vi.fn(() => {
		resolveExited();
	});
	const runtime = {
		configureWorkerPool: (
			_registry: unknown,
			_capacity: number,
			_baseline: number,
			spawns: typeof requestSpawns,
			retire: typeof retireWorker,
		) => {
			requestSpawns = spawns;
			retireWorker = retire;
			return "pool";
		},
		workerSpawnFailed,
		workerExited,
	} as unknown as CoreRuntime;
	const pool = await configureNodeActorWorkerPool(runtime, {}, 1);
	const connection = once(server, "connection");
	requestSpawns([{ workerId: 1, spawnToken: "token", class: "baseline" }]);
	const [socket] = await connection;
	const lines = createInterface({ input: socket });
	const messages = lines[Symbol.asyncIterator]();
	const first = await messages.next();
	return {
		pool,
		argv: JSON.parse(first.value!),
		expectedArgv,
		workerSpawnFailed,
		workerExited,
		socket,
		messages,
		async acknowledge() {
			socket.write("ready\n");
			expect(await messages.next()).toMatchObject({
				value: "ready",
				done: false,
			});
		},
		async retire() {
			retireWorker({ workerId: 1, workerEpoch: 1 });
			expect(await messages.next()).toMatchObject({
				value: "retiring",
				done: false,
			});
		},
		async acknowledgeAndRetire() {
			await this.acknowledge();
			await this.retire();
			await exited;
		},
		async close() {
			const closed = pool.close();
			await vi.advanceTimersByTimeAsync(5_000);
			await closed;
			lines.close();
			socket.destroy();
			await new Promise<void>((resolve, reject) => {
				server.close((error) => (error ? reject(error) : resolve()));
			});
		},
	};
}

test("replayed entrypoint receives the application path and all CLI arguments", async () => {
	const worker = await startWorker();
	try {
		expect(worker.argv).toEqual(worker.expectedArgv);
		await worker.acknowledgeAndRetire();
	} finally {
		await worker.close();
	}
});

test("unrelated worker messages cannot crash the host or retire a live worker", async () => {
	vi.stubEnv("RIVETKIT_TEST_UNRELATED_MESSAGES", "1");
	const worker = await startWorker();
	try {
		await worker.acknowledgeAndRetire();
		expect(worker.workerSpawnFailed).not.toHaveBeenCalled();
	} finally {
		await worker.close();
	}
});

test("closing an empty pool leaves no retirement deadline behind", async () => {
	const worker = await startWorker();
	try {
		await worker.acknowledgeAndRetire();
		await worker.pool.close();
		expect(vi.getTimerCount()).toBe(0);
	} finally {
		await worker.close();
	}
});

test("a delayed ready acknowledgement cannot time out a natively registered worker", async () => {
	const worker = await startWorker();
	try {
		worker.workerSpawnFailed.mockReturnValue(false);
		await vi.advanceTimersByTimeAsync(60_000);
		expect(worker.workerSpawnFailed).toHaveBeenCalledWith(
			{},
			1,
			"token",
			expect.stringContaining("60000ms"),
		);
		await worker.acknowledgeAndRetire();
		expect(worker.workerExited).toHaveBeenCalledWith({}, 1, 1);
	} finally {
		await worker.close();
	}
});

test("a timed out pending spawn terminates when native cancellation succeeds", async () => {
	const worker = await startWorker();
	try {
		await vi.advanceTimersByTimeAsync(60_000);
		expect(await worker.messages.next()).toMatchObject({ done: true });
		expect(worker.workerSpawnFailed).toHaveBeenCalledOnce();
		expect(worker.workerExited).not.toHaveBeenCalled();
	} finally {
		await worker.close();
	}
});

test("forced close interrupts retirement and waits for the worker to exit", async () => {
	vi.stubEnv("RIVETKIT_TEST_BLOCK_RETIRE", "1");
	const worker = await startWorker();
	try {
		await worker.acknowledge();
		await worker.retire();
		const closing = worker.pool.close();
		expect(worker.workerExited).not.toHaveBeenCalled();
		await worker.pool.close(true);
		await closing;
		expect(worker.workerExited).toHaveBeenCalledOnce();
	} finally {
		await worker.close();
	}
});
