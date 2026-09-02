use anyhow::Result;
use futures_util::TryStreamExt;
use gas::prelude::*;
use rivet_envoy_protocol as protocol;
use rivet_guard_core::websocket_handle::WebSocketReceiver;
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio_tungstenite::tungstenite::Message;

use super::LifecycleResult;
use crate::shared_state::{InFlightRequestHandle, display_id};

#[tracing::instrument(name = "ws_to_tunnel_task", skip_all)]
pub async fn task(
	ctx: StandaloneCtx,
	in_flight_req: InFlightRequestHandle,
	ws_rx: Arc<Mutex<WebSocketReceiver>>,
	ingress_bytes: Arc<AtomicU64>,
	mut ws_to_tunnel_abort_rx: watch::Receiver<()>,
) -> Result<LifecycleResult> {
	let mut ws_rx = ws_rx.lock().await;

	// Leaky bucket rate limit on consuming ws messages
	let mut rate_limit = rivet_util::throttle::RateLimiter::new(
		rivet_util::throttle::RateLimitMethod::LeakyBucket {
			requests: ctx
				.config()
				.pegboard()
				.gateway_websocket_rate_limit_requests(),
			drip_rate: Duration::from_millis(
				ctx.config()
					.pegboard()
					.gateway_websocket_rate_limit_drip_rate_ms(),
			),
		},
	);

	loop {
		tokio::select! {
			res = async {
				rate_limit.acquire().await;
				ws_rx.try_next().await
			} => {
				if let Some(msg) = res? {
					ingress_bytes.fetch_add(msg.len() as u64, Ordering::AcqRel);

					match msg {
						Message::Binary(data) => {
							let data_len = data.len();
							tracing::trace!(
								request_id=%display_id(&in_flight_req.request_id),
								data_len,
								binary = true,
								"received websocket message from client"
							);
							let ws_message =
								protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
									protocol::ToEnvoyWebSocketMessage {
										data: data.into(),
										binary: true,
									},
								);
							in_flight_req.send_message(ws_message, false).await?;
							tracing::trace!(
								request_id=%display_id(&in_flight_req.request_id),
								data_len,
								binary = true,
								"sent websocket message toward envoy"
							);
						}
						Message::Text(text) => {
							let data_len = text.as_bytes().len();
							tracing::trace!(
								request_id=%display_id(&in_flight_req.request_id),
								data_len,
								binary = false,
								"received websocket message from client"
							);
							let ws_message =
								protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
									protocol::ToEnvoyWebSocketMessage {
										data: text.as_bytes().to_vec(),
										binary: false,
									},
								);
							in_flight_req.send_message(ws_message, false).await?;
							tracing::trace!(
								request_id=%display_id(&in_flight_req.request_id),
								data_len,
								binary = false,
								"sent websocket message toward envoy"
							);
						}
						Message::Close(close) => {
							return Ok(LifecycleResult::ClientClose(close));
						}
						_ => {}
					}
				} else {
					tracing::debug!("websocket stream closed");
					return Ok(LifecycleResult::ClientClose(None));
				}
			}
			_ = ws_to_tunnel_abort_rx.changed() => {
				tracing::debug!("task aborted");
				return Ok(LifecycleResult::Aborted);
			}
		};
	}
}
