import { Worker } from "node:worker_threads";
import { CoreRegistry } from "../../rivetkit-napi/index.js";
import { expect, test } from "vitest";

test("NAPI worker environments resolve the main environment's pool", async () => {
	const registry = new CoreRegistry();
	const poolId = registry.configureWorkerPool(
		1,
		1,
		() => {},
		() => {},
	);
	const addonUrl = new URL("../../rivetkit-napi/index.js", import.meta.url)
		.href;
	const source = `
import { parentPort, workerData } from "node:worker_threads";
const imported = await import(workerData.addonUrl);
const binding = imported.default ?? imported;
const registry = new binding.CoreRegistry();
try {
	registry.attachWorker(workerData.poolId, 1, "not-pending", "baseline");
	parentPort.postMessage({ unexpectedSuccess: true });
} catch (error) {
	parentPort.postMessage({ error: String(error) });
}
`;
	const worker = new Worker(
		new URL(`data:text/javascript,${encodeURIComponent(source)}`),
		{ workerData: { addonUrl, poolId } },
	);

	try {
		const message = await new Promise<{ error?: string }>(
			(resolve, reject) => {
				worker.once("message", resolve);
				worker.once("error", reject);
			},
		);
		expect(message.error).toMatch(/has no pending spawn/);
		expect(message.error).not.toMatch(/pool is missing/);
	} finally {
		await worker.terminate();
		await registry.shutdown();
	}
}, 10_000);
