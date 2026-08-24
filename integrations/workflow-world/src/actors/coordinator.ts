import {
	type ListHooksParams,
	type ListWorkflowRunsParams,
	type WorkflowRun,
} from "@workflow/world";
import { actor } from "rivetkit";
import { encodeValue } from "../codec.js";
import { coordinatorDb } from "./db.js";
import {
	filterData,
	one,
	rowToRun,
	toMs,
	withActorTransaction,
	type Ctx,
} from "./shared.js";

export type CoordinatorContext = Ctx;

type CorrelationKind = "step" | "hook" | "wait";
type Timestamp = Date | number | string;

export type CoordinatorIndexUpdate =
	| { type: "run.upsert"; run: WorkflowRun; runRevision: number }
	| {
			type: "correlation.put" | "correlation.delete";
			correlationId: string;
			runId: string;
			kind: CorrelationKind;
			runRevision: number;
	  }
	| {
			type: "hook.upsert" | "hook.delete";
			hookId: string;
			runId: string;
			token: string;
			runRevision: number;
			createdAt: Timestamp;
			updatedAt: Timestamp;
	  };

function assertCoordinatorKey(c: { key: readonly unknown[] }) {
	if (String(c.key[0] ?? "") !== "coordinator" || c.key.length !== 1) {
		throw new Error('coordinator actor key must be ["coordinator"]');
	}
}

function assertRevision(revision: number) {
	if (!Number.isSafeInteger(revision) || revision < 0) {
		throw new Error("coordinator update requires a non-negative run revision");
	}
}

function timestamp(value: Timestamp): number {
	const result = toMs(value);
	if (result == null || !Number.isFinite(result)) {
		throw new Error("coordinator update requires a valid timestamp");
	}
	return result;
}

async function upsertRun(c: Ctx, run: WorkflowRun, runRevision: number) {
	await c.db.execute(
		`
		INSERT INTO runs_index (
			run_id, run_revision, status, deployment_id, workflow_name,
			spec_version, execution_context, attributes, input, output, error,
			expired_at, started_at, completed_at, created_at, updated_at
		)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(run_id) DO UPDATE SET
			run_revision = excluded.run_revision,
			status = excluded.status,
			deployment_id = excluded.deployment_id,
			workflow_name = excluded.workflow_name,
			spec_version = excluded.spec_version,
			execution_context = excluded.execution_context,
			attributes = excluded.attributes,
			input = excluded.input,
			output = excluded.output,
			error = excluded.error,
			expired_at = excluded.expired_at,
			started_at = excluded.started_at,
			completed_at = excluded.completed_at,
			created_at = excluded.created_at,
			updated_at = excluded.updated_at
		WHERE excluded.run_revision >= runs_index.run_revision
		`,
		run.runId,
		runRevision,
		run.status,
		run.deploymentId,
		run.workflowName,
		run.specVersion ?? null,
		encodeValue(run.executionContext),
		encodeValue(run.attributes),
		encodeValue(run.input),
		encodeValue(run.output),
		encodeValue(run.error),
		toMs(run.expiredAt),
		toMs(run.startedAt),
		toMs(run.completedAt),
		toMs(run.createdAt),
		toMs(run.updatedAt),
	);
}

async function updateCorrelation(
	c: Ctx,
	input: Extract<
		CoordinatorIndexUpdate,
		{ type: "correlation.put" | "correlation.delete" }
	>,
) {
	const existing = await one(
		c,
		"SELECT run_id, kind FROM correlation_index WHERE correlation_id = ?",
		input.correlationId,
	);
	if (
		existing &&
		(String(existing.run_id) !== input.runId ||
			String(existing.kind) !== input.kind)
	) {
		throw new Error(
			`correlation "${input.correlationId}" is already owned by another workflow entity`,
		);
	}

	const status = input.type === "correlation.put" ? "active" : "deleted";
	await c.db.execute(
		`
		INSERT INTO correlation_index (
			correlation_id, run_id, kind, run_revision, status
		)
		VALUES (?, ?, ?, ?, ?)
		ON CONFLICT(correlation_id) DO UPDATE SET
			run_revision = excluded.run_revision,
			status = excluded.status
		WHERE
			NOT (
				correlation_index.status = 'deleted'
				AND excluded.status = 'active'
			)
			AND (
				excluded.run_revision > correlation_index.run_revision
				OR (
					excluded.run_revision = correlation_index.run_revision
					AND (
						excluded.status = 'deleted'
						OR correlation_index.status = excluded.status
					)
				)
			)
		`,
		input.correlationId,
		input.runId,
		input.kind,
		input.runRevision,
		status,
	);
}

