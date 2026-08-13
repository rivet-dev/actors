use anyhow::{Context, bail};
use bytes::Bytes;
use depot::{
	conveyer::Db,
	error::{SqliteStorageError, is_head_fence_mismatch},
	workflows::compaction::DeltasAvailable,
};
use depot_client::{
	database::NativeDatabaseHandle,
	types::{BindParam, ColumnValue, ExecuteResult, QueryResult},
};
use depot_client_embedded::open_database_from_embedded_depot;
use futures_util::{FutureExt, TryStreamExt};
use gas::prelude::Id;
use gas::prelude::*;
use hyper_tungstenite::tungstenite::Message;
use pegboard::actor_kv;
use pegboard::pubsub_subjects::GatewayReceiverSubject;
use rivet_data::converted::{ActorNameKeyData, MetadataKeyData};
use rivet_envoy_protocol::{self as protocol, PROTOCOL_VERSION, versioned};
use rivet_guard_core::websocket_handle::WebSocketReceiver;
use rivet_perf::{perf_finish, perf_start};
use std::{
	collections::HashMap as StdHashMap,
	hash::Hash,
	sync::{Arc, atomic::Ordering},
	time::{Duration, Instant},
};
use tokio::{
	sync::{Mutex, MutexGuard, mpsc, watch},
	task::JoinSet,
};
use universaldb::prelude::*;
use universaldb::utils::end_of_key_range;
use universalpubsub::PublishOpts;
use vbare::OwnedVersionedData;

use crate::{
	LifecycleResult,
	actor_event_demuxer::ActorEventDemuxer,
	actor_kv_task, actor_remote_sqlite_task, actor_sqlite_page_task,
	conn::{Conn, RemoteSqliteExecutors},
	control_task, errors, metrics, sqlite_runtime, tunnel_message_task,
};

const MAX_REMOTE_SQL_BIND_BYTES: usize = 128 * 1024;

/// Max number of pages a single `get_pages` request may ask for. Each requested page number is ~4
/// bytes on the wire but forces the engine to fetch and materialize up to a full 4 KiB page inside
/// one UDB transaction, so an uncapped list is a large cost-asymmetry amplifier from an untrusted
/// runner. Sized well above observed production read batches (max ~1024 pages); the commit path has
/// its own lower cap (`MAX_COMMIT_DIRTY_PAGES`) since writes batch smaller than reads.
const MAX_GET_PAGES_PER_REQUEST: usize = 8192;

/// Wall-clock threshold above which a single handle_message invocation is logged as a head-of-line
/// blocking risk. The ws_to_tunnel_task loop is strictly serial per envoy, so any handler that
/// spends longer than this delays every subsequent WS message from the same envoy (including
/// pings, state updates, and other actors' KV ops). Picked above normal UDB-write tail latency
/// but well below `actor_start_threshold` (30s) so we get warned before the engine declares
/// actors lost.
const SLOW_HANDLE_WARN_THRESHOLD: Duration = Duration::from_secs(5);
const SLOW_SQLITE_REQUEST_WARN_THRESHOLD: Duration = Duration::from_secs(1);
pub(super) const TASK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub(super) enum TaskExit {
	Kv(actor_kv_task::Key),
	SqlitePage(actor_sqlite_page_task::Key),
	RemoteSqlite(actor_remote_sqlite_task::Key),
	Tunnel(tunnel_message_task::Key),
	Control,
}

struct TaskManager {
	ctx: StandaloneCtx,
	conn: Arc<Conn>,
	kv_tasks: StdHashMap<actor_kv_task::Key, mpsc::UnboundedSender<actor_kv_task::Message>>,
	sqlite_page_tasks: StdHashMap<
		actor_sqlite_page_task::Key,
		mpsc::UnboundedSender<actor_sqlite_page_task::Message>,
	>,
	remote_sqlite_tasks: StdHashMap<
		actor_remote_sqlite_task::Key,
		mpsc::UnboundedSender<actor_remote_sqlite_task::Message>,
	>,
	tunnel_tasks:
		StdHashMap<tunnel_message_task::Key, mpsc::UnboundedSender<tunnel_message_task::Message>>,
	control_tasks: StdHashMap<(), mpsc::UnboundedSender<control_task::Message>>,
	workers: JoinSet<Result<TaskExit>>,
}

impl TaskManager {
	fn new(ctx: StandaloneCtx, conn: Arc<Conn>) -> Self {
		Self {
			ctx,
			conn,
			kv_tasks: StdHashMap::new(),
			sqlite_page_tasks: StdHashMap::new(),
			remote_sqlite_tasks: StdHashMap::new(),
			tunnel_tasks: StdHashMap::new(),
			control_tasks: StdHashMap::new(),
			workers: JoinSet::new(),
		}
	}

	fn abort_all(&mut self) {
		let tunnel_task_count = self.tunnel_tasks.len();
		let kv_task_count = self.kv_tasks.len();
		let sqlite_page_task_count = self.sqlite_page_tasks.len();
		let remote_sqlite_task_count = self.remote_sqlite_tasks.len();
		self.kv_tasks.clear();
		self.sqlite_page_tasks.clear();
		self.remote_sqlite_tasks.clear();
		self.tunnel_tasks.clear();
		if tunnel_task_count > 0 {
			metrics::TUNNEL_TASKS_ACTIVE
				.with_label_values(&[
					self.conn.namespace_id.to_string().as_str(),
					&self.conn.pool_name,
				])
				.sub(tunnel_task_count as i64);
			metrics::ACTOR_TASKS_ACTIVE
				.with_label_values(&["tunnel_message"])
				.sub(tunnel_task_count as i64);
		}
		if kv_task_count > 0 {
			metrics::ACTOR_TASKS_ACTIVE
				.with_label_values(&["kv"])
				.sub(kv_task_count as i64);
		}
		if sqlite_page_task_count > 0 {
			metrics::ACTOR_TASKS_ACTIVE
				.with_label_values(&["sqlite_page"])
				.sub(sqlite_page_task_count as i64);
		}
		if remote_sqlite_task_count > 0 {
			metrics::ACTOR_TASKS_ACTIVE
				.with_label_values(&["remote_sqlite"])
				.sub(remote_sqlite_task_count as i64);
		}
		self.control_tasks.clear();
		self.workers.abort_all();
	}

	async fn join_next(&mut self) -> Option<Result<()>> {
		let res = self.workers.join_next().await?;
		Some(match res {
			Ok(Ok(exit)) => {
				self.remove_worker_task(exit);
				Ok(())
			}
			Ok(Err(err)) => Err(err),
			Err(err) => Err(err.into()),
		})
	}

	fn remove_worker_task(&mut self, exit: TaskExit) {
		match exit {
			TaskExit::Kv(key) => {
				if remove_closed_task(&mut self.kv_tasks, &key) {
					metrics::ACTOR_TASKS_ACTIVE.with_label_values(&["kv"]).dec();
				}
			}
			TaskExit::SqlitePage(key) => {
				if remove_closed_task(&mut self.sqlite_page_tasks, &key) {
					metrics::ACTOR_TASKS_ACTIVE
						.with_label_values(&["sqlite_page"])
						.dec();
				}
			}
			TaskExit::RemoteSqlite(key) => {
				if remove_closed_task(&mut self.remote_sqlite_tasks, &key) {
					metrics::ACTOR_TASKS_ACTIVE
						.with_label_values(&["remote_sqlite"])
						.dec();
				}
			}
			TaskExit::Tunnel(key) => {
				if remove_closed_task(&mut self.tunnel_tasks, &key) {
					tracing::trace!(
						namespace_id = %self.conn.namespace_id,
						pool_name = %self.conn.pool_name,
						envoy_key = %self.conn.envoy_key,
						gateway_id = %key.gateway_id_display(),
						request_id = %key.request_id_display(),
						"tunnel message task exited"
					);
					metrics::TUNNEL_TASKS_ACTIVE
						.with_label_values(&[
							self.conn.namespace_id.to_string().as_str(),
							&self.conn.pool_name,
						])
						.dec();
					metrics::ACTOR_TASKS_ACTIVE
						.with_label_values(&["tunnel_message"])
						.dec();
				}
			}
			TaskExit::Control => {
				remove_closed_task(&mut self.control_tasks, &());
			}
		}
	}

	fn enqueue_kv(&mut self, key: actor_kv_task::Key, msg: actor_kv_task::Message) -> Result<()> {
		let ctx = self.ctx.clone();
		let conn = self.conn.clone();
		enqueue_keyed_task(
			&mut self.kv_tasks,
			&mut self.workers,
			key,
			msg,
			Some("kv"),
			move |key, rx| actor_kv_task::task(ctx.clone(), conn.clone(), key, rx),
		)
	}

	fn enqueue_sqlite_page(
		&mut self,
		key: actor_sqlite_page_task::Key,
		msg: actor_sqlite_page_task::Message,
	) -> Result<()> {
		let ctx = self.ctx.clone();
		let conn = self.conn.clone();
		enqueue_keyed_task(
			&mut self.sqlite_page_tasks,
			&mut self.workers,
			key,
			msg,
			Some("sqlite_page"),
			move |key, rx| actor_sqlite_page_task::task(ctx.clone(), conn.clone(), key, rx),
		)
	}

	fn enqueue_remote_sqlite(
		&mut self,
		key: actor_remote_sqlite_task::Key,
		msg: actor_remote_sqlite_task::Message,
	) -> Result<()> {
		let ctx = self.ctx.clone();
		let conn = self.conn.clone();
		enqueue_keyed_task(
			&mut self.remote_sqlite_tasks,
			&mut self.workers,
			key,
			msg,
			Some("remote_sqlite"),
			move |key, rx| actor_remote_sqlite_task::task(ctx.clone(), conn.clone(), key, rx),
		)
	}

