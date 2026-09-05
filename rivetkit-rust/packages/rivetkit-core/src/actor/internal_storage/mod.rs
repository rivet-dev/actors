use std::collections::{BTreeSet, HashMap};
use std::io::Cursor;

use anyhow::{Context, Result, bail};
use rivetkit_actor_persist::versioned as persist_versioned;

use crate::actor::connection::{
	PersistedConnection, PersistedSubscription, encode_persisted_connection,
};
use crate::actor::keys::make_workflow_key;
use crate::actor::messages::WorkflowKvWrite;
use crate::actor::persist::{
	decode_latest_with_embedded_version, encode_latest_with_embedded_version,
};
use crate::actor::queue::{PersistedQueueMessage, QueueMetadata};
use crate::actor::state::PersistedActor;
use crate::error::KvRuntimeError;
use crate::sqlite::{BindParam, ColumnValue, SqliteBatchStatement, SqliteDb};
use crate::types::ListOpts;

pub(crate) mod queries;
pub(crate) mod schema;

pub(crate) use queries::*;

const USER_KV_VALUE_LIMIT: usize = 128 * 1024;
/// Mirrors the engine-side `MAX_KEY_SIZE` in
/// `engine/packages/pegboard/src/actor_kv/` so the documented 2 KiB key limit
/// keeps holding for SQLite-backed user KV.
const USER_KV_KEY_LIMIT: usize = 2048;
pub(crate) const USER_KV_BATCH_GET_MAX_KEYS: usize = 128;
const QUEUE_METADATA_PAGE_ROWS: usize = 128;
const QUEUE_MESSAGE_IDS_PER_QUERY: usize = 128;
const WORKFLOW_KV_VALUE_LIMIT: usize = 256 * 1024;
pub(crate) const RUN_WAKE_AT_META_KEY: &str = "run_wake_at";
const RUN_WAKE_AT_VERSION: u16 = 1;

/// Depot rejects SQLite commits that dirty more than `MAX_COMMIT_RAW_DIRTY_BYTES`
/// (320 pages * 4 KiB = 1.3 MiB) in `engine/packages/depot/src/conveyer/constants.rs`.
/// Budget row payload well below that because row writes also dirty index and
/// interior b-tree pages.
pub(crate) const KV_TX_MAX_PAYLOAD_BYTES: usize = 512 * 1024;
/// Row cap per transaction paired with the payload budget above.
pub(crate) const KV_TX_MAX_ROWS: usize = 128;
const CONNECTION_DESTINATION_ROWS_PER_RECORD: usize = 2;

