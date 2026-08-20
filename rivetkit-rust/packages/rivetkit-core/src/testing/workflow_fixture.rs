//! Versioned logical SQLite fixtures for workflow upgrade tests.
//!
//! This module is compiled only for tests or with `test-support`. It deliberately
//! exposes no production actor-database import path: callers can dump or restore
//! only the fixed Rivet-owned tables represented by [`WorkflowFixture`].

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use vbare::OwnedVersionedData;

use crate::ActorContext;
use crate::actor::keys::WORKFLOW_STORAGE_PREFIX;
use crate::sqlite::{BindParam, ColumnValue, SqliteBatchStatement};

const FIXTURE_VERSION: u16 = 1;
const SCHEMA_VERSION_META_KEY: &str = "schema_version";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureMetadata {
	pub fixture_name: String,
	pub source_rivetkit_version: String,
	pub source_workflow_version: String,
	pub source_revision: String,
	pub actor_id: String,
	pub registry_key: String,
	pub internal_schema_version: i64,
	pub fake_clock_seed: u64,
	pub generated_id_seed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureRuntimeRow {
	pub last_pushed_alarm: Option<i64>,
	pub inspector_token: Option<String>,
	pub queue_next_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureMetaRow {
	pub key: String,
	pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureActorRow {
	pub has_initialized: i64,
	pub input: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureWorkflowRow {
	pub key: Vec<u8>,
	pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureQueueRow {
	pub id: i64,
	pub name: String,
	pub body: Vec<u8>,
	pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureScheduleEventRow {
	pub event_id: String,
	pub trigger_at: i64,
	pub action: String,
	pub args: Option<Vec<u8>>,
	pub kind: i64,
	pub cron_expression: Option<String>,
	pub timezone: Option<String>,
	pub interval_ms: Option<i64>,
	pub last_started_at: Option<i64>,
	pub max_history: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixtureScheduleHistoryRow {
	pub id: i64,
	pub schedule_id: String,
	pub action: String,
	pub scheduled_at: i64,
	pub fired_at: i64,
	pub finished_at: Option<i64>,
	pub result: i64,
	pub error_group: Option<String>,
	pub error_code: Option<String>,
	pub error_message: Option<String>,
	pub error_metadata: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowFixture {
	pub metadata: WorkflowFixtureMetadata,
	pub meta_rows: Vec<WorkflowFixtureMetaRow>,
	pub runtime: Option<WorkflowFixtureRuntimeRow>,
	pub actor: Option<WorkflowFixtureActorRow>,
	pub actor_state: Option<Vec<u8>>,
	pub workflow_rows: Vec<WorkflowFixtureWorkflowRow>,
	pub queue_rows: Vec<WorkflowFixtureQueueRow>,
	pub schedule_events: Vec<WorkflowFixtureScheduleEventRow>,
	pub schedule_history: Vec<WorkflowFixtureScheduleHistoryRow>,
}

enum VersionedWorkflowFixture {
	V1(WorkflowFixture),
}

impl OwnedVersionedData for VersionedWorkflowFixture {
	type Latest = WorkflowFixture;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V1(fixture) => Ok(fixture),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			FIXTURE_VERSION => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("unsupported workflow fixture version {version}"),
		}
	}

	fn serialize_version(self, version: u16) -> Result<Vec<u8>> {
		match (self, version) {
			(Self::V1(fixture), FIXTURE_VERSION) => {
				serde_bare::to_vec(&fixture).map_err(Into::into)
			}
			(_, version) => bail!("unsupported workflow fixture version {version}"),
		}
	}
}

impl WorkflowFixture {
	pub fn encode(&self) -> Result<Vec<u8>> {
		VersionedWorkflowFixture::wrap_latest(self.clone())
			.serialize_with_embedded_version(FIXTURE_VERSION)
	}

	pub fn decode(bytes: &[u8]) -> Result<Self> {
		VersionedWorkflowFixture::deserialize_with_embedded_version(bytes)
	}
}

/// Dumps the fixed set of Rivet-owned logical rows required to resume a
/// workflow. Every query has an explicit ordering so equivalent databases
/// produce byte-identical fixtures regardless of SQLite page layout.
pub async fn dump_workflow_fixture(
	ctx: &ActorContext,
	metadata: WorkflowFixtureMetadata,
) -> Result<WorkflowFixture> {
	let db = ctx.sql();
	let meta_rows = db
		.query("SELECT key, value FROM _rivet_meta ORDER BY key", None)
		.await
		.context("dump workflow fixture metadata rows")?
		.rows
		.iter()
		.map(|row| {
			Ok(WorkflowFixtureMetaRow {
				key: text(row, 0, "metadata key")?,
				value: blob(row, 1, "metadata value")?,
			})
		})
		.collect::<Result<Vec<_>>>()?;
	let runtime = db
		.query(
			"SELECT last_pushed_alarm, inspector_token, queue_next_id FROM _rivet_runtime WHERE id = 1",
			None,
		)
		.await
		.context("dump workflow fixture runtime")?
		.rows
		.first()
		.map(|row| {
			Ok::<_, anyhow::Error>(WorkflowFixtureRuntimeRow {
				last_pushed_alarm: optional_integer(row, 0, "last_pushed_alarm")?,
				inspector_token: optional_text(row, 1, "inspector_token")?,
				queue_next_id: integer(row, 2, "queue_next_id")?,
			})
		})
		.transpose()?;
	let actor = db
		.query(
			"SELECT has_initialized, input FROM _rivet_actor WHERE id = 1",
			None,
		)
		.await
		.context("dump workflow fixture actor")?
		.rows
		.first()
		.map(|row| {
			Ok::<_, anyhow::Error>(WorkflowFixtureActorRow {
				has_initialized: integer(row, 0, "has_initialized")?,
				input: optional_blob(row, 1, "input")?,
			})
		})
		.transpose()?;
	let actor_state = db
		.query("SELECT state FROM _rivet_actor_state WHERE id = 1", None)
		.await
		.context("dump workflow fixture actor state")?
		.rows
		.first()
		.map(|row| blob(row, 0, "state"))
		.transpose()?;

	let workflow_rows = db
		.query("SELECT key, value FROM _rivet_wf_kv ORDER BY key", None)
		.await
		.context("dump workflow fixture history")?
		.rows
		.iter()
		.map(|row| {
			Ok(WorkflowFixtureWorkflowRow {
				key: blob(row, 0, "workflow key")?,
				value: blob(row, 1, "workflow value")?,
			})
		})
		.collect::<Result<Vec<_>>>()?;
	let queue_rows = db
		.query(
			"SELECT id, name, body, created_at FROM _rivet_queue ORDER BY id",
			None,
		)
		.await
		.context("dump workflow fixture queue")?
		.rows
		.iter()
		.map(|row| {
			Ok(WorkflowFixtureQueueRow {
				id: integer(row, 0, "queue id")?,
				name: text(row, 1, "queue name")?,
				body: blob(row, 2, "queue body")?,
				created_at: integer(row, 3, "queue created_at")?,
			})
		})
		.collect::<Result<Vec<_>>>()?;
	let schedule_events = db
		.query(
			"SELECT event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history FROM _rivet_schedule_events ORDER BY event_id",
			None,
		)
		.await
		.context("dump workflow fixture schedule events")?
		.rows
		.iter()
		.map(|row| {
			Ok(WorkflowFixtureScheduleEventRow {
				event_id: text(row, 0, "schedule event_id")?,
				trigger_at: integer(row, 1, "schedule trigger_at")?,
				action: text(row, 2, "schedule action")?,
				args: optional_blob(row, 3, "schedule args")?,
				kind: integer(row, 4, "schedule kind")?,
				cron_expression: optional_text(row, 5, "schedule cron_expression")?,
				timezone: optional_text(row, 6, "schedule timezone")?,
				interval_ms: optional_integer(row, 7, "schedule interval_ms")?,
				last_started_at: optional_integer(row, 8, "schedule last_started_at")?,
				max_history: integer(row, 9, "schedule max_history")?,
			})
		})
		.collect::<Result<Vec<_>>>()?;
	let schedule_history = db
		.query(
			"SELECT id, schedule_id, action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata FROM _rivet_schedule_history ORDER BY id",
			None,
		)
		.await
		.context("dump workflow fixture schedule history")?
		.rows
		.iter()
		.map(|row| {
			Ok(WorkflowFixtureScheduleHistoryRow {
				id: integer(row, 0, "schedule history id")?,
				schedule_id: text(row, 1, "schedule history schedule_id")?,
				action: text(row, 2, "schedule history action")?,
				scheduled_at: integer(row, 3, "schedule history scheduled_at")?,
				fired_at: integer(row, 4, "schedule history fired_at")?,
				finished_at: optional_integer(row, 5, "schedule history finished_at")?,
				result: integer(row, 6, "schedule history result")?,
				error_group: optional_text(row, 7, "schedule history error_group")?,
				error_code: optional_text(row, 8, "schedule history error_code")?,
				error_message: optional_text(row, 9, "schedule history error_message")?,
				error_metadata: optional_blob(row, 10, "schedule history error_metadata")?,
			})
		})
		.collect::<Result<Vec<_>>>()?;

	Ok(WorkflowFixture {
		metadata,
		meta_rows,
		runtime,
		actor,
		actor_state,
		workflow_rows,
		queue_rows,
		schedule_events,
		schedule_history,
	})
}

/// Restores a decoded fixture into an empty test actor database. This function
/// is unavailable without `test-support` and accepts no caller-provided SQL.
pub async fn restore_workflow_fixture(ctx: &ActorContext, fixture: &WorkflowFixture) -> Result<()> {
	validate_fixture_for_restore(fixture)?;
	let mut statements = vec![
		statement("DELETE FROM _rivet_schedule_history", None),
		statement("DELETE FROM _rivet_schedule_events", None),
		statement("DELETE FROM _rivet_queue", None),
		statement("DELETE FROM _rivet_wf_kv", None),
		statement("DELETE FROM _rivet_actor_state", None),
		statement("DELETE FROM _rivet_actor", None),
		statement("DELETE FROM _rivet_runtime", None),
		statement("DELETE FROM _rivet_meta", None),
	];
	for row in &fixture.meta_rows {
		statements.push(statement(
			"INSERT INTO _rivet_meta (key, value) VALUES (?, ?)",
			Some(vec![
				BindParam::Text(row.key.clone()),
				BindParam::Blob(row.value.clone()),
			]),
		));
	}

	if let Some(row) = &fixture.runtime {
		statements.push(statement(
			"INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, ?, ?, ?)",
			Some(vec![
				optional_integer_param(row.last_pushed_alarm),
				optional_text_param(row.inspector_token.clone()),
				BindParam::Integer(row.queue_next_id),
			]),
		));
	}
	if let Some(row) = &fixture.actor {
		statements.push(statement(
			"INSERT INTO _rivet_actor (id, has_initialized, input) VALUES (1, ?, ?)",
			Some(vec![
				BindParam::Integer(row.has_initialized),
				optional_blob_param(row.input.clone()),
			]),
		));
	}
	if let Some(state) = &fixture.actor_state {
		statements.push(statement(
			"INSERT INTO _rivet_actor_state (id, state) VALUES (1, ?)",
			Some(vec![BindParam::Blob(state.clone())]),
		));
	}
	for row in &fixture.workflow_rows {
		statements.push(statement(
			"INSERT INTO _rivet_wf_kv (key, value) VALUES (?, ?)",
			Some(vec![
				BindParam::Blob(row.key.clone()),
				BindParam::Blob(row.value.clone()),
			]),
		));
	}
	for row in &fixture.queue_rows {
		statements.push(statement(
			"INSERT INTO _rivet_queue (id, name, body, created_at) VALUES (?, ?, ?, ?)",
			Some(vec![
				BindParam::Integer(row.id),
				BindParam::Text(row.name.clone()),
				BindParam::Blob(row.body.clone()),
				BindParam::Integer(row.created_at),
			]),
		));
	}
	for row in &fixture.schedule_events {
		statements.push(statement(
			"INSERT INTO _rivet_schedule_events (event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
			Some(vec![
				BindParam::Text(row.event_id.clone()),
				BindParam::Integer(row.trigger_at),
				BindParam::Text(row.action.clone()),
				optional_blob_param(row.args.clone()),
				BindParam::Integer(row.kind),
				optional_text_param(row.cron_expression.clone()),
				optional_text_param(row.timezone.clone()),
				optional_integer_param(row.interval_ms),
				optional_integer_param(row.last_started_at),
				BindParam::Integer(row.max_history),
			]),
		));
	}
	for row in &fixture.schedule_history {
		statements.push(statement(
			"INSERT INTO _rivet_schedule_history (id, schedule_id, action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
			Some(vec![
				BindParam::Integer(row.id),
				BindParam::Text(row.schedule_id.clone()),
				BindParam::Text(row.action.clone()),
				BindParam::Integer(row.scheduled_at),
				BindParam::Integer(row.fired_at),
				optional_integer_param(row.finished_at),
				BindParam::Integer(row.result),
				optional_text_param(row.error_group.clone()),
				optional_text_param(row.error_code.clone()),
				optional_text_param(row.error_message.clone()),
				optional_blob_param(row.error_metadata.clone()),
			]),
		));
	}

	ctx.sql()
		.execute_batch(statements)
		.await
		.context("restore logical workflow fixture")?;
	Ok(())
}

fn validate_fixture_for_restore(fixture: &WorkflowFixture) -> Result<()> {
	let schema_row = fixture
		.meta_rows
		.iter()
		.find(|row| row.key == SCHEMA_VERSION_META_KEY)
		.context("workflow fixture is missing its schema_version metadata row")?;
	let schema_bytes: [u8; 8] = schema_row
		.value
		.as_slice()
		.try_into()
		.context("workflow fixture schema_version must be an i64 little-endian blob")?;
	let stored_schema_version = i64::from_le_bytes(schema_bytes);
	if stored_schema_version != fixture.metadata.internal_schema_version {
		bail!(
			"workflow fixture schema metadata mismatch: row is {stored_schema_version}, fixture declares {}",
			fixture.metadata.internal_schema_version,
		);
	}
	if stored_schema_version != crate::actor::internal_storage::schema::INTERNAL_SCHEMA_VERSION {
		bail!(
			"workflow fixture schema {stored_schema_version} cannot be restored into schema {}",
			crate::actor::internal_storage::schema::INTERNAL_SCHEMA_VERSION,
		);
	}
	if let Some(row) = fixture
		.workflow_rows
		.iter()
		.find(|row| !row.key.starts_with(&WORKFLOW_STORAGE_PREFIX))
	{
		bail!(
			"workflow fixture row escaped the {:?} namespace: {:?}",
			WORKFLOW_STORAGE_PREFIX,
			row.key,
		);
	}
	Ok(())
}

fn statement(sql: &str, params: Option<Vec<BindParam>>) -> SqliteBatchStatement {
	SqliteBatchStatement {
		sql: sql.to_owned(),
		params,
	}
}

fn integer(row: &[ColumnValue], index: usize, label: &str) -> Result<i64> {
	match row.get(index) {
		Some(ColumnValue::Integer(value)) => Ok(*value),
		value => bail!("invalid {label}: expected INTEGER, found {value:?}"),
	}
}

fn optional_integer(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<i64>> {
	match row.get(index) {
		Some(ColumnValue::Null) => Ok(None),
		Some(ColumnValue::Integer(value)) => Ok(Some(*value)),
		value => bail!("invalid {label}: expected NULL or INTEGER, found {value:?}"),
	}
}

fn text(row: &[ColumnValue], index: usize, label: &str) -> Result<String> {
	match row.get(index) {
		Some(ColumnValue::Text(value)) => Ok(value.clone()),
		value => bail!("invalid {label}: expected TEXT, found {value:?}"),
	}
}

fn optional_text(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<String>> {
	match row.get(index) {
		Some(ColumnValue::Null) => Ok(None),
		Some(ColumnValue::Text(value)) => Ok(Some(value.clone())),
		value => bail!("invalid {label}: expected NULL or TEXT, found {value:?}"),
	}
}

fn blob(row: &[ColumnValue], index: usize, label: &str) -> Result<Vec<u8>> {
	match row.get(index) {
		Some(ColumnValue::Blob(value)) => Ok(value.clone()),
		value => bail!("invalid {label}: expected BLOB, found {value:?}"),
	}
}

fn optional_blob(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<Vec<u8>>> {
	match row.get(index) {
		Some(ColumnValue::Null) => Ok(None),
		Some(ColumnValue::Blob(value)) => Ok(Some(value.clone())),
		value => bail!("invalid {label}: expected NULL or BLOB, found {value:?}"),
	}
}

fn optional_integer_param(value: Option<i64>) -> BindParam {
	value.map_or(BindParam::Null, BindParam::Integer)
}

fn optional_text_param(value: Option<String>) -> BindParam {
	value.map_or(BindParam::Null, BindParam::Text)
}

fn optional_blob_param(value: Option<Vec<u8>>) -> BindParam {
	value.map_or(BindParam::Null, BindParam::Blob)
}

#[cfg(test)]
#[path = "../../tests/workflow_fixture.rs"]
mod tests;