	fn enqueue_tunnel(
		&mut self,
		key: tunnel_message_task::Key,
		msg: tunnel_message_task::Message,
	) -> Result<()> {
		let ctx = self.ctx.clone();
		let conn = self.conn.clone();
		if self
			.tunnel_tasks
			.get(&key)
			.is_some_and(mpsc::UnboundedSender::is_closed)
		{
			tracing::trace!(
				namespace_id = %self.conn.namespace_id,
				pool_name = %self.conn.pool_name,
				envoy_key = %self.conn.envoy_key,
				gateway_id = %key.gateway_id_display(),
				request_id = %key.request_id_display(),
				"removing closed tunnel message task before enqueue"
			);
			self.tunnel_tasks.remove(&key);
			metrics::TUNNEL_TASKS_ACTIVE
				.with_label_values(&[
					self.conn.namespace_id.to_string().as_str(),
					&self.conn.pool_name,
				])
				.dec();
		}

		if !self.tunnel_tasks.contains_key(&key) {
			let (tx, rx) = mpsc::unbounded_channel();
			self.tunnel_tasks.insert(key.clone(), tx);
			self.workers
				.spawn(tunnel_message_task::task(ctx, conn, key.clone(), rx));
			tracing::trace!(
				namespace_id = %self.conn.namespace_id,
				pool_name = %self.conn.pool_name,
				envoy_key = %self.conn.envoy_key,
				gateway_id = %key.gateway_id_display(),
				request_id = %key.request_id_display(),
				"created tunnel message task"
			);
			metrics::TUNNEL_TASKS_ACTIVE
				.with_label_values(&[
					self.conn.namespace_id.to_string().as_str(),
					&self.conn.pool_name,
				])
				.inc();
		}

		let (message_index, message_kind, inner_data_len) = match &msg {
			tunnel_message_task::Message::Message(tunnel_msg) => (
				tunnel_msg.message_id.message_index,
				tunnel_message_kind_name(&tunnel_msg.message_kind),
				tunnel_message_inner_data_len(&tunnel_msg.message_kind),
			),
		};
		tracing::trace!(
			namespace_id = %self.conn.namespace_id,
			pool_name = %self.conn.pool_name,
			envoy_key = %self.conn.envoy_key,
			gateway_id = %key.gateway_id_display(),
			request_id = %key.request_id_display(),
			message_index,
			message_kind,
			inner_data_len,
			"enqueuing tunnel message task"
		);
		match self
			.tunnel_tasks
			.get(&key)
			.expect("task sender must exist")
			.send(msg)
		{
			Ok(()) => {
				tracing::trace!(
					namespace_id = %self.conn.namespace_id,
					pool_name = %self.conn.pool_name,
					envoy_key = %self.conn.envoy_key,
					gateway_id = %key.gateway_id_display(),
					request_id = %key.request_id_display(),
					message_index,
					message_kind,
					inner_data_len,
					"enqueued tunnel message task"
				);
				Ok(())
			}
			Err(mpsc::error::SendError(_)) => {
				tracing::warn!(
					namespace_id = %self.conn.namespace_id,
					pool_name = %self.conn.pool_name,
					envoy_key = %self.conn.envoy_key,
					gateway_id = %key.gateway_id_display(),
					request_id = %key.request_id_display(),
					message_index,
					message_kind,
					inner_data_len,
					"tunnel message task sender closed"
				);
				if self.tunnel_tasks.remove(&key).is_some() {
					metrics::TUNNEL_TASKS_ACTIVE
						.with_label_values(&[
							self.conn.namespace_id.to_string().as_str(),
							&self.conn.pool_name,
						])
						.dec();
				}
				bail!("websocket dispatcher task closed")
			}
		}
	}

	fn enqueue_control(&mut self, msg: control_task::Message) -> Result<()> {
		let ctx = self.ctx.clone();
		let conn = self.conn.clone();
		enqueue_keyed_task(
			&mut self.control_tasks,
			&mut self.workers,
			(),
			msg,
			None,
			move |(), rx| control_task::task(ctx.clone(), conn.clone(), rx),
		)
	}
}

impl Drop for TaskManager {
	fn drop(&mut self) {
		self.abort_all();
	}
}

fn remove_closed_task<K, M>(tasks: &mut StdHashMap<K, mpsc::UnboundedSender<M>>, key: &K) -> bool
where
	K: Eq + Hash,
{
	if tasks.get(key).is_some_and(mpsc::UnboundedSender::is_closed) {
		tasks.remove(key);
		true
	} else {
		false
	}
}

fn enqueue_keyed_task<K, M, F, Fut>(
	tasks: &mut StdHashMap<K, mpsc::UnboundedSender<M>>,
	workers: &mut JoinSet<Result<TaskExit>>,
	key: K,
	msg: M,
	task_kind: Option<&'static str>,
	spawn_worker: F,
) -> Result<()>
where
	K: Clone + Eq + Hash + Send + 'static,
	M: Send + 'static,
	F: FnOnce(K, mpsc::UnboundedReceiver<M>) -> Fut,
	Fut: std::future::Future<Output = Result<TaskExit>> + Send + 'static,
{
	if tasks
		.get(&key)
		.is_some_and(mpsc::UnboundedSender::is_closed)
	{
		tasks.remove(&key);
		if let Some(kind) = task_kind {
			metrics::ACTOR_TASKS_ACTIVE.with_label_values(&[kind]).dec();
		}
	}

	if !tasks.contains_key(&key) {
		let (tx, rx) = mpsc::unbounded_channel();
		tasks.insert(key.clone(), tx);
		workers.spawn(spawn_worker(key.clone(), rx));
		if let Some(kind) = task_kind {
			metrics::ACTOR_TASKS_ACTIVE.with_label_values(&[kind]).inc();
		}
	}

	match tasks.get(&key).expect("task sender must exist").send(msg) {
		Ok(()) => Ok(()),
		Err(mpsc::error::SendError(_)) => {
			tasks.remove(&key);
			if let Some(kind) = task_kind {
				metrics::ACTOR_TASKS_ACTIVE.with_label_values(&[kind]).dec();
			}
			bail!("websocket dispatcher task closed")
		}
	}
}

#[tracing::instrument(name = "ws_to_tunnel_task", skip_all, fields(ray_id=?ctx.ray_id(), req_id=?ctx.req_id(), namespace_id=%conn.namespace_id, pool_name=%conn.pool_name, envoy_key=%conn.envoy_key, protocol_version=%conn.protocol_version))]
pub async fn task(
	ctx: StandaloneCtx,
	conn: Arc<Conn>,
	ws_rx: Arc<Mutex<WebSocketReceiver>>,
	ws_to_tunnel_abort_rx: watch::Receiver<()>,
) -> Result<LifecycleResult> {
	let mut event_demuxer = ActorEventDemuxer::new(ctx.clone(), conn.envoy_key.clone());

	let res = task_inner(ctx, conn, ws_rx, ws_to_tunnel_abort_rx, &mut event_demuxer).await;

	// Must shutdown demuxer to allow for all in-flight events to finish
	event_demuxer.shutdown().await;

	res
}

#[tracing::instrument(skip_all, fields(namespace_id=%conn.namespace_id, pool_name=%conn.pool_name, envoy_key=%conn.envoy_key, protocol_version=%conn.protocol_version))]
pub async fn task_inner(
	ctx: StandaloneCtx,
	conn: Arc<Conn>,
	ws_rx: Arc<Mutex<WebSocketReceiver>>,
	mut ws_to_tunnel_abort_rx: watch::Receiver<()>,
	event_demuxer: &mut ActorEventDemuxer,
) -> Result<LifecycleResult> {
	let mut ws_rx = ws_rx.lock().await;
	let mut term_signal = rivet_runtime::TermSignal::get();
	let mut task_manager = TaskManager::new(ctx.clone(), conn.clone());

	// Leaky bucket rate limit on consuming envoy ws messages. The envoy connection multiplexes
	// every actor on a runner, so this bounds the rate at which a single untrusted runner can drive
	// engine work (task spawns, KV/SQLite ops). Reads are paused while empty, applying TCP
	// backpressure to the runner rather than dropping protocol messages.
	let mut rate_limit = rivet_util::throttle::RateLimiter::new(
		rivet_util::throttle::RateLimitMethod::LeakyBucket {
			requests: ctx
				.config()
				.pegboard()
				.envoy_websocket_rate_limit_requests(),
			drip_rate: Duration::from_micros(
				ctx.config()
					.pegboard()
					.envoy_websocket_rate_limit_drip_rate_us(),
			),
		},
	);

	loop {
		tokio::select! {
			recv = async {
				rate_limit.acquire().await;
				recv_msg(&mut ws_rx, &mut ws_to_tunnel_abort_rx, &mut term_signal).await
			} => {
				let branch_start = Instant::now();
				let branch_result: Result<Option<LifecycleResult>> = async {
					match recv? {
						Ok(Some(msg)) => {
							let msg = match decode_message(msg, conn.protocol_version)? {
								Ok(msg) => msg,
								Err(lifecycle) => {
									if let Some(lifecycle) = lifecycle {
										task_manager.abort_all();
										return Ok(Some(lifecycle));
									}
									return Ok(None);
								}
							};
							let kind = message_kind_label(&msg);
							let measure = perf_start!(
								&metrics::WS_MESSAGE_PROCESSING_DURATION,
								slow_ms = SLOW_HANDLE_WARN_THRESHOLD.as_millis() as u64,
								"pegboard_envoy_ws_message",
								labels: {
									namespace_id = %conn.namespace_id,
									pool_name = %conn.pool_name,
									message_kind = %kind,
								},
								fields: {
									envoy_key = %conn.envoy_key,
									protocol_version = %conn.protocol_version,
								},
							);
							let lifecycle = handle_message(
								&ctx,
								conn.clone(),
								event_demuxer,
								&mut task_manager,
								msg,
							)
							.await?;
							let elapsed = perf_finish!(measure, fields: { message_kind = %kind });
							if elapsed >= SLOW_HANDLE_WARN_THRESHOLD {
								metrics::WS_MESSAGE_SLOW_TOTAL
									.with_label_values(&[
										conn.namespace_id.to_string().as_str(),
										conn.pool_name.as_str(),
										kind,
									])
									.inc();
							}
							Ok(lifecycle)
						}
						Ok(None) => Ok(None),
						Err(lifecycle_res) => Ok(Some(lifecycle_res)),
					}
				}.await;
				metrics::WS_TO_TUNNEL_BRANCH_DURATION
					.with_label_values(&["ws_msg"])
					.observe(branch_start.elapsed().as_secs_f64());
				if let Some(lifecycle) = branch_result? {
					return Ok(lifecycle);
				}
			}
			worker = task_manager.join_next(), if !task_manager.workers.is_empty() => {
				let branch_start = Instant::now();
				let res = if let Some(result) = worker {
					result
				} else {
					Ok(())
				};
				metrics::WS_TO_TUNNEL_BRANCH_DURATION
					.with_label_values(&["completed_task"])
					.observe(branch_start.elapsed().as_secs_f64());
				res?;
			}
		}
	}
}

