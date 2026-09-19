import { connect } from "node:net";
import { createInterface } from "node:readline";
import { parentPort } from "node:worker_threads";
import type { ActorWorkerBootstrapState } from "../../src/registry/node-worker-pool";
import type { CoreRuntime } from "../../src/registry/runtime";

const bootstrap = (globalThis as Record<symbol, ActorWorkerBootstrapState>)[
	Symbol.for("rivetkit.actorWorkerThread.bootstrap")
];
const port = parentPort;
if (!port || !bootstrap) throw new Error("missing actor worker bootstrap");
bootstrap.claimed = true;
bootstrap.registry = {};
bootstrap.runtime = {
	detachWorker() {
		socket.write("retiring\n");
		if (process.env.RIVETKIT_TEST_BLOCK_RETIRE === "1") {
			Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0);
		}
		socket.end();
	},
} as CoreRuntime;
bootstrap.registration = { workerId: bootstrap.workerId, workerEpoch: 1 };
if (process.env.RIVETKIT_TEST_UNRELATED_MESSAGES === "1") {
	port.postMessage(null);
	port.postMessage({ message: "application data" });
}

// The test gates the acknowledgement independently of native registration.
const socket = connect(
	Number(process.env.RIVETKIT_TEST_WORKER_PORT),
	"127.0.0.1",
);
const lines = createInterface({ input: socket });
bootstrap.attachPromise = (async () => {
	for await (const line of lines) {
		if (line !== "ready") continue;
		port.postMessage({ kind: "ready", ...bootstrap.registration });
		if (process.env.RIVETKIT_TEST_UNRELATED_MESSAGES === "1") {
			port.postMessage({
				kind: "application",
				...bootstrap.registration,
			});
		}
		socket.write("ready\n");
		break;
	}
})();
socket.write(`${JSON.stringify(process.argv)}\n`);
await bootstrap.attachPromise;
