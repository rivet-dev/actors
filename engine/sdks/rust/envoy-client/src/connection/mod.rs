use std::sync::atomic::Ordering;

use rivet_envoy_protocol as protocol;
#[cfg(any(
	feature = "native-transport",
	all(feature = "wasm-transport", target_arch = "wasm32")
))]
use std::collections::HashMap;
use vbare::OwnedVersionedData;

use crate::context::{
	HttpConnectionTx, HttpWsTxMessage, SharedContext, WsConnectionTx, WsTxMessage, WsTxPayload,
};
#[cfg(any(
	feature = "native-transport",
	all(feature = "wasm-transport", target_arch = "wasm32")
))]
use crate::envoy::ToEnvoyMessage;
use crate::metrics::METRICS;
#[cfg(any(
	feature = "native-transport",
	all(feature = "wasm-transport", target_arch = "wasm32")
))]
use crate::stringify::stringify_to_envoy;
use crate::stringify::stringify_to_rivet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WsSendResult {
	Sent { session: u64 },
	Unavailable,
	StaleSession { current: Option<u64> },
}

pub(crate) const HTTP_WS_MESSAGE_CAPACITY: usize = 256;
pub(crate) const HTTP_WS_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
pub(crate) const WS_DATA_MESSAGE_CAPACITY: usize = 256;
pub(crate) const WS_CONTROL_MESSAGE_CAPACITY: usize = 32;
pub(crate) const WS_DATA_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
pub(crate) const WS_CONTROL_BYTE_CAPACITY: usize = 256 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum WsLane {
	Data,
	Control,
}

pub(crate) fn new_ws_connection() -> (
	WsConnectionTx,
	tokio::sync::mpsc::Receiver<WsTxMessage>,
	tokio::sync::mpsc::Receiver<WsTxMessage>,
) {
	let (data_tx, data_rx) = tokio::sync::mpsc::channel(WS_DATA_MESSAGE_CAPACITY);
	let (control_tx, control_rx) = tokio::sync::mpsc::channel(WS_CONTROL_MESSAGE_CAPACITY);
	let admission_gate = std::sync::Arc::new(std::sync::Mutex::new(()));
	(
		WsConnectionTx {
			data_tx,
			control_tx,
			data_byte_budget: std::sync::Arc::new(tokio::sync::Semaphore::new(
				WS_DATA_BYTE_CAPACITY,
			)),
			control_byte_budget: std::sync::Arc::new(tokio::sync::Semaphore::new(
				WS_CONTROL_BYTE_CAPACITY,
			)),
			closing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
			admission_gate,
		},
		data_rx,
		control_rx,
	)
}

impl WsConnectionTx {
	pub(crate) fn try_send(&self, lane: WsLane, data: Vec<u8>) -> Result<(), &'static str> {
		let _admission_guard = self
			.admission_gate
			.lock()
			.expect("websocket admission gate poisoned");
		if self.closing.load(Ordering::Acquire) {
			return Err("connection is closing");
		}
		let lane_label = lane.as_str();
		let Ok(byte_len) = u32::try_from(data.len()) else {
			METRICS
				.ws_tx_admission_failures_total
				.with_label_values(&[lane_label, "oversize"])
				.inc();
			return Err("message is too large");
		};
		let (tx, budget) = match lane {
			WsLane::Data => (&self.data_tx, &self.data_byte_budget),
			WsLane::Control => (&self.control_tx, &self.control_byte_budget),
		};
		let permit = budget
			.clone()
			.try_acquire_many_owned(byte_len)
			.map_err(|_| {
				METRICS
					.ws_tx_admission_failures_total
					.with_label_values(&[lane_label, "byte_budget"])
					.inc();
				"byte budget is saturated"
			})?;
		tx.try_send(WsTxMessage::Send(WsTxPayload {
			data,
			_byte_permit: permit,
		}))
		.map_err(|_| {
			METRICS
				.ws_tx_admission_failures_total
				.with_label_values(&[lane_label, "message_queue"])
				.inc();
			"message queue is saturated"
		})
	}

	pub(crate) fn try_close(&self) -> Result<(), &'static str> {
		let _admission_guard = self
			.admission_gate
			.lock()
			.expect("websocket admission gate poisoned");
		if self
			.closing
			.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
			.is_err()
		{
			return Ok(());
		}
		self.control_tx
			.try_send(WsTxMessage::Close)
			.map_err(|_| "control queue is saturated")
	}
}

impl WsLane {
	fn as_str(self) -> &'static str {
		match self {
			Self::Data => "data",
			Self::Control => "control",
		}
	}
}

