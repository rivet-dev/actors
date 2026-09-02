use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use gas::prelude::*;
use rivet_envoy_protocol as protocol;
use rivet_guard_core::{
	WebSocketHandle,
	errors::{
		WebSocketGarbageCollected, WebSocketServiceHibernate, WebSocketTunnelSubscriptionClosed,
	},
};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use super::LifecycleResult;
use crate::shared_state::{InFlightRequestHandle, InFlightTunnelMessage, MsgGcReason, display_id};

#[tracing::instrument(name = "tunnel_to_ws_task", skip_all)]
pub async fn task(
	in_flight_req: InFlightRequestHandle,
	client_ws: WebSocketHandle,
	mut stopped_sub: message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	mut msg_rx: mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	mut drop_rx: watch::Receiver<Option<MsgGcReason>>,
	can_hibernate: bool,
	egress_bytes: Arc<AtomicU64>,
	mut tunnel_to_ws_abort_rx: watch::Receiver<()>,
) -> Result<LifecycleResult> {
	loop {
		tokio::select! {
			res = msg_rx.recv() => {
				if let Some(msg) = res {
					match msg.message_kind {
						protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(ws_msg) => {
							let data_len = ws_msg.data.len();
							let binary = ws_msg.binary;
							tracing::trace!(
								request_id=%display_id(&in_flight_req.request_id),
								data_len,
								binary,
								"forwarding websocket message to client"
							);
							let msg = if ws_msg.binary {
								Message::Binary(ws_msg.data.into())
							} else {
								Message::Text(
									String::from_utf8_lossy(&ws_msg.data).into_owned().into(),
								)
							};

							egress_bytes.fetch_add(msg.len() as u64, Ordering::AcqRel);
							client_ws.send(msg).await?;
							tracing::trace!(
								request_id=%display_id(&in_flight_req.request_id),
								data_len,
								binary,
								"sent websocket message to client"
							);
						}
						protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(ack) => {
							tracing::debug!(
								request_id=%display_id(&in_flight_req.request_id),
								ack_index=?ack.index,
								"received WebSocketMessageAck from envoy"
							);
							in_flight_req
								.ack_pending_websocket_messages(ack.index)
								.await?;
						}
						protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(close) => {
							tracing::debug!(?close, "server closed websocket");

							if can_hibernate && close.hibernate {
								return Err(WebSocketServiceHibernate.build());
							} else {
								// Successful closure
								return Ok(LifecycleResult::ServerClose(close));
							}
						}
						_ => {}
					}
				} else {
					tracing::warn!("tunnel sub closed");
					return Err(WebSocketTunnelSubscriptionClosed {
						phase: "active_websocket".to_owned(),
					}
					.build());
				}
			}
			_ = stopped_sub.next() => {
				tracing::debug!("actor stopped during websocket handler loop");

				if can_hibernate {
					return Err(WebSocketServiceHibernate.build());
				} else {
					return Ok(LifecycleResult::ServerClose(protocol::ToRivetWebSocketClose {
						code: Some(1000),
						reason: Some("actor.stopped".to_owned()),
						hibernate: false,
					}));
				}
			}
			_ = drop_rx.changed() => {
				tracing::warn!(reason=?drop_rx.borrow().as_ref(), "garbage collected");
				return Err(WebSocketGarbageCollected {
					phase: "active_websocket".to_owned(),
					reason: format!("{:?}", drop_rx.borrow().as_ref()),
				}
				.build());
			}
			_ = tunnel_to_ws_abort_rx.changed() => {
				tracing::debug!("task aborted");
				return Ok(LifecycleResult::Aborted);
			}
		}
	}
}
