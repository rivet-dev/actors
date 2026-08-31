use super::*;

mod moved_tests {
	use std::sync::{Arc, Mutex};
	use std::time::Duration;

	use tokio::sync::mpsc;

	use rivetkit_actor_persist::{
		generated::{v1 as persist_v1, v4 as persist_v4},
		versioned as persist_versioned,
	};
	use vbare::OwnedVersionedData;

	use tokio::sync::Semaphore;

	use crate::actor::config::ActorConfig;
	use crate::actor::connection::{ConnHandle, HibernatableConnectionMetadata};
	use crate::actor::context::tests::{
		TestSqliteWriteGate, new_with_kv, new_with_kv_and_write_gate,
	};
	use crate::actor::internal_storage;
	use crate::actor::keys::{LAST_PUSHED_ALARM_KEY, PERSIST_DATA_KEY};
	use crate::actor::messages::StateDelta;
	use crate::actor::task::LifecycleEvent;
	use crate::kv::tests::new_in_memory;
	use crate::sqlite::BindParam;
	use crate::{ActorContext, RequestSaveOpts};

	use super::{
		PersistedActor, PersistedScheduleEvent, decode_last_pushed_alarm, decode_persisted_actor,
		encode_last_pushed_alarm, encode_persisted_actor, throttled_save_delay,
	};

	const PERSISTED_ACTOR_HEX: &str =
		"04000103010203010304050601076576656e742d312a000000000000000470696e6701020708";

	fn hex(bytes: &[u8]) -> String {
		bytes.iter().map(|byte| format!("{byte:02x}")).collect()
	}

	#[test]
	fn persisted_actor_round_trips_with_embedded_version() {
		let actor = PersistedActor {
			input: Some(vec![1, 2, 3]),
			has_initialized: true,
			state: vec![4, 5, 6],
			scheduled_events: vec![PersistedScheduleEvent {
				event_id: "event-1".into(),
				timestamp: 42,
				action: "ping".into(),
				args: Some(vec![7, 8]),
			}],
		};

		let encoded = encode_persisted_actor(&actor).expect("persisted actor should encode");
		assert_eq!(hex(&encoded), PERSISTED_ACTOR_HEX);
		let decoded = decode_persisted_actor(&encoded).expect("persisted actor should decode");

		assert_eq!(decoded, actor);
	}

	#[test]
	fn persisted_actor_decodes_old_typescript_v4_optional_schedule_args() {
		let encoded = persist_versioned::Actor::wrap_latest(persist_v4::Actor {
			input: None,
			has_initialized: true,
			state: vec![1],
			scheduled_events: vec![persist_v4::ScheduleEvent {
				event_id: "event-1".to_owned(),
				timestamp: 42,
				action: "ping".to_owned(),
				args: None,
			}],
		})
		.serialize_with_embedded_version(4)
		.expect("old TypeScript actor should encode");

		let decoded = decode_persisted_actor(&encoded).expect("old TypeScript actor should decode");
		assert_eq!(
			decoded,
			PersistedActor {
				input: None,
				has_initialized: true,
				state: vec![1],
				scheduled_events: vec![PersistedScheduleEvent {
					event_id: "event-1".to_owned(),
					timestamp: 42,
					action: "ping".to_owned(),
					args: None,
				}],
			}
		);
	}

	#[test]
	fn persisted_actor_decodes_old_typescript_v1_layout() {
		let payload = persist_versioned::Actor::V1(persist_v1::PersistedActor {
			input: Some(vec![1, 2]),
			has_initialized: true,
			state: vec![3, 4],
			connections: Vec::new(),
			scheduled_events: vec![persist_v1::PersistedScheduleEvent {
				event_id: "event-1".to_owned(),
				timestamp: 42,
				kind: persist_v1::PersistedScheduleEventKind::GenericPersistedScheduleEvent(
					persist_v1::GenericPersistedScheduleEvent {
						action: "ping".to_owned(),
						args: Some(vec![5, 6]),
					},
				),
			}],
		})
		.serialize_version(1)
		.expect("old TypeScript v1 actor should encode");
		let mut encoded = 1u16.to_le_bytes().to_vec();
		encoded.extend_from_slice(&payload);

		let decoded =
			decode_persisted_actor(&encoded).expect("old TypeScript v1 actor should decode");
		assert_eq!(
			decoded,
			PersistedActor {
				input: Some(vec![1, 2]),
				has_initialized: true,
				state: vec![3, 4],
				scheduled_events: vec![PersistedScheduleEvent {
					event_id: "event-1".to_owned(),
					timestamp: 42,
					action: "ping".to_owned(),
					args: Some(vec![5, 6]),
				}],
			}
		);
	}