#[cfg(test)]
pub(crate) async fn install_connection(shared: &SharedContext, tx: WsConnectionTx) -> u64 {
	let (http_tx, mut http_rx) =
		tokio::sync::mpsc::channel::<HttpWsTxMessage>(HTTP_WS_MESSAGE_CAPACITY);
	let http_byte_budget = std::sync::Arc::new(tokio::sync::Semaphore::new(HTTP_WS_BYTE_CAPACITY));
	let relay_tx = tx.clone();
	tokio::spawn(async move {
		while let Some(message) = http_rx.recv().await {
			let result = relay_tx
				.try_send(WsLane::Data, message.data)
				.map_err(anyhow::Error::msg);
			let _ = message.written.send(result);
		}
	});
	install_connection_with_http(shared, tx, http_tx, http_byte_budget).await
}

pub(crate) async fn install_connection_with_http(
	shared: &SharedContext,
	tx: WsConnectionTx,
	http_tx: tokio::sync::mpsc::Sender<HttpWsTxMessage>,
	http_byte_budget: std::sync::Arc<tokio::sync::Semaphore>,
) -> u64 {
	let mut guard = shared.ws_tx.lock().await;
	let mut http_guard = shared.http_ws_tx.lock().await;
	let session = shared
		.next_connection_session
		.fetch_add(1, Ordering::AcqRel)
		.saturating_add(1);
	shared.connection_session.store(session, Ordering::Release);
	let admission_gate = tx.admission_gate.clone();
	let closing = tx.closing.clone();
	*guard = Some(tx);
	*http_guard = Some(HttpConnectionTx {
		session,
		tx: http_tx,
		byte_budget: http_byte_budget,
		closing,
		admission_gate,
	});
	drop(http_guard);
	drop(guard);
	shared.connection_session_tx.send_replace(session);
	session
}

#[cfg(test)]
pub(crate) async fn remove_connection(shared: &SharedContext) {
	let session = shared.connection_session.load(Ordering::Acquire);
	remove_connection_for_session(shared, session).await;
}

pub(crate) async fn remove_connection_for_session(shared: &SharedContext, session: u64) {
	let mut guard = shared.ws_tx.lock().await;
	let mut http_guard = shared.http_ws_tx.lock().await;
	if shared.connection_session.load(Ordering::Acquire) != session {
		return;
	}
	*guard = None;
	*http_guard = None;
	shared.connection_session.store(0, Ordering::Release);
	drop(http_guard);
	drop(guard);
	shared.connection_session_tx.send_replace(0);
}

/// Sends a streaming HTTP frame on the exact WebSocket session that accepted its request start.
/// Success means the socket writer, not merely the local queue, accepted the frame.
pub(crate) async fn ws_send_http_for_session(
	shared: &SharedContext,
	message: protocol::ToRivet,
	expected_session: u64,
) -> WsSendResult {
	if tracing::enabled!(tracing::Level::DEBUG) {
		tracing::debug!(
			data = stringify_to_rivet(&message),
			"sending HTTP tunnel message"
		);
	}

	let encoded = crate::protocol::versioned::ToRivet::wrap_latest(message)
		.serialize(protocol::PROTOCOL_VERSION)
		.expect("failed to encode HTTP tunnel message");
	let Ok(encoded_len) = u32::try_from(encoded.len()) else {
		return WsSendResult::Unavailable;
	};
	if encoded.len() > HTTP_WS_BYTE_CAPACITY {
		return WsSendResult::Unavailable;
	}

	let connection = {
		let guard = shared.http_ws_tx.lock().await;
		let current = shared.connection_session.load(Ordering::Acquire);
		if current != expected_session {
			return WsSendResult::StaleSession {
				current: (current != 0).then_some(current),
			};
		}
		let Some(connection) = guard.as_ref() else {
			return WsSendResult::Unavailable;
		};
		if connection.session != expected_session {
			return WsSendResult::StaleSession {
				current: Some(connection.session),
			};
		}
		connection.clone()
	};
	let Ok(byte_permit) = connection
		.byte_budget
		.clone()
		.acquire_many_owned(encoded_len)
		.await
	else {
		return WsSendResult::Unavailable;
	};
	let (written, written_rx) = tokio::sync::oneshot::channel();
	let admitted = {
		let _admission_guard = connection
			.admission_gate
			.lock()
			.expect("websocket admission gate poisoned");
		if connection.closing.load(Ordering::Acquire) {
			false
		} else {
			connection
				.tx
				.try_send(HttpWsTxMessage {
					data: encoded,
					_byte_permit: byte_permit,
					written,
				})
				.is_ok()
		}
	};
	if !admitted {
		return WsSendResult::Unavailable;
	}

	match written_rx.await {
		Ok(Ok(())) => WsSendResult::Sent {
			session: expected_session,
		},
		Ok(Err(error)) => {
			tracing::debug!(?error, expected_session, "HTTP WebSocket write failed");
			WsSendResult::Unavailable
		}
		Err(_) => WsSendResult::Unavailable,
	}
}

