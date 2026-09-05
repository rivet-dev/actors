use std::fmt::Write as _;

use rusqlite::types::Value;
use rusqlite::{Connection, StatementStatus, params_from_iter};

use crate::actor::internal_storage::queries;
use crate::actor::internal_storage::schema::{CREATE_META_TABLE, MIGRATIONS};
use crate::actor::{internal_storage, schedule};
use crate::types::ListOpts;

// This suite covers SQL owned by rivetkit-core. User-provided SQL, including
// Actor Runtime Socket requests, is intentionally outside its scope.
const REPRESENTATIVE_ROWS: usize = 10_000;

#[derive(Clone, Copy, Debug)]
struct AllowedScan {
	table: &'static str,
	reason: &'static str,
	bound: &'static str,
}

#[derive(Clone, Debug, Default)]
struct QueryPlanExpectation {
	forbid_full_scan_of: &'static [&'static str],
	forbid_temp_sort: bool,
	forbid_auto_index: bool,
	expected_index: Option<&'static str>,
	allowed_scans: &'static [AllowedScan],
}

#[derive(Debug)]
struct QueryCase {
	id: &'static str,
	sql: String,
	params: Vec<Value>,
	expectation: QueryPlanExpectation,
}

#[derive(Clone, Copy, Debug)]
struct StatementMetrics {
	fullscan_steps: i32,
	sorts: i32,
	auto_indexes: i32,
	vm_steps: i32,
}

fn indexed(
	expected_index: Option<&'static str>,
	tables: &'static [&'static str],
) -> QueryPlanExpectation {
	QueryPlanExpectation {
		forbid_full_scan_of: tables,
		forbid_temp_sort: true,
		forbid_auto_index: true,
		expected_index,
		allowed_scans: &[],
	}
}

fn bounded_scan(allowed_scans: &'static [AllowedScan]) -> QueryPlanExpectation {
	QueryPlanExpectation {
		forbid_temp_sort: true,
		forbid_auto_index: true,
		allowed_scans,
		..QueryPlanExpectation::default()
	}
}

fn text(value: &str) -> Value {
	Value::Text(value.to_owned())
}

fn fixture(row_count: usize) -> Connection {
	let mut db = Connection::open_in_memory().expect("open efficiency fixture");
	db.execute_batch(CREATE_META_TABLE)
		.expect("create real metadata schema");
	for migration in MIGRATIONS {
		for sql in *migration {
			db.execute_batch(sql).expect("apply real internal schema");
		}
	}

	let tx = db.transaction().expect("begin fixture seed");
	tx.execute(
		"INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, 42, 'token', ?)",
		[row_count as i64 + 1],
	)
	.expect("seed runtime");
	tx.execute(
		"INSERT INTO _rivet_actor (id, has_initialized, input) VALUES (1, 1, x'01')",
		[],
	)
	.expect("seed actor");
	tx.execute(
		"INSERT INTO _rivet_actor_state (id, state) VALUES (1, x'02')",
		[],
	)
	.expect("seed actor state");
	tx.execute(
		"INSERT INTO _rivet_meta (key, value) VALUES ('fixture', x'03')",
		[],
	)
	.expect("seed metadata");

	let mut conn_insert = tx
		.prepare("INSERT INTO _rivet_conns (conn_id, parameters, gateway_id, request_id, request_path, request_headers) VALUES (?, x'01', x'00000000', x'00000000', '/', x'01')")
		.expect("prepare connection seed");
	let mut conn_state_insert = tx
		.prepare("INSERT INTO _rivet_conn_state (conn_id, state, server_message_index, client_message_index, subscriptions) VALUES (?, x'01', 0, 0, x'01')")
		.expect("prepare connection state seed");
	let mut queue_insert = tx
		.prepare(
			"INSERT INTO _rivet_queue (id, name, body, created_at) VALUES (?, 'message', x'01', ?)",
		)
		.expect("prepare queue seed");
	let mut user_kv_insert = tx
		.prepare("INSERT INTO _rivet_user_kv (key, value) VALUES (?, x'01')")
		.expect("prepare user kv seed");
	let mut schedule_insert = tx
		.prepare("INSERT INTO _rivet_schedule_events (event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history) VALUES (?, ?, 'run', x'01', ?, NULL, NULL, NULL, NULL, 100)")
		.expect("prepare schedule seed");
	let mut history_insert = tx
		.prepare("INSERT INTO _rivet_schedule_history (schedule_id, action, scheduled_at, fired_at, finished_at, result) VALUES (?, 'run', ?, ?, ?, ?)")
		.expect("prepare history seed");
	for index in 0..row_count {
		let key = format!("{index:08}");
		conn_insert.execute([&key]).expect("seed connection");
		conn_state_insert
			.execute([&key])
			.expect("seed connection state");
		queue_insert
			.execute((index as i64 + 1, index as i64))
			.expect("seed queue");
		user_kv_insert
			.execute([key.as_bytes()])
			.expect("seed user kv");
		let event_id = if index % 3 == 0 {
			format!("at:{key}")
		} else {
			format!("cron:{key}")
		};
		schedule_insert
			.execute((&event_id, index as i64, (index % 3) as i64))
			.expect("seed schedule");
		let schedule_id = format!("cron:{:04}", index % 100);
		history_insert
			.execute((
				&schedule_id,
				index as i64,
				index as i64,
				index as i64,
				if index % 997 == 0 { 0_i64 } else { 1_i64 },
			))
			.expect("seed schedule history");
	}
	drop(conn_insert);
	drop(conn_state_insert);
	drop(queue_insert);
	drop(user_kv_insert);
	drop(schedule_insert);
	drop(history_insert);
	tx.commit().expect("commit fixture seed");
	db.execute_batch("ANALYZE").expect("analyze fixture");
	db
}

