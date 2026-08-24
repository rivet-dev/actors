use anyhow::Result;
use gas::prelude::*;
use pegboard::pubsub_subjects::GatewayReceiverSubject;
use rivet_guard_core::errors::WebSocketTunnelPingTimeout;
use rivet_runner_protocol::{
	self as protocol, PROTOCOL_MK1_VERSION, PROTOCOL_MK2_VERSION, versioned,
};
use scc::{HashMap, hash_map::Entry};
use std::{
	ops::Deref,
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};
use universalpubsub::{NextOutput, PubSub, PublishOpts};
use vbare::OwnedVersionedData;

use crate::{WebsocketPendingLimitReached, metrics};

pub struct InFlightRequestHandle {
	pub msg_rx: mpsc::UnboundedReceiver<protocol::mk2::ToServerTunnelMessageKind>,
	/// Used to check if the request handler has been dropped.
	///
	/// This is separate from `msg_rx` there may still be messages that need to be sent to the
	/// request after `msg_rx` has dropped.
	pub drop_rx: watch::Receiver<Option<MsgGcReason>>,
	pub new: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFlightRequestState {
	AwaitingHttpResponseStart,
	AwaitingWebSocketOpen,
	ActiveWebSocket,
	Closed,
}

impl InFlightRequestState {
	fn accept_message(&mut self, message_kind: &protocol::mk2::ToServerTunnelMessageKind) -> bool {
		use protocol::mk2::ToServerTunnelMessageKind;

		match (self, message_kind) {
			(
				state @ InFlightRequestState::AwaitingHttpResponseStart,
				ToServerTunnelMessageKind::ToServerResponseStart(_)
				| ToServerTunnelMessageKind::ToServerResponseAbort,
			) => {
				*state = InFlightRequestState::Closed;
				true
			}
			(
				state @ InFlightRequestState::AwaitingWebSocketOpen,
				ToServerTunnelMessageKind::ToServerWebSocketOpen(_),
			) => {
				*state = InFlightRequestState::ActiveWebSocket;
				true
			}
			(
				state @ InFlightRequestState::AwaitingWebSocketOpen,
				ToServerTunnelMessageKind::ToServerWebSocketClose(_),
			)
			| (
				state @ InFlightRequestState::ActiveWebSocket,
				ToServerTunnelMessageKind::ToServerWebSocketClose(_),
			) => {
				*state = InFlightRequestState::Closed;
				true
			}
			(
				InFlightRequestState::ActiveWebSocket,
				ToServerTunnelMessageKind::ToServerWebSocketMessage(_)
				| ToServerTunnelMessageKind::ToServerWebSocketMessageAck(_),
			) => true,
			_ => false,
		}
	}
}

struct InFlightRequest {
	/// UPS subject to send messages to for this request.
	receiver_subject: String,
	protocol_version: u16,
	state: InFlightRequestState,
	/// Sender for incoming messages to this request.
	msg_tx: mpsc::UnboundedSender<protocol::mk2::ToServerTunnelMessageKind>,
	/// Used to check if the request handler has been dropped.
	drop_tx: watch::Sender<Option<MsgGcReason>>,
	/// True once first message for this request has been sent (so runner learned reply_to).
	opened: bool,
	/// Message index counter for this request.
	message_index: protocol::mk2::MessageIndex,
	hibernation_state: Option<HibernationState>,
	stopping: bool,
	last_pong: i64,
}

struct HibernationState {
	total_pending_ws_msgs_size: u64,
	pending_ws_msgs: Vec<PendingWebsocketMessage>,
	// Used to keep hibernating websockets from being GC'd
	last_ping: Instant,
}

pub struct PendingWebsocketMessage {
	payload: Vec<u8>,
	send_instant: Instant,
	message_index: protocol::mk2::MessageIndex,
}

#[derive(Debug)]
pub enum MsgGcReason {
	/// Gateway channel is closed and is not hibernating
	GatewayClosed,
	/// WebSocket pending messages (ToServerWebSocketMessageAck)
	WebSocketMessageNotAcked {
		#[allow(dead_code)]
		first_msg_index: u16,
		#[allow(dead_code)]
		last_msg_index: u16,
	},
	/// The gateway has not kept alive the in flight request during hibernation for the given timeout
	/// duration.
	HibernationTimeout,
}

pub struct SharedStateInner {
	ups: PubSub,
	gateway_id: protocol::mk2::GatewayId,
	receiver_subject: GatewayReceiverSubject,
	in_flight_requests: HashMap<protocol::mk2::RequestId, InFlightRequest>,
	hibernation_timeout: i64,
	// Config values
	gc_interval: Duration,
	tunnel_ping_timeout: i64,
	hws_message_ack_timeout: Duration,
	hws_max_pending_size: u64,
}

#[derive(Clone)]
pub struct SharedState(Arc<SharedStateInner>);

impl SharedState {
	pub fn new(config: &rivet_config::Config, ups: PubSub) -> Self {
		let gateway_id = protocol::util::generate_gateway_id();
		tracing::debug!(gateway_id = %protocol::util::id_to_string(&gateway_id), "setting up shared state for gateway");
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
		}))
	}

	pub fn gateway_id(&self) -> protocol::mk2::GatewayId {
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

	#[tracing::instrument(skip_all, fields(%receiver_subject, request_id=%protocol::util::id_to_string(&request_id)))]
	pub async fn start_in_flight_request(
		&self,
		receiver_subject: String,
		protocol_version: u16,
		request_id: protocol::mk2::RequestId,
		state: InFlightRequestState,
	) -> InFlightRequestHandle {
		let (msg_tx, msg_rx) = mpsc::unbounded_channel();
		let (drop_tx, drop_rx) = watch::channel(None);

		let new = match self.in_flight_requests.entry_async(request_id).await {
			Entry::Vacant(entry) => {
				entry.insert_entry(InFlightRequest {
					receiver_subject,
					protocol_version,
					state,
					msg_tx,
					drop_tx,
					opened: false,
					message_index: 0,
					hibernation_state: None,
					stopping: false,
					last_pong: util::timestamp::now(),
				});

				true
			}
			// If the entry already exists it means we transition from hibernating to active
			Entry::Occupied(mut entry) => {
				entry.receiver_subject = receiver_subject;
				entry.msg_tx = msg_tx;
				entry.drop_tx = drop_tx;
				entry.state = state;
				entry.opened = false;
				entry.last_pong = util::timestamp::now();

				if entry.stopping {
					entry.hibernation_state = None;
					entry.stopping = false;
				}

				false
			}
		};

		InFlightRequestHandle {
			msg_rx,
			drop_rx,
			new,
		}
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id)))]
	pub async fn send_message(
		&self,
		request_id: protocol::mk2::RequestId,
		message_kind: protocol::mk2::ToClientTunnelMessageKind,
	) -> Result<()> {
		let mut req = self
			.in_flight_requests
			.get_async(&request_id)
			.await
			.context("request not in flight")?;

		// Generate message ID
		let message_id = protocol::mk2::MessageId {
			gateway_id: self.gateway_id,
			request_id,
			message_index: req.message_index,
		};

		// Increment message index for next message
		let current_message_index = req.message_index;
		req.message_index = req.message_index.wrapping_add(1);

		let include_reply_to = !req.opened;
		if include_reply_to {
			// Mark as opened so subsequent messages skip reply_to
			req.opened = true;
		}

		// Check if this is a WebSocket message for hibernation tracking
		let is_ws_message = matches!(
			message_kind,
			protocol::mk2::ToClientTunnelMessageKind::ToClientWebSocketMessage(_)
		);

		let payload = protocol::mk2::ToClientTunnelMessage {
			message_id,
			message_kind,
		};

		let message_serialized = if protocol::is_mk2(req.protocol_version) {
			let message = protocol::mk2::ToRunner::ToClientTunnelMessage(payload);
			versioned::ToRunnerMk2::wrap_latest(message)
				.serialize_with_embedded_version(PROTOCOL_MK2_VERSION)?
		} else {
			let message = protocol::ToRunner::ToClientTunnelMessage(
				versioned::to_client_tunnel_message_mk2_to_mk1(payload),
			);
			versioned::ToRunner::wrap_latest(message)
				.serialize_with_embedded_version(PROTOCOL_MK1_VERSION)?
		};

		if let (Some(hs), true) = (&mut req.hibernation_state, is_ws_message) {
			hs.total_pending_ws_msgs_size += message_serialized.len() as u64;

			if hs.total_pending_ws_msgs_size > self.hws_max_pending_size
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

		self.ups
			.publish(
				&req.receiver_subject,
				&message_serialized,
				PublishOpts::one(),
			)
			.await?;

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id)))]
	pub async fn send_and_check_ping(&self, request_id: protocol::mk2::RequestId) -> Result<()> {
		let req = self
			.in_flight_requests
			.get_async(&request_id)
			.await
			.context("request not in flight")?;

		let now = util::timestamp::now();

		// Verify ping timeout
		let last_pong_age_ms = now.saturating_sub(req.last_pong);
		if last_pong_age_ms > self.tunnel_ping_timeout {
			tracing::warn!(runner_topic=%req.receiver_subject, "tunnel timeout");
			return Err(WebSocketTunnelPingTimeout {
				timeout_ms: self.tunnel_ping_timeout as u64,
				last_pong_age_ms: last_pong_age_ms as u64,
			}
			.build());
		}

		let message_serialized = if protocol::is_mk2(req.protocol_version) {
			let message = protocol::mk2::ToRunner::ToRunnerPing(protocol::mk2::ToRunnerPing {
				gateway_id: self.gateway_id,
				request_id,
				ts: now,
			});
			versioned::ToRunnerMk2::wrap_latest(message)
				.serialize_with_embedded_version(PROTOCOL_MK2_VERSION)?
		} else {
			let message = protocol::ToRunner::ToRunnerPing(protocol::ToRunnerPing {
				gateway_id: self.gateway_id,
				request_id,
				ts: now,
			});
			versioned::ToRunner::wrap_latest(message)
				.serialize_with_embedded_version(PROTOCOL_MK1_VERSION)?
		};

		self.ups
			.publish(
				&req.receiver_subject,
				&message_serialized,
				PublishOpts::one(),
			)
			.await?;

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id)))]
	pub async fn keepalive_hws(&self, request_id: protocol::mk2::RequestId) -> Result<()> {
		let mut req = self
			.in_flight_requests
			.get_async(&request_id)
			.await
			.context("request not in flight")?;

		if let Some(hs) = &mut req.hibernation_state {
			hs.last_ping = Instant::now();
		} else {
			tracing::warn!("should not call keepalive_hws for non-hibernating ws");
		}

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn receiver(&self) {
		// Automatically resubscribe if unsubscribed
		loop {
			tracing::debug!(gateway_id=%protocol::util::id_to_string(&self.gateway_id), "subscribing to gateway receiver");
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

			loop {
				let msg = match sub.next().await {
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
					payload_len = msg.payload.len(),
					"received message from pubsub"
				);

				match versioned::ToGateway::deserialize_with_embedded_version(&msg.payload) {
					Ok(protocol::mk2::ToGateway::ToGatewayPong(pong)) => {
						let Some(mut in_flight) =
							self.in_flight_requests.get_async(&pong.request_id).await
						else {
							tracing::debug!(
								request_id=%protocol::util::id_to_string(&pong.request_id),
								"in flight has already been disconnected, dropping ping"
							);
							continue;
						};

						let now = util::timestamp::now();
						in_flight.last_pong = now;

						let rtt = now.saturating_sub(pong.ts);
						metrics::TUNNEL_PING_DURATION.observe(rtt as f64 * 0.001);
					}
					Ok(protocol::mk2::ToGateway::ToServerTunnelMessage(msg)) => {
						let message_id = msg.message_id;

						let Some(mut in_flight) = self
							.in_flight_requests
							.get_async(&message_id.request_id)
							.await
						else {
							tracing::warn!(
								gateway_id=%protocol::util::id_to_string(&message_id.gateway_id),
								request_id=%protocol::util::id_to_string(&message_id.request_id),
								message_index=message_id.message_index,
								"in flight has already been disconnected, dropping message"
							);
							continue;
						};

						if !in_flight.state.accept_message(&msg.message_kind) {
							tracing::warn!(
								gateway_id=%protocol::util::id_to_string(&message_id.gateway_id),
								request_id=%protocol::util::id_to_string(&message_id.request_id),
								message_index=message_id.message_index,
								state=?in_flight.state,
								message_kind=?msg.message_kind,
								"dropping invalid tunnel message for request state"
							);
							continue;
						}

						// Send message to the request handler to emulate the real network action
						let inner_size = match &msg.message_kind {
							protocol::mk2::ToServerTunnelMessageKind::ToServerWebSocketMessage(
								ws_msg,
							) => ws_msg.data.len(),
							_ => 0,
						};
						tracing::debug!(
							gateway_id=%protocol::util::id_to_string(&message_id.gateway_id),
							request_id=%protocol::util::id_to_string(&message_id.request_id),
							message_index=message_id.message_index,
							inner_size,
							"forwarding message to request handler"
						);
						if in_flight.msg_tx.send(msg.message_kind.clone()).is_err() {
							tracing::warn!(
								gateway_id=%protocol::util::id_to_string(&message_id.gateway_id),
								request_id=%protocol::util::id_to_string(&message_id.request_id),
								receiver_subject=%in_flight.receiver_subject,
								"message handler channel closed",
							);
						}
					}
					Err(err) => {
						tracing::error!(?err, "failed to parse message");
					}
				}
			}
		}
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id), %enable))]
	pub async fn toggle_hibernation(
		&self,
		request_id: protocol::mk2::RequestId,
		enable: bool,
	) -> Result<()> {
		let mut req = self
			.in_flight_requests
			.get_async(&request_id)
			.await
			.context("request not in flight")?;

		match (req.hibernation_state.is_some(), enable) {
			(true, true) => {}
			(true, false) => req.hibernation_state = None,
			(false, true) => {
				req.hibernation_state = Some(HibernationState {
					total_pending_ws_msgs_size: 0,
					pending_ws_msgs: Vec::new(),
					last_ping: Instant::now(),
				});
			}
			(false, false) => {}
		}

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id)))]
	pub async fn resend_pending_websocket_messages(
		&self,
		request_id: protocol::mk2::RequestId,
	) -> Result<()> {
		let Some(mut req) = self.in_flight_requests.get_async(&request_id).await else {
			bail!("request not in flight");
		};

		let receiver_subject = req.receiver_subject.clone();

		if let Some(hs) = &mut req.hibernation_state {
			if !hs.pending_ws_msgs.is_empty() {
				tracing::debug!(len=?hs.pending_ws_msgs.len(), "resending pending messages");

				for pending_msg in &hs.pending_ws_msgs {
					self.ups
						.publish(&receiver_subject, &pending_msg.payload, PublishOpts::one())
						.await?;
				}
			}
		}

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id)))]
	pub async fn has_pending_websocket_messages(
		&self,
		request_id: protocol::mk2::RequestId,
	) -> Result<bool> {
		let Some(req) = self.in_flight_requests.get_async(&request_id).await else {
			bail!("request not in flight");
		};

		if let Some(hs) = &req.hibernation_state {
			Ok(!hs.pending_ws_msgs.is_empty())
		} else {
			Ok(false)
		}
	}

	#[tracing::instrument(skip_all, fields(request_id=%protocol::util::id_to_string(&request_id), %ack_index))]
	pub async fn ack_pending_websocket_messages(
		&self,
		request_id: protocol::mk2::RequestId,
		ack_index: u16,
	) -> Result<()> {
		let Some(mut req) = self.in_flight_requests.get_async(&request_id).await else {
			bail!("request not in flight");
		};

		let Some(hs) = &mut req.hibernation_state else {
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
	async fn gc(&self) {
		let mut interval = tokio::time::interval(self.gc_interval);
		interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

		loop {
			interval.tick().await;

			self.gc_in_flight_requests().await;
		}
	}

	/// This will remove all in flight requests that are cancelled or had an ack timeout.
	///
	/// Purging requests is done in a 2 phase commit in order to ensure that the InFlightRequest is
	/// kept until the ToClientWebSocketClose message has been successfully sent.
	///
	/// If we did not use a 2 phase commit (i.e. a single `retain` for any GC purge), the
	/// InFlightRequest would be removed immediately and the runner would never receive the
	/// ToClientWebSocketClose.
	///
	/// **Phase 1**
	///
	/// 1a. Find requests that need to be purged (either closed by gateway or message acknowledgement took too long)
	/// 1b. Flag the request as `stopping` to prevent re-purging this request in the next GC tick
	/// 1c. Send a `Timeout` message to `msg_tx` which will terminate the task in `handle_websocket`
	/// 1d. Once both tasks terminate, `handle_websocket` sends the `ToClientWebSocketClose` to the in flight request
	/// 1e. `handle_websocket` exits and drops `drop_rx`
	///
	/// **Phase 2**
	///
	/// 2a. Remove all requests where it was flagged as stopping and `drop_rx` has been dropped
	#[tracing::instrument(skip_all)]
	async fn gc_in_flight_requests(&self) {
		let now = Instant::now();
		let hibernation_timeout =
			Duration::from_millis(self.hibernation_timeout.try_into().unwrap_or(90_000));

		// First, check if an in flight req is beyond the timeout for tunnel message ack and websocket
		// message ack
		self.in_flight_requests
			.iter_mut_async(|mut entry| {
				let request_id = entry.key().clone();
				let req = &mut *entry;

				if req.stopping {
					return true;
				}

				let reason = 'reason: {
					if let Some(hs) = &req.hibernation_state {
						if let Some(earliest_pending_ws_msg) = hs.pending_ws_msgs.first() {
							if now.duration_since(earliest_pending_ws_msg.send_instant) > self.hws_message_ack_timeout {
								break 'reason Some(MsgGcReason::WebSocketMessageNotAcked {
									first_msg_index: earliest_pending_ws_msg.message_index,
									last_msg_index: req.message_index,
								});
							}
						}

						let hs_elapsed = hs.last_ping.elapsed();
						tracing::debug!(
							hs_elapsed=%hs_elapsed.as_secs_f64(),
							timeout=%hibernation_timeout.as_secs_f64(),
							"checking hibernating state elapsed time"
						);
						if hs_elapsed > hibernation_timeout {
							break 'reason Some(MsgGcReason::HibernationTimeout);
						}
					} else if req.msg_tx.is_closed() {
						break 'reason Some(MsgGcReason::GatewayClosed);
					}

					None
				};

				if let Some(reason) = reason {
					tracing::debug!(
						request_id=%protocol::util::id_to_string(&request_id),
						?reason,
						"gc stopping in flight request"
					);

					if req.drop_tx.send(Some(reason)).is_err() {
						tracing::debug!(request_id=%protocol::util::id_to_string(&request_id), "failed to send timeout msg to tunnel");
					}

					// Mark req as stopping to skip this loop next time the gc is run
					req.stopping = true;
				}

				true
			})
			.await;

		self.in_flight_requests
			.retain_async(|request_id, req| {
				// The reason we check for stopping here is because drop_tx could be dropped if we are
				// between websocket retries (we don't want to remove the in flight req in this case).
				// When the websocket reconnects a new channel will be created
				if req.stopping && req.drop_tx.is_closed() {
					tracing::debug!(
						request_id=%protocol::util::id_to_string(request_id),
						"gc removing in flight request"
					);

					return false;
				}

				true
			})
			.await;
	}
}