/// Splits `entries` into contiguous chunks that each stay within the
/// per-transaction row and payload budgets. A single oversized entry gets its
/// own chunk; per-entry limits are enforced by the callers.
pub(crate) fn split_kv_tx_chunks<'a, K, V>(entries: &'a [(K, V)]) -> Vec<&'a [(K, V)]>
where
	K: AsRef<[u8]>,
	V: AsRef<[u8]>,
{
	let mut chunks = Vec::new();
	let mut chunk_start = 0;
	let mut chunk_bytes: usize = 0;
	for (index, (key, value)) in entries.iter().enumerate() {
		let entry_bytes = key.as_ref().len() + value.as_ref().len();
		let chunk_rows = index - chunk_start;
		if chunk_rows > 0
			&& (chunk_rows >= KV_TX_MAX_ROWS
				|| chunk_bytes.saturating_add(entry_bytes) > KV_TX_MAX_PAYLOAD_BYTES)
		{
			chunks.push(&entries[chunk_start..index]);
			chunk_start = index;
			chunk_bytes = 0;
		}
		chunk_bytes = chunk_bytes.saturating_add(entry_bytes);
	}
	if chunk_start < entries.len() {
		chunks.push(&entries[chunk_start..]);
	}
	chunks
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InternalActorSnapshot {
	pub actor: PersistedActor,
	pub last_pushed_alarm: Option<i64>,
	pub run_wake_at: Option<i64>,
}

pub(crate) async fn load_actor_snapshot(db: &SqliteDb) -> Result<Option<InternalActorSnapshot>> {
	let actor_rows = db
		.query(LOAD_ACTOR_SNAPSHOT_SQL, None)
		.await
		.context("load internal actor rows")?;

	let Some(row) = actor_rows.rows.first() else {
		return Ok(None);
	};
	let has_initialized = read_bool(row, 0, "has_initialized")?;
	let input = read_optional_blob(row, 1, "input")?;
	let state = read_blob(row, 2, "state")?;
	let last_pushed_alarm = load_last_pushed_alarm(db).await?;
	let run_wake_at = load_run_wake_at(db).await?;

	Ok(Some(InternalActorSnapshot {
		actor: PersistedActor {
			input,
			has_initialized,
			state,
			scheduled_events: Vec::new(),
		},
		last_pushed_alarm,
		run_wake_at,
	}))
}

pub(crate) async fn persist_actor_snapshot(db: &SqliteDb, actor: &PersistedActor) -> Result<()> {
	let statements = vec![
		SqliteBatchStatement {
			sql: UPSERT_ACTOR_SQL.to_owned(),
			params: Some(vec![
				BindParam::Integer(if actor.has_initialized { 1 } else { 0 }),
				optional_blob_param(actor.input.clone()),
			]),
		},
		SqliteBatchStatement {
			sql: UPSERT_ACTOR_STATE_SQL.to_owned(),
			params: Some(vec![BindParam::Blob(actor.state.clone())]),
		},
	];

	db.execute_batch(statements)
		.await
		.context("persist internal actor snapshot")?;
	Ok(())
}

/// Imports the schedule vector from the legacy versioned actor snapshot exactly
/// once. Runtime state flushes must use [`persist_actor_snapshot`] so SQLite
/// schedule rows remain authoritative and are never rewritten from the legacy
/// field.
pub(crate) async fn import_legacy_actor_snapshot(
	db: &SqliteDb,
	actor: &PersistedActor,
) -> Result<()> {
	let mut statements = vec![
		SqliteBatchStatement {
			sql: UPSERT_ACTOR_SQL.to_owned(),
			params: Some(vec![
				BindParam::Integer(if actor.has_initialized { 1 } else { 0 }),
				optional_blob_param(actor.input.clone()),
			]),
		},
		SqliteBatchStatement {
			sql: UPSERT_ACTOR_STATE_SQL.to_owned(),
			params: Some(vec![BindParam::Blob(actor.state.clone())]),
		},
		SqliteBatchStatement {
			sql: RESET_SCHEDULE_TRACE_CONTEXTS_SQL.to_owned(),
			params: None,
		},
		SqliteBatchStatement {
			sql: RESET_SCHEDULES_FOR_LEGACY_IMPORT_SQL.to_owned(),
			params: None,
		},
	];
	for event in &actor.scheduled_events {
		statements.push(SqliteBatchStatement {
			sql: INSERT_SCHEDULE_EVENT_SQL.to_owned(),
			params: Some(vec![
				BindParam::Text(event.event_id.clone()),
				BindParam::Integer(event.timestamp),
				BindParam::Text(event.action.clone()),
				optional_blob_param(event.args.clone()),
				BindParam::Integer(0),
				BindParam::Null,
				BindParam::Null,
				BindParam::Null,
				BindParam::Null,
				BindParam::Integer(0),
			]),
		});
	}
	db.execute_batch(statements)
		.await
		.context("import legacy actor snapshot")?;
	Ok(())
}

pub(crate) async fn persist_actor_core_and_connections(
	db: &SqliteDb,
	actor: Option<&PersistedActor>,
	connections: &[PersistedConnection],
	removed_connections: &[String],
) -> Result<()> {
	let statements =
		build_actor_core_and_connection_statements(actor, connections, removed_connections)?;

	if statements.is_empty() {
		return Ok(());
	}

	db.execute_batch(statements)
		.await
		.context("persist internal actor state and connection deltas")?;
	Ok(())
}

pub(crate) async fn persist_actor_core_connections_and_workflow(
	db: &SqliteDb,
	actor: Option<&PersistedActor>,
	connections: &[PersistedConnection],
	removed_connections: &[String],
	workflow_writes: &[WorkflowKvWrite],
) -> Result<()> {
	let mut statements =
		build_actor_core_and_connection_statements(actor, connections, removed_connections)?;
	statements.extend(build_workflow_kv_statements(workflow_writes)?);
	validate_atomic_workflow_flush(&statements)?;

	if statements.is_empty() {
		return Ok(());
	}

	db.execute_batch(statements)
		.await
		.context("atomically persist actor state and workflow kv values")?;
	Ok(())
}

pub(crate) fn build_actor_core_and_connection_statements(
	actor: Option<&PersistedActor>,
	connections: &[PersistedConnection],
	removed_connections: &[String],
) -> Result<Vec<SqliteBatchStatement>> {
	let mut statements = Vec::new();

	if let Some(actor) = actor {
		statements.push(SqliteBatchStatement {
			sql: UPSERT_ACTOR_SQL.to_owned(),
			params: Some(vec![
				BindParam::Integer(if actor.has_initialized { 1 } else { 0 }),
				optional_blob_param(actor.input.clone()),
			]),
		});
		statements.push(SqliteBatchStatement {
			sql: UPSERT_ACTOR_STATE_SQL.to_owned(),
			params: Some(vec![BindParam::Blob(actor.state.clone())]),
		});
	}

	for connection in connections {
		push_connection_statements(&mut statements, connection)?;
	}

	for conn_id in removed_connections {
		push_delete_connection_statements(&mut statements, conn_id);
	}

	Ok(statements)
}

#[cfg(test)]
pub(crate) async fn persist_connection_snapshot(
	db: &SqliteDb,
	connection: &PersistedConnection,
) -> Result<()> {
	persist_connection_snapshots(db, std::slice::from_ref(connection)).await
}

/// Persists imported connection snapshots in bounded transactions. A legacy
/// connection is at most one actor-KV value, but a migration page can contain
/// hundreds of them and each connection expands to rows in two destination
/// tables; sending the whole page in one SQLite commit can exceed depot's
/// dirty-page limit.
pub(crate) async fn persist_connection_snapshots(
	db: &SqliteDb,
	connections: &[PersistedConnection],
) -> Result<()> {
	let max_connection_rows = KV_TX_MAX_ROWS / CONNECTION_DESTINATION_ROWS_PER_RECORD;
	let mut chunk_start = 0;
	let mut chunk_bytes: usize = 0;
	for (index, connection) in connections.iter().enumerate() {
		// The legacy encoding includes every nested header and subscription plus
		// its serialization overhead. Add the second SQLite primary key and a
		// fixed allowance for scalar columns instead of approximating nested
		// decoded fields and risking an under-budgeted transaction.
		let entry_bytes = encode_persisted_connection(connection)?
			.len()
			.saturating_add(connection.id.len())
			.saturating_add(32);
		let chunk_rows = index - chunk_start;
		if chunk_rows > 0
			&& (chunk_rows >= max_connection_rows
				|| chunk_bytes.saturating_add(entry_bytes) > KV_TX_MAX_PAYLOAD_BYTES)
		{
			persist_connection_snapshot_chunk(db, &connections[chunk_start..index]).await?;
			chunk_start = index;
			chunk_bytes = 0;
		}
		chunk_bytes = chunk_bytes.saturating_add(entry_bytes);
	}
	if chunk_start < connections.len() {
		persist_connection_snapshot_chunk(db, &connections[chunk_start..]).await?;
	}
	Ok(())
}

async fn persist_connection_snapshot_chunk(
	db: &SqliteDb,
	connections: &[PersistedConnection],
) -> Result<()> {
	let mut statements = Vec::with_capacity(connections.len().saturating_mul(2));
	for connection in connections {
		push_connection_statements(&mut statements, connection)?;
	}
	if statements.is_empty() {
		return Ok(());
	}
	db.execute_batch(statements)
		.await
		.context("persist internal connection snapshot chunk")?;
	Ok(())
}

pub(crate) async fn load_connections(db: &SqliteDb) -> Result<Vec<PersistedConnection>> {
	let result = db
		.query(LOAD_CONNECTIONS_SQL, None)
		.await
		.context("load internal connection rows")?;

	result
		.rows
		.iter()
		.map(|row| {
			let subscriptions: Vec<String> = decode_cbor_blob(row, 3, "connection subscriptions")?;
			let request_headers: HashMap<String, String> =
				decode_cbor_blob(row, 9, "connection request headers")?;
			Ok(PersistedConnection {
				id: read_text(row, 0, "conn_id")?,
				parameters: read_blob(row, 1, "connection parameters")?,
				state: read_blob(row, 2, "connection state")?,
				subscriptions: subscriptions
					.into_iter()
					.map(|event_name| PersistedSubscription { event_name })
					.collect(),
				gateway_id: read_fixed_4(row, 4, "connection gateway_id")?,
				request_id: read_fixed_4(row, 5, "connection request_id")?,
				server_message_index: read_u16(row, 6, "server_message_index")?,
				client_message_index: read_u16(row, 7, "client_message_index")?,
				request_path: read_text(row, 8, "connection request_path")?,
				request_headers,
			})
		})
		.collect()
}

pub(crate) async fn load_queue_metadata(db: &SqliteDb) -> Result<QueueMetadata> {
	let runtime = db
		.query(LOAD_QUEUE_NEXT_ID_SQL, None)
		.await
		.context("load internal queue next id")?;
	let stored_next_id = match runtime.rows.first() {
		Some(row) => u64::try_from(read_i64(row, 0, "queue_next_id")?)
			.context("invalid internal queue_next_id")?,
		None => 1,
	};

	let stats = db
		.query(LOAD_QUEUE_STATS_SQL, None)
		.await
		.context("load internal queue stats")?;
	let Some(row) = stats.rows.first() else {
		return Ok(QueueMetadata {
			next_id: stored_next_id.max(1),
			size: 0,
		});
	};
	let size = u32::try_from(read_i64(row, 0, "queue size")?)
		.context("internal queue size exceeds u32 range")?;
	let max_id = read_optional_i64(row, 1, "queue max id")?
		.and_then(|value| u64::try_from(value).ok())
		.unwrap_or(0);
	Ok(QueueMetadata {
		next_id: stored_next_id.max(max_id.saturating_add(1)).max(1),
		size,
	})
}

pub(crate) async fn persist_queue_message(
	db: &SqliteDb,
	id: u64,
	next_id: u64,
	message: &PersistedQueueMessage,
) -> Result<()> {
	let id = i64::try_from(id).context("queue message id exceeds sqlite integer range")?;
	let next_id = i64::try_from(next_id).context("queue next id exceeds sqlite integer range")?;
	db.execute_batch(vec![
		SqliteBatchStatement {
			sql: INSERT_QUEUE_MESSAGE_SQL.to_owned(),
			params: Some(vec![
				BindParam::Integer(id),
				BindParam::Text(message.name.clone()),
				BindParam::Blob(message.body.clone()),
				BindParam::Integer(message.created_at),
			]),
		},
		SqliteBatchStatement {
			sql: UPSERT_QUEUE_NEXT_ID_SQL.to_owned(),
			params: Some(vec![
				BindParam::Null,
				BindParam::Null,
				BindParam::Integer(next_id),
			]),
		},
	])
	.await
	.context("persist internal queue message")?;
	Ok(())
}

/// Persists imported queue rows without rewriting `queue_next_id` for every
/// message. The importer writes that singleton once after every row is copied.
pub(crate) async fn persist_queue_messages(
	db: &SqliteDb,
	messages: &[(u64, PersistedQueueMessage)],
) -> Result<()> {
	for chunk in split_queue_tx_chunks(messages) {
		let mut statements = Vec::with_capacity(chunk.len());
		for (id, message) in chunk {
			statements.push(SqliteBatchStatement {
				sql: INSERT_QUEUE_MESSAGE_SQL.to_owned(),
				params: Some(vec![
					BindParam::Integer(
						i64::try_from(*id)
							.context("queue message id exceeds sqlite integer range")?,
					),
					BindParam::Text(message.name.clone()),
					BindParam::Blob(message.body.clone()),
					BindParam::Integer(message.created_at),
				]),
			});
		}
		db.execute_batch(statements)
			.await
			.context("persist internal queue message chunk")?;
	}
	Ok(())
}

fn split_queue_tx_chunks(
	messages: &[(u64, PersistedQueueMessage)],
) -> Vec<&[(u64, PersistedQueueMessage)]> {
	let mut chunks = Vec::new();
	let mut chunk_start = 0;
	let mut chunk_bytes: usize = 0;
	for (index, (_, message)) in messages.iter().enumerate() {
		let entry_bytes = message.name.len() + message.body.len() + 16;
		let chunk_rows = index - chunk_start;
		if chunk_rows > 0
			&& (chunk_rows >= KV_TX_MAX_ROWS
				|| chunk_bytes.saturating_add(entry_bytes) > KV_TX_MAX_PAYLOAD_BYTES)
		{
			chunks.push(&messages[chunk_start..index]);
			chunk_start = index;
			chunk_bytes = 0;
		}
		chunk_bytes = chunk_bytes.saturating_add(entry_bytes);
	}
	if chunk_start < messages.len() {
		chunks.push(&messages[chunk_start..]);
	}
	chunks
}

pub(crate) async fn persist_queue_next_id(db: &SqliteDb, next_id: u64) -> Result<()> {
	let next_id = i64::try_from(next_id).context("queue next id exceeds sqlite integer range")?;
	db.execute(
		UPSERT_QUEUE_NEXT_ID_SQL,
		Some(vec![
			BindParam::Null,
			BindParam::Null,
			BindParam::Integer(next_id),
		]),
	)
	.await
	.context("persist internal queue next id")?;
	Ok(())
}

pub(crate) async fn load_queue_messages(db: &SqliteDb) -> Result<Vec<QueueMessageRow>> {
	let result = db
		.query(LOAD_QUEUE_MESSAGES_SQL, None)
		.await
		.context("load internal queue messages")?;
	decode_queue_message_rows(&result.rows)
}

pub(crate) async fn load_queue_message_name(db: &SqliteDb, id: u64) -> Result<Option<String>> {
	let id = i64::try_from(id).context("queue message id exceeds sqlite integer range")?;
	let result = db
		.query(
			LOAD_QUEUE_MESSAGE_NAME_SQL,
			Some(vec![BindParam::Integer(id)]),
		)
		.await
		.context("load internal queue message name")?;
	let Some(row) = result.rows.first() else {
		return Ok(None);
	};
	Ok(Some(read_text(row, 0, "queue message name")?))
}

pub(crate) async fn load_queue_messages_matching(
	db: &SqliteDb,
	names: Option<&BTreeSet<String>>,
	limit: u32,
) -> Result<Vec<QueueMessageRow>> {
	if limit == 0 {
		return Ok(Vec::new());
	}

	let Some(names) = names else {
		let result = db
			.query(
				LOAD_QUEUE_MESSAGES_LIMITED_SQL,
				Some(vec![BindParam::Integer(i64::from(limit))]),
			)
			.await
			.context("load internal queue messages in enqueue order")?;
		return decode_queue_message_rows(&result.rows);
	};

	if names.len() == 1 {
		let result = db
			.query(
				LOAD_QUEUE_MESSAGES_FOR_NAME_SQL,
				Some(vec![
					BindParam::Text(names.first().expect("one queue name").clone()),
					BindParam::Integer(i64::from(limit)),
				]),
			)
			.await
			.context("load internal queue messages for name")?;
		return decode_queue_message_rows(&result.rows);
	}

	let ids = load_queue_message_ids_for_names(db, names, limit).await?;
	load_queue_messages_by_ids(db, &ids).await
}

pub(crate) async fn has_queue_messages(
	db: &SqliteDb,
	names: Option<&BTreeSet<String>>,
) -> Result<bool> {
	let Some(names) = names else {
		return db
			.query(HAS_QUEUE_MESSAGES_SQL, None)
			.await
			.context("check for internal queue messages")
			.map(|result| !result.rows.is_empty());
	};

	if names.len() == 1 {
		return db
			.query(
				HAS_QUEUE_MESSAGES_FOR_NAME_SQL,
				Some(vec![BindParam::Text(
					names.first().expect("one queue name").clone(),
				)]),
			)
			.await
			.context("check for named internal queue messages")
			.map(|result| !result.rows.is_empty());
	}

	load_queue_message_ids_for_names(db, names, 1)
		.await
		.map(|ids| !ids.is_empty())
}

async fn load_queue_message_ids_for_names(
	db: &SqliteDb,
	names: &BTreeSet<String>,
	limit: u32,
) -> Result<Vec<u64>> {
	let mut ids = Vec::new();
	let mut after_id = -1_i64;
	loop {
		let result = db
			.query(
				LOAD_QUEUE_MESSAGE_METADATA_PAGE_SQL,
				Some(vec![
					BindParam::Integer(after_id),
					BindParam::Integer(QUEUE_METADATA_PAGE_ROWS as i64),
				]),
			)
			.await
			.context("load internal queue message metadata page")?;
		if result.rows.is_empty() {
			break;
		}
		for row in &result.rows {
			after_id = read_i64(row, 0, "queue message id")?;
			let name = read_text(row, 1, "queue message name")?;
			if names.contains(&name) {
				ids.push(u64::try_from(after_id).context("invalid queue message id")?);
				if ids.len() >= limit as usize {
					return Ok(ids);
				}
			}
		}
		if result.rows.len() < QUEUE_METADATA_PAGE_ROWS {
			break;
		}
	}
	Ok(ids)
}

async fn load_queue_messages_by_ids(db: &SqliteDb, ids: &[u64]) -> Result<Vec<QueueMessageRow>> {
	let mut messages = Vec::with_capacity(ids.len());
	for ids in ids.chunks(QUEUE_MESSAGE_IDS_PER_QUERY) {
		let params = ids
			.iter()
			.map(|id| {
				Ok(BindParam::Integer(i64::try_from(*id).context(
					"queue message id exceeds sqlite integer range",
				)?))
			})
			.collect::<Result<Vec<_>>>()?;
		let result = db
			.query(load_queue_messages_by_ids_sql(ids.len()), Some(params))
			.await
			.context("load internal queue messages by id")?;
		messages.extend(decode_queue_message_rows(&result.rows)?);
	}
	Ok(messages)
}

fn decode_queue_message_rows(rows: &[Vec<ColumnValue>]) -> Result<Vec<QueueMessageRow>> {
	rows.iter()
		.map(|row| {
			Ok(QueueMessageRow {
				id: u64::try_from(read_i64(row, 0, "queue message id")?)
					.context("invalid queue message id")?,
				message: PersistedQueueMessage {
					name: read_text(row, 1, "queue message name")?,
					body: read_blob(row, 2, "queue message body")?,
					created_at: read_i64(row, 3, "queue message created_at")?,
					failure_count: None,
					available_at: None,
					in_flight: None,
					in_flight_at: None,
				},
			})
		})
		.collect()
}

pub(crate) async fn delete_queue_messages(db: &SqliteDb, ids: &[u64]) -> Result<u32> {
	if ids.is_empty() {
		return Ok(0);
	}

	let mut statements = Vec::with_capacity(ids.len());
	for id in ids {
		statements.push(SqliteBatchStatement {
			sql: DELETE_QUEUE_MESSAGE_SQL.to_owned(),
			params: Some(vec![BindParam::Integer(
				i64::try_from(*id).context("queue message id exceeds sqlite integer range")?,
			)]),
		});
	}
	let results = db
		.execute_batch(statements)
		.await
		.context("delete internal queue messages")?;
	results.into_iter().try_fold(0u32, |deleted, result| {
		let changes = u32::try_from(result.changes)
			.context("deleted queue message count is outside u32 range")?;
		deleted
			.checked_add(changes)
			.context("deleted queue message count exceeds u32 range")
	})
}

pub(crate) async fn reset_queue(db: &SqliteDb) -> Result<()> {
	db.execute(RESET_QUEUE_SQL, None)
		.await
		.context("reset internal queue")?;
	Ok(())
}

#[derive(Debug)]
pub(crate) struct QueueMessageRow {
	pub id: u64,
	pub message: PersistedQueueMessage,
}

pub(crate) async fn user_kv_batch_get(
	db: &SqliteDb,
	keys: &[&[u8]],
) -> Result<Vec<Option<Vec<u8>>>> {
	let mut values_by_key = HashMap::with_capacity(keys.len());
	for keys in keys.chunks(USER_KV_BATCH_GET_MAX_KEYS) {
		let result = db
			.query(
				user_kv_batch_get_sql(keys.len()),
				Some(
					keys.iter()
						.map(|key| BindParam::Blob((*key).to_vec()))
						.collect(),
				),
			)
			.await
			.context("batch get user kv values from sqlite")?;
		for row in &result.rows {
			values_by_key.insert(
				read_blob(row, 0, "user kv key")?,
				read_blob(row, 1, "user kv value")?,
			);
		}
	}

	Ok(keys
		.iter()
		.map(|key| values_by_key.get(*key).cloned())
		.collect())
}

pub(crate) async fn user_kv_batch_put(db: &SqliteDb, entries: &[(&[u8], &[u8])]) -> Result<()> {
	for (key, value) in entries {
		if key.len() > USER_KV_KEY_LIMIT {
			return Err(KvRuntimeError::KeyTooLarge {
				size: key.len(),
				limit: USER_KV_KEY_LIMIT,
			}
			.build());
		}
		if value.len() > USER_KV_VALUE_LIMIT {
			return Err(KvRuntimeError::ValueTooLarge {
				size: value.len(),
				limit: USER_KV_VALUE_LIMIT,
			}
			.build());
		}
	}
	if entries.is_empty() {
		return Ok(());
	}
	// Batches above the per-transaction budget split across transactions so no
	// single commit exceeds the depot commit size limit. Oversized batch puts
	// are therefore not atomic; each chunk rolls forward independently.
	for chunk in split_kv_tx_chunks(entries) {
		let statements = chunk
			.iter()
			.map(|(key, value)| SqliteBatchStatement {
				sql: UPSERT_USER_KV_SQL.to_owned(),
				params: Some(vec![
					BindParam::Blob((*key).to_vec()),
					BindParam::Blob((*value).to_vec()),
				]),
			})
			.collect::<Vec<_>>();
		db.execute_batch(statements)
			.await
			.context("put user kv values into sqlite")?;
	}
	Ok(())
}

pub(crate) async fn user_kv_batch_delete(db: &SqliteDb, keys: &[&[u8]]) -> Result<()> {
	let statements = keys
		.iter()
		.map(|key| SqliteBatchStatement {
			sql: DELETE_USER_KV_SQL.to_owned(),
			params: Some(vec![BindParam::Blob((*key).to_vec())]),
		})
		.collect::<Vec<_>>();
	if statements.is_empty() {
		return Ok(());
	}
	db.execute_batch(statements)
		.await
		.context("delete user kv values from sqlite")?;
	Ok(())
}

pub(crate) async fn user_kv_delete_range(db: &SqliteDb, start: &[u8], end: &[u8]) -> Result<()> {
	db.execute(
		DELETE_USER_KV_RANGE_SQL,
		Some(vec![
			BindParam::Blob(start.to_vec()),
			BindParam::Blob(end.to_vec()),
		]),
	)
	.await
	.context("delete user kv range from sqlite")?;
	Ok(())
}

pub(crate) async fn user_kv_list_prefix(
	db: &SqliteDb,
	prefix: &[u8],
	opts: ListOpts,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
	match prefix_upper_bound(prefix) {
		Some(end) => {
			user_kv_list_where(
				db,
				"WHERE key >= ? AND key < ?",
				vec![BindParam::Blob(prefix.to_vec()), BindParam::Blob(end)],
				opts,
			)
			.await
		}
		None if prefix.is_empty() => user_kv_list_where(db, "", Vec::new(), opts).await,
		None => {
			user_kv_list_where(
				db,
				"WHERE key >= ?",
				vec![BindParam::Blob(prefix.to_vec())],
				opts,
			)
			.await
		}
	}
}

pub(crate) async fn user_kv_list_range(
	db: &SqliteDb,
	start: &[u8],
	end: &[u8],
	opts: ListOpts,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
	user_kv_list_where(
		db,
		"WHERE key >= ? AND key < ?",
		vec![
			BindParam::Blob(start.to_vec()),
			BindParam::Blob(end.to_vec()),
		],
		opts,
	)
	.await
}

pub(crate) async fn workflow_kv_batch_put(db: &SqliteDb, entries: &[(&[u8], &[u8])]) -> Result<()> {
	let mut statements = Vec::with_capacity(entries.len());
	for (key, value) in entries {
		if value.len() > WORKFLOW_KV_VALUE_LIMIT {
			bail!(
				"workflow kv value exceeds sqlite storage limit: {} > {}",
				value.len(),
				WORKFLOW_KV_VALUE_LIMIT
			);
		}
		statements.push(SqliteBatchStatement {
			sql: UPSERT_WORKFLOW_KV_SQL.to_owned(),
			params: Some(vec![
				BindParam::Blob((*key).to_vec()),
				BindParam::Blob((*value).to_vec()),
			]),
		});
	}
	if statements.is_empty() {
		return Ok(());
	}
	db.execute_batch(statements)
		.await
		.context("put workflow kv values into sqlite")?;
	Ok(())
}

fn build_workflow_kv_statements(writes: &[WorkflowKvWrite]) -> Result<Vec<SqliteBatchStatement>> {
	writes
		.iter()
		.map(|write| {
			if write.value.len() > WORKFLOW_KV_VALUE_LIMIT {
				bail!(
					"workflow kv value exceeds sqlite storage limit: {} > {}",
					write.value.len(),
					WORKFLOW_KV_VALUE_LIMIT
				);
			}
			Ok(SqliteBatchStatement {
				sql: UPSERT_WORKFLOW_KV_SQL.to_owned(),
				params: Some(vec![
					BindParam::Blob(make_workflow_key(&write.key)),
					BindParam::Blob(write.value.clone()),
				]),
			})
		})
		.collect()
}

fn validate_atomic_workflow_flush(statements: &[SqliteBatchStatement]) -> Result<()> {
	let row_count = statements.len();
	let payload_bytes = statement_bind_payload_len(statements);
	validate_atomic_state_transaction_budget(row_count, payload_bytes)
}

pub(crate) fn validate_atomic_state_transaction_budget(
	row_count: usize,
	payload_bytes: usize,
) -> Result<()> {
	if row_count > KV_TX_MAX_ROWS || payload_bytes > KV_TX_MAX_PAYLOAD_BYTES {
		bail!(
			"atomic SQLite and actor state transaction exceeds transaction budget: {row_count} rows and {payload_bytes} bytes (limits: {} rows and {} bytes)",
			KV_TX_MAX_ROWS,
			KV_TX_MAX_PAYLOAD_BYTES
		);
	}
	Ok(())
}

pub(crate) fn statement_bind_payload_len(statements: &[SqliteBatchStatement]) -> usize {
	statements
		.iter()
		.flat_map(|statement| statement.params.iter().flatten())
		.map(bind_param_payload_len)
		.fold(0usize, usize::saturating_add)
}

pub(crate) fn bind_param_payload_len(param: &BindParam) -> usize {
	match param {
		BindParam::Null => 0,
		BindParam::Integer(_) | BindParam::Float(_) => 8,
		BindParam::Text(value) => value.len(),
		BindParam::Blob(value) => value.len(),
	}
}

async fn user_kv_list_where(
	db: &SqliteDb,
	where_clause: &str,
	params: Vec<BindParam>,
	opts: ListOpts,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
	let mut params = params;
	if let Some(limit) = opts.limit {
		params.push(BindParam::Integer(i64::from(limit)));
	}
	let sql = user_kv_list_sql(where_clause, opts.reverse, opts.limit.is_some());
	let result = db
		.query(&sql, Some(params))
		.await
		.context("list user kv values from sqlite")?;
	decode_user_kv_rows(&result.rows)
}

fn decode_user_kv_rows(rows: &[Vec<ColumnValue>]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
	rows.iter()
		.map(|row| {
			Ok((
				read_blob(row, 0, "user kv key")?,
				read_blob(row, 1, "user kv value")?,
			))
		})
		.collect()
}

fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
	let mut end = prefix.to_vec();
	for index in (0..end.len()).rev() {
		if end[index] != u8::MAX {
			end[index] = end[index].saturating_add(1);
			end.truncate(index + 1);
			return Some(end);
		}
	}
	None
}

