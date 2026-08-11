use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bytes::Bytes;
use gas::prelude::*;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, StatusCode, body::Body};
use rivet_envoy_protocol as protocol;
use rivet_error::*;
use rivet_guard_core::{
	ResponseBody, WebSocketHandle,
	custom_serve::{CustomServeTrait, HibernationResult},
	errors::{
		ActorStoppedWhileWaiting, ActorStoppedWhileWaitingForWebSocketOpen,
		GatewayResponseStartTimeout, TunnelMessageTimeout, TunnelRequestAborted,
		TunnelResponseClosed, WebSocketClosedBeforeOpen, WebSocketOpenDropped,
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
use universaldb::utils::IsolationLevel::*;

use crate::shared_state::{
	InFlightRequestCtx, RequestProtocol, RequestStopResult, SharedState, display_id,
};

mod hibernation_task;
mod keepalive_task;
pub mod metrics;
mod metrics_task;
mod ping_task;
pub mod shared_state;
mod tunnel_to_ws_task;
mod ws_to_tunnel_task;

const RECORD_REQ_METRICS_TIMEOUT: Duration = Duration::from_secs(15);
const UPDATE_METRICS_INTERVAL: Duration = Duration::from_secs(15);
const PHASE_PRE_REQUEST: &str = "pre_request";
const PHASE_WAITING_FOR_RESPONSE_START: &str = "waiting_for_response_start";
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

pub struct PegboardGateway2 {
	ctx: StandaloneCtx,
	shared_state: SharedState,
	namespace_id: Id,
	pool_name: String,
	envoy_key: String,
	actor_id: Id,
	actor_key: Option<String>,
	actor_generation: Option<u32>,
	path: String,
}

impl PegboardGateway2 {
	#[tracing::instrument(skip_all, fields(?actor_id, actor_key=?actor_key, actor_generation=?actor_generation, ?namespace_id, %pool_name, %envoy_key, ?path))]
	pub fn new(
		ctx: StandaloneCtx,
		shared_state: SharedState,
		namespace_id: Id,
		pool_name: String,
		envoy_key: String,
		actor_id: Id,
		actor_key: Option<String>,
		actor_generation: Option<u32>,
		path: String,
	) -> Self {
		Self {
			ctx,
			shared_state,
			namespace_id,
			pool_name,
			envoy_key,
			actor_id,
			actor_key,
			actor_generation,
			path,
		}
	}
}

impl PegboardGateway2 {
	async fn handle_request_inner(
		&self,
		ctx: &StandaloneCtx,
		req: Request<Full<Bytes>>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>> {
		// Use the actor ID from the gateway instance
		let actor_id = self.actor_id.to_string();
		let request_id = req_ctx.in_flight_request_id()?;

		// Extract request parts
		let headers = req
			.headers()
			.iter()
			.filter_map(|(name, value)| {
				value
					.to_str()
					.ok()
					.map(|value_str| (name.to_string(), value_str.to_string()))
			})
			.collect::<HashMap<_, _>>();

		// NOTE: Size constraints have already been applied by guard
		let body_bytes = req
			.into_body()
			.collect()
			.await
			.context("failed to read body")?
			.to_bytes();

		let mut stopped_sub = ctx
			.subscribe::<pegboard::workflows::actor2::Stopped>(("actor_id", self.actor_id))
			.await?;

		// Verify envoy key is still the same after stopped sub is open to prevent race conditions with
		// actor reallocation
		let res = ctx
			.op(pegboard::ops::actor::get_for_gateway::Input {
				actor_id: self.actor_id,
			})
			.await?;
		let Some(envoy_key) = res.and_then(|x| x.envoy_key) else {
			// No envoy key
			return Err(ActorStoppedWhileWaiting {
				actor_id: self.actor_id.to_string(),
				phase: PHASE_PRE_REQUEST.to_owned(),
			}
			.build());
		};

		// Actor reallocated to a different envoy
		if self.envoy_key != envoy_key {
			tracing::debug!(
				gateway_envoy_key=%self.envoy_key,
				new_envoy_key=%envoy_key,
				"actor changed envoy while waiting for websocket open",
			);
			return Err(ActorStoppedWhileWaiting {
				actor_id: self.actor_id.to_string(),
				phase: PHASE_PRE_REQUEST.to_owned(),
			}
			.build());
		}

		// Build subject to publish to
		let tunnel_subject = pegboard::pubsub_subjects::EnvoyReceiverSubject::new(
			self.namespace_id,
			self.envoy_key.clone(),
		)
		.to_string();

		// Start listening for request responses
		let InFlightRequestCtx {
			mut msg_rx,
			mut drop_rx,
			handle: in_flight_req,
		} = self
			.shared_state
			.create_or_wake_in_flight_request(
				self.namespace_id,
				self.pool_name.as_str(),
				self.actor_key.clone(),
				self.actor_generation,
				RequestProtocol::Http,
				tunnel_subject,
				request_id,
				false,
			)
			.await?;

		let res = async {
			// Start request
			let message = protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				protocol::ToEnvoyRequestStart {
					actor_id: actor_id.clone(),
					method: req_ctx.method().to_string(),
					path: self.path.clone(),
					headers,
					body: if body_bytes.is_empty() {
						None
					} else {
						Some(body_bytes.to_vec())
					},
					stream: false,
				},
			);

			tokio::select! {
				// Prefer quick stop path
				biased;
				_ = stopped_sub.next() => {
					tracing::debug!("actor stopped while sending request");
					return Err(ActorStoppedWhileWaiting {
						actor_id: self.actor_id.to_string(),
						phase: PHASE_PRE_REQUEST.to_owned(),
					}
					.build());
				}
				res = in_flight_req.send_message(message, false) => res?,
			}

			// Wait for response
			tracing::debug!("gateway waiting for response from tunnel");
			let fut = async {
				loop {
					tokio::select! {
						res = msg_rx.recv() => {
							if let Some(msg) = res {
								match msg {
									protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
										response_start,
									) => {
										return anyhow::Ok(response_start);
									}
									protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort => {
										tracing::warn!("request aborted");
										return Err(TunnelRequestAborted {
											phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
										}
										.build());
									}
									_ => {
										tracing::warn!("received non-response message from pubsub");
									}
								}
							} else {
								tracing::warn!(
									request_id=%protocol::util::id_to_string(&request_id),
									"received empty message response during request init",
								);
								return Err(TunnelResponseClosed {
									phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
								}
								.build());
							}
						}
						_ = drop_rx.changed() => {
							tracing::warn!(reason=?drop_rx.borrow(), "tunnel message timeout");
							return Err(TunnelMessageTimeout {
								phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
								reason: format!("{:?}", drop_rx.borrow().as_ref()),
							}
							.build());
						}
						_ = stopped_sub.next() => {
							tracing::debug!("actor stopped while waiting for request response");
							return Err(ActorStoppedWhileWaiting {
								actor_id: self.actor_id.to_string(),
								phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
							}.build());
						}
					}
				}
			}
			.instrument(tracing::info_span!("wait_for_tunnel_response"));
			let response_start_timeout = Duration::from_millis(
				self.ctx
					.config()
					.pegboard()
					.gateway_response_start_timeout_ms(),
			);
			let response_start = tokio::time::timeout(response_start_timeout, fut)
				.await
				.map_err(|_| {
					tracing::warn!("timed out waiting for response start from envoy");

					GatewayResponseStartTimeout {
						phase: "response_start".to_owned(),
						timeout_ms: response_start_timeout.as_millis() as u64,
					}
					.build()
				})??;
			tracing::debug!("response handler task ended");

			// Build HTTP response
			let mut response_builder =
				Response::builder().status(StatusCode::from_u16(response_start.status)?);

			// Add headers from actor
			for (key, value) in response_start.headers {
				response_builder = response_builder.header(key, value);
			}

			// Add body
			let body = response_start.body.unwrap_or_default();
			let response =
				response_builder.body(ResponseBody::Full(Full::new(Bytes::from(body))))?;

			in_flight_req.stop(RequestStopResult::Success).await;

			Ok(response)
		}
		.await;

		if res.is_err() {
			in_flight_req.stop(RequestStopResult::EnvoyError).await;
		}

		res
	}

	async fn handle_websocket_inner(
		&self,
		ctx: &StandaloneCtx,
		req_ctx: &mut RequestContext,
		client_ws: WebSocketHandle,
		after_hibernation: bool,
	) -> Result<Option<CloseFrame>> {
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

		// Verify envoy key is still the same after stopped sub is open to prevent race conditions with
		// actor reallocation
		let res = ctx
			.op(pegboard::ops::actor::get_for_gateway::Input {
				actor_id: self.actor_id,
			})
			.await?;
		let Some(envoy_key) = res.and_then(|x| x.envoy_key) else {
			// No envoy key
			return Err(ActorStoppedWhileWaitingForWebSocketOpen {
				actor_id: self.actor_id.to_string(),
				phase: PHASE_PRE_WEBSOCKET_OPEN.to_owned(),
			}
			.build());
		};

		// Actor reallocated to a different envoy
		if self.envoy_key != envoy_key {
			tracing::debug!(
				gateway_envoy_key=%self.envoy_key,
				new_envoy_key=%envoy_key,
				"actor changed envoy while waiting for websocket open",
			);
			return Err(ActorStoppedWhileWaitingForWebSocketOpen {
				actor_id: self.actor_id.to_string(),
				phase: PHASE_PRE_WEBSOCKET_OPEN.to_owned(),
			}
			.build());
		}

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
			handle: in_flight_req,
		} = self
			.shared_state
			.create_or_wake_in_flight_request(
				self.namespace_id,
				self.pool_name.as_str(),
				self.actor_key.clone(),
				self.actor_generation,
				RequestProtocol::WebSocket,
				tunnel_subject.clone(),
				request_id,
				after_hibernation,
			)
			.await?;

		let res = async {
			// If we are reconnecting after hibernation, don't send an open message
			let can_hibernate = if after_hibernation {
				true
			} else {
				// Send WebSocket open message
				let open_message = protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
					protocol::ToEnvoyWebSocketOpen {
						actor_id: self.actor_id.to_string(),
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

				// Wait for WebSocket open acknowledgment
				let fut = async {
					loop {
						tokio::select! {
							res = msg_rx.recv() => {
								if let Some(msg) = res {
									match msg {
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

				open_msg.can_hibernate
			};

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
					ctx.clone(),
					self.actor_id,
					self.namespace_id,
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
impl CustomServeTrait for PegboardGateway2 {
	#[tracing::instrument(skip_all, fields(actor_id=?self.actor_id, actor_key=?self.actor_key, actor_generation=?self.actor_generation, namespace_id=?self.namespace_id, pool_name=%self.pool_name, envoy_key=%self.envoy_key))]
	async fn handle_request(
		&self,
		req: Request<Full<Bytes>>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>> {
		let ctx = self.ctx.with_ray(req_ctx.ray_id(), req_ctx.req_id())?;
		let req_body_size_hint = req.body().size_hint();

		let (res, metrics_res) = tokio::join!(
			self.handle_request_inner(&ctx, req, req_ctx),
			record_req_metrics(
				&ctx,
				self.actor_id,
				self.namespace_id,
				Metric::HttpIngress(
					req_body_size_hint
						.upper()
						.unwrap_or(req_body_size_hint.lower()) as usize
				),
			),
		);

		let response_size = match &res {
			Ok(res) => res.size_hint().upper().unwrap_or(res.size_hint().lower()),
			Err(_) => 0,
		};

		if let Err(err) = metrics_res {
			tracing::error!(?err, "http req ingress metrics failed");
		} else {
			let actor_id = self.actor_id;
			let namespace_id = self.namespace_id;
			let envoy_key = self.envoy_key.clone();
			tokio::spawn(
				async move {
					if let Err(err) = record_req_metrics(
						&ctx,
						actor_id,
						namespace_id,
						Metric::HttpEgress(response_size as usize),
					)
					.await
					{
						tracing::error!(
							?err,
							?namespace_id,
							%envoy_key,
							"http req egress metrics failed, likely corrupt now",
						);
					}
				}
				.in_current_span(),
			);
		}

		res
	}

	#[tracing::instrument(skip_all, fields(actor_id=?self.actor_id, actor_key=?self.actor_key, actor_generation=?self.actor_generation, namespace_id=?self.namespace_id, pool_name=%self.pool_name, envoy_key=%self.envoy_key))]
	async fn handle_websocket(
		&self,
		req_ctx: &mut RequestContext,
		client_ws: WebSocketHandle,
		after_hibernation: bool,
	) -> Result<Option<CloseFrame>> {
		let ctx = self.ctx.with_ray(req_ctx.ray_id(), req_ctx.req_id())?;
		let (res, metrics_res) = tokio::join!(
			self.handle_websocket_inner(&ctx, req_ctx, client_ws, after_hibernation),
			record_req_metrics(
				&ctx,
				self.actor_id,
				self.namespace_id,
				Metric::WebsocketOpen
			),
		);

		if let Err(err) = metrics_res {
			tracing::error!(?err, "ws open metrics failed");
		} else {
			let actor_id = self.actor_id;
			let namespace_id = self.namespace_id;
			let envoy_key = self.envoy_key.clone();
			tokio::spawn(
				async move {
					if let Err(err) =
						record_req_metrics(&ctx, actor_id, namespace_id, Metric::WebsocketClose)
							.await
					{
						tracing::error!(
							?err,
							?namespace_id,
							%envoy_key,
							"ws close metrics failed, likely corrupt now",
						);
					}
				}
				.in_current_span(),
			);
		}

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
			handle: in_flight_req,
		} = self
			.shared_state
			.get_hibernating_in_flight_request(
				self.namespace_id,
				self.pool_name.as_str(),
				self.actor_key.clone(),
				self.actor_generation,
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
				ctx.clone(),
				self.actor_id,
				self.namespace_id,
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
		let (record_start_res, hibernation_res, metrics_res, keepalive_res) = tokio::join!(
			record_req_metrics(
				&ctx,
				self.actor_id,
				self.namespace_id,
				Metric::WebsocketHibernate
			),
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
			async {
				if let Err(err) = record_start_res {
					tracing::error!(?err, "ws start hibernate metrics failed");
				} else {
					if let Err(err) = record_req_metrics(
						&ctx,
						self.actor_id,
						self.namespace_id,
						Metric::WebsocketStopHibernate,
					)
					.await
					{
						tracing::error!(
							?err,
							namespace_id=?self.namespace_id,
							envoy_key=%self.envoy_key,
							"ws stop hibernate metrics failed, likely corrupt now"
						);
					}
				}
			},
		);

		delete_res?;

		res
	}
}

#[derive(Debug)]
enum Metric {
	HttpIngress(usize),
	HttpEgress(usize),
	WebsocketOpen,
	// Ingress, Egress
	WebsocketTransfer(usize, usize),
	WebsocketClose,
	WebsocketHibernate,
	WebsocketStopHibernate,
}

#[tracing::instrument(skip_all, fields(?actor_id, ?metric))]
async fn record_req_metrics(
	ctx: &StandaloneCtx,
	actor_id: Id,
	namespace_id: Id,
	metric: Metric,
) -> Result<()> {
	let metric = &metric;
	tokio::time::timeout(
		RECORD_REQ_METRICS_TIMEOUT,
		ctx.udb()?
			.txn("gateway_record_req_metrics", |tx| async move {
				let tx = tx.with_subspace(pegboard::keys::subspace());

				let actor_name = tx
					.read(&pegboard::keys::actor::NameKey::new(actor_id), Serializable)
					.await?;

				metric_inc(&tx, namespace_id, &actor_name, metric);

				Ok(())
			})
			.instrument(tracing::info_span!("record_req_metrics_tx")),
	)
	.await
	.context("timed out recording req metrics")??;

	Ok(())
}

fn metric_inc(tx: &universaldb::Transaction, namespace_id: Id, name: &str, metric: &Metric) {
	match metric {
		Metric::HttpIngress(size) => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::GatewayIngress(
					name.to_string(),
					"http".to_string(),
				),
				(*size).try_into().unwrap_or_default(),
			);
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::Requests(name.to_string(), "http".to_string()),
				1,
			);
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActiveRequests(
					name.to_string(),
					"http".to_string(),
				),
				1,
			);
		}
		Metric::HttpEgress(size) => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::GatewayEgress(
					name.to_string(),
					"http".to_string(),
				),
				(*size).try_into().unwrap_or_default(),
			);
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActiveRequests(
					name.to_string(),
					"http".to_string(),
				),
				-1,
			);
		}
		Metric::WebsocketOpen => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::Requests(name.to_string(), "ws".to_string()),
				1,
			);
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActiveRequests(name.to_string(), "ws".to_string()),
				1,
			);
		}
		Metric::WebsocketTransfer(ingress_size, egress_size) => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::GatewayIngress(name.to_string(), "ws".to_string()),
				(*ingress_size).try_into().unwrap_or_default(),
			);
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::GatewayEgress(name.to_string(), "ws".to_string()),
				(*egress_size).try_into().unwrap_or_default(),
			);
		}
		Metric::WebsocketClose => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActiveRequests(name.to_string(), "ws".to_string()),
				-1,
			);
		}
		Metric::WebsocketHibernate => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::Requests(name.to_string(), "hws".to_string()),
				1,
			);
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActiveRequests(
					name.to_string(),
					"hws".to_string(),
				),
				1,
			);
		}
		Metric::WebsocketStopHibernate => {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActiveRequests(
					name.to_string(),
					"hws".to_string(),
				),
				-1,
			);
		}
	}
}
