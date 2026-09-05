//! SQL owned by RivetKit's internal actor storage.
//!
//! Every `SELECT`, `UPDATE`, or `DELETE` added or changed here must have
//! corresponding coverage in `sql_efficiency`. Simple inserts and schema DDL
//! require correctness or migration coverage instead.

pub(crate) const LOAD_ACTOR_SNAPSHOT_SQL: &str = "SELECT a.has_initialized, a.input, s.state FROM _rivet_actor a JOIN _rivet_actor_state s ON s.id = a.id WHERE a.id = 1";
pub(crate) const UPSERT_ACTOR_SQL: &str = "INSERT INTO _rivet_actor (id, has_initialized, input) VALUES (1, ?, ?) ON CONFLICT(id) DO UPDATE SET has_initialized = excluded.has_initialized, input = excluded.input";
pub(crate) const UPSERT_ACTOR_STATE_SQL: &str = "INSERT INTO _rivet_actor_state (id, state) VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET state = excluded.state";
pub(crate) const LOAD_CONNECTIONS_SQL: &str = "SELECT c.conn_id, c.parameters, s.state, s.subscriptions, c.gateway_id, c.request_id, s.server_message_index, s.client_message_index, c.request_path, c.request_headers FROM _rivet_conns c JOIN _rivet_conn_state s ON s.conn_id = c.conn_id ORDER BY c.conn_id";
pub(crate) const INSERT_CONNECTION_SQL: &str = "INSERT OR IGNORE INTO _rivet_conns (conn_id, parameters, gateway_id, request_id, request_path, request_headers) VALUES (?, ?, ?, ?, ?, ?)";
pub(crate) const UPSERT_CONNECTION_STATE_SQL: &str = "INSERT INTO _rivet_conn_state (conn_id, state, server_message_index, client_message_index, subscriptions) VALUES (?, ?, ?, ?, ?) ON CONFLICT(conn_id) DO UPDATE SET state = excluded.state, server_message_index = excluded.server_message_index, client_message_index = excluded.client_message_index, subscriptions = excluded.subscriptions";
pub(crate) const DELETE_CONN_STATE_SQL: &str = "DELETE FROM _rivet_conn_state WHERE conn_id = ?";
pub(crate) const DELETE_CONN_SQL: &str = "DELETE FROM _rivet_conns WHERE conn_id = ?";

pub(crate) const RESET_SCHEDULES_FOR_LEGACY_IMPORT_SQL: &str = "DELETE FROM _rivet_schedule_events";
pub(crate) const RESET_SCHEDULE_TRACE_CONTEXTS_SQL: &str = "DELETE FROM _rivet_meta WHERE key >= 'schedule_trace_context:' AND key < 'schedule_trace_context;'";
pub(crate) const INSERT_SCHEDULE_EVENT_SQL: &str = "INSERT INTO _rivet_schedule_events (event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
pub(crate) const UPSERT_RECURRING_SCHEDULE_SQL: &str = "INSERT INTO _rivet_schedule_events (event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(event_id) DO UPDATE SET trigger_at = excluded.trigger_at, action = excluded.action, args = excluded.args, kind = excluded.kind, cron_expression = excluded.cron_expression, timezone = excluded.timezone, interval_ms = excluded.interval_ms, max_history = excluded.max_history";
pub(crate) const CANCEL_SCHEDULE_SQL: &str =
	"DELETE FROM _rivet_schedule_events WHERE event_id = ? AND kind = ?";
pub(crate) const GET_SCHEDULED_EVENT_SQL: &str = "SELECT event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history FROM _rivet_schedule_events WHERE event_id = ? AND kind = ?";
pub(crate) const LIST_SCHEDULED_EVENTS_SQL: &str = "SELECT event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history FROM _rivet_schedule_events WHERE kind = ? ORDER BY trigger_at, event_id";
pub(crate) const DELETE_CRON_HISTORY_SQL: &str = "DELETE FROM _rivet_schedule_history WHERE schedule_id = ? AND EXISTS (SELECT 1 FROM _rivet_schedule_events WHERE event_id = ? AND kind != ?)";
pub(crate) const DELETE_CRON_SQL: &str =
	"DELETE FROM _rivet_schedule_events WHERE event_id = ? AND kind != ?";
pub(crate) const DELETE_CRON_IF_ACTION_SQL: &str =
	"DELETE FROM _rivet_schedule_events WHERE event_id = ? AND kind != ? AND action = ?";
