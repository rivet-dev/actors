use anyhow::{Context, Result, bail};

use super::queries::{LOAD_META_TEXT_SQL, UPSERT_META_TEXT_SQL};
use crate::sqlite::{BindParam, ColumnValue, SqliteBatchStatement, SqliteDb};

pub(crate) const INTERNAL_SCHEMA_VERSION: i64 = 1;

const SCHEMA_VERSION_KEY: &str = "schema_version";

// `_rivet_meta` is the bootstrap root created before the numbered migrations.
// `schema_version` cannot live in a table created by those migrations, and
// `kv_import_state` must survive clearing partially imported runtime tables so
// interrupted imports can be detected and retried. Fixed core-owned logical
// metadata may also live here when adding a column would break older runtimes'
// ability to open the database. This is not a general-purpose runtime KV store.
// W[bootstrap + bounded core metadata | point upsert | schedule metadata capped by max_schedules]
pub(crate) const CREATE_META_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS _rivet_meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
) STRICT, WITHOUT ROWID
"#;

// The position and count of entries in this array are the persisted internal
// schema-version history. Once a version ships, changing or reordering an
// existing entry would make the same stored version describe different schemas
// across runtime releases. Rewriting these entries in place is safe only while
// no internal schema version has shipped; after release, all changes must be
// appended as new migrations and INTERNAL_SCHEMA_VERSION must advance.
pub(crate) const MIGRATIONS: &[&[&str]] = &[&[
	// W[queue_next_id per enqueue; alarm per head-change; token once | point UPDATE of one column | <100 B | single-row: all runtime singletons on one leaf]
	r#"
CREATE TABLE _rivet_runtime (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    last_pushed_alarm INTEGER,
    inspector_token   TEXT,
    queue_next_id     INTEGER NOT NULL
) STRICT
"#,
	// W[once at init | single INSERT | input <=256 KiB | COLD: never rewritten; overflow chain isolated from hot state]
	r#"
CREATE TABLE _rivet_actor (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    has_initialized INTEGER NOT NULL,
    input           BLOB
) STRICT
"#,
	// W[debounced save ~1/s + immediate at shutdown | UPDATE state | <=256 KiB | HOT: sole column, so saves dirty only state pages]
	r#"
CREATE TABLE _rivet_actor_state (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    state BLOB NOT NULL
) STRICT
"#,
	// W[per schedule/cancel/fire, immediate | point insert/delete | <200 B | replaces full actor blob rewrite with one row]
	r#"
CREATE TABLE _rivet_schedule_events (
    event_id         TEXT PRIMARY KEY,
    trigger_at       INTEGER NOT NULL,
    action           TEXT NOT NULL,
    args             BLOB,
    kind             INTEGER NOT NULL,
    cron_expression  TEXT,
    timezone         TEXT,
    interval_ms      INTEGER,
    last_started_at  INTEGER,
    max_history      INTEGER NOT NULL
) STRICT, WITHOUT ROWID
"#,
	r#"
CREATE INDEX _rivet_schedule_events_trigger_at
    ON _rivet_schedule_events (trigger_at)
"#,
	// W[per recurring fire | point insert/update/prune | bounded rows]
	r#"
CREATE TABLE _rivet_schedule_history (
    id           INTEGER PRIMARY KEY,
    schedule_id  TEXT NOT NULL,
    action       TEXT NOT NULL,
    scheduled_at INTEGER NOT NULL,
    fired_at     INTEGER NOT NULL,
    finished_at  INTEGER,
    result         INTEGER NOT NULL,
    -- Mirrors the schedule-history subset of the canonical client-visible
    -- RivetError payload. Keep these columns synchronized with ScheduleErrorInfo.
    error_group    TEXT,
    error_code     TEXT,
    error_message  TEXT,
    error_metadata BLOB
) STRICT
"#,
	r#"
CREATE INDEX _rivet_schedule_history_schedule
    ON _rivet_schedule_history (schedule_id, fired_at DESC, id DESC)
"#,
	r#"
CREATE INDEX _rivet_schedule_history_fired_at
    ON _rivet_schedule_history (fired_at DESC, id DESC)
"#,
	r#"
CREATE INDEX _rivet_schedule_history_running
    ON _rivet_schedule_history (result)
    WHERE result = 0
"#,
	// W[once per connect, DELETE on disconnect | whole row | up to 256 KiB | COLD: immutable per conn, separate from hot message index]
	r#"
CREATE TABLE _rivet_conns (
    conn_id         TEXT PRIMARY KEY,
    parameters      BLOB NOT NULL,
    gateway_id      BLOB NOT NULL,
    request_id      BLOB NOT NULL,
    request_path    TEXT NOT NULL,
    request_headers BLOB NOT NULL
) STRICT, WITHOUT ROWID
"#,
	// W[dirty per WS message, written debounced ~1/s; rewritten at sleep | point UPDATE | ~100-300 B | HOT: compact conn state rows]
	r#"
CREATE TABLE _rivet_conn_state (
    conn_id              TEXT PRIMARY KEY,
    state                BLOB NOT NULL,
    server_message_index INTEGER NOT NULL,
    client_message_index INTEGER NOT NULL,
    subscriptions        BLOB NOT NULL
) STRICT, WITHOUT ROWID
"#,
	// W[per enqueue plus queue_next_id; batch DELETE on receive/ack | append/delete + named FIFO lookup | body <=256 KiB | INTEGER PK plus compact (name, id) index keeps bodies out of name scans]
	r#"
CREATE TABLE _rivet_queue (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    body       BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT
"#,
	r#"
CREATE INDEX _rivet_queue_name_id
    ON _rivet_queue (name, id)
"#,
	// RivetKit core owns this table's creation and migrations. External workflow
	// packages are format-compatible clients and must never create or migrate it;
	// any format change requires bidirectional old/new compatibility fixtures.
	// W[per workflow step flush | keyed upsert + range delete | values <=256 KiB | verbatim fdb-tuple keys in one clustered tree]
	r#"
CREATE TABLE _rivet_wf_kv (
    key   BLOB PRIMARY KEY,
    value BLOB NOT NULL
) STRICT, WITHOUT ROWID
"#,
	// W[per c.kv op (deprecated) | keyed upsert/delete/range | values <=128 KiB | verbatim raw KV key bytes]
	r#"
CREATE TABLE _rivet_user_kv (
    key   BLOB PRIMARY KEY,
    value BLOB NOT NULL
) STRICT, WITHOUT ROWID
	"#,
]];

pub(crate) async fn ensure_internal_schema(db: &SqliteDb) -> Result<()> {
	db.execute(CREATE_META_TABLE, None)
		.await
		.context("create rivet internal schema metadata table")?;

	let current_version = read_schema_version(db).await?;
	if current_version > INTERNAL_SCHEMA_VERSION {
		bail!(
			"actor sqlite internal schema version {current_version} is newer than supported version {INTERNAL_SCHEMA_VERSION}"
		);
	}
	if current_version == INTERNAL_SCHEMA_VERSION {
		return Ok(());
	}

	apply_schema_ladder(db, current_version).await
}

async fn apply_schema_ladder(db: &SqliteDb, current_version: i64) -> Result<()> {
	// Apply and record each version atomically so a failed upgrade resumes from
	// the last completed version without relying on backend-specific errors.
	for version in current_version + 1..=INTERNAL_SCHEMA_VERSION {
		db.execute_batch(migration_statements(version - 1, version)?)
			.await
			.with_context(|| format!("apply rivet internal schema migration v{version}"))?;
	}
	Ok(())
}

fn migration_statements(from_version: i64, to_version: i64) -> Result<Vec<SqliteBatchStatement>> {
	if from_version < 0 || to_version < from_version || to_version > INTERNAL_SCHEMA_VERSION {
		bail!("invalid internal schema migration range {from_version}..={to_version}");
	}

	let mut statements = Vec::new();
	for migration in &MIGRATIONS[from_version as usize..to_version as usize] {
		for sql in *migration {
			statements.push(SqliteBatchStatement {
				sql: (*sql).to_owned(),
				params: None,
			});
		}
	}
	statements.push(SqliteBatchStatement {
		sql: UPSERT_META_TEXT_SQL.to_owned(),
		params: Some(vec![
			BindParam::Text(SCHEMA_VERSION_KEY.to_owned()),
			BindParam::Blob(encode_schema_version(to_version)),
		]),
	});
	Ok(statements)
}

async fn read_schema_version(db: &SqliteDb) -> Result<i64> {
	let result = db
		.query(
			LOAD_META_TEXT_SQL,
			Some(vec![BindParam::Text(SCHEMA_VERSION_KEY.to_owned())]),
		)
		.await
		.context("read rivet internal schema version")?;

	let Some(row) = result.rows.first() else {
		return Ok(0);
	};
	let Some(ColumnValue::Blob(value)) = row.first() else {
		bail!("invalid rivet internal schema_version row");
	};
	decode_schema_version(value)
}

fn encode_schema_version(version: i64) -> Vec<u8> {
	version.to_le_bytes().to_vec()
}

fn decode_schema_version(value: &[u8]) -> Result<i64> {
	let bytes: [u8; 8] = value
		.try_into()
		.context("rivet internal schema_version must be an i64 little-endian blob")?;
	Ok(i64::from_le_bytes(bytes))
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn initialize_test_schema(conn: &rusqlite::Connection) -> Result<()> {
	conn.execute_batch(CREATE_META_TABLE)
		.context("create test internal schema metadata table")?;
	for migration in MIGRATIONS {
		for sql in *migration {
			conn.execute_batch(sql)
				.context("apply test internal schema migration")?;
		}
	}
	conn.execute(
		UPSERT_META_TEXT_SQL,
		rusqlite::params![
			SCHEMA_VERSION_KEY,
			encode_schema_version(INTERNAL_SCHEMA_VERSION)
		],
	)
	.context("record test internal schema version")?;
	Ok(())
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../../tests/internal_schema.rs"]
pub(crate) mod tests;