	#[test]
	fn persist_data_key_matches_typescript_layout() {
		assert_eq!(PERSIST_DATA_KEY, &[1]);
	}

	#[test]
	fn last_pushed_alarm_key_matches_actor_kv_layout() {
		assert_eq!(LAST_PUSHED_ALARM_KEY, &[6]);
	}

	#[test]
	fn last_pushed_alarm_round_trips_with_embedded_version() {
		let encoded = encode_last_pushed_alarm(Some(123)).expect("last pushed alarm should encode");
		let decoded = decode_last_pushed_alarm(&encoded).expect("last pushed alarm should decode");
		assert_eq!(decoded, Some(123));

		let encoded_none =
			encode_last_pushed_alarm(None).expect("empty last pushed alarm should encode");
		let decoded_none =
			decode_last_pushed_alarm(&encoded_none).expect("empty last pushed alarm should decode");
		assert_eq!(decoded_none, None);
	}

	#[test]
	fn throttled_save_delay_uses_remaining_interval() {
		let delay = throttled_save_delay(Duration::from_secs(1), Duration::from_millis(250), None);

		assert_eq!(delay, Duration::from_millis(750));
	}

	#[tokio::test]
	async fn request_save_coalesces_and_escalates_to_immediate() {
		let state = ActorContext::new_for_state_tests(new_in_memory(), ActorConfig::default());
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		state.configure_lifecycle_events(Some(events_tx));

		state.request_save(RequestSaveOpts::default());
		state.request_save(RequestSaveOpts::default());
		state.request_save(RequestSaveOpts {
			immediate: true,
			max_wait_ms: None,
		});
		state.request_save(RequestSaveOpts {
			immediate: true,
			max_wait_ms: None,
		});

		assert_eq!(
			events_rx.try_recv().expect("first save event should exist"),
			LifecycleEvent::SaveRequested { immediate: false }
		);
		assert_eq!(
			events_rx
				.try_recv()
				.expect("immediate save event should exist"),
			LifecycleEvent::SaveRequested { immediate: true }
		);
		assert!(
			events_rx.try_recv().is_err(),
			"save requests should coalesce"
		);
		assert!(state.save_requested());
		assert!(state.save_requested_immediate());
	}

	#[tokio::test]
	async fn request_save_max_wait_uses_requested_deadline() {
		let state = ActorContext::new_for_state_tests(
			new_in_memory(),
			ActorConfig {
				state_save_interval: Duration::from_secs(5),
				..ActorConfig::default()
			},
		);
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		state.configure_lifecycle_events(Some(events_tx));

		let now = std::time::Instant::now();
		state.request_save(RequestSaveOpts {
			immediate: false,
			max_wait_ms: Some(25),
		});

		assert_eq!(
			events_rx
				.try_recv()
				.expect("save-within event should exist"),
			LifecycleEvent::SaveRequested { immediate: false }
		);
		assert!(
			state.compute_save_deadline(false) <= now + Duration::from_millis(50),
			"save-within should bypass the normal throttle window"
		);
	}

	#[tokio::test]
	async fn request_save_hooks_observe_all_requests() {
		let state = ActorContext::new_for_state_tests(new_in_memory(), ActorConfig::default());
		let observed = Arc::new(Mutex::new(Vec::new()));
		state.on_request_save(Box::new({
			let observed = observed.clone();
			move |opts| {
				observed
					.lock()
					.expect("request-save hook log lock poisoned")
					.push(opts);
			}
		}));

		state.request_save(RequestSaveOpts::default());
		state.request_save(RequestSaveOpts {
			immediate: true,
			max_wait_ms: None,
		});
		state.request_save(RequestSaveOpts {
			immediate: false,
			max_wait_ms: Some(10),
		});

		assert_eq!(
			observed
				.lock()
				.expect("request-save hook log lock poisoned")
				.as_slice(),
			[
				RequestSaveOpts::default(),
				RequestSaveOpts {
					immediate: true,
					max_wait_ms: None
				},
				RequestSaveOpts {
					immediate: false,
					max_wait_ms: Some(10)
				},
			]
		);
	}

