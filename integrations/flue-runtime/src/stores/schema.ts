import { FLUE_SCHEMA_VERSION, DURABILITY_DEFAULT_MAX_ATTEMPTS } from './constants.js';
import type { AsyncSqlDb, AsyncSqlRunner } from './async-db.js';

// Last synchronized with Flue commit e79d5712820af588094a4e67a3ceafc816770396:
// https://github.com/rivet-dev/flue/tree/e79d5712820af588094a4e67a3ceafc816770396/packages/runtime/src
// DDL sources: sql-agent-execution-store.ts, sql-run-store.ts, sql-attachment-store.ts,
// runtime/event-stream-store.ts, and runtime/conversation-stream-store.ts.
export async function ensureAsyncSqlSchema(db: AsyncSqlDb): Promise<void> {
	await db.transaction(async (tx) => {
		await tx.query(`
			CREATE TABLE IF NOT EXISTS flue_meta (
				key TEXT PRIMARY KEY,
				value TEXT NOT NULL
			)
		`);
		const versionRows = await tx.query(`SELECT value FROM flue_meta WHERE key = 'schema_version'`);
		const storedVersion = versionRows[0]?.value;
		if (storedVersion === undefined || storedVersion === null) {
			await tx.query(`INSERT OR IGNORE INTO flue_meta (key, value) VALUES ('schema_version', ?)`, [
				String(FLUE_SCHEMA_VERSION),
			]);
		} else if (String(storedVersion) !== String(FLUE_SCHEMA_VERSION)) {
			throw new Error(
				`[flue] Persisted store schema version ${String(storedVersion)} is not supported by this adapter.`,
			);
		}

		await ensureTables(tx);
	});
}

