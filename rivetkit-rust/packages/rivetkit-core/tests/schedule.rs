use super::*;

mod moved_tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::time::Duration;

	use anyhow::anyhow;
	use chrono::{TimeZone, Utc};
	use rivet_error::RivetError;
	use rivet_error::{MacroMarker, RivetErrorKind, RivetErrorSchema};
	use tokio::task::yield_now;
	use tokio::time::advance;

	use super::*;
	use crate::ActorConfig;
	use crate::actor::schedule::{
		DEFAULT_MAX_HISTORY, GLOBAL_HISTORY_PRUNE_INTERVAL, MIN_INTERVAL_MS,
	};
	use crate::testing::{ActorContextHarness, actor_context};

	const BASE_TIME: i64 = 1_700_000_000_000;
	static ACTION_NOT_FOUND_SCHEMA: RivetErrorSchema = RivetErrorSchema {
		group: "actor",
		code: "action_not_found",
		default_message: "Action not found",
		meta_type: None,
		_macro_marker: MacroMarker { _private: () },
	};
	static PUBLIC_SCHEDULE_ERROR_SCHEMA: RivetErrorSchema = RivetErrorSchema {
		group: "schedule",
		code: "test_failure",
		default_message: "Scheduled action failed",
		meta_type: None,
		_macro_marker: MacroMarker { _private: () },
	};

	fn context(actor_id: &str) -> ActorContext {
		let ctx = actor_context(actor_id, "schedule-test", Vec::new(), "local");
		ctx.set_schedule_time_for_tests(BASE_TIME);
		ctx
	}

	fn error_code(error: &anyhow::Error) -> (String, String) {
		let error = RivetError::extract(error);
		(error.group().to_owned(), error.code().to_owned())
	}

	fn utc_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
		Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
			.single()
			.expect("valid UTC test timestamp")
			.timestamp_millis()
	}

	async fn advance_schedule_time(ctx: &ActorContext, timestamp_ms: i64, duration: Duration) {
		ctx.set_schedule_time_for_tests(timestamp_ms);
		advance(duration).await;
		yield_now().await;
	}

	#[tokio::test]
	async fn one_shot_crud_is_sqlite_backed() {
		let harness = ActorContextHarness::new();
		let ctx = harness.context("actor-one-shot", "actor", Vec::new(), "local");
		ctx.set_schedule_time_for_tests(BASE_TIME);

		let first = ctx
			.after(Duration::from_secs(5), "first", &[1, 2])
			.await
			.expect("schedule after");
		let second = ctx
			.at(BASE_TIME + 10_000, "second", &[])
			.await
			.expect("schedule at");

		let other = harness.context("actor-one-shot", "actor", Vec::new(), "local");
		let listed = other.list_scheduled_events().await.expect("list schedules");
		assert_eq!(listed.len(), 2);
		assert_eq!(listed[0].id, first);
		assert_eq!(listed[0].args, vec![1, 2]);
		assert_eq!(listed[1].id, second);
		assert_eq!(
			other
				.get_scheduled_event(&first)
				.await
				.expect("get schedule")
				.expect("schedule exists")
				.run_at,
			BASE_TIME + 5_000
		);
		assert!(
			other
				.cancel_schedule(&first)
				.await
				.expect("cancel schedule")
		);
		assert!(!other.cancel_schedule(&first).await.expect("cancel twice"));
		assert_eq!(other.list_scheduled_events().await.unwrap().len(), 1);
	}

	#[tokio::test]
	async fn actor_state_persistence_does_not_rewrite_schedules() {
		let ctx = context("actor-snapshot-independent");
		let event_id = ctx.at(BASE_TIME + 5_000, "original", &[1]).await.unwrap();
		crate::actor::internal_storage::persist_actor_snapshot(
			ctx.sql(),
			&crate::actor::state::PersistedActor {
				has_initialized: true,
				state: vec![9],
				scheduled_events: vec![crate::actor::state::PersistedScheduleEvent {
					event_id: "legacy-replacement".to_owned(),
					timestamp: 1,
					action: "replacement".to_owned(),
					args: None,
				}],
				..Default::default()
			},
		)
		.await
		.unwrap();

		let event = ctx.get_scheduled_event(&event_id).await.unwrap().unwrap();
		assert_eq!(event.action, "original");
		assert_eq!(ctx.list_scheduled_events().await.unwrap().len(), 1);
	}

	#[tokio::test]
	async fn recurring_crud_defaults_and_upsert_cadence_rules() {
		let ctx = context("actor-recurring-crud");
		ctx.cron_every("refresh", 5_000, "refresh-v1", &[1], None)
			.await
			.expect("create recurring schedule");

		let initial = ctx.cron_get("refresh").await.unwrap().unwrap();
		assert_eq!(initial.kind, ScheduleKind::Every);
		assert_eq!(initial.next_run_at, BASE_TIME + 5_000);
		assert_eq!(initial.max_history, DEFAULT_MAX_HISTORY);

		ctx.set_schedule_time_for_tests(BASE_TIME + 1_000);
		ctx.cron_every("refresh", 5_000, "refresh-v2", &[2], Some(7))
			.await
			.expect("update action without resetting cadence");
		let action_update = ctx.cron_get("refresh").await.unwrap().unwrap();
		assert_eq!(action_update.next_run_at, BASE_TIME + 5_000);
		assert_eq!(action_update.action, "refresh-v2");
		assert_eq!(action_update.args, vec![2]);
		assert_eq!(action_update.max_history, 7);

		ctx.cron_every("refresh", 10_000, "refresh-v2", &[2], Some(7))
			.await
			.expect("change cadence");
		assert_eq!(
			ctx.cron_get("refresh").await.unwrap().unwrap().next_run_at,
			BASE_TIME + 11_000
		);
		assert_eq!(ctx.cron_list().await.unwrap().len(), 1);
		assert!(ctx.cron_delete("refresh").await.unwrap());
		assert!(!ctx.cron_delete("refresh").await.unwrap());
		assert!(ctx.cron_get("refresh").await.unwrap().is_none());
	}

	#[tokio::test]
	async fn recurring_validation_is_fail_closed() {
		let ctx = context("actor-recurring-validation");
		let cases = [
			ctx.cron_every("fast", MIN_INTERVAL_MS - 1, "tick", &[], None)
				.await
				.expect_err("interval below minimum"),
			ctx.cron_every("history", MIN_INTERVAL_MS, "tick", &[], Some(1_001))
				.await
				.expect_err("history above maximum"),
			ctx.cron_set("", "* * * * *", None, "tick", &[], None)
				.await
				.expect_err("empty name"),
			ctx.cron_set("bad-cron", "* * *", None, "tick", &[], None)
				.await
				.expect_err("invalid cron"),
			ctx.cron_set(
				"bad-zone",
				"* * * * *",
				Some("Mars/Olympus_Mons"),
				"tick",
				&[],
				None,
			)
			.await
			.expect_err("invalid timezone"),
		];
		assert_eq!(
			error_code(&cases[0]),
			("schedule".into(), "invalid_interval".into())
		);
		assert_eq!(
			error_code(&cases[1]),
			("schedule".into(), "invalid_max_history".into())
		);
		assert_eq!(
			error_code(&cases[2]),
			("schedule".into(), "invalid_name".into())
		);
		assert_eq!(
			error_code(&cases[3]),
			("schedule".into(), "invalid_cron_expression".into())
		);
		assert_eq!(
			error_code(&cases[4]),
			("schedule".into(), "invalid_timezone".into())
		);
		assert!(ctx.cron_list().await.unwrap().is_empty());
	}

	#[test]
	fn cron_timezone_and_dst_semantics_are_deterministic() {
		let daily_9 = parse_cron("0 9 * * *").unwrap();
		assert_eq!(
			next_cron_timestamp_ms(
				&daily_9,
				parse_timezone("America/Los_Angeles").unwrap(),
				utc_ms(2026, 1, 1, 16, 0),
			)
			.unwrap(),
			utc_ms(2026, 1, 1, 17, 0)
		);

		// 02:30 does not exist on the spring-forward day, so that occurrence is skipped.
		let gap = parse_cron("30 2 * * *").unwrap();
		assert_eq!(
			next_cron_timestamp_ms(
				&gap,
				parse_timezone("America/Los_Angeles").unwrap(),
				utc_ms(2026, 3, 8, 0, 0),
			)
			.unwrap(),
			utc_ms(2026, 3, 9, 9, 30)
		);

		// 01:30 occurs twice on fall-back; the first wall-clock occurrence wins.
		let fold = parse_cron("30 1 * * *").unwrap();
		assert_eq!(
			next_cron_timestamp_ms(
				&fold,
				parse_timezone("America/Los_Angeles").unwrap(),
				utc_ms(2026, 11, 1, 0, 0),
			)
			.unwrap(),
			utc_ms(2026, 11, 1, 8, 30)
		);
	}

	#[tokio::test]
	async fn due_recurring_jobs_rearm_without_drift_and_skip_overlap() {
		let ctx = context("actor-overlap");
		ctx.cron_every("tick", 5_000, "tick", &[9], Some(10))
			.await
			.unwrap();

		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let first = ctx.take_due_schedule_dispatches().await.unwrap();
		assert_eq!(first.len(), 1);
		assert_eq!(first[0].action, "tick");
		assert_eq!(first[0].args, vec![9]);
		assert_eq!(first[0].fire.name.as_deref(), Some("tick"));
		assert_eq!(first[0].fire.scheduled_at, BASE_TIME + 5_000);
		assert_eq!(
			ctx.cron_get("tick").await.unwrap().unwrap().next_run_at,
			BASE_TIME + 10_000
		);

		ctx.set_schedule_time_for_tests(BASE_TIME + 16_000);
		assert!(ctx.take_due_schedule_dispatches().await.unwrap().is_empty());
		assert_eq!(
			ctx.cron_get("tick").await.unwrap().unwrap().next_run_at,
			BASE_TIME + 20_000,
			"missed interval ticks coalesce while remaining anchored to the prior deadline"
		);

		ctx.finish_schedule_dispatch(&first[0].event_id, first[0].history_id, None)
			.await;
		let history = ctx.cron_history("tick", None).await.unwrap();
		assert_eq!(history.len(), 2);
		assert_eq!(history[0].result, "skipped");
		assert_eq!(history[1].result, "ok");
	}

	#[tokio::test]
	async fn claimed_due_work_survives_alarm_resync_failure() {
		let ctx = context("actor-alarm-resync-failure");
		let event_id = ctx.at(BASE_TIME, "tick", &[7]).await.unwrap();
		ctx.fail_next_schedule_alarm_sync_for_tests();

		let dispatches = ctx.take_due_schedule_dispatches().await.unwrap();
		assert_eq!(dispatches.len(), 1);
		assert_eq!(dispatches[0].event_id, event_id);
		assert_eq!(dispatches[0].args, vec![7]);
		assert!(ctx.get_scheduled_event(&event_id).await.unwrap().is_none());
	}

	#[tokio::test]
	async fn due_one_shots_are_claimed_across_batch_boundaries() {
		let ctx = context("actor-one-shot-claim-batches");
		for index in 0..129 {
			ctx.at(BASE_TIME, "tick", &[index]).await.unwrap();
		}

		let dispatches = ctx.take_due_schedule_dispatches().await.unwrap();
		assert_eq!(dispatches.len(), 129);
		assert!(ctx.list_scheduled_events().await.unwrap().is_empty());
		assert!(ctx.take_due_schedule_dispatches().await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn different_recurring_names_can_run_concurrently() {
		let ctx = context("actor-concurrent-names");
		ctx.cron_every("first", 5_000, "tick", &[], None)
			.await
			.unwrap();
		ctx.cron_every("second", 5_000, "tick", &[], None)
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatches = ctx.take_due_schedule_dispatches().await.unwrap();
		assert_eq!(dispatches.len(), 2);
		assert_ne!(dispatches[0].event_id, dispatches[1].event_id);
		for dispatch in dispatches {
			ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
				.await;
		}
	}

	#[tokio::test]
	async fn missing_recurring_action_records_error_and_deletes_job() {
		let ctx = context("actor-missing-action");
		ctx.cron_every("missing", 5_000, "removedAction", &[], Some(10))
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);

		let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(events_tx));
		let reply = tokio::spawn(async move {
			let crate::actor::messages::ActorEvent::Action { reply, .. } = events_rx
				.recv()
				.await
				.expect("scheduled action should dispatch")
			else {
				panic!("expected action event")
			};
			reply.send(Err(anyhow::Error::new(rivet_error::RivetError {
				kind: RivetErrorKind::Static(&ACTION_NOT_FOUND_SCHEMA),
				meta: None,
				message: None,
				actor: None,
			})));
		});

		ctx.drain_overdue_scheduled_events().await.unwrap();
		reply.await.unwrap();
		for _ in 0..20 {
			if ctx.cron_get("missing").await.unwrap().is_none() {
				break;
			}
			yield_now().await;
		}
		assert!(ctx.cron_get("missing").await.unwrap().is_none());
		let history = ctx.cron_history("missing", None).await.unwrap();
		assert_eq!(history.len(), 1);
		assert_eq!(history[0].result, "error");
		assert_eq!(history[0].error.as_ref().unwrap().code, "action_not_found");
	}

	#[tokio::test]
	async fn stale_missing_action_does_not_delete_replacement() {
		let ctx = context("actor-missing-action-replacement");
		ctx.cron_every("job", 5_000, "removedAction", &[], Some(10))
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);

		ctx.cron_every("job", 5_000, "replacementAction", &[2], Some(10))
			.await
			.unwrap();
		assert!(
			!ctx.cron_delete_if_action("job", &dispatch.action)
				.await
				.unwrap()
		);
		let replacement = ctx.cron_get("job").await.unwrap().unwrap();
		assert_eq!(replacement.action, "replacementAction");
		assert_eq!(replacement.args, vec![2]);
	}

	#[tokio::test]
	async fn deleting_recurring_schedule_removes_history_before_name_reuse() {
		let ctx = context("actor-delete-history");
		ctx.cron_every("job", 5_000, "originalAction", &[], Some(10))
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
			.await;
		assert_eq!(ctx.cron_history("job", None).await.unwrap().len(), 1);

		assert!(ctx.cron_delete("job").await.unwrap());
		assert!(ctx.cron_history("job", None).await.unwrap().is_empty());

		ctx.cron_every("job", 5_000, "replacementAction", &[], Some(10))
			.await
			.unwrap();
		assert!(ctx.cron_history("job", None).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn pending_schedule_cap_allows_replacement_and_freed_capacity() {
		let harness = ActorContextHarness::new();
		let ctx = harness.context_with_config(
			"actor-schedule-cap",
			"actor",
			Vec::new(),
			"local",
			ActorConfig {
				max_schedules: 1,
				..Default::default()
			},
		);
		ctx.set_schedule_time_for_tests(BASE_TIME);

		ctx.cron_every("job", 5_000, "first", &[], None)
			.await
			.unwrap();
		ctx.cron_every("job", 5_000, "replacement", &[], None)
			.await
			.expect("upsert at the cap should be allowed");
		let error = ctx
			.at(BASE_TIME + 10_000, "one-shot", &[])
			.await
			.expect_err("new one-shot should exceed cap");
		assert_eq!(
			error_code(&error),
			("schedule".into(), "max_schedules_exceeded".into())
		);

		assert!(ctx.cron_delete("job").await.unwrap());
		let one_shot = ctx
			.at(BASE_TIME + 10_000, "one-shot", &[])
			.await
			.expect("deletion should free capacity");
		assert!(ctx.cancel_schedule(&one_shot).await.unwrap());
		ctx.cron_every("job-2", 5_000, "tick", &[], None)
			.await
			.expect("cancellation should free capacity");
	}

	#[tokio::test]
	async fn pending_schedule_cap_is_atomic_for_concurrent_creates() {
		let harness = ActorContextHarness::new();
		let ctx = harness.context_with_config(
			"actor-schedule-cap-concurrent",
			"actor",
			Vec::new(),
			"local",
			ActorConfig {
				max_schedules: 1,
				..Default::default()
			},
		);
		ctx.set_schedule_time_for_tests(BASE_TIME);

		let (first, second) = tokio::join!(
			ctx.at(BASE_TIME + 5_000, "first", &[]),
			ctx.at(BASE_TIME + 5_000, "second", &[]),
		);
		assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
		assert_eq!(ctx.list_scheduled_events().await.unwrap().len(), 1);
	}

	#[tokio::test]
	async fn lowering_schedule_cap_preserves_existing_rows() {
		let harness = ActorContextHarness::new();
		let initial = harness.context_with_config(
			"actor-schedule-cap-lowered",
			"actor",
			Vec::new(),
			"local",
			ActorConfig {
				max_schedules: 2,
				..Default::default()
			},
		);
		initial.set_schedule_time_for_tests(BASE_TIME);
		initial.at(BASE_TIME + 5_000, "first", &[]).await.unwrap();
		initial.at(BASE_TIME + 6_000, "second", &[]).await.unwrap();

		let lowered = harness.context_with_config(
			"actor-schedule-cap-lowered",
			"actor",
			Vec::new(),
			"local",
			ActorConfig {
				max_schedules: 1,
				..Default::default()
			},
		);
		lowered.set_schedule_time_for_tests(BASE_TIME);
		assert_eq!(lowered.list_scheduled_events().await.unwrap().len(), 2);
		assert!(lowered.at(BASE_TIME + 7_000, "third", &[]).await.is_err());
	}

	#[tokio::test]
	async fn history_is_bounded_can_be_disabled_and_sanitizes_errors() {
		let ctx = context("actor-history");
		ctx.cron_every("tick", 5_000, "tick", &[], Some(2))
			.await
			.unwrap();

		for step in 1..=3 {
			ctx.set_schedule_time_for_tests(BASE_TIME + step * 5_000);
			let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
			let error = (step == 3).then(|| anyhow!("private implementation detail"));
			ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, error.as_ref())
				.await;
		}

		let history = ctx.cron_history("tick", Some(10)).await.unwrap();
		assert_eq!(history.len(), 2);
		assert_eq!(history[0].result, "error");
		let error = history[0].error.as_ref().expect("error metadata");
		assert_eq!(error.code, "internal_error");
		assert!(!error.message.contains("private implementation detail"));

		ctx.cron_every("tick", 5_000, "tick", &[], Some(0))
			.await
			.unwrap();
		assert!(ctx.cron_history("tick", None).await.unwrap().is_empty());
		ctx.set_schedule_time_for_tests(BASE_TIME + 20_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		assert!(dispatch.history_id.is_none());
		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
			.await;
		assert!(ctx.cron_history("tick", None).await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn history_persists_public_error_fields_and_metadata_relationally() {
		let ctx = context("actor-history-metadata");
		ctx.cron_every("tick", 5_000, "tick", &[], Some(10))
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		let error = anyhow::Error::new(rivet_error::RivetError {
			kind: RivetErrorKind::Static(&PUBLIC_SCHEDULE_ERROR_SCHEMA),
			meta: Some(
				serde_json::value::RawValue::from_string(
					r#"{"attempt":2,"retryable":false}"#.to_owned(),
				)
				.unwrap(),
			),
			message: Some("Public failure".to_owned()),
			actor: None,
		});
		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, Some(&error))
			.await;

		let history = ctx.cron_history("tick", None).await.unwrap();
		let stored = history[0].error.as_ref().unwrap();
		assert_eq!(stored.group, "schedule");
		assert_eq!(stored.code, "test_failure");
		assert_eq!(stored.message, "Public failure");
		assert_eq!(
			stored.metadata,
			Some(serde_json::json!({ "attempt": 2, "retryable": false }))
		);

		let row = ctx
			.sql()
			.query(
				"SELECT error_group, error_code, error_message, error_metadata FROM _rivet_schedule_history",
				None,
			)
			.await
			.unwrap();
		assert!(matches!(
			row.rows[0][0],
			crate::sqlite::ColumnValue::Text(_)
		));
		assert!(matches!(
			row.rows[0][1],
			crate::sqlite::ColumnValue::Text(_)
		));
		assert!(matches!(
			row.rows[0][2],
			crate::sqlite::ColumnValue::Text(_)
		));
		assert!(matches!(
			row.rows[0][3],
			crate::sqlite::ColumnValue::Blob(_)
		));
	}

	#[tokio::test]
	async fn actor_history_is_globally_bounded() {
		let ctx = context("actor-global-history-bound");
		ctx.sql()
			.execute(
				"WITH RECURSIVE n(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value < 10001) INSERT INTO _rivet_schedule_history (schedule_id, action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata) SELECT 'cron:seed', 'seed', value, value, value, 1, NULL, NULL, NULL, NULL FROM n",
				None,
			)
			.await
			.unwrap();
		ctx.cron_every("tick", 5_000, "tick", &[], Some(1))
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
			.await;

		let count = ctx
			.sql()
			.query("SELECT COUNT(*) FROM _rivet_schedule_history", None)
			.await
			.unwrap();
		assert_eq!(
			count.rows,
			vec![vec![crate::sqlite::ColumnValue::Integer(9_900)]]
		);
		let range = ctx
			.sql()
			.query(
				"SELECT MIN(fired_at), MAX(fired_at) FROM _rivet_schedule_history",
				None,
			)
			.await
			.unwrap();
		assert_eq!(
			range.rows,
			vec![vec![
				crate::sqlite::ColumnValue::Integer(103),
				crate::sqlite::ColumnValue::Integer(BASE_TIME + 5_000),
			]]
		);
	}

	#[tokio::test]
	async fn global_history_pruning_is_periodic() {
		let ctx = context("actor-periodic-global-history-prune");
		assert!(ctx.should_prune_global_history());
		ctx.record_schedule_history_inserted();
		for _ in 1..GLOBAL_HISTORY_PRUNE_INTERVAL {
			assert!(!ctx.should_prune_global_history());
			ctx.record_schedule_history_inserted();
		}
		assert!(ctx.should_prune_global_history());
	}

	#[tokio::test]
	async fn failed_history_insert_keeps_global_pruning_due() {
		let ctx = context("actor-failed-global-history-prune");
		ctx.sql()
			.execute(
				"WITH RECURSIVE n(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value < 9999) INSERT INTO _rivet_schedule_history (schedule_id, action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata) SELECT 'cron:seed', 'seed', value, value, value, 1, NULL, NULL, NULL, NULL FROM n",
				None,
			)
			.await
			.unwrap();
		ctx.cron_every("tick", 5_000, "tick", &[], Some(1))
			.await
			.unwrap();
		ctx.sql()
			.execute(
				"CREATE TRIGGER fail_schedule_history_insert BEFORE INSERT ON _rivet_schedule_history BEGIN SELECT RAISE(FAIL, 'injected history failure'); END;",
				None,
			)
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		assert!(ctx.take_due_schedule_dispatches().await.is_err());
		assert!(ctx.should_prune_global_history());

		ctx.sql()
			.execute("DROP TRIGGER fail_schedule_history_insert;", None)
			.await
			.unwrap();
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
			.await;

		let count = ctx
			.sql()
			.query("SELECT COUNT(*) FROM _rivet_schedule_history", None)
			.await
			.unwrap();
		assert_eq!(
			count.rows,
			vec![vec![crate::sqlite::ColumnValue::Integer(9_900)]]
		);
	}

	#[tokio::test]
	async fn running_history_is_recovered_as_interrupted() {
		let ctx = context("actor-interrupted");
		ctx.cron_every("tick", 5_000, "tick", &[], Some(10))
			.await
			.unwrap();
		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		assert_eq!(
			ctx.cron_history("tick", None).await.unwrap()[0].result,
			"running"
		);

		ctx.set_schedule_time_for_tests(BASE_TIME + 6_000);
		ctx.recover_interrupted_schedule_history().await.unwrap();
		let history = ctx.cron_history("tick", None).await.unwrap();
		assert_eq!(history[0].result, "error");
		assert_eq!(history[0].finished_at, Some(BASE_TIME + 6_000));
		assert_eq!(history[0].error.as_ref().unwrap().code, "interrupted");
		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
			.await;
	}

	#[tokio::test]
	async fn schedule_lifecycle_increments_inspector_revision() {
		let ctx = context("actor-inspector-schedules");
		let inspector = crate::inspector::Inspector::new();
		ctx.configure_inspector(Some(inspector.clone()));

		ctx.cron_every("tick", 5_000, "tick", &[], Some(10))
			.await
			.unwrap();
		assert_eq!(inspector.snapshot().schedule_revision, 1);

		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let dispatch = ctx.take_due_schedule_dispatches().await.unwrap().remove(0);
		assert_eq!(inspector.snapshot().schedule_revision, 2);

		ctx.finish_schedule_dispatch(&dispatch.event_id, dispatch.history_id, None)
			.await;
		assert_eq!(inspector.snapshot().schedule_revision, 3);

		assert!(ctx.cron_delete("tick").await.unwrap());
		assert_eq!(inspector.snapshot().schedule_revision, 4);
	}

	#[tokio::test(start_paused = true)]
	async fn local_alarm_uses_tokio_virtual_time() {
		let ctx = context("actor-local-timer");
		let fired = Arc::new(AtomicUsize::new(0));
		ctx.set_local_alarm_callback(Some(Arc::new({
			let fired = fired.clone();
			move || {
				let fired = fired.clone();
				Box::pin(async move {
					fired.fetch_add(1, Ordering::SeqCst);
				})
			}
		})));

		ctx.after(Duration::from_secs(5), "tick", &[])
			.await
			.unwrap();
		yield_now().await;
		advance_schedule_time(&ctx, BASE_TIME + 4_999, Duration::from_millis(4_999)).await;
		assert_eq!(fired.load(Ordering::SeqCst), 0);
		advance_schedule_time(&ctx, BASE_TIME + 5_000, Duration::from_millis(1)).await;
		assert_eq!(fired.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn run_wake_and_schedule_share_the_earliest_alarm_without_overwriting() {
		let ctx = context("actor-run-wake-multiplex");
		ctx.set_run_wake_at(Some(BASE_TIME + 10_000)).await.unwrap();
		ctx.wait_for_pending_alarm_writes().await;
		assert_eq!(
			crate::actor::internal_storage::load_last_pushed_alarm(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 10_000),
		);

		ctx.at(BASE_TIME + 5_000, "tick", &[]).await.unwrap();
		ctx.wait_for_pending_alarm_writes().await;
		assert_eq!(
			crate::actor::internal_storage::load_last_pushed_alarm(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 5_000),
		);

		ctx.set_schedule_time_for_tests(BASE_TIME + 5_000);
		let schedules = ctx.take_due_schedule_dispatches().await.unwrap();
		assert_eq!(schedules.len(), 1);
		assert_eq!(ctx.run_wake_at(), Some(BASE_TIME + 10_000));
		ctx.wait_for_pending_alarm_writes().await;
		assert_eq!(
			crate::actor::internal_storage::load_last_pushed_alarm(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 10_000),
		);

		ctx.set_schedule_time_for_tests(BASE_TIME + 10_000);
		let (wake_at, _) = ctx
			.consume_due_run_wake()
			.await
			.unwrap()
			.expect("run wake should be due");
		assert_eq!(wake_at, BASE_TIME + 10_000);
		assert_eq!(ctx.run_wake_at(), None);
	}

	#[tokio::test]
	async fn stale_run_wake_restore_does_not_resurrect_a_cleared_deadline() {
		let ctx = context("actor-run-wake-stale-clear");
		ctx.set_run_wake_at(Some(BASE_TIME)).await.unwrap();
		let (wake_at, revision) = ctx
			.consume_due_run_wake()
			.await
			.unwrap()
			.expect("run wake should be due");

		ctx.set_run_wake_at(None).await.unwrap();
		assert!(
			!ctx.restore_run_wake_at_if_unchanged(wake_at, revision)
				.await
				.unwrap()
		);
		assert_eq!(ctx.run_wake_at(), None);
	}

	#[tokio::test]
	async fn stale_run_wake_restore_does_not_overwrite_a_newer_deadline() {
		let ctx = context("actor-run-wake-stale-replace");
		ctx.set_run_wake_at(Some(BASE_TIME)).await.unwrap();
		let (wake_at, revision) = ctx
			.consume_due_run_wake()
			.await
			.unwrap()
			.expect("run wake should be due");

		let newer = BASE_TIME + 10_000;
		ctx.set_run_wake_at(Some(newer)).await.unwrap();
		assert!(
			!ctx.restore_run_wake_at_if_unchanged(wake_at, revision)
				.await
				.unwrap()
		);
		assert_eq!(ctx.run_wake_at(), Some(newer));
	}

	#[tokio::test]
	async fn clearing_run_wake_preserves_a_later_schedule_alarm() {
		let ctx = context("actor-run-wake-clear");
		ctx.at(BASE_TIME + 10_000, "tick", &[]).await.unwrap();
		ctx.set_run_wake_at(Some(BASE_TIME + 5_000)).await.unwrap();
		ctx.wait_for_pending_alarm_writes().await;
		assert_eq!(
			crate::actor::internal_storage::load_last_pushed_alarm(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 5_000),
		);

		ctx.set_run_wake_at(None).await.unwrap();
		ctx.wait_for_pending_alarm_writes().await;
		assert_eq!(
			crate::actor::internal_storage::load_last_pushed_alarm(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 10_000),
		);
	}

	#[tokio::test]
	async fn run_wake_is_persisted_independently_from_transport_alarm() {
		let harness = ActorContextHarness::new();
		let ctx = harness.context("actor-run-wake-persist", "actor", Vec::new(), "local");
		ctx.set_schedule_time_for_tests(BASE_TIME);
		ctx.set_run_wake_at(Some(BASE_TIME + 7_000)).await.unwrap();
		ctx.wait_for_pending_alarm_writes().await;
		crate::actor::internal_storage::persist_last_pushed_alarm(
			ctx.sql(),
			Some(BASE_TIME + 9_000),
		)
		.await
		.unwrap();

		assert_eq!(
			crate::actor::internal_storage::load_run_wake_at(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 7_000),
		);
		assert_eq!(
			crate::actor::internal_storage::load_last_pushed_alarm(ctx.sql())
				.await
				.unwrap(),
			Some(BASE_TIME + 9_000),
		);
	}
}
