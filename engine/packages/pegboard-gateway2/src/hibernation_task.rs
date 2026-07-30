use anyhow::Result;
use futures_util::TryStreamExt;
use gas::prelude::*;
use rivet_envoy_protocol as protocol;
use rivet_guard_core::{
	WebSocketHandle,
	errors::{WebSocketGarbageCollected, WebSocketTunnelSubscriptionClosed},
	websocket_handle::WebSocketReceiver,
};
use std::pin::Pin;
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::shared_state::{InFlightRequestHandle, InFlightTunnelMessage, MsgGcReason};

use super::HibernationLifecycleResult;

/// Peeks client ws until a message is received.
#[tracing::instrument(name = "hibernation_task", skip_all)]
pub async fn task(
	client_ws: WebSocketHandle,
	in_flight_req: InFlightRequestHandle,
	ctx: StandaloneCtx,
	actor_id: Id,
	mut msg_rx: mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	mut drop_rx: watch::Receiver<Option<MsgGcReason>>,
	egress_bytes: Arc<AtomicU64>,
	mut hibernation_abort_rx: watch::Receiver<()>,
) -> Result<HibernationLifecycleResult> {
	let mut ready_sub = ctx
		.subscribe::<pegboard::workflows::actor2::Ready>(("actor_id", actor_id))
		.await?;

	// Fetch actor info after sub to prevent race condition
	if let Some(actor) = ctx
		.op(pegboard::ops::actor::get_for_gateway::Input { actor_id })
		.await?
	{
		if actor.envoy_key.is_some() {
			tracing::debug!("actor became ready during hibernation");

			return Ok(HibernationLifecycleResult::Continue);
		}
	}

	let ws_rx = client_ws.recv();
	let mut guard = ws_rx.lock().await;
	let mut ws_rx = std::pin::Pin::new(&mut *guard);

	loop {
		tokio::select! {
			res = msg_rx.recv() => {
				if let Some(msg) = res {
					match msg.message_kind {
						protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(ws_msg) => {
							tracing::trace!(
								request_id=%protocol::util::id_to_string(&in_flight_req.request_id),
								data_len=ws_msg.data.len(),
								binary=ws_msg.binary,
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
						}
						protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(ack) => {
							tracing::debug!(
								request_id=%protocol::util::id_to_string(&in_flight_req.request_id),
								ack_index=?ack.index,
								"received WebSocketMessageAck from envoy"
							);
							in_flight_req
								.ack_pending_websocket_messages(ack.index)
								.await?;
						}
						_ => {}
					}
				} else {
					tracing::warn!("tunnel sub closed");
					return Err(WebSocketTunnelSubscriptionClosed {
						phase: "hibernating_websocket".to_owned(),
					}
					.build());
				}
			}
			_ = drop_rx.changed() => {
				tracing::warn!(reason=?drop_rx.borrow().as_ref(), "garbage collected");
				return Err(WebSocketGarbageCollected {
					phase: "hibernating_websocket".to_owned(),
					reason: format!("{:?}", drop_rx.borrow().as_ref()),
				}
				.build());
			}
			hibernation_res = peek_ws_during_hibernation(&mut ws_rx) => {
				let hibernation_res = hibernation_res?;

				match &hibernation_res {
					HibernationLifecycleResult::Continue => {
						tracing::debug!("received websocket message during hibernation");
					}
					HibernationLifecycleResult::Close => {
						tracing::debug!("websocket stream closed during hibernation");
					}
					HibernationLifecycleResult::Aborted => {}
				}

				return Ok(hibernation_res);
			}
			_ = ready_sub.next() => {
				tracing::debug!("actor became ready during hibernation");

				return Ok(HibernationLifecycleResult::Continue);
			}
			_ = hibernation_abort_rx.changed() => return Ok(HibernationLifecycleResult::Aborted),
		}
	}
}

#[tracing::instrument(skip_all)]
async fn peek_ws_during_hibernation(
	ws_rx: &mut Pin<&mut WebSocketReceiver>,
) -> Result<HibernationLifecycleResult> {
	loop {
		if let Some(msg) = ws_rx.as_mut().peek().await {
			match msg {
				Ok(Message::Binary(_)) | Ok(Message::Text(_)) => {
					return Ok(HibernationLifecycleResult::Continue);
				}
				Ok(Message::Close(_)) => return Ok(HibernationLifecycleResult::Close),
				// Ignore rest
				_ => {
					ws_rx.try_next().await?;
				}
			}
		} else {
			return Ok(HibernationLifecycleResult::Close);
		}
	}
}
