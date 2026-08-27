use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::*;
use crate::action;
use crate::event::Event;
use rivetkit_core::actor::schedule::ScheduleKind;
use rivetkit_core::testing::{ActorContextHarness, actor_context};
use rivetkit_core::{ActorConfig, ActorWorkKind};

struct EmptyActor;

impl Actor for EmptyActor {
	type State = ();
	type Input = ();
	type Actions = ();
	type Events = ();
	type Queue = ();
	type ConnParams = TestConnParams;
	type ConnState = TestConnState;
	type Action = action::Raw;
}

struct StatefulActor;

impl Actor for StatefulActor {
	type State = TestState;
	type Input = ();
	type Actions = ();
	type Events = ();
	type Queue = ();
	type ConnParams = TestConnParams;
	type ConnState = TestConnState;
	type Action = action::Raw;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestState {
	count: u32,
	label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TestConnParams {
	label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TestConnState {
	value: i64,
}

#[test]
fn typed_ctx_broadcast_accepts_cbor_payloads() {
	let ctx = Ctx::<EmptyActor>::new(actor_context("actor-id", "test", Vec::new(), "local"));

	assert!(ctx.broadcast("x", &42u32).is_ok());
}

#[test]
fn typed_ctx_emit_accepts_named_events() {
	#[derive(Serialize, Deserialize)]
	struct CountChanged {
		count: u32,
	}

	impl Event for CountChanged {
		const NAME: &'static str = "countChanged";
	}

	let ctx = Ctx::<EmptyActor>::new(actor_context("actor-id", "test", Vec::new(), "local"));

	assert!(ctx.emit(CountChanged { count: 3 }).is_ok());
}

#[test]
fn state_cell_reads_writes_and_tracks_dirty() {
	let inner = actor_context("actor-id", "test", Vec::new(), "local");
	let save_requests = Arc::new(AtomicUsize::new(0));
	inner.on_request_save(Box::new({
		let save_requests = Arc::clone(&save_requests);
		move |_| {
			save_requests.fetch_add(1, Ordering::SeqCst);
		}
	}));
	let ctx = Ctx::<StatefulActor>::with_state(
		inner,
		TestState {
			count: 1,
			label: "initial".into(),
		},
	);

	assert_eq!(ctx.state().count, 1);
	assert!(!ctx.state_dirty());

	{
		let mut state = ctx.state_mut();
		state.count += 1;
		state.label = "mutated".into();
	}

	assert!(ctx.state_dirty());
	assert_eq!(save_requests.load(Ordering::SeqCst), 1);
	assert_eq!(
		*ctx.state(),
		TestState {
			count: 2,
			label: "mutated".into(),
		}
	);

	ctx.clear_state_dirty();
	assert!(!ctx.state_dirty());

	ctx.set_state(TestState {
		count: 7,
		label: "set".into(),
	});

	assert!(ctx.state_dirty());
	assert_eq!(save_requests.load(Ordering::SeqCst), 2);
	assert_eq!(ctx.state().count, 7);
}

#[test]
fn state_snapshot_round_trips_and_seeds_cell() {
	let ctx = Ctx::<StatefulActor>::with_state(
		actor_context("actor-id", "test", Vec::new(), "local"),
		TestState {
			count: 5,
			label: "snapshot".into(),
		},
	);

	let StateDelta::ActorState(bytes) = ctx.encode_state_delta().expect("encode state snapshot")
	else {
		panic!("expected actor state delta");
	};

	assert_eq!(
		Ctx::<StatefulActor>::decode_state_snapshot(&bytes).expect("decode state snapshot"),
		TestState {
			count: 5,
			label: "snapshot".into(),
		}
	);

	ctx.set_state(TestState {
		count: 99,
		label: "dirty".into(),
	});
	assert!(ctx.state_dirty());

	ctx.set_state_from_snapshot(&bytes)
		.expect("seed state from snapshot");

	assert!(!ctx.state_dirty());
	assert_eq!(
		*ctx.state(),
		TestState {
			count: 5,
			label: "snapshot".into(),
		}
	);
}

#[test]
fn conn_ctx_round_trips_typed_params_and_state() {
	let conn = ConnHandle::new(
		"conn-id",
		cbor(&TestConnParams {
			label: "hello".into(),
		}),
		cbor(&TestConnState { value: 5 }),
		true,
	);
	let conn_ctx = ConnCtx::<EmptyActor>::new(conn);

	assert_eq!(conn_ctx.id(), "conn-id");
	assert_eq!(
		conn_ctx.params().expect("decode params"),
		TestConnParams {
			label: "hello".into(),
		}
	);
	assert_eq!(
		conn_ctx.state().expect("decode state"),
		TestConnState { value: 5 }
	);
	assert!(conn_ctx.is_hibernatable());

	conn_ctx
		.set_state(&TestConnState { value: 8 })
		.expect("encode state");
	assert_eq!(
		conn_ctx.state().expect("decode updated state"),
		TestConnState { value: 8 }
	);
}

#[tokio::test]
async fn scheduling_surface_matches_typescript_api() {
	const BASE_TIME: i64 = 1_700_000_000_000;
	let inner = ActorContextHarness::new().context_with_config(
		"typed-scheduling-api",
		"test",
		Vec::new(),
		"local",
		ActorConfig {
			max_schedules: 3,
			..Default::default()
		},
	);
	inner.set_schedule_time_for_tests(BASE_TIME);
	let ctx = Ctx::<EmptyActor>::new(inner);
	let args = action::encode_positional(&("payload", 7u32)).expect("encode action args");

	let after_id = ctx
		.schedule()
		.after(Duration::from_secs(5), "afterAction", &args)
		.await
		.expect("schedule after");
	let at_id = ctx
		.schedule()
		.at(BASE_TIME + 10_000, "atAction", &args)
		.await
		.expect("schedule at");

	let after = ctx
		.schedule()
		.get(&after_id)
		.await
		.expect("get one-shot")
		.expect("one-shot exists");
	assert_eq!(after.action, "afterAction");
	assert_eq!(after.args, args);
	assert_eq!(after.run_at, BASE_TIME + 5_000);
	assert_eq!(
		ctx.schedule().list().await.expect("list one-shots").len(),
		2
	);
	assert!(
		ctx.schedule()
			.cancel(&at_id)
			.await
			.expect("cancel one-shot")
	);
	assert!(
		ctx.schedule()
			.get(&at_id)
			.await
			.expect("get cancelled one-shot")
			.is_none()
	);

	ctx.cron()
		.set(CronSetOptions {
			name: "daily",
			expression: "0 9 * * *",
			timezone: Some("America/Los_Angeles"),
			action: "dailyAction",
			args: &args,
			max_history: Some(25),
		})
		.await
		.expect("set cron job");
	ctx.cron()
		.every(CronEveryOptions {
			name: "frequent",
			interval: Duration::from_secs(5),
			action: "frequentAction",
			args: &args,
			max_history: None,
		})
		.await
		.expect("set interval job");

	let daily = ctx
		.cron()
		.get("daily")
		.await
		.expect("get cron job")
		.expect("cron job exists");
	assert_eq!(daily.kind, ScheduleKind::Cron);
	assert_eq!(daily.expression.as_deref(), Some("0 9 * * *"));
	assert_eq!(daily.timezone.as_deref(), Some("America/Los_Angeles"));
	assert_eq!(daily.max_history, 25);

	let frequent = ctx
		.cron()
		.get("frequent")
		.await
		.expect("get interval job")
		.expect("interval job exists");
	assert_eq!(frequent.kind, ScheduleKind::Every);
	assert_eq!(frequent.interval, Some(Duration::from_secs(5)));
	assert_eq!(frequent.max_history, 100);
	assert_eq!(
		ctx.cron().list().await.expect("list recurring jobs").len(),
		2
	);
	assert!(
		ctx.schedule()
			.after(Duration::from_secs(20), "overLimit", &[])
			.await
			.is_err()
	);
	assert!(
		ctx.cron()
			.history("daily", Some(10))
			.await
			.expect("read cron history")
			.is_empty()
	);
	assert!(ctx.cron().delete("daily").await.expect("delete cron job"));
	assert!(
		ctx.cron()
			.delete("frequent")
			.await
			.expect("delete interval job")
	);
}

#[test]
fn sleep_lifecycle_methods_accessible_via_inner() {
	let inner_ctx = actor_context("actor-id", "test", Vec::new(), "local");
	let ctx = Ctx::<EmptyActor>::new(inner_ctx);

	// All these methods should be accessible on ctx.inner()
	let inner = ctx.inner();

	// Methods we're testing:
	// 1. keep_awake - accessible as public async fn on ActorContext
	let _ = inner.keep_awake(async {}); // type check that method exists

	// 2. keep_awake_region - accessible as public fn on ActorContext, returns KeepAwakeRegion
	let _ = inner.keep_awake_region(); // type check that method exists

	// 3. internal_keep_awake - accessible as public async fn on ActorContext
	let _ = inner.internal_keep_awake(async {}); // type check that method exists

	// 4. register_task - accessible as public fn on ActorContext
	let _ = inner.register_task(async {}); // type check that method exists

	// 5. track_work - accessible as public async fn on ActorContext
	let _ = inner.track_work(ActorWorkKind::KeepAwake, async {}); // type check that method exists

	// 6. spawn_work - accessible as public fn on ActorContext
	let _ = inner.spawn_work(ActorWorkKind::KeepAwake, async {}); // type check that method exists

	// 7. keep_awake_count - accessible as public fn on ActorContext
	let _ = inner.keep_awake_count(); // type check that method exists

	// 8. internal_keep_awake_count - accessible as public fn on ActorContext
	let _ = inner.internal_keep_awake_count(); // type check that method exists

	// 9. is_destroy_requested - accessible as public fn on ActorContext
	let _ = inner.is_destroy_requested(); // type check that method exists

	// 10. wait_for_destroy_completion_public - accessible as public async fn on ActorContext
	let _ = inner.wait_for_destroy_completion_public(); // type check that method exists

	// 11. actor_abort_signal - accessible as public fn on ActorContext (doc hidden)
	let _ = inner.actor_abort_signal(); // type check that method exists

	// 12. shutdown_deadline_token - accessible as public fn on ActorContext (doc hidden)
	let _ = inner.shutdown_deadline_token(); // type check that method exists

	// 13. cancel_actor_abort_signal - accessible as public fn on ActorContext (doc hidden)
	let _ = inner.cancel_actor_abort_signal(); // type check that method exists
}

#[test]
fn sleep_types_accessible_from_rivetkit_core() {
	// ActorWorkKind type is accessible (exported from rivetkit-core)
	let _ = ActorWorkKind::KeepAwake;
	let _ = ActorWorkKind::InternalKeepAwake;
	let _ = ActorWorkKind::RegisteredTask;
	let _ = ActorWorkKind::WaitUntil;

	// These can be used as arguments to methods like spawn_work, track_work
	let inner_ctx = actor_context("test", "test", vec![], "local");
	let _fut_kind = ActorWorkKind::KeepAwake;
	let _ = _fut_kind;
	let _ = inner_ctx;
}

fn cbor<T: Serialize>(value: &T) -> Vec<u8> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded).expect("encode test value as cbor");
	encoded
}