#[cfg(all(feature = "native-transport", feature = "wasm-transport"))]
compile_error!(
	"`native-transport` and `wasm-transport` are mutually exclusive. Enable exactly one envoy-client transport."
);

#[cfg(not(any(feature = "native-transport", feature = "wasm-transport")))]
compile_error!(
	"rivet-envoy-client requires a WebSocket transport. Enable `native-transport` or `wasm-transport`."
);

#[cfg(feature = "native-transport")]
mod native;
#[cfg(feature = "wasm-transport")]
mod wasm;

#[cfg(feature = "native-transport")]
pub use native::start_connection;
#[cfg(feature = "wasm-transport")]
pub use wasm::start_connection;

#[cfg(any(
	feature = "native-transport",
	all(feature = "wasm-transport", target_arch = "wasm32")
))]
async fn send_initial_metadata(shared: &SharedContext) {
	let mut prepopulate_map = HashMap::new();
	for (name, actor) in &shared.config.prepopulate_actor_names {
		prepopulate_map.insert(
			name.clone(),
			protocol::ActorName {
				metadata: serde_json::to_string(&actor.metadata)
					.unwrap_or_else(|_| "{}".to_string()),
			},
		);
	}

	let metadata_json = shared
		.config
		.metadata
		.as_ref()
		.map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string()));

	ws_send(
		shared,
		protocol::ToRivet::ToRivetMetadata(protocol::ToRivetMetadata {
			prepopulate_actor_names: Some(prepopulate_map),
			metadata: metadata_json,
		}),
	)
	.await;
}

#[cfg(any(
	feature = "native-transport",
	all(feature = "wasm-transport", target_arch = "wasm32")
))]
async fn forward_to_envoy(shared: &SharedContext, session: u64, message: protocol::ToEnvoy) {
	if tracing::enabled!(tracing::Level::DEBUG) {
		tracing::debug!(data = stringify_to_envoy(&message), "received message");
	}

	match message {
		protocol::ToEnvoy::ToEnvoyPing(ping) => {
			shared
				.last_ping_ts
				.store(crate::time::now_millis(), Ordering::Release);
			ws_send(
				shared,
				protocol::ToRivet::ToRivetPong(protocol::ToRivetPong { ts: ping.ts }),
			)
			.await;
		}
		other => {
			let _ = crate::envoy::send_to_envoy_tx(
				shared,
				ToEnvoyMessage::ConnMessage {
					message: other,
					session,
				},
			);
		}
	}
}

/// Send a message over the WebSocket. Returns true if the message could not be sent.
pub async fn ws_send(shared: &SharedContext, message: protocol::ToRivet) -> bool {
	!matches!(
		ws_send_for_session(shared, message, None).await,
		WsSendResult::Sent { .. }
	)
}