#[tracing::instrument(skip_all)]
async fn recv_msg(
	ws_rx: &mut MutexGuard<'_, WebSocketReceiver>,
	ws_to_tunnel_abort_rx: &mut watch::Receiver<()>,
	term_signal: &mut rivet_runtime::TermSignal,
) -> Result<std::result::Result<Option<Bytes>, LifecycleResult>> {
	let msg = tokio::select! {
		res = ws_rx.try_next() => {
			if let Some(msg) = res? {
				msg
			} else {
				tracing::debug!("websocket closed");
				return Ok(Err(LifecycleResult::Closed {
					incoming_close_code: None,
					incoming_close_reason: None,
				}));
			}
		}
		_ = ws_to_tunnel_abort_rx.changed() => {
			tracing::debug!("task aborted");
			return Ok(Err(LifecycleResult::Aborted));
		}
		_ = term_signal.recv() => {
			return Err(errors::WsError::GoingAway.build());
		}
	};

	match msg {
		Message::Binary(data) => {
			tracing::trace!(
				data_len = data.len(),
				"received binary message from WebSocket"
			);

			Ok(Ok(Some(data)))
		}
		Message::Close(frame) => {
			let (incoming_close_code, incoming_close_reason) = frame
				.as_ref()
				.map(|f| (Some(u16::from(f.code)), Some(f.reason.to_string())))
				.unwrap_or((None, None));
			tracing::debug!(
				?incoming_close_code,
				?incoming_close_reason,
				"websocket closed"
			);
			return Ok(Err(LifecycleResult::Closed {
				incoming_close_code,
				incoming_close_reason,
			}));
		}
		_ => {
			// Ignore other message types
			Ok(Ok(None))
		}
	}
}

/// Returns a short, bounded label identifying the message variant. Used as the `message_kind`
/// label on `WS_MESSAGE_PROCESSING_DURATION` and the slow-handle warning. Keep the set small and
/// stable; new variants should be added explicitly rather than falling through to a wildcard.
fn message_kind_label(msg: &protocol::ToRivet) -> &'static str {
	match msg {
		protocol::ToRivet::ToRivetPong(_) => "pong",
		protocol::ToRivet::ToRivetKvRequest(_) => "kv_request",
		protocol::ToRivet::ToRivetSqliteGetPagesRequest(_) => "sqlite_get_pages",
		protocol::ToRivet::ToRivetSqliteCommitRequest(_) => "sqlite_commit",
		protocol::ToRivet::ToRivetSqliteExecRequest(_) => "sqlite_exec",
		protocol::ToRivet::ToRivetSqliteExecuteRequest(_) => "sqlite_execute",
		protocol::ToRivet::ToRivetSqliteExecuteBatchRequest(_) => "sqlite_execute_batch",
		protocol::ToRivet::ToRivetTunnelMessage(_) => "tunnel_message",
		protocol::ToRivet::ToRivetMetadata(_) => "metadata",
		protocol::ToRivet::ToRivetEvents(_) => "events",
		protocol::ToRivet::ToRivetAckCommands(_) => "ack_commands",
		protocol::ToRivet::ToRivetStopping => "stopping",
	}
}

fn decode_message(
	msg: Bytes,
	protocol_version: u16,
) -> Result<std::result::Result<protocol::ToRivet, Option<LifecycleResult>>> {
	match versioned::ToRivet::deserialize(&msg, protocol_version) {
		Ok(msg) => Ok(Ok(msg)),
		Err(err) => {
			tracing::warn!(?err, msg_len = msg.len(), "failed to deserialize message");
			Ok(Err(None))
		}
	}
}

#[tracing::instrument(skip_all)]
async fn handle_message(
	ctx: &StandaloneCtx,
	conn: Arc<Conn>,
	event_demuxer: &mut ActorEventDemuxer,
	task_manager: &mut TaskManager,
	msg: protocol::ToRivet,
) -> Result<Option<LifecycleResult>> {
	tracing::debug!(?msg, "received message from envoy");

	dispatch_message(ctx, conn, event_demuxer, task_manager, msg).await
}

#[tracing::instrument(skip_all)]
async fn dispatch_message(
	ctx: &StandaloneCtx,
	conn: Arc<Conn>,
	event_demuxer: &mut ActorEventDemuxer,
	task_manager: &mut TaskManager,
	msg: protocol::ToRivet,
) -> Result<Option<LifecycleResult>> {
	match msg {
		protocol::ToRivet::ToRivetPong(pong) => {
			let now = util::timestamp::now();
			let rtt = now.saturating_sub(pong.ts);

			let rtt = if let Ok(rtt) = u32::try_from(rtt) {
				rtt
			} else {
				tracing::debug!("ping ts in the future, ignoring");
				u32::MAX
			};

			conn.last_rtt.store(rtt, Ordering::SeqCst);
			conn.last_ping_ts
				.store(util::timestamp::now(), Ordering::SeqCst);
			metrics::ENVOY_PING_LAG_SECONDS
				.with_label_values(&[conn.namespace_id.to_string().as_str(), &conn.pool_name])
				.observe(rtt as f64 / 1000.0);
		}
		// Process KV request
		protocol::ToRivet::ToRivetKvRequest(req) => {
			let key = actor_kv_task::Key::new(req.actor_id.clone());
			task_manager.enqueue_kv(key, actor_kv_task::Message::Request(req))?;
		}
		protocol::ToRivet::ToRivetSqliteGetPagesRequest(req) => {
			let Some(generation) = req.data.expected_generation else {
				send_sqlite_get_pages_response(
					&conn,
					req.request_id,
					protocol::SqliteGetPagesResponse::SqliteErrorResponse(
						sqlite_protocol_error_response(
							"sqlite get_pages missing expectedGeneration",
						),
					),
				)
				.await?;
				return Ok(None);
			};
			let key = actor_sqlite_page_task::Key::new(req.data.actor_id.clone(), generation);
			task_manager
				.enqueue_sqlite_page(key, actor_sqlite_page_task::Message::GetPages(req))?;
		}
		protocol::ToRivet::ToRivetSqliteCommitRequest(req) => {
			let Some(generation) = req.data.expected_generation else {
				send_sqlite_commit_response(
					&conn,
					req.request_id,
					protocol::SqliteCommitResponse::SqliteErrorResponse(
						sqlite_protocol_error_response("sqlite commit missing expectedGeneration"),
					),
				)
				.await?;
				return Ok(None);
			};
			let key = actor_sqlite_page_task::Key::new(req.data.actor_id.clone(), generation);
			task_manager.enqueue_sqlite_page(key, actor_sqlite_page_task::Message::Commit(req))?;
		}
		protocol::ToRivet::ToRivetSqliteExecRequest(req) => {
			let key =
				actor_remote_sqlite_task::Key::new(req.data.actor_id.clone(), req.data.generation);
			task_manager
				.enqueue_remote_sqlite(key, actor_remote_sqlite_task::Message::Exec(req))?;
		}
		protocol::ToRivet::ToRivetSqliteExecuteRequest(req) => {
			let key =
				actor_remote_sqlite_task::Key::new(req.data.actor_id.clone(), req.data.generation);
			task_manager
				.enqueue_remote_sqlite(key, actor_remote_sqlite_task::Message::Execute(req))?;
		}
		protocol::ToRivet::ToRivetSqliteExecuteBatchRequest(req) => {
			let key =
				actor_remote_sqlite_task::Key::new(req.data.actor_id.clone(), req.data.generation);
			task_manager
				.enqueue_remote_sqlite(key, actor_remote_sqlite_task::Message::ExecuteBatch(req))?;
		}
		protocol::ToRivet::ToRivetTunnelMessage(tunnel_msg) => {
			let inner_data_len = tunnel_message_inner_data_len(&tunnel_msg.message_kind);
			if inner_data_len > ctx.config().pegboard().envoy_max_response_payload_size() {
				return Err(
					errors::WsError::InvalidPacket("payload too large".to_string()).build(),
				);
			}
			let key = tunnel_message_task::Key::new(
				tunnel_msg.message_id.gateway_id,
				tunnel_msg.message_id.request_id,
			);
			task_manager.enqueue_tunnel(key, tunnel_message_task::Message::Message(tunnel_msg))?;
		}
		protocol::ToRivet::ToRivetMetadata(metadata) => {
			task_manager.enqueue_control(control_task::Message::Metadata(metadata))?;
		}
		// Forward to demuxer which forwards to actor wf
		protocol::ToRivet::ToRivetEvents(events) => {
			for event in events {
				event_demuxer.ingest(Id::parse(&event.checkpoint.actor_id)?, event);
			}
		}
		protocol::ToRivet::ToRivetAckCommands(ack) => {
			task_manager.enqueue_control(control_task::Message::AckCommands(ack))?;
		}
		protocol::ToRivet::ToRivetStopping => {
			if !conn.reported_stopping.swap(true, Ordering::SeqCst) {
				metrics::transition_envoy_connection_state(
					conn.namespace_id.to_string().as_str(),
					&conn.pool_name,
					conn.protocol_version.to_string().as_str(),
					metrics::EnvoyState::Connected,
					metrics::EnvoyState::Stopping,
					"envoy_reported_stopping",
				);
			}

			// For serverful, remove from lb
			if !conn.is_serverless() {
				ctx.op(pegboard::ops::envoy::expire::Input {
					namespace_id: conn.namespace_id,
					envoy_key: conn.envoy_key.to_string(),
					skip_if_fresh: false,
				})
				.await?;
			}

			let ctx = ctx.clone();
			let namespace_id = conn.namespace_id;
			let envoy_key = conn.envoy_key.clone();
			let actor_eviction_delay = conn.pool.actor_eviction_delay();
			let actor_eviction_period = conn.pool.actor_eviction_period();
			let actor_eviction_rate = conn.pool.actor_eviction_rate();

			// TODO: Drop guard
			// Evict all actors
			tokio::spawn(async move {
				if actor_eviction_delay != 0 {
					tokio::time::sleep(Duration::from_secs(actor_eviction_delay as u64)).await;
				}

				let res = ctx
					.op(pegboard::ops::envoy::evict_actors::Input {
						namespace_id,
						envoy_key: envoy_key.clone(),
						throttle: Some(pegboard::ops::envoy::evict_actors::Throttle {
							rate: actor_eviction_rate,
							period: Duration::from_secs(actor_eviction_period as u64),
						}),
					})
					.await;

				if let Err(err) = res {
					tracing::error!(?namespace_id, %envoy_key, ?err, "failed to evict actors");
				}
			});
		}
	}

	Ok(None)
}