fn push_connection_statements(
	statements: &mut Vec<SqliteBatchStatement>,
	connection: &PersistedConnection,
) -> Result<()> {
	let request_headers =
		encode_cbor_blob(&connection.request_headers, "connection request headers")?;
	let subscriptions = encode_cbor_blob(
		&connection
			.subscriptions
			.iter()
			.map(|subscription| subscription.event_name.clone())
			.collect::<Vec<_>>(),
		"connection subscriptions",
	)?;

	statements.push(SqliteBatchStatement {
		sql: INSERT_CONNECTION_SQL.to_owned(),
		params: Some(vec![
			BindParam::Text(connection.id.clone()),
			BindParam::Blob(connection.parameters.clone()),
			BindParam::Blob(connection.gateway_id.to_vec()),
			BindParam::Blob(connection.request_id.to_vec()),
			BindParam::Text(connection.request_path.clone()),
			BindParam::Blob(request_headers),
		]),
	});
	statements.push(SqliteBatchStatement {
		sql: UPSERT_CONNECTION_STATE_SQL.to_owned(),
		params: Some(vec![
			BindParam::Text(connection.id.clone()),
			BindParam::Blob(connection.state.clone()),
			BindParam::Integer(i64::from(connection.server_message_index)),
			BindParam::Integer(i64::from(connection.client_message_index)),
			BindParam::Blob(subscriptions),
		]),
	});
	Ok(())
}