	#[tokio::test]
	async fn apply_state_deltas_writes_actor_and_connection_state() {
		let kv = new_in_memory();
		let ctx = new_with_kv("actor-1", "state-deltas", Vec::new(), "local", kv.clone());
		let conn = ConnHandle::new("conn-1", Vec::new(), vec![1, 1, 1], true);
		conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"req1",
			server_message_index: 3,
			client_message_index: 7,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(conn.clone());

		ctx.save_state(vec![
			StateDelta::ActorState(vec![1, 2, 3]),
			StateDelta::ConnHibernation {
				conn: conn.id().into(),
				bytes: vec![9, 8, 7],
			},
		])
		.await
		.expect("delta save should succeed");

		let persisted = internal_storage::load_actor_snapshot(ctx.sql())
			.await
			.expect("actor state should load")
			.expect("actor state should be persisted")
			.actor;
		assert_eq!(persisted.state, vec![1, 2, 3]);

		let persisted = internal_storage::load_connections(ctx.sql())
			.await
			.expect("connection hibernation should load")
			.into_iter()
			.find(|persisted| persisted.id == conn.id())
			.expect("connection hibernation should be persisted");
		assert_eq!(persisted.state, vec![9, 8, 7]);

		ctx.save_state(vec![StateDelta::ConnHibernationRemoved(conn.id().into())])
			.await
			.expect("hibernation delete should succeed");
		assert!(
			internal_storage::load_connections(ctx.sql())
				.await
				.expect("deleted hibernation should load")
				.into_iter()
				.all(|persisted| persisted.id != conn.id())
		);
	}