pub(super) async fn handle_kv_request(
	ctx: &StandaloneCtx,
	conn: &Conn,
	req: protocol::ToRivetKvRequest,
) -> Result<()> {
	let actor_id = match Id::parse(&req.actor_id) {
		Ok(actor_id) => actor_id,
		Err(err) => {
			send_actor_kv_error(conn, req.request_id, &err.to_string()).await?;
			return Ok(());
		}
	};

	let actor_res = ctx
		.op(pegboard::ops::actor::get_for_kv::Input { actor_id })
		.await
		.with_context(|| format!("failed to get envoy for actor: {}", actor_id))?;

	let Some(actor) = actor_res else {
		send_actor_kv_error(conn, req.request_id, "actor does not exist").await?;
		return Ok(());
	};

	if actor.namespace_id != conn.namespace_id {
		send_actor_kv_error(conn, req.request_id, "actor does not exist").await?;
		return Ok(());
	}

	let recipient = actor_kv::Recipient {
		actor_id,
		namespace_id: conn.namespace_id,
		name: actor.name,
	};

	match req.data {
		protocol::KvRequestData::KvGetRequest(body) => {
			let res = actor_kv::get(&*ctx.udb()?, &recipient, body.keys).await;
			send_actor_kv_response(
				conn,
				req.request_id,
				match res {
					Ok((keys, values, metadata)) => {
						protocol::KvResponseData::KvGetResponse(protocol::KvGetResponse {
							keys,
							values,
							metadata,
						})
					}
					Err(err) => {
						protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
							message: err.to_string(),
						})
					}
				},
				"KV get response",
			)
			.await?;
		}
		protocol::KvRequestData::KvListRequest(body) => {
			let res = actor_kv::list(
				&*ctx.udb()?,
				&recipient,
				body.query,
				body.reverse.unwrap_or_default(),
				body.limit
					.map(TryInto::try_into)
					.transpose()
					.context("KV list limit value overflow")?,
			)
			.await;
			send_actor_kv_response(
				conn,
				req.request_id,
				match res {
					Ok((keys, values, metadata)) => {
						protocol::KvResponseData::KvListResponse(protocol::KvListResponse {
							keys,
							values,
							metadata,
						})
					}
					Err(err) => {
						protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
							message: err.to_string(),
						})
					}
				},
				"KV list response",
			)
			.await?;
		}
		protocol::KvRequestData::KvPutRequest(body) => {
			let res = actor_kv::put(&*ctx.udb()?, &recipient, body.keys, body.values).await;
			send_actor_kv_response(
				conn,
				req.request_id,
				match res {
					Ok(()) => protocol::KvResponseData::KvPutResponse,
					Err(err) => {
						protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
							message: err.to_string(),
						})
					}
				},
				"KV put response",
			)
			.await?;
		}
		protocol::KvRequestData::KvDeleteRequest(body) => {
			let res = actor_kv::delete(&*ctx.udb()?, &recipient, body.keys).await;
			send_actor_kv_response(
				conn,
				req.request_id,
				match res {
					Ok(()) => protocol::KvResponseData::KvDeleteResponse,
					Err(err) => {
						protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
							message: err.to_string(),
						})
					}
				},
				"KV delete response",
			)
			.await?;
		}
		protocol::KvRequestData::KvDeleteRangeRequest(body) => {
			let res = actor_kv::delete_range(&*ctx.udb()?, &recipient, body.start, body.end).await;
			send_actor_kv_response(
				conn,
				req.request_id,
				match res {
					Ok(()) => protocol::KvResponseData::KvDeleteResponse,
					Err(err) => {
						protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
							message: err.to_string(),
						})
					}
				},
				"KV delete range response",
			)
			.await?;
		}
		protocol::KvRequestData::KvDropRequest => {
			let res = actor_kv::delete_all(&*ctx.udb()?, &recipient).await;
			send_actor_kv_response(
				conn,
				req.request_id,
				match res {
					Ok(()) => protocol::KvResponseData::KvDropResponse,
					Err(err) => {
						protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
							message: err.to_string(),
						})
					}
				},
				"KV drop response",
			)
			.await?;
		}
	}

	Ok(())
}

pub(super) struct TimedSqliteCommitResponse {
	pub(super) response: protocol::SqliteCommitResponse,
	pub(super) commit_completed_at: Instant,
}

pub(super) async fn handle_sqlite_get_pages_response(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteGetPagesRequest,
) -> protocol::SqliteGetPagesResponse {
	let start = Instant::now();
	let actor_id = request.actor_id.clone();
	let pgnos = request.pgnos.clone();
	let requested_pages = pgnos.len();
	let request_bytes = requested_pages * std::mem::size_of::<u32>();
	let expected_generation = request.expected_generation;
	let expected_head_txid = request.expected_head_txid;
	let response = match handle_sqlite_get_pages(ctx, conn, request).await {
		Ok(response) => response,
		Err(err) => {
			if is_startup_database_miss(&err, expected_generation, expected_head_txid) {
				tracing::debug!(
					actor_id = %actor_id,
					?pgnos,
					"sqlite get_pages did not find an existing actor database"
				);
			} else {
				tracing::error!(
					actor_id = %actor_id,
					?pgnos,
					?expected_generation,
					?expected_head_txid,
					?err,
					"sqlite get_pages request failed"
				);
			}
			protocol::SqliteGetPagesResponse::SqliteErrorResponse(sqlite_error_response(&err))
		}
	};
	record_sqlite_request_metrics(
		conn,
		"get_pages",
		sqlite_get_pages_response_kind(&response),
		start,
	);
	metrics::SQLITE_REQUEST_PAGES
		.with_label_values(&[
			conn.namespace_id.to_string().as_str(),
			conn.pool_name.as_str(),
			"get_pages",
			"request",
		])
		.observe(requested_pages as f64);
	record_sqlite_payload_bytes(conn, "get_pages", "request", request_bytes);
	if let protocol::SqliteGetPagesResponse::SqliteGetPagesOk(ok) = &response {
		let response_pages = ok.pages.len();
		let response_bytes = ok
			.pages
			.iter()
			.map(|page| page.bytes.as_ref().map_or(0, Vec::len))
			.sum();
		metrics::SQLITE_REQUEST_PAGES
			.with_label_values(&[
				conn.namespace_id.to_string().as_str(),
				conn.pool_name.as_str(),
				"get_pages",
				"response",
			])
			.observe(response_pages as f64);
		record_sqlite_payload_bytes(conn, "get_pages", "response", response_bytes);
	}
	response
}

pub(super) async fn handle_sqlite_commit_response(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteCommitRequest,
) -> TimedSqliteCommitResponse {
	let start = Instant::now();
	let actor_id = request.actor_id.clone();
	let dirty_pages = request.dirty_pages.len();
	let request_bytes = request
		.dirty_pages
		.iter()
		.map(|page| page.bytes.len())
		.sum();
	let response = match handle_sqlite_commit(ctx, conn, request).await {
		Ok(response) => response,
		Err(err) => {
			tracing::error!(actor_id = %actor_id, ?err, "sqlite commit request failed");
			protocol::SqliteCommitResponse::SqliteErrorResponse(sqlite_error_response(&err))
		}
	};
	record_sqlite_request_metrics(
		conn,
		"commit",
		sqlite_commit_response_kind(&response),
		start,
	);
	metrics::SQLITE_REQUEST_DIRTY_PAGES
		.with_label_values(&[
			conn.namespace_id.to_string().as_str(),
			conn.pool_name.as_str(),
			"commit",
		])
		.observe(dirty_pages as f64);
	record_sqlite_payload_bytes(conn, "commit", "request", request_bytes);
	TimedSqliteCommitResponse {
		response,
		commit_completed_at: Instant::now(),
	}
}

pub(super) async fn handle_remote_sqlite_exec_response(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteExecRequest,
) -> protocol::SqliteExecResponse {
	let start = Instant::now();
	let actor_id = request.actor_id.clone();
	let request_bytes = request.sql.len();
	let response = match handle_remote_sqlite_exec(ctx, conn, request).await {
		Ok(result) => protocol::SqliteExecResponse::SqliteExecOk(protocol::SqliteExecOk {
			result: protocol_query_result(result),
		}),
		Err(err) => {
			tracing::error!(actor_id = %actor_id, ?err, "remote sqlite exec request failed");
			protocol::SqliteExecResponse::SqliteErrorResponse(sqlite_error_response(&err))
		}
	};
	record_sqlite_request_metrics(conn, "exec", sqlite_exec_response_kind(&response), start);
	record_sqlite_payload_bytes(conn, "exec", "request", request_bytes);
	if let protocol::SqliteExecResponse::SqliteExecOk(ok) = &response {
		record_sqlite_payload_bytes(conn, "exec", "response", query_result_bytes(&ok.result));
	}
	response
}