async function ensureTables(tx: AsyncSqlRunner): Promise<void> {
	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_sessions (
			id TEXT PRIMARY KEY,
			data TEXT NOT NULL
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_session_entries (
			session_id TEXT NOT NULL,
			entry_id TEXT NOT NULL,
			position INTEGER NOT NULL,
			data TEXT NOT NULL,
			PRIMARY KEY (session_id, entry_id)
		)
	`);
	await tx.query(`
		CREATE INDEX IF NOT EXISTS flue_session_entries_session_position_idx
		ON flue_session_entries (session_id, position ASC)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_image_chunks (
			owner_kind TEXT NOT NULL,
			owner_id TEXT NOT NULL,
			owner_part TEXT NOT NULL,
			image_id TEXT NOT NULL,
			chunk_index INTEGER NOT NULL,
			chunk_count INTEGER NOT NULL,
			data TEXT NOT NULL,
			PRIMARY KEY (owner_kind, owner_id, owner_part, image_id, chunk_index)
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_agent_submissions (
			sequence INTEGER PRIMARY KEY AUTOINCREMENT,
			submission_id TEXT NOT NULL UNIQUE,
			session_key TEXT NOT NULL,
			kind TEXT NOT NULL,
			payload TEXT NOT NULL,
			status TEXT NOT NULL,
			accepted_at INTEGER NOT NULL,
			canonical_ready_at INTEGER,
			attempt_id TEXT,
			input_applied_at INTEGER,
			recovery_requested_at INTEGER,
			abort_requested_at INTEGER,
			started_at INTEGER,
			settled_at INTEGER,
			error TEXT,
			attempt_count INTEGER NOT NULL DEFAULT 0,
			max_retry INTEGER NOT NULL DEFAULT ${DURABILITY_DEFAULT_MAX_ATTEMPTS},
			timeout_at INTEGER NOT NULL DEFAULT 0,
			owner_id TEXT,
			lease_expires_at INTEGER NOT NULL DEFAULT 0,
			settlement_record_id TEXT,
			settlement_record TEXT
		)
	`);
	await tx.query(`
		CREATE INDEX IF NOT EXISTS flue_agent_submissions_status_sequence_idx
		ON flue_agent_submissions (status, sequence ASC)
	`);
	await tx.query(`
		CREATE INDEX IF NOT EXISTS flue_agent_submissions_session_status_sequence_idx
		ON flue_agent_submissions (session_key, status, sequence ASC)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_agent_turn_journals (
			submission_id TEXT PRIMARY KEY,
			session_key TEXT NOT NULL,
			kind TEXT NOT NULL,
			attempt_id TEXT NOT NULL,
			operation_id TEXT NOT NULL,
			turn_id TEXT NOT NULL,
			phase TEXT NOT NULL,
			revision INTEGER NOT NULL,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL,
			checkpoint_leaf_id TEXT,
			tool_request_json TEXT,
			stream_key TEXT,
			stream_consumed_at INTEGER,
			committed INTEGER NOT NULL DEFAULT 0,
			committed_leaf_id TEXT
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_agent_stream_chunks (
			stream_key TEXT NOT NULL,
			segment_index INTEGER NOT NULL,
			body TEXT NOT NULL,
			PRIMARY KEY (stream_key, segment_index)
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_agent_session_deletions (
			session_key TEXT PRIMARY KEY,
			started_at INTEGER NOT NULL
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_agent_dispatch_receipts (
			dispatch_id TEXT PRIMARY KEY,
			accepted_at INTEGER NOT NULL
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_agent_attempt_markers (
			submission_id TEXT NOT NULL,
			attempt_id TEXT NOT NULL,
			created_at INTEGER NOT NULL,
			PRIMARY KEY (submission_id, attempt_id)
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_runs (
			run_id TEXT PRIMARY KEY,
			workflow_name TEXT NOT NULL,
			status TEXT NOT NULL,
			started_at TEXT NOT NULL,
			payload TEXT,
			traceparent TEXT,
			tracestate TEXT,
			ended_at TEXT,
			is_error INTEGER,
			duration_ms INTEGER,
			result TEXT,
			error TEXT
		)
	`);
	await tx.query(`
		CREATE INDEX IF NOT EXISTS flue_runs_workflow_started_idx
		ON flue_runs (workflow_name, started_at DESC)
	`);
	await tx.query(`
		CREATE INDEX IF NOT EXISTS flue_runs_status_started_idx
		ON flue_runs (status, started_at DESC, run_id DESC)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_event_streams (
			path TEXT PRIMARY KEY,
			next_offset INTEGER NOT NULL DEFAULT 0,
			closed INTEGER NOT NULL DEFAULT 0
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_event_stream_entries (
			path TEXT NOT NULL,
			seq INTEGER NOT NULL,
			data TEXT NOT NULL,
			PRIMARY KEY (path, seq)
		)
	`);
	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_event_stream_keys (
			path TEXT NOT NULL,
			key TEXT NOT NULL,
			seq INTEGER NOT NULL,
			data TEXT NOT NULL,
			PRIMARY KEY (path, key)
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_conversation_streams (
			path TEXT PRIMARY KEY,
			identity_json TEXT NOT NULL,
			producer_id TEXT,
			producer_epoch INTEGER NOT NULL DEFAULT 0,
			next_producer_sequence INTEGER NOT NULL DEFAULT 0,
			next_offset INTEGER NOT NULL DEFAULT 0,
			incarnation TEXT NOT NULL
		)
	`);
	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_conversation_stream_batches (
			path TEXT NOT NULL,
			seq INTEGER NOT NULL,
			producer_id TEXT NOT NULL,
			producer_epoch INTEGER NOT NULL,
			producer_sequence INTEGER NOT NULL,
			data TEXT NOT NULL,
			submission_id TEXT,
			attempt_id TEXT,
			PRIMARY KEY (path, seq),
			UNIQUE (path, producer_id, producer_epoch, producer_sequence)
		)
	`);

	await tx.query(`
		CREATE TABLE IF NOT EXISTS flue_attachments (
			stream_path TEXT NOT NULL,
			attachment_id TEXT NOT NULL,
			conversation_id TEXT NOT NULL,
			mime_type TEXT NOT NULL,
			filename TEXT,
			size INTEGER NOT NULL,
			digest TEXT NOT NULL,
			data TEXT NOT NULL,
			PRIMARY KEY (stream_path, attachment_id)
		)
	`);
}
