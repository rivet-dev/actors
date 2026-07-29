use std::any::{Any, TypeId};
use std::io::Cursor;
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::FutureExt;
use rivetkit_core::actor::ShutdownKind;
use rivetkit_core::error::{ActorLifecycle, ActorRuntime, action_not_found};
use rivetkit_core::{ActorEvent, ActorEvents, ActorStart, QueueSendResult, QueueSendStatus, Reply};
use serde::de::DeserializeOwned;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
	action::ActionSet,
	actor::Actor,
	context::{ConnCtx, Ctx},
	event::RuntimeEvent,
	queue::QueueSet,
};

#[derive(Debug)]
pub struct Start<A: Actor> {
	pub ctx: Ctx<A>,
	pub input: Input<A>,
	pub is_new: bool,
	pub snapshot: Snapshot,
	pub hibernated: Vec<Hibernated<A>>,
	pub events: Events<A>,
	#[doc(hidden)]
	pub startup_ready: Option<tokio::sync::oneshot::Sender<Result<()>>>,
}

#[derive(Debug)]
pub struct Input<A: Actor> {
	bytes: Option<Vec<u8>>,
	_p: PhantomData<fn() -> A>,
}

impl<A: Actor> Input<A> {
	pub fn is_present(&self) -> bool {
		self.bytes.is_some()
	}

	pub fn decode(&self) -> Result<A::Input> {
		// Treat empty input bytes as absent so a zero-length payload defaults
		// like a missing one, mirroring snapshot decoding. The engine encodes
		// an omitted `input` as no bytes, but an empty base64 string decodes to
		// an empty (non-null) buffer.
		match self.present_bytes() {
			Some(bytes) => decode_cbor(bytes, "actor input"),
			None if TypeId::of::<A::Input>() == TypeId::of::<()>() => {
				let unit: Box<dyn Any> = Box::new(());
				Ok(*unit
					.downcast::<A::Input>()
					.expect("unit input type id should downcast"))
			}
			None => Err(ActorRuntime::MissingInput.build()),
		}
	}

	pub fn decode_or<F>(&self, f: F) -> Result<A::Input>
	where
		F: FnOnce() -> A::Input,
	{
		match self.present_bytes() {
			Some(bytes) => decode_cbor(bytes, "actor input"),
			None => Ok(f()),
		}
	}

	/// Input bytes with empty buffers normalized to `None`, so a zero-length
	/// payload is treated the same as an omitted one.
	fn present_bytes(&self) -> Option<&[u8]> {
		self.bytes.as_deref().filter(|bytes| !bytes.is_empty())
	}

	pub fn decode_or_default(&self) -> Result<A::Input>
	where
		A::Input: Default,
	{
		self.decode_or(A::Input::default)
	}

	pub fn raw(&self) -> Option<&[u8]> {
		self.bytes.as_deref()
	}
}

#[derive(Debug)]
pub struct Snapshot {
	is_new: bool,
	bytes: Option<Vec<u8>>,
}

impl Snapshot {
	pub fn is_new(&self) -> bool {
		self.is_new
	}

	pub fn decode<S>(&self) -> Result<Option<S>>
	where
		S: DeserializeOwned,
	{
		let Some(bytes) = self.bytes.as_deref().filter(|bytes| !bytes.is_empty()) else {
			return Ok(None);
		};
		decode_cbor(bytes, "actor snapshot").map(Some)
	}

	pub fn decode_or_default<S>(&self) -> Result<S>
	where
		S: DeserializeOwned + Default,
	{
		Ok(self.decode()?.unwrap_or_default())
	}

	pub fn raw(&self) -> Option<&[u8]> {
		self.bytes.as_deref()
	}
}

#[derive(Debug)]
pub struct Hibernated<A: Actor> {
	pub conn: ConnCtx<A>,
}

#[derive(Debug)]
pub struct Events<A: Actor> {
	ctx: Ctx<A>,
	rx: ActorEvents,
	_p: PhantomData<fn() -> A>,
}

impl<A: Actor> Events<A> {
	pub(crate) async fn recv_raw(&mut self) -> Option<ActorEvent> {
		self.rx.recv().await
	}

	pub async fn recv(&mut self) -> Option<RuntimeEvent<A>> {
		loop {
			let event = self.rx.recv().await?;
			if let Some(event) = self.handle_runtime_event(event).await {
				return Some(wrap_event(event));
			}
		}
	}

	pub fn try_recv(&mut self) -> Option<RuntimeEvent<A>> {
		while let Some(event) = self.rx.try_recv() {
			if let Some(event) = self.handle_runtime_event_sync(event) {
				return Some(wrap_event(event));
			}
		}
		None
	}

	async fn handle_runtime_event(&self, event: ActorEvent) -> Option<ActorEvent> {
		match event {
			ActorEvent::ConnectionOpen { reply, .. } => {
				reply.send(Ok(()));
				None
			}
			ActorEvent::DisconnectConn { conn_id, reply } => {
				reply.send(self.ctx.disconnect_conn(&conn_id).await);
				None
			}
			event => Some(event),
		}
	}

	fn handle_runtime_event_sync(&self, event: ActorEvent) -> Option<ActorEvent> {
		match event {
			ActorEvent::ConnectionOpen { reply, .. } => {
				reply.send(Ok(()));
				None
			}
			ActorEvent::DisconnectConn { conn_id, reply } => {
				let ctx = self.ctx.clone();
				tokio::spawn(async move {
					reply.send(ctx.disconnect_conn(&conn_id).await);
				});
				None
			}
			event => Some(event),
		}
	}
}

pub async fn run_actor<A: Actor>(start: Start<A>) -> Result<()> {
	let Start {
		ctx,
		input,
		is_new,
		snapshot,
		hibernated: _,
		mut events,
		startup_ready,
	} = start;

	let state = match snapshot.decode()? {
		Some(state) => state,
		// Absent input falls back to the input type's default, matching
		// rivetkit-typescript where createState receives undefined input.
		None => A::create_state(&ctx, input.decode_or_default()?).await?,
	};
	ctx.set_state(state);
	ctx.clear_state_dirty();

	let actor = Arc::new(A::create(&ctx).await?);
	if is_new {
		actor.clone().on_create(ctx.clone()).await?;
	}
	actor.clone().on_start(ctx.clone()).await?;
	if let Some(reply) = startup_ready {
		let _ = reply.send(Ok(()));
	}

	let run_cancel = CancellationToken::new();
	let run_task = spawn_run_task(actor.clone(), ctx.clone(), run_cancel.clone());

	while let Some(event) = events.recv_raw().await {
		let should_stop = handle_actor_event(actor.clone(), ctx.clone(), event).await?;
		if should_stop {
			break;
		}
	}

	stop_run_task(&ctx, run_cancel, run_task).await
}

