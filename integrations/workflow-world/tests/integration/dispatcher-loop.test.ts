import { randomUUID } from "node:crypto";
import { afterEach, describe, expect, test } from "vitest";
import {
	startDispatchLoop,
	type DispatchContext,
} from "../../src/actors/dispatcher.ts";
import {
	makeDispatcherDriver,
	type DispatcherDriver,
} from "../helpers/dispatcher-driver.ts";
import { startMockApp, waitFor, type MockApp } from "../helpers/harness.ts";

const uid = () => randomUUID().slice(0, 8);

async function enqueue(
	ctx: DispatchContext,
	runtimeUrl: string,
	runId: string,
	messageId: string,
	overrides: Partial<{ next_at: number; created_at: number }> = {},
) {
	const now = Date.now();
	await ctx.db.execute(
		`
		INSERT INTO dispatcher_queue (
			message_id, queue_name, route, runtime_url, body, headers, idem_key,
			status, attempt, next_at, created_at, updated_at
		)
		VALUES (?, ?, 'flow', ?, ?, NULL, NULL, 'ready', 0, ?, ?, ?)
		`,
		messageId,
		`__wkf_workflow_${runId}`,
		runtimeUrl,
		JSON.stringify({ runId }),
		overrides.next_at ?? now,
		overrides.created_at ?? now,
		now,
	);
}

function rows(ctx: DispatchContext) {
	return ctx.db.execute(
		"SELECT message_id, status, attempt FROM dispatcher_queue ORDER BY created_at ASC",
	);
}

describe("dispatcher drain loop (deterministic driver)", () => {
	let driver: DispatcherDriver | undefined;
	let app: MockApp | undefined;
	afterEach(async () => {
		driver?.close();
		if (app) await app.close();
		driver = undefined;
		app = undefined;
	});

	test("single drain loop claims each message once while deliveries overlap", async () => {
		app = await startMockApp();
		app.responseDelayMs = 60; // widen the window so parallel loops would overlap
		driver = await makeDispatcherDriver("wrun_latch");
		const runId = `wrun_latch_${uid()}`;
		await enqueue(driver.ctx, app.url, runId, "msg_a", { created_at: 1 });
		await enqueue(driver.ctx, app.url, runId, "msg_b", { created_at: 2 });

		// Two concurrent start signals must still produce one claim loop and one
		// delivery per message. HTTP deliveries intentionally overlap because a
		// workflow turn can wait for a continuation enqueued behind it.
		startDispatchLoop(driver.ctx, runId);
		startDispatchLoop(driver.ctx, runId);

		await waitFor("both delivered", () => app!.count(runId) === 2, 8_000);
		expect(app.maxConcurrent).toBe(2);
	});

	test("delivery failures retry with backoff then succeed", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_retry");
		const runId = `wrun_retry_${uid()}`;
		app.failuresBeforeSuccess.set(runId, 2);
		await enqueue(driver.ctx, app.url, runId, "msg_retry");
		startDispatchLoop(driver.ctx, runId);
		await waitFor("3 attempts", () => app!.count(runId) === 3, 8_000);
		await waitFor(
			"row done on attempt 3",
			async () =>
				(await rows(driver!.ctx)).some(
					(r) => r.status === "done" && Number(r.attempt) === 3,
				),
			2_000,
		);
	});

	test("permanently failing delivery dead-letters at the attempt cap", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_dead");
		const runId = `wrun_dead_${uid()}`;
		app.alwaysFail.add(runId);
		await enqueue(driver.ctx, app.url, runId, "msg_dead");
		startDispatchLoop(driver.ctx, runId);
		await waitFor(
			"failed at cap",
			async () =>
				(await rows(driver!.ctx)).some(
					(r) => r.status === "failed" && Number(r.attempt) === 3,
				),
			8_000,
		);
		expect(app.count(runId)).toBe(3);
	});

	test("long-step {timeoutSeconds} response defers redelivery instead of acking", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_timeout");
		const runId = `wrun_timeout_${uid()}`;
		app.timeoutResponse.set(runId, 0); // first response carries timeoutSeconds
		await enqueue(driver.ctx, app.url, runId, "msg_timeout");
		startDispatchLoop(driver.ctx, runId);
		// timeoutSeconds reschedules (not done); redelivered, then acks normally.
		await waitFor("redelivered", () => app!.count(runId) === 2, 8_000);
		await waitFor(
			"eventually done",
			async () => (await rows(driver!.ctx)).some((r) => r.status === "done"),
			2_000,
		);
	});

	test("malformed timeoutSeconds is retried, not silently acked", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_malformed");
		const runId = `wrun_malformed_${uid()}`;
		app.malformedSuccess.add(runId);
		await enqueue(driver.ctx, app.url, runId, "msg_malformed");
		startDispatchLoop(driver.ctx, runId);
		await waitFor(
			"retried to cap then failed",
			async () =>
				(await rows(driver!.ctx)).some(
					(r) => r.status === "failed" && Number(r.attempt) === 3,
				),
			8_000,
		);
		expect(app.count(runId)).toBe(3);
	});
});
