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

async function insert(
	ctx: DispatchContext,
	runtimeUrl: string,
	runId: string,
	messageId: string,
	status: string,
	startedAtAgoMs: number | null,
) {
	const now = Date.now();
	await ctx.db.execute(
		`
		INSERT INTO dispatcher_queue (
			message_id, queue_name, route, runtime_url, body, headers, idem_key,
			status, attempt, next_at, created_at, updated_at, started_at
		)
		VALUES (?, ?, 'flow', ?, ?, NULL, NULL, ?, ?, ?, ?, ?, ?)
		`,
		messageId,
		`__wkf_workflow_${runId}`,
		runtimeUrl,
		JSON.stringify({ runId }),
		status,
		status === "inflight" ? 1 : 0,
		now,
		now,
		now,
		startedAtAgoMs == null ? null : now - startedAtAgoMs,
	);
}

function snapshot(ctx: DispatchContext) {
	return ctx.db.execute(
		"SELECT message_id, status, attempt FROM dispatcher_queue ORDER BY message_id",
	);
}

describe("crash/restart dispatch reconciliation", () => {
	let driver: DispatcherDriver | undefined;
	let app: MockApp | undefined;
	afterEach(async () => {
		driver?.close();
		if (app) await app.close();
		driver = undefined;
		app = undefined;
	});

	test("restart reconciles stranded ready and stale-inflight rows to done, each delivered", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_crash");
		const runId = `wrun_crash_${uid()}`;

		// Crash signature left in the durable queue:
		//  - msg_ready:  enqueued but the drain loop never started (crash before loop)
		//  - msg_stale:  claimed inflight, then the process died mid-delivery
		//  - msg_done2:  already completed before the crash (must NOT redeliver)
		await insert(driver.ctx, app.url, runId, "msg_ready", "ready", null);
		await insert(driver.ctx, app.url, runId, "msg_stale", "inflight", 60_000);
		await insert(driver.ctx, app.url, runId, "msg_done2", "done", 60_000);

		// Restart: fresh actor context (latch cleared) over the same db, then the
		// onWake/self-heal equivalent kicks the drain loop.
		const ctx = driver.restart();
		startDispatchLoop(ctx, runId);

		await waitFor(
			"ready + stale reconciled to done",
			async () => {
				const rows = await snapshot(ctx);
				const ready = rows.find((r) => r.message_id === "msg_ready");
				const stale = rows.find((r) => r.message_id === "msg_stale");
				return ready?.status === "done" && stale?.status === "done";
			},
			8_000,
		);

		const rows = await snapshot(ctx);
		// No row left stranded in ready/inflight.
		expect(rows.every((r) => r.status === "done")).toBe(true);
		// The already-done row was never redelivered.
		expect(app.count(runId)).toBe(2);
		expect(app.deliveries.some((d) => d.messageId === "msg_done2")).toBe(false);
	});

	test("restart schedules a wake for inflight work that is not stale yet", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_fresh");
		const runId = `wrun_fresh_${uid()}`;
		await insert(driver.ctx, app.url, runId, "msg_fresh", "inflight", 0);

		const ctx = driver.restart();
		startDispatchLoop(ctx, runId);
		expect((await snapshot(ctx))[0]?.status).toBe("inflight");

		await waitFor(
			"fresh inflight row to reach its stale deadline and redeliver",
			async () => (await snapshot(ctx))[0]?.status === "done",
			3_000,
		);
		expect(app.count(runId)).toBe(1);
	});

	test("repeated restarts mid-cycle never strand or duplicate a completed delivery", async () => {
		app = await startMockApp();
		driver = await makeDispatcherDriver("wrun_chaos");
		const runId = `wrun_chaos_${uid()}`;
		await insert(driver.ctx, app.url, runId, "msg_chaos", "ready", null);

		// Bombard with restarts at varied points; each restart clears the latch and
		// re-kicks the loop. Stale reclaim (>400ms) + the single-consumer latch must
		// converge to exactly one completed delivery with the row marked done.
		for (let i = 0; i < 5; i++) {
			const ctx = driver.restart();
			startDispatchLoop(ctx, runId);
			await new Promise((r) => setTimeout(r, 120));
		}
		const finalCtx = driver.restart();
		startDispatchLoop(finalCtx, runId);

		await waitFor(
			"converged to done",
			async () =>
				(await snapshot(finalCtx)).some((r) => r.status === "done"),
			8_000,
		);
		// At-least-once is the contract; with a fast healthy app it must converge to
		// exactly one successful delivery (no thrash, no duplicate completion).
		expect(app.count(runId)).toBe(1);
		const rows = await snapshot(finalCtx);
		expect(rows.filter((r) => r.status === "done")).toHaveLength(1);
	});
});
