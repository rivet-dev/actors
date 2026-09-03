use std::collections::HashMap;
use std::sync::Arc;

use rivet_envoy_client::actor::ToActor;
use rivet_envoy_client::async_counter::AsyncCounter;
use rivet_envoy_client::commands::{handle_commands, send_command_ack};
use rivet_envoy_client::config::{
	BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
	WebSocketSender,
};
use rivet_envoy_client::context::{SharedContext, WsTxMessage};
use rivet_envoy_client::envoy::EnvoyContext;
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_client::sqlite::{
	RemoteSqliteRequest, fail_sent_remote_sqlite_requests_with_indeterminate_result,
	handle_remote_sqlite_request,
};
use rivet_envoy_client::utils::{BufferMap, RemoteSqliteIndeterminateResultError};
use rivet_envoy_protocol as protocol;
use tokio::sync::mpsc;
use vbare::OwnedVersionedData;

struct IdleCallbacks;

impl EnvoyCallbacks for IdleCallbacks {
	fn on_actor_start(
		&self,
		_handle: EnvoyHandle,
		_actor_id: String,
		_generation: u32,
		_config: protocol::ActorConfig,
		_preloaded_kv: Option<protocol::PreloadedKv>,
	) -> BoxFuture<anyhow::Result<()>> {
		Box::pin(async { Ok(()) })
	}

	fn on_shutdown(&self) {}

	fn fetch(
		&self,
		_handle: EnvoyHandle,
		_actor_id: String,
		_gateway_id: protocol::GatewayId,
		_request_id: protocol::RequestId,
		_request: HttpRequest,
	) -> BoxFuture<anyhow::Result<HttpResponse>> {
		Box::pin(async { anyhow::bail!("fetch should not be called in command tests") })
	}

	fn websocket(
		&self,
		_handle: EnvoyHandle,
		_actor_id: String,
		_gateway_id: protocol::GatewayId,
		_request_id: protocol::RequestId,
		_request: HttpRequest,
		_path: String,
		_headers: HashMap<String, String>,
		_is_hibernatable: bool,
		_is_restoring_hibernatable: bool,
		_sender: WebSocketSender,
	) -> BoxFuture<anyhow::Result<WebSocketHandler>> {
		Box::pin(async { anyhow::bail!("websocket should not be called in command tests") })
	}

	fn can_hibernate(
		&self,
		_actor_id: &str,
		_gateway_id: &protocol::GatewayId,
		_request_id: &protocol::RequestId,
		_request: &HttpRequest,
	) -> BoxFuture<anyhow::Result<bool>> {
		Box::pin(async { Ok(false) })
	}
}

fn new_envoy_context() -> EnvoyContext {
	let (envoy_tx, _envoy_rx) = mpsc::unbounded_channel();
	let shared = Arc::new(SharedContext {
		config: EnvoyConfig {
			version: 1,
			endpoint: "http://127.0.0.1:1".to_string(),
			token: None,
			namespace: "test".to_string(),
			pool_name: "test".to_string(),
			prepopulate_actor_names: HashMap::new(),
			metadata: None,
			not_global: true,
			debug_latency_ms: None,
			callbacks: Arc::new(IdleCallbacks),
		},
		envoy_key: "test-envoy".to_string(),
		envoy_tx,
		actors: Arc::new(std::sync::Mutex::new(HashMap::new())),
		actors_notify: Arc::new(tokio::sync::Notify::new()),
		live_tunnel_requests: Arc::new(std::sync::Mutex::new(HashMap::new())),
		pending_hibernation_restores: Arc::new(std::sync::Mutex::new(HashMap::new())),
		ws_tx: Arc::new(tokio::sync::Mutex::new(
			None::<mpsc::UnboundedSender<WsTxMessage>>,
		)),
		http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
		connection_session: std::sync::atomic::AtomicU64::new(0),
		next_connection_session: std::sync::atomic::AtomicU64::new(0),
		connection_session_tx: tokio::sync::watch::channel(0).0,
		protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
		shutting_down: std::sync::atomic::AtomicBool::new(false),
		last_ping_ts: std::sync::atomic::AtomicI64::new(0),
		stopped_tx: tokio::sync::watch::channel(true).0,
	});
	EnvoyContext {
		shared,
		shutting_down: false,
		actors: HashMap::new(),
		buffered_actor_messages: HashMap::new(),
		kv_requests: HashMap::new(),
		next_kv_request_id: 0,
		sqlite_requests: HashMap::new(),
		next_sqlite_request_id: 0,
		remote_sqlite_requests: HashMap::new(),
		next_remote_sqlite_request_id: 0,
		request_to_actor: BufferMap::new(),
		http_request_routes: BufferMap::new(),
		http_message_indices: BufferMap::new(),
		http_request_cancellations: HashMap::new(),
		buffered_messages: Vec::new(),
		processed_command_idx: HashMap::new(),
	}
}

