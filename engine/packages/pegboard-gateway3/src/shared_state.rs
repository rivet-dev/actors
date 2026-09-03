use anyhow::Result;
use gas::prelude::*;
use pegboard::pubsub_subjects::GatewayReceiverSubject;
use rivet_envoy_protocol::{self as protocol, PROTOCOL_VERSION, versioned};
use rivet_guard_core::errors::{
	RequestDeliveryUnconfirmed, TunnelMessageTimeout, WebSocketTunnelPingTimeout,
};
use scc::{HashMap, hash_map::Entry};
use std::{
	fmt,
	ops::Deref,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, mpsc, watch};
use universalpubsub::{NextOutput, PubSub, PublishOpts};
use vbare::OwnedVersionedData;

use crate::{
	WebsocketPendingLimitReached,
	http_stream::{HttpResponseQueueBudget, HttpResponseQueueOverloaded, HttpResponseQueuePermit},
	metrics,
};

#[derive(Debug, Clone, Copy)]
pub enum RequestProtocol {
	Http,
	WebSocket,
}

impl fmt::Display for RequestProtocol {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			RequestProtocol::Http => write!(f, "http"),
			RequestProtocol::WebSocket => write!(f, "websocket"),
		}
	}
}

pub(crate) fn display_id(id: &[u8]) -> DisplayId<'_> {
	DisplayId(id)
}

fn display_optional_id(id: Option<&[u8]>) -> DisplayOptionalId<'_> {
	DisplayOptionalId(id)
}

#[derive(Clone, Copy)]
pub(crate) struct DisplayId<'a>(&'a [u8]);

impl fmt::Display for DisplayId<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for byte in self.0 {
			write!(f, "{byte:02x}")?;
		}
		Ok(())
	}
}

struct DisplayOptionalId<'a>(Option<&'a [u8]>);

impl fmt::Display for DisplayOptionalId<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.0 {
			Some(id) => display_id(id).fmt(f),
			None => write!(f, "unknown"),
		}
	}
}

fn to_envoy_tunnel_message_kind_name(kind: &protocol::ToEnvoyTunnelMessageKind) -> &'static str {
	match kind {
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(_) => "ToEnvoyRequestStart",
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(_) => "ToEnvoyRequestChunk",
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(_) => "ToEnvoyRequestAbort",
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestBodyCancel => "ToEnvoyRequestBodyCancel",
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyResponseBodyWindowUpdate(_) => {
			"ToEnvoyResponseBodyWindowUpdate"
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(_) => "ToEnvoyWebSocketOpen",
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(_) => "ToEnvoyWebSocketMessage",
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(_) => "ToEnvoyWebSocketClose",
	}
}

fn to_rivet_tunnel_message_kind_name(kind: &protocol::ToRivetTunnelMessageKind) -> &'static str {
	match kind {
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_) => "ToRivetResponseStart",
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(_) => "ToRivetResponseChunk",
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_) => "ToRivetResponseAbort",
		protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyWindowUpdate(_) => {
			"ToRivetRequestBodyWindowUpdate"
		}
		protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyCancel => "ToRivetRequestBodyCancel",
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(_) => "ToRivetWebSocketOpen",
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(_) => "ToRivetWebSocketMessage",
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(_) => {
			"ToRivetWebSocketMessageAck"
		}
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(_) => "ToRivetWebSocketClose",
	}
}

fn retries_no_responders(
	request_protocol: RequestProtocol,
	message_kind: &protocol::ToEnvoyTunnelMessageKind,
) -> bool {
	match request_protocol {
		RequestProtocol::Http => matches!(
			message_kind,
			protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(_)
		),
		RequestProtocol::WebSocket => true,
	}
}

/// Threshold above which a single tunnel-ping RTT triggers a structured warn log.
/// Loaded once from `RIVET_GATEWAY3_SLOW_PING_THRESHOLD_MS` at process start; defaults
/// to 50 ms. The histogram captures the full RTT distribution regardless; this only
/// controls the structured-log signal that surfaces individual slow events for
/// post-hoc correlation with actor/envoy identity.
static SLOW_PING_THRESHOLD_MS: AtomicU64 = AtomicU64::new(50);

pub fn init_slow_ping_threshold_from_env() {
	if let Ok(raw) = std::env::var("RIVET_GATEWAY3_SLOW_PING_THRESHOLD_MS") {
		if let Ok(v) = raw.parse::<u64>() {
			SLOW_PING_THRESHOLD_MS.store(v, Ordering::Relaxed);
		}
	}
}

#[derive(Debug)]
pub enum MsgGcReason {
	/// Gateway channel is closed and is not hibernating
	GatewayClosed,
	/// WebSocket pending messages (ToRivetWebSocketMessageAck)
	WebSocketMessageNotAcked {
		#[allow(dead_code)]
		first_msg_index: u16,
		#[allow(dead_code)]
		last_msg_index: u16,
	},
	/// The gateway has not kept alive the in flight request during hibernation for the given timeout
	/// duration.
	HibernationTimeout,
	/// The HTTP response producer outran the gateway consumer.
	HttpResponseQueueOverloaded,
}

#[derive(Clone, Copy, Debug)]
pub enum RequestStopResult {
	Success,
	ClientDisconnect,
	ActorReadyTimeout,
	RequestTimeout,
	EnvoyError,
}

impl RequestStopResult {
	fn as_str(self) -> &'static str {
		match self {
			RequestStopResult::Success => "success",
			RequestStopResult::ClientDisconnect => "client_disconnect",
			RequestStopResult::ActorReadyTimeout => "actor_ready_timeout",
			RequestStopResult::RequestTimeout => "request_timeout",
			RequestStopResult::EnvoyError => "envoy_error",
		}
	}

	fn lifecycle_result(self) -> rivet_guard_core::metrics::PegboardGatewayResult {
		use rivet_guard_core::metrics::PegboardGatewayResult;
		match self {
			Self::Success => PegboardGatewayResult::Success,
			Self::ClientDisconnect => PegboardGatewayResult::ClientDisconnect,
			Self::ActorReadyTimeout => PegboardGatewayResult::ActorReadyTimeout,
			Self::RequestTimeout => PegboardGatewayResult::RequestTimeout,
			Self::EnvoyError => PegboardGatewayResult::EnvoyError,
		}
	}
}

pub struct SharedStateInner {
	ups: PubSub,
	gateway_id: protocol::GatewayId,
	receiver_subject: GatewayReceiverSubject,
	in_flight_requests: HashMap<protocol::RequestId, InFlightRequest>,
	hibernation_timeout: i64,
	// Config values
	gc_interval: Duration,
	tunnel_ping_timeout: i64,
	hws_message_ack_timeout: Duration,
	hws_max_pending_size: u64,
	buffered_http_response_max_bytes: usize,
	http_response_queue_max_messages: usize,
	streaming_http_response_queue_max_bytes: usize,
}

#[derive(Clone)]
pub struct SharedState(Arc<SharedStateInner>);

impl SharedState {
	pub fn new(config: &rivet_config::Config, ups: PubSub) -> Self {
		init_slow_ping_threshold_from_env();

		let gateway_id = protocol::util::generate_gateway_id();
		tracing::debug!(gateway_id = %display_id(&gateway_id), "setting up shared state for gateway");
		let receiver_subject = GatewayReceiverSubject::new(gateway_id);

		let pegboard_config = config.pegboard();
		Self(Arc::new(SharedStateInner {
			ups,
			gateway_id,
			receiver_subject,
			in_flight_requests: HashMap::new(),
			hibernation_timeout: pegboard_config.hibernating_request_eligible_threshold(),
			gc_interval: Duration::from_millis(pegboard_config.gateway_gc_interval_ms()),
			tunnel_ping_timeout: pegboard_config.gateway_tunnel_ping_timeout_ms(),
			hws_message_ack_timeout: Duration::from_millis(
				pegboard_config.gateway_hws_message_ack_timeout_ms(),
			),
			hws_max_pending_size: pegboard_config.gateway_hws_max_pending_size(),
			buffered_http_response_max_bytes: pegboard_config.envoy_max_response_payload_size(),
			http_response_queue_max_messages: pegboard_config
				.gateway_http_response_queue_max_messages(),
			streaming_http_response_queue_max_bytes: pegboard_config
				.gateway_streaming_http_response_queue_max_bytes()
				.min(protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as usize),
		}))
	}

	pub fn gateway_id(&self) -> protocol::GatewayId {
		self.gateway_id
	}