fn push_delete_connection_statements(statements: &mut Vec<SqliteBatchStatement>, conn_id: &str) {
	statements.push(SqliteBatchStatement {
		sql: DELETE_CONN_STATE_SQL.to_owned(),
		params: Some(vec![BindParam::Text(conn_id.to_owned())]),
	});
	statements.push(SqliteBatchStatement {
		sql: DELETE_CONN_SQL.to_owned(),
		params: Some(vec![BindParam::Text(conn_id.to_owned())]),
	});
}

pub(crate) async fn load_last_pushed_alarm(db: &SqliteDb) -> Result<Option<i64>> {
	let result = db
		.query(LOAD_LAST_PUSHED_ALARM_SQL, None)
		.await
		.context("load internal last pushed alarm")?;
	let Some(row) = result.rows.first() else {
		return Ok(None);
	};
	read_optional_i64(row, 0, "last_pushed_alarm")
}

pub(crate) async fn persist_last_pushed_alarm(db: &SqliteDb, alarm_ts: Option<i64>) -> Result<()> {
	db.execute(
		UPSERT_LAST_PUSHED_ALARM_SQL,
		Some(vec![
			optional_i64_param(alarm_ts),
			BindParam::Null,
			BindParam::Integer(1),
		]),
	)
	.await
	.context("persist internal last pushed alarm")?;
	Ok(())
}

