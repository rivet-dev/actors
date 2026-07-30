use std::collections::HashMap;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;

use crate::async_counter::AsyncCounter;
use rivet_envoy_protocol as protocol;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::Instrument;

use crate::actor::ToActor;
use crate::commands::{ACK_COMMANDS_INTERVAL_MS, handle_commands, send_command_ack};
use crate::config::EnvoyConfig;
use crate::connection::{start_connection, ws_send};
use crate::context::{SharedContext, WsTxMessage};
use crate::events::{handle_ack_events, handle_send_events, resend_unacknowledged_events};
use crate::handle::EnvoyHandle;
use crate::kv::{
	KV_CLEANUP_INTERVAL_MS, KvRequestEntry, cleanup_old_kv_requests, handle_kv_request,
	handle_kv_response, process_unsent_kv_requests,
};
use crate::metrics::METRICS;
use crate::sqlite::{
	RemoteSqliteRequest, RemoteSqliteRequestEntry, RemoteSqliteResponseEnvelope, SqliteRequest,
	SqliteRequestEntry, SqliteResponse, cleanup_old_remote_sqlite_requests,
	cleanup_old_sqlite_requests, fail_remote_sqlite_requests_with_shutdown,
	fail_sent_remote_sqlite_requests_with_indeterminate_result, fail_sqlite_requests_with_shutdown,
	handle_remote_sqlite_exec_response, handle_remote_sqlite_execute_batch_response,
	handle_remote_sqlite_execute_response, handle_remote_sqlite_request,
	handle_sqlite_commit_response, handle_sqlite_get_pages_response, handle_sqlite_request,
	process_unsent_remote_sqlite_requests, process_unsent_sqlite_requests,
};
use crate::tunnel::{
	handle_tunnel_message, resend_buffered_tunnel_messages, send_hibernatable_ws_message_ack,
};
use crate::utils::{BufferMap, EnvoyShutdownError, SleepFuture, boxed_sleep, spawn_detached};

/// Process-wide envoy slot. Holds the handle inside a mutex so a stopped
/// handle (e.g. from a shutdown-during-build race in serverless mode) can be
/// replaced on the next `start_envoy_sync` call.
#[cfg(not(target_arch = "wasm32"))]
static GLOBAL_ENVOY: OnceLock<Mutex<Option<EnvoyHandle>>> = OnceLock::new();

pub struct EnvoyContext {
	pub shared: Arc<SharedContext>,
	pub shutting_down: bool,
	pub actors: HashMap<String, HashMap<u32, ActorEntry>>,
	pub buffered_actor_messages: HashMap<String, Vec<BufferedActorMessage>>,
	pub kv_requests: HashMap<u32, KvRequestEntry>,
	pub next_kv_request_id: u32,
	pub sqlite_requests: HashMap<u32, SqliteRequestEntry>,
	pub next_sqlite_request_id: u32,
	pub remote_sqlite_requests: HashMap<u32, RemoteSqliteRequestEntry>,
	pub next_remote_sqlite_request_id: u32,
	pub request_to_actor: BufferMap<String>,
	pub buffered_messages: Vec<protocol::ToRivetTunnelMessage>,
	/// Highest command index processed per `(actor_id, generation)`, used to
	/// drop replayed commands from `pegboard-envoy` after a reconnect. Persists
	/// across `remove_actor` so a replayed `CommandStartActor` for an
	/// already-stopped actor cannot resurrect it.
	pub processed_command_idx: HashMap<(String, u32), i64>,
}

pub struct ActorEntry {
	pub handle: mpsc::UnboundedSender<ToActor>,
	pub active_http_request_count: Arc<AsyncCounter>,
	pub name: String,
	pub event_history: Vec<protocol::EventWrapper>,
	pub last_command_idx: i64,
	pub received_stop: bool,
}

pub enum BufferedActorMessage {
	WsMsg {
		message_id: protocol::MessageId,
		msg: protocol::ToEnvoyWebSocketMessage,
	},
	WsClose {
		message_id: protocol::MessageId,
		close: protocol::ToEnvoyWebSocketClose,
	},
}