	#[tracing::instrument(skip_all)]
	pub async fn start(&self) -> Result<()> {
		let self_clone = self.clone();
		tokio::spawn(async move { self_clone.receiver().await });

		let self_clone = self.clone();
		tokio::spawn(async move { self_clone.gc().await });

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn receiver(&self) {
		// Automatically resubscribe if unsubscribed
		loop {
			tracing::debug!(
				gateway_id=%display_id(&self.gateway_id),
				receiver_subject=%self.receiver_subject,
				"subscribing to gateway receiver"
			);
			let mut sub = match self.ups.subscribe(&self.receiver_subject).await {
				Ok(sub) => sub,
				Err(err) => {
					tracing::error!(
						?err,
						"failed to open gateway subscription, retrying in 2 seconds"
					);
					tokio::time::sleep(Duration::from_secs(2)).await;
					continue;
				}
			};
			tracing::trace!(
				gateway_id=%display_id(&self.gateway_id),
				receiver_subject=%self.receiver_subject,
				"subscribed to gateway receiver"
			);

			loop {
				let raw_msg = match sub.next().await {
					Ok(NextOutput::Message(msg)) => msg,
					Ok(NextOutput::Unsubscribed) => {
						tracing::error!(
							"gateway subscription unsubscribed, in flight messages may be lost"
						);
						break;
					}
					Ok(NextOutput::NoResponders) => {
						tracing::error!(
							"gateway subscription no responders, in flight messages may be lost"
						);
						break;
					}
					Err(err) => {
						tracing::error!(
							?err,
							"gateway subscription errored, in flight messages may be lost"
						);
						break;
					}
				};
				tracing::trace!(
					gateway_id=%display_id(&self.gateway_id),
					receiver_subject=%self.receiver_subject,
					message_id=?raw_msg.message_id,
					payload_len=raw_msg.payload.len(),
					"received raw gateway message from pubsub"
				);

				let msg =
					match versioned::ToGateway::deserialize_with_embedded_version(&raw_msg.payload)
					{
						Ok(msg) => msg,
						Err(err) => {
							tracing::error!(?err, message_id=?raw_msg.message_id, "failed to parse message");
							continue;
						}
					};

				let request_id = match &msg {
					protocol::ToGateway::ToGatewayPong(pong) => pong.request_id,
					protocol::ToGateway::ToRivetTunnelMessage(msg) => msg.message_id.request_id,
				};
				let (gateway_id, message_index, message_kind, is_websocket_open) = match &msg {
					protocol::ToGateway::ToGatewayPong(_) => (None, None, "ToGatewayPong", false),
					protocol::ToGateway::ToRivetTunnelMessage(msg) => (
						Some(msg.message_id.gateway_id),
						Some(msg.message_id.message_index),
						to_rivet_tunnel_message_kind_name(&msg.message_kind),
						matches!(
							&msg.message_kind,
							protocol::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(_)
						),
					),
				};
				tracing::trace!(
					message_id=?raw_msg.message_id,
					payload_len=raw_msg.payload.len(),
					request_id=%display_id(&request_id),
					gateway_id=%display_optional_id(gateway_id.as_ref().map(|id| id.as_slice())),
					?message_index,
					message_kind,
					is_websocket_open,
					"decoded gateway message"
				);

				let Some(mut in_flight) = self.in_flight_requests.get_async(&request_id).await
				else {
					metrics::IN_FLIGHT_DROPPED_TOTAL
						.with_label_values(&["", "", "", "client_disconnect"])
						.inc();
					if is_websocket_open {
						tracing::warn!(
							gateway_id = %display_optional_id(gateway_id.as_ref().map(|id| id.as_slice())),
							request_id=%display_id(&request_id),
							?message_index,
							message_kind,
							message_id=?raw_msg.message_id,
							"in flight has already been disconnected, dropping websocket open"
						);
					} else {
						tracing::debug!(
							gateway_id = %display_optional_id(gateway_id.as_ref().map(|id| id.as_slice())),
							request_id=%display_id(&request_id),
							?message_index,
							message_kind,
							message_id=?raw_msg.message_id,
							"in flight has already been disconnected, dropping message"
						);
					}
					continue;
				};

				if is_websocket_open {
					tracing::debug!(
						gateway_id = %display_optional_id(gateway_id.as_ref().map(|id| id.as_slice())),
						request_id=%display_id(&request_id),
						?message_index,
						message_kind,
						message_id=?raw_msg.message_id,
						"received websocket open from envoy"
					);
				}
				tracing::trace!(
					gateway_id = %display_optional_id(gateway_id.as_ref().map(|id| id.as_slice())),
					request_id=%display_id(&request_id),
					?message_index,
					message_kind,
					message_id=?raw_msg.message_id,
					"delivering gateway message to request handler"
				);

				in_flight.recv_message(msg);
			}
		}
	}

	#[tracing::instrument(skip_all, fields(%receiver_subject, actor_key=?actor_key, actor_generation=?actor_generation, request_id=%display_id(&request_id)))]
	pub async fn create_or_wake_in_flight_request(
		&self,
		namespace_id: Id,
		actor_id: Id,
		pool_name: &str,
		actor_key: Option<String>,
		actor_generation: Option<u32>,
		envoy_protocol_version: Option<u16>,
		protocol: RequestProtocol,
		receiver_subject: String,
		request_id: protocol::RequestId,
		lifecycle: rivet_guard_core::metrics::PegboardGatewayLifecycle,
		after_hibernation: bool,
	) -> Result<InFlightRequestCtx> {
		let (msg_tx, msg_rx) = mpsc::unbounded_channel();
		let (drop_tx, drop_rx) = watch::channel(None);
		let (http_response_abort_tx, http_response_abort_rx) = watch::channel(None);
		let http_response_queue_budget = if matches!(protocol, RequestProtocol::Http) {
			Some(Arc::new(HttpResponseQueueBudget::new(
				self.buffered_http_response_max_bytes,
				self.http_response_queue_max_messages,
				self.streaming_http_response_queue_max_bytes,
			)))
		} else {
			None
		};

		let (new, send_lock, request_body_window, upload_cancel_tx) = match self
			.in_flight_requests
			.entry_async(request_id)
			.instrument(tracing::info_span!("entry_async"))
			.await
		{
			Entry::Vacant(entry) => {
				let send_lock = Arc::new(Mutex::new(()));
				let request_body_window = Arc::new(HttpBodySendWindow::new());
				let (upload_cancel_tx, _) = watch::channel(false);
				entry.insert_entry(InFlightRequest {
					namespace_id,
					actor_id,
					pool_name: pool_name.to_string(),
					actor_key,
					actor_generation,
					envoy_protocol_version,
					protocol,
					receiver_subject,
					message_index: 0,
					http_send_state: HttpSendState::PreCommit,
					abort_sent: false,
					http_response_abort_tx,
					send_lock: send_lock.clone(),
					request_body_window: request_body_window.clone(),
					upload_cancel_tx: upload_cancel_tx.clone(),
					lifecycle,
					created_at: Instant::now(),
					state: InFlightRequestState::Active {
						msg_tx,
						drop_tx,
						http_response_queue_budget,
						last_pong: util::timestamp::now(),
						hibernation_state: None,
					},
				});
				metrics::IN_FLIGHT
					.with_label_values(&[
						namespace_id.to_string().as_str(),
						pool_name,
						protocol.to_string().as_str(),
					])
					.inc();

				(true, send_lock, request_body_window, upload_cancel_tx)
			}
			// If the entry already exists it means we transition from hibernating to active
			Entry::Occupied(mut entry) => {
				let send_lock = entry.send_lock.clone();
				let request_body_window = entry.request_body_window.clone();
				let upload_cancel_tx = entry.upload_cancel_tx.clone();
				entry.http_response_abort_tx = http_response_abort_tx;
				entry.actor_id = actor_id;
				entry.actor_key = actor_key;
				entry.actor_generation = actor_generation;
				entry.envoy_protocol_version = envoy_protocol_version;
				entry.wake(
					receiver_subject,
					msg_tx,
					drop_tx,
					http_response_queue_budget,
				);

				(false, send_lock, request_body_window, upload_cancel_tx)
			}
		};

		ensure!(
			!after_hibernation || !new,
			"should not be creating a new in flight entry after hibernation"
		);

		Ok(InFlightRequestCtx {
			msg_rx,
			drop_rx,
			http_response_abort_rx,
			handle: InFlightRequestHandle {
				shared_state: self.clone(),
				request_id,
				send_lock,
				request_body_window,
				upload_cancel_tx,
			},
		})
	}

	#[tracing::instrument(skip_all, fields(actor_key=?actor_key, actor_generation=?actor_generation, request_id=%display_id(&request_id)))]
	pub async fn get_hibernating_in_flight_request(
		&self,
		namespace_id: Id,
		pool_name: &str,
		actor_key: Option<String>,
		actor_generation: Option<u32>,
		request_id: protocol::RequestId,
	) -> Result<InFlightRequestCtx> {
		let mut req = self
			.in_flight_requests
			.get_async(&request_id)
			.await
			.context("request not in flight")?;

		let (msg_tx, msg_rx) = mpsc::unbounded_channel();
		let (drop_tx, drop_rx) = watch::channel(None);
		let http_response_abort_rx = req.http_response_abort_tx.subscribe();
		let send_lock = req.send_lock.clone();
		let request_body_window = req.request_body_window.clone();
		let upload_cancel_tx = req.upload_cancel_tx.clone();

		req.hibernate(msg_tx, drop_tx, None);
		req.namespace_id = namespace_id;
		req.pool_name = pool_name.to_string();
		req.actor_key = actor_key;
		req.actor_generation = actor_generation;

		Ok(InFlightRequestCtx {
			msg_rx,
			drop_rx,
			http_response_abort_rx,
			handle: InFlightRequestHandle {
				shared_state: self.clone(),
				request_id,
				send_lock,
				request_body_window,
				upload_cancel_tx,
			},
		})
	}

	#[tracing::instrument(skip_all)]
	async fn gc(&self) {
		let mut interval = tokio::time::interval(self.gc_interval);
		interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

		loop {
			interval.tick().await;

			self.gc_in_flight_requests().await;
		}
	}

	#[tracing::instrument(skip_all)]
	async fn gc_in_flight_requests(&self) {
		let now = Instant::now();
		let hibernation_timeout =
			Duration::from_millis(self.hibernation_timeout.try_into().unwrap_or(90_000));

		self.in_flight_requests
			.retain_async(|request_id, req| {
				if let Some(reason) =
					req.expired(&self.hws_message_ack_timeout, &now, &hibernation_timeout)
				{
					tracing::debug!(
						request_id=%display_id(request_id),
						?reason,
						"gc removing in flight request"
					);

					match &req.state {
						InFlightRequestState::Active { drop_tx, .. } => {
							if drop_tx.send(Some(reason)).is_err() {
								tracing::debug!(
									request_id=%display_id(request_id),
									"failed to send gc reason msg to gateway",
								);
							}
						}
						InFlightRequestState::PendingHibernation { .. } => {}
						InFlightRequestState::Hibernating { drop_tx, .. } => {
							if drop_tx.send(Some(reason)).is_err() {
								tracing::debug!(
									request_id=%display_id(request_id),
									"failed to send gc reason msg to gateway",
								);
							}
						}
					}

					req.observe_terminal(RequestStopResult::RequestTimeout);
					metrics::IN_FLIGHT
						.with_label_values(&[
							req.namespace_id.to_string().as_str(),
							req.pool_name.as_str(),
							req.protocol.to_string().as_str(),
						])
						.dec();

					false
				} else {
					true
				}
			})
			.instrument(tracing::info_span!("retain_async"))
			.await;
	}
}

impl Deref for SharedState {
	type Target = SharedStateInner;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

pub struct InFlightRequestCtx {
	pub msg_rx: mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	/// Used to check if the request handler has been dropped.
	///
	/// This is separate from `msg_rx` there may still be messages that need to be sent to the
	/// request after `msg_rx` has dropped.
	pub drop_rx: watch::Receiver<Option<MsgGcReason>>,
	pub http_response_abort_rx: watch::Receiver<Option<protocol::HttpStreamAbortReason>>,
	pub handle: InFlightRequestHandle,
}

#[derive(Debug)]
struct HttpBodySendWindowState {
	sent_bytes: u64,
	consumed_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct HttpBodySendWindow {
	state: Mutex<HttpBodySendWindowState>,
	credit_available: Notify,
	outstanding_bytes: AtomicU64,
}

struct HttpRequestWindowStall(Instant);

impl Drop for HttpRequestWindowStall {
	fn drop(&mut self) {
		metrics::HTTP_BODY_WINDOW_STALL_DURATION_SECONDS
			.with_label_values(&["request"])
			.observe(self.0.elapsed().as_secs_f64());
	}
}

impl HttpBodySendWindow {
	fn new() -> Self {
		metrics::HTTP_BODY_WINDOW_BYTES
			.with_label_values(&["request"])
			.add(protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as i64);
		Self {
			state: Mutex::new(HttpBodySendWindowState {
				sent_bytes: 0,
				consumed_bytes: 0,
			}),
			credit_available: Notify::new(),
			outstanding_bytes: AtomicU64::new(0),
		}
	}