fn spawn_run_task<A: Actor>(
	actor: Arc<A>,
	ctx: Ctx<A>,
	cancel: CancellationToken,
) -> JoinHandle<Result<()>> {
	tokio::spawn(async move {
		let result = tokio::select! {
			_ = cancel.cancelled() => return Ok(()),
			result = AssertUnwindSafe(actor.run(ctx.clone())).catch_unwind() => result,
		};
		let result = result.unwrap_or_else(|_| Err(anyhow::anyhow!("actor run handler panicked")));
		if let Err(error) = &result {
			report_run_error(&ctx, error);
		}
		result
	})
}

/// Reports a `run` failure to the engine as an errored stop. The event loop in
/// `run_actor` does not observe the run task until shutdown, so the report
/// must happen here for the engine to learn about the crash promptly. Errors
/// that surface after the abort signal fires are shutdown-induced (cancelled
/// waits during sleep or destroy) and are not crashes.
fn report_run_error<A: Actor>(ctx: &Ctx<A>, error: &anyhow::Error) {
	if ctx.abort_signal().is_cancelled() {
		tracing::debug!(?error, "actor run task failed during shutdown");
		return;
	}
	if let Err(stop_error) = ctx.stop_with_error(format!("{error:#}")) {
		if ctx.inner().is_destroy_requested() {
			tracing::debug!(
				?stop_error,
				run_error = ?error,
				"actor run failed while a stop was already requested"
			);
		} else {
			tracing::error!(
				?stop_error,
				run_error = ?error,
				"failed to report actor run error as errored stop"
			);
		}
	}
}

async fn stop_run_task<A: Actor>(
	ctx: &Ctx<A>,
	cancel: CancellationToken,
	mut task: JoinHandle<Result<()>>,
) -> Result<()> {
	ctx.inner().cancel_actor_abort_signal();
	match tokio::time::timeout(Duration::from_millis(100), &mut task).await {
		Ok(result) => result.context("join actor run task")?,
		Err(_) => {
			cancel.cancel();
			task.await.context("join actor run task")?
		}
	}
}

async fn handle_actor_event<A: Actor>(
	actor: Arc<A>,
	ctx: Ctx<A>,
	event: ActorEvent,
) -> Result<bool> {
	match event {
		ActorEvent::Action {
			name,
			args,
			conn,
			reply,
			..
		} => {
			let handler_ctx = ctx.with_conn(conn.map(ConnCtx::from));
			match <A::Actions as ActionSet<A>>::dispatch(
				actor,
				handler_ctx.clone(),
				name.as_str(),
				args.as_slice(),
			) {
				Some(future) => {
					spawn_action_reply(handler_ctx, reply, future);
				}
				None => {
					reply.send(Err(action_not_found(name)));
				}
			}
		}
		ActorEvent::HttpRequest { request, reply } => {
			reply.send(actor.on_fetch(ctx, request).await);
		}
		ActorEvent::QueueSend {
			name,
			body,
			conn,
			reply,
			..
		} => {
			let handler_ctx = ctx.with_conn(Some(ConnCtx::from(conn)));
			match <A::Queue as QueueSet<A>>::dispatch(
				actor,
				handler_ctx.clone(),
				name.as_str(),
				body.as_slice(),
			) {
				Some(future) => {
					spawn_queue_reply(handler_ctx, reply, future);
				}
				None => {
					reply.send(Err(ActorRuntime::NotFound {
						resource: "queue handler".to_owned(),
						id: name,
					}
					.build()));
				}
			}
		}
		ActorEvent::WebSocketOpen {
			ws, request, reply, ..
		} => {
			reply.send(
				actor
					.on_websocket(ctx, ws, request.unwrap_or_default())
					.await,
			);
		}
		ActorEvent::ConnectionPreflight {
			conn,
			params,
			reply,
			..
		} => {
			let result = async {
				let params = decode_conn_params::<A>(&params)?;
				actor
					.clone()
					.on_before_connect(ctx.clone(), &params)
					.await?;
				let conn_state = actor.clone().create_conn_state(ctx.clone(), params).await?;
				let conn = ConnCtx::from(conn);
				conn.set_state(&conn_state)?;
				actor.on_connect(ctx, conn).await
			}
			.await;
			reply.send(result);
		}
		ActorEvent::ConnectionOpen { reply, .. } => {
			reply.send(Ok(()));
		}
		ActorEvent::ConnectionClosed { conn } => {
			actor.on_disconnect(ctx, ConnCtx::from(conn)).await;
		}
		ActorEvent::SubscribeRequest {
			conn,
			event_name,
			reply,
		} => {
			reply.send(
				actor
					.on_subscribe(ctx, ConnCtx::from(conn), event_name)
					.await,
			);
		}
		ActorEvent::SerializeState { reply, .. } => {
			let result = async {
				if ctx.state_dirty() {
					actor.on_state_change(ctx.clone()).await?;
				}
				let delta = ctx.encode_state_delta()?;
				ctx.clear_state_dirty();
				Ok(vec![delta])
			}
			.await;
			reply.send(result);
		}
		ActorEvent::RunGracefulCleanup { reason, reply } => {
			let result = match reason {
				ShutdownKind::Sleep => actor.on_sleep(ctx).await,
				ShutdownKind::Destroy => actor.on_destroy(ctx).await,
			};
			reply.send(result);
			return Ok(true);
		}
		ActorEvent::DisconnectConn { conn_id, reply } => {
			reply.send(ctx.disconnect_conn(&conn_id).await);
		}
		ActorEvent::WorkflowHistoryRequested { reply } => {
			reply.send(Err(not_configured("workflow history")));
		}
		ActorEvent::WorkflowReplayRequested { reply, .. } => {
			reply.send(Err(not_configured("workflow replay")));
		}
	}

	Ok(false)
}

fn spawn_action_reply<A: Actor>(
	ctx: Ctx<A>,
	reply: Reply<Vec<u8>>,
	future: crate::action::BoxActionFuture,
) {
	tokio::spawn(async move {
		let abort = ctx.abort_signal();
		tokio::select! {
			_ = abort.cancelled() => {
				reply.send(Err(ActorLifecycle::Stopping.build()));
			}
			result = future => {
				reply.send(result);
			}
		}
	});
}

