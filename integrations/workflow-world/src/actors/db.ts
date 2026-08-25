import { db } from "rivetkit/db";
import {
	migrations,
	type MigrationDatabase,
} from "rivetkit/unstable/migrations";

const bootstrapWorkflowDb = async (database: MigrationDatabase) => {
	await database.execute(`
		CREATE TABLE IF NOT EXISTS workflow_runs (
			run_id TEXT PRIMARY KEY,
			run_revision INTEGER NOT NULL DEFAULT 0,
			status TEXT NOT NULL,
			deployment_id TEXT NOT NULL,
			workflow_name TEXT NOT NULL,
			spec_version INTEGER,
			execution_context BLOB,
			attributes BLOB,
			input BLOB,
			output BLOB,
			error BLOB,
			expired_at INTEGER,
			started_at INTEGER,
			completed_at INTEGER,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL
		)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS workflow_events (
			event_id TEXT PRIMARY KEY,
			run_id TEXT NOT NULL,
			event_type TEXT NOT NULL,
			correlation_id TEXT,
			event_data BLOB,
			spec_version INTEGER,
			created_at INTEGER NOT NULL
		)
	`);
	await database.execute(`
		CREATE UNIQUE INDEX IF NOT EXISTS uq_events_create_step
		ON workflow_events(correlation_id)
		WHERE event_type = 'step_created'
	`);
	await database.execute(`
		CREATE UNIQUE INDEX IF NOT EXISTS uq_events_create_hook
		ON workflow_events(correlation_id)
		WHERE event_type = 'hook_created'
	`);
	await database.execute(`
		CREATE UNIQUE INDEX IF NOT EXISTS uq_events_create_wait
		ON workflow_events(correlation_id)
		WHERE event_type = 'wait_created'
	`);
	// Creation-event dedup (SPEC §3.2). IMPORTANT: every index here is scoped to a
	// SINGLE workflowRun actor's own `c.db` — there is one `workflow_events` table
	// per run. `uq_events_create_run` enforces "one run_created per database",
	// which is correct ONLY because each run owns its database. Do NOT move these
	// tables into a shared/global database without re-scoping these indexes by
	// `run_id`, or the run-singleton guarantee silently collapses.
	await database.execute(`
		CREATE UNIQUE INDEX IF NOT EXISTS uq_events_create_run
		ON workflow_events(event_type)
		WHERE event_type = 'run_created'
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS workflow_steps (
			step_id TEXT PRIMARY KEY,
			run_id TEXT NOT NULL,
			step_name TEXT NOT NULL,
			status TEXT NOT NULL,
			input BLOB,
			output BLOB,
			error BLOB,
			attempt INTEGER NOT NULL,
			started_at INTEGER,
			completed_at INTEGER,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL,
			retry_after INTEGER,
			spec_version INTEGER
		)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS workflow_hooks (
			hook_id TEXT PRIMARY KEY,
			run_id TEXT NOT NULL,
			token TEXT NOT NULL,
			token_generation INTEGER,
			owner_id TEXT NOT NULL,
			project_id TEXT NOT NULL,
			environment TEXT NOT NULL,
			metadata BLOB,
			created_at INTEGER NOT NULL,
			spec_version INTEGER,
			is_webhook INTEGER
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS idx_workflow_hooks_token
		ON workflow_hooks(token)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS workflow_waits (
			wait_id TEXT PRIMARY KEY,
			run_id TEXT NOT NULL,
			status TEXT NOT NULL,
			resume_at INTEGER,
			runtime_url TEXT,
			queue_name TEXT,
			headers TEXT,
			resume_enqueued_at INTEGER,
			completed_at INTEGER,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL,
			spec_version INTEGER
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS idx_workflow_waits_due
		ON workflow_waits(status, resume_at)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS idx_workflow_events_correlation
		ON workflow_events(correlation_id, event_id)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS dispatcher_queue (
			message_id TEXT PRIMARY KEY,
			queue_name TEXT NOT NULL,
			route TEXT NOT NULL,
			runtime_url TEXT NOT NULL,
			body TEXT NOT NULL,
			headers TEXT,
			idem_key TEXT,
			status TEXT NOT NULL,
			attempt INTEGER NOT NULL DEFAULT 0,
			next_at INTEGER NOT NULL,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL,
			started_at INTEGER,
			finished_at INTEGER,
			http_status INTEGER,
			error TEXT
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS idx_dispatcher_queue_ready
		ON dispatcher_queue(status, next_at, created_at)
	`);
	await database.execute(`
		CREATE UNIQUE INDEX IF NOT EXISTS uq_dispatcher_queue_idem
		ON dispatcher_queue(idem_key) WHERE idem_key IS NOT NULL
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS stream_chunks (
			stream_id TEXT NOT NULL,
			sequence INTEGER NOT NULL,
			chunk_data BLOB,
			eof INTEGER NOT NULL,
			created_at INTEGER NOT NULL,
			PRIMARY KEY (stream_id, sequence)
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS idx_stream_chunks_stream
		ON stream_chunks(stream_id, sequence)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS run_streams (
			stream_id TEXT PRIMARY KEY
		)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS pending_ops (
			op_id TEXT PRIMARY KEY,
			op_type TEXT NOT NULL,
			payload BLOB NOT NULL,
			operation_revision INTEGER NOT NULL,
			status TEXT NOT NULL CHECK (status IN ('ready', 'inflight')),
			claim_id TEXT,
			next_at INTEGER NOT NULL,
			attempt INTEGER NOT NULL DEFAULT 0,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS idx_pending_ops_due
		ON pending_ops(status, next_at, created_at)
	`);
};