	pub(crate) async fn reserve(&self, bytes: u64) -> Result<()> {
		ensure!(
			bytes <= protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES,
			"HTTP body frame exceeds the flow-control window"
		);
		if bytes == 0 {
			return Ok(());
		}

		let mut stall = None;
		loop {
			let notified = self.credit_available.notified();
			{
				let mut state = self.state.lock().await;
				let outstanding = state
					.sent_bytes
					.checked_sub(state.consumed_bytes)
					.context("HTTP body flow-control accounting underflow")?;
				let available = protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES
					.checked_sub(outstanding)
					.context("HTTP body flow-control window exceeded")?;
				if bytes <= available {
					state.sent_bytes = state
						.sent_bytes
						.checked_add(bytes)
						.context("HTTP body sent-byte counter overflow")?;
					self.outstanding_bytes.fetch_add(bytes, Ordering::AcqRel);
					metrics::HTTP_BODY_BUFFERED_BYTES
						.with_label_values(&["request"])
						.add(bytes as i64);
					return Ok(());
				}
			}
			if stall.is_none() {
				metrics::HTTP_BODY_WINDOW_EXHAUSTED_TOTAL
					.with_label_values(&["request"])
					.inc();
				stall = Some(HttpRequestWindowStall(Instant::now()));
			}
			notified.await;
		}
	}

