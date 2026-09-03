use rivet_envoy_protocol as protocol;

use crate::connection::ws_send;
use crate::envoy::EnvoyContext;
use crate::stringify::stringify_event_wrapper;

pub async fn handle_send_events(ctx: &mut EnvoyContext, events: Vec<protocol::EventWrapper>) {
	tracing::info!(event_count = events.len(), "sending events");
	for event in &events {
		tracing::info!(event = %stringify_event_wrapper(event), "sending event");
	}

	// Record in history per actor
	for event in &events {
		let mut remove_after_stop = false;
		let entry =
			ctx.get_actor_entry_mut(&event.checkpoint.actor_id, event.checkpoint.generation);
		if let Some(entry) = entry {
			entry.event_history.push(event.clone());

			if let protocol::Event::EventActorStateUpdate(ref state_update) = event.inner {
				if matches!(
					state_update.state,
					protocol::ActorState::ActorStateStopped(_)
				) {
					// If the actor is being stopped by rivet, we don't need the entry anymore
					if entry.received_stop {
						remove_after_stop = true;
					}
				}
			}
		}
		if remove_after_stop {
			ctx.remove_actor(&event.checkpoint.actor_id, event.checkpoint.generation);
		}
	}

	// Send if connected
	ws_send(&ctx.shared, protocol::ToRivet::ToRivetEvents(events)).await;
}

pub fn handle_ack_events(ctx: &mut EnvoyContext, ack: protocol::ToEnvoyAckEvents) {
	for checkpoint in &ack.last_event_checkpoints {
		let entry = ctx.get_actor_entry_mut(&checkpoint.actor_id, checkpoint.generation);
		if let Some(entry) = entry {
			entry
				.event_history
				.retain(|event| event.checkpoint.index > checkpoint.index);
		}
	}
}

// TODO: If the envoy disconnects, actor stops, then envoy reconnects, we will send the stop event but there
// is no mechanism to remove the actor entry afterwards. We only remove the actor entry if rivet stops the actor.
pub async fn resend_unacknowledged_events(ctx: &EnvoyContext) {
	let mut events: Vec<protocol::EventWrapper> = Vec::new();

	for generations in ctx.actors.values() {
		for entry in generations.values() {
			events.extend(entry.event_history.iter().cloned());
		}
	}

	if events.is_empty() {
		return;
	}

	tracing::info!(count = events.len(), "resending unacknowledged events");
	for event in &events {
		tracing::info!(event = %stringify_event_wrapper(event), "resending event");
	}

	ws_send(&ctx.shared, protocol::ToRivet::ToRivetEvents(events)).await;
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::Arc;

	use crate::async_counter::AsyncCounter;
	use rivet_envoy_protocol as protocol;
	use tokio::sync::mpsc;

	use super::handle_send_events;
	use crate::actor::ToActor;
	use crate::config::{
		BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
		WebSocketSender,
	};
	use crate::context::{SharedContext, WsTxMessage};
	use crate::envoy::EnvoyContext;
	use crate::handle::EnvoyHandle;

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
			Box::pin(async { anyhow::bail!("fetch should not be called in event tests") })
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
			Box::pin(async { anyhow::bail!("websocket should not be called in event tests") })
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

	fn new_envoy_context() -> (EnvoyContext, EnvoyHandle) {
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
		let handle = EnvoyHandle {
			shared: shared.clone(),
			started_rx: tokio::sync::watch::channel(()).1,
		};
		(
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
				request_to_actor: crate::utils::BufferMap::new(),
				http_request_routes: crate::utils::BufferMap::new(),
				http_message_indices: crate::utils::BufferMap::new(),
				http_request_cancellations: HashMap::new(),
				buffered_messages: Vec::new(),
				processed_command_idx: HashMap::new(),
			},
			handle,
		)
	}

	fn insert_actor(
		ctx: &mut EnvoyContext,
		actor_id: &str,
		generation: u32,
		counter: Arc<AsyncCounter>,
		received_stop: bool,
	) {
		let handle = mpsc::unbounded_channel::<ToActor>().0;
		ctx.insert_actor(
			actor_id.to_string(),
			generation,
			handle.clone(),
			counter.clone(),
			format!("{actor_id}-{generation}"),
			0,
		);
		ctx.actors
			.get_mut(actor_id)
			.and_then(|generations| generations.get_mut(&generation))
			.expect("actor should be inserted")
			.received_stop = received_stop;
	}

	fn stopped_event(actor_id: &str, generation: u32) -> protocol::EventWrapper {
		protocol::EventWrapper {
			checkpoint: protocol::ActorCheckpoint {
				actor_id: actor_id.to_string(),
				generation,
				index: 0,
			},
			inner: protocol::Event::EventActorStateUpdate(protocol::EventActorStateUpdate {
				state: protocol::ActorState::ActorStateStopped(protocol::ActorStateStopped {
					code: protocol::StopCode::Ok,
					message: None,
				}),
			}),
		}
	}

	#[tokio::test]
	async fn stop_event_removes_actor_from_primary_and_shared_registries() {
		let (mut ctx, handle) = new_envoy_context();
		let counter = Arc::new(AsyncCounter::new());
		insert_actor(&mut ctx, "actor-stop", 1, counter.clone(), true);

		assert!(handle.http_request_counter("actor-stop", Some(1)).is_some());

		handle_send_events(&mut ctx, vec![stopped_event("actor-stop", 1)]).await;

		assert!(ctx.actors.get("actor-stop").is_none());
		assert!(
			ctx.shared
				.actors
				.lock()
				.expect("shared actor registry poisoned")
				.get("actor-stop")
				.is_none()
		);
		assert!(handle.http_request_counter("actor-stop", Some(1)).is_none());
	}

	#[tokio::test]
	async fn stop_event_only_removes_the_stopped_generation() {
		let (mut ctx, handle) = new_envoy_context();
		let stopped_counter = Arc::new(AsyncCounter::new());
		let live_counter = Arc::new(AsyncCounter::new());
		insert_actor(&mut ctx, "actor-shared", 1, stopped_counter, true);
		insert_actor(&mut ctx, "actor-shared", 2, live_counter.clone(), false);

		handle_send_events(&mut ctx, vec![stopped_event("actor-shared", 1)]).await;

		assert!(
			handle
				.http_request_counter("actor-shared", Some(1))
				.is_none()
		);
		let remaining = handle
			.http_request_counter("actor-shared", Some(2))
			.expect("other generation should remain visible");
		assert!(Arc::ptr_eq(&remaining, &live_counter));
		assert!(
			ctx.actors
				.get("actor-shared")
				.expect("actor id should remain")
				.contains_key(&2)
		);
		assert!(
			!ctx.actors
				.get("actor-shared")
				.expect("actor id should remain")
				.contains_key(&1)
		);
	}
}