pub(crate) async fn load_run_wake_at(db: &SqliteDb) -> Result<Option<i64>> {
	let result = db
		.query(
			LOAD_RUN_WAKE_AT_SQL,
			Some(vec![BindParam::Text(RUN_WAKE_AT_META_KEY.to_owned())]),
		)
		.await
		.context("load internal run wake deadline")?;
	let Some(row) = result.rows.first() else {
		return Ok(None);
	};
	let payload = read_blob(row, 0, "run wake deadline")?;
	decode_latest_with_embedded_version::<persist_versioned::RunWakeAt>(
		&payload,
		"run wake deadline",
	)
}

pub(crate) async fn persist_run_wake_at(db: &SqliteDb, wake_at: Option<i64>) -> Result<()> {
	let payload = encode_latest_with_embedded_version::<persist_versioned::RunWakeAt>(
		wake_at,
		RUN_WAKE_AT_VERSION,
		"run wake deadline",
	)?;
	db.execute(
		UPSERT_RUN_WAKE_AT_SQL,
		Some(vec![
			BindParam::Text(RUN_WAKE_AT_META_KEY.to_owned()),
			BindParam::Blob(payload),
		]),
	)
	.await
	.context("persist internal run wake deadline")?;
	Ok(())
}

pub(crate) async fn load_inspector_token(db: &SqliteDb) -> Result<Option<String>> {
	let result = db
		.query(LOAD_INSPECTOR_TOKEN_SQL, None)
		.await
		.context("load internal inspector token")?;
	let Some(row) = result.rows.first() else {
		return Ok(None);
	};
	read_optional_text(row, 0, "inspector_token")
}