	pub(crate) async fn update_consumed(&self, consumed_bytes: u64) -> Result<()> {
		let mut state = self.state.lock().await;
		ensure!(
			consumed_bytes >= state.consumed_bytes,
			"HTTP body consumed-byte counter moved backwards"
		);
		ensure!(
			consumed_bytes <= state.sent_bytes,
			"HTTP body consumed-byte counter exceeds sent bytes"
		);
		if consumed_bytes == state.consumed_bytes {
			return Ok(());
		}
		let consumed_delta = consumed_bytes - state.consumed_bytes;
		state.consumed_bytes = consumed_bytes;
		drop(state);
		self.outstanding_bytes
			.fetch_sub(consumed_delta, Ordering::AcqRel);
		metrics::HTTP_BODY_BUFFERED_BYTES
			.with_label_values(&["request"])
			.sub(consumed_delta as i64);
		self.credit_available.notify_waiters();
		Ok(())
	}
}

impl Drop for HttpBodySendWindow {
	fn drop(&mut self) {
		let outstanding = self.outstanding_bytes.swap(0, Ordering::AcqRel);
		metrics::HTTP_BODY_BUFFERED_BYTES
			.with_label_values(&["request"])
			.sub(outstanding as i64);
		metrics::HTTP_BODY_WINDOW_BYTES
			.with_label_values(&["request"])
			.sub(protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as i64);
	}
}

#[derive(Debug)]
pub struct InFlightTunnelMessage {
	pub message_id: protocol::MessageId,
	pub message_kind: protocol::ToRivetTunnelMessageKind,
	_http_response_queue_permit: Option<HttpResponseQueuePermit>,
}

#[derive(Clone)]
pub struct InFlightRequestHandle {
	shared_state: SharedState,
	pub request_id: protocol::RequestId,
	send_lock: Arc<Mutex<()>>,
	request_body_window: Arc<HttpBodySendWindow>,
	upload_cancel_tx: watch::Sender<bool>,
}

impl InFlightRequestHandle {
	pub(crate) async fn begin_http_abort(&self) -> Option<(String, Option<u32>)> {
		let Some(mut request) = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
		else {
			return None;
		};
		if request.abort_sent {
			return None;
		}
		request.abort_sent = true;
		Some((request.actor_id.to_string(), request.actor_generation))
	}

	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id)))]
	pub async fn send_message(
		&self,
		message_kind: protocol::ToEnvoyTunnelMessageKind,
		ephemeral: bool,
	) -> Result<()> {
		// Admission to this FIFO mutex is the message handoff boundary. Once admitted, move the
		// guard and frame into an independent task so cancelling the caller cannot leave an index
		// gap or let a later abort/window frame overtake this message.
		let send_guard = self.send_lock.clone().lock_owned().await;
		let handle = self.clone();
		tokio::spawn(
			async move {
				handle
					.send_message_admitted(message_kind, ephemeral, send_guard)
					.await
			}
			.in_current_span(),
		)
		.await
		.context("gateway outbound message task failed")?
	}

	async fn send_message_admitted(
		&self,
		message_kind: protocol::ToEnvoyTunnelMessageKind,
		ephemeral: bool,
		_send_guard: tokio::sync::OwnedMutexGuard<()>,
	) -> Result<()> {
		let mut req = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
			.context("request not in flight")?;

		// Generate message ID
		let message_id = protocol::MessageId {
			gateway_id: self.shared_state.gateway_id,
			request_id: self.request_id,
			message_index: req.message_index,
		};
		let gateway_id = message_id.gateway_id;
		let request_id = message_id.request_id;
		let message_kind_name = to_envoy_tunnel_message_kind_name(&message_kind);
		let is_ws_open = matches!(
			&message_kind,
			protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(_)
		);
		let is_http = matches!(req.protocol, RequestProtocol::Http);
		let is_request_start = matches!(
			&message_kind,
			protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(_)
		);
		let is_request_abort = matches!(
			&message_kind,
			protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(_)
		);
		ensure!(
			!is_http
				|| match req.http_send_state {
					HttpSendState::PreCommit => is_request_start,
					HttpSendState::Committed => !is_request_start,
					HttpSendState::Indeterminate => is_request_abort,
					HttpSendState::Terminal => is_request_abort,
				},
			"HTTP tunnel message is invalid for the current send state"
		);
		let retry_no_responders = retries_no_responders(req.protocol, &message_kind);

		// HTTP advances only after the Envoy transport acknowledges admission. WebSocket keeps its
		// existing pre-send indexing because hibernation replay depends on that behavior.
		let current_message_index = req.message_index;
		if !is_http {
			req.message_index = req.message_index.wrapping_add(1);
		}

		// Check if this is a WebSocket message for hibernation tracking
		let is_ws_message = matches!(
			&message_kind,
			protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(_)
		);

		if is_ws_open {
			tracing::debug!(
				namespace_id = %req.namespace_id,
				pool_name = %req.pool_name,
				actor_key = ?req.actor_key,
				actor_generation = ?req.actor_generation,
				protocol = %req.protocol,
				receiver_subject = %req.receiver_subject,
				gateway_id = %display_id(&gateway_id),
				request_id = %display_id(&request_id),
				message_index = current_message_index,
				message_kind = message_kind_name,
				"sending websocket open to envoy"
			);
		}

		let payload = protocol::ToEnvoyTunnelMessage {
			message_id,
			message_kind,
		};

		let message = protocol::ToEnvoyConn::ToEnvoyTunnelMessage(payload);
		let message_serialized = versioned::ToEnvoyConn::wrap_latest(message)
			.serialize_with_embedded_version(PROTOCOL_VERSION)?;

		if let (Some(hs), true) = (req.hibernation_state_mut(), is_ws_message) {
			hs.total_pending_ws_msgs_size += message_serialized.len() as u64;

			if hs.total_pending_ws_msgs_size > self.shared_state.hws_max_pending_size
				|| hs.pending_ws_msgs.len() >= u16::MAX as usize
			{
				return Err(WebsocketPendingLimitReached {}.build());
			}

			let pending_ws_msg = PendingWebsocketMessage {
				payload: message_serialized.clone(),
				send_instant: Instant::now(),
				message_index: current_message_index,
			};

			hs.pending_ws_msgs.push(pending_ws_msg);
			tracing::debug!(
				index = current_message_index,
				new_count = hs.pending_ws_msgs.len(),
				"pushed pending websocket message"
			);
		}

		let receiver_subject = req.receiver_subject.clone();

		// TODO: Remove all this log and metric slop
		let namespace_id = req.namespace_id;
		let pool_name = req.pool_name.clone();
		let actor_key = req.actor_key.clone();
		let actor_generation = req.actor_generation.clone();
		let protocol = req.protocol.to_string();

		// Release lock before further async work
		drop(req);

		// Cap retries so a permanently-gone receiver fails fast instead of pinning the
		// request forever. Worst-case backoff total is ~19s, which stays under the default
		// tunnel ping timeout (30s) so the ping path can take over if the receiver is truly lost.
		let mut backoff = rivet_util::throttle::Backoff::new(6, Some(8), 100, 5);
		let first_attempt_at = Instant::now();
		let mut attempt = 0;
		loop {
			if !backoff.tick().await {
				if is_http
					&& let Some(mut req) = self
						.shared_state
						.in_flight_requests
						.get_async(&self.request_id)
						.await
				{
					req.http_send_state = HttpSendState::Terminal;
				}
				tracing::warn!(
					%namespace_id,
					%pool_name,
					?actor_key,
					?actor_generation,
					%protocol,
					%receiver_subject,
					gateway_id = %display_id(&gateway_id),
					request_id = %display_id(&request_id),
					message_index = current_message_index,
					message_kind = message_kind_name,
					"no responders for gateway message after retry budget exhausted, aborting"
				);
				return Err(TunnelMessageTimeout {
					phase: if is_http {
						"request_start"
					} else {
						"active_websocket"
					}
					.to_owned(),
					reason: "no_responders_after_retry_budget_exhausted".to_owned(),
				}
				.build());
			}
			attempt += 1;

			// NOTE: This is recorded before the response is received so its more accurate
			metrics::MSG_SENT_TOTAL
				.with_label_values(&[
					namespace_id.to_string().as_str(),
					&pool_name,
					if attempt == 1 { "init" } else { "retry" },
				])
				.inc();

			// Send tunnel msgs via request, this ensures if the other end is closed we get `NoResponders`
			tracing::trace!(
				%namespace_id,
				%pool_name,
				?actor_key,
				?actor_generation,
				%protocol,
				%receiver_subject,
				gateway_id = %display_id(&gateway_id),
				request_id = %display_id(&request_id),
				message_index = current_message_index,
				message_kind = message_kind_name,
				payload_len = message_serialized.len(),
				attempt,
				"sending gateway message to envoy"
			);
			let publish_timeout =
				Duration::from_millis(self.shared_state.tunnel_ping_timeout.max(1) as u64);
			let res = match tokio::time::timeout(
				publish_timeout,
				self.shared_state
					.ups
					.request(&receiver_subject, &message_serialized),
			)
			.await
			{
				Ok(Ok(res)) => res,
				Ok(Err(error)) => {
					if is_request_start {
						self.mark_request_start_indeterminate(current_message_index)
							.await?;
						tracing::warn!(
							?error,
							"request start handoff failed with unknown delivery status"
						);
						return Err(RequestDeliveryUnconfirmed {
							phase: "request_start".to_owned(),
							reason: "envoy_handoff_error".to_owned(),
						}
						.build());
					}
					if is_http && !is_request_start {
						metrics::HTTP_POST_COMMIT_FAILURE_TOTAL
							.with_label_values(&["ups_error"])
							.inc();
					}
					if is_http
						&& let Some(mut req) = self
							.shared_state
							.in_flight_requests
							.get_async(&self.request_id)
							.await
					{
						req.http_send_state = HttpSendState::Terminal;
					}
					return Err(error.into());
				}
				Err(_) => {
					if is_request_start {
						self.mark_request_start_indeterminate(current_message_index)
							.await?;
						return Err(RequestDeliveryUnconfirmed {
							phase: "request_start".to_owned(),
							reason: "envoy_handoff_ack_timeout".to_owned(),
						}
						.build());
					}
					if is_http && !is_request_start {
						metrics::HTTP_POST_COMMIT_FAILURE_TOTAL
							.with_label_values(&["handoff_timeout"])
							.inc();
					}
					if is_http
						&& let Some(mut req) = self
							.shared_state
							.in_flight_requests
							.get_async(&self.request_id)
							.await
					{
						req.http_send_state = HttpSendState::Terminal;
					}
					return Err(TunnelMessageTimeout {
						phase: if is_request_start {
							"request_start"
						} else {
							"committed_http"
						}
						.to_owned(),
						reason: "envoy_handoff_ack_timeout".to_owned(),
					}
					.build());
				}
			};

			if matches!(res, NextOutput::Unsubscribed) {
				if is_request_start {
					self.mark_request_start_indeterminate(current_message_index)
						.await?;
					return Err(RequestDeliveryUnconfirmed {
						phase: "request_start".to_owned(),
						reason: "envoy_handoff_subscription_closed".to_owned(),
					}
					.build());
				}
				if is_http && !is_request_start {
					metrics::HTTP_POST_COMMIT_FAILURE_TOTAL
						.with_label_values(&["subscription_closed"])
						.inc();
				}
				if is_http
					&& let Some(mut req) = self
						.shared_state
						.in_flight_requests
						.get_async(&self.request_id)
						.await
				{
					req.http_send_state = HttpSendState::Terminal;
				}
				return Err(TunnelMessageTimeout {
					phase: if is_http && !is_request_start {
						"committed_http"
					} else if is_http {
						"request_start"
					} else {
						"active_websocket"
					}
					.to_owned(),
					reason: "envoy_subscription_closed".to_owned(),
				}
				.build());
			} else if matches!(res, NextOutput::NoResponders) {
				// Ignore no responders
				if ephemeral {
					if is_http
						&& let Some(mut req) = self
							.shared_state
							.in_flight_requests
							.get_async(&self.request_id)
							.await
					{
						req.http_send_state = HttpSendState::Terminal;
					}
					tracing::debug!(
						%namespace_id,
						%pool_name,
						?actor_key,
						?actor_generation,
						%protocol,
						%receiver_subject,
						gateway_id = %display_id(&gateway_id),
						request_id = %display_id(&request_id),
						message_index = current_message_index,
						message_kind = message_kind_name,
						payload_len = message_serialized.len(),
						attempt,
						"no responders for gateway message, ignoring because message is ephemeral"
					);
					break;
				}
				if !retry_no_responders {
					if is_http && !is_request_start {
						metrics::HTTP_POST_COMMIT_FAILURE_TOTAL
							.with_label_values(&["no_responder"])
							.inc();
					}
					if let Some(mut req) = self
						.shared_state
						.in_flight_requests
						.get_async(&self.request_id)
						.await
					{
						req.http_send_state = HttpSendState::Terminal;
					}
					return Err(TunnelMessageTimeout {
						phase: "committed_http".to_owned(),
						reason: "no_responder_after_request_start".to_owned(),
					}
					.build());
				}

				metrics::REQUEST_RETRIES_TOTAL
					.with_label_values(&[
						namespace_id.to_string().as_str(),
						pool_name.as_str(),
						protocol.to_string().as_str(),
						attempt_bucket(attempt),
					])
					.inc();
				tracing::warn!(
					%namespace_id,
					%pool_name,
					?actor_key,
					?actor_generation,
					%protocol,
					%receiver_subject,
					gateway_id = %display_id(&gateway_id),
					request_id = %display_id(&request_id),
					message_index = current_message_index,
					message_kind = message_kind_name,
					attempt,
					elapsed_since_first_attempt_ms =
						first_attempt_at.elapsed().as_secs_f64() * 1000.0,
					"no responders for gateway message, retrying with backoff"
				);
			} else {
				if is_http {
					let mut req = self
						.shared_state
						.in_flight_requests
						.get_async(&self.request_id)
						.await
						.context("request removed during HTTP start handoff")?;
					ensure!(
						req.message_index == current_message_index,
						"HTTP message index changed during an admitted handoff"
					);
					req.message_index = req.message_index.wrapping_add(1);
					if is_request_start {
						req.http_send_state = HttpSendState::Committed;
					} else if is_request_abort {
						req.http_send_state = HttpSendState::Terminal;
					}
				}
				tracing::trace!(
					%namespace_id,
					%pool_name,
					?actor_key,
					?actor_generation,
					%protocol,
					%receiver_subject,
					gateway_id = %display_id(&gateway_id),
					request_id = %display_id(&request_id),
					message_index = current_message_index,
					message_kind = message_kind_name,
					payload_len = message_serialized.len(),
					attempt,
					elapsed_since_first_attempt_ms =
						first_attempt_at.elapsed().as_secs_f64() * 1000.0,
					"sent gateway message to envoy"
				);
				if is_ws_open {
					tracing::debug!(
						%namespace_id,
						%pool_name,
						?actor_key,
						?actor_generation,
						%protocol,
						%receiver_subject,
						gateway_id = %display_id(&gateway_id),
						request_id = %display_id(&request_id),
						message_index = current_message_index,
						message_kind = message_kind_name,
						attempt,
						elapsed_since_first_attempt_ms =
							first_attempt_at.elapsed().as_secs_f64() * 1000.0,
						"sent websocket open to envoy"
					);
				}
				break;
			}
		}
		if is_http && is_request_abort {
			if let Some(mut req) = self
				.shared_state
				.in_flight_requests
				.get_async(&self.request_id)
				.await
			{
				req.http_send_state = HttpSendState::Terminal;
			}
		}

		Ok(())
	}

	async fn mark_request_start_indeterminate(
		&self,
		current_message_index: protocol::MessageIndex,
	) -> Result<()> {
		let mut request = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
			.context("request removed during indeterminate HTTP handoff")?;
		ensure!(
			request.http_send_state == HttpSendState::PreCommit
				&& request.message_index == current_message_index,
			"HTTP request changed during indeterminate start handoff"
		);
		request.message_index = request.message_index.wrapping_add(1);
		request.http_send_state = HttpSendState::Indeterminate;
		Ok(())
	}

	pub(crate) async fn reserve_request_body_bytes(&self, bytes: usize) -> Result<()> {
		self.request_body_window.reserve(bytes as u64).await
	}

	pub(crate) async fn update_request_body_consumed(&self, consumed_bytes: u64) -> Result<()> {
		self.request_body_window
			.update_consumed(consumed_bytes)
			.await
	}

	pub(crate) fn subscribe_upload_cancel(&self) -> watch::Receiver<bool> {
		self.upload_cancel_tx.subscribe()
	}

	pub(crate) fn cancel_upload(&self) {
		self.upload_cancel_tx.send_replace(true);
	}

	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id)))]
	pub async fn send_and_check_ping(&self) -> Result<()> {
		let req = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
			.context("request not in flight")?;

		let last_pong = match &req.state {
			InFlightRequestState::Active { last_pong, .. } => *last_pong,
			InFlightRequestState::PendingHibernation { .. }
			| InFlightRequestState::Hibernating { .. } => {
				bail!("cannot check ping on hibernating req")
			}
		};

		let receiver_subject = req.receiver_subject.clone();

		// TODO: Remove all this log and metric slop
		let namespace_id = req.namespace_id;
		let pool_name = req.pool_name.clone();
		let actor_key = req.actor_key.clone();
		let actor_generation = req.actor_generation.clone();
		let protocol = req.protocol.to_string();

		// Release lock before further async work
		drop(req);

		let now = util::timestamp::now();

		// Verify ping timeout
		let last_pong_age_ms = now.saturating_sub(last_pong);
		metrics::LAST_PONG_AGE_SECONDS
			.with_label_values(&[
				namespace_id.to_string().as_str(),
				pool_name.as_str(),
				protocol.as_str(),
			])
			.observe(last_pong_age_ms.max(0) as f64 * 0.001);
		if last_pong_age_ms > self.shared_state.tunnel_ping_timeout {
			tracing::warn!(
				%namespace_id,
				%pool_name,
				?actor_key,
				?actor_generation,
				%protocol,
				%receiver_subject,
				last_pong_age_ms,
				timeout_ms = self.shared_state.tunnel_ping_timeout,
				"tunnel timeout"
			);
			return Err(WebSocketTunnelPingTimeout {
				timeout_ms: self.shared_state.tunnel_ping_timeout as u64,
				last_pong_age_ms: last_pong_age_ms as u64,
			}
			.build());
		}

		let message = protocol::ToEnvoyConn::ToEnvoyConnPing(protocol::ToEnvoyConnPing {
			gateway_id: self.shared_state.gateway_id,
			request_id: self.request_id,
			ts: now,
		});
		let message_serialized = versioned::ToEnvoyConn::wrap_latest(message)
			.serialize_with_embedded_version(PROTOCOL_VERSION)?;

		self.shared_state
			.ups
			.publish(&receiver_subject, &message_serialized, PublishOpts::one())
			.await?;

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id)))]
	pub async fn keepalive_hws(&self) -> Result<()> {
		let mut req = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
			.context("request not in flight")?;

		if let Some(hs) = req.hibernation_state_mut() {
			hs.last_ping = Instant::now();
		} else {
			tracing::warn!("should not call keepalive_hws for non-hibernating ws");
		}

		Ok(())
	}
	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id), %enable))]
	pub async fn toggle_hibernatable(&self, enable: bool) -> Result<()> {
		let mut req = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
			.context("request not in flight")?;

		match &mut req.state {
			InFlightRequestState::Active {
				hibernation_state, ..
			} => match (hibernation_state.is_some(), enable) {
				(true, true) => {}
				(true, false) => *hibernation_state = None,
				(false, true) => {
					*hibernation_state = Some(HibernationState {
						total_pending_ws_msgs_size: 0,
						pending_ws_msgs: Vec::new(),
						pending_tunnel_msgs: Vec::new(),
						last_ping: Instant::now(),
					});
				}
				(false, false) => {}
			},
			InFlightRequestState::PendingHibernation { .. }
			| InFlightRequestState::Hibernating { .. } => {
				tracing::warn!("cannot toggle hibernation on hibernating request");
			}
		}

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id)))]
	pub async fn resend_pending_websocket_messages(&self) -> Result<()> {
		let Some(mut req) = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
		else {
			bail!("request not in flight");
		};

		let receiver_subject = req.receiver_subject.clone();

		if let Some(hs) = req.hibernation_state_mut() {
			if !hs.pending_ws_msgs.is_empty() {
				tracing::debug!(len=?hs.pending_ws_msgs.len(), "resending pending messages");

				// NOTE: This holds a lock on the in flight req for the entire time we publish, but cloning ws
				// messages is expensive. We assume these publish calls are quick
				for pending_msg in &hs.pending_ws_msgs {
					self.shared_state
						.ups
						.publish(&receiver_subject, &pending_msg.payload, PublishOpts::one())
						.await?;
				}
			}
		}

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id)))]
	pub async fn has_pending_websocket_messages(&self) -> Result<bool> {
		let Some(req) = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
		else {
			bail!("request not in flight");
		};

		match &req.state {
			InFlightRequestState::Active {
				hibernation_state: Some(hibernation_state),
				..
			}
			| InFlightRequestState::PendingHibernation { hibernation_state }
			| InFlightRequestState::Hibernating {
				hibernation_state, ..
			} => Ok(!hibernation_state.pending_ws_msgs.is_empty()),
			_ => Ok(false),
		}
	}

	#[tracing::instrument(skip_all, fields(request_id=%display_id(&self.request_id), %ack_index))]
	pub async fn ack_pending_websocket_messages(&self, ack_index: u16) -> Result<()> {
		let Some(mut req) = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
		else {
			bail!("request not in flight");
		};

		let Some(hs) = req.hibernation_state_mut() else {
			tracing::warn!("cannot ack ws messages, hibernation is not enabled");
			return Ok(());
		};

		// Retain messages with index > ack_index (messages that haven't been acknowledged yet)
		let len_before = hs.pending_ws_msgs.len();
		hs.pending_ws_msgs
			.retain(|msg| wrapping_gt(msg.message_index, ack_index));

		let len_after = hs.pending_ws_msgs.len();
		tracing::debug!(
			removed_count = len_before - len_after,
			remaining_count = len_after,
			"acked pending websocket messages"
		);

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	pub async fn start_hibernation(&self) -> Result<()> {
		let Some(mut req) = self
			.shared_state
			.in_flight_requests
			.get_async(&self.request_id)
			.await
		else {
			bail!("request not in flight");
		};

		match &mut req.state {
			InFlightRequestState::Active {
				hibernation_state, ..
			} => {
				req.state = InFlightRequestState::PendingHibernation {
					hibernation_state: std::mem::take(hibernation_state)
						.context("should be hibernatable")?,
				};
			}
			InFlightRequestState::PendingHibernation { .. } => {
				tracing::warn!("request already hibernating");
			}
			InFlightRequestState::Hibernating { .. } => {
				tracing::warn!("request already hibernating");
			}
		}

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	pub async fn stop(&self, result: RequestStopResult) {
		// Serialize removal behind any admitted outbound handoff so commitment and message index
		// are recorded before terminal cleanup wins the race.
		let _send_guard = self.send_lock.clone().lock_owned().await;
		if let Some((_, req)) = self
			.shared_state
			.in_flight_requests
			.remove_async(&self.request_id)
			.await
		{
			tracing::trace!(
				namespace_id = %req.namespace_id,
				pool_name = %req.pool_name,
				actor_key = ?req.actor_key,
				actor_generation = ?req.actor_generation,
				protocol = %req.protocol,
				receiver_subject = %req.receiver_subject,
				request_id = %display_id(&self.request_id),
				result = result.as_str(),
				"stopping in flight request"
			);
			req.observe_terminal(result);
			metrics::IN_FLIGHT
				.with_label_values(&[
					req.namespace_id.to_string().as_str(),
					req.pool_name.as_str(),
					req.protocol.to_string().as_str(),
				])
				.dec();
		}
	}
}

fn attempt_bucket(attempt: u32) -> &'static str {
	match attempt {
		0 => "1",
		1 => "1",
		2 => "2",
		3 => "3",
		4..=u32::MAX => "4+",
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use rivet_error::RivetError;
	use universalpubsub::driver::memory::MemoryDriver;

	use super::*;

	fn request_start() -> protocol::ToEnvoyTunnelMessageKind {
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(protocol::ToEnvoyRequestStart {
			actor_id: "test-actor".into(),
			actor_generation: Some(1),
			method: "POST".into(),
			path: "/".into(),
			headers: Default::default(),
			body: None,
			stream: true,
			response_stream: true,
		})
	}

	#[test]
	fn http_retries_only_request_start_before_streaming() {
		assert!(retries_no_responders(
			RequestProtocol::Http,
			&request_start()
		));
		assert!(!retries_no_responders(
			RequestProtocol::Http,
			&protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				protocol::ToEnvoyRequestChunk {
					body: Vec::new(),
					finish: false,
				},
			),
		));
		assert!(!retries_no_responders(
			RequestProtocol::Http,
			&protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				protocol::ToEnvoyRequestAbort {
					actor_id: Some("test-actor".to_owned()),
					actor_generation: Some(1),
					reason: protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::Cancelled,
						detail: None,
					},
				},
			),
		));
	}

	#[test]
	fn websocket_preserves_existing_retry_policy() {
		assert!(retries_no_responders(
			RequestProtocol::WebSocket,
			&protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(
				protocol::ToEnvoyWebSocketClose {
					code: None,
					reason: None
				},
			),
		));
	}

	#[tokio::test]
	async fn delivered_request_start_ack_timeout_is_indeterminate_and_sends_exact_abort() {
		let ups = PubSub::new(Arc::new(MemoryDriver::new(
			"gateway3-indeterminate-request-start".to_owned(),
		)));
		let receiver_subject = "test.envoy.receiver".to_owned();
		let actor_id = Id::new_v1(2);
		let expected_actor_id = actor_id.to_string();
		let mut receiver = ups
			.subscribe(&receiver_subject)
			.await
			.expect("subscribe test Envoy receiver");
		let observer = tokio::spawn(async move {
			let NextOutput::Message(start) = receiver.next().await.expect("receive request start")
			else {
				panic!("expected request start");
			};
			let start = versioned::ToEnvoyConn::deserialize_with_embedded_version(&start.payload)
				.expect("decode request start");
			assert!(matches!(
				start,
				protocol::ToEnvoyConn::ToEnvoyTunnelMessage(protocol::ToEnvoyTunnelMessage {
					message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(_),
					..
				})
			));
			// Deliberately drop this handoff acknowledgement after observing delivery.

			let NextOutput::Message(abort_message) =
				receiver.next().await.expect("receive cancellation")
			else {
				panic!("expected request cancellation");
			};
			let decoded =
				versioned::ToEnvoyConn::deserialize_with_embedded_version(&abort_message.payload)
					.expect("decode request cancellation");
			let protocol::ToEnvoyConn::ToEnvoyTunnelMessage(protocol::ToEnvoyTunnelMessage {
				message_id,
				message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(abort),
			}) = decoded
			else {
				panic!("expected exact request cancellation");
			};
			assert_eq!(message_id.message_index, 1);
			assert_eq!(abort.actor_id.as_deref(), Some(expected_actor_id.as_str()));
			assert_eq!(abort.actor_generation, Some(1));
			abort_message
				.reply(&[])
				.await
				.expect("acknowledge request cancellation");
		});

		let mut root = rivet_config::config::Root::default();
		root.pegboard = Some(rivet_config::config::Pegboard {
			gateway_tunnel_ping_timeout_ms: Some(20),
			..Default::default()
		});
		let config = rivet_config::Config::from_root(root);
		let shared_state = SharedState::new(&config, ups);
		let request_id = [9, 8, 7, 6];
		let InFlightRequestCtx { handle, .. } = shared_state
			.create_or_wake_in_flight_request(
				Id::new_v1(1),
				actor_id,
				"test-pool",
				Some("test-key".to_owned()),
				Some(1),
				Some(7),
				RequestProtocol::Http,
				receiver_subject,
				request_id,
				rivet_guard_core::metrics::PegboardGatewayLifecycle::new(
					rivet_guard_core::metrics::PegboardGatewayVersion::V3,
					Some(7),
					rivet_guard_core::metrics::PegboardGatewayRequestKind::Http,
				),
				false,
			)
			.await
			.expect("create in-flight request");

		let error = handle
			.send_message(request_start(), false)
			.await
			.expect_err("lost handoff acknowledgement must fail");
		let error = RivetError::extract(&error);
		assert_eq!(error.group(), "guard");
		assert_eq!(error.code(), "request_delivery_unconfirmed");

		crate::http_stream::send_http_request_abort(
			&handle,
			protocol::HttpStreamAbortReasonKind::Cancelled,
			Some("request start delivery was indeterminate".to_owned()),
		)
		.await;

		observer.await.expect("join Envoy observer");
		let request = shared_state
			.in_flight_requests
			.get_async(&request_id)
			.await
			.expect("request remains until handler cleanup");
		assert_eq!(request.http_send_state, HttpSendState::Terminal);
		drop(request);
		handle.stop(RequestStopResult::EnvoyError).await;
		assert!(
			shared_state
				.in_flight_requests
				.get_async(&request_id)
				.await
				.is_none()
		);
	}

	#[tokio::test]
	async fn request_body_window_blocks_until_actor_consumption_is_acknowledged() {
		let window = Arc::new(HttpBodySendWindow::new());
		window
			.reserve(protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES)
			.await
			.expect("reserve initial request window");

		let blocked = tokio::spawn({
			let window = window.clone();
			async move { window.reserve(1).await }
		});
		assert!(
			tokio::time::timeout(Duration::from_millis(20), async {
				while !blocked.is_finished() {
					tokio::task::yield_now().await;
				}
			})
			.await
			.is_err(),
			"request body exceeded its initial byte window",
		);

		window
			.update_consumed(1)
			.await
			.expect("return one byte of request credit");
		blocked
			.await
			.expect("join blocked request reservation")
			.expect("reserve request byte after credit");
	}

	#[tokio::test]
	async fn request_body_window_rejects_regression_and_over_credit() {
		let window = HttpBodySendWindow::new();
		window.reserve(10).await.expect("reserve request bytes");
		window
			.update_consumed(5)
			.await
			.expect("consume request bytes");
		assert!(window.update_consumed(4).await.is_err());
		assert!(window.update_consumed(11).await.is_err());
		window
			.update_consumed(5)
			.await
			.expect("duplicate cumulative request credit");
	}

	#[tokio::test]
	async fn envoy_response_abort_has_an_out_of_band_delivery_signal() {
		let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
		let (drop_tx, _drop_rx) = watch::channel(None);
		let (http_response_abort_tx, mut http_response_abort_rx) = watch::channel(None);
		let (upload_cancel_tx, _upload_cancel_rx) = watch::channel(false);
		let reason = protocol::HttpStreamAbortReason {
			kind: protocol::HttpStreamAbortReasonKind::InternalError,
			detail: Some("Envoy session closed".to_owned()),
		};
		let message_id = protocol::MessageId {
			gateway_id: [1, 2, 3, 4],
			request_id: [5, 6, 7, 8],
			message_index: 2,
		};
		let mut request = InFlightRequest {
			namespace_id: Id::new_v1(1),
			actor_id: Id::new_v1(2),
			pool_name: "test".to_owned(),
			actor_key: Some("actor".to_owned()),
			actor_generation: Some(1),
			envoy_protocol_version: Some(7),
			protocol: RequestProtocol::Http,
			receiver_subject: "test".to_owned(),
			message_index: 0,
			http_send_state: HttpSendState::Committed,
			abort_sent: false,
			http_response_abort_tx,
			send_lock: Arc::new(Mutex::new(())),
			request_body_window: Arc::new(HttpBodySendWindow::new()),
			upload_cancel_tx,
			lifecycle: rivet_guard_core::metrics::PegboardGatewayLifecycle::new(
				rivet_guard_core::metrics::PegboardGatewayVersion::V3,
				Some(7),
				rivet_guard_core::metrics::PegboardGatewayRequestKind::Http,
			),
			created_at: Instant::now(),
			state: InFlightRequestState::Active {
				msg_tx,
				drop_tx,
				http_response_queue_budget: None,
				last_pong: util::timestamp::now(),
				hibernation_state: None,
			},
		};

		request.recv_message(protocol::ToGateway::ToRivetTunnelMessage(
			protocol::ToRivetTunnelMessage {
				message_id: message_id.clone(),
				message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
					protocol::ToRivetResponseAbort {
						reason: reason.clone(),
					},
				),
			},
		));

		let queued = msg_rx.recv().await.expect("ordered response abort");
		assert_eq!(queued.message_id.message_index, message_id.message_index);
		assert!(matches!(
			queued.message_kind,
			protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_)
		));
		http_response_abort_rx
			.changed()
			.await
			.expect("out-of-band response abort signal");
		assert!(matches!(
			http_response_abort_rx.borrow().as_ref(),
			Some(protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::InternalError,
				..
			})
		));
	}
}

