import { beforeEach, afterEach, describe, expect, test } from "vitest";
import {
	claimNextDispatch,
	failDispatch,
	markDispatchDone,
	nextDispatchAt,
	nextInflightRecoveryAt,
	reclaimStaleInflight,
	rescheduleDispatch,
	STALE_INFLIGHT_MS,
} from "../../src/actors/dispatcher.ts";
import { migrateWorkflowDb } from "../../src/actors/db.ts";
import { makeCtx, type TestCtx } from "../helpers/db.ts";

const BASE = {
	queue_name: "__wkf_workflow_probe",
	route: "flow",
	runtime_url: "http://127.0.0.1:1",
	body: JSON.stringify({ runId: "wrun_x" }),
};

async function insertRow(
	ctx: TestCtx,
	messageId: string,
	overrides: Partial<{
		status: string;
		attempt: number;
		next_at: number;
		created_at: number;
		started_at: number | null;
		idem_key: string | null;
	}> = {},
) {
	const now = Date.now();
	await ctx.db.execute(
		`
		INSERT INTO dispatcher_queue (
			message_id, queue_name, route, runtime_url, body, headers, idem_key,
			status, attempt, next_at, created_at, updated_at, started_at
		)
		VALUES (?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)
		`,
		messageId,
		BASE.queue_name,
		BASE.route,
		BASE.runtime_url,
		BASE.body,
		overrides.idem_key ?? null,
		overrides.status ?? "ready",
		overrides.attempt ?? 0,
		overrides.next_at ?? now,
		overrides.created_at ?? now,
		now,
		overrides.started_at ?? null,
	);
}

async function row(ctx: TestCtx, messageId: string) {
	return (
		await ctx.db.execute(
			"SELECT * FROM dispatcher_queue WHERE message_id = ?",
			messageId,
		)
	)[0];
}