fn stop_command(actor_id: &str, generation: u32, index: i64) -> protocol::CommandWrapper {
	protocol::CommandWrapper {
		checkpoint: protocol::ActorCheckpoint {
			actor_id: actor_id.to_string(),
			generation,
			index,
		},
		inner: protocol::Command::CommandStopActor(protocol::CommandStopActor {
			reason: protocol::StopActorReason::StopIntent,
		}),
	}
}

fn execute_request() -> protocol::SqliteExecuteRequest {
	protocol::SqliteExecuteRequest {
		namespace_id: "test".to_string(),
		actor_id: "actor-replay".to_string(),
		generation: 1,
		sql: "insert into test values (?)".to_string(),
		params: Some(vec![protocol::SqliteBindParam::SqliteValueText(
			protocol::SqliteValueText {
				value: "value".to_string(),
			},
		)]),
	}
}

#[tokio::test]
async fn replayed_stop_command_is_dropped() {
	let mut ctx = new_envoy_context();
	let (actor_tx, mut actor_rx) = mpsc::unbounded_channel::<ToActor>();
	ctx.insert_actor(
		"actor-replay".to_string(),
		1,
		actor_tx,
		Arc::new(AsyncCounter::new()),
		"actor-replay".to_string(),
		-1,
	);

	handle_commands(&mut ctx, vec![stop_command("actor-replay", 1, 5)]).await;
	assert!(matches!(
		actor_rx.try_recv(),
		Ok(ToActor::Stop { command_idx: 5, .. })
	));

	// Same index replayed: should be skipped.
	handle_commands(&mut ctx, vec![stop_command("actor-replay", 1, 5)]).await;
	assert!(actor_rx.try_recv().is_err());

	// Lower index from a stale replay: should also be skipped.
	handle_commands(&mut ctx, vec![stop_command("actor-replay", 1, 3)]).await;
	assert!(actor_rx.try_recv().is_err());

	// Higher index is processed.
	handle_commands(&mut ctx, vec![stop_command("actor-replay", 1, 7)]).await;
	assert!(matches!(
		actor_rx.try_recv(),
		Ok(ToActor::Stop { command_idx: 7, .. })
	));
}

