import { SPEC_VERSION_CURRENT, type WorkflowRun } from "@workflow/world";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import {
	applyCoordinatorUpdate,
	coordinator,
	type CoordinatorContext,
} from "../../src/actors/coordinator.ts";
import { migrateCoordinatorDb } from "../../src/actors/db.ts";
import { makeCtx, type TestCtx } from "../helpers/db.ts";

type Ctx = CoordinatorContext & { raw: TestCtx["raw"]; close: () => void };
type ActorCtx = CoordinatorContext & { key: readonly unknown[] };

const actions = coordinator.config.actions as unknown as {
	listRuns: (c: ActorCtx, params?: Record<string, unknown>) => Promise<{
		data: WorkflowRun[];
		cursor: string | null;
		hasMore: boolean;
	}>;
	listHookIndex: (
		c: ActorCtx,
		params?: Record<string, unknown>,
	) => Promise<{
		data: { hookId: string; runId: string }[];
		cursor: string | null;
		hasMore: boolean;
	}>;
};

async function makeCoordCtx(): Promise<Ctx> {
	const base = await makeCtx(migrateCoordinatorDb);
	return { db: base.db, raw: base.raw, close: base.close };
}

function run(
	runId: string,
	status: WorkflowRun["status"] = "running",
	createdAt = new Date("2026-01-01T00:00:00.000Z"),
): WorkflowRun {
	return {
		runId,
		status,
		deploymentId: "rivet",
		workflowName: "wf",
		specVersion: SPEC_VERSION_CURRENT,
		createdAt,
		updatedAt: createdAt,
	} as WorkflowRun;
}

describe("coordinator derived indexes", () => {
	let ctx: Ctx;
	beforeEach(async () => {
		ctx = await makeCoordCtx();
	});
	afterEach(() => ctx.close());

	test("run upserts cannot regress to an older revision", async () => {
		await applyCoordinatorUpdate(ctx, {
			type: "run.upsert",
			run: run("wrun_1", "completed"),
			runRevision: 2,
		});
		await applyCoordinatorUpdate(ctx, {
			type: "run.upsert",
			run: run("wrun_1", "running"),
			runRevision: 1,
		});

		const row = ctx.raw
			.prepare("SELECT run_revision, status FROM runs_index")
			.get();
		expect(row).toEqual({ run_revision: 2, status: "completed" });
	});

	test("correlation tombstones fence delayed puts and retain ownership", async () => {
		await applyCoordinatorUpdate(ctx, {
			type: "correlation.delete",
			correlationId: "hook_1",
			runId: "wrun_1",
			kind: "hook",
			runRevision: 3,
		});
		await applyCoordinatorUpdate(ctx, {
			type: "correlation.put",
			correlationId: "hook_1",
			runId: "wrun_1",
			kind: "hook",
			runRevision: 3,
		});
		expect(
			ctx.raw
				.prepare(
					"SELECT run_revision, status FROM correlation_index WHERE correlation_id = ?",
				)
				.get("hook_1"),
		).toEqual({ run_revision: 3, status: "deleted" });

		await expect(
			applyCoordinatorUpdate(ctx, {
				type: "correlation.put",
				correlationId: "hook_1",
				runId: "wrun_2",
				kind: "hook",
				runRevision: 4,
			}),
		).rejects.toThrow("another workflow entity");
	});

	test("hook tombstones fence delayed upserts", async () => {
		const common = {
			hookId: "hook_1",
			runId: "wrun_1",
			token: "token_1",
			createdAt: 10,
			updatedAt: 20,
		};
		await applyCoordinatorUpdate(ctx, {
			...common,
			type: "hook.delete",
			runRevision: 5,
		});
		await applyCoordinatorUpdate(ctx, {
			...common,
			type: "hook.upsert",
			runRevision: 6,
		});
		expect(
			ctx.raw
				.prepare("SELECT run_revision, status FROM hooks_index")
				.get(),
		).toEqual({ run_revision: 5, status: "deleted" });
	});

	test("run and hook listings use stable tuple cursors and hide tombstones", async () => {
		const createdAt = new Date("2026-01-01T00:00:00.000Z");
		for (const runId of ["wrun_1", "wrun_2", "wrun_3"]) {
			await applyCoordinatorUpdate(ctx, {
				type: "run.upsert",
				run: run(runId, "running", createdAt),
				runRevision: 1,
			});
		}
		const actorCtx = { db: ctx.db, key: ["coordinator"] };
		const firstRuns = await actions.listRuns(actorCtx, {
			pagination: { limit: 2 },
		});
		expect(firstRuns.data.map((value) => value.runId)).toEqual([
			"wrun_3",
			"wrun_2",
		]);
		expect(firstRuns.hasMore).toBe(true);
		const secondRuns = await actions.listRuns(actorCtx, {
			pagination: { limit: 2, cursor: firstRuns.cursor },
		});
		expect(secondRuns.data.map((value) => value.runId)).toEqual(["wrun_1"]);

		for (const hookId of ["hook_1", "hook_2", "hook_3"]) {
			await applyCoordinatorUpdate(ctx, {
				type: hookId === "hook_2" ? "hook.delete" : "hook.upsert",
				hookId,
				runId: `run_${hookId}`,
				token: `token_${hookId}`,
				runRevision: 1,
				createdAt: 10,
				updatedAt: 10,
			});
		}
		const hooks = await actions.listHookIndex(actorCtx, {
			pagination: { limit: 10 },
		});
		expect(hooks.data.map((value) => value.hookId)).toEqual([
			"hook_3",
			"hook_1",
		]);
	});
});