struct InFlightRequest {
	namespace_id: Id,
	actor_id: Id,
	pool_name: String,
	actor_key: Option<String>,
	actor_generation: Option<u32>,
	envoy_protocol_version: Option<u16>,
	protocol: RequestProtocol,
	/// UPS subject to send messages to for this request.
	receiver_subject: String,
	/// Message index counter for this request.
	message_index: protocol::MessageIndex,
	http_send_state: HttpSendState,
	abort_sent: bool,
	http_response_abort_tx: watch::Sender<Option<protocol::HttpStreamAbortReason>>,
	send_lock: Arc<Mutex<()>>,
	request_body_window: Arc<HttpBodySendWindow>,
	upload_cancel_tx: watch::Sender<bool>,
	lifecycle: rivet_guard_core::metrics::PegboardGatewayLifecycle,
	created_at: Instant,
	state: InFlightRequestState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HttpSendState {
	PreCommit,
	Committed,
	Indeterminate,
	Terminal,
}

impl InFlightRequest {
	fn observe_terminal(&self, result: RequestStopResult) {
		self.lifecycle.finish(result.lifecycle_result());
		let result = result.as_str();
		metrics::REQUEST_DURATION_SECONDS
			.with_label_values(&[
				self.namespace_id.to_string().as_str(),
				self.pool_name.as_str(),
				self.protocol.to_string().as_str(),
				result,
			])
			.observe(self.created_at.elapsed().as_secs_f64());
	}

