use super::*;

mod moved_tests {
	use std::collections::HashMap;
	use std::sync::Arc as StdArc;
	use std::time::Duration;

	use rivet_error::{RivetError as RivetTransportError, RivetErrorSchema};
	use rivetkit_core::sqlite::ColumnValue;
	use rivetkit_core::testing::{ActorContextHarness, actor_context};
	use tokio::sync::oneshot;

	use super::*;

	fn test_adapter_config() -> AdapterConfig {
		let timeout = Duration::from_secs(1);
		AdapterConfig {
			create_state_timeout: timeout,
			on_create_timeout: timeout,
			create_vars_timeout: timeout,
			on_migrate_timeout: timeout,
			on_wake_timeout: timeout,
			on_before_actor_start_timeout: timeout,
			create_conn_state_timeout: timeout,
			on_before_connect_timeout: timeout,
			on_connect_timeout: timeout,
			action_timeout: timeout,
			on_request_timeout: timeout,
		}
	}

	fn empty_bindings() -> CallbackBindings {
		CallbackBindings {
			create_state: None,
			on_create: None,
			create_conn_state: None,
			create_vars: None,
			on_migrate: None,
			on_wake: None,
			on_before_actor_start: None,
			on_sleep: None,
			on_destroy: None,
			on_before_connect: None,
			on_connect: None,
			on_disconnect_final: None,
			on_before_subscribe: None,
			actions: HashMap::new(),
			on_before_action_response: None,
			on_queue_send: None,
			on_request: None,
			on_websocket: None,
			run: None,
			get_workflow_history: None,
			replay_workflow: None,
			serialize_state: None,
		}
	}

	fn assert_error_code(error: anyhow::Error, code: &str) {
		let error = RivetTransportError::extract(&error);
		assert_eq!(error.code(), code);
	}

	#[test]
	fn startup_snapshot_recovery_only_treats_empty_stateful_snapshot_as_new() {
		assert_eq!(normalize_startup_snapshot(true, Some(Vec::new())), None);
		assert_eq!(
			normalize_startup_snapshot(false, Some(Vec::new())),
			Some(Vec::new())
		);
		assert_eq!(
			normalize_startup_snapshot(true, Some(vec![1, 2, 3])),
			Some(vec![1, 2, 3])
		);
		assert_eq!(normalize_startup_snapshot(true, None), None);
	}

	fn schema_ptr(error: &anyhow::Error) -> *const RivetErrorSchema {
		error
			.chain()
			.find_map(|cause| cause.downcast_ref::<RivetTransportError>())
			.and_then(|error| error.schema())
			.map(|schema| schema as *const RivetErrorSchema)
			.expect("expected rivet error")
	}

	#[tokio::test(flavor = "current_thread")]
	async fn with_dispatch_cancel_token_cleans_up_after_success() {
		let cancel_token = with_dispatch_cancel_token(|cancel_token| async move {
			Ok::<_, anyhow::Error>(cancel_token)
		})
		.await
		.expect("successful dispatch should resolve");

		assert!(cancel_token.is_cancelled());
	}

	#[tokio::test(flavor = "current_thread")]
	async fn with_dispatch_cancel_token_cleans_up_after_panic() {
		let seen_cancel_token = StdArc::new(parking_lot::Mutex::new(None));

		let join_error = tokio::spawn({
			let seen_cancel_token = StdArc::clone(&seen_cancel_token);
			async move {
				let _ = with_dispatch_cancel_token(|cancel_token| async move {
					*seen_cancel_token.lock() = Some(cancel_token);
					panic!("dispatch panic");
					#[allow(unreachable_code)]
					Ok::<(), anyhow::Error>(())
				})
				.await;
			}
		})
		.await
		.expect_err("panic dispatch should panic");

		assert!(join_error.is_panic());
		let cancel_token = seen_cancel_token
			.lock()
			.clone()
			.expect("panic path should observe the dispatch token");
		assert!(cancel_token.is_cancelled());
	}