pub enum ToEnvoyMessage {
	ConnMessage {
		message: protocol::ToEnvoy,
	},
	ConnClose {
		evict: bool,
	},
	SendEvents {
		events: Vec<protocol::EventWrapper>,
	},
	KvRequest {
		actor_id: String,
		data: protocol::KvRequestData,
		response_tx: oneshot::Sender<anyhow::Result<protocol::KvResponseData>>,
	},
	SqliteRequest {
		request: SqliteRequest,
		response_tx: oneshot::Sender<anyhow::Result<SqliteResponse>>,
	},
	RemoteSqliteRequest {
		request: RemoteSqliteRequest,
		expected_session: Option<u64>,
		response_tx: oneshot::Sender<anyhow::Result<RemoteSqliteResponseEnvelope>>,
	},
	BufferTunnelMsg {
		msg: protocol::ToRivetTunnelMessage,
	},
	ActorIntent {
		actor_id: String,
		generation: Option<u32>,
		intent: protocol::ActorIntent,
		error: Option<String>,
	},
	SetAlarm {
		actor_id: String,
		generation: Option<u32>,
		alarm_ts: Option<i64>,
		ack_tx: Option<oneshot::Sender<()>>,
	},
	HwsAck {
		gateway_id: protocol::GatewayId,
		request_id: protocol::RequestId,
		envoy_message_index: u16,
	},
	HttpRequestComplete {
		gateway_id: protocol::GatewayId,
		request_id: protocol::RequestId,
	},
	GetActor {
		actor_id: String,
		generation: Option<u32>,
		response_tx: oneshot::Sender<Option<ActorInfo>>,
	},
	Shutdown,
	Stop,
}

/// Information about an actor, returned by `EnvoyHandle::get_actor`.
#[derive(Clone)]
pub struct ActorInfo {
	pub name: String,
	pub generation: u32,
	pub active_http_request_count: Arc<AsyncCounter>,
}

impl EnvoyContext {
	pub fn insert_actor(
		&mut self,
		actor_id: String,
		generation: u32,
		handle: mpsc::UnboundedSender<ToActor>,
		active_http_request_count: Arc<AsyncCounter>,
		name: String,
		last_command_idx: i64,
	) {
		let buffered_actor_id = actor_id.clone();
		let buffered_handle = handle.clone();
		self.actors
			.entry(actor_id.clone())
			.or_insert_with(HashMap::new)
			.insert(
				generation,
				ActorEntry {
					handle: handle.clone(),
					active_http_request_count: active_http_request_count.clone(),
					name,
					event_history: Vec::new(),
					last_command_idx,
					received_stop: false,
				},
			);
		self.shared
			.actors
			.lock()
			.expect("shared actor registry poisoned")
			.entry(actor_id)
			.or_insert_with(HashMap::new)
			.insert(
				generation,
				crate::context::SharedActorEntry {
					handle,
					active_http_request_count,
				},
			);

		self.shared.actors_notify.notify_waiters();

		if let Some(messages) = self.buffered_actor_messages.remove(&buffered_actor_id) {
			for message in messages {
				match message {
					BufferedActorMessage::WsMsg { message_id, msg } => {
						let _ = buffered_handle.send(ToActor::WsMsg { message_id, msg });
					}
					BufferedActorMessage::WsClose { message_id, close } => {
						let _ = buffered_handle.send(ToActor::WsClose { message_id, close });
					}
				}
			}
		}
	}

	pub fn remove_actor(&mut self, actor_id: &str, generation: u32) {
		if let Some(generations) = self.actors.get_mut(actor_id) {
			generations.remove(&generation);
			if generations.is_empty() {
				self.actors.remove(actor_id);
			}
		}

		let mut shared = self
			.shared
			.actors
			.lock()
			.expect("shared actor registry poisoned");
		if let Some(generations) = shared.get_mut(actor_id) {
			generations.remove(&generation);
			if generations.is_empty() {
				shared.remove(actor_id);
			}
		}
		self.shared.actors_notify.notify_waiters();
	}

