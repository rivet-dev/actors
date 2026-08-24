import { WorkflowRunNotFoundError } from "@workflow/errors";
import {
	EventSchema,
	HookSchema,
	StepSchema,
	WorkflowRunSchema,
	type Hook,
	type Wait,
} from "@workflow/world";
import { compact, decodeValue } from "../codec.js";

export type Db = {
	execute(sql: string, ...args: unknown[]): Promise<Record<string, unknown>[]>;
	transaction<T>(callback: (tx: Db) => Promise<T> | T): Promise<T>;
};

export type Ctx = { db: Db };

export async function withActorTransaction<T>(
	c: Ctx,
	fn: (tx: Ctx) => Promise<T>,
): Promise<T> {
	return c.db.transaction((db) => fn({ db }));
}

export function toDate(value: unknown): Date | undefined {
	return value == null ? undefined : new Date(Number(value));
}

export function toMs(value: unknown): number | null {
	if (value == null) return null;
	if (value instanceof Date) return value.getTime();
	return new Date(value as string | number).getTime();
}

export function rowToRun(row: Record<string, unknown>) {
	return WorkflowRunSchema.parse(
		compact({
			runId: row.run_id,
			status: row.status,
			deploymentId: row.deployment_id,
			workflowName: row.workflow_name,
			specVersion:
				row.spec_version == null ? undefined : Number(row.spec_version),
			executionContext: decodeValue(row.execution_context),
			attributes: decodeValue(row.attributes) ?? {},
			input: decodeValue(row.input),
			output: decodeValue(row.output),
			error: decodeValue(row.error),
			expiredAt: toDate(row.expired_at),
			startedAt: toDate(row.started_at),
			completedAt: toDate(row.completed_at),
			createdAt: toDate(row.created_at),
			updatedAt: toDate(row.updated_at),
		}),
	);
}

export function rowToEvent(row: Record<string, unknown>) {
	return EventSchema.parse(
		compact({
			runId: row.run_id,
			eventId: row.event_id,
			eventType: row.event_type,
			correlationId: row.correlation_id,
			eventData: decodeValue(row.event_data),
			specVersion:
				row.spec_version == null ? undefined : Number(row.spec_version),
			createdAt: toDate(row.created_at),
		}),
	);
}

export function rowToStep(row: Record<string, unknown>) {
	return StepSchema.parse(
		compact({
			runId: row.run_id,
			stepId: row.step_id,
			stepName: row.step_name,
			status: row.status,
			input: decodeValue(row.input),
			output: decodeValue(row.output),
			error: decodeValue(row.error),
			attempt: Number(row.attempt),
			startedAt: toDate(row.started_at),
			completedAt: toDate(row.completed_at),
			createdAt: toDate(row.created_at),
			updatedAt: toDate(row.updated_at),
			retryAfter: toDate(row.retry_after),
			specVersion:
				row.spec_version == null ? undefined : Number(row.spec_version),
		}),
	);
}

export function rowToHook(row: Record<string, unknown>) {
	const parsed = HookSchema.parse(
		compact({
			runId: row.run_id,
			hookId: row.hook_id,
			token: row.token,
			ownerId: row.owner_id,
			projectId: row.project_id,
			environment: row.environment,
			metadata: decodeValue(row.metadata),
			createdAt: toDate(row.created_at),
			specVersion:
				row.spec_version == null ? undefined : Number(row.spec_version),
			isWebhook: row.is_webhook == null ? undefined : Boolean(row.is_webhook),
		}),
	);
	parsed.isWebhook ??= true;
	return parsed;
}

export function rowToWait(row: Record<string, unknown>): Wait {
	return compact({
		waitId: row.wait_id,
		runId: row.run_id,
		status: row.status,
		resumeAt: toDate(row.resume_at),
		completedAt: toDate(row.completed_at),
		createdAt: toDate(row.created_at),
		updatedAt: toDate(row.updated_at),
		specVersion: row.spec_version == null ? undefined : Number(row.spec_version),
	}) as Wait;
}

export function filterData<T extends { input?: unknown; output?: unknown }>(
	value: T,
	resolveData?: string,
) {
	if (resolveData !== "none") return value;
	return { ...value, input: undefined, output: undefined };
}

export function filterHookData<T extends { metadata?: unknown }>(
	value: T,
	resolveData?: string,
) {
	if (resolveData !== "none") return value;
	return { ...value, metadata: undefined };
}

export async function one(c: Ctx, sql: string, ...args: unknown[]) {
	return (await c.db.execute(sql, ...args))[0];
}

export async function requireRun(c: Ctx, runId: string) {
	const row = await one(c, "SELECT * FROM workflow_runs WHERE run_id = ?", runId);
	if (!row) throw new WorkflowRunNotFoundError(runId);
	return rowToRun(row);
}