	#[tokio::test(flavor = "current_thread")]
	async fn with_dispatch_cancel_token_does_not_leak_under_mixed_load() {
		for i in 0..1000 {
			if i % 2 == 0 {
				let cancel_token = with_dispatch_cancel_token(|cancel_token| async move {
					Ok::<_, anyhow::Error>(cancel_token)
				})
				.await
				.expect("successful dispatch should resolve");
				assert!(cancel_token.is_cancelled());
				continue;
			}

			let join_error = tokio::spawn(async move {
				let _ = with_dispatch_cancel_token(|_| async move {
					panic!("dispatch panic");
					#[allow(unreachable_code)]
					Ok::<(), anyhow::Error>(())
				})
				.await;
			})
			.await
			.expect_err("panic dispatch should panic");

			assert!(join_error.is_panic());
		}
	}

	#[tokio::test]
	async fn action_dispatch_missing_action_returns_not_found() {
		let bindings = Arc::new(empty_bindings());
		let config = test_adapter_config();
		let core_ctx = actor_context("actor-missing-action", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let abort = CancellationToken::new();
		let dirty = Arc::new(AtomicBool::new(false));
		let mut tasks = JoinSet::new();
		let (_registered_task_tx, mut registered_task_rx) = unbounded_channel();
		let (tx, rx) = oneshot::channel();

		dispatch_event(
			ActorEvent::Action {
				name: "missing".to_owned(),
				args: vec![1, 2, 3],
				conn: None,
				scheduled_fire: None,
				reply: tx.into(),
			},
			&bindings,
			&config,
			&ctx,
			&abort,
			&mut tasks,
			&mut registered_task_rx,
			&dirty,
		)
		.await;

		drain_tasks(&mut tasks, &mut registered_task_rx).await;

		let error = rx
			.await
			.expect("action reply should resolve")
			.expect_err("missing action should error");
		let error = RivetTransportError::extract(&error);
		assert_eq!(error.code(), "action_not_found");
	}

	#[test]
	fn action_not_found_reuses_static_schema() {
		let first = action_not_found("missing-first".to_owned());
		let first_schema = schema_ptr(&first);

		for i in 0..100 {
			let error = action_not_found(format!("missing-{i}"));
			assert_eq!(schema_ptr(&error), first_schema);
		}
	}

	#[tokio::test]
	async fn subscribe_request_without_guard_is_allowed() {
		let bindings = Arc::new(empty_bindings());
		let config = test_adapter_config();
		let core_ctx = actor_context("actor-subscribe", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let abort = CancellationToken::new();
		let dirty = Arc::new(AtomicBool::new(false));
		let mut tasks = JoinSet::new();
		let (_registered_task_tx, mut registered_task_rx) = unbounded_channel();
		let (tx, rx) = oneshot::channel();
		let conn = rivetkit_core::ConnHandle::new("conn-subscribe", Vec::new(), Vec::new(), false);

		dispatch_event(
			ActorEvent::SubscribeRequest {
				conn,
				event_name: "ping".to_owned(),
				reply: tx.into(),
			},
			&bindings,
			&config,
			&ctx,
			&abort,
			&mut tasks,
			&mut registered_task_rx,
			&dirty,
		)
		.await;

		drain_tasks(&mut tasks, &mut registered_task_rx).await;

		rx.await
			.expect("subscribe reply should resolve")
			.expect("subscribe without guard should be allowed");
	}

	#[tokio::test]
	async fn connection_open_without_callbacks_is_allowed() {
		let bindings = Arc::new(empty_bindings());
		let config = test_adapter_config();
		let core_ctx = actor_context("actor-connection-open", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let abort = CancellationToken::new();
		let dirty = Arc::new(AtomicBool::new(false));
		let mut tasks = JoinSet::new();
		let (_registered_task_tx, mut registered_task_rx) = unbounded_channel();
		let (tx, rx) = oneshot::channel();
		let conn = rivetkit_core::ConnHandle::new("conn-open", vec![1, 2, 3], Vec::new(), false);

		dispatch_event(
			ActorEvent::ConnectionPreflight {
				conn: conn.clone(),
				params: vec![4, 5, 6],
				request: None,
				reply: tx.into(),
			},
			&bindings,
			&config,
			&ctx,
			&abort,
			&mut tasks,
			&mut registered_task_rx,
			&dirty,
		)
		.await;

		drain_tasks(&mut tasks, &mut registered_task_rx).await;

		rx.await
			.expect("connection-open reply should resolve")
			.expect("connection-open without callbacks should be allowed");
		assert!(conn.state().is_empty());
	}

	#[tokio::test]
	async fn workflow_requests_without_callbacks_drop_replies() {
		let bindings = Arc::new(empty_bindings());
		let config = test_adapter_config();
		let core_ctx = actor_context("actor-workflow", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let abort = CancellationToken::new();
		let dirty = Arc::new(AtomicBool::new(false));
		let mut tasks = JoinSet::new();
		let (_registered_task_tx, mut registered_task_rx) = unbounded_channel();
		let (history_tx, history_rx) = oneshot::channel();
		let (replay_tx, replay_rx) = oneshot::channel();

		dispatch_event(
			ActorEvent::WorkflowHistoryRequested {
				reply: history_tx.into(),
			},
			&bindings,
			&config,
			&ctx,
			&abort,
			&mut tasks,
			&mut registered_task_rx,
			&dirty,
		)
		.await;
		dispatch_event(
			ActorEvent::WorkflowReplayRequested {
				entry_id: Some("step-1".to_owned()),
				reply: replay_tx.into(),
			},
			&bindings,
			&config,
			&ctx,
			&abort,
			&mut tasks,
			&mut registered_task_rx,
			&dirty,
		)
		.await;

		drain_tasks(&mut tasks, &mut registered_task_rx).await;

		let history_error = history_rx
			.await
			.expect("workflow history reply should resolve")
			.expect_err("workflow history without a callback should drop its reply");
		let history_error = RivetTransportError::extract(&history_error);
		assert_eq!(history_error.group(), "actor");
		assert_eq!(history_error.code(), "dropped_reply");
		let replay_error = replay_rx
			.await
			.expect("workflow replay reply should resolve")
			.expect_err("workflow replay without a callback should drop its reply");
		let replay_error = RivetTransportError::extract(&replay_error);
		assert_eq!(replay_error.group(), "actor");
		assert_eq!(replay_error.code(), "dropped_reply");
	}

	#[tokio::test]
	async fn persisted_run_wake_is_consumed_after_the_handler_is_removed() {
		let bindings = Arc::new(empty_bindings());
		let config = test_adapter_config();
		let core_ctx = actor_context("actor-run-wake", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let abort = CancellationToken::new();
		let dirty = Arc::new(AtomicBool::new(false));
		let mut tasks = JoinSet::new();
		let (_registered_task_tx, mut registered_task_rx) = unbounded_channel();
		let (tx, rx) = oneshot::channel();

			dispatch_event(
			ActorEvent::RunWake {
				wake_at: 123,
				wake_revision: 1,
				reply: tx.into(),
			},
			&bindings,
			&config,
			&ctx,
			&abort,
			&mut tasks,
			&mut registered_task_rx,
			&dirty,
		)
		.await;

		rx.await
			.expect("run wake reply should resolve")
			.expect("an obsolete run wake should be consumed");
	}

	#[tokio::test]
	async fn suppressed_run_restores_exact_deadline_after_clearing_active_marker() {
		let core_ctx = actor_context("actor-suppressed-run", "actor", Vec::new(), "local");
		let active = RunHandlerActiveGuard::new(core_ctx.clone());
		assert!(core_ctx.run_handler_active());

		let restored = StdArc::new(parking_lot::Mutex::new(None));
		restore_suppressed_run_wake(active, false, Some((123, 7)), {
			let core_ctx = core_ctx.clone();
			let restored = StdArc::clone(&restored);
			move |wake_at, wake_revision| async move {
				assert!(!core_ctx.run_handler_active());
				*restored.lock() = Some((wake_at, wake_revision));
				Ok(())
			}
		})
		.await
		.expect("suppressed wake should restore");

		assert_eq!(*restored.lock(), Some((123, 7)));
	}

	#[tokio::test]
	async fn spawn_reply_sends_stopping_when_abort_is_cancelled() {
		let mut tasks = JoinSet::new();
		let (_registered_task_tx, mut registered_task_rx) = unbounded_channel();
		let abort = CancellationToken::new();
		let (tx, rx) = oneshot::channel();

		spawn_reply(&mut tasks, abort.clone(), tx.into(), async move {
			tokio::time::sleep(Duration::from_secs(60)).await;
			Ok::<_, anyhow::Error>(Vec::<u8>::new())
		});

		abort.cancel();
		drain_tasks(&mut tasks, &mut registered_task_rx).await;

		let error = rx
			.await
			.expect("abort reply should resolve")
			.expect_err("abort should return an error");
		let error = RivetTransportError::extract(&error);
		assert_eq!(error.code(), "stopping");
	}

	#[tokio::test]
	async fn callback_timeout_returns_structured_error_with_metadata() {
		let timeout = Duration::from_millis(10);
		let error = with_timeout("onWake", timeout, std::future::pending::<Result<()>>())
			.await
			.expect_err("callback timeout should fail");
		let error = RivetTransportError::extract(&error);
		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "callback_timed_out");
		assert_eq!(
			error.message(),
			format!(
				"callback `onWake` timed out after {} ms",
				timeout.as_millis()
			)
		);
		assert_eq!(
			error.metadata(),
			Some(serde_json::json!({
				"callback_name": "onWake",
				"duration_ms": timeout.as_millis() as u64,
			}))
		);
	}

	#[tokio::test]
	async fn structured_timeout_returns_action_timeout_error() {
		let error = with_structured_timeout(
			"actor",
			"action_timed_out",
			"Action timed out",
			None,
			Duration::from_millis(10),
			std::future::pending::<Result<()>>(),
		)
		.await
		.expect_err("structured timeout should fail");
		let error = RivetTransportError::extract(&error);
		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "action_timed_out");
		assert_eq!(error.message(), "Action timed out");
	}

	#[test]
	fn unknown_structured_timeout_uses_dynamic_error_kind() {
		let first = structured_timeout_kind("test", "slow_callback", "first message");
		assert_eq!(first.group(), "test");
		assert_eq!(first.code(), "slow_callback");
		assert_eq!(first.default_message(), "first message");
		assert!(first.schema().is_none());

		for i in 0..100 {
			let kind = structured_timeout_kind("test", "slow_callback", &format!("message {i}"));
			assert_eq!(kind.group(), "test");
			assert_eq!(kind.code(), "slow_callback");
			assert!(kind.schema().is_none());
		}
	}

	#[tokio::test]
	async fn run_adapter_loop_resets_stale_shared_end_reason_before_wake() {
		let bindings = Arc::new(empty_bindings());
		let config = Arc::new(test_adapter_config());
		let core_ctx = actor_context("actor-wake-reset", "actor", Vec::new(), "local");
		core_ctx.set_started(true);
		let stale_ctx = ActorContext::new(core_ctx.clone());
		stale_ctx.set_end_reason(EndReason::Sleep);

		let (events_tx, events_rx) = unbounded_channel();
		let (first_tx, first_rx) = oneshot::channel();
		let (second_tx, second_rx) = oneshot::channel();

		events_tx
			.send(ActorEvent::Action {
				name: "missing-first".to_owned(),
				args: Vec::new(),
				conn: None,
				scheduled_fire: None,
				reply: first_tx.into(),
			})
			.expect("first action event should send");
		events_tx
			.send(ActorEvent::Action {
				name: "missing-second".to_owned(),
				args: Vec::new(),
				conn: None,
				scheduled_fire: None,
				reply: second_tx.into(),
			})
			.expect("second action event should send");
		drop(events_tx);

		run_adapter_loop(
			bindings,
			config,
			ActorStart {
				ctx: core_ctx,
				input: None,
				is_new: true,
				snapshot: Some(Vec::new()),
				hibernated: Vec::new(),
				events: ActorEvents::from(events_rx),
				startup_ready: None,
			},
		)
		.await
		.expect("adapter loop should finish cleanly");

		let first_error = first_rx
			.await
			.expect("first action reply should resolve")
			.expect_err("missing action should error");
		assert_error_code(first_error, "action_not_found");

		let second_error = second_rx
			.await
			.expect("second action reply should resolve")
			.expect_err("second missing action should error");
		assert_error_code(second_error, "action_not_found");
		assert_eq!(stale_ctx.take_end_reason(), None);
	}

	#[tokio::test]
	async fn preamble_marks_initialized_and_reloads_as_wake_from_sqlite() {
		let harness = ActorContextHarness::new();
		let config = test_adapter_config();
		let bindings = empty_bindings();

		let first_core_ctx = harness.context("actor-preamble", "actor", Vec::new(), "local");
		let first_ctx = ActorContext::new(first_core_ctx.clone());
		first_ctx
			.set_state_initial(vec![9, 9, 9])
			.expect("initial state should set");

		run_preamble(&bindings, &config, &first_ctx, None, true, None, Vec::new())
			.await
			.expect("first-create preamble should succeed");

		let persisted = first_core_ctx
			.sql()
			.query(
				"SELECT a.has_initialized, s.state FROM _rivet_actor AS a JOIN _rivet_actor_state AS s ON s.id = a.id WHERE a.id = 1",
				None,
			)
			.await
			.expect("persisted actor should load from sqlite");
		assert_eq!(
			persisted.rows,
			vec![vec![
				ColumnValue::Integer(1),
				ColumnValue::Blob(vec![9, 9, 9]),
			]],
		);

		let second_core_ctx = harness.context("actor-preamble", "actor", Vec::new(), "local");
		second_core_ctx.set_state_initial(vec![9, 9, 9]);
		let second_ctx = ActorContext::new(second_core_ctx);

		run_preamble(
			&bindings,
			&config,
			&second_ctx,
			None,
			false,
			Some(vec![9, 9, 9]),
			Vec::new(),
		)
		.await
		.expect("wake preamble should succeed");

		assert_eq!(second_ctx.inner().state(), vec![9, 9, 9]);
	}

	#[tokio::test]
	async fn maybe_serialize_skips_save_when_adapter_is_clean() {
		let bindings = empty_bindings();
		let core_ctx = actor_context("actor-serialize-clean", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let dirty = AtomicBool::new(false);

		let deltas = maybe_serialize(&bindings, &ctx, &dirty, SerializeStateReason::Save)
			.await
			.expect("clean save serialize should not fail");

		assert!(deltas.is_empty());
		assert!(!dirty.load(Ordering::Acquire));
	}

	#[tokio::test]
	async fn maybe_serialize_inspector_does_not_consume_pending_save() {
		let bindings = empty_bindings();
		let core_ctx = actor_context("actor-serialize-inspector", "actor", Vec::new(), "local");
		let ctx = ActorContext::new(core_ctx);
		let dirty = AtomicBool::new(true);
		let calls = Arc::new(Mutex::new(Vec::new()));

		let inspector_deltas = maybe_serialize_with(
			&bindings,
			&ctx,
			&dirty,
			SerializeStateReason::Inspector,
			|_, _, reason| {
				let calls = Arc::clone(&calls);
				async move {
					calls.lock().push(reason);
					Ok(vec![StateDelta::ActorState(vec![1, 2, 3])])
				}
			},
		)
		.await
		.expect("inspector serialize should succeed");

		assert_eq!(inspector_deltas.len(), 1);
		assert!(dirty.load(Ordering::Acquire));

		let save_deltas = maybe_serialize_with(
			&bindings,
			&ctx,
			&dirty,
			SerializeStateReason::Save,
			|_, _, reason| {
				let calls = Arc::clone(&calls);
				async move {
					calls.lock().push(reason);
					Ok(vec![StateDelta::ActorState(vec![4, 5, 6])])
				}
			},
		)
		.await
		.expect("save serialize should still run after inspector");

		assert_eq!(save_deltas.len(), 1);
		assert!(!dirty.load(Ordering::Acquire));
		assert_eq!(*calls.lock(), vec!["inspector", "save"]);
	}
}