#[tokio::test]
async fn dedup_is_per_actor_and_generation() {
	let mut ctx = new_envoy_context();
	let (tx_a1, mut rx_a1) = mpsc::unbounded_channel::<ToActor>();
	let (tx_a2, mut rx_a2) = mpsc::unbounded_channel::<ToActor>();
	let (tx_b1, mut rx_b1) = mpsc::unbounded_channel::<ToActor>();
	ctx.insert_actor(
		"actor-a".to_string(),
		1,
		tx_a1,
		Arc::new(AsyncCounter::new()),
		"actor-a".to_string(),
		-1,
	);
	ctx.insert_actor(
		"actor-a".to_string(),
		2,
		tx_a2,
		Arc::new(AsyncCounter::new()),
		"actor-a".to_string(),
		-1,
	);
	ctx.insert_actor(
		"actor-b".to_string(),
		1,
		tx_b1,
		Arc::new(AsyncCounter::new()),
		"actor-b".to_string(),
		-1,
	);

	handle_commands(&mut ctx, vec![stop_command("actor-a", 1, 5)]).await;
	assert!(rx_a1.try_recv().is_ok());

	// Same actor_id, different generation: not deduped.
	handle_commands(&mut ctx, vec![stop_command("actor-a", 2, 5)]).await;
	assert!(rx_a2.try_recv().is_ok());

	// Different actor_id, same index: not deduped.
	handle_commands(&mut ctx, vec![stop_command("actor-b", 1, 5)]).await;
	assert!(rx_b1.try_recv().is_ok());
}

#[tokio::test]
async fn replayed_command_is_dropped_after_remote_sql_lost_response() {
	let mut ctx = new_envoy_context();
	let (actor_tx, mut actor_rx) = mpsc::unbounded_channel::<ToActor>();
	ctx.insert_actor(
		"actor-replay".to_string(),
		1,
		actor_tx,
		Arc::new(AsyncCounter::new()),
		"actor-replay".to_string(),
		-1,
	);

	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);
	let (sql_tx, sql_rx) = tokio::sync::oneshot::channel();
	handle_remote_sqlite_request(
		&mut ctx,
		RemoteSqliteRequest::Execute(execute_request()),
		None,
		sql_tx,
	)
	.await;
	assert!(matches!(ws_rx.recv().await, Some(WsTxMessage::Send(_))));

	handle_commands(&mut ctx, vec![stop_command("actor-replay", 1, 5)]).await;
	assert!(matches!(
		actor_rx.try_recv(),
		Ok(ToActor::Stop { command_idx: 5, .. })
	));

	fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx);
	let err = sql_rx
		.await
		.expect("response sender should complete")
		.expect_err("sent remote SQL should become indeterminate");
	assert!(
		err.downcast_ref::<RemoteSqliteIndeterminateResultError>()
			.is_some()
	);

	handle_commands(&mut ctx, vec![stop_command("actor-replay", 1, 5)]).await;
	assert!(actor_rx.try_recv().is_err());
}

fn decode_ack_checkpoints(msg: WsTxMessage) -> Vec<protocol::ActorCheckpoint> {
	let WsTxMessage::Send(bytes) = msg else {
		panic!("expected a websocket send, got a close");
	};
	let message = protocol::versioned::ToRivet::deserialize(&bytes, protocol::PROTOCOL_VERSION)
		.expect("failed to decode ToRivet message");
	match message {
		protocol::ToRivet::ToRivetAckCommands(val) => val.last_command_checkpoints,
		_ => panic!("expected ToRivetAckCommands"),
	}
}

#[tokio::test]
async fn stop_command_is_acked_immediately() {
	let mut ctx = new_envoy_context();
	let (actor_tx, mut actor_rx) = mpsc::unbounded_channel::<ToActor>();
	ctx.insert_actor(
		"actor-a".to_string(),
		1,
		actor_tx,
		Arc::new(AsyncCounter::new()),
		"actor-a".to_string(),
		-1,
	);

	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);

	handle_commands(&mut ctx, vec![stop_command("actor-a", 1, 5)]).await;
	assert!(matches!(
		actor_rx.try_recv(),
		Ok(ToActor::Stop { command_idx: 5, .. })
	));

	// The stop must be acked right away rather than waiting for the periodic
	// tick, otherwise the actor entry is gone before the next ack.
	let checkpoints =
		decode_ack_checkpoints(ws_rx.try_recv().expect("stop should trigger an immediate ack"));
	assert_eq!(checkpoints.len(), 1);
	assert_eq!(checkpoints[0].actor_id, "actor-a");
	assert_eq!(checkpoints[0].generation, 1);
	assert_eq!(checkpoints[0].index, 5);

	// A successful immediate ack must still retain the dedup entry. Only the
	// periodic tick clears it, so a replay can re-ack if the server never
	// committed this ack.
	assert_eq!(
		ctx.processed_command_idx.get(&("actor-a".to_string(), 1)),
		Some(&5)
	);
}