fn explain_plan(db: &Connection, sql: &str, params: &[Value]) -> Vec<String> {
	let mut statement = db
		.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
		.expect("prepare query plan");
	statement
		.query_map(params_from_iter(params.iter()), |row| {
			row.get::<_, String>(3)
		})
		.expect("execute query plan")
		.map(|detail| normalize_plan_detail(&detail.expect("read query plan detail")))
		.collect()
}

fn normalize_plan_detail(detail: &str) -> String {
	detail
		.to_ascii_lowercase()
		.replace(['`', '"', '[', ']'], "")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

fn plan_has_scan(plan: &[String], table: &str) -> bool {
	let table = table.to_ascii_lowercase();
	plan.iter().any(|detail| {
		(detail.starts_with("scan ") || detail.contains(" scan "))
			&& detail.split_whitespace().any(|word| word == table)
	})
}

fn assert_query_plan(
	query_id: &str,
	plan: &[String],
	expectation: &QueryPlanExpectation,
) -> Result<(), String> {
	let rendered = plan.join("\n");
	for table in expectation.forbid_full_scan_of {
		if plan_has_scan(plan, table)
			&& !expectation
				.allowed_scans
				.iter()
				.any(|allowed| allowed.table == *table)
		{
			return Err(format!(
				"{query_id}: forbidden full scan of {table}\n{rendered}"
			));
		}
	}
	if expectation.forbid_temp_sort && rendered.contains("temp b-tree") {
		return Err(format!("{query_id}: temporary sort\n{rendered}"));
	}
	if expectation.forbid_auto_index && rendered.contains("automatic") {
		return Err(format!("{query_id}: automatic index\n{rendered}"));
	}
	if let Some(expected_index) = expectation.expected_index
		&& !rendered.contains(&expected_index.to_ascii_lowercase())
	{
		return Err(format!(
			"{query_id}: expected index {expected_index}\n{rendered}"
		));
	}
	for allowed in expectation.allowed_scans {
		assert!(
			!allowed.reason.is_empty(),
			"{query_id}: scan reason is empty"
		);
		assert!(!allowed.bound.is_empty(), "{query_id}: scan bound is empty");
	}
	Ok(())
}

fn execute_with_status(db: &Connection, sql: &str, params: &[Value]) -> StatementMetrics {
	let mut statement = db.prepare(sql).expect("prepare measured statement");
	for status in [
		StatementStatus::FullscanStep,
		StatementStatus::Sort,
		StatementStatus::AutoIndex,
		StatementStatus::VmStep,
	] {
		statement.reset_status(status);
	}
	if statement.readonly() {
		let mut rows = statement
			.query(params_from_iter(params.iter()))
			.expect("execute measured query");
		while rows.next().expect("read measured row").is_some() {}
	} else {
		statement
			.execute(params_from_iter(params.iter()))
			.expect("execute measured statement");
	}
	StatementMetrics {
		fullscan_steps: statement.get_status(StatementStatus::FullscanStep),
		sorts: statement.get_status(StatementStatus::Sort),
		auto_indexes: statement.get_status(StatementStatus::AutoIndex),
		vm_steps: statement.get_status(StatementStatus::VmStep),
	}
}

fn query_catalog() -> Vec<QueryCase> {
	let all_schedules = &["_rivet_schedule_events"];
	let all_history = &["_rivet_schedule_history"];
	let all_user_kv = &["_rivet_user_kv"];
	vec![
		QueryCase {
			id: "actor.snapshot",
			sql: internal_storage::LOAD_ACTOR_SNAPSHOT_SQL.into(),
			params: vec![],
			expectation: indexed(None, &["_rivet_actor", "_rivet_actor_state"]),
		},
		QueryCase {
			id: "connection.list",
			sql: internal_storage::LOAD_CONNECTIONS_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "c",
				reason: "actor startup restores every live connection",
				bound: "rows are lifecycle-bounded to currently live or hibernated connections and are deleted on disconnect",
			}]),
		},
		QueryCase {
			id: "migration.reset_schedules",
			sql: internal_storage::RESET_SCHEDULES_FOR_LEGACY_IMPORT_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_schedule_events",
				reason: "one-time legacy import replaces the complete pre-import schedule set",
				bound: "the legacy actor snapshot containing the source schedule vector is capped at 256 KiB",
			}]),
		},
		QueryCase {
			id: "migration.reset_schedule_trace_contexts",
			sql: internal_storage::RESET_SCHEDULE_TRACE_CONTEXTS_SQL.into(),
			params: vec![],
			expectation: indexed(None, &["_rivet_meta"]),
		},
		QueryCase {
			id: "queue.next_id",
			sql: internal_storage::LOAD_QUEUE_NEXT_ID_SQL.into(),
			params: vec![],
			expectation: indexed(None, &["_rivet_runtime"]),
		},
		QueryCase {
			id: "queue.stats",
			sql: internal_storage::LOAD_QUEUE_STATS_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_queue",
				reason: "startup validates persisted queue cardinality and maximum id",
				bound: "ActorConfig.max_queue_size caps queue rows (default 1,000)",
			}]),
		},
		QueryCase {
			id: "queue.list",
			sql: internal_storage::LOAD_QUEUE_MESSAGES_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_queue",
				reason: "startup restores queued messages in primary-key order",
				bound: "ActorConfig.max_queue_size caps queue rows (default 1,000)",
			}]),
		},
		QueryCase {
			id: "queue.receive",
			sql: internal_storage::LOAD_QUEUE_MESSAGES_LIMITED_SQL.into(),
			params: vec![5_i64.into()],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_queue",
				reason: "unfiltered queue receive walks global enqueue order",
				bound: "LIMIT caps returned message bodies; ActorConfig.max_queue_size caps the queue",
			}]),
		},
		QueryCase {
			id: "queue.receive_name",
			sql: internal_storage::LOAD_QUEUE_MESSAGES_FOR_NAME_SQL.into(),
			params: vec![text("message"), 5_i64.into()],
			expectation: indexed(Some("_rivet_queue_name_id"), &["_rivet_queue"]),
		},
		QueryCase {
			id: "queue.available_name",
			sql: internal_storage::HAS_QUEUE_MESSAGES_FOR_NAME_SQL.into(),
			params: vec![text("message")],
			expectation: indexed(Some("_rivet_queue_name_id"), &["_rivet_queue"]),
		},
		QueryCase {
			id: "queue.filter_metadata_page",
			sql: internal_storage::LOAD_QUEUE_MESSAGE_METADATA_PAGE_SQL.into(),
			params: vec![0_i64.into(), 128_i64.into()],
			expectation: indexed(None, &["_rivet_queue"]),
		},
		QueryCase {
			id: "queue.load_name",
			sql: internal_storage::LOAD_QUEUE_MESSAGE_NAME_SQL.into(),
			params: vec![1_i64.into()],
			expectation: indexed(None, &["_rivet_queue"]),
		},
		QueryCase {
			id: "queue.load_ids",
			sql: internal_storage::load_queue_messages_by_ids_sql(3),
			params: vec![1_i64.into(), 2_i64.into(), 3_i64.into()],
			expectation: indexed(None, &["_rivet_queue"]),
		},
		QueryCase {
			id: "queue.available",
			sql: internal_storage::HAS_QUEUE_MESSAGES_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_queue",
				reason: "unfiltered availability checks stop at the first queue row",
				bound: "LIMIT 1",
			}]),
		},
		QueryCase {
			id: "queue.delete",
			sql: internal_storage::DELETE_QUEUE_MESSAGE_SQL.into(),
			params: vec![1_i64.into()],
			expectation: indexed(None, &["_rivet_queue"]),
		},
		QueryCase {
			id: "queue.reset",
			sql: internal_storage::RESET_QUEUE_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_queue",
				reason: "explicit reset removes the complete logical queue",
				bound: "ActorConfig.max_queue_size caps queue rows (default 1,000)",
			}]),
		},
		QueryCase {
			id: "user_kv.batch_get",
			sql: internal_storage::user_kv_batch_get_sql(3),
			params: vec![
				Value::Blob(b"00000001".to_vec()),
				Value::Blob(b"00005000".to_vec()),
				Value::Blob(b"00009999".to_vec()),
			],
			expectation: indexed(None, all_user_kv),
		},
		QueryCase {
			id: "user_kv.delete",
			sql: internal_storage::DELETE_USER_KV_SQL.into(),
			params: vec![Value::Blob(b"00000001".to_vec())],
			expectation: indexed(None, all_user_kv),
		},
		QueryCase {
			id: "user_kv.delete_range",
			sql: internal_storage::DELETE_USER_KV_RANGE_SQL.into(),
			params: vec![
				Value::Blob(b"00000010".to_vec()),
				Value::Blob(b"00000020".to_vec()),
			],
			expectation: indexed(None, all_user_kv),
		},
		QueryCase {
			id: "user_kv.list_range",
			sql: internal_storage::user_kv_list_sql("WHERE key >= ? AND key < ?", false, true),
			params: vec![
				Value::Blob(b"00000010".to_vec()),
				Value::Blob(b"00000100".to_vec()),
				20_i64.into(),
			],
			expectation: indexed(None, all_user_kv),
		},
		QueryCase {
			id: "user_kv.list_prefix_open",
			sql: internal_storage::user_kv_list_sql("WHERE key >= ?", true, true),
			params: vec![Value::Blob(b"00009900".to_vec()), 20_i64.into()],
			expectation: indexed(None, all_user_kv),
		},
		QueryCase {
			id: "user_kv.list_prefix",
			sql: internal_storage::user_kv_list_sql("WHERE key >= ? AND key < ?", false, false),
			params: vec![
				Value::Blob(b"000000".to_vec()),
				Value::Blob(b"000001".to_vec()),
			],
			expectation: indexed(None, all_user_kv),
		},
		QueryCase {
			id: "user_kv.list_all",
			sql: internal_storage::user_kv_list_sql("", false, false),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_user_kv",
				reason: "the explicit list-all API returns the complete actor KV namespace in primary-key order",
				bound: "callers can set ListOpts.limit when they do not want to materialize the complete namespace",
			}]),
		},
		QueryCase {
			id: "connection.delete_state",
			sql: internal_storage::DELETE_CONN_STATE_SQL.into(),
			params: vec![text("00000001")],
			expectation: indexed(None, &["_rivet_conn_state"]),
		},
		QueryCase {
			id: "connection.delete",
			sql: internal_storage::DELETE_CONN_SQL.into(),
			params: vec![text("00000001")],
			expectation: indexed(None, &["_rivet_conns"]),
		},
		QueryCase {
			id: "runtime.last_alarm",
			sql: internal_storage::LOAD_LAST_PUSHED_ALARM_SQL.into(),
			params: vec![],
			expectation: indexed(None, &["_rivet_runtime"]),
		},
		QueryCase {
			id: "runtime.run_wake",
			sql: internal_storage::LOAD_RUN_WAKE_AT_SQL.into(),
			params: vec![text(internal_storage::RUN_WAKE_AT_META_KEY)],
			expectation: indexed(None, &["_rivet_meta"]),
		},
		QueryCase {
			id: "runtime.inspector_token",
			sql: internal_storage::LOAD_INSPECTOR_TOKEN_SQL.into(),
			params: vec![],
			expectation: indexed(None, &["_rivet_runtime"]),
		},
		QueryCase {
			id: "meta.get",
			sql: internal_storage::LOAD_META_TEXT_SQL.into(),
			params: vec![text("fixture")],
			expectation: indexed(None, &["_rivet_meta"]),
		},
		QueryCase {
			id: "schedule.cancel",
			sql: queries::CANCEL_SCHEDULE_SQL.into(),
			params: vec![text("at:00000000"), 0_i64.into()],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.delete_orphan_trace_context",
			sql: queries::DELETE_ORPHAN_SCHEDULE_TRACE_CONTEXT_SQL.into(),
			params: vec![
				text("schedule_trace_context:at:00000000"),
				text("at:00000000"),
			],
			expectation: indexed(None, &["_rivet_meta", "_rivet_schedule_events"]),
		},
		QueryCase {
			id: "schedule.delete_trace_context",
			sql: queries::DELETE_SCHEDULE_TRACE_CONTEXT_SQL.into(),
			params: vec![text("schedule_trace_context:at:00000000")],
			expectation: indexed(None, &["_rivet_meta"]),
		},
		QueryCase {
			id: "schedule.get_one_shot",
			sql: queries::GET_SCHEDULED_EVENT_SQL.into(),
			params: vec![text("at:00000000"), 0_i64.into()],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.list_one_shots",
			sql: queries::LIST_SCHEDULED_EVENTS_SQL.into(),
			params: vec![0_i64.into()],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_schedule_events",
				reason: "the API enumerates all one-shot schedules in trigger order",
				bound: "ActorConfig.max_schedules caps all schedules (default 1,000)",
			}]),
		},
		QueryCase {
			id: "schedule.delete_cron_history",
			sql: queries::DELETE_CRON_HISTORY_SQL.into(),
			params: vec![text("cron:00000001"), text("cron:00000001"), 0_i64.into()],
			expectation: indexed(Some("_rivet_schedule_history_schedule"), all_history),
		},
		QueryCase {
			id: "schedule.delete_cron",
			sql: queries::DELETE_CRON_SQL.into(),
			params: vec![text("cron:00000001"), 0_i64.into()],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.delete_cron_if_action",
			sql: queries::DELETE_CRON_IF_ACTION_SQL.into(),
			params: vec![text("cron:00000001"), 0_i64.into(), text("run")],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.list_recurring",
			sql: queries::LIST_CRONS_SQL.into(),
			params: vec![0_i64.into()],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_schedule_events",
				reason: "the API enumerates all recurring schedules in trigger order",
				bound: "ActorConfig.max_schedules caps all schedules (default 1,000)",
			}]),
		},
		QueryCase {
			id: "schedule.history",
			sql: queries::CRON_HISTORY_SQL.into(),
			params: vec![text("cron:0001"), 20_i64.into()],
			expectation: indexed(Some("_rivet_schedule_history_schedule"), all_history),
		},
		QueryCase {
			id: "schedule.get",
			sql: queries::LOAD_SCHEDULE_SQL.into(),
			params: vec![text("cron:00000001")],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.count",
			sql: queries::COUNT_SCHEDULES_SQL.into(),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_schedule_events",
				reason: "SQLite represents its optimized COUNT(*) opcode as a covering-index scan in the query plan",
				bound: "runtime VM-step scaling verifies row-count-independent work; ActorConfig.max_schedules also caps rows",
			}]),
		},
		QueryCase {
			id: "schedule.due",
			sql: queries::TAKE_DUE_SCHEDULES_SQL.into(),
			params: vec![5_i64.into()],
			expectation: indexed(
				Some("_rivet_schedule_events_trigger_at"),
				&["_rivet_schedule_events", "_rivet_meta"],
			),
		},
		QueryCase {
			id: "schedule.claim_one_shots",
			sql: queries::claim_one_shots_sql(3),
			params: vec![
				0_i64.into(),
				text("at:00000000"),
				0_i64.into(),
				text("at:00000003"),
				3_i64.into(),
				text("at:00000006"),
				6_i64.into(),
			],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.delete_claimed_trace_contexts",
			sql: queries::delete_schedule_trace_contexts_sql(3),
			params: vec![
				text("schedule_trace_context:at:00000000"),
				text("schedule_trace_context:at:00000003"),
				text("schedule_trace_context:at:00000006"),
			],
			expectation: indexed(None, &["_rivet_meta"]),
		},
		QueryCase {
			id: "schedule.advance_skipped",
			sql: queries::ADVANCE_SKIPPED_SCHEDULE_SQL.into(),
			params: vec![20_000_i64.into(), text("cron:00000001")],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.advance",
			sql: queries::ADVANCE_SCHEDULE_SQL.into(),
			params: vec![20_000_i64.into(), 10_000_i64.into(), text("cron:00000002")],
			expectation: indexed(None, all_schedules),
		},
		QueryCase {
			id: "schedule.finish_history",
			sql: queries::FINISH_HISTORY_SQL.into(),
			params: vec![
				20_000_i64.into(),
				1_i64.into(),
				Value::Null,
				Value::Null,
				Value::Null,
				Value::Null,
				1_i64.into(),
				0_i64.into(),
			],
			expectation: indexed(None, all_history),
		},
		QueryCase {
			id: "schedule.recover_history",
			sql: queries::RECOVER_HISTORY_SQL.into(),
			params: vec![
				20_000_i64.into(),
				2_i64.into(),
				text("schedule"),
				text("interrupted"),
				text("interrupted"),
				Value::Null,
			],
			expectation: indexed(Some("_rivet_schedule_history_running"), all_history),
		},
		QueryCase {
			id: "schedule.next_future",
			sql: queries::NEXT_FUTURE_SCHEDULE_SQL.into(),
			params: vec![5_000_i64.into()],
			expectation: indexed(Some("_rivet_schedule_events_trigger_at"), all_schedules),
		},
		QueryCase {
			id: "schedule.next",
			sql: queries::NEXT_SCHEDULE_SQL.into(),
			params: vec![],
			expectation: indexed(Some("_rivet_schedule_events_trigger_at"), all_schedules),
		},
		QueryCase {
			id: "schedule.prune",
			sql: queries::PRUNE_SCHEDULE_HISTORY_SQL.into(),
			params: vec![text("cron:0001"), 20_i64.into()],
			expectation: indexed(Some("_rivet_schedule_history_schedule"), all_history),
		},
		QueryCase {
			id: "schedule.prune_global",
			sql: queries::PRUNE_GLOBAL_HISTORY_SQL.into(),
			params: vec![schedule::GLOBAL_HISTORY_RETAINED_ROWS.into()],
			expectation: QueryPlanExpectation {
				forbid_full_scan_of: all_history,
				forbid_temp_sort: true,
				forbid_auto_index: true,
				expected_index: Some("_rivet_schedule_history_fired_at"),
				allowed_scans: &[AllowedScan {
					table: "_rivet_schedule_history",
					reason: "global pruning walks the history ordering index to skip retained rows and delete only the overflow",
					bound: "periodic pruning retains 9,900 rows, leaving headroom beneath MAX_ACTOR_HISTORY",
				}],
			},
		},
		QueryCase {
			id: "cleanup.select_chunk",
			sql: internal_storage::clear_table_select_sql(
				"_rivet_user_kv",
				"key",
				"length(key) + length(value)",
				internal_storage::KV_TX_MAX_ROWS,
			),
			params: vec![],
			expectation: bounded_scan(&[AllowedScan {
				table: "_rivet_user_kv",
				reason: "interrupted legacy import cleanup walks one primary-key chunk",
				bound: "each statement is limited to KV_TX_MAX_ROWS (128)",
			}]),
		},
		QueryCase {
			id: "cleanup.delete_row",
			sql: internal_storage::clear_table_delete_sql("_rivet_user_kv", "key"),
			params: vec![Value::Blob(b"00000001".to_vec())],
			expectation: indexed(None, all_user_kv),
		},
	]
}