pub(crate) async fn persist_inspector_token(db: &SqliteDb, token: &str) -> Result<()> {
	db.execute(
		UPSERT_INSPECTOR_TOKEN_SQL,
		Some(vec![
			BindParam::Null,
			BindParam::Text(token.to_owned()),
			BindParam::Integer(1),
		]),
	)
	.await
	.context("persist internal inspector token")?;
	Ok(())
}

/// Reads migration bookkeeping from the bootstrap `_rivet_meta` root. This is
/// not a general-purpose runtime KV accessor.
pub(crate) async fn load_meta_text(db: &SqliteDb, key: &str) -> Result<Option<String>> {
	let result = db
		.query(
			LOAD_META_TEXT_SQL,
			Some(vec![BindParam::Text(key.to_owned())]),
		)
		.await
		.context("load internal meta text")?;
	let Some(row) = result.rows.first() else {
		return Ok(None);
	};
	let bytes = read_blob(row, 0, "meta text")?;
	String::from_utf8(bytes)
		.context("decode internal meta text")
		.map(Some)
}

/// Writes migration bookkeeping to the bootstrap `_rivet_meta` root. This is
/// not a general-purpose runtime KV accessor.
pub(crate) async fn persist_meta_text(db: &SqliteDb, key: &str, value: &str) -> Result<()> {
	db.execute(
		UPSERT_META_TEXT_SQL,
		Some(vec![
			BindParam::Text(key.to_owned()),
			BindParam::Blob(value.as_bytes().to_vec()),
		]),
	)
	.await
	.context("persist internal meta text")?;
	Ok(())
}