	#[tokio::test]
	async fn state_transaction_commits_user_sql_actor_state_and_dirty_connection_together() {
		let ctx = new_with_kv(
			"actor-state-tx",
			"state-tx",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute(
				"CREATE TABLE user_values (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
				None,
			)
			.await
			.expect("create user table");

		let conn = ConnHandle::new("conn-state-tx", Vec::new(), vec![1], true);
		conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"tx01",
			server_message_index: 1,
			client_message_index: 2,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(conn.clone());
		conn.set_state_initial(vec![7, 8, 9]);
		ctx.request_hibernation_transport_save(conn.id());
		let removed_conn = ConnHandle::new("conn-state-tx-removed", Vec::new(), vec![3], true);
		removed_conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"tx03",
			server_message_index: 3,
			client_message_index: 4,
			request_path: "/removed".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(removed_conn.clone());
		ctx.save_state(vec![StateDelta::ConnHibernation {
			conn: removed_conn.id().to_owned(),
			bytes: vec![3],
		}])
		.await
		.expect("seed connection that will be removed");
		ctx.request_hibernation_transport_removal(removed_conn.id().to_owned());

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute(
				"INSERT INTO user_values (id, value) VALUES (1, 'committed')",
				None,
			)
			.await
			.expect("insert user row");
		transaction
			.commit(vec![StateDelta::ActorState(vec![4, 5, 6])])
			.await
			.expect("commit state transaction");

		let rows = ctx
			.sql()
			.query("SELECT value FROM user_values WHERE id = 1", None)
			.await
			.expect("query user row");
		assert_eq!(
			rows.rows,
			vec![vec![crate::sqlite::ColumnValue::Text("committed".into())]],
		);
		let actor = internal_storage::load_actor_snapshot(ctx.sql())
			.await
			.expect("load actor snapshot")
			.expect("actor snapshot should exist")
			.actor;
		assert_eq!(actor.state, vec![4, 5, 6]);
		let connections = internal_storage::load_connections(ctx.sql())
			.await
			.expect("load connection snapshots");
		assert_eq!(connections.len(), 1);
		assert_eq!(connections[0].id, conn.id());
		assert_eq!(connections[0].state, vec![7, 8, 9]);
		assert!(!ctx.has_pending_hibernation_changes());
	}

	#[tokio::test]
	async fn state_transaction_begin_failure_restores_scheduled_state_save() {
		let ctx = new_with_kv(
			"actor-state-tx-begin-failure",
			"state-tx-begin-failure",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.set_input(Some(vec![1]));
		assert!(ctx.0.pending_save.lock().is_some());
		let epoch_before = ctx.state_transaction_epoch();

		let result = ctx.begin_state_transaction(Some(Duration::ZERO)).await;
		assert!(
			result.is_err(),
			"zero timeout must reject transaction begin"
		);

		assert!(
			ctx.0.pending_save.lock().is_some(),
			"begin failure must reschedule the save cleared before acquiring exclusion",
		);
		assert_eq!(ctx.state_transaction_epoch(), epoch_before + 2);
	}

	#[tokio::test]
	async fn state_transaction_callback_rollback_reschedules_state_and_reverts_sql() {
		let ctx = new_with_kv(
			"actor-state-tx-rollback",
			"state-tx-rollback",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute(
				"CREATE TABLE user_values (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
				None,
			)
			.await
			.expect("create user table");
		ctx.set_input(Some(vec![1]));

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute(
				"INSERT INTO user_values (id, value) VALUES (1, 'rolled-back')",
				None,
			)
			.await
			.expect("insert user row");
		transaction
			.rollback()
			.await
			.expect("roll back state transaction");

		let rows = ctx
			.sql()
			.query("SELECT value FROM user_values", None)
			.await
			.expect("query user rows");
		assert!(rows.rows.is_empty());
		assert!(ctx.0.pending_save.lock().is_some());
	}

	#[tokio::test]
	async fn save_serialized_during_state_transaction_is_discarded_after_rollback() {
		let ctx = new_with_kv(
			"actor-state-tx-stale-save",
			"state-tx-stale-save",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.save_state(vec![StateDelta::ActorState(vec![1])])
			.await
			.expect("seed actor state");

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		let serialization_epoch = ctx.state_transaction_epoch();
		let stale_save = tokio::spawn({
			let ctx = ctx.clone();
			async move {
				ctx.save_state_with_revision_at_transaction_epoch(
					vec![StateDelta::ActorState(vec![2])],
					ctx.save_request_revision(),
					serialization_epoch,
				)
				.await
			}
		});
		tokio::task::yield_now().await;
		transaction
			.rollback()
			.await
			.expect("roll back state transaction");

		assert!(!stale_save.await.expect("stale save task").expect("stale save"));
		assert_eq!(ctx.state(), vec![1]);
	}

	#[tokio::test]
	async fn state_transaction_rollback_restores_live_connection_state_at_admission() {
		let ctx = new_with_kv(
			"actor-state-tx-conn-rollback",
			"state-tx-conn-rollback",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		let conn = ConnHandle::new("conn-state-tx-rollback", Vec::new(), vec![1], true);
		ctx.add_conn(conn.clone());
		conn.set_state(vec![2]);

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		conn.set_state(vec![3]);
		transaction
			.rollback()
			.await
			.expect("roll back state transaction");

		assert_eq!(conn.state(), vec![2]);
	}

	#[tokio::test]
	async fn state_transaction_finalization_is_one_shot_across_clones() {
		let ctx = new_with_kv(
			"actor-state-tx-finalize",
			"state-tx-finalize",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		let cloned = transaction.clone();
		transaction
			.commit(Vec::new())
			.await
			.expect("first finalization should commit");

		assert!(cloned.commit(Vec::new()).await.is_err());
		assert!(cloned.rollback().await.is_err());
		ctx.sql()
			.execute("SELECT 1", None)
			.await
			.expect("coordinator should be released after one commit");
	}

	#[tokio::test]
	async fn state_transaction_combined_statement_budget_accepts_exact_boundary() {
		let ctx = new_with_kv(
			"actor-state-tx-budget-boundary",
			"state-tx-budget-boundary",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		// Actor state adds two internal upserts, reaching the combined 128-row
		// limit exactly.
		for _ in 0..126 {
			transaction
				.execute("SELECT 1", None)
				.await
				.expect("execute boundary statement");
		}
		transaction
			.commit(vec![StateDelta::ActorState(vec![1])])
			.await
			.expect("exact transaction budget boundary should commit");
		assert_eq!(
			internal_storage::load_actor_snapshot(ctx.sql())
				.await
				.expect("load actor snapshot")
				.expect("actor snapshot should exist")
				.actor
				.state,
			vec![1],
		);
	}

	#[tokio::test]
	async fn state_transaction_combined_payload_budget_accepts_exact_boundary() {
		let ctx = new_with_kv(
			"actor-state-tx-payload-boundary",
			"state-tx-payload-boundary",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		// The actor upsert contributes one eight-byte integer and the state
		// upsert contributes the one-byte state below.
		transaction
			.execute(
				"SELECT ?",
				Some(vec![BindParam::Blob(vec![
					0;
					internal_storage::KV_TX_MAX_PAYLOAD_BYTES
						- 9
				])]),
			)
			.await
			.expect("execute exact payload boundary statement");
		transaction
			.commit(vec![StateDelta::ActorState(vec![1])])
			.await
			.expect("exact payload budget boundary should commit");
	}

	#[tokio::test]
	async fn state_transaction_rejects_manual_transaction_terminators() {
		for sql in [
			"COMMIT",
			"/* leading comment */ END TRANSACTION",
			"-- leading comment\nROLLBACK TRANSACTION",
		] {
			let ctx = new_with_kv(
				format!("actor-state-tx-terminal-{sql}"),
				"state-tx-terminal",
				Vec::new(),
				"local",
				new_in_memory(),
			);
			let transaction = ctx
				.begin_state_transaction(None)
				.await
				.expect("begin state transaction");
			let error = transaction
				.execute(sql, None)
				.await
				.expect_err("manual transaction terminator must be rejected");
			assert!(
				format!("{error:#}").contains("cannot execute transaction terminator"),
				"unexpected error for {sql}: {error:#}",
			);
			transaction
				.rollback()
				.await
				.expect("rejected terminator must leave rollback available");
		}
	}

	#[tokio::test]
	async fn state_transaction_allows_rollback_to_savepoint() {
		let ctx = new_with_kv(
			"actor-state-tx-rollback-savepoint",
			"state-tx-rollback-savepoint",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute("CREATE TABLE user_values (value TEXT NOT NULL)", None)
			.await
			.expect("create user table");
		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute("SAVEPOINT nested", None)
			.await
			.expect("create savepoint");
		transaction
			.execute("INSERT INTO user_values VALUES ('discarded')", None)
			.await
			.expect("insert user row");
		transaction
			.execute(
				"ROLLBACK TRANSACTION /* comment */ TO SAVEPOINT nested",
				None,
			)
			.await
			.expect("rollback to savepoint remains supported");
		transaction
			.execute("RELEASE SAVEPOINT nested", None)
			.await
			.expect("release savepoint");
		transaction
			.commit(vec![StateDelta::ActorState(vec![9])])
			.await
			.expect("commit state transaction");

		assert!(
			ctx.sql()
				.query("SELECT value FROM user_values", None)
				.await
				.expect("query user values")
				.rows
				.is_empty()
		);
	}

	#[tokio::test]
	async fn state_transaction_counts_unbound_sql_text_toward_payload_budget() {
		let ctx = new_with_kv(
			"actor-state-tx-inline-payload",
			"state-tx-inline-payload",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute("CREATE TABLE user_values (value TEXT NOT NULL)", None)
			.await
			.expect("create user table");

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute("INSERT INTO user_values VALUES ('before')", None)
			.await
			.expect("insert user row");
		transaction
			.execute(
				format!(
					"SELECT '{}'",
					"x".repeat(internal_storage::KV_TX_MAX_PAYLOAD_BYTES)
				),
				None,
			)
			.await
			.expect("execute oversized inline literal before commit validation");
		let error = transaction
			.commit(Vec::new())
			.await
			.expect_err("inline SQL text must count toward the payload budget");
		assert!(format!("{error:#}").contains("exceeds transaction budget"));

		let rows = ctx
			.sql()
			.query("SELECT value FROM user_values", None)
			.await
			.expect("query rolled-back user rows");
		assert!(rows.rows.is_empty());
	}

	#[tokio::test]
	async fn state_transaction_combined_statement_budget_overflow_rolls_back_user_sql() {
		let ctx = new_with_kv(
			"actor-state-tx-budget-overflow",
			"state-tx-budget-overflow",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute(
				"CREATE TABLE user_values (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
				None,
			)
			.await
			.expect("create user table");
		ctx.sql()
			.execute(
				"INSERT INTO user_values (id, value) VALUES (1, 'before')",
				None,
			)
			.await
			.expect("seed user row");

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute("UPDATE user_values SET value = 'after' WHERE id = 1", None)
			.await
			.expect("update user row");
		for _ in 0..126 {
			transaction
				.execute("SELECT 1", None)
				.await
				.expect("execute counted statement");
		}
		let error = transaction
			.commit(vec![StateDelta::ActorState(vec![2])])
			.await
			.expect_err("combined state statements must overflow row budget");
		assert!(format!("{error:#}").contains("exceeds transaction budget"));

		let rows = ctx
			.sql()
			.query("SELECT value FROM user_values WHERE id = 1", None)
			.await
			.expect("query rolled-back user row");
		assert_eq!(
			rows.rows,
			vec![vec![crate::sqlite::ColumnValue::Text("before".into())]],
		);
		assert!(
			internal_storage::load_actor_snapshot(ctx.sql())
				.await
				.expect("load actor snapshot")
				.is_none(),
		);
	}

	#[tokio::test]
	async fn actor_state_payload_overflow_rolls_back_user_sql() {
		let ctx = new_with_kv(
			"actor-state-tx-state-overflow",
			"state-tx-state-overflow",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute(
				"CREATE TABLE user_values (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
				None,
			)
			.await
			.expect("create user table");
		ctx.sql()
			.execute(
				"INSERT INTO user_values (id, value) VALUES (1, 'before')",
				None,
			)
			.await
			.expect("seed user row");

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute("UPDATE user_values SET value = 'after' WHERE id = 1", None)
			.await
			.expect("update user row");
		transaction
			.execute(
				"SELECT ?",
				Some(vec![BindParam::Blob(vec![
					0;
					internal_storage::KV_TX_MAX_PAYLOAD_BYTES
						- 8
				])]),
			)
			.await
			.expect("execute user payload at pre-state limit");
		transaction
			.commit(vec![StateDelta::ActorState(vec![2])])
			.await
			.expect_err("actor state byte must overflow combined payload budget");

		let rows = ctx
			.sql()
			.query("SELECT value FROM user_values WHERE id = 1", None)
			.await
			.expect("query rolled-back user row");
		assert_eq!(
			rows.rows,
			vec![vec![crate::sqlite::ColumnValue::Text("before".into())]],
		);
		assert!(
			internal_storage::load_actor_snapshot(ctx.sql())
				.await
				.expect("load actor snapshot")
				.is_none(),
		);
	}

	#[tokio::test]
	async fn dropping_state_transaction_releases_state_save_exclusion() {
		let ctx = new_with_kv(
			"actor-state-tx-drop",
			"state-tx-drop",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		let save_guard = Arc::clone(&ctx.0.save_guard);
		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		assert!(save_guard.try_lock().is_err());

		drop(transaction);
		let acquired_save_guard =
			tokio::time::timeout(Duration::from_millis(100), save_guard.lock())
				.await
				.expect("dropping transaction must release save exclusion");
		drop(acquired_save_guard);
		tokio::time::timeout(Duration::from_secs(1), ctx.sql().execute("SELECT 1", None))
			.await
			.expect("dropping transaction must release sqlite coordinator")
			.expect("regular sqlite work should resume after dropped transaction");
	}

	#[tokio::test]
	async fn state_transaction_failure_rolls_back_sql_and_restores_hibernation_changes() {
		let ctx = new_with_kv(
			"actor-state-tx-failure",
			"state-tx-failure",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		ctx.sql()
			.execute(
				"CREATE TABLE user_values (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
				None,
			)
			.await
			.expect("create user table");
		ctx.sql()
			.execute(
				"INSERT INTO user_values (id, value) VALUES (1, 'before')",
				None,
			)
			.await
			.expect("seed user row");

		let conn = ConnHandle::new("conn-state-tx-failure", Vec::new(), vec![1], true);
		conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"tx02",
			server_message_index: 1,
			client_message_index: 2,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(conn.clone());
		conn.set_state_initial(vec![2]);
		ctx.request_hibernation_transport_save(conn.id());

		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		transaction
			.execute("UPDATE user_values SET value = 'after' WHERE id = 1", None)
			.await
			.expect("update user row");
		transaction
			.commit(vec![
				StateDelta::ActorState(vec![9]),
				StateDelta::ConnHibernation {
					conn: "missing-connection".to_owned(),
					bytes: vec![3],
				},
			])
			.await
			.expect_err("invalid hibernation delta must fail the atomic commit");

		let rows = ctx
			.sql()
			.query("SELECT value FROM user_values WHERE id = 1", None)
			.await
			.expect("query user row");
		assert_eq!(
			rows.rows,
			vec![vec![crate::sqlite::ColumnValue::Text("before".into())]],
		);
		assert!(
			internal_storage::load_actor_snapshot(ctx.sql())
				.await
				.expect("load actor snapshot")
				.is_none(),
		);
		assert!(ctx.has_pending_hibernation_changes());
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn state_mutation_after_transaction_snapshot_remains_dirty() {
		let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
		let release = Arc::new(Semaphore::new(0));
		let ctx = Arc::new(new_with_kv_and_write_gate(
			"actor-state-tx-revision",
			"state-tx-revision",
			Vec::new(),
			"local",
			new_in_memory(),
			TestSqliteWriteGate {
				sql_prefix: "INSERT INTO _rivet_actor (",
				entered_tx,
				release: release.clone(),
			},
		));
		ctx.set_input(Some(vec![1]));
		let transaction = ctx
			.begin_state_transaction(None)
			.await
			.expect("begin state transaction");
		let commit = tokio::spawn({
			let transaction = transaction.clone();
			async move {
				transaction
					.commit(vec![StateDelta::ActorState(vec![1])])
					.await
			}
		});
		entered_rx
			.recv()
			.await
			.expect("commit should reach actor snapshot write");

		ctx.set_input(Some(vec![2]));
		release.add_permits(1);
		commit
			.await
			.expect("commit task should not panic")
			.expect("state transaction should commit");

		assert!(
			ctx.0.state_dirty.load(std::sync::atomic::Ordering::SeqCst),
			"mutation after the serialized revision must remain dirty",
		);
	}

	#[tokio::test]
	async fn save_state_applies_actor_upsert_and_hibernation_delete_in_one_batch() {
		let kv = new_in_memory();
		let ctx = new_with_kv(
			"actor-batch",
			"state-batch",
			Vec::new(),
			"local",
			kv.clone(),
		);

		let removed_conn = ConnHandle::new("conn-removed", Vec::new(), vec![4, 4, 4], true);
		removed_conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"req1",
			server_message_index: 1,
			client_message_index: 1,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(removed_conn.clone());
		ctx.save_state(vec![StateDelta::ConnHibernation {
			conn: removed_conn.id().into(),
			bytes: vec![5, 5, 5],
		}])
		.await
		.expect("seed delete target should persist");

		let added_conn = ConnHandle::new("conn-added", Vec::new(), vec![6, 6, 6], true);
		added_conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"req2",
			server_message_index: 2,
			client_message_index: 2,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(added_conn.clone());

		ctx.save_state(vec![
			StateDelta::ActorState(vec![7, 8, 9]),
			StateDelta::ConnHibernation {
				conn: added_conn.id().into(),
				bytes: vec![1, 2, 3],
			},
			StateDelta::ConnHibernationRemoved(removed_conn.id().into()),
		])
		.await
		.expect("combined delta save should succeed");

		let persisted = internal_storage::load_actor_snapshot(ctx.sql())
			.await
			.expect("actor state should load")
			.expect("actor state should be persisted")
			.actor;
		assert_eq!(persisted.state, vec![7, 8, 9]);

		let connections = internal_storage::load_connections(ctx.sql())
			.await
			.expect("connection hibernation should load");
		let added = connections
			.iter()
			.find(|persisted| persisted.id == added_conn.id())
			.expect("added hibernation should exist");
		assert_eq!(added.state, vec![1, 2, 3]);

		assert!(
			connections
				.iter()
				.all(|persisted| persisted.id != removed_conn.id())
		);
	}

	// Covers the pending-write waiter contract: `wait_for_pending_state_writes`
	// must observe a save whose sqlite write is still in flight and only return
	// once that write completes. The remote sqlite executor gate stalls the
	// first save's connection insert, so the in-flight window is event-ordered
	// rather than timing-dependent.
	#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
	async fn concurrent_save_state_calls_overlap_during_sqlite_write() {
		let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
		let release = Arc::new(Semaphore::new(0));
		let ctx = Arc::new(new_with_kv_and_write_gate(
			"actor-overlap",
			"state-overlap",
			Vec::new(),
			"local",
			new_in_memory(),
			TestSqliteWriteGate {
				sql_prefix: "INSERT OR IGNORE INTO _rivet_conns",
				entered_tx,
				release: release.clone(),
			},
		));

		let conn_1 = ConnHandle::new("conn-overlap-1", Vec::new(), vec![1], true);
		conn_1.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"rq01",
			server_message_index: 1,
			client_message_index: 1,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(conn_1.clone());

		let conn_2 = ConnHandle::new("conn-overlap-2", Vec::new(), vec![2], true);
		conn_2.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: *b"gate",
			request_id: *b"rq02",
			server_message_index: 1,
			client_message_index: 1,
			request_path: "/ws".to_owned(),
			request_headers: Default::default(),
		}));
		ctx.add_conn(conn_2.clone());

		let first_save = tokio::spawn({
			let ctx = Arc::clone(&ctx);
			let conn = conn_1.id().to_owned();
			async move {
				ctx.save_state(vec![StateDelta::ConnHibernation {
					conn,
					bytes: vec![10],
				}])
				.await
				.expect("first save should succeed");
			}
		});

		entered_rx
			.recv()
			.await
			.expect("first save should reach the stalled sqlite write");

		let second_save = tokio::spawn({
			let ctx = Arc::clone(&ctx);
			let conn = conn_2.id().to_owned();
			async move {
				ctx.save_state(vec![StateDelta::ConnHibernation {
					conn,
					bytes: vec![20],
				}])
				.await
				.expect("second save should succeed");
			}
		});

		let mut wait_task = tokio::spawn({
			let ctx = Arc::clone(&ctx);
			async move {
				ctx.wait_for_pending_state_writes().await;
			}
		});
		// The first save is provably mid-write here (its insert is stalled at
		// the executor gate), so a waiter that returns within this bound did
		// not observe the in-flight write.
		assert!(
			tokio::time::timeout(Duration::from_millis(50), &mut wait_task)
				.await
				.is_err(),
			"pending-write waiters must observe the stalled in-flight write",
		);

		// One permit per gated connection insert.
		release.add_permits(2);

		first_save.await.expect("first save task should not panic");
		second_save
			.await
			.expect("second save task should not panic");
		wait_task
			.await
			.expect("pending write waiter should not panic");
		entered_rx
			.recv()
			.await
			.expect("second save should also pass the sqlite write gate");

		let connections = internal_storage::load_connections(ctx.sql())
			.await
			.expect("connection states should load");
		let conn_1_persisted = connections
			.iter()
			.find(|persisted| persisted.id == conn_1.id())
			.expect("first conn state should be persisted");
		assert_eq!(conn_1_persisted.state, vec![10]);
		let conn_2_persisted = connections
			.iter()
			.find(|persisted| persisted.id == conn_2.id())
			.expect("second conn state should be persisted");
		assert_eq!(conn_2_persisted.state, vec![20]);
	}

	#[tokio::test]
	async fn save_state_resets_pending_request_flags() {
		let ctx = new_with_kv(
			"actor-1",
			"save-state-flags",
			Vec::new(),
			"local",
			new_in_memory(),
		);
		let (events_tx, _events_rx) = mpsc::unbounded_channel();
		ctx.configure_lifecycle_events(Some(events_tx));

		ctx.request_save(RequestSaveOpts {
			immediate: true,
			max_wait_ms: None,
		});
		assert!(ctx.save_requested());
		assert!(ctx.save_requested_immediate());

		ctx.save_state(vec![StateDelta::ActorState(vec![4, 5, 6])])
			.await
			.expect("bypass save should succeed");

		assert!(!ctx.save_requested());
		assert!(!ctx.save_requested_immediate());
	}

	#[tokio::test]
	async fn flush_on_shutdown_tracks_immediate_persist_until_teardown() {
		let kv = new_in_memory();
		let state = new_with_kv("state-test", "state-test", Vec::new(), "local", kv.clone());

		state.set_initial_state(vec![7, 8, 9]);
		state.flush_on_shutdown();

		assert!(state.tracked_persist_pending());

		state.wait_for_pending_writes().await;
		assert!(!state.tracked_persist_pending());

		let persisted = internal_storage::load_actor_snapshot(state.sql())
			.await
			.expect("actor state should load")
			.expect("actor state should be persisted")
			.actor;
		assert_eq!(persisted.state, vec![7, 8, 9]);
	}
}