#[test]
fn sql_efficiency_production_queries_use_expected_plans() {
	let db = fixture(REPRESENTATIVE_ROWS);
	let mut failures = String::new();
	for case in query_catalog() {
		let plan = explain_plan(&db, &case.sql, &case.params);
		if let Err(error) = assert_query_plan(case.id, &plan, &case.expectation) {
			let _ = writeln!(failures, "{error}\n");
		}
	}
	assert!(failures.is_empty(), "query plan failures:\n{failures}");
}

#[test]
fn sql_efficiency_indexed_queries_have_no_runtime_scan_sort_or_auto_index() {
	let db = fixture(REPRESENTATIVE_ROWS);
	let mut failures = String::new();
	for case in query_catalog()
		.into_iter()
		.filter(|case| case.expectation.allowed_scans.is_empty())
	{
		let metrics = execute_with_status(&db, &case.sql, &case.params);
		if metrics.fullscan_steps != 0 || metrics.sorts != 0 || metrics.auto_indexes != 0 {
			let plan = explain_plan(&db, &case.sql, &case.params);
			let _ = writeln!(
				failures,
				"{}: metrics={metrics:?}\n{}\n",
				case.id,
				plan.join("\n")
			);
		}
	}
	assert!(
		failures.is_empty(),
		"runtime efficiency failures:\n{failures}"
	);
}