	pub fn get_actor(&self, actor_id: &str, generation: Option<u32>) -> Option<&ActorEntry> {
		let gens = self.actors.get(actor_id)?;
		if gens.is_empty() {
			return None;
		}

		if let Some(g) = generation {
			return gens.get(&g);
		}

		// Return highest generation non-closed entry
		// HashMap doesn't guarantee order, so find max key
		let mut best: Option<&ActorEntry> = None;
		let mut best_gen: u32 = 0;
		for (&g, entry) in gens {
			if !entry.handle.is_closed() && (best.is_none() || g > best_gen) {
				best = Some(entry);
				best_gen = g;
			}
		}
		best
	}

	pub fn get_actor_entry_mut(
		&mut self,
		actor_id: &str,
		generation: u32,
	) -> Option<&mut ActorEntry> {
		self.actors
			.get_mut(actor_id)
			.and_then(|gens| gens.get_mut(&generation))
	}
}

pub async fn start_envoy(config: EnvoyConfig) -> EnvoyHandle {
	let handle = start_envoy_sync(config);
	handle
		.started()
		.await
		.expect("envoy failed to start before returning handle");
	handle
}

pub fn start_envoy_sync(config: EnvoyConfig) -> EnvoyHandle {
	#[cfg(target_arch = "wasm32")]
	{
		start_envoy_sync_inner(config)
	}

	#[cfg(not(target_arch = "wasm32"))]
	{
		if config.not_global {
			return start_envoy_sync_inner(config);
		}

		let slot = GLOBAL_ENVOY.get_or_init(|| Mutex::new(None));
		let mut guard = slot.lock();
		if let Some(handle) = guard.as_ref() {
			if !handle.is_stopped() {
				return handle.clone();
			}
		}
		let handle = start_envoy_sync_inner(config);
		*guard = Some(handle.clone());
		handle
	}
}

fn start_envoy_sync_inner(config: EnvoyConfig) -> EnvoyHandle {
	let (envoy_tx, envoy_rx) = mpsc::unbounded_channel::<ToEnvoyMessage>();
	let (start_tx, start_rx) = tokio::sync::watch::channel(());
	let (stopped_tx, _stopped_rx) = tokio::sync::watch::channel(false);
	let (connection_session_tx, _connection_session_rx) = tokio::sync::watch::channel(0);

	let envoy_key = uuid::Uuid::new_v4().to_string();
	let shared = Arc::new(SharedContext {
		config,
		envoy_key,
		envoy_tx: envoy_tx.clone(),
		actors: Arc::new(std::sync::Mutex::new(HashMap::new())),
		actors_notify: Arc::new(tokio::sync::Notify::new()),
		live_tunnel_requests: Arc::new(std::sync::Mutex::new(HashMap::new())),
		pending_hibernation_restores: Arc::new(std::sync::Mutex::new(HashMap::new())),
		ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
		connection_session: std::sync::atomic::AtomicU64::new(0),
		next_connection_session: std::sync::atomic::AtomicU64::new(0),
		connection_session_tx,
		protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
		shutting_down: std::sync::atomic::AtomicBool::new(false),
		last_ping_ts: std::sync::atomic::AtomicI64::new(0),
		stopped_tx,
	});

	let handle = EnvoyHandle {
		shared: shared.clone(),
		started_rx: start_rx,
	};

	start_connection(shared.clone());

	let ctx = EnvoyContext {
		shared: shared.clone(),
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
		buffered_messages: Vec::new(),
		processed_command_idx: HashMap::new(),
	};

	tracing::info!(envoy_key = %shared.envoy_key, "starting envoy");
	let span = tracing::info_span!("envoy_client", envoy_key = %shared.envoy_key);
	spawn_detached(envoy_loop(ctx, envoy_rx, start_tx).instrument(span));

	handle
}