pub(super) async fn handle_remote_sqlite_execute_response(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteExecuteRequest,
) -> protocol::SqliteExecuteResponse {
	let start = Instant::now();
	let actor_id = request.actor_id.clone();
	let request_bytes = request.sql.len() + bind_params_bytes(request.params.as_ref());
	let response = match handle_remote_sqlite_execute(ctx, conn, request).await {
		Ok(result) => protocol::SqliteExecuteResponse::SqliteExecuteOk(protocol::SqliteExecuteOk {
			result: protocol_execute_result(result),
		}),
		Err(err) => {
			tracing::error!(actor_id = %actor_id, ?err, "remote sqlite execute request failed");
			protocol::SqliteExecuteResponse::SqliteErrorResponse(sqlite_error_response(&err))
		}
	};
	record_sqlite_request_metrics(
		conn,
		"execute",
		sqlite_execute_response_kind(&response),
		start,
	);
	record_sqlite_payload_bytes(conn, "execute", "request", request_bytes);
	if let protocol::SqliteExecuteResponse::SqliteExecuteOk(ok) = &response {
		record_sqlite_payload_bytes(
			conn,
			"execute",
			"response",
			execute_result_bytes(&ok.result),
		);
	}
	response
}

pub(super) async fn handle_remote_sqlite_execute_batch_response(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteExecuteBatchRequest,
) -> protocol::SqliteExecuteBatchResponse {
	let start = Instant::now();
	let actor_id = request.actor_id.clone();
	let request_bytes = request
		.statements
		.iter()
		.map(|statement| statement.sql.len() + bind_params_bytes(statement.params.as_ref()))
		.sum();
	let response = match handle_remote_sqlite_execute_batch(ctx, conn, request).await {
		Ok(results) => protocol::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(
			protocol::SqliteExecuteBatchOk {
				results: results.into_iter().map(protocol_execute_result).collect(),
			},
		),
		Err(err) => {
			tracing::error!(actor_id = %actor_id, ?err, "remote sqlite execute batch request failed");
			protocol::SqliteExecuteBatchResponse::SqliteErrorResponse(sqlite_error_response(&err))
		}
	};
	record_sqlite_request_metrics(
		conn,
		"execute_batch",
		sqlite_execute_batch_response_kind(&response),
		start,
	);
	record_sqlite_payload_bytes(conn, "execute_batch", "request", request_bytes);
	if let protocol::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(ok) = &response {
		record_sqlite_payload_bytes(
			conn,
			"execute_batch",
			"response",
			ok.results.iter().map(execute_result_bytes).sum(),
		);
	}
	response
}

pub(super) async fn ack_commands(
	ctx: &StandaloneCtx,
	namespace_id: Id,
	envoy_key: &str,
	ack: protocol::ToRivetAckCommands,
) -> Result<()> {
	let ack = &ack;
	ctx.udb()?
		.txn("envoy_handle_ack", |tx| async move {
			let tx = tx.with_subspace(pegboard::keys::subspace());

			for checkpoint in &ack.last_command_checkpoints {
				let start = tx.pack(
					&pegboard::keys::envoy::ActorCommandKey::subspace_with_actor(
						namespace_id,
						envoy_key.to_string(),
						Id::parse(&checkpoint.actor_id)?,
						checkpoint.generation,
					),
				);
				let end = end_of_key_range(&tx.pack(
					&pegboard::keys::envoy::ActorCommandKey::subspace_with_index(
						namespace_id,
						envoy_key.to_string(),
						Id::parse(&checkpoint.actor_id)?,
						checkpoint.generation,
						checkpoint.index,
					),
				));
				tx.clear_range(&start, &end);
			}

			Ok(())
		})
		.await
}

pub(super) async fn handle_metadata(
	ctx: &StandaloneCtx,
	namespace_id: Id,
	envoy_key: &str,
	metadata: protocol::ToRivetMetadata,
) -> Result<()> {
	let metadata = &metadata;
	ctx.udb()?
		.txn("envoy_handle_metadata", |tx| {
			async move {
				let tx = tx.with_subspace(pegboard::keys::subspace());

				// Populate actor names if provided
				if let Some(actor_names) = &metadata.prepopulate_actor_names {
					// Write each actor name into the namespace actor names list
					for (name, data) in actor_names {
						let metadata = serde_json::from_str::<
							serde_json::Map<String, serde_json::Value>,
						>(&data.metadata)
						.unwrap_or_default();

						tx.write(
							&pegboard::keys::ns::ActorNameKey::new(namespace_id, name.clone()),
							ActorNameKeyData { metadata },
						)?;
					}
				}

				// Write envoy metadata
				if let Some(metadata) = &metadata.metadata {
					let metadata = MetadataKeyData {
						metadata:
							serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
								&metadata,
							)
							.unwrap_or_default(),
					};

					let metadata_key = pegboard::keys::envoy::MetadataKey::new(
						namespace_id,
						envoy_key.to_string(),
					);

					// Clear old metadata
					tx.delete_key_subspace(&metadata_key);

					// Write metadata
					for (i, chunk) in metadata_key.split(metadata)?.into_iter().enumerate() {
						let chunk_key = metadata_key.chunk(i);

						tx.set(&tx.pack(&chunk_key), &chunk);
					}
				}
				Ok(())
			}
		})
		.await
}

#[tracing::instrument(skip_all)]
pub(super) async fn handle_tunnel_message(
	ctx: &StandaloneCtx,
	conn: &Conn,
	msg: protocol::ToRivetTunnelMessage,
) -> Result<()> {
	// Extract inner data length before consuming msg
	let inner_data_len = tunnel_message_inner_data_len(&msg.message_kind);
	let gateway_id = msg.message_id.gateway_id;
	let request_id = msg.message_id.request_id;
	let message_index = msg.message_id.message_index;
	let message_kind = tunnel_message_kind_name(&msg.message_kind);
	let is_websocket_open = matches!(
		&msg.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(_)
	);
	let is_websocket_close = matches!(
		&msg.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(_)
	);

	// Observe envoy-side actor wake duration on the matching ToRivetWebSocketOpen
	// reply, or as an error if the envoy closed the websocket before opening it.
	// Mirrors the gateway-side `pegboard_gateway_websocket_open_wait_seconds`.
	if is_websocket_open || is_websocket_close {
		if let Some((_, start)) = conn
			.pending_websocket_opens
			.remove_async(&(gateway_id, request_id))
			.await
		{
			let result = if is_websocket_open { "ok" } else { "error" };
			metrics::ACTOR_WAKE_DURATION
				.with_label_values(&[
					conn.namespace_id.to_string().as_str(),
					conn.pool_name.as_str(),
					result,
				])
				.observe(start.elapsed().as_secs_f64());
		}
	}

	// Enforce incoming payload size
	if inner_data_len > ctx.config().pegboard().envoy_max_response_payload_size() {
		return Err(errors::WsError::InvalidPacket("payload too large".to_string()).build());
	}

	// if !authorized_tunnel_routes
	// 	.contains_async(&(msg.message_id.gateway_id, msg.message_id.request_id))
	// 	.await
	// {
	// 	return Err(
	// 		errors::WsError::InvalidPacket("unauthorized tunnel message".to_string()).build(),
	// 	);
	// }

	let gateway_reply_to = GatewayReceiverSubject::new(msg.message_id.gateway_id);
	let msg_serialized =
		versioned::ToGateway::wrap_latest(protocol::ToGateway::ToRivetTunnelMessage(msg))
			.serialize_with_embedded_version(PROTOCOL_VERSION)
			.context("failed to serialize tunnel message for gateway")?;

	tracing::trace!(
		namespace_id = %conn.namespace_id,
		pool_name = %conn.pool_name,
		envoy_key = %conn.envoy_key,
		gateway_id = %tunnel_message_task::display_id(&gateway_id),
		request_id = %tunnel_message_task::display_id(&request_id),
		message_index,
		message_kind,
		inner_data_len = inner_data_len,
		serialized_len = msg_serialized.len(),
		"publishing tunnel message to gateway"
	);
	if is_websocket_open {
		tracing::debug!(
			namespace_id = %conn.namespace_id,
			pool_name = %conn.pool_name,
			envoy_key = %conn.envoy_key,
			gateway_id = %tunnel_message_task::display_id(&gateway_id),
			request_id = %tunnel_message_task::display_id(&request_id),
			message_index,
			message_kind,
			gateway_reply_to = %gateway_reply_to,
			"publishing websocket open to gateway"
		);
	}

	match ctx
		.ups()?
		.publish(&gateway_reply_to, &msg_serialized, PublishOpts::one())
		.await
	{
		Ok(message_id) => {
			metrics::TUNNEL_PUBLISH_TOTAL
				.with_label_values(&[
					conn.namespace_id.to_string().as_str(),
					conn.pool_name.as_str(),
					"ok",
				])
				.inc();
			tracing::trace!(
				namespace_id = %conn.namespace_id,
				pool_name = %conn.pool_name,
				envoy_key = %conn.envoy_key,
				gateway_id = %tunnel_message_task::display_id(&gateway_id),
				request_id = %tunnel_message_task::display_id(&request_id),
				message_index,
				message_kind,
				gateway_reply_to = %gateway_reply_to,
				?message_id,
				"published tunnel message to gateway"
			);
			if is_websocket_open {
				tracing::debug!(
					namespace_id = %conn.namespace_id,
					pool_name = %conn.pool_name,
					envoy_key = %conn.envoy_key,
					gateway_id = %tunnel_message_task::display_id(&gateway_id),
					request_id = %tunnel_message_task::display_id(&request_id),
					message_index,
					message_kind,
					gateway_reply_to = %gateway_reply_to,
					?message_id,
					"published websocket open to gateway"
				);
			}
		}
		Err(err) => {
			metrics::TUNNEL_PUBLISH_TOTAL
				.with_label_values(&[
					conn.namespace_id.to_string().as_str(),
					conn.pool_name.as_str(),
					"error",
				])
				.inc();
			tracing::warn!(
				namespace_id = %conn.namespace_id,
				pool_name = %conn.pool_name,
				envoy_key = %conn.envoy_key,
				gateway_id = %tunnel_message_task::display_id(&gateway_id),
				request_id = %tunnel_message_task::display_id(&request_id),
				message_index,
				message_kind,
				gateway_reply_to = %gateway_reply_to,
				?err,
				"failed to publish tunnel message to gateway"
			);
			return Err(err).with_context(|| {
				format!(
					"failed to publish tunnel message to gateway reply topic: {}",
					gateway_reply_to
				)
			});
		}
	}

	Ok(())
}

