use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bytes::Bytes;
use gas::prelude::*;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming as BodyIncoming};
use rivet_envoy_protocol as protocol;
use rivet_error::*;
use rivet_guard_core::{
	ResponseBody, WebSocketHandle,
	custom_serve::{CustomServeTrait, HibernationResult},
	errors::{
		ActorStoppedWhileWaitingForWebSocketOpen, WebSocketClosedBeforeOpen, WebSocketOpenDropped,
		WebSocketOpenResponseClosed, WebSocketOpenTimeout,
	},
	request_context::RequestContext,
	utils::is_ws_hibernate,
};
use std::{
	collections::HashMap,
	sync::{Arc, atomic::AtomicU64},
	time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::protocol::frame::{CloseFrame, coding::CloseCode};

use crate::shared_state::{
	InFlightRequestCtx, RequestProtocol, RequestStopResult, SharedState, display_id,
};

mod hibernation_task;
mod http_stream;
mod keepalive_task;
pub mod metrics;
mod metrics_task;
mod ping_task;
mod request_metrics;
pub mod shared_state;
mod tunnel_to_ws_task;
mod ws_to_tunnel_task;

use request_metrics::{RequestKind, RequestMetrics};

const UPDATE_METRICS_INTERVAL: Duration = Duration::from_secs(15);
const PHASE_PRE_WEBSOCKET_OPEN: &str = "pre_websocket_open";
const PHASE_WAITING_FOR_WEBSOCKET_OPEN: &str = "waiting_for_websocket_open";
const SLOW_WEBSOCKET_OPEN_WAIT_THRESHOLD: Duration = Duration::from_secs(1);

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_pending_limit_reached",
	"Reached limit on pending websocket messages, aborting connection."
)]
pub struct WebsocketPendingLimitReached;

#[derive(Debug)]
enum LifecycleResult {
	ServerClose(protocol::ToRivetWebSocketClose),
	ClientClose(Option<CloseFrame>),
	Aborted,
}

#[derive(Debug)]
enum HibernationLifecycleResult {
	Continue,
	Close,
	Aborted,
}

pub struct PegboardGateway3 {
	ctx: StandaloneCtx,
	shared_state: SharedState,
	lifecycle: rivet_guard_core::metrics::PegboardGatewayLifecycle,
	namespace_id: Id,
	pool_name: String,
	envoy_key: String,
	envoy_protocol_version: Option<u16>,
	actor_id: Id,
	actor_key: Option<String>,
	actor_generation: u32,
	path: String,
	prepared_http_request: tokio::sync::Mutex<Option<http_stream::PreparedHttpRequest>>,
}

impl PegboardGateway3 {
	#[tracing::instrument(skip_all, fields(?actor_id, actor_key=?actor_key, actor_generation=?actor_generation, ?namespace_id, %pool_name, %envoy_key, ?path))]
	pub fn new(
		ctx: StandaloneCtx,
		shared_state: SharedState,
		lifecycle: rivet_guard_core::metrics::PegboardGatewayLifecycle,
		namespace_id: Id,
		pool_name: String,
		envoy_key: String,
		envoy_protocol_version: Option<u16>,
		actor_id: Id,
		actor_key: Option<String>,
		actor_generation: u32,
		path: String,
	) -> Self {
		Self {
			ctx,
			shared_state,
			lifecycle,
			namespace_id,
			pool_name,
			envoy_key,
			envoy_protocol_version,
			actor_id,
			actor_key,
			actor_generation,
			path,
			prepared_http_request: tokio::sync::Mutex::new(None),
		}
	}
}