async fn envoy_loop(
	mut ctx: EnvoyContext,
	mut rx: mpsc::UnboundedReceiver<ToEnvoyMessage>,
	start_tx: tokio::sync::watch::Sender<()>,
) {
	let mut ack_tick = boxed_sleep(std::time::Duration::from_millis(ACK_COMMANDS_INTERVAL_MS));
	let mut kv_cleanup_tick = boxed_sleep(std::time::Duration::from_millis(KV_CLEANUP_INTERVAL_MS));

	let mut lost_timeout: Option<SleepFuture> = None;

	loop {
		let iter_start = crate::time::Instant::now();
		#[allow(unused_assignments)]
		let mut branch: &'static str = "unknown";
		tokio::select! {
			msg = rx.recv() => {
				branch = "envoy_msg";
				let Some(msg) = msg else {
					observe_envoy_loop_iteration(branch, iter_start);
					break;
				};
				METRICS.envoy_tx_depth.dec();

				match msg {
					ToEnvoyMessage::ConnMessage { message } => {
						lost_timeout = handle_conn_message(&mut ctx, &start_tx, lost_timeout, message).await;
					}
					ToEnvoyMessage::ConnClose { evict } => {
						fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx);
						lost_timeout = handle_conn_close(&ctx, lost_timeout);
						if evict {
							observe_envoy_loop_iteration(branch, iter_start);
							break;
						}
					}
					ToEnvoyMessage::SendEvents { events } => {
						handle_send_events(&mut ctx, events).await;
					}
					ToEnvoyMessage::KvRequest { actor_id, data, response_tx } => {
						handle_kv_request(&mut ctx, actor_id, data, response_tx).await;
					}
					ToEnvoyMessage::SqliteRequest { request, response_tx } => {
						handle_sqlite_request(&mut ctx, request, response_tx).await;
					}
					ToEnvoyMessage::RemoteSqliteRequest { request, expected_session, response_tx } => {
						handle_remote_sqlite_request(&mut ctx, request, expected_session, response_tx).await;
					}
					ToEnvoyMessage::BufferTunnelMsg { msg } => {
						ctx.buffered_messages.push(msg);
					}
					ToEnvoyMessage::ActorIntent { actor_id, generation, intent, error } => {
						if let Some(entry) = ctx.get_actor(&actor_id, generation) {
							let _ = entry.handle.send(ToActor::Intent { intent, error });
						}
					}
					ToEnvoyMessage::SetAlarm { actor_id, generation, alarm_ts, ack_tx } => {
						if let Some(entry) = ctx.get_actor(&actor_id, generation) {
							if let Err(error) = entry.handle.send(ToActor::SetAlarm { alarm_ts, ack_tx }) {
								if let ToActor::SetAlarm { ack_tx: Some(ack_tx), .. } = error.0 {
									let _ = ack_tx.send(());
								}
							}
						} else if let Some(ack_tx) = ack_tx {
							let _ = ack_tx.send(());
						}
					}
					ToEnvoyMessage::HwsAck { gateway_id, request_id, envoy_message_index } => {
						send_hibernatable_ws_message_ack(&mut ctx, gateway_id, request_id, envoy_message_index);
					}
					ToEnvoyMessage::HttpRequestComplete { gateway_id, request_id } => {
						ctx.request_to_actor.remove(&[&gateway_id, &request_id]);
					}
					ToEnvoyMessage::GetActor { actor_id, generation, response_tx } => {
						let info = ctx.get_actor(&actor_id, generation).map(|entry| {
							let actor_gen = generation.unwrap_or_else(|| {
								ctx.actors
									.get(&actor_id)
									.and_then(|gens| {
										gens.iter()
											.filter(|(_, e)| !e.handle.is_closed())
											.map(|(&g, _)| g)
											.max()
									})
									.unwrap_or(0)
							});
							ActorInfo {
								name: entry.name.clone(),
								generation: actor_gen,
								active_http_request_count: entry
									.active_http_request_count
									.clone(),
							}
						});
						let _ = response_tx.send(info);
					}
					ToEnvoyMessage::Shutdown => {
						handle_shutdown(&mut ctx).await;
					}
					ToEnvoyMessage::Stop => {
						observe_envoy_loop_iteration(branch, iter_start);
						break;
					}
				}
			}
			_ = ack_tick.as_mut() => {
				branch = "ack_tick";
				send_command_ack(&mut ctx).await;
				ack_tick = boxed_sleep(std::time::Duration::from_millis(ACK_COMMANDS_INTERVAL_MS));
			}
			_ = kv_cleanup_tick.as_mut() => {
				branch = "cleanup_tick";
				cleanup_old_kv_requests(&mut ctx);
				cleanup_old_sqlite_requests(&mut ctx);
				cleanup_old_remote_sqlite_requests(&mut ctx);
				kv_cleanup_tick = boxed_sleep(std::time::Duration::from_millis(KV_CLEANUP_INTERVAL_MS));
			}
			_ = async {
				match lost_timeout.as_mut() {
					Some(timeout) => timeout.as_mut().await,
					None => std::future::pending::<()>().await,
				}
			} => {
				branch = "lost_timeout";
				// Lost timeout fired
				for (_id, request) in ctx.kv_requests.drain() {
					METRICS.kv_requests_inflight.dec();
					let _ = request.response_tx.send(Err(anyhow::anyhow!(EnvoyShutdownError)));
				}
				fail_sqlite_requests_with_shutdown(&mut ctx);
				fail_remote_sqlite_requests_with_shutdown(&mut ctx);

				if !ctx.actors.is_empty() {
					tracing::warn!("stopping all actors due to envoy lost threshold");
					for (_actor_id, gens) in &ctx.actors {
						for (_g, entry) in gens {
							if !entry.handle.is_closed() {
								let _ = entry.handle.send(ToActor::Lost);
							}
						}
					}
					ctx.actors.clear();
					ctx.shared
						.actors
						.lock()
						.expect("shared actor registry poisoned")
						.clear();
				}

				lost_timeout = None;
			}
		}
		observe_envoy_loop_iteration(branch, iter_start);
	}

	// Cleanup
	{
		let guard = ctx.shared.ws_tx.lock().await;
		if let Some(tx) = guard.as_ref() {
			let _ = tx.send(WsTxMessage::Close);
		}
	}

	for (_id, request) in ctx.kv_requests.drain() {
		METRICS.kv_requests_inflight.dec();
		let _ = request
			.response_tx
			.send(Err(anyhow::anyhow!("envoy shutting down")));
	}
	fail_sqlite_requests_with_shutdown(&mut ctx);
	fail_remote_sqlite_requests_with_shutdown(&mut ctx);

	ctx.actors.clear();
	ctx.shared
		.actors
		.lock()
		.expect("shared actor registry poisoned")
		.clear();

	tracing::info!("envoy stopped");

	ctx.shared.config.callbacks.on_shutdown();

	// Latched signal: waiters on `EnvoyHandle::wait_stopped` observe this and
	// any future callers of `wait_stopped` resolve immediately because watch
	// retains the last value.
	let _ = ctx.shared.stopped_tx.send(true);
}

