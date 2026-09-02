//! Tests for `EnvoyHandle::wait_actors_drained`, used by the container-runner to flush
//! an in-flight actor stop before the envoy announces it is going away.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rivet_envoy_client::actor::ToActor;
use rivet_envoy_client::async_counter::AsyncCounter;
use rivet_envoy_client::config::{
	BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
	WebSocketSender,
};
use rivet_envoy_client::context::{SharedActorEntry, SharedContext, WsTxMessage};
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_protocol as protocol;
use tokio::sync::mpsc;

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
		Box::pin(async { anyhow::bail!("fetch unused") })
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
		Box::pin(async { anyhow::bail!("websocket unused") })
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

/// Build a `SharedContext` with a controllable stopped flag and empty actor registry.
fn build_shared(stopped: bool) -> Arc<SharedContext> {
	let (envoy_tx, _envoy_rx) = mpsc::unbounded_channel();
	Arc::new(SharedContext {
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
		stopped_tx: tokio::sync::watch::channel(stopped).0,
	})
}

/// Register a fake actor. The returned receiver must be kept alive so the actor handle
/// stays open and counts toward `active_actor_count`.
fn insert_actor(shared: &Arc<SharedContext>, id: &str, generation: u32) -> mpsc::UnboundedReceiver<ToActor> {
	let (tx, rx) = mpsc::unbounded_channel::<ToActor>();
	shared
		.actors
		.lock()
		.unwrap()
		.entry(id.to_string())
		.or_default()
		.insert(
			generation,
			SharedActorEntry {
				handle: tx,
				active_http_request_count: Arc::new(AsyncCounter::new()),
			},
		);
	rx
}

/// Deregister an actor and ping the notify, mirroring `remove_actor`.
fn remove_actor(shared: &Arc<SharedContext>, id: &str) {
	shared.actors.lock().unwrap().remove(id);
	shared.actors_notify.notify_waiters();
}

#[tokio::test]
async fn returns_immediately_when_no_actors() {
	let shared = build_shared(false);
	let handle = EnvoyHandle::from_shared(shared);
	tokio::time::timeout(Duration::from_secs(1), handle.wait_actors_drained())
		.await
		.expect("should resolve immediately when there are no actors");
}

#[tokio::test]
async fn returns_immediately_when_already_stopped() {
	let shared = build_shared(true);
	let _rx = insert_actor(&shared, "a", 0); // active actor, but envoy is stopped
	let handle = EnvoyHandle::from_shared(shared);
	tokio::time::timeout(Duration::from_secs(1), handle.wait_actors_drained())
		.await
		.expect("should resolve immediately when the envoy has stopped");
}

#[tokio::test]
async fn pending_until_actor_removed() {
	let shared = build_shared(false);
	let _rx = insert_actor(&shared, "a", 0);
	let handle = EnvoyHandle::from_shared(shared.clone());

	let waiter = tokio::spawn(async move { handle.wait_actors_drained().await });
	tokio::time::sleep(Duration::from_millis(150)).await;
	assert!(!waiter.is_finished(), "should stay pending while an actor is active");

	remove_actor(&shared, "a");
	tokio::time::timeout(Duration::from_secs(1), waiter)
		.await
		.expect("should resolve after the actor is removed")
		.expect("waiter task panicked");
}

#[tokio::test]
async fn pending_until_last_of_multiple_actors_removed() {
	let shared = build_shared(false);
	let _rx_a = insert_actor(&shared, "a", 0);
	let _rx_b = insert_actor(&shared, "b", 0);
	let handle = EnvoyHandle::from_shared(shared.clone());

	let waiter = tokio::spawn(async move { handle.wait_actors_drained().await });

	tokio::time::sleep(Duration::from_millis(100)).await;
	remove_actor(&shared, "a");
	tokio::time::sleep(Duration::from_millis(100)).await;
	assert!(!waiter.is_finished(), "should stay pending while one actor remains");

	remove_actor(&shared, "b");
	tokio::time::timeout(Duration::from_secs(1), waiter)
		.await
		.expect("should resolve after the last actor is removed")
		.expect("waiter task panicked");
}

#[tokio::test]
async fn returns_when_envoy_stops_mid_wait() {
	let shared = build_shared(false);
	let _rx = insert_actor(&shared, "a", 0);
	let handle = EnvoyHandle::from_shared(shared.clone());

	let waiter = tokio::spawn(async move { handle.wait_actors_drained().await });
	tokio::time::sleep(Duration::from_millis(150)).await;
	assert!(!waiter.is_finished(), "should stay pending while the actor is active");

	// Envoy stops while the actor is still registered.
	shared.stopped_tx.send(true).unwrap();
	tokio::time::timeout(Duration::from_secs(1), waiter)
		.await
		.expect("should resolve when the envoy stops")
		.expect("waiter task panicked");
}