describe("dispatcher queue SQL helpers", () => {
	let ctx: TestCtx;
	beforeEach(async () => {
		ctx = await makeCtx(migrateWorkflowDb);
	});
	afterEach(() => ctx.close());

	test("claimNextDispatch claims the oldest ready row, flips to inflight, bumps attempt", async () => {
		await insertRow(ctx, "msg_a", { created_at: 1000 });
		await insertRow(ctx, "msg_b", { created_at: 2000 });
		const claimed = await claimNextDispatch(ctx);
		expect(claimed?.message_id).toBe("msg_a");
		expect(claimed?.attempt).toBe(1);
		expect((await row(ctx, "msg_a")).status).toBe("inflight");
		expect((await row(ctx, "msg_b")).status).toBe("ready");
	});

	test("consecutive claims take distinct rows (single-consumer ordering)", async () => {
		await insertRow(ctx, "msg_a", { created_at: 1000 });
		await insertRow(ctx, "msg_b", { created_at: 2000 });
		const first = await claimNextDispatch(ctx);
		const second = await claimNextDispatch(ctx);
		expect(first?.message_id).toBe("msg_a");
		expect(second?.message_id).toBe("msg_b");
		expect(await claimNextDispatch(ctx)).toBeNull();
	});

	test("claimNextDispatch skips rows whose next_at is in the future", async () => {
		await insertRow(ctx, "msg_future", { next_at: Date.now() + 60_000 });
		expect(await claimNextDispatch(ctx)).toBeNull();
	});

	test("reclaimStaleInflight returns stale rows to ready without a terminal error/finished stamp", async () => {
		// started_at far in the past, attempt below the cap.
		await insertRow(ctx, "msg_stale", {
			status: "inflight",
			attempt: 1,
			started_at: Date.now() - 10 * 60_000,
		});
		await reclaimStaleInflight(ctx);
		const r = await row(ctx, "msg_stale");
		expect(r.status).toBe("ready");
		// Transient reclaim must NOT mark the row finished or stamp an error.
		expect(r.finished_at).toBeNull();
		expect(r.error).toBeNull();
	});

	test("reclaimStaleInflight dead-letters stale rows that hit the attempt cap", async () => {
		await insertRow(ctx, "msg_dead", {
			status: "inflight",
			attempt: 256,
			started_at: Date.now() - 10 * 60_000,
		});
		await reclaimStaleInflight(ctx);
		const r = await row(ctx, "msg_dead");
		expect(r.status).toBe("failed");
		expect(String(r.error)).toContain("max attempts");
	});

	test("reclaimStaleInflight leaves fresh inflight rows untouched", async () => {
		await insertRow(ctx, "msg_fresh", {
			status: "inflight",
			attempt: 1,
			started_at: Date.now(),
		});
		await reclaimStaleInflight(ctx);
		expect((await row(ctx, "msg_fresh")).status).toBe("inflight");
	});

	test("reclaimStaleInflight leaves locally active requests untouched", async () => {
		await insertRow(ctx, "msg_active", {
			status: "inflight",
			attempt: 1,
			started_at: Date.now() - 10 * 60_000,
		});
		await reclaimStaleInflight(ctx, new Set(["msg_active"]));
		expect((await row(ctx, "msg_active")).status).toBe("inflight");
	});

	test("markDispatchDone / rescheduleDispatch / failDispatch transitions", async () => {
		await insertRow(ctx, "msg_done", { status: "inflight" });
		await markDispatchDone(ctx, "msg_done", 0, 200);
		expect((await row(ctx, "msg_done")).status).toBe("done");
		expect(Number((await row(ctx, "msg_done")).http_status)).toBe(200);

		await insertRow(ctx, "msg_retry", { status: "inflight" });
		await rescheduleDispatch(ctx, "msg_retry", 0, 5_000, 503, "HTTP 503");
		const retry = await row(ctx, "msg_retry");
		expect(retry.status).toBe("ready");
		expect(Number(retry.next_at)).toBeGreaterThan(Date.now());

		await insertRow(ctx, "msg_fail", { status: "inflight" });
		await failDispatch(ctx, "msg_fail", 0, 503, "dead letter");
		expect((await row(ctx, "msg_fail")).status).toBe("failed");
	});

	test("a stale attempt cannot overwrite a newer claim", async () => {
		await insertRow(ctx, "msg_raced", { status: "inflight", attempt: 2 });
		await rescheduleDispatch(ctx, "msg_raced", 1, 0, 500, "old attempt");
		const raced = await row(ctx, "msg_raced");
		expect(raced.status).toBe("inflight");
		expect(raced.error).toBeNull();
	});

	test("nextDispatchAt returns the earliest ready next_at", async () => {
		await insertRow(ctx, "msg_late", { next_at: 9000 });
		await insertRow(ctx, "msg_early", { next_at: 3000 });
		expect(await nextDispatchAt(ctx)).toBe(3000);
	});

	test("nextInflightRecoveryAt returns the first stale deadline", async () => {
		const now = Date.now();
		await insertRow(ctx, "msg_late", {
			status: "inflight",
			started_at: now - 100,
		});
		await insertRow(ctx, "msg_early", {
			status: "inflight",
			started_at: now - 300,
		});
		const recoveryAt = await nextInflightRecoveryAt(ctx);
		expect(recoveryAt).not.toBeNull();
		expect(recoveryAt!).toBeGreaterThanOrEqual(now);
		expect(recoveryAt!).toBeLessThanOrEqual(now + 150);
	});

	test("nextInflightRecoveryAt re-arms locally active stale rows in the future", async () => {
		await insertRow(ctx, "msg_active", {
			status: "inflight",
			started_at: Date.now() - 10 * 60_000,
		});
		const before = Date.now();
		const recoveryAt = await nextInflightRecoveryAt(
			ctx,
			new Set(["msg_active"]),
		);
		expect(recoveryAt).toBeGreaterThanOrEqual(before + STALE_INFLIGHT_MS);
		expect(recoveryAt).toBeLessThanOrEqual(Date.now() + STALE_INFLIGHT_MS);
	});

	test("re-delivered enqueue with the same idem_key returns the ORIGINAL message id (deterministic)", async () => {
		// Mirrors dispatcher.enqueue: INSERT OR IGNORE then the dedup SELECT. A
		// re-delivery generates a fresh messageId (msg_B) but must resolve back to
		// the originally-stored messageId (msg_A) for the same idem_key, so the
		// caller never sees a new id on redelivery (matches world-postgres jobKey).
		await insertRow(ctx, "msg_A", { idem_key: "stable" });
		const dedupSelect = async (incomingMessageId: string, idemKey: string) => {
			await ctx.db.execute(
				`
				INSERT OR IGNORE INTO dispatcher_queue (
					message_id, queue_name, route, runtime_url, body, headers, idem_key,
					status, attempt, next_at, created_at, updated_at
				)
				VALUES (?, ?, ?, ?, ?, NULL, ?, 'ready', 0, ?, ?, ?)
				`,
				incomingMessageId,
				BASE.queue_name,
				BASE.route,
				BASE.runtime_url,
				BASE.body,
				idemKey,
				Date.now(),
				Date.now(),
				Date.now(),
			);
			const stored = await ctx.db.execute(
				`
				SELECT message_id FROM dispatcher_queue
				WHERE message_id = ? OR (? IS NOT NULL AND idem_key = ?)
				LIMIT 1
				`,
				incomingMessageId,
				idemKey,
				idemKey,
			);
			return String(stored[0]?.message_id ?? incomingMessageId);
		};
		expect(await dedupSelect("msg_B", "stable")).toBe("msg_A");
		expect(await dedupSelect("msg_C", "stable")).toBe("msg_A");
		// A genuinely new idem_key is NOT deduped (gets its own id).
		expect(await dedupSelect("msg_D", "other")).toBe("msg_D");
	});

	test("idempotency unique index collapses duplicate idem_key enqueues", async () => {
		await insertRow(ctx, "msg_1", { idem_key: "dup" });
		await ctx.db.execute(
			`
			INSERT OR IGNORE INTO dispatcher_queue (
				message_id, queue_name, route, runtime_url, body, headers, idem_key,
				status, attempt, next_at, created_at, updated_at
			)
			VALUES ('msg_2', ?, ?, ?, ?, NULL, 'dup', 'ready', 0, ?, ?, ?)
			`,
			BASE.queue_name,
			BASE.route,
			BASE.runtime_url,
			BASE.body,
			Date.now(),
			Date.now(),
			Date.now(),
		);
		const rows = await ctx.db.execute(
			"SELECT message_id FROM dispatcher_queue WHERE idem_key = 'dup'",
		);
		expect(rows).toHaveLength(1);
		expect(String(rows[0].message_id)).toBe("msg_1");
	});
});