pub(crate) const LIST_CRONS_SQL: &str = "SELECT event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history FROM _rivet_schedule_events WHERE kind != ? ORDER BY trigger_at, event_id";
pub(crate) const CRON_HISTORY_SQL: &str = "SELECT action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata FROM _rivet_schedule_history WHERE schedule_id = ? ORDER BY fired_at DESC, id DESC LIMIT ?";
pub(crate) const LOAD_SCHEDULE_SQL: &str = "SELECT event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history FROM _rivet_schedule_events WHERE event_id = ?";
pub(crate) const COUNT_SCHEDULES_SQL: &str = "SELECT COUNT(*) FROM _rivet_schedule_events";
pub(crate) const TAKE_DUE_SCHEDULES_SQL: &str = "SELECT event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history, (SELECT value FROM _rivet_meta WHERE key = 'schedule_trace_context:' || event_id) FROM _rivet_schedule_events WHERE trigger_at <= ? ORDER BY trigger_at, event_id";
pub(crate) const UPSERT_SCHEDULE_TRACE_CONTEXT_SQL: &str = "INSERT INTO _rivet_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value";
pub(crate) const DELETE_SCHEDULE_TRACE_CONTEXT_SQL: &str = "DELETE FROM _rivet_meta WHERE key = ?";
pub(crate) const DELETE_ORPHAN_SCHEDULE_TRACE_CONTEXT_SQL: &str = "DELETE FROM _rivet_meta WHERE key = ? AND NOT EXISTS (SELECT 1 FROM _rivet_schedule_events WHERE event_id = ?)";
pub(crate) const ADVANCE_SKIPPED_SCHEDULE_SQL: &str =
	"UPDATE _rivet_schedule_events SET trigger_at = ? WHERE event_id = ?";
pub(crate) const ADVANCE_SCHEDULE_SQL: &str =
	"UPDATE _rivet_schedule_events SET trigger_at = ?, last_started_at = ? WHERE event_id = ?";
pub(crate) const INSERT_SCHEDULE_HISTORY_SQL: &str = "INSERT INTO _rivet_schedule_history (schedule_id, action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
pub(crate) const FINISH_HISTORY_SQL: &str = "UPDATE _rivet_schedule_history SET finished_at = ?, result = ?, error_group = ?, error_code = ?, error_message = ?, error_metadata = ? WHERE id = ? AND result = ?";
pub(crate) const RECOVER_HISTORY_SQL: &str = "UPDATE _rivet_schedule_history SET finished_at = ?, result = ?, error_group = ?, error_code = ?, error_message = ?, error_metadata = ? WHERE result = 0";
pub(crate) const NEXT_FUTURE_SCHEDULE_SQL: &str =
	"SELECT MIN(trigger_at) FROM _rivet_schedule_events WHERE trigger_at > ?";
pub(crate) const NEXT_SCHEDULE_SQL: &str = "SELECT MIN(trigger_at) FROM _rivet_schedule_events";
pub(crate) const PRUNE_SCHEDULE_HISTORY_SQL: &str = "DELETE FROM _rivet_schedule_history WHERE id IN (SELECT id FROM _rivet_schedule_history WHERE schedule_id = ? ORDER BY fired_at DESC, id DESC LIMIT -1 OFFSET ?)";
pub(crate) const PRUNE_GLOBAL_HISTORY_SQL: &str = "DELETE FROM _rivet_schedule_history WHERE id IN (SELECT id FROM _rivet_schedule_history ORDER BY fired_at DESC, id DESC LIMIT -1 OFFSET ?)";

pub(crate) fn claim_one_shots_sql(event_count: usize) -> String {
	let placeholders = std::iter::repeat_n("(?, ?)", event_count)
		.collect::<Vec<_>>()
		.join(", ");
	format!(
		"DELETE FROM _rivet_schedule_events WHERE kind = ? AND (event_id, trigger_at) IN ({placeholders})"
	)
}

pub(crate) fn delete_schedule_trace_contexts_sql(event_count: usize) -> String {
	let placeholders = std::iter::repeat_n("?", event_count)
		.collect::<Vec<_>>()
		.join(", ");
	format!("DELETE FROM _rivet_meta WHERE key IN ({placeholders})")
}

pub(crate) const LOAD_QUEUE_NEXT_ID_SQL: &str =
	"SELECT queue_next_id FROM _rivet_runtime WHERE id = 1";
pub(crate) const LOAD_QUEUE_STATS_SQL: &str = "SELECT COUNT(*), MAX(id) FROM _rivet_queue";
pub(crate) const LOAD_QUEUE_MESSAGES_SQL: &str =
	"SELECT id, name, body, created_at FROM _rivet_queue ORDER BY id";
pub(crate) const LOAD_QUEUE_MESSAGES_LIMITED_SQL: &str =
	"SELECT id, name, body, created_at FROM _rivet_queue ORDER BY id LIMIT ?";