const bootstrapCoordinatorDb = async (database: MigrationDatabase) => {
	await database.execute(`
		CREATE TABLE IF NOT EXISTS runs_index (
			run_id TEXT PRIMARY KEY,
			run_revision INTEGER NOT NULL,
			status TEXT NOT NULL,
			deployment_id TEXT NOT NULL,
			workflow_name TEXT NOT NULL,
			spec_version INTEGER,
			execution_context BLOB,
			attributes BLOB,
			input BLOB,
			output BLOB,
			error BLOB,
			expired_at INTEGER,
			started_at INTEGER,
			completed_at INTEGER,
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS runs_index_list
		ON runs_index(created_at DESC, run_id DESC)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS runs_index_name_list
		ON runs_index(workflow_name, created_at DESC, run_id DESC)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS runs_index_status_list
		ON runs_index(status, created_at DESC, run_id DESC)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS runs_index_name_status_list
		ON runs_index(workflow_name, status, created_at DESC, run_id DESC)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS correlation_index (
			correlation_id TEXT PRIMARY KEY,
			run_id TEXT NOT NULL,
			kind TEXT NOT NULL CHECK (kind IN ('step', 'hook', 'wait')),
			run_revision INTEGER NOT NULL,
			status TEXT NOT NULL CHECK (status IN ('active', 'deleted'))
		)
	`);
	await database.execute(`
		CREATE TABLE IF NOT EXISTS hooks_index (
			hook_id TEXT PRIMARY KEY,
			run_id TEXT NOT NULL,
			run_revision INTEGER NOT NULL,
			token TEXT NOT NULL,
			status TEXT NOT NULL CHECK (status IN ('active', 'deleted')),
			created_at INTEGER NOT NULL,
			updated_at INTEGER NOT NULL
		)
	`);
	await database.execute(`
		CREATE INDEX IF NOT EXISTS hooks_index_list
		ON hooks_index(status, created_at DESC, hook_id DESC)
	`);
};

const bootstrapHookTokenDb = async (database: MigrationDatabase) => {
	await database.execute(`
		CREATE TABLE IF NOT EXISTS hook_token_state (
			singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
			generation INTEGER NOT NULL,
			run_id TEXT,
			hook_id TEXT,
			status TEXT NOT NULL CHECK (status IN ('empty', 'pending', 'confirmed')),
			created_at INTEGER,
			updated_at INTEGER NOT NULL,
			expires_at INTEGER
		)
	`);
	await database.execute(
		`INSERT OR IGNORE INTO hook_token_state (
			singleton, generation, status, updated_at
		) VALUES (1, 0, 'empty', ?)`,
		Date.now(),
	);
};

// Durable schema-version trackers. A renamed table reads as version 0 and
// replays the ladder, so these names must not change.
export const migrateWorkflowDb = migrations({
	tableName: "vercel_world_workflow_schema_version",
	migrations: [{ version: 1, up: bootstrapWorkflowDb }],
});
export const migrateCoordinatorDb = migrations({
	tableName: "vercel_world_coordinator_schema_version",
	migrations: [{ version: 1, up: bootstrapCoordinatorDb }],
});
export const migrateHookTokenDb = migrations({
	tableName: "vercel_world_hook_token_schema_version",
	migrations: [{ version: 1, up: bootstrapHookTokenDb }],
});
export const workflowDb = db({ onMigrate: migrateWorkflowDb });
export const coordinatorDb = db({ onMigrate: migrateCoordinatorDb });
export const hookTokenDb = db({ onMigrate: migrateHookTokenDb });