#[tokio::test]
async fn stop_ack_retried_via_replay_after_failed_send() {
	let mut ctx = new_envoy_context();
	let (actor_tx, mut actor_rx) = mpsc::unbounded_channel::<ToActor>();
	ctx.insert_actor(
		"actor-a".to_string(),
		1,
		actor_tx,
		Arc::new(AsyncCounter::new()),
		"actor-a".to_string(),
		-1,
	);

	// No websocket is connected, so the immediate ack send fails. The processed
	// index must be retained so a later replay can re-ack it.
	handle_commands(&mut ctx, vec![stop_command("actor-a", 1, 5)]).await;
	assert!(matches!(
		actor_rx.try_recv(),
		Ok(ToActor::Stop { command_idx: 5, .. })
	));
	assert_eq!(
		ctx.processed_command_idx.get(&("actor-a".to_string(), 1)),
		Some(&5)
	);

	// Remove the actor, as happens once it emits its Stopped event. The re-ack
	// must still work with no live actor, sourced from the dedup map.
	ctx.remove_actor("actor-a", 1);

	// Reconnect and replay the same stop. Dedup skips reprocessing, but the batch
	// still carries a stop, so the retained checkpoint is re-acked.
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);
	handle_commands(&mut ctx, vec![stop_command("actor-a", 1, 5)]).await;
	assert!(
		actor_rx.try_recv().is_err(),
		"replayed stop must not be reprocessed"
	);

	let checkpoints =
		decode_ack_checkpoints(ws_rx.try_recv().expect("replayed stop should re-ack"));
	assert_eq!(checkpoints.len(), 1);
	assert_eq!(checkpoints[0].index, 5);
}

#[tokio::test]
async fn unknown_actor_stop_is_acked() {
	let mut ctx = new_envoy_context();
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);

	// No actor inserted: models a stop replayed to a process that never started
	// it (e.g. after restart). It must still be acked to stop the replay.
	handle_commands(&mut ctx, vec![stop_command("actor-gone", 3, 9)]).await;

	let checkpoints = decode_ack_checkpoints(
		ws_rx
			.try_recv()
			.expect("unknown-actor stop should still ack"),
	);
	assert_eq!(checkpoints.len(), 1);
	assert_eq!(checkpoints[0].actor_id, "actor-gone");
	assert_eq!(checkpoints[0].generation, 3);
	assert_eq!(checkpoints[0].index, 9);
}

#[tokio::test]
async fn live_actor_is_reacked_on_each_tick() {
	let mut ctx = new_envoy_context();
	let (actor_tx, _actor_rx) = mpsc::unbounded_channel::<ToActor>();
	// A live actor whose latest command index is 3.
	ctx.insert_actor(
		"actor-a".to_string(),
		1,
		actor_tx,
		Arc::new(AsyncCounter::new()),
		"actor-a".to_string(),
		3,
	);

	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);

	// The first tick acks index 3 and clears the dedup map. The second tick must
	// still re-ack from the live-actor scan, recovering an ack the server may
	// never have committed.
	send_command_ack(&mut ctx).await;
	let first = decode_ack_checkpoints(ws_rx.try_recv().expect("first tick should ack"));
	assert_eq!(first.len(), 1);
	assert_eq!(first[0].index, 3);

	send_command_ack(&mut ctx).await;
	let second = decode_ack_checkpoints(ws_rx.try_recv().expect("second tick should re-ack"));
	assert_eq!(second.len(), 1);
	assert_eq!(second[0].actor_id, "actor-a");
	assert_eq!(second[0].index, 3);
}
