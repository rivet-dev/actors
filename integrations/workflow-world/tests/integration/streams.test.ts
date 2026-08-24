import { randomUUID } from "node:crypto";
import { setupTest } from "rivetkit/test";
import { afterEach, describe, expect, test } from "vitest";
import { registry } from "../../src/actors.ts";
import { RivetClientWorld } from "../../src/index.ts";

const uid = () => randomUUID().slice(0, 8);
const ENV = "RIVET_WORLD_RIVET_INJECT_BROADCAST_DROP_RATE";
const READ_DELAY = "RIVET_WORLD_RIVET_INJECT_STREAM_READ_DELAY_MS";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function readN(
	stream: ReadableStream<Uint8Array>,
	n: number,
	timeoutMs = 8_000,
) {
	const reader = stream.getReader();
	const out: string[] = [];
	const deadline = Date.now() + timeoutMs;
	while (out.length < n) {
		const remaining = deadline - Date.now();
		if (remaining <= 0) {
			void reader.cancel();
			throw new Error(`readN timed out after ${out.length}/${n} chunks`);
		}
		const result = await Promise.race([
			reader.read(),
			new Promise<{ timeout: true }>((r) =>
				setTimeout(() => r({ timeout: true }), remaining),
			),
		]);
		if ("timeout" in result) {
			void reader.cancel();
			throw new Error(`readN timed out after ${out.length}/${n} chunks`);
		}
		if (result.done) break;
		out.push(new TextDecoder().decode(result.value));
	}
	void reader.cancel();
	return out;
}

async function readAll(stream: ReadableStream<Uint8Array>, timeoutMs = 8_000) {
	const reader = stream.getReader();
	const out: string[] = [];
	const deadline = Date.now() + timeoutMs;
	for (;;) {
		const remaining = deadline - Date.now();
		if (remaining <= 0) throw new Error("readAll timed out");
		const result = await Promise.race([
			reader.read(),
			new Promise<{ timeout: true }>((r) =>
				setTimeout(() => r({ timeout: true }), remaining),
			),
		]);
		if ("timeout" in result) throw new Error("readAll timed out");
		if (result.done) return out;
		out.push(new TextDecoder().decode(result.value));
	}
}

describe("live stream broadcast-loss resilience", () => {
	let world: RivetClientWorld | undefined;
	afterEach(async () => {
		delete process.env[ENV];
		delete process.env[READ_DELAY];
		if (world) await world.close();
		world = undefined;
	});

	test("re-drains a chunk that lands mid-drain on a still-open stream (no signal lost)", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const name = `stream-race-${uid()}`;
		const runId = `wrun_stream_${uid()}`;

		// Hold each read open 150ms so a write can land *during* the in-flight
		// drain. Its broadcast arrives while draining; without the pending re-drain
		// guard the signal is coalesced away and the chunk is stranded (the stream
		// is never closed, so nothing else recovers it).
		process.env[READ_DELAY] = "150";
		await world.writeToStream(name, runId, "c0");
		const live = await world.readFromStream(runId, name, 0);
		await sleep(50); // initial drain is mid-flight (in the 150ms read window)
		await world.writeToStream(name, runId, "c1");

		expect(await readN(live, 2)).toEqual(["c0", "c1"]);
	});

	test("recovers every chunk via re-drain when ALL append broadcasts are dropped", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const name = `stream-dropped-${uid()}`;
		const runId = `wrun_stream_${uid()}`;

		// Drop 100% of append broadcasts; only the EOF (close) broadcast survives,
		// and its re-drain must catch up all chunks whose signals were lost.
		process.env[ENV] = "1";
		const live = await world.readFromStream(runId, name, 0);
		const expected = ["a", "b", "c", "d", "e"];
		for (const chunk of expected) {
			await world.writeToStream(name, runId, chunk);
		}
		await world.closeStream(name, runId);

		const received = await readAll(live);
		expect(received).toEqual(expected);
	});

	test("recovers all chunks under partial broadcast loss with no gaps or dupes", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const name = `stream-partial-${uid()}`;
		const runId = `wrun_stream_${uid()}`;

		process.env[ENV] = "0.5"; // drop every other append broadcast
		const live = await world.readFromStream(runId, name, 0);
		const expected = Array.from({ length: 8 }, (_, i) => `chunk-${i}`);
		for (const chunk of expected) {
			await world.writeToStream(name, runId, chunk);
		}
		await world.closeStream(name, runId);

		const received = await readAll(live);
		expect(received).toEqual(expected);
	});

	test("delivers chunks written before the reader connects (initial drain)", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const name = `stream-preexisting-${uid()}`;
		const runId = `wrun_stream_${uid()}`;

		await world.writeToStream(name, runId, "first");
		await world.writeToStream(name, runId, "second");
		const live = await world.readFromStream(runId, name, 0);
		await world.closeStream(name, runId);

		expect(await readAll(live)).toEqual(["first", "second"]);
	});

	test("catches up after the reader actor connection opens", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const name = `stream-opening-${uid()}`;
		const runId = `wrun_stream_${uid()}`;

		// Drop the append broadcast to model a write that lands while the actor
		// connection is still opening. The on-open drain must recover the chunk.
		process.env[ENV] = "1";
		const live = await world.readFromStream(runId, name, 0);
		await world.writeToStream(name, runId, "during-open");
		process.env[ENV] = "0";
		await world.closeStream(name, runId);

		expect(await readAll(live)).toEqual(["during-open"]);
	});

	test("isolates the same stream name between workflow runs", async (c) => {
		const { client } = await setupTest(c, registry);
		world = new RivetClientWorld({ client, runtimeUrl: "http://127.0.0.1:1" });
		const name = `shared-${uid()}`;
		const firstRun = `wrun_stream_${uid()}`;
		const secondRun = `wrun_stream_${uid()}`;
		await world.writeToStream(name, firstRun, "first");
		await world.closeStream(name, firstRun);
		await world.writeToStream(name, secondRun, "second");
		await world.closeStream(name, secondRun);

		expect(await readAll(await world.readFromStream(firstRun, name))).toEqual([
			"first",
		]);
		expect(await readAll(await world.readFromStream(secondRun, name))).toEqual([
			"second",
		]);
	});
});