pub(crate) async fn clear_imported_storage(db: &SqliteDb, actor_id: &str) -> Result<()> {
	// Delete bounded sets of rows in separate commits. A single `DELETE FROM`
	// over a large interrupted import can itself exceed depot's dirty-page
	// limit, permanently preventing the importer from recovering.
	for (table, key_column, size_expression) in [
		(
			"_rivet_conn_state",
			"conn_id",
			"length(conn_id) + length(state) + length(subscriptions) + 16",
		),
		(
			"_rivet_conns",
			"conn_id",
			"length(conn_id) + length(parameters) + length(request_path) + length(request_headers) + 16",
		),
		(
			"_rivet_schedule_events",
			"event_id",
			"length(event_id) + length(action) + coalesce(length(args), 0) + 16",
		),
		("_rivet_actor_state", "id", "length(state) + 8"),
		("_rivet_actor", "id", "coalesce(length(input), 0) + 16"),
		("_rivet_queue", "id", "length(name) + length(body) + 16"),
		("_rivet_wf_kv", "key", "length(key) + length(value)"),
		("_rivet_user_kv", "key", "length(key) + length(value)"),
		("_rivet_runtime", "id", "64"),
	] {
		clear_table_bounded(db, actor_id, table, key_column, size_expression)
			.await
			.with_context(|| format!("clear partially imported {table} rows"))?;
	}
	db.execute(RESET_SCHEDULE_TRACE_CONTEXTS_SQL, None)
		.await
		.context("clear imported schedule trace contexts")?;
	Ok(())
}