fn observe_envoy_loop_iteration(branch: &'static str, start: crate::time::Instant) {
	let elapsed = start.elapsed();
	METRICS
		.envoy_loop_iteration_duration_seconds
		.with_label_values(&[branch])
		.observe(elapsed.as_secs_f64());
}

/// Send a message into the envoy_loop's mpsc and bump the depth gauge.
/// Producers should prefer this over calling `shared.envoy_tx.send` directly
/// so the `envoy_tx_depth` gauge stays in sync.
pub fn send_to_envoy_tx(
	shared: &crate::context::SharedContext,
	msg: ToEnvoyMessage,
) -> Result<(), tokio::sync::mpsc::error::SendError<ToEnvoyMessage>> {
	match shared.envoy_tx.send(msg) {
		Ok(()) => {
			METRICS.envoy_tx_depth.inc();
			Ok(())
		}
		Err(e) => Err(e),
	}
}

async fn handle_conn_message(
	ctx: &mut EnvoyContext,
	start_tx: &tokio::sync::watch::Sender<()>,
	mut lost_timeout: Option<SleepFuture>,
	message: protocol::ToEnvoy,
) -> Option<SleepFuture> {
	match message {
		protocol::ToEnvoy::ToEnvoyInit(init) => {
			{
				let mut guard = ctx.shared.protocol_metadata.lock().await;
				*guard = Some(init.metadata.clone());
			}
			tracing::info!(?init.metadata, "received init");

			lost_timeout = None;
			resend_unacknowledged_events(ctx).await;
			process_unsent_kv_requests(ctx).await;
			process_unsent_sqlite_requests(ctx).await;
			process_unsent_remote_sqlite_requests(ctx).await;
			resend_buffered_tunnel_messages(ctx).await;

			let _ = start_tx.send(());
		}
		protocol::ToEnvoy::ToEnvoyCommands(commands) => {
			handle_commands(ctx, commands).await;
		}
		protocol::ToEnvoy::ToEnvoyAckEvents(ack) => {
			handle_ack_events(ctx, ack);
		}
		protocol::ToEnvoy::ToEnvoyKvResponse(response) => {
			handle_kv_response(ctx, response).await;
		}
		protocol::ToEnvoy::ToEnvoySqliteGetPagesResponse(response) => {
			handle_sqlite_get_pages_response(ctx, response).await;
		}
		protocol::ToEnvoy::ToEnvoySqliteCommitResponse(response) => {
			handle_sqlite_commit_response(ctx, response).await;
		}
		protocol::ToEnvoy::ToEnvoySqliteExecResponse(response) => {
			handle_remote_sqlite_exec_response(ctx, response).await;
		}
		protocol::ToEnvoy::ToEnvoySqliteExecuteResponse(response) => {
			handle_remote_sqlite_execute_response(ctx, response).await;
		}
		protocol::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(response) => {
			handle_remote_sqlite_execute_batch_response(ctx, response).await;
		}
		protocol::ToEnvoy::ToEnvoyTunnelMessage(tunnel_msg) => {
			handle_tunnel_message(ctx, tunnel_msg).await;
		}
		protocol::ToEnvoy::ToEnvoyPing(_) => {
			// Should be handled by connection task
		}
	}

	lost_timeout
}