async function updateHook(
	c: Ctx,
	input: Extract<
		CoordinatorIndexUpdate,
		{ type: "hook.upsert" | "hook.delete" }
	>,
) {
	const existing = await one(
		c,
		"SELECT run_id FROM hooks_index WHERE hook_id = ?",
		input.hookId,
	);
	if (existing && String(existing.run_id) !== input.runId) {
		throw new Error(`hook "${input.hookId}" is already owned by another workflow run`);
	}

	const status = input.type === "hook.upsert" ? "active" : "deleted";
	await c.db.execute(
		`
		INSERT INTO hooks_index (
			hook_id, run_id, run_revision, token, status, created_at, updated_at
		)
		VALUES (?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(hook_id) DO UPDATE SET
			run_revision = excluded.run_revision,
			token = excluded.token,
			status = excluded.status,
			created_at = excluded.created_at,
			updated_at = excluded.updated_at
		WHERE
			NOT (hooks_index.status = 'deleted' AND excluded.status = 'active')
			AND (
				excluded.run_revision > hooks_index.run_revision
				OR (
					excluded.run_revision = hooks_index.run_revision
					AND (
						excluded.status = 'deleted'
						OR hooks_index.status = excluded.status
					)
				)
			)
		`,
		input.hookId,
		input.runId,
		input.runRevision,
		input.token,
		status,
		timestamp(input.createdAt),
		timestamp(input.updatedAt),
	);
}

/** Applies one derived index update. Canonical workflow progress never awaits this. */
export async function applyCoordinatorUpdate(
	c: CoordinatorContext,
	input: CoordinatorIndexUpdate,
) {
	assertRevision(input.runRevision);
	await withActorTransaction(c, async (tx) => {
		switch (input.type) {
			case "run.upsert":
				await upsertRun(tx, input.run, input.runRevision);
				break;
			case "correlation.put":
			case "correlation.delete":
				await updateCorrelation(tx, input);
				break;
			case "hook.upsert":
			case "hook.delete":
				await updateHook(tx, input);
				break;
		}
	});
}

type IndexCursor = readonly [createdAt: number, id: string];

function encodeCursor(createdAt: unknown, id: unknown): string {
	return JSON.stringify([Number(createdAt), String(id)] satisfies IndexCursor);
}

function decodeCursor(cursor: string | undefined): IndexCursor | undefined {
	if (cursor == null) return undefined;
	try {
		const value: unknown = JSON.parse(cursor);
		if (
			!Array.isArray(value) ||
			value.length !== 2 ||
			!Number.isFinite(value[0]) ||
			typeof value[1] !== "string"
		) {
			throw new Error();
		}
		return [Number(value[0]), value[1]];
	} catch {
		throw new Error("invalid coordinator pagination cursor");
	}
}

export const coordinator = actor({
	db: coordinatorDb,
	actions: {
		update: async (c, input: CoordinatorIndexUpdate) => {
			assertCoordinatorKey(c);
			await applyCoordinatorUpdate(c as unknown as CoordinatorContext, input);
		},
		getRunIdByCorrelation: async (c, correlationId: string) => {
			assertCoordinatorKey(c);
			const row = await one(
				c,
				`SELECT run_id FROM correlation_index
				 WHERE correlation_id = ? AND status = 'active'`,
				correlationId,
			);
			return row?.run_id == null ? null : String(row.run_id);
		},
		getRunIdByHook: async (c, hookId: string) => {
			assertCoordinatorKey(c);
			const row = await one(
				c,
				"SELECT run_id FROM hooks_index WHERE hook_id = ? AND status = 'active'",
				hookId,
			);
			return row?.run_id == null ? null : String(row.run_id);
		},
		listHookIndex: async (c, params?: ListHooksParams) => {
			assertCoordinatorKey(c);
			const limit = params?.pagination?.limit ?? 100;
			const cursor = decodeCursor(params?.pagination?.cursor);
			const where = ["status = 'active'"];
			const args: unknown[] = [];
			if (cursor) {
				where.push("(created_at, hook_id) < (?, ?)");
				args.push(...cursor);
			}
			const rows = await c.db.execute(
				`
				SELECT hook_id, run_id, created_at
				FROM hooks_index
				WHERE ${where.join(" AND ")}
				ORDER BY created_at DESC, hook_id DESC
				LIMIT ?
				`,
				...args,
				limit + 1,
			);
			const values = rows.slice(0, limit);
			const last = values.at(-1);
			return {
				data: values.map((row) => ({
					hookId: String(row.hook_id),
					runId: String(row.run_id),
				})),
				cursor: last ? encodeCursor(last.created_at, last.hook_id) : null,
				hasMore: rows.length > limit,
			};
		},
		listRuns: async (c, params?: ListWorkflowRunsParams) => {
			assertCoordinatorKey(c);
			const limit = params?.pagination?.limit ?? 20;
			const cursor = decodeCursor(params?.pagination?.cursor);
			const where: string[] = [];
			const args: unknown[] = [];
			if (cursor) {
				where.push("(created_at, run_id) < (?, ?)");
				args.push(...cursor);
			}
			if (params?.workflowName) {
				where.push("workflow_name = ?");
				args.push(params.workflowName);
			}
			if (params?.status) {
				where.push("status = ?");
				args.push(params.status);
			}
			const rows = await c.db.execute(
				`
				SELECT * FROM runs_index
				${where.length > 0 ? `WHERE ${where.join(" AND ")}` : ""}
				ORDER BY created_at DESC, run_id DESC
				LIMIT ?
				`,
				...args,
				limit + 1,
			);
			const values = rows.slice(0, limit);
			const last = values.at(-1);
			return {
				data: values.map((row) =>
					filterData(rowToRun(row), params?.resolveData),
				),
				cursor: last ? encodeCursor(last.created_at, last.run_id) : null,
				hasMore: rows.length > limit,
			};
		},
	},
});