fn measured_vm_steps(row_count: usize, sql: &str, params: &[Value]) -> i32 {
	let db = fixture(row_count);
	execute_with_status(&db, sql, params).vm_steps
}

#[test]
fn sql_efficiency_hot_paths_do_not_scale_with_unrelated_rows() {
	let cases = [
		(
			"connection.delete",
			internal_storage::DELETE_CONN_SQL.to_owned(),
			vec![Value::Text("00000050".to_owned())],
		),
		(
			"queue.delete",
			internal_storage::DELETE_QUEUE_MESSAGE_SQL.to_owned(),
			vec![Value::Integer(50)],
		),
		(
			"user_kv.batch_get",
			internal_storage::user_kv_batch_get_sql(1),
			vec![Value::Blob(b"00000050".to_vec())],
		),
		(
			"schedule.get",
			queries::LOAD_SCHEDULE_SQL.to_owned(),
			vec![Value::Text("cron:00000050".to_owned())],
		),
		(
			"schedule.due",
			queries::TAKE_DUE_SCHEDULES_SQL.to_owned(),
			vec![Value::Integer(5)],
		),
	];
	for (id, sql, params) in cases {
		let small = measured_vm_steps(100, &sql, &params);
		let large = measured_vm_steps(REPRESENTATIVE_ROWS, &sql, &params);
		assert!(
			large <= small.saturating_mul(3).saturating_add(100),
			"{id} scales with unrelated cardinality: small={small}, large={large}"
		);
	}
}

