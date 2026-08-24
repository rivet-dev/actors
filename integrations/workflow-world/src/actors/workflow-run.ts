import {
	EntityConflictError,
	HookNotFoundError,
	TooEarlyError,
	WorkflowRunNotFoundError,
	WorkflowWorldError,
} from "@workflow/errors";
import {
	MessageId,
	SPEC_VERSION_CURRENT,
	stripEventDataRefs,
	type CreateEventParams,
	type CreateEventRequest,
	type Event,
	type EventResult,
	type GetEventParams,
	type GetHookParams,
	type GetStepParams,
	type GetWorkflowRunParams,
	type Hook,
	type ListEventsByCorrelationIdParams,
	type ListEventsParams,
	type ListHooksParams,
	type ListWorkflowRunStepsParams,
	type RunCreatedEventRequest,
	type Step,
	type Wait,
	type WorkflowRun,
} from "@workflow/world";
import { actor } from "rivetkit";
import { isDeepStrictEqual } from "node:util";
import { monotonicFactory } from "ulid";
import { decodeValue, encodeValue } from "../codec.js";
import type { CoordinatorIndexUpdate } from "./coordinator.js";
import { workflowDb } from "./db.js";
import {
	createDispatcherVars,
	enqueueDispatch,
	forceStaleInflightForTesting,
	insertDispatch,
	inspectQueue,
	nextInflightRecoveryAt,
	startDispatchLoop,
	type DispatchContext,
} from "./dispatcher.js";
import {
	expireClosedStreamForTesting,
	cleanupExpiredStreams,
	getStreamChunks,
	getStreamInfo,
	listStreams,
	STREAM_TTL_MS,
	streamAppended,
	writeStream,
} from "./streams.js";
import {
	filterData,
	filterHookData,
	one,
	requireRun,
	rowToEvent,
	rowToHook,
	rowToStep,
	rowToWait,
	toMs,
	withActorTransaction,
	type Ctx,
} from "./shared.js";

const ulid = monotonicFactory();
const terminalRuns = new Set(["completed", "failed", "cancelled"]);
const terminalSteps = new Set(["completed", "failed", "cancelled"]);
const creationEvents = new Set([
	"run_created",
	"step_created",
	"hook_created",
	"wait_created",
]);
type InitialDispatchConfig = {
	runtimeUrl: string;
	queueName: string;
	headers: Record<string, string>;
	initial: {
		messageId: string;
		body: string;
		idempotencyKey: string;
	};
};

type HookSideEffects = {
	confirm?: { token: string; hookId: string; generation: number };
};

type PendingOperation =
	| CoordinatorIndexUpdate
	| ({ type: "hook.confirm" } & NonNullable<HookSideEffects["confirm"]> & {
			runId: string;
	  })
	| {
			type: "hook.release";
			token: string;
			hookId: string;
			generation: number;
			runId: string;
	  };

type HookConflictEventRequest = {
	eventType: "hook_conflict";
	specVersion?: number;
	correlationId: string;
	eventData: {
		token: string;
		conflictingRunId?: string;
	};
};

type StorableEventRequest =
	| CreateEventRequest
	| RunCreatedEventRequest
	| HookConflictEventRequest;

type WorkflowVars = DispatchContext["vars"] & {
	recoveryPrearmPromise: Promise<void> | null;
	recoveryDeletePromise: Promise<void> | null;
	recoveryWriters: number;
	pendingDraining: boolean;
	pendingGeneration: number;
};

type WorkflowContext = Omit<DispatchContext, "vars"> & {
	vars: WorkflowVars;
	key: readonly string[];
	client<T = any>(): T;
	broadcast(name: "streamAppended", value: unknown): void;
	cron: {
		every(options: {
			name: string;
			interval: number;
			action: string;
			args?: unknown[];
			maxHistory?: number;
		}): Promise<void>;
		delete(name: string): Promise<boolean>;
	};
};

// Durable schedule id. Runs armed by an earlier release still hold this name,
// and only a delete under the exact name stops them, so it must not change.
const RECOVERY_CRON = "vercel-world-recovery";
const RECOVERY_INTERVAL_MS = 5_000;

async function prearmRecovery(c: WorkflowContext) {
	// Recurring schedules advance before dispatch. If a fire is consumed while
	// this action is committing, the next recovery fire remains durable.
	if (c.vars.recoveryDeletePromise) {
		await c.vars.recoveryDeletePromise;
	}
	if (!c.vars.recoveryPrearmPromise) {
		const arm = c.cron.every({
			name: RECOVERY_CRON,
			interval: RECOVERY_INTERVAL_MS,
			action: "wake",
			args: [],
			maxHistory: 0,
		});
		c.vars.recoveryPrearmPromise = arm;
		void arm.catch(() => {
			if (c.vars.recoveryPrearmPromise === arm) {
				c.vars.recoveryPrearmPromise = null;
			}
		});
	}
	await c.vars.recoveryPrearmPromise;
}

async function finishRecoveryArm(c: WorkflowContext) {
	const nextWakeAt = await armNextWake(c);
	// An immediately-due one-shot may already be claimed and waiting behind this
	// action, so keep the recurring backstop until a later wake drains that work.
	// A wake safely beyond the next backstop interval cannot be claimed during
	// this action; the exact durable alarm is sufficient until then.
	if (
		c.vars.recoveryWriters === 0 &&
		!c.vars.recoveryDeletePromise &&
		(nextWakeAt == null || nextWakeAt > Date.now() + RECOVERY_INTERVAL_MS)
	) {
		const deletion = c.cron.delete(RECOVERY_CRON).then(() => undefined);
		const tracked = deletion.finally(() => {
			// A failed delete is ambiguous: force the next writer to re-arm after the
			// delete settles instead of assuming the named cron still exists.
			c.vars.recoveryPrearmPromise = null;
			if (c.vars.recoveryDeletePromise === tracked) {
				c.vars.recoveryDeletePromise = null;
			}
		});
		c.vars.recoveryDeletePromise = tracked;
		await tracked;
	}
}