fn spawn_queue_reply<A: Actor>(
	ctx: Ctx<A>,
	reply: Reply<QueueSendResult>,
	future: crate::queue::BoxQueueFuture,
) {
	tokio::spawn(async move {
		let abort = ctx.abort_signal();
		let result = tokio::select! {
			_ = abort.cancelled() => Err(ActorLifecycle::Stopping.build()),
			result = future => result.map(|response| QueueSendResult {
				status: QueueSendStatus::Completed,
				response,
			}),
		};
		reply.send(result);
	});
}

fn not_configured(component: impl Into<String>) -> anyhow::Error {
	ActorRuntime::NotConfigured {
		component: component.into(),
	}
	.build()
}

#[doc(hidden)]
pub fn wrap_start<A: Actor>(core_start: ActorStart) -> Result<Start<A>> {
	let ActorStart {
		ctx,
		input,
		is_new,
		snapshot,
		hibernated,
		events,
		startup_ready,
	} = core_start;

	let hibernated = hibernated
		.into_iter()
		.map(|(conn, bytes)| Hibernated {
			conn: ConnCtx::from({
				conn.set_state(bytes);
				conn
			}),
		})
		.collect();

	let ctx = Ctx::new(ctx);

	Ok(Start {
		ctx: ctx.clone(),
		input: Input {
			bytes: input,
			_p: PhantomData,
		},
		is_new,
		snapshot: Snapshot {
			is_new,
			bytes: snapshot,
		},
		hibernated,
		events: Events {
			ctx,
			rx: events,
			_p: PhantomData,
		},
		startup_ready,
	})
}

fn wrap_event<A: Actor>(event: ActorEvent) -> RuntimeEvent<A> {
	RuntimeEvent::from_core(event)
}

fn decode_cbor<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
	ciborium::from_reader(Cursor::new(bytes)).with_context(|| format!("decode {label} from cbor"))
}

fn decode_conn_params<A: Actor>(bytes: &[u8]) -> Result<A::ConnParams> {
	if bytes.is_empty() || bytes == [0xf6] {
		return Ok(A::ConnParams::default());
	}
	decode_cbor(bytes, "connection params")
}

#[cfg(test)]
mod tests {
	use std::future::Future;
	use std::pin::Pin;
	use std::sync::OnceLock;

	use async_trait::async_trait;
	use rivetkit_core::{ConnHandle, QueueNextOpts, StateDelta, WebSocket};
	use serde::{Deserialize, Serialize};
	use tokio::sync::mpsc::unbounded_channel;
	use tokio::sync::{Barrier, oneshot};

	use super::*;
	use crate::action::{self, Action, Handles, encode_positional};
	use crate::queue::{HandlesQueue, QueueMessage};

	type BoxTestFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;
	static ACTION_BARRIER: OnceLock<Arc<Barrier>> = OnceLock::new();
	static QUEUE_PULL_DRAINED: OnceLock<parking_lot::Mutex<Option<oneshot::Sender<Vec<u32>>>>> =
		OnceLock::new();

	struct EmptyActor;

