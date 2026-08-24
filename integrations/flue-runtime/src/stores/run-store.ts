import type {
	CreateRunInput,
	EndRunInput,
	ListRunsOpts,
	ListRunsResponse,
	RunPointer,
	RunRecord,
	RunStatus,
	RunStore,
	WorkflowRunPointer,
} from '@flue/runtime/adapter-kit';
import type { AsyncSqlDb, AsyncSqlRow } from './async-db.js';
import { DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT } from './constants.js';
import { clampLimit } from './helpers.js';

export function createAsyncRunStore(db: AsyncSqlDb): RunStore {
	return new AsyncRunStore(db);
}

class AsyncRunStore implements RunStore {
	private db: AsyncSqlDb;

	constructor(db: AsyncSqlDb) {
		this.db = db;
	}

	async createRun(input: CreateRunInput): Promise<void> {
		await this.db.query(
			`INSERT OR IGNORE INTO flue_runs
			 (run_id, workflow_name, status, started_at, payload, traceparent, tracestate,
			  ended_at, is_error, duration_ms, result, error)
			 VALUES (?, ?, 'active', ?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL)`,
			[input.runId, input.workflowName, input.startedAt, serializeJson(input.input),
				input.traceCarrier?.traceparent ?? null, input.traceCarrier?.tracestate ?? null],
		);
	}

	async endRun(input: EndRunInput): Promise<void> {
		await this.db.query(
			`UPDATE flue_runs
			 SET status = ?, ended_at = ?, is_error = ?, duration_ms = ?, result = ?, error = ?
			 WHERE run_id = ?`,
			[
				input.isError ? 'errored' : 'completed',
				input.endedAt,
				input.isError ? 1 : 0,
				input.durationMs,
				serializeJson(input.result),
				serializeJson(input.error),
				input.runId,
			],
		);
	}

	async getRun(runId: string): Promise<RunRecord | null> {
		const rows = await this.db.query('SELECT * FROM flue_runs WHERE run_id = ? LIMIT 1', [runId]);
		const row = rows[0];
		return row ? rowToRunRecord(row) : null;
	}

	async lookupRun(runId: string): Promise<WorkflowRunPointer | null> {
		const rows = await this.db.query(
			`SELECT run_id, workflow_name
			 FROM flue_runs WHERE run_id = ? LIMIT 1`,
			[runId],
		);
		const row = rows[0];
		return row ? { runId: String(row.run_id), workflowName: String(row.workflow_name) } : null;
	}

	async listRuns(opts: ListRunsOpts = {}): Promise<ListRunsResponse> {
		const limit = clampLimit(opts.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
		const cursor = decodeRunCursor(opts.cursor);
		const wheres: string[] = [];
		const bindings: Array<string | number> = [];
		if (opts.status) {
			wheres.push('status = ?');
			bindings.push(opts.status);
		}
		if (opts.workflowName) {
			wheres.push('workflow_name = ?');
			bindings.push(opts.workflowName);
		}
		if (cursor) {
			wheres.push('(started_at < ? OR (started_at = ? AND run_id < ?))');
			bindings.push(cursor.startedAt, cursor.startedAt, cursor.runId);
		}
		const where = wheres.length > 0 ? `WHERE ${wheres.join(' AND ')}` : '';
		const rows = await this.db.query(
			`SELECT run_id, workflow_name, status, started_at, ended_at, duration_ms, is_error
			 FROM flue_runs ${where}
			 ORDER BY started_at DESC, run_id DESC
			 LIMIT ?`,
			[...bindings, limit + 1],
		);
		const hasMore = rows.length > limit;
		const page = (hasMore ? rows.slice(0, limit) : rows).map(rowToRunPointer);
		const last = page.at(-1);
		return { runs: page, nextCursor: hasMore && last ? encodeRunCursor(last) : undefined };
	}
}

function serializeJson(value: unknown): string | null {
	return JSON.stringify(value) ?? null;
}

function rowToRunRecord(row: AsyncSqlRow): RunRecord {
	return {
		runId: String(row.run_id),
		workflowName: String(row.workflow_name),
		status: String(row.status) as RunStatus,
		startedAt: String(row.started_at),
		...(typeof row.payload === 'string' ? { input: JSON.parse(row.payload) as unknown } : {}),
		...(typeof row.traceparent === 'string' ? { traceCarrier: {
			traceparent: row.traceparent,
			...(typeof row.tracestate === 'string' ? { tracestate: row.tracestate } : {}),
		} } : {}),
		...(typeof row.ended_at === 'string' ? { endedAt: row.ended_at } : {}),
		...(row.is_error === null || row.is_error === undefined ? {} : { isError: Boolean(row.is_error) }),
		...(row.duration_ms != null ? { durationMs: Number(row.duration_ms) } : {}),
		...(typeof row.result === 'string' ? { result: JSON.parse(row.result) as unknown } : {}),
		...(typeof row.error === 'string' ? { error: JSON.parse(row.error) as unknown } : {}),
	};
}

function rowToRunPointer(row: AsyncSqlRow): RunPointer {
	return {
		runId: String(row.run_id),
		workflowName: String(row.workflow_name),
		status: String(row.status) as RunStatus,
		startedAt: String(row.started_at),
		...(typeof row.ended_at === 'string' ? { endedAt: row.ended_at } : {}),
		...(row.duration_ms != null ? { durationMs: Number(row.duration_ms) } : {}),
		...(row.is_error === null || row.is_error === undefined ? {} : { isError: Boolean(row.is_error) }),
	};
}

interface RunCursor {
	startedAt: string;
	runId: string;
}

function encodeRunCursor(pointer: { startedAt: string; runId: string }): string {
	return Buffer.from(JSON.stringify({ s: pointer.startedAt, r: pointer.runId })).toString(
		'base64url',
	);
}

function decodeRunCursor(cursor: string | undefined): RunCursor | undefined {
	if (!cursor) return undefined;
	try {
		const decoded = JSON.parse(Buffer.from(cursor, 'base64url').toString('utf8')) as {
			s?: unknown;
			r?: unknown;
		};
		if (typeof decoded.s === 'string' && typeof decoded.r === 'string') {
			return { startedAt: decoded.s, runId: decoded.r };
		}
	} catch {}
	return undefined;
}