const createWorkflowVars = (): WorkflowVars => ({
	...createDispatcherVars(),
	recoveryPrearmPromise: null,
	recoveryDeletePromise: null,
	recoveryWriters: 0,
	pendingDraining: false,
	pendingGeneration: 0,
});

function asWorkflowContext(c: unknown) {
	const ctx = c as WorkflowContext;
	ctx.armNextWake = async () => {
		await armNextWake(ctx);
	};
	return ctx;
}

async function withSerializedAction<T>(
	c: WorkflowContext,
	fn: () => Promise<T>,
): Promise<T> {
	const previous = c.vars.serialTail ?? Promise.resolve();
	let release!: () => void;
	const current = new Promise<void>((resolve) => {
		release = resolve;
	});
	c.vars.serialTail = previous.then(() => current);
	await previous;
	try {
		return await fn();
	} finally {
		release();
	}
}

async function withRecoveryWriter<T>(
	c: WorkflowContext,
	fn: () => Promise<T>,
): Promise<T> {
	c.vars.recoveryWriters++;
	try {
		await prearmRecovery(c);
		return await fn();
	} finally {
		try {
			await armNextWake(c);
		} catch {
			// The recurring recovery cron remains the durable fallback.
		} finally {
			c.vars.recoveryWriters--;
		}
	}
}

function assertRunKey(c: { key: readonly string[] }, runId: string) {
	const actorRunId = c.key[0];
	if (!actorRunId || actorRunId !== runId) {
		throw new Error(
			`workflowRun actor key mismatch: expected ${actorRunId ?? "<missing>"}, received ${runId}`,
		);
	}
}

function errorDetails(error: unknown): { message: string; stack?: string } {
	if (typeof error === "string") return { message: error };
	if (error instanceof Error) {
		return { message: error.message, stack: error.stack };
	}
	return { message: "Unknown error" };
}

async function currentRunStatus(c: Ctx, runId: string) {
	const row = await one(
		c,
		"SELECT status FROM workflow_runs WHERE run_id = ?",
		runId,
	);
	return row?.status == null ? null : String(row.status);
}

async function insertEvent(
	c: Ctx,
	runId: string,
	eventId: string,
	data: StorableEventRequest,
	specVersion: number,
	now: number,
) {
	// Preserve eventData for every event that carries it — including run_started,
	// whose optional creation fields back the resilient-start recovery path
	// (recreate the run from run_started when run_created was lost; SPEC §3.2,
	// `@workflow/world` queue.d.ts).
	const storedEventData = "eventData" in data ? data.eventData : undefined;
	const dedupeCreation = creationEvents.has(data.eventType);
	await c.db.execute(
		`
			INSERT ${dedupeCreation ? "OR IGNORE" : ""} INTO workflow_events (
				event_id, run_id, event_type, correlation_id, event_data, spec_version, created_at
			)
			VALUES (?, ?, ?, ?, ?, ?, ?)
		`,
		eventId,
		runId,
		data.eventType,
		"correlationId" in data ? data.correlationId : null,
		encodeValue(storedEventData),
		specVersion,
		now,
	);
	const inserted = await one(
		c,
		"SELECT * FROM workflow_events WHERE event_id = ?",
		eventId,
	);
	if (inserted) return rowToEvent(inserted);
	if (dedupeCreation) {
		const existing =
			data.eventType === "run_created"
				? await one(
						c,
						"SELECT * FROM workflow_events WHERE run_id = ? AND event_type = 'run_created'",
						runId,
					)
				: await one(
						c,
						"SELECT * FROM workflow_events WHERE run_id = ? AND event_type = ? AND correlation_id = ?",
						runId,
						data.eventType,
						"correlationId" in data ? data.correlationId : null,
					);
		if (existing) return rowToEvent(existing);
	}
	return rowToEvent({
		...data,
		run_id: runId,
		event_id: eventId,
		event_type: data.eventType,
		correlation_id: "correlationId" in data ? data.correlationId : null,
		event_data: encodeValue(storedEventData),
		spec_version: specVersion,
		created_at: now,
	});
}

async function listEventsForRun(c: Ctx, runId: string) {
	return (
		await c.db.execute(
			"SELECT * FROM workflow_events WHERE run_id = ? ORDER BY event_id",
			runId,
		)
	).map(rowToEvent);
}

async function releaseRunHooks(c: Ctx, runId: string) {
	await c.db.execute("DELETE FROM workflow_hooks WHERE run_id = ?", runId);
	await c.db.execute("DELETE FROM workflow_waits WHERE run_id = ?", runId);
}

const OUTBOX_CLAIM_MS = 60_000;
const OUTBOX_BATCH = 32;
const OUTBOX_DRAIN_BUDGET_MS = 5_000;

async function withinDeadline<T>(promise: Promise<T>, deadline: number): Promise<T> {
	const remaining = deadline - Date.now();
	if (remaining <= 0) throw new Error("pending operation drain exceeded its time budget");
	let timeout: ReturnType<typeof setTimeout> | undefined;
	try {
		return await Promise.race([
			promise,
			new Promise<never>((_, reject) => {
				timeout = setTimeout(
					() => reject(new Error("pending operation drain exceeded its time budget")),
					remaining,
				);
			}),
		]);
	} finally {
		if (timeout) clearTimeout(timeout);
	}
}