#[test]
fn sql_efficiency_negative_control_detects_unindexed_access() {
	let db = fixture(REPRESENTATIVE_ROWS);
	let sql = "SELECT event_id FROM _rivet_schedule_events WHERE action = ? ORDER BY event_id";
	let params = vec![Value::Text("run".to_owned())];
	let plan = explain_plan(&db, sql, &params);
	let expectation = indexed(None, &["_rivet_schedule_events"]);
	let error = assert_query_plan("negative.unindexed_schedule_action", &plan, &expectation)
		.expect_err("negative control must detect a full scan");
	let metrics = execute_with_status(&db, sql, &params);
	assert!(error.contains("forbidden full scan"), "{error}");
	assert!(
		metrics.fullscan_steps > 0 || metrics.sorts > 0,
		"negative control must record inefficient work: {metrics:?}"
	);
}

#[tokio::test]
async fn sql_efficiency_unlimited_kv_listing_uses_one_ordered_query() {
	let ctx = crate::testing::actor_context("sql-efficiency-list", "actor", Vec::new(), "local");
	crate::actor::internal_storage::schema::ensure_internal_schema(ctx.sql())
		.await
		.expect("initialize KV listing fixture");
	let entries = (0..300)
		.map(|index| {
			(
				format!("prefix:{index:04}").into_bytes(),
				format!("value:{index:04}").into_bytes(),
			)
		})
		.collect::<Vec<_>>();
	let refs = entries
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	internal_storage::user_kv_batch_put(ctx.sql(), &refs)
		.await
		.expect("seed KV listing fixture");

	let forward = internal_storage::user_kv_list_prefix(ctx.sql(), b"prefix:", ListOpts::default())
		.await
		.expect("list all KV values forward");
	assert_eq!(forward, entries);

	let reverse = internal_storage::user_kv_list_prefix(
		ctx.sql(),
		b"prefix:",
		ListOpts {
			reverse: true,
			limit: None,
		},
	)
	.await
	.expect("list all KV values in reverse");
	assert_eq!(reverse, entries.into_iter().rev().collect::<Vec<_>>());
}