fn tunnel_message_kind_name(kind: &protocol::ToRivetTunnelMessageKind) -> &'static str {
	use protocol::ToRivetTunnelMessageKind;
	match kind {
		ToRivetTunnelMessageKind::ToRivetResponseStart(_) => "ToRivetResponseStart",
		ToRivetTunnelMessageKind::ToRivetResponseChunk(_) => "ToRivetResponseChunk",
		ToRivetTunnelMessageKind::ToRivetResponseAbort => "ToRivetResponseAbort",
		ToRivetTunnelMessageKind::ToRivetWebSocketOpen(_) => "ToRivetWebSocketOpen",
		ToRivetTunnelMessageKind::ToRivetWebSocketMessage(_) => "ToRivetWebSocketMessage",
		ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(_) => "ToRivetWebSocketMessageAck",
		ToRivetTunnelMessageKind::ToRivetWebSocketClose(_) => "ToRivetWebSocketClose",
	}
}

async fn handle_sqlite_get_pages(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteGetPagesRequest,
) -> Result<protocol::SqliteGetPagesResponse> {
	if request.pgnos.len() > MAX_GET_PAGES_PER_REQUEST {
		return Ok(protocol::SqliteGetPagesResponse::SqliteErrorResponse(
			sqlite_protocol_error_response(&format!(
				"sqlite get_pages requested {} pages, exceeding limit {}",
				request.pgnos.len(),
				MAX_GET_PAGES_PER_REQUEST,
			)),
		));
	}

	validate_sqlite_actor_for_request(ctx, conn, &request.actor_id, request.expected_generation)
		.await?;

	let actor_db = actor_db(ctx, conn, request.actor_id.clone()).await?;
	let result = actor_db
		.get_pages_with_options(
			request.pgnos,
			depot::types::GetPagesOptions {
				expected_head_txid: request.expected_head_txid,
				expand_overflow: true,
				..Default::default()
			},
		)
		.await?;
	Ok(sqlite_get_pages_ok(result).await?)
}

async fn sqlite_get_pages_ok(
	result: depot::types::GetPagesResult,
) -> Result<protocol::SqliteGetPagesResponse> {
	Ok(protocol::SqliteGetPagesResponse::SqliteGetPagesOk(
		protocol::SqliteGetPagesOk {
			pages: result
				.pages
				.into_iter()
				.map(sqlite_runtime::protocol_sqlite_conveyer_fetched_page)
				.collect(),
			head_txid: Some(result.head_txid),
		},
	))
}

async fn handle_sqlite_commit(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteCommitRequest,
) -> Result<protocol::SqliteCommitResponse> {
	let dispatch_measure = perf_start!(
		&crate::metrics::SQLITE_COMMIT_ENVOY_DISPATCH_DURATION,
		slow_ms = 100,
		"sqlite_commit_envoy_dispatch",
		labels: {
			namespace_id = %conn.namespace_id,
			pool_name = %conn.pool_name,
		},
		fields: {
			envoy_key = %conn.envoy_key,
			actor_id = %request.actor_id,
			expected_generation = ?request.expected_generation,
		},
	);
	validate_sqlite_actor_for_request(ctx, conn, &request.actor_id, request.expected_generation)
		.await?;
	perf_finish!(dispatch_measure);

	let actor_id = request.actor_id.clone();
	let actor_db = actor_db(ctx, conn, actor_id.clone()).await?;
	let engine_result = actor_db
		.commit_with_options(
			request
				.dirty_pages
				.into_iter()
				.map(pump_dirty_page)
				.collect(),
			request.db_size_pages,
			request.now_ms,
			depot::types::CommitOptions {
				expected_head_txid: request.expected_head_txid,
				disable_size_cap: ctx.config().sqlite().unstable_disable_commit_size_cap(),
			},
		)
		.await;
	let response_measure = perf_start!(
		&crate::metrics::SQLITE_COMMIT_ENVOY_RESPONSE_DURATION,
		slow_ms = 100,
		"sqlite_commit_envoy_response",
		labels: {
			namespace_id = %conn.namespace_id,
			pool_name = %conn.pool_name,
		},
		fields: {
			envoy_key = %conn.envoy_key,
			actor_id = %actor_id,
		},
	);
	let response = match engine_result {
		Ok(result) => Ok(protocol::SqliteCommitResponse::SqliteCommitOk(
			protocol::SqliteCommitOk {
				head_txid: Some(result.head_txid),
			},
		)),
		Err(err) => match depot_error(&err) {
			Some(SqliteStorageError::CommitTooLarge {
				actual_size_bytes,
				max_size_bytes,
			}) => Ok(protocol::SqliteCommitResponse::SqliteErrorResponse(
				protocol::SqliteErrorResponse {
					group: "depot".to_string(),
					code: "commit_too_large".to_string(),
					message: format!(
						"sqlite commit too large: actual_size_bytes={actual_size_bytes}, max_size_bytes={max_size_bytes}"
					),
				},
			)),
			_ => Err(err),
		},
	}?;
	perf_finish!(response_measure, fields: { response_kind = %sqlite_commit_response_kind(&response) });
	Ok(response)
}

fn sqlite_commit_response_kind(response: &protocol::SqliteCommitResponse) -> &'static str {
	match response {
		protocol::SqliteCommitResponse::SqliteCommitOk(_) => "ok",
		protocol::SqliteCommitResponse::SqliteErrorResponse(_) => "error",
	}
}

fn sqlite_get_pages_response_kind(response: &protocol::SqliteGetPagesResponse) -> &'static str {
	match response {
		protocol::SqliteGetPagesResponse::SqliteGetPagesOk(_) => "ok",
		protocol::SqliteGetPagesResponse::SqliteErrorResponse(_) => "error",
	}
}

fn sqlite_exec_response_kind(response: &protocol::SqliteExecResponse) -> &'static str {
	match response {
		protocol::SqliteExecResponse::SqliteExecOk(_) => "ok",
		protocol::SqliteExecResponse::SqliteErrorResponse(_) => "error",
	}
}

fn sqlite_execute_response_kind(response: &protocol::SqliteExecuteResponse) -> &'static str {
	match response {
		protocol::SqliteExecuteResponse::SqliteExecuteOk(_) => "ok",
		protocol::SqliteExecuteResponse::SqliteErrorResponse(_) => "error",
	}
}

fn sqlite_execute_batch_response_kind(
	response: &protocol::SqliteExecuteBatchResponse,
) -> &'static str {
	match response {
		protocol::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(_) => "ok",
		protocol::SqliteExecuteBatchResponse::SqliteErrorResponse(_) => "error",
	}
}

fn record_sqlite_request_metrics(
	conn: &Conn,
	request_type: &'static str,
	result: &'static str,
	start: Instant,
) {
	let elapsed = start.elapsed();
	metrics::SQLITE_REQUEST_TOTAL
		.with_label_values(&[
			conn.namespace_id.to_string().as_str(),
			conn.pool_name.as_str(),
			request_type,
			result,
		])
		.inc();
	metrics::SQLITE_REQUEST_DURATION
		.with_label_values(&[
			conn.namespace_id.to_string().as_str(),
			conn.pool_name.as_str(),
			request_type,
			result,
		])
		.observe(elapsed.as_secs_f64());
	if elapsed >= SLOW_SQLITE_REQUEST_WARN_THRESHOLD {
		tracing::warn!(
			namespace_id = %conn.namespace_id,
			pool_name = %conn.pool_name,
			request_type,
			result,
			duration_ms = elapsed.as_millis() as u64,
			"slow pegboard envoy sqlite request"
		);
	}
}

fn record_sqlite_payload_bytes(
	conn: &Conn,
	request_type: &'static str,
	direction: &'static str,
	bytes: usize,
) {
	if bytes == 0 {
		return;
	}
	metrics::SQLITE_REQUEST_PAYLOAD_BYTES
		.with_label_values(&[
			conn.namespace_id.to_string().as_str(),
			conn.pool_name.as_str(),
			request_type,
			direction,
		])
		.inc_by(u64::try_from(bytes).unwrap_or(u64::MAX));
}

async fn handle_remote_sqlite_exec(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteExecRequest,
) -> Result<QueryResult> {
	validate_remote_sqlite_actor(
		ctx,
		conn,
		&request.namespace_id,
		&request.actor_id,
		request.generation,
	)
	.await?;
	let actor_db = actor_db(ctx, conn, request.actor_id.clone()).await?;
	let database = remote_sqlite_executor_from_parts(
		&conn.remote_sqlite_executors,
		actor_db,
		&request.actor_id,
		request.generation,
	)
	.await?;
	database.exec(request.sql).await
}

async fn handle_remote_sqlite_execute(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteExecuteRequest,
) -> Result<ExecuteResult> {
	validate_remote_sqlite_actor(
		ctx,
		conn,
		&request.namespace_id,
		&request.actor_id,
		request.generation,
	)
	.await?;
	validate_remote_sqlite_params(request.params.as_ref())?;
	let params = request
		.params
		.map(|params| params.into_iter().map(bind_param_from_protocol).collect());
	let actor_db = actor_db(ctx, conn, request.actor_id.clone()).await?;
	let database = remote_sqlite_executor_from_parts(
		&conn.remote_sqlite_executors,
		actor_db,
		&request.actor_id,
		request.generation,
	)
	.await?;
	database.execute(request.sql, params).await
}