pub(crate) const LOAD_QUEUE_MESSAGES_FOR_NAME_SQL: &str = "SELECT id, name, body, created_at FROM _rivet_queue INDEXED BY _rivet_queue_name_id WHERE name = ? ORDER BY id LIMIT ?";
pub(crate) const HAS_QUEUE_MESSAGES_SQL: &str = "SELECT 1 FROM _rivet_queue LIMIT 1";
pub(crate) const HAS_QUEUE_MESSAGES_FOR_NAME_SQL: &str =
	"SELECT 1 FROM _rivet_queue INDEXED BY _rivet_queue_name_id WHERE name = ? LIMIT 1";
pub(crate) const LOAD_QUEUE_MESSAGE_METADATA_PAGE_SQL: &str =
	"SELECT id, name FROM _rivet_queue WHERE id > ? ORDER BY id LIMIT ?";
pub(crate) const LOAD_QUEUE_MESSAGE_NAME_SQL: &str = "SELECT name FROM _rivet_queue WHERE id = ?";
pub(crate) fn load_queue_messages_by_ids_sql(id_count: usize) -> String {
	let placeholders = std::iter::repeat_n("?", id_count)
		.collect::<Vec<_>>()
		.join(", ");
	format!(
		"SELECT id, name, body, created_at FROM _rivet_queue WHERE id IN ({placeholders}) ORDER BY id"
	)
}
pub(crate) const INSERT_QUEUE_MESSAGE_SQL: &str =
	"INSERT OR REPLACE INTO _rivet_queue (id, name, body, created_at) VALUES (?, ?, ?, ?)";
pub(crate) const DELETE_QUEUE_MESSAGE_SQL: &str = "DELETE FROM _rivet_queue WHERE id = ?";
pub(crate) const RESET_QUEUE_SQL: &str = "DELETE FROM _rivet_queue";

pub(crate) const DELETE_USER_KV_SQL: &str = "DELETE FROM _rivet_user_kv WHERE key = ?";
pub(crate) const DELETE_USER_KV_RANGE_SQL: &str =
	"DELETE FROM _rivet_user_kv WHERE key >= ? AND key < ?";
pub(crate) const UPSERT_USER_KV_SQL: &str = "INSERT INTO _rivet_user_kv (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value";
pub(crate) const UPSERT_WORKFLOW_KV_SQL: &str = "INSERT INTO _rivet_wf_kv (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value";

pub(crate) const LOAD_LAST_PUSHED_ALARM_SQL: &str =
	"SELECT last_pushed_alarm FROM _rivet_runtime WHERE id = 1";
pub(crate) const LOAD_RUN_WAKE_AT_SQL: &str = "SELECT value FROM _rivet_meta WHERE key = ?";
pub(crate) const LOAD_INSPECTOR_TOKEN_SQL: &str =
	"SELECT inspector_token FROM _rivet_runtime WHERE id = 1";
pub(crate) const UPSERT_QUEUE_NEXT_ID_SQL: &str = "INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET queue_next_id = excluded.queue_next_id";
pub(crate) const UPSERT_LAST_PUSHED_ALARM_SQL: &str = "INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET last_pushed_alarm = excluded.last_pushed_alarm";
pub(crate) const UPSERT_RUN_WAKE_AT_SQL: &str = "INSERT INTO _rivet_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value";
pub(crate) const UPSERT_INSPECTOR_TOKEN_SQL: &str = "INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET inspector_token = excluded.inspector_token";
pub(crate) const LOAD_META_TEXT_SQL: &str = "SELECT value FROM _rivet_meta WHERE key = ?";
pub(crate) const UPSERT_META_TEXT_SQL: &str = "INSERT INTO _rivet_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value";

pub(crate) fn user_kv_batch_get_sql(key_count: usize) -> String {
	let placeholders = std::iter::repeat_n("?", key_count)
		.collect::<Vec<_>>()
		.join(", ");
	format!("SELECT key, value FROM _rivet_user_kv WHERE key IN ({placeholders})")
}

pub(crate) fn user_kv_list_sql(where_clause: &str, reverse: bool, limited: bool) -> String {
	let order = if reverse { "DESC" } else { "ASC" };
	let limit_clause = if limited { " LIMIT ?" } else { "" };
	format!(
		"SELECT key, value FROM _rivet_user_kv {where_clause} ORDER BY key {order}{limit_clause}"
	)
}

pub(crate) fn clear_table_select_sql(
	table: &str,
	key_column: &str,
	size_expression: &str,
	max_rows: usize,
) -> String {
	format!(
		"SELECT {key_column}, {size_expression} FROM {table} ORDER BY {key_column} LIMIT {max_rows}"
	)
}

pub(crate) fn clear_table_delete_sql(table: &str, key_column: &str) -> String {
	format!("DELETE FROM {table} WHERE {key_column} = ?")
}