async fn clear_table_bounded(
	db: &SqliteDb,
	actor_id: &str,
	table: &str,
	key_column: &str,
	size_expression: &str,
) -> Result<()> {
	// An interrupted legacy import can leave enough data that one `DELETE FROM`
	// exceeds depot's dirty-page commit limit. Delete estimated-size chunks in
	// separate transactions so cleanup can always make progress.
	let mut cleared_rows = 0usize;
	let mut cleared_bytes = 0usize;
	let mut last_logged_rows = 0usize;
	let mut last_logged_bytes = 0usize;
	loop {
		let rows = db
			.query(
				clear_table_select_sql(table, key_column, size_expression, KV_TX_MAX_ROWS),
				None,
			)
			.await?;
		if rows.rows.is_empty() {
			if cleared_rows > 0 {
				tracing::info!(
					actor_id,
					phase = "cleanup",
					table,
					record_count = cleared_rows,
					byte_count = cleared_bytes,
					"legacy kv import cleanup table completed"
				);
			}
			return Ok(());
		}

		let mut statements = Vec::new();
		let mut payload_bytes = 0usize;
		for row in rows.rows {
			let key = match row.first() {
				Some(ColumnValue::Integer(value)) => BindParam::Integer(*value),
				Some(ColumnValue::Text(value)) => BindParam::Text(value.clone()),
				Some(ColumnValue::Blob(value)) => BindParam::Blob(value.clone()),
				other => bail!("invalid cleanup key for {table}: {other:?}"),
			};
			let row_bytes = match row.get(1) {
				Some(ColumnValue::Integer(value)) => usize::try_from(*value).unwrap_or(usize::MAX),
				other => bail!("invalid cleanup size for {table}: {other:?}"),
			};
			if !statements.is_empty()
				&& payload_bytes.saturating_add(row_bytes) > KV_TX_MAX_PAYLOAD_BYTES
			{
				break;
			}
			payload_bytes = payload_bytes.saturating_add(row_bytes);
			statements.push(SqliteBatchStatement {
				sql: clear_table_delete_sql(table, key_column),
				params: Some(vec![key]),
			});
		}

		cleared_rows = cleared_rows.saturating_add(statements.len());
		cleared_bytes = cleared_bytes.saturating_add(payload_bytes);
		db.execute_batch(statements)
			.await
			.with_context(|| format!("delete bounded {table} row chunk"))?;
		if cleared_rows.saturating_sub(last_logged_rows) >= 1_024
			|| cleared_bytes.saturating_sub(last_logged_bytes) >= 16 * 1024 * 1024
		{
			last_logged_rows = cleared_rows;
			last_logged_bytes = cleared_bytes;
			tracing::info!(
				actor_id,
				phase = "cleanup",
				table,
				record_count = cleared_rows,
				byte_count = cleared_bytes,
				"legacy kv import cleanup progress"
			);
		}
	}
}

fn optional_blob_param(value: Option<Vec<u8>>) -> BindParam {
	value.map(BindParam::Blob).unwrap_or(BindParam::Null)
}

fn optional_i64_param(value: Option<i64>) -> BindParam {
	value.map(BindParam::Integer).unwrap_or(BindParam::Null)
}

fn read_bool(row: &[ColumnValue], index: usize, label: &str) -> Result<bool> {
	match row.get(index) {
		Some(ColumnValue::Integer(0)) => Ok(false),
		Some(ColumnValue::Integer(1)) => Ok(true),
		other => bail!("invalid internal {label}: expected bool integer, got {other:?}"),
	}
}

fn read_i64(row: &[ColumnValue], index: usize, label: &str) -> Result<i64> {
	match row.get(index) {
		Some(ColumnValue::Integer(value)) => Ok(*value),
		other => bail!("invalid internal {label}: expected integer, got {other:?}"),
	}
}

fn read_u16(row: &[ColumnValue], index: usize, label: &str) -> Result<u16> {
	let value = read_i64(row, index, label)?;
	u16::try_from(value)
		.with_context(|| format!("invalid internal {label}: expected u16 integer, got {value}"))
}

fn read_optional_i64(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<i64>> {
	match row.get(index) {
		Some(ColumnValue::Null) | None => Ok(None),
		Some(ColumnValue::Integer(value)) => Ok(Some(*value)),
		other => bail!("invalid internal {label}: expected nullable integer, got {other:?}"),
	}
}

fn read_text(row: &[ColumnValue], index: usize, label: &str) -> Result<String> {
	match row.get(index) {
		Some(ColumnValue::Text(value)) => Ok(value.clone()),
		other => bail!("invalid internal {label}: expected text, got {other:?}"),
	}
}

fn read_optional_text(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<String>> {
	match row.get(index) {
		Some(ColumnValue::Null) | None => Ok(None),
		Some(ColumnValue::Text(value)) => Ok(Some(value.clone())),
		other => bail!("invalid internal {label}: expected nullable text, got {other:?}"),
	}
}

fn read_blob(row: &[ColumnValue], index: usize, label: &str) -> Result<Vec<u8>> {
	match row.get(index) {
		Some(ColumnValue::Blob(value)) => Ok(value.clone()),
		// Depot's remote query path reports zero-length blobs as NULL.
		Some(ColumnValue::Null) => Ok(Vec::new()),
		other => bail!("invalid internal {label}: expected blob, got {other:?}"),
	}
}

fn read_fixed_4(row: &[ColumnValue], index: usize, label: &str) -> Result<[u8; 4]> {
	let value = read_blob(row, index, label)?;
	value
		.as_slice()
		.try_into()
		.with_context(|| format!("invalid internal {label}: expected 4 bytes"))
}

fn read_optional_blob(row: &[ColumnValue], index: usize, label: &str) -> Result<Option<Vec<u8>>> {
	match row.get(index) {
		Some(ColumnValue::Null) | None => Ok(None),
		Some(ColumnValue::Blob(value)) => Ok(Some(value.clone())),
		other => bail!("invalid internal {label}: expected nullable blob, got {other:?}"),
	}
}

fn encode_cbor_blob(value: &impl serde::Serialize, label: &str) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded)
		.with_context(|| format!("encode internal {label} as cbor"))?;
	Ok(encoded)
}

fn decode_cbor_blob<T: serde::de::DeserializeOwned>(
	row: &[ColumnValue],
	index: usize,
	label: &str,
) -> Result<T> {
	let bytes = read_blob(row, index, label)?;
	ciborium::from_reader(Cursor::new(bytes))
		.with_context(|| format!("decode internal {label} from cbor"))
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../../tests/internal_storage.rs"]
pub(crate) mod tests;