/// Atomically checks a request's WebSocket affinity and admits it to the
/// writer. The session comparison deliberately happens while holding the same
/// mutex used by connect/disconnect. A coordinator-side check alone would have
/// a race where the old connection disappears immediately before the send and
/// the transaction statement is accidentally queued for its replacement.
pub(crate) async fn ws_send_for_session(
	shared: &SharedContext,
	message: protocol::ToRivet,
	expected_session: Option<u64>,
) -> WsSendResult {
	if tracing::enabled!(tracing::Level::DEBUG) {
		tracing::debug!(data = stringify_to_rivet(&message), "sending message");
	}

	let message_kind = to_rivet_kind(&message);
	let wait_start = crate::time::Instant::now();
	let guard = shared.ws_tx.lock().await;
	let wait_elapsed = wait_start.elapsed();
	METRICS
		.ws_tx_lock_wait_duration_seconds
		.with_label_values(&[message_kind])
		.observe(wait_elapsed.as_secs_f64());

	let hold_start = crate::time::Instant::now();
	let current = shared.connection_session.load(Ordering::Acquire);
	if let Some(expected) = expected_session
		&& current != expected
	{
		return WsSendResult::StaleSession {
			current: (current != 0).then_some(current),
		};
	}
	let Some(tx) = guard.as_ref() else {
		// Still observe hold duration on the early-return path.
		METRICS
			.ws_tx_lock_hold_duration_seconds
			.with_label_values(&[message_kind])
			.observe(hold_start.elapsed().as_secs_f64());
		tracing::error!("websocket not available for sending");
		return WsSendResult::Unavailable;
	};

	let encoded = crate::protocol::versioned::ToRivet::wrap_latest(message)
		.serialize(protocol::PROTOCOL_VERSION)
		.expect("failed to encode message");
	if tx.try_send(to_ws_lane(message_kind), encoded).is_err() {
		return WsSendResult::Unavailable;
	}
	drop(guard);
	METRICS
		.ws_tx_lock_hold_duration_seconds
		.with_label_values(&[message_kind])
		.observe(hold_start.elapsed().as_secs_f64());
	WsSendResult::Sent { session: current }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn data_saturation_does_not_block_control_messages() {
		let (tx, mut data_rx, mut control_rx) = new_ws_connection();

		tx.try_send(WsLane::Data, vec![0; WS_DATA_BYTE_CAPACITY])
			.expect("the first data message should consume the full data budget");
		assert!(tx.try_send(WsLane::Data, vec![1]).is_err());

		tx.try_send(WsLane::Control, vec![2])
			.expect("control traffic has a reserved byte budget");
		assert!(matches!(control_rx.try_recv(), Ok(WsTxMessage::Send(_))));
		assert!(matches!(data_rx.try_recv(), Ok(WsTxMessage::Send(_))));
	}

	#[test]
	fn control_queue_has_its_own_message_bound() {
		let (tx, _data_rx, mut control_rx) = new_ws_connection();

		for _ in 0..WS_CONTROL_MESSAGE_CAPACITY {
			tx.try_send(WsLane::Control, vec![0])
				.expect("control queue should accept up to its configured capacity");
		}
		assert!(tx.try_send(WsLane::Control, vec![1]).is_err());
		assert!(matches!(control_rx.try_recv(), Ok(WsTxMessage::Send(_))));
	}

	#[test]
	fn close_stops_new_admissions() {
		let (tx, _data_rx, mut control_rx) = new_ws_connection();

		tx.try_close().expect("close should be admitted");
		assert!(matches!(control_rx.try_recv(), Ok(WsTxMessage::Close)));
		assert!(tx.try_send(WsLane::Data, vec![0]).is_err());
		assert!(tx.try_send(WsLane::Control, vec![0]).is_err());
	}
}

/// Bounded label set for `ws_tx` send paths.
fn to_rivet_kind(message: &protocol::ToRivet) -> &'static str {
	match message {
		protocol::ToRivet::ToRivetMetadata(_) => "metadata",
		protocol::ToRivet::ToRivetEvents(_) => "events",
		protocol::ToRivet::ToRivetAckCommands(_) => "ack_commands",
		protocol::ToRivet::ToRivetStopping => "stopping",
		protocol::ToRivet::ToRivetPong(_) => "pong",
		protocol::ToRivet::ToRivetKvRequest(_) => "kv_request",
		protocol::ToRivet::ToRivetSqliteGetPagesRequest(_) => "sqlite_get_pages",
		protocol::ToRivet::ToRivetSqliteCommitRequest(_) => "sqlite_commit",
		protocol::ToRivet::ToRivetSqliteCommitStageBeginRequest(_) => "sqlite_commit_stage_begin",
		protocol::ToRivet::ToRivetSqliteCommitStageSegmentRequest(_) => {
			"sqlite_commit_stage_segment"
		}
		protocol::ToRivet::ToRivetSqliteCommitFinalizeRequest(_) => "sqlite_commit_finalize",
		protocol::ToRivet::ToRivetSqliteExecRequest(_) => "sqlite_exec",
		protocol::ToRivet::ToRivetSqliteExecuteRequest(_) => "sqlite_execute",
		protocol::ToRivet::ToRivetSqliteExecuteBatchRequest(_) => "sqlite_execute_batch",
		protocol::ToRivet::ToRivetTunnelMessage(_) => "tunnel_message",
	}
}

fn to_ws_lane(message_kind: &'static str) -> WsLane {
	match message_kind {
		"metadata" | "ack_commands" | "stopping" | "pong" => WsLane::Control,
		_ => WsLane::Data,
	}
}

#[cfg(any(
	feature = "native-transport",
	all(feature = "wasm-transport", target_arch = "wasm32")
))]
fn ws_url(shared: &SharedContext) -> String {
	let ws_endpoint = shared
		.config
		.endpoint
		.replace("http://", "ws://")
		.replace("https://", "wss://");
	let base_url = ws_endpoint.trim_end_matches('/');

	format!(
		"{}/envoys/connect?protocol_version={}&namespace={}&envoy_key={}&version={}&pool_name={}",
		base_url,
		protocol::PROTOCOL_VERSION,
		urlencoding::encode(&shared.config.namespace),
		urlencoding::encode(&shared.envoy_key),
		urlencoding::encode(&shared.config.version.to_string()),
		urlencoding::encode(&shared.config.pool_name),
	)
}