	fn hibernation_state_mut(&mut self) -> Option<&mut HibernationState> {
		match &mut self.state {
			InFlightRequestState::Active {
				hibernation_state, ..
			} => hibernation_state.as_mut(),
			InFlightRequestState::PendingHibernation { hibernation_state } => {
				Some(hibernation_state)
			}
			InFlightRequestState::Hibernating {
				hibernation_state, ..
			} => Some(hibernation_state),
		}
	}

	#[tracing::instrument(skip_all)]
	fn recv_message(&mut self, msg: protocol::ToGateway) {
		let http_response_abort = match &msg {
			protocol::ToGateway::ToRivetTunnelMessage(msg) => match &msg.message_kind {
				protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(abort) => {
					Some(abort.reason.clone())
				}
				_ => None,
			},
			_ => None,
		};
		match msg {
			protocol::ToGateway::ToGatewayPong(pong) => {
				match &mut self.state {
					InFlightRequestState::Active { last_pong, .. } => {
						let now = util::timestamp::now();
						*last_pong = now;

						let rtt = now.saturating_sub(pong.ts);
						let namespace_id_str = self.namespace_id.to_string();
						metrics::TUNNEL_PING_DURATION
							.with_label_values(&[
								namespace_id_str.as_str(),
								self.pool_name.as_str(),
								self.protocol.to_string().as_str(),
							])
							.observe(rtt as f64 * 0.001);

						// Slow-ping structured log. Threshold is intentionally well below the
						// tunnel_ping_timeout so we get an early-warning record per slow round
						// trip. Tunable via RIVET_GATEWAY3_SLOW_PING_THRESHOLD_MS, defaults to
						// 50 ms (~25× healthy baseline of 2 ms).
						let slow_threshold_ms = SLOW_PING_THRESHOLD_MS.load(Ordering::Relaxed);
						if (rtt as u64) > slow_threshold_ms {
							tracing::warn!(
								namespace_id = %self.namespace_id,
								pool_name = %self.pool_name,
								actor_key = ?self.actor_key,
								actor_generation = ?self.actor_generation,
								protocol = %self.protocol,
								receiver_subject = %self.receiver_subject,
								rtt_ms = rtt as u64,
								threshold_ms = slow_threshold_ms,
								"slow tunnel ping"
							);
						}
					}
					// Ignore pings during hibernation
					InFlightRequestState::PendingHibernation { .. }
					| InFlightRequestState::Hibernating { .. } => {}
				}
			}
			protocol::ToGateway::ToRivetTunnelMessage(msg) => match &mut self.state {
				InFlightRequestState::Active {
					msg_tx,
					drop_tx,
					http_response_queue_budget,
					hibernation_state,
					..
				} => {
					let returned_msg = forward_tunnel_message(
						&self.receiver_subject,
						self.actor_key.as_deref(),
						self.actor_generation,
						msg_tx,
						drop_tx,
						http_response_queue_budget.as_ref(),
						msg,
					);

					if let Some(returned_msg) = returned_msg {
						if let Some(hibernation_state) = hibernation_state {
							hibernation_state.pending_tunnel_msgs.push(returned_msg);
						}
					}
				}
				InFlightRequestState::PendingHibernation { hibernation_state } => {
					hibernation_state.pending_tunnel_msgs.push(msg);
				}
				InFlightRequestState::Hibernating {
					msg_tx,
					drop_tx,
					http_response_queue_budget,
					hibernation_state,
					..
				} => {
					let returned_msg = forward_tunnel_message(
						&self.receiver_subject,
						self.actor_key.as_deref(),
						self.actor_generation,
						msg_tx,
						drop_tx,
						http_response_queue_budget.as_ref(),
						msg,
					);

					if let Some(returned_msg) = returned_msg {
						hibernation_state.pending_tunnel_msgs.push(returned_msg);
					}
				}
			},
		}
		if let Some(reason) = http_response_abort {
			self.http_response_abort_tx.send_replace(Some(reason));
		}
	}