impl PegboardGateway3 {
	async fn handle_websocket_inner(
		&self,
		ctx: &StandaloneCtx,
		req_ctx: &mut RequestContext,
		client_ws: WebSocketHandle,
		after_hibernation: bool,
	) -> Result<Option<CloseFrame>> {
		let actor_generation = self.actor_generation;
		let request_id = req_ctx.in_flight_request_id()?;
		let gateway_id = self.shared_state.gateway_id();

		// Extract headers
		let mut request_headers = HashMap::new();
		for (name, value) in req_ctx.headers() {
			if let Result::Ok(value_str) = value.to_str() {
				request_headers.insert(name.to_string(), value_str.to_string());
			}
		}

		let mut stopped_sub = ctx
			.subscribe::<pegboard::workflows::actor2::Stopped>(("actor_id", self.actor_id))
			.await?;

		// Build subject to publish to
		let tunnel_subject = pegboard::pubsub_subjects::EnvoyReceiverSubject::new(
			self.namespace_id,
			self.envoy_key.clone(),
		)
		.to_string();

		// Start listening for WebSocket messages
		let InFlightRequestCtx {
			mut msg_rx,
			mut drop_rx,
			http_response_abort_rx: _,
			handle: in_flight_req,
		} = self
			.shared_state
			.create_or_wake_in_flight_request(
				self.namespace_id,
				self.actor_id,
				self.pool_name.as_str(),
				self.actor_key.clone(),
				Some(actor_generation),
				self.envoy_protocol_version,
				RequestProtocol::WebSocket,
				tunnel_subject.clone(),
				request_id,
				self.lifecycle.clone(),
				after_hibernation,
			)
			.await?;

		let res = async {
			// Envoy restores and rebinds a hibernating websocket before sending the open
			// acknowledgment. Do not resume client forwarding until that acknowledgment arrives.
			if !after_hibernation {
				// Send WebSocket open message
				let open_message = protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
					protocol::ToEnvoyWebSocketOpen {
						actor_id: self.actor_id.to_string(),
						actor_generation: Some(actor_generation),
						path: self.path.clone(),
						headers: request_headers,
					},
				);

				tokio::select! {
					// Prefer quick stop path
					biased;
					_ = stopped_sub.next() => {
						tracing::debug!("actor stopped while waiting for websocket open");
						return Err(ActorStoppedWhileWaitingForWebSocketOpen {
							actor_id: self.actor_id.to_string(),
							phase: PHASE_PRE_WEBSOCKET_OPEN.to_owned(),
						}
						.build());
					}
					res = in_flight_req.send_message(open_message, false) => res?,
				}
			}

			tracing::debug!(
				actor_id = %self.actor_id,
				actor_key = ?self.actor_key,
				actor_generation = ?self.actor_generation,
				namespace_id = %self.namespace_id,
				pool_name = %self.pool_name,
				envoy_key = %self.envoy_key,
				gateway_id = %display_id(&gateway_id),
				request_id = %display_id(&request_id),
				path = %self.path,
				"gateway waiting for websocket open from tunnel"
			);

			// Wait for WebSocket open acknowledgment. On restore, this is also the
			// generation-rebind barrier for buffered and newly received client frames.
			let fut = async {
				loop {
					tokio::select! {
						res = msg_rx.recv() => {
							if let Some(msg) = res {
								match msg.message_kind {
									protocol::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(msg) => {
										tracing::trace!(
											actor_id = %self.actor_id,
											actor_key = ?self.actor_key,
											actor_generation = ?self.actor_generation,
											namespace_id = %self.namespace_id,
											pool_name = %self.pool_name,
											envoy_key = %self.envoy_key,
											gateway_id = %display_id(&gateway_id),
											request_id = %display_id(&request_id),
											can_hibernate = msg.can_hibernate,
											"websocket open reached gateway handler"
										);
										tracing::debug!(
											actor_id = %self.actor_id,
											actor_key = ?self.actor_key,
											actor_generation = ?self.actor_generation,
											namespace_id = %self.namespace_id,
											pool_name = %self.pool_name,
											envoy_key = %self.envoy_key,
											gateway_id = %display_id(&gateway_id),
											request_id = %display_id(&request_id),
											can_hibernate = msg.can_hibernate,
											"received websocket open from envoy"
										);
										return anyhow::Ok(msg);
									}
									protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(close) => {
										tracing::warn!(
											actor_id = %self.actor_id,
											actor_key = ?self.actor_key,
											actor_generation = ?self.actor_generation,
											namespace_id = %self.namespace_id,
											pool_name = %self.pool_name,
											envoy_key = %self.envoy_key,
											gateway_id = %display_id(&gateway_id),
											request_id = %display_id(&request_id),
											?close,
											"websocket closed before opening"
										);
										return Err(WebSocketClosedBeforeOpen {
											close_code: close
												.code
												.map(|code| code.to_string())
												.unwrap_or_else(|| "none".to_owned()),
											close_reason: close.reason.unwrap_or_else(|| "none".to_owned()),
										}
										.build());
									}
									_ => {
										tracing::warn!(
											actor_id = %self.actor_id,
											actor_key = ?self.actor_key,
											actor_generation = ?self.actor_generation,
											namespace_id = %self.namespace_id,
											pool_name = %self.pool_name,
											envoy_key = %self.envoy_key,
											gateway_id = %display_id(&gateway_id),
											request_id = %display_id(&request_id),
											"received unexpected message while waiting for websocket open"
										);
									}
								}
							} else {
								tracing::warn!(
									actor_id = %self.actor_id,
									actor_key = ?self.actor_key,
									actor_generation = ?self.actor_generation,
									namespace_id = %self.namespace_id,
									pool_name = %self.pool_name,
									envoy_key = %self.envoy_key,
									gateway_id = %display_id(&gateway_id),
									request_id = %display_id(&request_id),
									"received empty message response during ws init",
								);
								break;
							}
						}
						_ = stopped_sub.next() => {
							tracing::warn!(
								actor_id = %self.actor_id,
								actor_key = ?self.actor_key,
								actor_generation = ?self.actor_generation,
								namespace_id = %self.namespace_id,
								pool_name = %self.pool_name,
								envoy_key = %self.envoy_key,
								gateway_id = %display_id(&gateway_id),
								request_id = %display_id(&request_id),
								path = %self.path,
								"actor stopped while waiting for websocket open"
							);
							return Err(ActorStoppedWhileWaitingForWebSocketOpen {
								actor_id: self.actor_id.to_string(),
								phase: PHASE_WAITING_FOR_WEBSOCKET_OPEN.to_owned(),
							}
							.build());
						}
						_ = drop_rx.changed() => {
							tracing::warn!(
								actor_id = %self.actor_id,
								actor_key = ?self.actor_key,
								actor_generation = ?self.actor_generation,
								namespace_id = %self.namespace_id,
								pool_name = %self.pool_name,
								envoy_key = %self.envoy_key,
								gateway_id = %display_id(&gateway_id),
								request_id = %display_id(&request_id),
								reason = ?drop_rx.borrow(),
								"websocket open dropped"
							);
							return Err(WebSocketOpenDropped {
								phase: PHASE_WAITING_FOR_WEBSOCKET_OPEN.to_owned(),
								reason: format!("{:?}", drop_rx.borrow().as_ref()),
							}
							.build());
						}
					}
				}

				Err(WebSocketOpenResponseClosed {
					phase: PHASE_WAITING_FOR_WEBSOCKET_OPEN.to_owned(),
				}
				.build())
			};

			let websocket_open_timeout = Duration::from_millis(
				self.ctx
					.config()
					.pegboard()
					.gateway_websocket_open_timeout_ms(),
			);
			let open_wait_start = Instant::now();
			let open_msg_result = tokio::time::timeout(websocket_open_timeout, fut).await;
			let open_wait_result = match &open_msg_result {
				Ok(Ok(_)) => "ok",
				Ok(Err(_)) => "error",
				Err(_) => "timeout",
			};
			metrics::WEBSOCKET_OPEN_WAIT_SECONDS
				.with_label_values(&[
					self.namespace_id.to_string().as_str(),
					self.pool_name.as_str(),
					open_wait_result,
				])
				.observe(open_wait_start.elapsed().as_secs_f64());
			if open_wait_start.elapsed() >= SLOW_WEBSOCKET_OPEN_WAIT_THRESHOLD {
				tracing::warn!(
					actor_id = %self.actor_id,
					actor_key = ?self.actor_key,
					actor_generation = ?self.actor_generation,
					namespace_id = %self.namespace_id,
					pool_name = %self.pool_name,
					envoy_key = %self.envoy_key,
					gateway_id = %display_id(&gateway_id),
					request_id = %display_id(&request_id),
					result = open_wait_result,
					duration_ms = open_wait_start.elapsed().as_millis() as u64,
					"slow websocket open wait"
				);
			}
			let open_msg = open_msg_result.map_err(|_| {
				tracing::warn!(
					actor_id = %self.actor_id,
					actor_key = ?self.actor_key,
					actor_generation = ?self.actor_generation,
					namespace_id = %self.namespace_id,
					pool_name = %self.pool_name,
					envoy_key = %self.envoy_key,
					gateway_id = %display_id(&gateway_id),
					request_id = %display_id(&request_id),
					timeout_ms = websocket_open_timeout.as_millis() as u64,
					path = %self.path,
					"timed out waiting for websocket open from envoy"
				);

				WebSocketOpenTimeout {
					timeout_ms: websocket_open_timeout.as_millis() as u64,
				}
				.build()
			})??;

			in_flight_req
				.toggle_hibernatable(open_msg.can_hibernate)
				.await?;

			let can_hibernate = open_msg.can_hibernate;

			let ingress_bytes = Arc::new(AtomicU64::new(0));
			let egress_bytes = Arc::new(AtomicU64::new(0));

			// Send pending messages
			in_flight_req.resend_pending_websocket_messages().await?;

			let ws_rx = client_ws.recv();

			let (tunnel_to_ws_abort_tx, tunnel_to_ws_abort_rx) = watch::channel(());
			let (ws_to_tunnel_abort_tx, ws_to_tunnel_abort_rx) = watch::channel(());
			let (ping_abort_tx, ping_abort_rx) = watch::channel(());
			let (keepalive_abort_tx, keepalive_abort_rx) = watch::channel(());
			let (metrics_abort_tx, metrics_abort_rx) = watch::channel(());

			let tunnel_to_ws = tokio::spawn(
				tunnel_to_ws_task::task(
					in_flight_req.clone(),
					client_ws,
					stopped_sub,
					msg_rx,
					drop_rx,
					can_hibernate,
					egress_bytes.clone(),
					tunnel_to_ws_abort_rx,
				)
				.in_current_span(),
			);
			let ws_to_tunnel = tokio::spawn(
				ws_to_tunnel_task::task(
					ctx.clone(),
					in_flight_req.clone(),
					ws_rx,
					ingress_bytes.clone(),
					ws_to_tunnel_abort_rx,
				)
				.in_current_span(),
			);
			let update_ping_interval = Duration::from_millis(
				self.ctx
					.config()
					.pegboard()
					.gateway_update_ping_interval_ms(),
			);
			let ping = tokio::spawn(
				ping_task::task(in_flight_req.clone(), ping_abort_rx, update_ping_interval)
					.in_current_span(),
			);
			let metrics = tokio::spawn(
				metrics_task::task(
					RequestMetrics::new(
						ctx.clone(),
						self.actor_id,
						self.namespace_id,
						self.envoy_key.clone(),
						RequestKind::Websocket,
					),
					ingress_bytes,
					egress_bytes,
					metrics_abort_rx,
				)
				.in_current_span(),
			);
			let keepalive = if can_hibernate {
				Some(tokio::spawn(
					keepalive_task::task(
						in_flight_req.clone(),
						ctx.clone(),
						self.actor_id,
						self.shared_state.gateway_id(),
						request_id,
						keepalive_abort_rx,
					)
					.in_current_span(),
				))
			} else {
				None
			};

			// Wait for all tasks to complete
			let (tunnel_to_ws_res, ws_to_tunnel_res, ping_res, metrics_res, keepalive_res) = tokio::join!(
				async {
					let res = tunnel_to_ws.await?;

					// Abort other if not aborted
					if !matches!(res, Ok(LifecycleResult::Aborted)) {
						tracing::debug!(?res, "tunnel to ws task completed, aborting others");

						let _ = ping_abort_tx.send(());
						let _ = ws_to_tunnel_abort_tx.send(());
						let _ = keepalive_abort_tx.send(());
						let _ = metrics_abort_tx.send(());
					} else {
						tracing::debug!(?res, "tunnel to ws task completed");
					}

					res
				},
				async {
					let res = ws_to_tunnel.await?;

					// Abort other if not aborted
					if !matches!(res, Ok(LifecycleResult::Aborted)) {
						tracing::debug!(?res, "ws to tunnel task completed, aborting others");

						let _ = ping_abort_tx.send(());
						let _ = tunnel_to_ws_abort_tx.send(());
						let _ = keepalive_abort_tx.send(());
						let _ = metrics_abort_tx.send(());
					} else {
						tracing::debug!(?res, "ws to tunnel task completed");
					}

					res
				},
				async {
					let res = ping.await?;

					// Abort others if not aborted
					if !matches!(res, Ok(LifecycleResult::Aborted)) {
						tracing::debug!(?res, "ping task completed, aborting others");

						let _ = ws_to_tunnel_abort_tx.send(());
						let _ = tunnel_to_ws_abort_tx.send(());
						let _ = keepalive_abort_tx.send(());
						let _ = metrics_abort_tx.send(());
					} else {
						tracing::debug!(?res, "ping task completed");
					}

					res
				},
				async {
					let res = metrics.await?;

					// Abort others if not aborted
					if !matches!(res, Ok(LifecycleResult::Aborted)) {
						tracing::debug!(?res, "metrics task completed, aborting others");

						let _ = ws_to_tunnel_abort_tx.send(());
						let _ = tunnel_to_ws_abort_tx.send(());
						let _ = ping_abort_tx.send(());
						let _ = keepalive_abort_tx.send(());
					} else {
						tracing::debug!(?res, "metrics task completed");
					}

					res
				},
				async {
					let Some(keepalive) = keepalive else {
						return Ok(LifecycleResult::Aborted);
					};

					let res = keepalive.await?;

					// Abort others if not aborted
					if !matches!(res, Ok(LifecycleResult::Aborted)) {
						tracing::debug!(?res, "keepalive task completed, aborting others");

						let _ = ws_to_tunnel_abort_tx.send(());
						let _ = tunnel_to_ws_abort_tx.send(());
						let _ = ping_abort_tx.send(());
						let _ = metrics_abort_tx.send(());
					} else {
						tracing::debug!(?res, "keepalive task completed");
					}

					res
				},
			);

			// Determine single result from all tasks
			let mut lifecycle_res = match (
				tunnel_to_ws_res,
				ws_to_tunnel_res,
				ping_res,
				metrics_res,
				keepalive_res,
			) {
				// Prefer error
				(Err(err), _, _, _, _) => Err(err),
				(_, Err(err), _, _, _) => Err(err),
				(_, _, Err(err), _, _) => Err(err),
				(_, _, _, Err(err), _) => Err(err),
				(_, _, _, _, Err(err)) => Err(err),
				// Prefer non aborted result if all succeed
				(Ok(res), Ok(LifecycleResult::Aborted), _, _, _) => Ok(res),
				(Ok(LifecycleResult::Aborted), Ok(res), _, _, _) => Ok(res),
				// Unlikely case
				(res, _, _, _, _) => res,
			};

			// Send close frame to envoy if not hibernating
			if !&lifecycle_res
				.as_ref()
				.map_or_else(is_ws_hibernate, |_| false)
			{
				let close_reason_label = match &lifecycle_res {
					Ok(LifecycleResult::ServerClose(_)) => "server_close",
					Ok(LifecycleResult::ClientClose(_)) => "client_close",
					Ok(LifecycleResult::Aborted) | Err(_) => "abort",
				};
				let (close_code, close_reason) = match &mut lifecycle_res {
					// Taking here because it won't be used again
					Ok(LifecycleResult::ClientClose(Some(close))) => {
						(close.code, Some(std::mem::take(&mut close.reason)))
					}
					Ok(_) => (CloseCode::Normal.into(), None),
					Err(_) => (CloseCode::Error.into(), Some("ws.downstream_closed".into())),
				};
				let close_message = protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(
					protocol::ToEnvoyWebSocketClose {
						code: Some(close_code.into()),
						reason: close_reason.map(|x| x.as_str().to_string()),
					},
				);

				if let Err(err) = in_flight_req.send_message(close_message, true).await {
					tracing::error!(?err, "error sending close message");
				} else {
					metrics::CLOSE_SENT_TOTAL
						.with_label_values(&[
							self.namespace_id.to_string().as_str(),
							self.pool_name.as_str(),
							RequestProtocol::WebSocket.to_string().as_str(),
							close_reason_label,
						])
						.inc();
				}

				let stop_result = match &lifecycle_res {
					Ok(LifecycleResult::ServerClose(_)) => RequestStopResult::Success,
					Ok(LifecycleResult::ClientClose(_)) => RequestStopResult::ClientDisconnect,
					Ok(LifecycleResult::Aborted) => RequestStopResult::RequestTimeout,
					Err(_) => RequestStopResult::EnvoyError,
				};
				in_flight_req.stop(stop_result).await;
			} else {
				in_flight_req.start_hibernation().await?;
			}

			// Send WebSocket close message to client
			match lifecycle_res {
				Ok(LifecycleResult::ServerClose(close)) => {
					if let Some(code) = close.code {
						Ok(Some(CloseFrame {
							code: code.into(),
							reason: close.reason.unwrap_or_default().into(),
						}))
					} else {
						Ok(None)
					}
				}
				Ok(_) => Ok(None),
				Err(err) => Err(err),
			}
		}
		.await;

