use super::*;

#[path = "metrics_helpers.rs"]
mod metrics_helpers;

mod moved_tests {
	#[cfg(feature = "sqlite-local")]
	use std::collections::BTreeMap;
	use std::panic::{AssertUnwindSafe, catch_unwind};
	#[cfg(feature = "sqlite-local")]
	use std::sync::Arc;
	use std::time::Duration;

	use rivet_metrics::prometheus::{IntGauge, Opts, Registry};

	use crate::actor::task_types::UserTaskKind;

	use super::metrics_helpers::{
		metric_line_for_actor, metric_name_matches, render_global_metrics,
	};
	use super::*;

	#[cfg(feature = "sqlite-local")]
	#[derive(Default)]
	struct InMemorySqliteTransport {
		state: parking_lot::Mutex<InMemorySqliteState>,
	}

	#[cfg(feature = "sqlite-local")]
	#[derive(Default)]
	struct InMemorySqliteState {
		pages: BTreeMap<u32, Vec<u8>>,
		head_txid: u64,
	}

	#[cfg(feature = "sqlite-local")]
	#[async_trait::async_trait]
	impl depot_client::vfs::SqliteTransport for InMemorySqliteTransport {
		async fn get_pages(
			&self,
			request: rivet_envoy_client::protocol::SqliteGetPagesRequest,
		) -> anyhow::Result<rivet_envoy_client::protocol::SqliteGetPagesResponse> {
			let state = self.state.lock();
			Ok(
				rivet_envoy_client::protocol::SqliteGetPagesResponse::SqliteGetPagesOk(
					rivet_envoy_client::protocol::SqliteGetPagesOk {
						pages: request
							.pgnos
							.into_iter()
							.map(|pgno| rivet_envoy_client::protocol::SqliteFetchedPage {
								pgno,
								bytes: state.pages.get(&pgno).cloned(),
							})
							.collect(),
						head_txid: Some(state.head_txid),
					},
				),
			)
		}

		async fn commit(
			&self,
			request: rivet_envoy_client::protocol::SqliteCommitRequest,
		) -> anyhow::Result<rivet_envoy_client::protocol::SqliteCommitResponse> {
			let mut state = self.state.lock();
			for page in request.dirty_pages {
				state.pages.insert(page.pgno, page.bytes);
			}
			state.pages.retain(|pgno, _| *pgno <= request.db_size_pages);
			state.head_txid = state.head_txid.saturating_add(1);
			Ok(
				rivet_envoy_client::protocol::SqliteCommitResponse::SqliteCommitOk(
					rivet_envoy_client::protocol::SqliteCommitOk {
						head_txid: Some(state.head_txid),
					},
				),
			)
		}
	}

	#[test]
	fn duplicate_metric_registration_uses_noop_fallback() {
		let registry = Registry::new();
		let first = IntGauge::with_opts(Opts::new(
			"duplicate_actor_metric",
			"first duplicate metric",
		))
		.expect("first gauge should be valid");
		let second = IntGauge::with_opts(Opts::new(
			"duplicate_actor_metric",
			"second duplicate metric",
		))
		.expect("second gauge should be valid");

		register_metric(&registry, first.clone());
		let result = catch_unwind(AssertUnwindSafe(|| {
			register_metric(&registry, second.clone());
		}));

		assert!(result.is_ok());
		assert_eq!(
			1,
			registry
				.gather()
				.iter()
				.filter(|family| family.name() == "duplicate_actor_metric")
				.count()
		);
	}

	#[test]
	fn actor_startup_duration_metrics_render() {
		let actor_name = "counter-startup";
		let metrics = ActorMetrics::new(actor_name);

		metrics.observe_create_state(Duration::from_millis(10));
		metrics.observe_create_vars(Duration::from_millis(20));
		metrics.observe_startup_phase(
			startup_phase::StartupPhase::RuntimePreamble,
			Some(true),
			"success",
			Duration::from_millis(30),
		);

		let rendered = render_global_metrics();
		assert_metric_value(
			&rendered,
			"rivetkit_actor_create_state_duration_seconds_count",
			actor_name,
			"1",
		);
		assert_metric_value(
			&rendered,
			"rivetkit_actor_create_vars_duration_seconds_count",
			actor_name,
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_startup_phase_duration_seconds_count",
			actor_name,
			&[
				"phase=\"runtime_preamble\"",
				"is_new=\"true\"",
				"outcome=\"success\"",
			],
			"1",
		);
	}