	/// Transition from hibernating to active
	fn wake(
		&mut self,
		receiver_subject: String,
		msg_tx: mpsc::UnboundedSender<InFlightTunnelMessage>,
		drop_tx: watch::Sender<Option<MsgGcReason>>,
		http_response_queue_budget: Option<Arc<HttpResponseQueueBudget>>,
	) {
		self.receiver_subject = receiver_subject;

		let mut pending_tunnel_msgs = Vec::new();

		// TODO: Kinda ugly but avoids clones and whatnot
		replace_with::replace_with_or_abort(&mut self.state, |state| match state {
			// Already active. This happens when guard retries the upstream request after
			// the original handler errored (e.g. the actor restarted mid-request) so a fresh
			// pair of channels arrives for the same request_id. Drop the previous
			// `msg_tx`/`drop_tx` (the old handler is already unwinding so its sender side is
			// abandoned) and adopt the new ones. `hibernation_state` is preserved so any
			// pending_ws_msgs the previous attempt accumulated stay attached to the request.
			// `last_pong` is reset to `now()` because we have no pong from the new handler
			// yet but we want to give the retry a fresh ping window.
			InFlightRequestState::Active {
				hibernation_state, ..
			} => InFlightRequestState::Active {
				msg_tx,
				drop_tx,
				http_response_queue_budget,
				last_pong: util::timestamp::now(),
				hibernation_state,
			},
			// Forward pending tunnel msgs if any
			InFlightRequestState::PendingHibernation {
				mut hibernation_state,
			}
			| InFlightRequestState::Hibernating {
				mut hibernation_state,
				..
			} => {
				pending_tunnel_msgs = std::mem::take(&mut hibernation_state.pending_tunnel_msgs);

				InFlightRequestState::Active {
					msg_tx,
					drop_tx,
					http_response_queue_budget,
					last_pong: util::timestamp::now(),
					hibernation_state: Some(hibernation_state),
				}
			}
		});

		// Should be active at this point
		if let InFlightRequestState::Active {
			msg_tx,
			drop_tx,
			http_response_queue_budget,
			..
		} = &self.state
		{
			for msg in pending_tunnel_msgs {
				forward_tunnel_message(
					&self.receiver_subject,
					self.actor_key.as_deref(),
					self.actor_generation,
					msg_tx,
					drop_tx,
					http_response_queue_budget.as_ref(),
					msg,
				);
			}
		}
	}