async function armNextWake(c: WorkflowContext) {
	const [dispatchReady, dispatchInflight, pending, streamExpiry] = await Promise.all([
		one(c, "SELECT MIN(next_at) AS due FROM dispatcher_queue WHERE status = 'ready'"),
		nextInflightRecoveryAt(c, c.vars.activeMessageIds).then((due) =>
			due == null ? undefined : { due },
		),
		one(c, "SELECT MIN(next_at) AS due FROM pending_ops"),
		STREAM_TTL_MS === 0
			? Promise.resolve(undefined)
			: one(c, `SELECT MIN(created_at + ?) AS due FROM stream_chunks
				WHERE eof = 1`, STREAM_TTL_MS),
	]);
	const due = [
		dispatchReady?.due,
		dispatchInflight?.due,
		pending?.due,
		streamExpiry?.due,
	]
		.filter((value): value is number | string => value != null)
		.map(Number)
		.filter(Number.isFinite);
	if (due.length === 0) return null;
	const target = Math.min(...due);
	if (c.vars.wakeArmedAt != null && c.vars.wakeArmedAt <= target) {
		return c.vars.wakeArmedAt;
	}
	await c.schedule.at(Math.max(Date.now(), target), "wake");
	c.vars.wakeArmedAt = target;
	return target;
}

async function addPendingOp(
	c: Ctx,
	opId: string,
	runRevision: number,
	payload: PendingOperation,
) {
	const now = Date.now();
	await c.db.execute(
		`INSERT INTO pending_ops (
			op_id, op_type, payload, operation_revision, status, next_at,
			attempt, created_at, updated_at
		) VALUES (?, ?, ?, ?, 'ready', ?, 0, ?, ?)
		ON CONFLICT(op_id) DO UPDATE SET
			op_type = excluded.op_type,
			payload = excluded.payload,
			operation_revision = excluded.operation_revision,
			status = 'ready',
			claim_id = NULL,
			next_at = excluded.next_at,
			updated_at = excluded.updated_at
		WHERE excluded.operation_revision > pending_ops.operation_revision`,
		opId,
		payload.type,
		encodeValue(payload),
		runRevision,
		now,
		now,
		now,
	);
}

async function drainPendingOps(c: WorkflowContext) {
	const deadline = Date.now() + OUTBOX_DRAIN_BUDGET_MS;
	for (let processed = 0; processed < OUTBOX_BATCH; processed++) {
		const now = Date.now();
		const claimId = `claim_${ulid()}`;
		const rows = await c.db.execute(
			`UPDATE pending_ops SET status = 'inflight', claim_id = ?,
			 next_at = ?, attempt = attempt + 1, updated_at = ?
			 WHERE op_id = (
				SELECT op_id FROM pending_ops
				WHERE (status = 'ready' AND next_at <= ?)
				   OR (status = 'inflight' AND next_at <= ?)
				ORDER BY created_at ASC LIMIT 1
			 ) RETURNING op_id, payload, operation_revision, attempt`,
			claimId,
			now + OUTBOX_CLAIM_MS,
			now,
			now,
			now,
		);
		const row = rows[0];
		if (!row) return;
		await armNextWake(c);
		try {
			const payload = decodeValue<PendingOperation>(row.payload);
			if (!payload) throw new Error("pending coordinator update is empty");
			if (payload.type === "hook.confirm") {
				const result = await withinDeadline<{ ok: boolean }>(
					c.client<any>().hookToken.getOrCreate([payload.token]).confirm(
						payload.token,
						payload.generation,
						payload.runId,
						payload.hookId,
					),
					deadline,
				);
				if (!result.ok) throw new Error("hook confirmation generation is stale");
			} else if (payload.type === "hook.release") {
				await withinDeadline(
					c.client<any>().hookToken.getOrCreate([payload.token]).release(
						payload.token,
						payload.generation,
						payload.runId,
						payload.hookId,
					),
					deadline,
				);
			} else {
				await withinDeadline(
					c.client<any>().coordinator.getOrCreate(["coordinator"]).update(payload),
					deadline,
				);
			}
			await c.db.execute(
				`DELETE FROM pending_ops
				 WHERE op_id = ? AND claim_id = ? AND operation_revision = ?`,
				String(row.op_id),
				claimId,
				Number(row.operation_revision),
			);
		} catch {
			await c.db.execute(
				`UPDATE pending_ops SET status = 'ready', claim_id = NULL,
				 next_at = ?, updated_at = ? WHERE op_id = ? AND claim_id = ?`,
				now + Math.min(30_000, 1_000 * 2 ** Math.min(Number(row.attempt) - 1, 5)),
				now,
				String(row.op_id),
				claimId,
			);
			await armNextWake(c);
			return;
		}
	}
	// A full batch means there may be more work. Yield through the actor alarm.
	await c.schedule.after(0, "wake");
}

function startPendingDrain(c: WorkflowContext) {
	c.vars.pendingGeneration++;
	if (c.vars.pendingDraining) return;
	const launch = () => {
		const generation = c.vars.pendingGeneration;
		c.vars.pendingDraining = true;
		void c
			.keepAwake(
				drainPendingOps(c).finally(() => {
					c.vars.pendingDraining = false;
					if (c.vars.pendingGeneration !== generation) launch();
				}),
			)
			.catch(() => {});
	};
	launch();
}