async fn handle_remote_sqlite_execute_batch(
	ctx: &StandaloneCtx,
	conn: &Conn,
	request: protocol::SqliteExecuteBatchRequest,
) -> Result<Vec<ExecuteResult>> {
	validate_remote_sqlite_actor(
		ctx,
		conn,
		&request.namespace_id,
		&request.actor_id,
		request.generation,
	)
	.await?;
	validate_remote_sqlite_batch(&request.statements)?;

	let actor_db = actor_db(ctx, conn, request.actor_id.clone()).await?;
	let database = remote_sqlite_executor_from_parts(
		&conn.remote_sqlite_executors,
		actor_db,
		&request.actor_id,
		request.generation,
	)
	.await?;
	database.exec("BEGIN".to_owned()).await?;
	let mut results = Vec::with_capacity(request.statements.len());
	for statement in request.statements {
		let params = statement
			.params
			.map(|params| params.into_iter().map(bind_param_from_protocol).collect());
		match database.execute(statement.sql, params).await {
			Ok(result) => results.push(result),
			Err(error) => {
				return match database.exec("ROLLBACK".to_owned()).await {
					Ok(_) => Err(error.context("execute remote sqlite batch statement")),
					Err(rollback_error) => Err(error
						.context("execute remote sqlite batch statement")
						.context(rollback_error.context("rollback remote sqlite batch"))),
				};
			}
		}
	}
	if let Err(error) = database.exec("COMMIT".to_owned()).await {
		return match database.exec("ROLLBACK".to_owned()).await {
			Ok(_) => Err(error.context("commit remote sqlite batch")),
			Err(rollback_error) => Err(error.context("commit remote sqlite batch").context(
				rollback_error.context("rollback remote sqlite batch after failed commit"),
			)),
		};
	}
	Ok(results)
}

async fn validate_remote_sqlite_actor(
	ctx: &StandaloneCtx,
	conn: &Conn,
	namespace_name: &str,
	actor_id: &str,
	generation: u64,
) -> Result<()> {
	if namespace_name != conn.namespace_name {
		bail!("actor does not exist");
	}
	validate_sqlite_actor(ctx, conn, actor_id).await?;
	validate_remote_sqlite_generation(ctx, conn, actor_id, generation).await
}

async fn validate_sqlite_actor(ctx: &StandaloneCtx, conn: &Conn, actor_id: &str) -> Result<()> {
	let actor_id = Id::parse(actor_id).context("invalid sqlite actor id")?;
	let actor = ctx
		.op(pegboard::ops::actor::get_for_kv::Input { actor_id })
		.await?
		.ok_or_else(|| anyhow::anyhow!("actor does not exist"))?;

	if actor.namespace_id != conn.namespace_id {
		bail!("actor does not exist");
	}

	Ok(())
}

async fn validate_sqlite_actor_for_request(
	ctx: &StandaloneCtx,
	conn: &Conn,
	actor_id: &str,
	expected_generation: Option<u64>,
) -> Result<()> {
	if let Some(generation) = expected_generation {
		validate_remote_sqlite_generation(ctx, conn, actor_id, generation).await
	} else {
		validate_sqlite_actor(ctx, conn, actor_id).await
	}
}

async fn validate_remote_sqlite_generation(
	ctx: &StandaloneCtx,
	conn: &Conn,
	actor_id: &str,
	generation: u64,
) -> Result<()> {
	let actor_id = Id::parse(actor_id).context("invalid sqlite actor id")?;
	let generation = u32::try_from(generation).context("invalid sqlite actor generation")?;
	let namespace_id = conn.namespace_id;
	let envoy_key = conn.envoy_key.clone();
	let (current_generation, has_pending_start_command) = ctx
		.udb()?
		.txn("envoy_check_pending_start_command", |tx| {
			let envoy_key = envoy_key.clone();
			async move {
				let tx = tx.with_subspace(pegboard::keys::subspace());
				let current_generation = tx
					.read_opt(
						&pegboard::keys::actor::GenerationKey::new(actor_id),
						Serializable,
					)
					.await?;

				if let Some(current_generation) = current_generation {
					Ok((Some(current_generation), false))
				} else {
					let active_generation = tx
						.read_opt(
							&pegboard::keys::envoy::ActorKey::new(
								namespace_id,
								envoy_key.clone(),
								actor_id,
							),
							Serializable,
						)
						.await?;

					let command_subspace = pegboard::keys::subspace().subspace(
						&pegboard::keys::envoy::ActorCommandKey::subspace_with_actor(
							namespace_id,
							envoy_key,
							actor_id,
							generation,
						),
					);
					let mut command_entries = tx.get_ranges_keyvalues(
						RangeOption {
							mode: StreamingMode::WantAll,
							..(&command_subspace).into()
						},
						Serializable,
					);
					let mut has_pending_start_command = false;
					while let Some(entry) = command_entries.try_next().await? {
						let (_, command) =
							tx.read_entry::<pegboard::keys::envoy::ActorCommandKey>(&entry)?;
						match command {
							protocol::ActorCommandKeyData::CommandStartActor(_) => {
								has_pending_start_command = true;
								break;
							}
							protocol::ActorCommandKeyData::CommandStopActor(_) => {}
						}
					}

					Ok((active_generation, has_pending_start_command))
				}
			}
		})
		.await?;

	if current_generation != Some(generation) && !has_pending_start_command {
		bail!("actor does not exist");
	}

	Ok(())
}

async fn remote_sqlite_executor_cell(
	executors: &RemoteSqliteExecutors,
	actor_id: &str,
	generation: u64,
) -> Arc<tokio::sync::OnceCell<NativeDatabaseHandle>> {
	let key = (actor_id.to_string(), generation);
	executors
		.entry_async(key)
		.await
		.or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
		.get()
		.clone()
}

async fn remote_sqlite_executor_from_parts(
	executors: &RemoteSqliteExecutors,
	actor_db: Arc<Db>,
	actor_id: &str,
	generation: u64,
) -> Result<NativeDatabaseHandle> {
	let cell = remote_sqlite_executor_cell(executors, actor_id, generation).await;
	let actor_id = actor_id.to_string();
	let database = cell
		.get_or_try_init(|| async move {
			open_database_from_embedded_depot(
				actor_db,
				actor_id,
				generation,
				tokio::runtime::Handle::current(),
				None,
			)
			.await
		})
		.await?;
	Ok(database.clone())
}

#[cfg(test)]
async fn remove_remote_sqlite_executor_generation(
	executors: &RemoteSqliteExecutors,
	actor_id: &str,
	generation: u64,
) {
	let _ = executors
		.remove_async(&(actor_id.to_string(), generation))
		.await;
}

#[cfg(test)]
fn remove_remote_sqlite_executors_for_actor(executors: &RemoteSqliteExecutors, actor_id: &str) {
	executors.retain_sync(|(entry_actor_id, _), _| entry_actor_id != actor_id);
}

#[cfg(test)]
fn clear_remote_sqlite_executors(executors: &RemoteSqliteExecutors) {
	executors.clear_sync();
}

fn validate_remote_sqlite_params(params: Option<&Vec<protocol::SqliteBindParam>>) -> Result<()> {
	let Some(params) = params else {
		return Ok(());
	};
	let total = bind_params_bytes(Some(params));
	if total > MAX_REMOTE_SQL_BIND_BYTES {
		bail!(
			"remote sqlite bind params had {total} bytes, exceeding limit {MAX_REMOTE_SQL_BIND_BYTES}"
		);
	}
	Ok(())
}

fn validate_remote_sqlite_batch(statements: &[protocol::SqliteBatchStatement]) -> Result<()> {
	for statement in statements {
		validate_remote_sqlite_params(statement.params.as_ref())?;
	}
	Ok(())
}

fn bind_params_bytes(params: Option<&Vec<protocol::SqliteBindParam>>) -> usize {
	params.map_or(0, |params| {
		params.iter().map(bind_param_bytes).sum::<usize>()
	})
}

fn bind_param_bytes(param: &protocol::SqliteBindParam) -> usize {
	match param {
		protocol::SqliteBindParam::SqliteValueNull => 0,
		protocol::SqliteBindParam::SqliteValueInteger(_) => std::mem::size_of::<i64>(),
		protocol::SqliteBindParam::SqliteValueFloat(_) => std::mem::size_of::<u64>(),
		protocol::SqliteBindParam::SqliteValueText(value) => value.value.len(),
		protocol::SqliteBindParam::SqliteValueBlob(value) => value.value.len(),
	}
}

fn pump_dirty_page(page: protocol::SqliteDirtyPage) -> depot::types::DirtyPage {
	depot::types::DirtyPage {
		pgno: page.pgno,
		bytes: page.bytes,
	}
}

fn bind_param_from_protocol(param: protocol::SqliteBindParam) -> BindParam {
	match param {
		protocol::SqliteBindParam::SqliteValueNull => BindParam::Null,
		protocol::SqliteBindParam::SqliteValueInteger(value) => BindParam::Integer(value.value),
		protocol::SqliteBindParam::SqliteValueFloat(value) => {
			BindParam::Float(f64::from_bits(u64::from_be_bytes(value.value)))
		}
		protocol::SqliteBindParam::SqliteValueText(value) => BindParam::Text(value.value),
		protocol::SqliteBindParam::SqliteValueBlob(value) => BindParam::Blob(value.value),
	}
}

fn protocol_query_result(result: QueryResult) -> protocol::SqliteQueryResult {
	protocol::SqliteQueryResult {
		columns: result.columns,
		rows: result
			.rows
			.into_iter()
			.map(|row| row.into_iter().map(protocol_column_value).collect())
			.collect(),
	}
}

fn protocol_execute_result(result: ExecuteResult) -> protocol::SqliteExecuteResult {
	protocol::SqliteExecuteResult {
		columns: result.columns,
		rows: result
			.rows
			.into_iter()
			.map(|row| row.into_iter().map(protocol_column_value).collect())
			.collect(),
		changes: result.changes,
		last_insert_row_id: result.last_insert_row_id,
	}
}

fn query_result_bytes(result: &protocol::SqliteQueryResult) -> usize {
	result.columns.iter().map(String::len).sum::<usize>()
		+ result
			.rows
			.iter()
			.flatten()
			.map(protocol_column_value_bytes)
			.sum::<usize>()
}