	/// Transition from pending hibernation to hibernating
	fn hibernate(
		&mut self,
		msg_tx: mpsc::UnboundedSender<InFlightTunnelMessage>,
		drop_tx: watch::Sender<Option<MsgGcReason>>,
		http_response_queue_budget: Option<Arc<HttpResponseQueueBudget>>,
	) {
		let mut pending_tunnel_msgs = Vec::new();

		// TODO: Kinda ugly but avoids clones and whatnot
		replace_with::replace_with_or_abort(&mut self.state, |state| match state {
			state @ InFlightRequestState::Active { .. } => {
				tracing::warn!(
					"should not be hibernating a request that isn't pending hibernation"
				);

				state
			}
			// Forward pending tunnel msgs
			InFlightRequestState::PendingHibernation {
				mut hibernation_state,
			} => {
				pending_tunnel_msgs = std::mem::take(&mut hibernation_state.pending_tunnel_msgs);

				InFlightRequestState::Hibernating {
					msg_tx,
					drop_tx,
					http_response_queue_budget,
					hibernation_state,
				}
			}
			state @ InFlightRequestState::Hibernating { .. } => {
				tracing::warn!("should not be hibernating already hibernating in flight req");

				state
			}
		});

		// Should be hibernating at this point
		if let InFlightRequestState::Hibernating {
			msg_tx,
			drop_tx,
			http_response_queue_budget,
			..
		} = &self.state
		{
			for msg in pending_tunnel_msgs {
				forward_tunnel_message(
					&self.receiver_subject,
					self.actor_key.as_deref(),
					self.actor_generation,
					msg_tx,
					drop_tx,
					http_response_queue_budget.as_ref(),
					msg,
				);
			}
		}
	}

	fn expired(
		&self,
		hws_message_ack_timeout: &Duration,
		now: &Instant,
		hibernation_timeout: &Duration,
	) -> Option<MsgGcReason> {
		match &self.state {
			InFlightRequestState::Active {
				hibernation_state: Some(hibernation_state),
				..
			}
			| InFlightRequestState::PendingHibernation { hibernation_state }
			| InFlightRequestState::Hibernating {
				hibernation_state, ..
			} => {
				if let Some(earliest_pending_ws_msg) = hibernation_state.pending_ws_msgs.first() {
					if &now.duration_since(earliest_pending_ws_msg.send_instant)
						> hws_message_ack_timeout
					{
						return Some(MsgGcReason::WebSocketMessageNotAcked {
							first_msg_index: earliest_pending_ws_msg.message_index,
							last_msg_index: self.message_index,
						});
					}
				}

				let hs_elapsed = hibernation_state.last_ping.elapsed();
				tracing::debug!(
					hs_elapsed=%hs_elapsed.as_secs_f64(),
					timeout=%hibernation_timeout.as_secs_f64(),
					"checking hibernating state elapsed time"
				);

				if &hs_elapsed > hibernation_timeout {
					return Some(MsgGcReason::HibernationTimeout);
				}

				None
			}
			InFlightRequestState::Active { msg_tx, .. } => {
				if msg_tx.is_closed() {
					Some(MsgGcReason::GatewayClosed)
				} else {
					None
				}
			}
		}
	}
}

enum InFlightRequestState {
	Active {
		/// Sender for incoming messages to this request.
		msg_tx: mpsc::UnboundedSender<InFlightTunnelMessage>,
		/// Used to check if the request handler has been dropped.
		drop_tx: watch::Sender<Option<MsgGcReason>>,
		http_response_queue_budget: Option<Arc<HttpResponseQueueBudget>>,
		last_pong: i64,
		hibernation_state: Option<HibernationState>,
	},
	/// In between active request handling and hibernation handling.
	PendingHibernation { hibernation_state: HibernationState },
	Hibernating {
		/// Sender for incoming messages to this request.
		msg_tx: mpsc::UnboundedSender<InFlightTunnelMessage>,
		/// Used to check if the hibernation handler has been dropped.
		drop_tx: watch::Sender<Option<MsgGcReason>>,
		http_response_queue_budget: Option<Arc<HttpResponseQueueBudget>>,
		hibernation_state: HibernationState,
	},
}

struct HibernationState {
	total_pending_ws_msgs_size: u64,
	/// Messages from the client that haven't been ack'd yet
	pending_ws_msgs: Vec<PendingWebsocketMessage>,
	/// Messages from the envoy that need to be forwarded to the client but can't yet because its hibernating
	pending_tunnel_msgs: Vec<protocol::ToRivetTunnelMessage>,
	// Used to keep hibernating websockets from being GC'd
	last_ping: Instant,
}

pub struct PendingWebsocketMessage {
	payload: Vec<u8>,
	send_instant: Instant,
	message_index: protocol::MessageIndex,
}

/// Returns `Some` if the message was not sent.
fn forward_tunnel_message(
	receiver_subject: &str,
	actor_key: Option<&str>,
	actor_generation: Option<u32>,
	msg_tx: &mpsc::UnboundedSender<InFlightTunnelMessage>,
	drop_tx: &watch::Sender<Option<MsgGcReason>>,
	http_response_queue_budget: Option<&Arc<HttpResponseQueueBudget>>,
	msg: protocol::ToRivetTunnelMessage,
) -> Option<protocol::ToRivetTunnelMessage> {
	let message_id = msg.message_id;
	// Send message to the request handler to emulate the real network action
	let inner_size = match &msg.message_kind {
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(ws_msg) => ws_msg.data.len(),
		_ => 0,
	};
	let http_response_queue_permit = match http_response_queue_budget
		.map(|budget| budget.try_reserve_message(&msg.message_kind))
		.transpose()
	{
		Ok(permit) => permit,
		Err(HttpResponseQueueOverloaded { bytes }) => {
			tracing::warn!(
				gateway_id=%display_id(&message_id.gateway_id),
				request_id=%display_id(&message_id.request_id),
				message_index=message_id.message_index,
				http_response_bytes = bytes,
				"actor response producer outpaced downstream delivery and filled the HTTP response queue"
			);
			drop_tx.send_replace(Some(MsgGcReason::HttpResponseQueueOverloaded));
			return None;
		}
	};
	tracing::debug!(
		gateway_id=%display_id(&message_id.gateway_id),
		request_id=%display_id(&message_id.request_id),
		message_index=message_id.message_index,
		actor_key,
		actor_generation,
		inner_size,
		"forwarding message to request handler"
	);

	let tunnel_msg = InFlightTunnelMessage {
		message_id: message_id.clone(),
		message_kind: msg.message_kind,
		_http_response_queue_permit: http_response_queue_permit,
	};

	if let Err(send_err) = msg_tx.send(tunnel_msg) {
		tracing::debug!(
			gateway_id=%display_id(&send_err.0.message_id.gateway_id),
			request_id=%display_id(&send_err.0.message_id.request_id),
			receiver_subject=%receiver_subject,
			actor_key,
			actor_generation,
			"message handler channel closed, saving to pending msgs",
		);

		Some(protocol::ToRivetTunnelMessage {
			message_id: send_err.0.message_id,
			message_kind: send_err.0.message_kind,
		})
	} else {
		tracing::trace!(
			gateway_id=%display_id(&message_id.gateway_id),
			request_id=%display_id(&message_id.request_id),
			message_index=message_id.message_index,
			actor_key,
			actor_generation,
			inner_size,
			"delivered message to request handler channel"
		);
		None
	}
}

fn wrapping_gt(a: u16, b: u16) -> bool {
	a != b && a.wrapping_sub(b) < u16::MAX / 2
}

// fn wrapping_lt(a: u16, b: u16) -> bool {
//     b.wrapping_sub(a) < u16::MAX / 2
// }