	#[test]
	fn startup_timer_records_total_success_and_error() {
		let success_actor = "counter-startup-total-success";
		let success_metrics = ActorMetrics::new(success_actor);
		let mut success_timer = success_metrics.begin_startup_timer();
		success_timer.set_is_new(true);
		success_timer.finish_success();

		let error_actor = "counter-startup-total-error";
		let error_metrics = ActorMetrics::new(error_actor);
		{
			let mut error_timer = error_metrics.begin_startup_timer();
			error_timer.set_is_new(false);
		}

		let rendered = render_global_metrics();
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_startup_phase_duration_seconds_count",
			success_actor,
			&["phase=\"total\"", "is_new=\"true\"", "outcome=\"success\""],
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_startup_phase_duration_seconds_count",
			error_actor,
			&["phase=\"total\"", "is_new=\"false\"", "outcome=\"error\""],
			"1",
		);
	}

	#[cfg(feature = "sqlite-local")]
	#[test]
	fn sqlite_metrics_render_lifecycle_and_startup_kind_labels() {
		let actor_name = "counter-sqlite-labels";
		let metrics = ActorMetrics::new(actor_name);

		metrics.begin_startup();
		metrics.set_startup_is_new(false);
		metrics.set_startup_phase(startup_phase::StartupPhase::RuntimePreamble);
		depot_client::vfs::SqliteVfsMetrics::record_get_pages_request(&metrics, 2, 1, 4096);
		depot_client::vfs::SqliteVfsMetrics::observe_open_phase(
			&metrics,
			depot_client::vfs::SqliteOpenPhase::InitialPreload,
			"success",
			Duration::from_millis(5).as_nanos() as u64,
		);
		depot_client::vfs::SqliteVfsMetrics::record_startup_preload_pages(&metrics, "requested", 2);
		metrics.finish_startup();
		depot_client::vfs::SqliteVfsMetrics::record_get_pages_request(&metrics, 1, 0, 4096);

		let rendered = render_global_metrics();
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_sqlite_vfs_get_pages_total",
			actor_name,
			&[
				"actor_lifecycle_bucket=\"runtime_preamble\"",
				"is_new=\"false\"",
			],
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_sqlite_vfs_get_pages_total",
			actor_name,
			&["actor_lifecycle_bucket=\"ready_0_1s\"", "is_new=\"false\""],
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_sqlite_open_phase_duration_seconds_count",
			actor_name,
			&[
				"phase=\"initial_preload\"",
				"is_new=\"false\"",
				"outcome=\"success\"",
			],
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivetkit_actor_sqlite_startup_preload_pages_total",
			actor_name,
			&["is_new=\"false\"", "kind=\"requested\""],
			"2",
		);
	}

	#[cfg(feature = "sqlite-local")]
	#[test]
	fn sqlite_profile_reuses_pre_resolved_metric_handles() {
		let actor_name = "counter-sqlite-pre-resolved-handles";
		let first = ActorMetrics::new(actor_name);
		let second = ActorMetrics::new(actor_name);

		let first_low_card = first
			.sqlite_low_card_handles("proxy")
			.expect("low-cardinality handles should be admitted");
		let second_low_card = second
			.sqlite_low_card_handles("proxy")
			.expect("low-cardinality handles should be reused");
		assert!(std::ptr::eq(first_low_card, second_low_card));

		let first_fingerprint = first
			.admit_sqlite_fingerprint_tuple(
				"statement",
				"select-pre-resolved-handles",
				"query",
				"autocommit",
				"proxy",
			)
			.expect("fingerprint handles should be admitted");
		let second_fingerprint = second
			.admit_sqlite_fingerprint_tuple(
				"statement",
				"select-pre-resolved-handles",
				"query",
				"autocommit",
				"proxy",
			)
			.expect("fingerprint handles should be reused");
		assert!(Arc::ptr_eq(&first_fingerprint, &second_fingerprint));
	}

	#[cfg(feature = "sqlite-local")]
	#[test]
	fn sqlite_profiling_metrics_render_statement_and_transaction_end_to_end() {
		use depot_client::vfs::{
			SqliteGetPagesProfile, SqliteOperationMetric, SqliteOperationProfile,
			SqliteTransactionMetric, SqliteVfsMetrics,
		};

		let actor_name = "counter-sqlite-profile-e2e";
		let metrics = ActorMetrics::new(actor_name);
		let statement_profile = SqliteOperationProfile {
			worker_wait_ns: 2_000_000,
			storage_ns: 3_000_000,
			sqlite_requested_pages: 3,
			cache_hit_pages: 1,
			cache_miss_pages: 2,
			depot_demand_requested_pages: 2,
			response_present_pages: 3,
			overflow_expansion_extra_pages: 1,
			storage_response_bytes: 12_288,
			bind_count: 1,
			bind_logical_bytes: 8,
			result_rows: 3,
			result_columns: 2,
			result_logical_bytes: 48,
			get_pages_round_trips: 1,
			get_pages_requests: std::array::from_fn(|index| {
				(index == 0).then_some(SqliteGetPagesProfile {
					ordinal: 1,
					duration_ns: 3_000_000,
					demand_requested: 2,
					response_present: 3,
					overflow_expansion_extra: 1,
					response_bytes: 12_288,
					success: true,
					..Default::default()
				})
			}),
			..Default::default()
		};
		metrics.observe_operation_profile(&SqliteOperationMetric {
			operation_type: "statement",
			fingerprint: "select-e2e000000000001".to_owned(),
			fingerprint_source: "query",
			transaction_mode: "autocommit",
			storage_transport: "proxy",
			outcome: "success",
			sql_bytes: 32,
			total_ns: 10_000_000,
			transaction_wait_ns: 1_000_000,
			profile: statement_profile,
		});
		metrics.observe_transaction_profile(&SqliteTransactionMetric {
			fingerprint: "txn-e2e0000000000002".to_owned(),
			fingerprint_source: "name",
			shape_fingerprint: "shape-e2e000000000003".to_owned(),
			statement_fingerprint_hashes: [None;
				depot_client::vfs::MAX_PROFILED_TRANSACTION_STATEMENTS],
			omitted_statement_fingerprints: 0,
			storage_transport: "proxy",
			outcome: "rollback",
			total_ns: 25_000_000,
			transaction_wait_ns: 2_000_000,
			worker_wait_ns: 3_000_000,
			storage_ns: 4_000_000,
			local_work_ns: 5_000_000,
			application_time_ns: 11_000_000,
			commit_ns: 0,
			get_pages_round_trips: 2,
			statement_count: 3,
			dirty_pages: 2,
			dirty_bytes: 8_192,
		});

		let rendered = render_global_metrics();
		assert_metric_value_with_labels(
			&rendered,
			"rivet_rivetkit_sqlite_duration_seconds_count",
			actor_name,
			&[
				"type=\"statement\"",
				"fingerprint=\"select-e2e000000000001\"",
				"outcome_class=\"success\"",
			],
			"1",
		);
		for phase in ["transaction_wait", "worker_wait", "storage", "local_work"] {
			assert_metric_value_with_labels(
				&rendered,
				"rivet_rivetkit_sqlite_phase_duration_seconds_count",
				actor_name,
				&["type=\"statement\"", &format!("phase=\"{phase}\"")],
				"1",
			);
		}
		assert_metric_value_with_labels(
			&rendered,
			"rivet_rivetkit_sqlite_outcome_total",
			actor_name,
			&["type=\"transaction\"", "outcome=\"rollback\""],
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivet_rivetkit_sqlite_transaction_statement_count_count",
			actor_name,
			&["fingerprint=\"txn-e2e0000000000002\""],
			"1",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivet_rivetkit_sqlite_local_pages_total",
			actor_name,
			&["type=\"statement\"", "page_kind=\"cache_miss\""],
			"2",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivet_rivetkit_sqlite_local_bytes_total",
			actor_name,
			&["type=\"statement\"", "byte_kind=\"result_logical\""],
			"48",
		);
		assert_metric_value_with_labels(
			&rendered,
			"rivet_rivetkit_sqlite_get_pages_duration_seconds_count",
			actor_name,
			&["request_ordinal=\"1\"", "outcome_class=\"success\""],
			"1",
		);
		assert!(
			std::mem::size_of::<SqliteOperationProfile>() < 2 * 1024,
			"per-operation storage profile must stay below 2 KiB",
		);
	}

	#[cfg(feature = "sqlite-local")]
	#[test]
	fn sqlite_fast_statement_requires_repetition_before_fingerprint_admission() {
		use depot_client::vfs::{SqliteOperationMetric, SqliteVfsMetrics};

		let actor_name = "counter-sqlite-profile-repetition";
		let metrics = ActorMetrics::new_with_sqlite_profiling(
			actor_name,
			crate::SqliteProfilingConfig {
				slow_operation_threshold_ms: 100,
				..Default::default()
			},
		);
		let profile = SqliteOperationMetric {
			operation_type: "statement",
			fingerprint: "select-repeat00000001".to_owned(),
			fingerprint_source: "query",
			transaction_mode: "autocommit",
			storage_transport: "proxy",
			outcome: "success",
			sql_bytes: 8,
			total_ns: 1_000_000,
			transaction_wait_ns: 0,
			profile: Default::default(),
		};

		assert!(!metrics.observe_operation_profile(&profile));
		assert!(metrics.observe_operation_profile(&profile));

		let rendered = render_global_metrics();
		assert!(rendered.lines().any(|line| {
			line.starts_with("rivet_rivetkit_sqlite_duration_seconds_count{")
				&& line.contains(&format!("actor_name=\"{actor_name}\""))
				&& line.contains("fingerprint=\"other\"")
		}));
		assert!(rendered.lines().any(|line| {
			line.starts_with("rivet_rivetkit_sqlite_duration_seconds_count{")
				&& line.contains(&format!("actor_name=\"{actor_name}\""))
				&& line.contains("fingerprint=\"select-repeat00000001\"")
		}));
	}

	#[cfg(feature = "sqlite-local")]
	#[test]
	fn sqlite_disabled_profiling_records_no_profile_metrics_or_events() {
		use depot_client::vfs::{SqliteOperationMetric, SqliteVfsMetrics};

		let actor_name = "counter-sqlite-profile-disabled";
		let metrics = ActorMetrics::new_with_sqlite_profiling(
			actor_name,
			crate::SqliteProfilingConfig {
				enabled: false,
				..Default::default()
			},
		);
		let profile = SqliteOperationMetric {
			operation_type: "statement",
			fingerprint: "select-disabled000001".to_owned(),
			fingerprint_source: "query",
			transaction_mode: "autocommit",
			storage_transport: "proxy",
			outcome: "error",
			sql_bytes: 8,
			total_ns: 100_000_000,
			transaction_wait_ns: 0,
			profile: Default::default(),
		};

		assert!(!metrics.observe_operation_profile(&profile));
		metrics.set_worker_queue_depth(1);
		metrics.set_worker_inflight(true);
		metrics.set_coordinator_queue_depth(1);
		metrics.emit_operation_diagnostic_event("private-actor-id", Some(1), &profile);

		let rendered = render_global_metrics();
		assert!(!rendered.lines().any(|line| {
			line.contains(&format!("actor_name=\"{actor_name}\""))
				&& line.starts_with("rivet_rivetkit_sqlite_")
		}));
	}

	#[cfg(feature = "sqlite-local")]
	#[test]
	fn sqlite_diagnostic_rate_limit_reports_dropped_events() {
		use depot_client::vfs::{SqliteOperationMetric, SqliteVfsMetrics};

		let actor_name = "counter-sqlite-profile-event-drop";
		let metrics = ActorMetrics::new_with_sqlite_profiling(
			actor_name,
			crate::SqliteProfilingConfig {
				slow_operation_threshold_ms: 0,
				max_diagnostic_events_per_minute: 0,
				..Default::default()
			},
		);
		metrics.emit_operation_diagnostic_event(
			"private-actor-id",
			Some(1),
			&SqliteOperationMetric {
				operation_type: "statement",
				fingerprint: "select-drop0000000001".to_owned(),
				fingerprint_source: "query",
				transaction_mode: "autocommit",
				storage_transport: "proxy",
				outcome: "success",
				sql_bytes: 8,
				total_ns: 1,
				transaction_wait_ns: 0,
				profile: Default::default(),
			},
		);

		let rendered = render_global_metrics();
		assert_metric_value_with_label(
			&rendered,
			"rivet_rivetkit_sqlite_event_dropped_total",
			actor_name,
			"reason=\"rate_limit\"",
			"1",
		);
	}

	#[cfg(feature = "sqlite-local")]
	#[tokio::test(flavor = "multi_thread")]
	async fn sqlite_operations_report_profiles_through_the_native_stack() {
		use depot_client::vfs::SqliteVfsMetrics;

		let actor_name = "counter-sqlite-native-stack";
		let actor_id = "counter-sqlite-native-stack-id";
		let profiling = crate::SqliteProfilingConfig {
			slow_operation_threshold_ms: u64::MAX,
			baseline_sample_rate: 0.0,
			max_diagnostic_events_per_minute: 0,
			..Default::default()
		};
		let metrics = Arc::new(ActorMetrics::new_with_sqlite_profiling(
			actor_name,
			profiling.clone(),
		));
		let metric_sink: Arc<dyn SqliteVfsMetrics> = metrics.clone();
		let transport = Arc::new(InMemorySqliteTransport::default());
		let native_db = depot_client::database::open_database_from_transport(
			transport.clone(),
			actor_id.to_owned(),
			1,
			tokio::runtime::Handle::current(),
			Some(metric_sink.clone()),
		)
		.await
		.expect("native database should open");
		let db = crate::SqliteDb::from_native_database_for_test(
			actor_id,
			1,
			native_db,
			metric_sink.clone(),
			profiling,
		);

		db.exec("CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL)")
			.await
			.expect("table should be created");
		for (id, value) in [(1, "alpha"), (2, "beta")] {
			db.execute(
				"INSERT INTO items (id, value) VALUES (?, ?)",
				Some(vec![
					crate::BindParam::Integer(id),
					crate::BindParam::Text(value.to_owned()),
				]),
			)
			.await
			.expect("row should be inserted");
		}
		let first = db
			.query(
				"SELECT value FROM items WHERE id = ?",
				Some(vec![crate::BindParam::Integer(1)]),
			)
			.await
			.expect("first query should execute");
		let second = db
			.query(
				"SELECT value FROM items WHERE id = ?",
				Some(vec![crate::BindParam::Integer(2)]),
			)
			.await
			.expect("second query should execute");
		assert_eq!(first.rows.len(), 1);
		assert_eq!(second.rows.len(), 1);
		db.exec(
			"WITH RECURSIVE ids(id) AS (SELECT 3 UNION ALL SELECT id + 1 FROM ids WHERE id < 220) INSERT INTO items (id, value) SELECT id, zeroblob(4096) FROM ids",
		)
		.await
		.expect("database should grow beyond the preload window");

		let transaction = db
			.begin_named_transaction(Some("update-item"), None)
			.await
			.expect("named transaction should begin");
		transaction
			.execute(
				"UPDATE items SET value = ? WHERE id = ?",
				Some(vec![
					crate::BindParam::Text("updated".to_owned()),
					crate::BindParam::Integer(1),
				]),
			)
			.await
			.expect("transaction statement should execute");
		transaction
			.commit()
			.await
			.expect("transaction should commit");
		db.close()
			.await
			.expect("first native database should close");

		let reopened_native_db = depot_client::database::open_database_from_transport(
			transport,
			actor_id.to_owned(),
			2,
			tokio::runtime::Handle::current(),
			Some(metric_sink.clone()),
		)
		.await
		.expect("native database should reopen");
		let reopened = crate::SqliteDb::from_native_database_for_test(
			actor_id,
			2,
			reopened_native_db,
			metric_sink,
			crate::SqliteProfilingConfig {
				slow_operation_threshold_ms: u64::MAX,
				baseline_sample_rate: 0.0,
				max_diagnostic_events_per_minute: 0,
				..Default::default()
			},
		);
		let cold_result = reopened
			.query(
				"SELECT value FROM items WHERE id = ?",
				Some(vec![crate::BindParam::Integer(220)]),
			)
			.await
			.expect("cold query should cross the VFS transport");
		assert_eq!(cold_result.rows.len(), 1);
		reopened
			.close()
			.await
			.expect("reopened native database should close");

		let rendered = render_global_metrics();
		assert!(
			rendered.lines().any(|line| {
				line.starts_with("rivet_rivetkit_sqlite_duration_seconds_count{")
					&& line.contains(&format!("actor_name=\"{actor_name}\""))
					&& line.contains("type=\"statement\"")
					&& line.contains("fingerprint=\"select-")
					&& line.ends_with(" 2")
			}),
			"repeated executions of one query should render twice under one admitted select fingerprint:\n{rendered}"
		);
		assert!(
			rendered.lines().any(|line| {
				line.starts_with("rivet_rivetkit_sqlite_outcome_total{")
					&& line.contains(&format!("actor_name=\"{actor_name}\""))
					&& line.contains("type=\"transaction\"")
					&& line.contains("fingerprint=\"txn-")
					&& line.contains("outcome=\"success\"")
					&& line.ends_with(" 1")
			}),
			"named transaction outcome should render:\n{rendered}"
		);
		for byte_kind in [
			"bind_logical",
			"result_logical",
			"storage_response",
			"dirty",
		] {
			assert!(
				rendered.lines().any(|line| {
					line.starts_with("rivet_rivetkit_sqlite_local_bytes_total{")
						&& line.contains(&format!("actor_name=\"{actor_name}\""))
						&& line.contains(&format!("byte_kind=\"{byte_kind}\""))
				}),
				"{byte_kind} should be reported through the native stack:\n{rendered}"
			);
		}
	}

	#[test]
	fn actor_active_count_tracks_metric_lifetime() {
		let actor_name = "counter-active";
		let metrics = ActorMetrics::new(actor_name);

		let rendered = render_global_metrics();
		let line = rendered
			.lines()
			.find(|line| metric_line_for_actor(line, "rivetkit_actor_active_count", actor_name))
			.expect("active actor count metric should render");
		assert!(line.ends_with('1'), "actor should be active: {line}");

		drop(metrics);

		let rendered = render_global_metrics();
		let line = rendered
			.lines()
			.find(|line| metric_line_for_actor(line, "rivetkit_actor_active_count", actor_name))
			.expect("inactive actor count metric should remain");
		assert!(line.ends_with('0'), "actor should be inactive: {line}");
	}

	#[test]
	fn actor_current_gauges_aggregate_by_actor_name() {
		let actor_name = "counter-gauge-aggregate";
		let first = ActorMetrics::new(actor_name);
		let second = ActorMetrics::new(actor_name);

		first.set_active_connections(2);
		second.set_active_connections(3);
		first.set_queue_depth(4);
		second.set_queue_depth(5);
		first.set_dispatch_inbox_depth(6);
		second.set_dispatch_inbox_depth(7);
		first.begin_user_task(UserTaskKind::Action);
		second.begin_user_task(UserTaskKind::Action);

		let rendered = render_global_metrics();
		assert_metric_value(
			&rendered,
			"rivetkit_actor_connections_active",
			actor_name,
			"5",
		);
		assert_metric_value(&rendered, "rivetkit_actor_queue_depth", actor_name, "9");
		assert_metric_value_with_label(
			&rendered,
			"rivetkit_actor_inbox_depth",
			actor_name,
			"inbox=\"dispatch\"",
			"13",
		);
		assert_metric_value_with_label(
			&rendered,
			"rivetkit_actor_user_tasks_active",
			actor_name,
			"kind=\"action\"",
			"2",
		);

		first.set_active_connections(1);
		first.end_user_task(UserTaskKind::Action, Duration::from_millis(1));
		drop(first);

		let rendered = render_global_metrics();
		assert_metric_value(
			&rendered,
			"rivetkit_actor_connections_active",
			actor_name,
			"3",
		);
		assert_metric_value(&rendered, "rivetkit_actor_queue_depth", actor_name, "5");
		assert_metric_value_with_label(
			&rendered,
			"rivetkit_actor_user_tasks_active",
			actor_name,
			"kind=\"action\"",
			"1",
		);

		drop(second);

		let rendered = render_global_metrics();
		assert_metric_value(
			&rendered,
			"rivetkit_actor_connections_active",
			actor_name,
			"0",
		);
		assert_metric_value(&rendered, "rivetkit_actor_queue_depth", actor_name, "0");
		assert_metric_value_with_label(
			&rendered,
			"rivetkit_actor_user_tasks_active",
			actor_name,
			"kind=\"action\"",
			"0",
		);
	}

	fn assert_metric_value(metrics: &str, name: &str, actor_name: &str, value: &str) {
		assert_metric_value_with_label(metrics, name, actor_name, "", value);
	}

	fn assert_metric_value_with_label(
		metrics: &str,
		name: &str,
		actor_name: &str,
		label: &str,
		value: &str,
	) {
		let labels = if label.is_empty() {
			Vec::new()
		} else {
			vec![label]
		};
		assert_metric_value_with_labels(metrics, name, actor_name, &labels, value);
	}

	fn assert_metric_value_with_labels(
		metrics: &str,
		name: &str,
		actor_name: &str,
		labels: &[&str],
		value: &str,
	) {
		let line = metrics
			.lines()
			.find(|line| {
				metric_name_matches(line, name)
					&& line.contains(&format!("actor_name=\"{actor_name}\""))
					&& labels.iter().all(|label| line.contains(label))
			})
			.unwrap_or_else(|| panic!("{name} should render"));
		assert!(
			line.ends_with(value),
			"{name} should have value {value}: {line}"
		);
	}
}