	impl Actor for EmptyActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;
	}

	struct UnitActor;

	impl Actor for UnitActor {
		type State = ();
		type Input = UnitInput;
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;
	}

	struct LifecycleActor;

	#[async_trait]
	impl Actor for LifecycleActor {
		type State = LifecycleState;
		type Input = LifecycleInput;
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ConnParams;
		type ConnState = ConnState;
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, input: Self::Input) -> Result<Self::State> {
			Ok(LifecycleState {
				count: input.count,
				log: vec!["create_state".into()],
			})
		}

		async fn create(ctx: &Ctx<Self>) -> Result<Self> {
			ctx.state_mut().log.push("create".into());
			Ok(Self)
		}

		async fn run(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.abort_signal().cancelled().await;
			ctx.state_mut().log.push("run_aborted".into());
			Ok(())
		}

		async fn on_create(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.state_mut().log.push("on_create".into());
			Ok(())
		}

		async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.state_mut().log.push("on_start".into());
			Ok(())
		}

		async fn on_state_change(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.state_mut().log.push("on_state_change".into());
			Ok(())
		}

		async fn on_sleep(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.state_mut().log.push("on_sleep".into());
			Ok(())
		}

		async fn on_destroy(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.state_mut().log.push("on_destroy".into());
			Ok(())
		}

		async fn on_before_connect(
			self: Arc<Self>,
			ctx: Ctx<Self>,
			params: &Self::ConnParams,
		) -> Result<()> {
			if !params.allow {
				anyhow::bail!("connection rejected");
			}
			ctx.state_mut()
				.log
				.push(format!("on_before_connect:{}", params.value));
			Ok(())
		}

		async fn create_conn_state(
			self: Arc<Self>,
			_ctx: Ctx<Self>,
			params: Self::ConnParams,
		) -> Result<Self::ConnState> {
			Ok(ConnState {
				value: params.value + 1,
			})
		}

		async fn on_connect(self: Arc<Self>, ctx: Ctx<Self>, conn: ConnCtx<Self>) -> Result<()> {
			let conn_state = conn.state()?;
			ctx.state_mut()
				.log
				.push(format!("on_connect:{}:{}", conn.id(), conn_state.value));
			Ok(())
		}

		async fn on_disconnect(self: Arc<Self>, ctx: Ctx<Self>, conn: ConnCtx<Self>) {
			ctx.state_mut()
				.log
				.push(format!("on_disconnect:{}", conn.id()));
		}

		async fn on_subscribe(
			self: Arc<Self>,
			ctx: Ctx<Self>,
			conn: ConnCtx<Self>,
			event_name: String,
		) -> Result<()> {
			let conn_state = conn.state()?;
			ctx.state_mut()
				.log
				.push(format!("on_subscribe:{event_name}:{}", conn_state.value));
			if event_name == "denied" {
				anyhow::bail!("subscribe denied");
			}
			Ok(())
		}
	}

	#[derive(Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct LifecycleInput {
		count: u32,
	}

	#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct LifecycleState {
		count: u32,
		log: Vec<String>,
	}

	#[derive(Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct UnitInput;

	#[derive(Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct ExampleState {
		count: u32,
		label: String,
	}

	#[test]
	fn input_decode_round_trips_unit() {
		let bytes = cbor(&());
		let input = Input::<EmptyActor> {
			bytes: Some(bytes.clone()),
			_p: PhantomData,
		};

		assert!(input.is_present());
		assert_eq!(input.raw(), Some(bytes.as_slice()));
		assert_eq!(input.decode().expect("decode unit input"), ());
	}

	#[test]
	fn input_decode_round_trips_unit_struct() {
		let input = Input::<UnitActor> {
			bytes: Some(cbor(&UnitInput)),
			_p: PhantomData,
		};

		assert_eq!(input.decode().expect("decode unit struct input"), UnitInput);
	}

	#[test]
	fn input_decode_or_default_uses_default_when_missing() {
		let input = Input::<DefaultActor> {
			bytes: None,
			_p: PhantomData,
		};

		assert_eq!(
			input.decode_or_default().expect("default input"),
			DefaultInput { count: 7 }
		);
	}

	#[test]
	fn input_decode_treats_missing_unit_as_unit() {
		let input = Input::<EmptyActor> {
			bytes: None,
			_p: PhantomData,
		};

		assert_eq!(input.decode().expect("missing unit input"), ());
	}

	#[test]
	fn input_decode_or_default_treats_empty_bytes_as_missing() {
		// An empty base64 input decodes to an empty (non-null) buffer, which
		// must default rather than fail CBOR decoding.
		let input = Input::<DefaultActor> {
			bytes: Some(Vec::new()),
			_p: PhantomData,
		};

		assert_eq!(
			input.decode_or_default().expect("default input"),
			DefaultInput { count: 7 }
		);
	}

	#[test]
	fn input_decode_treats_empty_unit_as_unit() {
		let input = Input::<EmptyActor> {
			bytes: Some(Vec::new()),
			_p: PhantomData,
		};

		assert_eq!(input.decode().expect("empty unit input"), ());
	}

	#[test]
	fn connection_params_decode_null_as_default() {
		assert_eq!(
			decode_conn_params::<LifecycleActor>(&[0xf6]).expect("decode null conn params"),
			ConnParams::default()
		);
		assert_eq!(
			decode_conn_params::<LifecycleActor>(&[]).expect("decode empty conn params"),
			ConnParams::default()
		);
	}

	#[test]
	fn snapshot_decode_round_trips_map_struct() {
		let snapshot = Snapshot {
			is_new: false,
			bytes: Some(cbor(&ExampleState {
				count: 9,
				label: "hi".into(),
			})),
		};

		assert!(!snapshot.is_new());
		assert_eq!(
			snapshot.decode::<ExampleState>().expect("decode snapshot"),
			Some(ExampleState {
				count: 9,
				label: "hi".into(),
			})
		);
	}

	#[test]
	fn snapshot_decode_or_default_uses_default_when_missing() {
		let snapshot = Snapshot {
			is_new: true,
			bytes: None,
		};

		assert!(snapshot.is_new());
		assert_eq!(
			snapshot
				.decode_or_default::<ExampleState>()
				.expect("default snapshot"),
			ExampleState::default()
		);
	}

	#[test]
	fn empty_snapshot_decodes_as_missing_without_changing_newness() {
		let snapshot = Snapshot {
			is_new: false,
			bytes: Some(Vec::new()),
		};

		assert!(!snapshot.is_new());
		assert_eq!(
			snapshot
				.decode_or_default::<ExampleState>()
				.expect("default empty snapshot"),
			ExampleState::default()
		);
	}

	#[test]
	fn wrap_start_rehydrates_hibernated_connection_state() {
		let (tx, rx) = unbounded_channel();
		drop(tx);
		let start = wrap_start::<ConnActor>(ActorStart {
			ctx: rivetkit_core::testing::actor_context("actor-id", "test", Vec::new(), "local"),
			input: None,
			is_new: true,
			snapshot: None,
			hibernated: vec![(
				rivetkit_core::ConnHandle::new(
					"conn-id",
					cbor(&()),
					cbor(&ConnState { value: 1 }),
					true,
				),
				cbor(&ConnState { value: 5 }),
			)],
			events: rx.into(),
			startup_ready: None,
		})
		.expect("wrap start");

		assert_eq!(
			start.hibernated[0]
				.conn
				.state()
				.expect("decode hibernated conn state"),
			ConnState { value: 5 }
		);
	}

	#[test]
	fn events_try_recv_wraps_core_events() {
		let (tx, rx) = unbounded_channel();
		tx.send(ActorEvent::ConnectionClosed {
			conn: rivetkit_core::ConnHandle::new("conn-id", cbor(&()), cbor(&()), true),
		})
		.expect("queue event");

		let mut events = Events::<EmptyActor> {
			ctx: Ctx::new(rivetkit_core::testing::actor_context(
				"actor-id",
				"test",
				Vec::new(),
				"local",
			)),
			rx: rx.into(),
			_p: PhantomData,
		};

		let Some(RuntimeEvent::ConnClosed(closed)) = events.try_recv() else {
			panic!("expected typed connection-closed event");
		};

		assert_eq!(closed.conn.id(), "conn-id");
	}

	#[tokio::test]
	async fn run_actor_creates_state_and_replies_with_snapshot() {
		let (tx, rx) = unbounded_channel();
		let start = lifecycle_start(Some(cbor(&LifecycleInput { count: 3 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));

		let deltas = request_serialize(&tx).await;
		let state = decode_actor_state(deltas);
		assert_eq!(state.count, 3);
		assert_eq!(
			state.log,
			[
				"create_state",
				"create",
				"on_create",
				"on_start",
				"on_state_change",
			]
		);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_rehydrates_snapshot_without_on_create() {
		let snapshot = LifecycleState {
			count: 8,
			log: vec!["snapshot".into()],
		};
		let (tx, rx) = unbounded_channel();
		let start = lifecycle_start(None, Some(cbor(&snapshot)), rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));

		let state = decode_actor_state(request_serialize(&tx).await);
		assert_eq!(state.count, 8);
		assert_eq!(
			state.log,
			["snapshot", "create", "on_start", "on_state_change"]
		);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_default_fetch_replies_404() {
		let (tx, rx) = unbounded_channel();
		let start = lifecycle_start(Some(cbor(&LifecycleInput { count: 1 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));

		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::HttpRequest {
			request: rivetkit_core::Request::default(),
			reply: reply_tx.into(),
		})
		.expect("send http event");

		let response = reply_rx.await.expect("http reply").expect("http response");
		assert_eq!(response.status().as_u16(), 404);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_missing_unit_input_creates_state() {
		let (tx, rx) = unbounded_channel();
		let start = unit_creation_start(None, None, rx.into());
		let actor = tokio::spawn(run_actor::<UnitCreationActor>(start));

		let state = decode_unit_creation_state(request_serialize(&tx).await);
		assert_eq!(state.created, 1);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_invalid_input_fails_to_start() {
		// Non-empty bytes that are not valid CBOR for the input type must fail
		// the actor start rather than silently defaulting.
		let (_tx, rx) = unbounded_channel();
		let start = lifecycle_start(Some(vec![0xff, 0xff, 0xff]), None, rx.into());

		let error = run_actor::<LifecycleActor>(start)
			.await
			.expect_err("invalid input should fail actor start");
		assert!(
			format!("{error:#}").contains("decode actor input from cbor"),
			"unexpected error: {error:#}"
		);
	}

	#[tokio::test]
	async fn run_actor_default_websocket_rejects() {
		let (tx, rx) = unbounded_channel();
		let start = lifecycle_start(Some(cbor(&LifecycleInput { count: 1 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));

		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::WebSocketOpen {
			conn: conn("ws-conn", (), ConnState::default()),
			ws: WebSocket::new(),
			request: Some(rivetkit_core::Request::default()),
			reply: reply_tx.into(),
		})
		.expect("send websocket event");

		let error = reply_rx
			.await
			.expect("websocket reply")
			.expect_err("default websocket should reject");
		assert!(error.to_string().contains("websockets not supported"));

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_connection_hooks_store_state_and_disconnect() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) =
			lifecycle_start_with_ctx(Some(cbor(&LifecycleInput { count: 1 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));
		let params = ConnParams {
			allow: true,
			value: 41,
		};
		let conn = conn("conn-hooks", params.clone(), ConnState::default());

		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::ConnectionPreflight {
			conn: conn.clone(),
			params: cbor(&params),
			request: None,
			reply: reply_tx.into(),
		})
		.expect("send connection preflight");
		reply_rx
			.await
			.expect("connection preflight reply")
			.expect("connection preflight result");
		assert_eq!(
			decode_cbor::<ConnState>(&conn.state(), "connection state").expect("conn state"),
			ConnState { value: 42 }
		);

		tx.send(ActorEvent::ConnectionClosed { conn })
			.expect("send connection closed");
		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");

		let log = &ctx.state().log;
		assert!(log.contains(&"on_before_connect:41".to_owned()));
		assert!(log.contains(&"on_connect:conn-hooks:42".to_owned()));
		assert!(log.contains(&"on_disconnect:conn-hooks".to_owned()));
	}

	#[tokio::test]
	async fn run_actor_subscribe_hook_allows_and_denies() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) =
			lifecycle_start_with_ctx(Some(cbor(&LifecycleInput { count: 1 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));
		let conn = conn(
			"subscribe-conn",
			ConnParams::default(),
			ConnState { value: 91 },
		);

		request_subscribe(&tx, conn.clone(), "chat.message")
			.await
			.expect("subscribe should be allowed");
		let error = request_subscribe(&tx, conn, "denied")
			.await
			.expect_err("subscribe should be denied");
		assert!(error.to_string().contains("subscribe denied"));

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");

		let log = &ctx.state().log;
		assert!(log.contains(&"on_subscribe:chat.message:91".to_owned()));
		assert!(log.contains(&"on_subscribe:denied:91".to_owned()));
	}

	#[tokio::test]
	async fn run_actor_connection_preflight_rejects_before_connect() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) =
			lifecycle_start_with_ctx(Some(cbor(&LifecycleInput { count: 1 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));
		let params = ConnParams {
			allow: false,
			value: 5,
		};

		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::ConnectionPreflight {
			conn: conn("conn-reject", params.clone(), ConnState::default()),
			params: cbor(&params),
			request: None,
			reply: reply_tx.into(),
		})
		.expect("send rejected connection preflight");

		let error = reply_rx
			.await
			.expect("connection preflight reply")
			.expect_err("connection preflight should reject");
		assert!(error.to_string().contains("connection rejected"));

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
		assert!(
			!ctx.state()
				.log
				.iter()
				.any(|entry| entry.starts_with("on_connect:conn-reject"))
		);
	}

	#[tokio::test]
	async fn run_actor_cancels_run_with_abort_signal() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) =
			lifecycle_start_with_ctx(Some(cbor(&LifecycleInput { count: 2 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");

		assert!(ctx.state().log.iter().any(|entry| entry == "run_aborted"));
	}

	#[tokio::test]
	async fn run_actor_destroy_cleanup_fires() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) =
			lifecycle_start_with_ctx(Some(cbor(&LifecycleInput { count: 2 })), None, rx.into());
		let actor = tokio::spawn(run_actor::<LifecycleActor>(start));

		request_destroy(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");

		assert!(ctx.state().log.iter().any(|entry| entry == "on_destroy"));
	}

	#[tokio::test]
	async fn run_actor_error_requests_errored_stop() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) = unit_start_with_ctx::<FailingRunActor>(rx.into());
		// The harness has no ActorTask to complete startup, so mark the
		// lifecycle started directly; stop requests are rejected before then.
		ctx.inner().set_started(true);
		let actor = tokio::spawn(run_actor::<FailingRunActor>(start));

		// The errored stop is requested inside the spawned run task right
		// after `run` returns, so yielding the current-thread runtime drives
		// it to completion deterministically.
		for _ in 0..1000 {
			if ctx.inner().is_destroy_requested() {
				break;
			}
			tokio::task::yield_now().await;
		}
		assert!(
			ctx.inner().is_destroy_requested(),
			"run error should request an errored stop"
		);

		drop(tx);
		let error = actor
			.await
			.expect("join run_actor")
			.expect_err("run_actor should propagate the run error");
		assert!(format!("{error:#}").contains("boom in run"));
	}

	#[tokio::test]
	async fn run_actor_panic_requests_errored_stop() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) = unit_start_with_ctx::<PanickingRunActor>(rx.into());
		ctx.inner().set_started(true);
		let actor = tokio::spawn(run_actor::<PanickingRunActor>(start));

		// See run_actor_error_requests_errored_stop for why yielding drives
		// the spawned run task deterministically.
		for _ in 0..1000 {
			if ctx.inner().is_destroy_requested() {
				break;
			}
			tokio::task::yield_now().await;
		}
		assert!(
			ctx.inner().is_destroy_requested(),
			"run panic should request an errored stop"
		);

		drop(tx);
		let error = actor
			.await
			.expect("join run_actor")
			.expect_err("run_actor should propagate the panic as an error");
		assert!(format!("{error:#}").contains("panicked"));
	}

	#[tokio::test]
	async fn run_actor_shutdown_error_is_not_reported_as_crash() {
		let (tx, rx) = unbounded_channel();
		let (start, ctx) = unit_start_with_ctx::<ShutdownErrRunActor>(rx.into());
		ctx.inner().set_started(true);
		let actor = tokio::spawn(run_actor::<ShutdownErrRunActor>(start));

		// Closing the events channel starts the shutdown join path, which
		// cancels the abort signal before joining the run task.
		drop(tx);
		actor
			.await
			.expect("join run_actor")
			.expect_err("run error should still propagate during shutdown");
		assert!(
			!ctx.inner().is_destroy_requested(),
			"shutdown-induced run errors must not be reported as crashes"
		);
	}

	#[tokio::test]
	async fn run_actor_dispatches_typed_actions_by_arg_shape() {
		let (tx, rx) = unbounded_channel();
		let start = action_start(rx.into());
		let actor = tokio::spawn(run_actor::<ActionActor>(start));

		assert_eq!(
			decode_cbor::<u32>(
				&request_action(
					&tx,
					"add",
					&encode_positional(&Add { left: 2, right: 3 }).expect("encode add"),
					None,
				)
				.await
				.expect("add result"),
				"add output",
			)
			.expect("decode add output"),
			5
		);
		assert_eq!(
			decode_cbor::<u32>(
				&request_action(
					&tx,
					"scale",
					&encode_positional(&Scale(4, 5)).expect("encode scale"),
					None,
				)
				.await
				.expect("scale result"),
				"scale output",
			)
			.expect("decode scale output"),
			20
		);
		assert_eq!(
			decode_cbor::<String>(
				&request_action(
					&tx,
					"echo",
					&encode_positional(&Echo("hi".to_owned())).expect("encode echo"),
					None,
				)
				.await
				.expect("echo result"),
				"echo output",
			)
			.expect("decode echo output"),
			"hi"
		);
		assert_eq!(
			decode_cbor::<String>(
				&request_action(
					&tx,
					"ping",
					&encode_positional(&Ping).expect("encode ping"),
					None,
				)
				.await
				.expect("ping result"),
				"ping output",
			)
			.expect("decode ping output"),
			"pong"
		);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_dispatches_actions_concurrently() {
		let (tx, rx) = unbounded_channel();
		let _ = ACTION_BARRIER.set(Arc::new(Barrier::new(2)));
		let start = action_start(rx.into());
		let actor = tokio::spawn(run_actor::<ActionActor>(start));
		let first_args = encode_positional(&WaitForPeer {
			label: "first".to_owned(),
		})
		.expect("encode first wait");
		let second_args = encode_positional(&WaitForPeer {
			label: "second".to_owned(),
		})
		.expect("encode second wait");

		let first = request_action_rx(&tx, "wait", &first_args, None);
		let second = request_action_rx(&tx, "wait", &second_args, None);

		let (first, second) = tokio::time::timeout(Duration::from_secs(1), async move {
			tokio::join!(first, second)
		})
		.await
		.expect("concurrent handlers should rendezvous");
		assert_eq!(
			decode_cbor::<String>(&first.expect("first result"), "first wait output")
				.expect("decode first wait"),
			"first"
		);
		assert_eq!(
			decode_cbor::<String>(&second.expect("second result"), "second wait output")
				.expect("decode second wait"),
			"second"
		);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_action_errors_and_unknown_action_are_structured() {
		let (tx, rx) = unbounded_channel();
		let start = action_start(rx.into());
		let actor = tokio::spawn(run_actor::<ActionActor>(start));

		let error = request_action(
			&tx,
			"fail",
			&encode_positional(&Fail).expect("encode fail"),
			None,
		)
		.await
		.expect_err("fail action should error");
		assert!(error.to_string().contains("intentional action failure"));

		let error = request_action(&tx, "missing", &[], None)
			.await
			.expect_err("missing action should error");
		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "action_not_found");

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_action_receives_per_call_connection_state() {
		let (tx, rx) = unbounded_channel();
		let start = action_start(rx.into());
		let actor = tokio::spawn(run_actor::<ActionActor>(start));
		let conn = conn(
			"action-conn",
			ConnParams::default(),
			ConnState { value: 77 },
		);

		let output = request_action(
			&tx,
			"connValue",
			&encode_positional(&ConnValue).expect("encode conn value"),
			Some(conn),
		)
		.await
		.expect("conn value result");

		assert_eq!(
			decode_cbor::<u32>(&output, "conn value output").expect("decode conn value"),
			77
		);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_dispatches_typed_queue_send() {
		let (tx, rx) = unbounded_channel();
		let start = action_start(rx.into());
		let actor = tokio::spawn(run_actor::<ActionActor>(start));
		let conn = conn("queue-conn", ConnParams::default(), ConnState::default());

		let result = request_queue_send(
			&tx,
			"double",
			&cbor(&QueueDouble { value: 21 }),
			conn.clone(),
		)
		.await
		.expect("queue result");
		assert_eq!(result.status, QueueSendStatus::Completed);
		assert_eq!(
			decode_cbor::<u32>(
				result.response.as_deref().expect("queue response"),
				"queue response",
			)
			.expect("decode queue response"),
			42
		);

		let error = request_queue_send(&tx, "missing", &[], conn)
			.await
			.expect_err("missing queue should error");
		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "not_found");

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	#[tokio::test]
	async fn run_actor_can_pull_typed_queue_backlog_until_abort() {
		let (done_tx, done_rx) = oneshot::channel();
		let drained = QUEUE_PULL_DRAINED.get_or_init(|| parking_lot::Mutex::new(None));
		assert!(
			drained.lock().replace(done_tx).is_none(),
			"queue pull notifier already installed"
		);

		let (tx, rx) = unbounded_channel();
		let start = queue_pull_start(rx.into());
		start
			.ctx
			.queue()
			.send("double", &QueueDouble { value: 3 })
			.await
			.expect("send first queue message");
		start
			.ctx
			.queue()
			.send("double", &QueueDouble { value: 4 })
			.await
			.expect("send second queue message");

		let actor = tokio::spawn(run_actor::<QueuePullActor>(start));
		let values = done_rx.await.expect("queue drain notification");
		assert_eq!(values, vec![3, 4]);

		let state = request_serialize(&tx).await;
		let [StateDelta::ActorState(bytes)] = state.as_slice() else {
			panic!("expected actor state delta");
		};
		assert_eq!(
			decode_cbor::<Vec<u32>>(bytes, "queue pull state").expect("decode state"),
			vec![3, 4]
		);

		request_sleep(&tx).await;
		actor.await.expect("join run_actor").expect("run actor");
	}

	struct DefaultActor;

	impl Actor for DefaultActor {
		type State = ();
		type Input = DefaultInput;
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;
	}

	#[derive(Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct DefaultInput {
		count: u32,
	}

	impl Default for DefaultInput {
		fn default() -> Self {
			Self { count: 7 }
		}
	}

	struct ConnActor;

	impl Actor for ConnActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ConnState;
		type Action = action::Raw;
	}

	#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct ConnParams {
		allow: bool,
		value: u32,
	}

	#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct ConnState {
		value: u32,
	}

	struct UnitCreationActor;

	#[async_trait]
	impl Actor for UnitCreationActor {
		type State = UnitCreationState;
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, (): Self::Input) -> Result<Self::State> {
			Ok(UnitCreationState { created: 1 })
		}

		async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
			Ok(Self)
		}
	}

	#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
	struct UnitCreationState {
		created: u32,
	}

	struct ActionActor;

	#[async_trait]
	impl Actor for ActionActor {
		type State = ();
		type Input = ();
		type Actions = (Add, Scale, Echo, Ping, Fail, WaitForPeer, ConnValue);
		type Events = ();
		type Queue = (QueueDouble,);
		type ConnParams = ConnParams;
		type ConnState = ConnState;
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, (): Self::Input) -> Result<Self::State> {
			Ok(())
		}

		async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
			Ok(Self)
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct Add {
		left: u32,
		right: u32,
	}

	impl Action for Add {
		type Output = u32;

		const NAME: &'static str = "add";
	}

	impl Handles<Add> for ActionActor {
		type Future = BoxTestFuture<u32>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: Add) -> Self::Future {
			Box::pin(async move { Ok(action.left + action.right) })
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct Scale(u32, u32);

	impl Action for Scale {
		type Output = u32;

		const NAME: &'static str = "scale";
	}

	impl Handles<Scale> for ActionActor {
		type Future = BoxTestFuture<u32>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: Scale) -> Self::Future {
			Box::pin(async move { Ok(action.0 * action.1) })
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct Echo(String);

	impl Action for Echo {
		type Output = String;

		const NAME: &'static str = "echo";
	}

	impl Handles<Echo> for ActionActor {
		type Future = BoxTestFuture<String>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: Echo) -> Self::Future {
			Box::pin(async move { Ok(action.0) })
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct Ping;

	impl Action for Ping {
		type Output = String;

		const NAME: &'static str = "ping";
	}

	impl Handles<Ping> for ActionActor {
		type Future = BoxTestFuture<String>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: Ping) -> Self::Future {
			Box::pin(async move { Ok("pong".to_owned()) })
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct Fail;

	impl Action for Fail {
		type Output = ();

		const NAME: &'static str = "fail";
	}

	impl Handles<Fail> for ActionActor {
		type Future = BoxTestFuture<()>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: Fail) -> Self::Future {
			Box::pin(async move { anyhow::bail!("intentional action failure") })
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct WaitForPeer {
		label: String,
	}

	impl Action for WaitForPeer {
		type Output = String;

		const NAME: &'static str = "wait";
	}

	impl Handles<WaitForPeer> for ActionActor {
		type Future = BoxTestFuture<String>;

		fn handle(self: Arc<Self>, _ctx: Ctx<Self>, action: WaitForPeer) -> Self::Future {
			Box::pin(async move {
				ACTION_BARRIER
					.get()
					.expect("action barrier should be installed")
					.wait()
					.await;
				Ok(action.label)
			})
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct ConnValue;

	impl Action for ConnValue {
		type Output = u32;

		const NAME: &'static str = "connValue";
	}

	impl Handles<ConnValue> for ActionActor {
		type Future = BoxTestFuture<u32>;

		fn handle(self: Arc<Self>, ctx: Ctx<Self>, _action: ConnValue) -> Self::Future {
			Box::pin(async move {
				let conn = ctx.conn().context("missing action connection")?;
				Ok(conn.state()?.value)
			})
		}
	}

	#[derive(Debug, Clone, Serialize, Deserialize)]
	struct QueueDouble {
		value: u32,
	}

	impl QueueMessage for QueueDouble {
		type Reply = u32;

		const NAME: &'static str = "double";
	}

	impl HandlesQueue<QueueDouble> for ActionActor {
		type Future = BoxTestFuture<u32>;

		fn handle_queue(self: Arc<Self>, _ctx: Ctx<Self>, message: QueueDouble) -> Self::Future {
			Box::pin(async move { Ok(message.value * 2) })
		}
	}

	struct QueuePullActor;

	#[async_trait]
	impl Actor for QueuePullActor {
		type State = Vec<u32>;
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, (): Self::Input) -> Result<Self::State> {
			Ok(Vec::new())
		}

		async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
			Ok(Self)
		}

		async fn run(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			let mut values = Vec::new();

			for _ in 0..2 {
				let message = ctx
					.queue()
					.next_typed::<QueueDouble>(QueueNextOpts {
						completable: true,
						..Default::default()
					})
					.await?
					.context("expected queued message")?;
				let value = message.body().value;
				message.complete(value * 2).await?;
				values.push(value);
			}

			ctx.state_mut().extend(values.iter().copied());
			if let Some(done) = QUEUE_PULL_DRAINED
				.get_or_init(|| parking_lot::Mutex::new(None))
				.lock()
				.take()
			{
				let _ = done.send(values);
			}

			ctx.abort_signal().cancelled().await;
			Ok(())
		}
	}

	struct FailingRunActor;

	#[async_trait]
	impl Actor for FailingRunActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, (): Self::Input) -> Result<Self::State> {
			Ok(())
		}

		async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
			Ok(Self)
		}

		async fn run(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
			anyhow::bail!("boom in run")
		}
	}

	struct PanickingRunActor;

	#[async_trait]
	impl Actor for PanickingRunActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, (): Self::Input) -> Result<Self::State> {
			Ok(())
		}

		async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
			Ok(Self)
		}

		async fn run(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
			panic!("run task panic under test");
		}
	}

	struct ShutdownErrRunActor;

	#[async_trait]
	impl Actor for ShutdownErrRunActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;

		async fn create_state(_ctx: &Ctx<Self>, (): Self::Input) -> Result<Self::State> {
			Ok(())
		}

		async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
			Ok(Self)
		}

		async fn run(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
			ctx.abort_signal().cancelled().await;
			anyhow::bail!("wait cancelled by shutdown")
		}
	}

	fn unit_start_with_ctx<A: Actor>(rx: ActorEvents) -> (Start<A>, Ctx<A>) {
		let ctx = Ctx::new(rivetkit_core::testing::actor_context(
			"actor-id",
			"test",
			Vec::new(),
			"local",
		));

		let start = Start {
			ctx: ctx.clone(),
			input: Input {
				bytes: None,
				_p: PhantomData,
			},
			is_new: true,
			snapshot: Snapshot {
				is_new: true,
				bytes: None,
			},
			hibernated: Vec::new(),
			events: Events {
				ctx: ctx.clone(),
				rx,
				_p: PhantomData,
			},
			startup_ready: None,
		};

		(start, ctx)
	}

	fn cbor<T: Serialize>(value: &T) -> Vec<u8> {
		let mut encoded = Vec::new();
		ciborium::into_writer(value, &mut encoded).expect("encode test value as cbor");
		encoded
	}

	fn lifecycle_start(
		input: Option<Vec<u8>>,
		snapshot: Option<Vec<u8>>,
		rx: ActorEvents,
	) -> Start<LifecycleActor> {
		lifecycle_start_with_ctx(input, snapshot, rx).0
	}

	fn lifecycle_start_with_ctx(
		input: Option<Vec<u8>>,
		snapshot: Option<Vec<u8>>,
		rx: ActorEvents,
	) -> (Start<LifecycleActor>, Ctx<LifecycleActor>) {
		let ctx = Ctx::new(rivetkit_core::testing::actor_context(
			"actor-id",
			"test",
			Vec::new(),
			"local",
		));

		let is_new = snapshot.is_none();
		let start = Start {
			ctx: ctx.clone(),
			input: Input {
				bytes: input,
				_p: PhantomData,
			},
			is_new,
			snapshot: Snapshot {
				is_new,
				bytes: snapshot,
			},
			hibernated: Vec::new(),
			events: Events {
				ctx: ctx.clone(),
				rx,
				_p: PhantomData,
			},
			startup_ready: None,
		};

		(start, ctx)
	}

	fn unit_creation_start(
		input: Option<Vec<u8>>,
		snapshot: Option<Vec<u8>>,
		rx: ActorEvents,
	) -> Start<UnitCreationActor> {
		let ctx = Ctx::new(rivetkit_core::testing::actor_context(
			"actor-id",
			"unit-creation",
			Vec::new(),
			"local",
		));

		let is_new = snapshot.is_none();
		Start {
			ctx: ctx.clone(),
			input: Input {
				bytes: input,
				_p: PhantomData,
			},
			is_new,
			snapshot: Snapshot {
				is_new,
				bytes: snapshot,
			},
			hibernated: Vec::new(),
			events: Events {
				ctx,
				rx,
				_p: PhantomData,
			},
			startup_ready: None,
		}
	}

	fn action_start(rx: ActorEvents) -> Start<ActionActor> {
		let ctx = Ctx::new(rivetkit_core::testing::actor_context(
			"actor-id",
			"action-test",
			Vec::new(),
			"local",
		));

		Start {
			ctx: ctx.clone(),
			input: Input {
				bytes: None,
				_p: PhantomData,
			},
			is_new: true,
			snapshot: Snapshot {
				is_new: true,
				bytes: None,
			},
			hibernated: Vec::new(),
			events: Events {
				ctx,
				rx,
				_p: PhantomData,
			},
			startup_ready: None,
		}
	}

	fn queue_pull_start(rx: ActorEvents) -> Start<QueuePullActor> {
		let ctx = Ctx::new(rivetkit_core::testing::actor_context(
			"actor-id",
			"queue-pull-test",
			Vec::new(),
			"local",
		));

		Start {
			ctx: ctx.clone(),
			input: Input {
				bytes: None,
				_p: PhantomData,
			},
			is_new: true,
			snapshot: Snapshot {
				is_new: true,
				bytes: None,
			},
			hibernated: Vec::new(),
			events: Events {
				ctx,
				rx,
				_p: PhantomData,
			},
			startup_ready: None,
		}
	}

	async fn request_serialize(
		tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>,
	) -> Vec<StateDelta> {
		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::SerializeState {
			reason: rivetkit_core::SerializeStateReason::Save,
			reply: reply_tx.into(),
		})
		.expect("send serialize event");
		reply_rx
			.await
			.expect("serialize reply")
			.expect("serialize result")
	}

	async fn request_sleep(tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>) {
		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::RunGracefulCleanup {
			reason: ShutdownKind::Sleep,
			reply: reply_tx.into(),
		})
		.expect("send sleep cleanup");
		reply_rx.await.expect("sleep reply").expect("sleep result");
	}

	async fn request_action(
		tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>,
		name: &str,
		args: &[u8],
		conn: Option<ConnHandle>,
	) -> Result<Vec<u8>> {
		request_action_rx(tx, name, args, conn).await
	}

	async fn request_action_rx(
		tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>,
		name: &str,
		args: &[u8],
		conn: Option<ConnHandle>,
	) -> Result<Vec<u8>> {
		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::Action {
			name: name.to_owned(),
			args: args.to_vec(),
			conn,
			scheduled_fire: None,
			reply: reply_tx.into(),
		})
		.expect("send action event");
		reply_rx.await.expect("action reply")
	}

	async fn request_queue_send(
		tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>,
		name: &str,
		body: &[u8],
		conn: ConnHandle,
	) -> Result<QueueSendResult> {
		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::QueueSend {
			name: name.to_owned(),
			body: body.to_vec(),
			conn,
			request: rivetkit_core::Request::default(),
			wait: true,
			timeout_ms: None,
			reply: reply_tx.into(),
		})
		.expect("send queue event");
		reply_rx.await.expect("queue reply")
	}

	async fn request_subscribe(
		tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>,
		conn: ConnHandle,
		event_name: &str,
	) -> Result<()> {
		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::SubscribeRequest {
			conn,
			event_name: event_name.to_owned(),
			reply: reply_tx.into(),
		})
		.expect("send subscribe event");
		reply_rx.await.expect("subscribe reply")
	}

	async fn request_destroy(tx: &tokio::sync::mpsc::UnboundedSender<ActorEvent>) {
		let (reply_tx, reply_rx) = oneshot::channel();
		tx.send(ActorEvent::RunGracefulCleanup {
			reason: ShutdownKind::Destroy,
			reply: reply_tx.into(),
		})
		.expect("send destroy cleanup");
		reply_rx
			.await
			.expect("destroy reply")
			.expect("destroy result");
	}

	fn decode_actor_state(deltas: Vec<StateDelta>) -> LifecycleState {
		let [StateDelta::ActorState(bytes)] = deltas.as_slice() else {
			panic!("expected one actor state delta");
		};
		decode_cbor(bytes, "actor state").expect("decode actor state")
	}

	fn decode_unit_creation_state(deltas: Vec<StateDelta>) -> UnitCreationState {
		let [StateDelta::ActorState(bytes)] = deltas.as_slice() else {
			panic!("expected one actor state delta");
		};
		decode_cbor(bytes, "actor state").expect("decode actor state")
	}

	fn conn<P: Serialize, S: Serialize>(id: &str, params: P, state: S) -> ConnHandle {
		ConnHandle::new(id, cbor(&params), cbor(&state), true)
	}
}