impl Deref for SharedState {
	type Target = SharedStateInner;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

fn wrapping_gt(a: u16, b: u16) -> bool {
	a != b && a.wrapping_sub(b) < u16::MAX / 2
}

#[cfg(test)]
mod tests {
	use super::InFlightRequestState;
	use rivet_runner_protocol as protocol;

	#[test]
	fn http_requests_only_accept_http_terminal_messages() {
		let mut state = InFlightRequestState::AwaitingHttpResponseStart;
		assert!(
			state.accept_message(&protocol::mk2::ToServerTunnelMessageKind::ToServerResponseAbort,)
		);
		assert_eq!(state, InFlightRequestState::Closed);

		let mut state = InFlightRequestState::AwaitingHttpResponseStart;
		assert!(!state.accept_message(
			&protocol::mk2::ToServerTunnelMessageKind::ToServerWebSocketMessage(
				protocol::mk2::ToServerWebSocketMessage {
					data: Vec::new(),
					binary: false,
				},
			),
		));
		assert_eq!(state, InFlightRequestState::AwaitingHttpResponseStart);
	}

	#[test]
	fn websockets_must_open_before_streaming() {
		let mut state = InFlightRequestState::AwaitingWebSocketOpen;
		assert!(!state.accept_message(
			&protocol::mk2::ToServerTunnelMessageKind::ToServerWebSocketMessage(
				protocol::mk2::ToServerWebSocketMessage {
					data: Vec::new(),
					binary: false,
				},
			),
		));
		assert_eq!(state, InFlightRequestState::AwaitingWebSocketOpen);

		assert!(state.accept_message(
			&protocol::mk2::ToServerTunnelMessageKind::ToServerWebSocketOpen(
				protocol::mk2::ToServerWebSocketOpen {
					can_hibernate: false,
				},
			),
		));
		assert_eq!(state, InFlightRequestState::ActiveWebSocket);
	}

	#[test]
	fn active_websockets_reject_http_messages() {
		let mut state = InFlightRequestState::ActiveWebSocket;
		assert!(
			!state
				.accept_message(&protocol::mk2::ToServerTunnelMessageKind::ToServerResponseAbort,)
		);
		assert_eq!(state, InFlightRequestState::ActiveWebSocket);
	}
}

// fn wrapping_lt(a: u16, b: u16) -> bool {
//     b.wrapping_sub(a) < u16::MAX / 2
// }