		if res
			.as_ref()
			.map_or_else(|err| !is_ws_hibernate(err), |_| false)
		{
			in_flight_req.stop(RequestStopResult::EnvoyError).await;
		}

		res
	}
}

#[async_trait]
impl CustomServeTrait for PegboardGateway3 {
	fn streams_request_body(&self) -> bool {
		true
	}

	async fn prepare_streaming_request(&self, req_ctx: &mut RequestContext) -> Result<()> {
		let ctx = self.ctx.with_ray(req_ctx.ray_id(), req_ctx.req_id())?;
		let prepared = self.prepare_http_exchange(&ctx, req_ctx).await?;
		let mut slot = self.prepared_http_request.lock().await;
		anyhow::ensure!(slot.is_none(), "HTTP request was prepared more than once");
		*slot = Some(prepared);
		Ok(())
	}

	#[tracing::instrument(skip_all, fields(actor_id=?self.actor_id, actor_key=?self.actor_key, actor_generation=?self.actor_generation, namespace_id=?self.namespace_id, pool_name=%self.pool_name, envoy_key=%self.envoy_key))]
	async fn handle_request(
		&self,
		req: Request<Full<Bytes>>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>> {
		self.handle_http_request(req, req_ctx).await
	}

	#[tracing::instrument(skip_all, fields(actor_id=?self.actor_id, actor_key=?self.actor_key, actor_generation=?self.actor_generation, namespace_id=?self.namespace_id, pool_name=%self.pool_name, envoy_key=%self.envoy_key))]
	async fn handle_streaming_request(
		&self,
		req: Request<BodyIncoming>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>> {
		self.handle_http_request(req, req_ctx).await
	}

	#[tracing::instrument(skip_all, fields(actor_id=?self.actor_id, actor_key=?self.actor_key, actor_generation=?self.actor_generation, namespace_id=?self.namespace_id, pool_name=%self.pool_name, envoy_key=%self.envoy_key))]
	async fn handle_websocket(
		&self,
		req_ctx: &mut RequestContext,
		client_ws: WebSocketHandle,
		after_hibernation: bool,
	) -> Result<Option<CloseFrame>> {
		let ctx = self.ctx.with_ray(req_ctx.ray_id(), req_ctx.req_id())?;
		let request_metrics = RequestMetrics::new(
			ctx.clone(),
			self.actor_id,
			self.namespace_id,
			self.envoy_key.clone(),
			RequestKind::Websocket,
		);
		let (res, active_metrics) = tokio::join!(
			self.handle_websocket_inner(&ctx, req_ctx, client_ws, after_hibernation),
			request_metrics.begin(0),
		);
		active_metrics.finish_in_background(0);

		res
	}

	#[tracing::instrument(skip_all, fields(actor_id=?self.actor_id, actor_key=?self.actor_key, actor_generation=?self.actor_generation, namespace_id=?self.namespace_id, pool_name=%self.pool_name, envoy_key=%self.envoy_key))]
	async fn handle_websocket_hibernation(
		&self,
		req_ctx: &mut RequestContext,
		client_ws: WebSocketHandle,
	) -> Result<HibernationResult> {
		let ctx = self.ctx.with_ray(req_ctx.ray_id(), req_ctx.req_id())?;
		let request_id = req_ctx.in_flight_request_id()?;
		let InFlightRequestCtx {
			msg_rx,
			drop_rx,
			http_response_abort_rx: _,
			handle: in_flight_req,
		} = self
			.shared_state
			.get_hibernating_in_flight_request(
				self.namespace_id,
				self.pool_name.as_str(),
				self.actor_key.clone(),
				Some(self.actor_generation),
				request_id,
			)
			.await?;

		// Immediately rewake if we have pending ws messages from the client
		if in_flight_req.has_pending_websocket_messages().await? {
			tracing::debug!("exiting hibernation due to pending ws messages");

			return Ok(HibernationResult::Continue);
		}

		// Unused during hibernation
		let ingress_bytes = Arc::new(AtomicU64::new(0));
		let egress_bytes = Arc::new(AtomicU64::new(0));
		let request_metrics = RequestMetrics::new(
			ctx.clone(),
			self.actor_id,
			self.namespace_id,
			self.envoy_key.clone(),
			RequestKind::HibernatingWebsocket,
		);

		let (hibernation_abort_tx, hibernation_abort_rx) = watch::channel(());
		let (keepalive_abort_tx, keepalive_abort_rx) = watch::channel(());
		let (metrics_abort_tx, metrics_abort_rx) = watch::channel(());

		let hibernation = tokio::spawn(
			hibernation_task::task(
				client_ws,
				in_flight_req.clone(),
				ctx.clone(),
				self.actor_id,
				msg_rx,
				drop_rx,
				egress_bytes.clone(),
				hibernation_abort_rx,
			)
			.in_current_span(),
		);
		let metrics = tokio::spawn(
			metrics_task::task(
				request_metrics.clone(),
				ingress_bytes,
				egress_bytes,
				metrics_abort_rx,
			)
			.in_current_span(),
		);
		let keepalive = tokio::spawn(
			keepalive_task::task(
				in_flight_req.clone(),
				ctx.clone(),
				self.actor_id,
				self.shared_state.gateway_id(),
				request_id,
				keepalive_abort_rx,
			)
			.in_current_span(),
		);

		// Wait for all tasks to complete or ws msg from client
		let (active_metrics, hibernation_res, metrics_res, keepalive_res) = tokio::join!(
			request_metrics.begin(0),
			async {
				let res = hibernation.await?;

				// Abort other if not aborted
				if !matches!(res, Ok(HibernationLifecycleResult::Aborted)) {
					tracing::debug!(?res, "hibernation completed, aborting others");

					let _ = keepalive_abort_tx.send(());
					let _ = metrics_abort_tx.send(());
				} else {
					tracing::debug!(?res, "hibernation completed");
				}

				res
			},
			async {
				let res = metrics.await?;

				// Abort others if not aborted
				if !matches!(res, Ok(LifecycleResult::Aborted)) {
					tracing::debug!(?res, "metrics task completed, aborting others");

					let _ = hibernation_abort_tx.send(());
					let _ = keepalive_abort_tx.send(());
				} else {
					tracing::debug!(?res, "metrics task completed");
				}

				res
			},
			async {
				let res = keepalive.await?;

				// Abort others if not aborted
				if !matches!(res, Ok(LifecycleResult::Aborted)) {
					tracing::debug!(?res, "keepalive task completed, aborting others");

					let _ = hibernation_abort_tx.send(());
					let _ = metrics_abort_tx.send(());
				} else {
					tracing::debug!(?res, "keepalive task completed");
				}

				res
			},
		);

		// Determine single result from all tasks
		let res = match (hibernation_res, metrics_res, keepalive_res) {
			// Prefer error
			(Err(err), _, _) => Err(err),
			(_, Err(err), _) => Err(err),
			(_, _, Err(err)) => Err(err),
			// Prefer hibernation result
			(Ok(res), _, _) => match res {
				HibernationLifecycleResult::Continue => Ok(HibernationResult::Continue),
				HibernationLifecycleResult::Close => Ok(HibernationResult::Close),
				// Should be unreachable
				HibernationLifecycleResult::Aborted => Err(anyhow!("hibernation aborted")),
			},
		};

		let (delete_res, _) = tokio::join!(
			async {
				match &res {
					Ok(HibernationResult::Continue) => {}
					Ok(HibernationResult::Close) | Err(_) => {
						let stop_result = match &res {
							Ok(HibernationResult::Close) => RequestStopResult::ClientDisconnect,
							Ok(HibernationResult::Continue) => RequestStopResult::Success,
							Err(_) => RequestStopResult::ActorReadyTimeout,
						};
						in_flight_req.stop(stop_result).await;

						// No longer an active hibernating request, delete entry
						ctx.op(pegboard::ops::actor::hibernating_request::delete::Input {
							actor_id: self.actor_id,
							gateway_id: self.shared_state.gateway_id(),
							request_id,
						})
						.await?;
					}
				}

				anyhow::Ok(())
			},
			active_metrics.finish(0),
		);

		delete_res?;

		res
	}
}
