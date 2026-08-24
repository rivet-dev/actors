import { afterEach, describe, expect, test } from "vitest";
import {
	migrateCoordinatorDb,
	migrateHookTokenDb,
	migrateWorkflowDb,
} from "../../src/actors/db.ts";
import { makeCtx, type TestCtx } from "../helpers/db.ts";

const contexts: TestCtx[] = [];
afterEach(() => {
	for (const ctx of contexts.splice(0)) ctx.close();
});

function columns(ctx: TestCtx, table: string): string[] {
	return ctx.raw
		.prepare(`PRAGMA table_info(${table})`)
		.all()
		.map((row) => String(row.name));
}

describe("Workflow World database migrations", () => {
	test("creates the consolidated workflowRun schema at v1", async () => {
		const ctx = await makeCtx(migrateWorkflowDb);
		contexts.push(ctx);
		expect(columns(ctx, "workflow_runs")).toEqual(
			expect.arrayContaining(["run_revision", "attributes"]),
		);
		expect(columns(ctx, "workflow_waits")).toEqual(
			expect.arrayContaining([
				"runtime_url",
				"queue_name",
				"headers",
				"resume_enqueued_at",
			]),
		);
		expect(columns(ctx, "dispatcher_queue")).toContain("idem_key");
		expect(columns(ctx, "workflow_hooks")).toContain("token_generation");
		expect(columns(ctx, "stream_chunks")).toContain("sequence");
		expect(columns(ctx, "pending_ops")).toEqual(
			expect.arrayContaining(["operation_revision", "claim_id", "next_at"]),
		);
	});

	test("creates the derived coordinator schema at v1", async () => {
		const ctx = await makeCtx(migrateCoordinatorDb);
		contexts.push(ctx);
		expect(columns(ctx, "runs_index")).toEqual(
			expect.arrayContaining(["run_revision", "attributes"]),
		);
		expect(columns(ctx, "correlation_index")).toEqual(
			expect.arrayContaining(["kind", "run_revision", "status"]),
		);
		expect(columns(ctx, "hooks_index")).toEqual(
			expect.arrayContaining(["run_revision", "token", "status"]),
		);
	});

	test("creates generation-fenced hook ownership at v1", async () => {
		const ctx = await makeCtx(migrateHookTokenDb);
		contexts.push(ctx);
		expect(columns(ctx, "hook_token_state")).toEqual(
			expect.arrayContaining(["generation", "status", "expires_at"]),
		);
	});
});