fn handle_conn_close(ctx: &EnvoyContext, lost_timeout: Option<SleepFuture>) -> Option<SleepFuture> {
	if lost_timeout.is_some() {
		return lost_timeout;
	}

	// Read threshold from protocol metadata, fall back to 10 seconds
	let lost_threshold = {
		let metadata = ctx.shared.protocol_metadata.try_lock().ok();
		metadata
			.and_then(|guard| guard.as_ref().map(|m| m.envoy_lost_threshold as u64))
			.unwrap_or(10_000)
	};

	tracing::debug!(ms = lost_threshold, "starting envoy lost timeout");

	Some(boxed_sleep(std::time::Duration::from_millis(
		lost_threshold,
	)))
}

async fn handle_shutdown(ctx: &mut EnvoyContext) {
	if ctx.shutting_down {
		return;
	}
	ctx.shutting_down = true;
	ctx.shared.shutting_down.store(true, Ordering::Release);

	tracing::debug!("envoy received shutdown");

	ws_send(&ctx.shared, protocol::ToRivet::ToRivetStopping).await;

	// Wait for all actors to finish. The process manager (Docker,
	// k8s, etc.) provides the ultimate shutdown deadline.
	let actor_handles: Vec<mpsc::UnboundedSender<ToActor>> = ctx
		.actors
		.values()
		.flat_map(|gens| gens.values())
		.filter(|entry| !entry.handle.is_closed())
		.map(|entry| entry.handle.clone())
		.collect();

	let shared = ctx.shared.clone();
	let shutdown_span = tracing::debug_span!(
		parent: tracing::Span::current(),
		"envoy_graceful_shutdown",
		envoy_key = %ctx.shared.envoy_key,
	);
	spawn_detached(
		async move {
			futures_util::future::join_all(actor_handles.iter().map(|h| h.closed())).await;
			tracing::debug!("all actors stopped during graceful shutdown");
			let _ = send_to_envoy_tx(&shared, ToEnvoyMessage::Stop);
		}
		.instrument(shutdown_span),
	);
}