fn execute_result_bytes(result: &protocol::SqliteExecuteResult) -> usize {
	result.columns.iter().map(String::len).sum::<usize>()
		+ result
			.rows
			.iter()
			.flatten()
			.map(protocol_column_value_bytes)
			.sum::<usize>()
		+ std::mem::size_of::<u64>()
		+ std::mem::size_of::<i64>()
}

fn protocol_column_value_bytes(value: &protocol::SqliteColumnValue) -> usize {
	match value {
		protocol::SqliteColumnValue::SqliteValueNull => 0,
		protocol::SqliteColumnValue::SqliteValueInteger(_) => std::mem::size_of::<i64>(),
		protocol::SqliteColumnValue::SqliteValueFloat(_) => std::mem::size_of::<u64>(),
		protocol::SqliteColumnValue::SqliteValueText(value) => value.value.len(),
		protocol::SqliteColumnValue::SqliteValueBlob(value) => value.value.len(),
	}
}

fn protocol_column_value(value: ColumnValue) -> protocol::SqliteColumnValue {
	match value {
		ColumnValue::Null => protocol::SqliteColumnValue::SqliteValueNull,
		ColumnValue::Integer(value) => {
			protocol::SqliteColumnValue::SqliteValueInteger(protocol::SqliteValueInteger { value })
		}
		ColumnValue::Float(value) => {
			protocol::SqliteColumnValue::SqliteValueFloat(protocol::SqliteValueFloat {
				value: value.to_bits().to_be_bytes(),
			})
		}
		ColumnValue::Text(value) => {
			protocol::SqliteColumnValue::SqliteValueText(protocol::SqliteValueText { value })
		}
		ColumnValue::Blob(value) => {
			protocol::SqliteColumnValue::SqliteValueBlob(protocol::SqliteValueBlob { value })
		}
	}
}

async fn actor_db(ctx: &StandaloneCtx, conn: &Conn, actor_id: String) -> Result<Arc<Db>> {
	let compaction_disabled = ctx.config().sqlite().unstable_disable_compaction();
	let db = conn
		.actor_dbs
		.entry_async(actor_id.clone())
		.await
		.or_insert_with(|| {
			if compaction_disabled {
				return Arc::new(Db::new(
					conn.udb.clone(),
					conn.namespace_id,
					actor_id,
					conn.node_id,
				));
			}

			let compaction_signaler = Arc::new(move |_signal: DeltasAvailable| {
				async move {
					// TODO: Add back after enabling hot compaction
					// let tag_value = database_branch_tag_value(signal.database_branch_id);
					// let workflow_id = signal_ctx
					// 	.workflow(DbManagerInput {
					// 		database_branch_id: signal.database_branch_id,
					// 		actor_id: Some(actor_id),
					// 	})
					// 	.tag(DATABASE_BRANCH_ID_TAG, &tag_value)
					// 	.unique()
					// 	.dispatch()
					// 	.await?;
					// signal_ctx
					// 	.signal(signal)
					// 	.to_workflow_id(workflow_id)
					// 	.send()
					// 	.await?;
					Ok(())
				}
				.boxed()
			});

			Arc::new(Db::new_with_compaction_signaler(
				conn.udb.clone(),
				conn.namespace_id,
				actor_id,
				conn.node_id,
				compaction_signaler,
			))
		})
		.get()
		.clone();
	Ok(db)
}

fn depot_error(err: &anyhow::Error) -> Option<&SqliteStorageError> {
	err.chain()
		.find_map(|source| source.downcast_ref::<SqliteStorageError>())
}

fn is_startup_database_miss(
	err: &anyhow::Error,
	_expected_generation: Option<u64>,
	expected_head_txid: Option<u64>,
) -> bool {
	expected_head_txid.is_none()
		&& matches!(depot_error(err), Some(SqliteStorageError::DatabaseNotFound))
}

fn sqlite_error_reason(err: &anyhow::Error) -> String {
	err.chain()
		.map(ToString::to_string)
		.collect::<Vec<_>>()
		.join(": ")
}

fn sqlite_error_response(err: &anyhow::Error) -> protocol::SqliteErrorResponse {
	let structured = depot_error(err)
		.map(|err| rivet_error::RivetError::extract(&err.clone().build()))
		.unwrap_or_else(|| rivet_error::RivetError::extract(err));
	if is_head_fence_mismatch(structured.group(), structured.code()) {
		tracing::error!(
			error_group = structured.group(),
			error_code = structured.code(),
			"sqlite head fence mismatch from Depot; this indicates multiple actor instances are accessing the same sqlite database in parallel, which is incorrect actor lifecycle behavior"
		);
	}
	protocol::SqliteErrorResponse {
		group: structured.group().to_string(),
		code: structured.code().to_string(),
		message: sqlite_error_reason(err),
	}
}

fn sqlite_protocol_error_response(message: &str) -> protocol::SqliteErrorResponse {
	protocol::SqliteErrorResponse {
		group: "sqlite".to_string(),
		code: "invalid_request".to_string(),
		message: message.to_string(),
	}
}

/// Returns the length of the inner data payload for a tunnel message kind.
fn tunnel_message_inner_data_len(kind: &protocol::ToRivetTunnelMessageKind) -> usize {
	use protocol::ToRivetTunnelMessageKind;
	match kind {
		ToRivetTunnelMessageKind::ToRivetResponseStart(resp) => {
			resp.body.as_ref().map_or(0, |b| b.len())
		}
		ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => chunk.body.len(),
		ToRivetTunnelMessageKind::ToRivetWebSocketMessage(msg) => msg.data.len(),
		ToRivetTunnelMessageKind::ToRivetResponseAbort
		| ToRivetTunnelMessageKind::ToRivetWebSocketOpen(_)
		| ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(_)
		| ToRivetTunnelMessageKind::ToRivetWebSocketClose(_) => 0,
	}
}

#[cfg(test)]
#[path = "../tests/support/ws_to_tunnel_task.rs"]
mod tests;

async fn send_actor_kv_error(conn: &Conn, request_id: u32, message: &str) -> Result<()> {
	send_actor_kv_response(
		conn,
		request_id,
		protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
			message: message.to_string(),
		}),
		"KV actor validation error",
	)
	.await
}

async fn send_actor_kv_response(
	conn: &Conn,
	request_id: u32,
	data: protocol::KvResponseData,
	description: &str,
) -> Result<()> {
	let res_msg = versioned::ToEnvoy::wrap_latest(protocol::ToEnvoy::ToEnvoyKvResponse(
		protocol::ToEnvoyKvResponse { request_id, data },
	));

	let res_msg_serialized = res_msg
		.serialize(conn.protocol_version)
		.with_context(|| format!("failed to serialize {description}"))?;
	let _in_flight = WsResponseInFlightGuard::new();
	conn.ws_handle
		.send(Message::Binary(res_msg_serialized.into()))
		.await
		.with_context(|| format!("failed to send {description} to client"))?;

	Ok(())
}

pub(super) async fn send_sqlite_get_pages_response(
	conn: &Conn,
	request_id: u32,
	data: protocol::SqliteGetPagesResponse,
) -> Result<()> {
	send_to_envoy(
		conn,
		protocol::ToEnvoy::ToEnvoySqliteGetPagesResponse(protocol::ToEnvoySqliteGetPagesResponse {
			request_id,
			data,
		}),
		"sqlite get_pages response",
	)
	.await
}

pub(super) async fn send_sqlite_commit_response(
	conn: &Conn,
	request_id: u32,
	data: protocol::SqliteCommitResponse,
) -> Result<()> {
	send_to_envoy(
		conn,
		protocol::ToEnvoy::ToEnvoySqliteCommitResponse(protocol::ToEnvoySqliteCommitResponse {
			request_id,
			data,
		}),
		"sqlite commit response",
	)
	.await
}

pub(super) async fn send_sqlite_exec_response(
	conn: &Conn,
	request_id: u32,
	data: protocol::SqliteExecResponse,
) -> Result<()> {
	send_to_envoy(
		conn,
		protocol::ToEnvoy::ToEnvoySqliteExecResponse(protocol::ToEnvoySqliteExecResponse {
			request_id,
			data,
		}),
		"sqlite exec response",
	)
	.await
}

pub(super) async fn send_sqlite_execute_response(
	conn: &Conn,
	request_id: u32,
	data: protocol::SqliteExecuteResponse,
) -> Result<()> {
	send_to_envoy(
		conn,
		protocol::ToEnvoy::ToEnvoySqliteExecuteResponse(protocol::ToEnvoySqliteExecuteResponse {
			request_id,
			data,
		}),
		"sqlite execute response",
	)
	.await
}

pub(super) async fn send_sqlite_execute_batch_response(
	conn: &Conn,
	request_id: u32,
	data: protocol::SqliteExecuteBatchResponse,
) -> Result<()> {
	send_to_envoy(
		conn,
		protocol::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(
			protocol::ToEnvoySqliteExecuteBatchResponse { request_id, data },
		),
		"sqlite execute batch response",
	)
	.await
}

async fn send_to_envoy(conn: &Conn, msg: protocol::ToEnvoy, description: &str) -> Result<()> {
	let serialized = versioned::ToEnvoy::wrap_latest(msg)
		.serialize(conn.protocol_version)
		.with_context(|| format!("failed to serialize {description}"))?;
	let _in_flight = WsResponseInFlightGuard::new();
	conn.ws_handle
		.send(Message::Binary(serialized.into()))
		.await
		.with_context(|| format!("failed to send {description}"))?;

	Ok(())
}

/// RAII guard tracking responses currently being written to the envoy WebSocket.
/// Increments [`metrics::WS_RESPONSES_IN_FLIGHT`] on construction and decrements on drop,
/// so the gauge captures the time spent in `WebSocketHandle::send` (lock-wait plus network write).
pub(super) struct WsResponseInFlightGuard;

impl WsResponseInFlightGuard {
	pub(super) fn new() -> Self {
		metrics::WS_RESPONSES_IN_FLIGHT.inc();
		Self
	}
}

impl Drop for WsResponseInFlightGuard {
	fn drop(&mut self) {
		metrics::WS_RESPONSES_IN_FLIGHT.dec();
	}
}
