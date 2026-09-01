use std::io::Cursor;

use anyhow::Result;
use rivetkit_core::{ActorEvent, ActorStart, SerializeStateReason, ShutdownKind, StateDelta};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};

use rivetkit::{Actor, RuntimeEvent, Start, start::wrap_start};

#[derive(Debug)]
struct CounterActor;

impl Actor for CounterActor {
	type State = ();
	type Input = ();
	type Actions = ();
	type Events = ();
	type Queue = ();
	type ConnParams = ();
	type ConnState = ();
	type Action = CounterAction;
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CounterAction {
	Increment,
	Get,
}

async fn run(start: Start<CounterActor>) -> Result<()> {
	let mut count = start.snapshot.decode_or_default::<u32>()?;
	let mut events = start.events;

	while let Some(event) = events.recv().await {
		match event {
			RuntimeEvent::Action(action) => match action.decode()? {
				CounterAction::Increment => {
					count += 1;
					action.ok(&());
				}
				CounterAction::Get => action.ok(&count),
			},
			RuntimeEvent::SerializeState(save) => save.save(&count),
			RuntimeEvent::Sleep(sleep) => sleep.ok(),
			other => panic!("unexpected canned event: {other:?}"),
		}
	}

	Ok(())
}

#[tokio::test]
async fn canned_actor_start_drives_typed_counter_actor() {
	let (event_tx, event_rx) = mpsc::unbounded_channel();
	let start = wrap_start::<CounterActor>(ActorStart {
		ctx: rivetkit_core::testing::actor_context("actor-id", "counter", Vec::new(), "local"),
		input: None,
		is_new: true,
		snapshot: None,
		hibernated: Vec::new(),
		events: event_rx.into(),
		startup_ready: None,
	})
	.expect("wrap canned actor start");

	let run_task = tokio::spawn(run(start));

	let increment_reply = send_action(&event_tx, "increment").await;
	let unit: () = decode_cbor(&increment_reply).expect("decode increment reply");
	assert_eq!(unit, ());

	let get_reply = send_action(&event_tx, "get").await;
	let count: u32 = decode_cbor(&get_reply).expect("decode get reply");
	assert_eq!(count, 1);

	let (serialize_tx, serialize_rx) = oneshot::channel();
	event_tx
		.send(ActorEvent::SerializeState {
			reason: SerializeStateReason::Save,
			reply: serialize_tx.into(),
		})
		.expect("send serialize-state event");
	let deltas = serialize_rx
		.await
		.expect("receive serialize-state reply")
		.expect("serialize-state succeeds");
	assert_eq!(deltas.len(), 1);
	let StateDelta::ActorState(bytes) = &deltas[0] else {
		panic!("expected a single actor-state delta");
	};
	let saved_count: u32 = decode_cbor(bytes).expect("decode serialized actor state");
	assert_eq!(saved_count, 1);

	let (sleep_tx, sleep_rx) = oneshot::channel();
	event_tx
		.send(ActorEvent::RunGracefulCleanup {
			reason: ShutdownKind::Sleep,
			reply: sleep_tx.into(),
		})
		.expect("send sleep event");
	sleep_rx
		.await
		.expect("receive sleep reply")
		.expect("sleep succeeds");

	drop(event_tx);
	run_task
		.await
		.expect("join canned run task")
		.expect("run exits cleanly");
}

async fn send_action(event_tx: &mpsc::UnboundedSender<ActorEvent>, name: &str) -> Vec<u8> {
	let (reply_tx, reply_rx) = oneshot::channel();
	event_tx
		.send(ActorEvent::Action {
			name: name.to_owned(),
			args: Vec::new(),
			conn: None,
			scheduled_fire: None,
			invocation_telemetry: None,
			reply: reply_tx.into(),
		})
		.expect("send action event");

	reply_rx
		.await
		.expect("receive action reply")
		.expect("action succeeds")
}

fn decode_cbor<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
	Ok(ciborium::from_reader(Cursor::new(bytes))?)
}