export const workflowRun = actor({
	db: workflowDb,
	events: { streamAppended },
	createVars: createWorkflowVars,
	onWake: async (c) => {
		const ctx = asWorkflowContext(c);
		await withSerializedAction(ctx, async () => {
			await prearmRecovery(ctx);
			ctx.vars.wakeArmedAt = null;
			await cleanupExpiredStreams(ctx);
			await drainPendingOps(ctx);
			startDispatchLoop(ctx, ctx.key[0] ?? "__health");
			await finishRecoveryArm(ctx);
		});
	},
	actions: {
		appendEvent: async (
			c,
			runIdOrNull: string | null,
			data: StorableEventRequest,
			params?: CreateEventParams,
			initialDispatch?: InitialDispatchConfig,
			hookSideEffects?: HookSideEffects,
		): Promise<EventResult> => {
			const wfc = asWorkflowContext(c);
			return withRecoveryWriter(wfc, async () => {
			const actorRunId = wfc.key[0];
			if (!actorRunId) throw new Error("workflowRun actor key is missing");
			if (runIdOrNull != null) assertRunKey(wfc, runIdOrNull);
			const result = await withActorTransaction(wfc, async (c): Promise<EventResult> => {
			const runId =
				data.eventType === "run_created" && !runIdOrNull
					? actorRunId
					: runIdOrNull;
			if (!runId) throw new Error("runId is required");

			const specVersion = data.specVersion ?? SPEC_VERSION_CURRENT;
			const now = Date.now();
			const resolveData = params?.resolveData ?? "all";
			let run: WorkflowRun | undefined;
			let step: Step | undefined;
			let hook: Hook | undefined;
			let hooksToDelete: Array<{ hook: Hook; generation: number | null }> = [];
			let wait: Wait | undefined;
			let stepCreated: boolean | undefined;

			const status =
				data.eventType === "run_created"
					? null
					: await currentRunStatus(c, runId);
			if (status && terminalRuns.has(status)) {
				throw new EntityConflictError(
					`Cannot modify run in terminal state "${status}"`,
				);
			}
			if (
				status == null &&
				data.eventType !== "run_created" &&
				data.eventType !== "run_started"
			) {
				throw new WorkflowRunNotFoundError(runId);
			}

			if (data.eventType === "run_created") {
				await c.db.execute(
					`
					INSERT OR IGNORE INTO workflow_runs (
						run_id, status, deployment_id, workflow_name, spec_version,
						execution_context, attributes, input, created_at, updated_at
					)
					VALUES (?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?)
					`,
					runId,
					data.eventData.deploymentId,
					data.eventData.workflowName,
					specVersion,
					encodeValue(data.eventData.executionContext),
					encodeValue(data.eventData.attributes ?? {}),
					encodeValue(data.eventData.input),
					now,
					now,
				);
				run = await requireRun(c, runId);
				if (initialDispatch) {
					await insertDispatch(
						c,
						initialDispatch.initial.messageId,
						initialDispatch.queueName,
						initialDispatch.runtimeUrl,
						initialDispatch.initial.body,
						initialDispatch.headers,
						0,
						initialDispatch.initial.idempotencyKey,
					);
				}
			}

			if (data.eventType === "run_started") {
				// Resilient start (SPEC §3.2): if run_created was lost, recreate the
				// run from the creation fields carried on run_started when present.
				const startData = data.eventData;
				if (startData?.deploymentId && startData.workflowName) {
					await c.db.execute(
						`
						INSERT OR IGNORE INTO workflow_runs (
							run_id, status, deployment_id, workflow_name, spec_version,
							execution_context, attributes, input, created_at, updated_at
						)
						VALUES (?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?)
						`,
						runId,
						startData.deploymentId,
						startData.workflowName,
						specVersion,
						encodeValue(startData.executionContext),
						encodeValue(startData.attributes ?? {}),
						encodeValue(startData.input),
						now,
						now,
					);
				}
				await c.db.execute(
					`
					UPDATE workflow_runs
					SET status = 'running', started_at = COALESCE(started_at, ?), updated_at = ?
					WHERE run_id = ?
					`,
					now,
					now,
					runId,
				);
				run = await requireRun(c, runId);
			}

			if (data.eventType === "run_completed") {
				hooksToDelete = (
					await c.db.execute("SELECT * FROM workflow_hooks WHERE run_id = ?", runId)
				).map((row) => ({
					hook: rowToHook(row),
					generation:
						row.token_generation == null ? null : Number(row.token_generation),
				}));
				await c.db.execute(
					`
					UPDATE workflow_runs
					SET status = 'completed', output = ?, completed_at = ?, updated_at = ?
					WHERE run_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')
					`,
					encodeValue(data.eventData.output),
					now,
					now,
					runId,
				);
				run = await requireRun(c, runId);
				await releaseRunHooks(c, runId);
			}

			if (data.eventType === "run_failed") {
				hooksToDelete = (
					await c.db.execute("SELECT * FROM workflow_hooks WHERE run_id = ?", runId)
				).map((row) => ({
					hook: rowToHook(row),
					generation:
						row.token_generation == null ? null : Number(row.token_generation),
				}));
				const error = errorDetails(data.eventData.error);
				await c.db.execute(
					`
					UPDATE workflow_runs
					SET status = 'failed', error = ?, completed_at = ?, updated_at = ?
					WHERE run_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')
					`,
					encodeValue({
						message: error.message,
						stack: error.stack,
						code: data.eventData.errorCode,
					}),
					now,
					now,
					runId,
				);
				run = await requireRun(c, runId);
				await releaseRunHooks(c, runId);
			}

			if (data.eventType === "run_cancelled") {
				hooksToDelete = (
					await c.db.execute("SELECT * FROM workflow_hooks WHERE run_id = ?", runId)
				).map((row) => ({
					hook: rowToHook(row),
					generation:
						row.token_generation == null ? null : Number(row.token_generation),
				}));
				await c.db.execute(
					`
					UPDATE workflow_runs
					SET status = 'cancelled', completed_at = ?, updated_at = ?
					WHERE run_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')
					`,
					now,
					now,
					runId,
				);
				run = await requireRun(c, runId);
				await releaseRunHooks(c, runId);
			}

			if (data.eventType === "step_created") {
				await c.db.execute(
					`
					INSERT OR IGNORE INTO workflow_steps (
						step_id, run_id, step_name, status, input, attempt, created_at, updated_at, spec_version
					)
					VALUES (?, ?, ?, 'pending', ?, 0, ?, ?, ?)
					`,
					data.correlationId,
					runId,
					data.eventData.stepName,
					encodeValue(data.eventData.input),
					now,
					now,
					specVersion,
				);
				const row = await one(
					c,
					"SELECT * FROM workflow_steps WHERE step_id = ?",
					data.correlationId,
				);
				if (row) step = rowToStep(row);
			}

			if (data.eventType === "step_started") {
				let existing = await one(
					c,
					"SELECT * FROM workflow_steps WHERE step_id = ?",
					data.correlationId,
				);
				const lazyData =
					data.eventData?.stepName != null &&
					data.eventData.input !== undefined
						? {
								stepName: data.eventData.stepName,
								input: data.eventData.input,
							}
						: null;
				if (!existing && lazyData) {
					const inserted = await c.db.execute(
						`
						INSERT OR IGNORE INTO workflow_steps (
							step_id, run_id, step_name, status, input, attempt, created_at, updated_at, spec_version
						)
						VALUES (?, ?, ?, 'pending', ?, 0, ?, ?, ?)
						RETURNING step_id
						`,
						data.correlationId,
						runId,
						lazyData.stepName,
						encodeValue(lazyData.input),
						now,
						now,
						specVersion,
					);
					if (inserted.length === 0) {
						throw new EntityConflictError(
							`Step "${data.correlationId}" already exists`,
						);
					}
					stepCreated = true;
					// Lazy step creation must be visible to event-log replay exactly as
					// eager creation is. Both rows live in this actor transaction, so the
					// synthetic creation and the materialized step cannot diverge.
					await insertEvent(
						c,
						runId,
						`wevt_${ulid()}`,
						{
							eventType: "step_created",
							correlationId: data.correlationId,
							eventData: {
								stepName: lazyData.stepName,
								input: lazyData.input,
							},
						},
						specVersion,
						now,
					);
					existing = await one(
						c,
						"SELECT * FROM workflow_steps WHERE step_id = ?",
						data.correlationId,
					);
				} else if (existing && lazyData) {
					if (
						String(existing.step_name) !== lazyData.stepName ||
						!isDeepStrictEqual(decodeValue(existing.input), lazyData.input)
					) {
						throw new EntityConflictError(
							`Step "${data.correlationId}" already exists with different data`,
						);
					}
				}
				if (!existing)
					throw new WorkflowWorldError(`Step "${data.correlationId}" not found`);
				if (
					String(existing.status) === "running" ||
					terminalSteps.has(String(existing.status))
				) {
					// Vercel may redeliver an accepted step start. Returning the existing
					// step keeps delivery idempotent without incrementing its attempt.
					step = rowToStep(existing);
				} else {
					if (
						existing.retry_after != null &&
						Number(existing.retry_after) > Date.now()
					) {
						throw new TooEarlyError(
							`Cannot start step "${data.correlationId}": retryAfter timestamp has not been reached yet`,
							{
								retryAfter: Math.ceil(
									(Number(existing.retry_after) - Date.now()) / 1000,
								),
							},
						);
					}
					await c.db.execute(
						`
						UPDATE workflow_steps
						SET status = 'running',
							attempt = attempt + 1,
							started_at = COALESCE(started_at, ?),
							retry_after = NULL,
							updated_at = ?
						WHERE step_id = ? AND status NOT IN ('running', 'completed', 'failed', 'cancelled')
						`,
						now,
						now,
						data.correlationId,
					);
					step = rowToStep(
						await one(
							c,
							"SELECT * FROM workflow_steps WHERE step_id = ?",
							data.correlationId,
						),
					);
				}
			}

			if (data.eventType === "step_completed") {
				const updated = await c.db.execute(
					`
					UPDATE workflow_steps
					SET status = 'completed', output = ?, completed_at = ?, updated_at = ?
					WHERE step_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')
					RETURNING *
					`,
					encodeValue(data.eventData.result),
					now,
					now,
					data.correlationId,
				);
				if (updated[0]) {
					step = rowToStep(updated[0]);
				} else {
					const existing = await one(
						c,
						"SELECT * FROM workflow_steps WHERE step_id = ?",
						data.correlationId,
					);
					if (!existing)
						throw new WorkflowWorldError(`Step "${data.correlationId}" not found`);
					if (terminalSteps.has(String(existing.status))) {
						throw new EntityConflictError(
							`Cannot modify step in terminal state "${existing.status}"`,
						);
					}
					step = rowToStep(existing);
				}
			}

			if (data.eventType === "step_failed") {
				const error = errorDetails(data.eventData.error);
				const updated = await c.db.execute(
					`
					UPDATE workflow_steps
					SET status = 'failed', error = ?, completed_at = ?, updated_at = ?
					WHERE step_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')
					RETURNING *
					`,
					encodeValue({ message: error.message, stack: error.stack }),
					now,
					now,
					data.correlationId,
				);
				if (updated[0]) {
					step = rowToStep(updated[0]);
				} else {
					const existing = await one(
						c,
						"SELECT * FROM workflow_steps WHERE step_id = ?",
						data.correlationId,
					);
					if (!existing)
						throw new WorkflowWorldError(`Step "${data.correlationId}" not found`);
					if (terminalSteps.has(String(existing.status))) {
						throw new EntityConflictError(
							`Cannot modify step in terminal state "${existing.status}"`,
						);
					}
					step = rowToStep(existing);
				}
			}

			if (data.eventType === "step_retrying") {
				const error = errorDetails(data.eventData.error);
				const updated = await c.db.execute(
					`
					UPDATE workflow_steps
					SET status = 'pending', error = ?, retry_after = ?, updated_at = ?
					WHERE step_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')
					RETURNING *
					`,
					encodeValue({ message: error.message, stack: error.stack }),
					toMs(data.eventData.retryAfter),
					now,
					data.correlationId,
				);
				if (updated[0]) {
					step = rowToStep(updated[0]);
				} else {
					const existing = await one(
						c,
						"SELECT * FROM workflow_steps WHERE step_id = ?",
						data.correlationId,
					);
					if (!existing)
						throw new WorkflowWorldError(`Step "${data.correlationId}" not found`);
					if (terminalSteps.has(String(existing.status))) {
						throw new EntityConflictError(
							`Cannot modify step in terminal state "${existing.status}"`,
						);
					}
					step = rowToStep(existing);
				}
			}

			if (data.eventType === "hook_created") {
				const existingCreation = await one(
					c,
					`SELECT 1 FROM workflow_events
					 WHERE event_type = 'hook_created' AND correlation_id = ?`,
					data.correlationId,
				);
				if (!existingCreation) await c.db.execute(
					`
					INSERT OR IGNORE INTO workflow_hooks (
						hook_id, run_id, token, token_generation, owner_id, project_id, environment,
						metadata, created_at, spec_version, is_webhook
					)
					VALUES (?, ?, ?, ?, '', '', '', ?, ?, ?, ?)
					`,
					data.correlationId,
					runId,
					data.eventData.token,
					hookSideEffects?.confirm?.generation ?? null,
					encodeValue(data.eventData.metadata),
					now,
					specVersion,
					1,
				);
				const row = await one(
					c,
					"SELECT * FROM workflow_hooks WHERE hook_id = ?",
					data.correlationId,
				);
				if (existingCreation && !row) {
					throw new EntityConflictError(
						`Hook "${data.correlationId}" was already disposed`,
					);
				}
				if (row && String(row.token) !== data.eventData.token) {
					throw new EntityConflictError(
						`Hook "${data.correlationId}" already uses a different token`,
					);
				}
				if (row) hook = rowToHook(row);
			}

			if (data.eventType === "hook_disposed") {
				const existingHook = await one(
					c,
					"SELECT * FROM workflow_hooks WHERE hook_id = ?",
					data.correlationId,
				);
				if (existingHook) {
					hooksToDelete.push({
						hook: rowToHook(existingHook),
						generation:
							existingHook.token_generation == null
								? null
								: Number(existingHook.token_generation),
					});
				}
				await c.db.execute(
					"DELETE FROM workflow_hooks WHERE hook_id = ?",
					data.correlationId,
				);
			}

			if (data.eventType === "wait_created") {
				const waitId = `${runId}-${data.correlationId}`;
				const resumeAt = toMs(data.eventData.resumeAt);
				await c.db.execute(
					`
					INSERT OR IGNORE INTO workflow_waits (
						wait_id, run_id, status, resume_at, created_at, updated_at, spec_version
					)
					VALUES (?, ?, 'waiting', ?, ?, ?, ?)
					RETURNING wait_id
					`,
					waitId,
					runId,
					resumeAt,
					now,
					now,
					specVersion,
				);
				wait = rowToWait(
					await one(c, "SELECT * FROM workflow_waits WHERE wait_id = ?", waitId),
				);
			}

			if (data.eventType === "wait_completed") {
				const waitId = `${runId}-${data.correlationId}`;
				await c.db.execute(
					`
					UPDATE workflow_waits
					SET status = 'completed', completed_at = ?, updated_at = ?
					WHERE wait_id = ? AND status = 'waiting'
					`,
					now,
					now,
					waitId,
				);
				const row = await one(
					c,
					"SELECT * FROM workflow_waits WHERE wait_id = ?",
					waitId,
				);
				if (row) wait = rowToWait(row);
			}

			const event = await insertEvent(
				c,
				runId,
				`wevt_${ulid()}`,
				data,
				specVersion,
				now,
			);
			await c.db.execute(
				"UPDATE workflow_runs SET run_revision = run_revision + 1 WHERE run_id = ?",
				runId,
			);
			const revisionRow = await one(
				c,
				"SELECT run_revision FROM workflow_runs WHERE run_id = ?",
				runId,
			);
			const runRevision = Number(revisionRow?.run_revision ?? 0);
			if (run) {
				run = await requireRun(c, runId);
				await addPendingOp(c, "coordinator:run", runRevision, {
					type: "run.upsert",
					run,
					runRevision,
				});
			}
			const indexCorrelationId =
				"correlationId" in data ? data.correlationId : undefined;
			if (
				indexCorrelationId != null &&
				(data.eventType === "step_created" ||
					stepCreated === true ||
					data.eventType === "hook_created" ||
					data.eventType === "wait_created")
			) {
				await addPendingOp(c, `coordinator:correlation:${indexCorrelationId}`, runRevision, {
					type: "correlation.put",
					correlationId: indexCorrelationId,
					runId,
					kind:
						data.eventType === "step_created" || stepCreated === true
							? "step"
							: data.eventType === "hook_created"
								? "hook"
								: "wait",
					runRevision,
				});
			}
			if (data.eventType === "hook_created" && hook) {
				await addPendingOp(c, `coordinator:hook:${hook.hookId}`, runRevision, {
					type: "hook.upsert",
					hookId: hook.hookId,
					runId,
					token: hook.token,
					runRevision,
					createdAt: hook.createdAt,
					updatedAt: hook.createdAt,
				});
			}
			for (const { hook: deletedHook, generation } of hooksToDelete) {
				await addPendingOp(c, `coordinator:hook:${deletedHook.hookId}`, runRevision, {
					type: "hook.delete",
					hookId: deletedHook.hookId,
					runId,
					token: deletedHook.token,
					runRevision,
					createdAt: deletedHook.createdAt,
					updatedAt: Date.now(),
				});
				await addPendingOp(
					c,
					`coordinator:correlation:${deletedHook.hookId}`,
					runRevision,
					{
						type: "correlation.delete",
						correlationId: deletedHook.hookId,
						runId,
						kind: "hook",
						runRevision,
					},
				);
				if (generation != null) {
					await addPendingOp(c, `hook-token:${deletedHook.token}`, runRevision, {
						type: "hook.release",
						token: deletedHook.token,
						hookId: deletedHook.hookId,
						generation,
						runId,
					});
				}
			}
			if (hookSideEffects?.confirm) {
				await addPendingOp(c, `hook-token:${hookSideEffects.confirm.token}`, runRevision, {
					type: "hook.confirm",
					runId,
					...hookSideEffects.confirm,
				});
			}

			let events: Event[] | undefined;
			let cursor: string | null | undefined;
			let hasMore: boolean | undefined;
			if (data.eventType === "run_started" && run) {
				events = (await listEventsForRun(c, runId)).map((item) =>
					stripEventDataRefs(item, resolveData),
				);
				cursor = events.at(-1)?.eventId ?? null;
				hasMore = false;
			}

			return {
				event: stripEventDataRefs(event, resolveData),
				run: run && (filterData(run, resolveData) as WorkflowRun),
				step: step && (filterData(step, resolveData) as Step),
				hook: hook && (filterHookData(hook, resolveData) as Hook),
				wait,
				stepCreated,
				events,
				cursor,
				hasMore,
			} as EventResult;
			});
			if (data.eventType === "run_created" && initialDispatch) {
				startDispatchLoop(wfc, actorRunId);
			}
			await wfc.schedule.after(0, "wake");
			startPendingDrain(wfc);
			return result;
			});
		},
		getRun: async (c, runId: string, params?: GetWorkflowRunParams) => {
			assertRunKey(c, runId);
			return filterData(
				await requireRun(c, runId),
				params?.resolveData,
			) as WorkflowRun;
		},
		getEvent: async (
			c,
			runId: string,
			eventId: string,
			params?: GetEventParams,
		) => {
			assertRunKey(c, runId);
			const row = await one(
				c,
				`
				SELECT * FROM workflow_events
				WHERE run_id = ? AND event_id = ?
				`,
				runId,
				eventId,
			);
			if (!row) throw new WorkflowWorldError(`Event not found: ${eventId}`);
			return stripEventDataRefs(rowToEvent(row), params?.resolveData ?? "all");
		},
		listEvents: async (c, params: ListEventsParams) => {
			assertRunKey(c, params.runId);
			const limit = params.pagination?.limit ?? 100;
			const cursor = params.pagination?.cursor;
			const desc = params.pagination?.sortOrder === "desc";
			const rows = await c.db.execute(
				`
				SELECT * FROM workflow_events
				WHERE run_id = ? AND (? IS NULL OR event_id ${desc ? "<" : ">"} ?)
				ORDER BY event_id ${desc ? "DESC" : "ASC"}
				LIMIT ?
				`,
				params.runId,
				cursor ?? null,
				cursor ?? null,
				limit + 1,
			);
			const values = rows.slice(0, limit);
			return {
				data: values.map((row) =>
					stripEventDataRefs(rowToEvent(row), params.resolveData ?? "all"),
				),
				cursor: values.at(-1)?.event_id?.toString() ?? null,
				hasMore: rows.length > limit,
			};
		},
		listEventsByCorrelationId: async (
			c,
			params: ListEventsByCorrelationIdParams,
		) => {
			const limit = params.pagination?.limit ?? 100;
			const cursor = params.pagination?.cursor;
			const rows = await c.db.execute(
				`
				SELECT * FROM workflow_events
				WHERE correlation_id = ? AND (? IS NULL OR event_id > ?)
				ORDER BY event_id ASC
				LIMIT ?
				`,
				params.correlationId,
				cursor ?? null,
				cursor ?? null,
				limit + 1,
			);
			const values = rows.slice(0, limit);
			return {
				data: values.map((row) =>
					stripEventDataRefs(rowToEvent(row), params.resolveData ?? "all"),
				),
				cursor: values.at(-1)?.event_id?.toString() ?? null,
				hasMore: rows.length > limit,
			};
		},
		getStep: async (
			c,
			runId: string | undefined,
			stepId: string,
			params?: GetStepParams,
		) => {
			if (runId) assertRunKey(c, runId);
			const row = runId
				? await one(
						c,
						"SELECT * FROM workflow_steps WHERE run_id = ? AND step_id = ?",
						runId,
						stepId,
					)
				: await one(c, "SELECT * FROM workflow_steps WHERE step_id = ?", stepId);
			if (!row) throw new WorkflowWorldError(`Step not found: ${stepId}`);
			return filterData(rowToStep(row), params?.resolveData) as Step;
		},
		listSteps: async (c, params: ListWorkflowRunStepsParams) => {
			assertRunKey(c, params.runId);
			const limit = params.pagination?.limit ?? 20;
			const cursor = params.pagination?.cursor;
			const rows = await c.db.execute(
				`
				SELECT * FROM workflow_steps
				WHERE run_id = ? AND (? IS NULL OR step_id < ?)
				ORDER BY step_id DESC
				LIMIT ?
				`,
				params.runId,
				cursor ?? null,
				cursor ?? null,
				limit + 1,
			);
			const values = rows.slice(0, limit);
			return {
				data: values.map((row) =>
					filterData(rowToStep(row), params.resolveData),
				),
				cursor: values.at(-1)?.step_id?.toString() ?? null,
				hasMore: rows.length > limit,
			};
		},
		getHook: async (c, hookId: string, params?: GetHookParams) => {
			const row = await one(
				c,
				"SELECT * FROM workflow_hooks WHERE hook_id = ?",
				hookId,
			);
			if (!row) throw new HookNotFoundError(hookId);
			return filterHookData(rowToHook(row), params?.resolveData) as Hook;
		},
		ownsHook: async (c, hookId: string, token: string) => {
			const runId = c.key[0];
			if (!runId) throw new Error("workflowRun actor key is missing");
			const row = await one(
				c,
				`
				SELECT 1 AS owns_hook
				FROM workflow_hooks
				WHERE run_id = ? AND hook_id = ? AND token = ?
				`,
				runId,
				hookId,
				token,
			);
			return row != null;
		},
		listHooks: async (c, params: ListHooksParams) => {
			if (params.runId) assertRunKey(c, params.runId);
			const limit = params.pagination?.limit ?? 100;
			const cursor = params.pagination?.cursor;
			const rows = await c.db.execute(
				`
				SELECT * FROM workflow_hooks
				WHERE (? IS NULL OR run_id = ?) AND (? IS NULL OR hook_id > ?)
				ORDER BY hook_id ASC
				LIMIT ?
				`,
				params.runId ?? null,
				params.runId ?? null,
				cursor ?? null,
				cursor ?? null,
				limit + 1,
			);
			const values = rows.slice(0, limit);
			return {
				data: values.map((row) =>
					filterHookData(rowToHook(row), params.resolveData),
				),
				cursor: values.at(-1)?.hook_id?.toString() ?? null,
				hasMore: rows.length > limit,
			};
		},
		wake: async (c) => {
			const ctx = asWorkflowContext(c);
			await withSerializedAction(ctx, async () => {
				await prearmRecovery(ctx);
				ctx.vars.wakeArmedAt = null;
				await cleanupExpiredStreams(ctx);
				await drainPendingOps(ctx);
				startDispatchLoop(ctx, ctx.key[0] ?? "__health");
				await finishRecoveryArm(ctx);
			});
		},
		enqueue: (
			c,
			partition: string,
			messageId: string,
			queueName: string,
			runtimeUrl: string,
			body: string,
			headers?: Record<string, string>,
			delaySeconds?: number,
			idempotencyKey?: string,
		) => {
			const ctx = asWorkflowContext(c);
			return withRecoveryWriter(ctx, () =>
				enqueueDispatch(
					ctx,
					partition,
					messageId,
					queueName,
					runtimeUrl,
					body,
					headers,
					delaySeconds,
					idempotencyKey,
				),
			);
		},
		inspectQueue: (c) => inspectQueue(c),
		forceStaleInflightForTesting: (
			c,
			partition: string,
			messageId: string,
			ageMs?: number,
		) =>
			forceStaleInflightForTesting(
				c as unknown as WorkflowContext,
				partition,
				messageId,
				ageMs,
			),
		writeStream: async (
			c,
			streamId: string,
			runId: string,
			chunk: string,
			eof: boolean,
		) => {
			const ctx = asWorkflowContext(c);
			return withRecoveryWriter(ctx, async () => {
				const result = await writeStream(ctx, streamId, runId, chunk, eof);
				if (eof && STREAM_TTL_MS > 0) {
					await ctx.schedule.at(Date.now() + STREAM_TTL_MS, "wake");
				}
				return result;
			});
		},
		getStreamChunks: (
			c,
			streamId: string,
			runId: string,
			startIndex?: number,
			limit?: number,
		) => getStreamChunks(c as unknown as WorkflowContext, streamId, runId, startIndex, limit),
		getStreamInfo: (c, streamId: string, runId: string) =>
			getStreamInfo(c as unknown as WorkflowContext, streamId, runId),
		listStreams: (c, runId: string) =>
			listStreams(c as unknown as WorkflowContext, runId),
		expireClosedStreamForTesting: (c, streamId: string, runId: string) =>
			expireClosedStreamForTesting(
				c as unknown as WorkflowContext,
				streamId,
				runId,
			),
	},
	options: { sleepTimeout: 1000 },
});
