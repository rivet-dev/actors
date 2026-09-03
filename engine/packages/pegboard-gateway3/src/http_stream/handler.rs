use std::{
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use anyhow::{Result, anyhow};
use bytes::Bytes;
use gas::prelude::*;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode, body::Body};
use rivet_envoy_protocol as protocol;
use rivet_guard_core::{
	ResponseBody,
	errors::{ActorStoppedWhileWaiting, InvalidRequestBody},
	request_context::RequestContext,
};
use tokio::sync::{mpsc, watch};
use tracing::Instrument;

use super::{
	request::{
		should_stream_http_request_body, stream_http_request_and_wait_for_response,
		wait_for_http_response_start,
	},
	response::drain_http_response_stream,
	send_http_request_abort,
};
use crate::{
	PegboardGateway3, metrics_task,
	request_metrics::{RequestKind, RequestMetrics},
	shared_state::{InFlightRequestCtx, RequestProtocol, RequestStopResult},
};

const PHASE_PRE_REQUEST: &str = "pre_request";

pub(crate) struct PreparedHttpRequest {
	msg_rx: mpsc::UnboundedReceiver<crate::shared_state::InFlightTunnelMessage>,
	drop_rx: watch::Receiver<Option<crate::shared_state::MsgGcReason>>,
	http_response_abort_rx: watch::Receiver<Option<protocol::HttpStreamAbortReason>>,
	in_flight_req: crate::shared_state::InFlightRequestHandle,
	stopped_sub: message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	client_disconnect_guard: HttpClientDisconnectGuard,
	request_id: protocol::RequestId,
	request_stream: bool,
}

impl PegboardGateway3 {
	pub(crate) async fn prepare_http_exchange(
		&self,
		ctx: &StandaloneCtx,
		req_ctx: &mut RequestContext,
	) -> Result<PreparedHttpRequest> {
		let actor_generation = self.actor_generation;
		let max_request_body_size = ctx.config().guard().http_max_request_body_size();
		if req_ctx
			.request_body_exact_size()
			.is_some_and(|size| size > max_request_body_size as u64)
		{
			return Err(InvalidRequestBody {
				reason: format!("request body exceeded the {max_request_body_size}-byte limit"),
			}
			.build());
		}

		let request_stream = should_stream_http_request_body(
			req_ctx.method(),
			req_ctx.request_body_exact_size(),
			req_ctx.request_body_is_end_stream(),
		);
		let request_id = req_ctx.in_flight_request_id()?;
		let headers = req_ctx
			.headers()
			.iter()
			.filter_map(|(name, value)| {
				value
					.to_str()
					.ok()
					.map(|value| (name.to_string(), value.to_owned()))
			})
			.collect();
		let mut stopped_sub = ctx
			.subscribe::<pegboard::workflows::actor2::Stopped>(("actor_id", self.actor_id))
			.await?;

		let tunnel_subject = pegboard::pubsub_subjects::EnvoyReceiverSubject::new(
			self.namespace_id,
			self.envoy_key.clone(),
		)
		.to_string();
		let InFlightRequestCtx {
			msg_rx,
			drop_rx,
			http_response_abort_rx,
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
				RequestProtocol::Http,
				tunnel_subject,
				request_id,
				self.lifecycle.clone(),
				false,
			)
			.await?;
		let client_disconnect_guard = HttpClientDisconnectGuard::new(in_flight_req.clone());
		let message = protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
			protocol::ToEnvoyRequestStart {
				actor_id: self.actor_id.to_string(),
				actor_generation: Some(actor_generation),
				method: req_ctx.method().to_string(),
				path: self.path.clone(),
				headers,
				body: None,
				stream: request_stream,
				response_stream: true,
			},
		);
		send_to_envoy_or_actor_stopped(
			&in_flight_req,
			&mut stopped_sub,
			self.actor_id,
			PHASE_PRE_REQUEST,
			message,
			false,
		)
		.await?;

		Ok(PreparedHttpRequest {
			msg_rx,
			drop_rx,
			http_response_abort_rx,
			in_flight_req,
			stopped_sub,
			client_disconnect_guard,
			request_id,
			request_stream,
		})
	}

	pub(crate) async fn handle_http_request<B>(
		&self,
		req: Request<B>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>>
	where
		B: Body<Data = Bytes> + Unpin,
		B::Error: std::error::Error + Send + Sync + 'static,
	{
		let ctx = self.ctx.with_ray(req_ctx.ray_id(), req_ctx.req_id())?;
		let ingress_bytes = Arc::new(AtomicU64::new(0));
		let egress_bytes = Arc::new(AtomicU64::new(0));
		let request_metrics = RequestMetrics::new(
			ctx.clone(),
			self.actor_id,
			self.namespace_id,
			self.envoy_key.clone(),
			RequestKind::Http,
		);
		let (metrics_abort_tx, metrics_abort_rx) = watch::channel(());
		let transfer_metrics = request_metrics.clone();
		let transfer_ingress_bytes = ingress_bytes.clone();
		let transfer_egress_bytes = egress_bytes.clone();
		tokio::spawn(
			async move {
				if let Err(error) = metrics_task::task(
					transfer_metrics,
					transfer_ingress_bytes,
					transfer_egress_bytes,
					metrics_abort_rx,
				)
				.await
				{
					tracing::error!(?error, "HTTP transfer metrics task failed");
				}
			}
			.in_current_span(),
		);

		let (res, active_metrics) = tokio::join!(
			self.handle_request_inner(
				&ctx,
				req,
				req_ctx,
				ingress_bytes.clone(),
				egress_bytes.clone(),
			),
			request_metrics.begin(0),
		);

		match res {
			Ok(response) => {
				let (parts, body) = response.into_parts();
				Ok(Response::from_parts(
					parts,
					body.with_completion(move || {
						let _ = metrics_abort_tx.send(());
						active_metrics.finish_in_background(0);
					}),
				))
			}
			Err(error) => {
				let _ = metrics_abort_tx.send(());
				active_metrics.finish_in_background(0);
				Err(error)
			}
		}
	}

	async fn handle_request_inner<B>(
		&self,
		ctx: &StandaloneCtx,
		req: Request<B>,
		req_ctx: &mut RequestContext,
		ingress_bytes: Arc<AtomicU64>,
		egress_bytes: Arc<AtomicU64>,
	) -> Result<Response<ResponseBody>>
	where
		B: Body<Data = Bytes> + Unpin,
		B::Error: std::error::Error + Send + Sync + 'static,
	{
		let prepared = match self.prepared_http_request.lock().await.take() {
			Some(prepared) => prepared,
			None => self.prepare_http_exchange(ctx, req_ctx).await?,
		};
		let PreparedHttpRequest {
			mut msg_rx,
			mut drop_rx,
			http_response_abort_rx,
			in_flight_req,
			mut stopped_sub,
			mut client_disconnect_guard,
			request_id,
			request_stream,
		} = prepared;

		let request_body_size_hint = req.body().size_hint();
		let max_request_body_size = ctx.config().guard().http_max_request_body_size();
		if request_body_size_hint
			.upper()
			.is_some_and(|body_size| body_size > max_request_body_size as u64)
		{
			return Err(InvalidRequestBody {
				reason: format!("request body exceeded the {max_request_body_size}-byte limit"),
			}
			.build());
		}
		let (_, body) = req.into_parts();
		let streaming_body = request_stream.then_some(body);

		let client_disconnect = req_ctx.client_disconnect_token();
		let exchange = async {
			let response_start_timeout = Duration::from_millis(
				self.ctx
					.config()
					.pegboard()
					.gateway_response_start_timeout_ms(),
			);
			let response_start_deadline = tokio::time::Instant::now() + response_start_timeout;
			let response_start = if let Some(body) = streaming_body {
				stream_http_request_and_wait_for_response(
					&in_flight_req,
					&mut msg_rx,
					&mut drop_rx,
					&mut stopped_sub,
					self.actor_id,
					request_id,
					body,
					max_request_body_size,
					ingress_bytes,
					response_start_deadline,
					response_start_timeout,
				)
				.instrument(tracing::info_span!("stream_request_and_wait_for_response"))
				.await?
			} else {
				wait_for_http_response_start(
					&in_flight_req,
					&mut msg_rx,
					&mut drop_rx,
					&mut stopped_sub,
					self.actor_id,
					request_id,
					response_start_deadline,
					response_start_timeout,
				)
				.instrument(tracing::info_span!("wait_for_tunnel_response"))
				.await?
			};
			let (response_start_message_id, mut response_start) = response_start;

			let mut response_builder =
				Response::builder().status(StatusCode::from_u16(response_start.status)?);
			for (key, value) in response_start.headers {
				response_builder = response_builder.header(key, value);
			}

			let response = if response_start.stream {
				let body_channel_capacity = self
					.ctx
					.config()
					.pegboard()
					.gateway_http_response_body_channel_capacity();
				let (body_tx, response_body, terminal_error) =
					ResponseBody::channel_with_terminal(body_channel_capacity);
				let response_consumed_bytes = Arc::new(AtomicU64::new(0));
				let flow_metrics = crate::metrics::HttpResponseFlowMetrics::new();
				let (response_consumed_tx, response_consumed_rx) = mpsc::unbounded_channel();
				let idle_timeout = self
					.ctx
					.config()
					.pegboard()
					.gateway_response_chunk_idle_timeout_ms()
					.map(|timeout_ms| Duration::from_millis(timeout_ms.max(1)));
				let tunnel_ping_interval = Duration::from_millis(
					self.ctx
						.config()
						.pegboard()
						.gateway_update_ping_interval_ms()
						.max(1),
				);
				let expected_message_index =
					response_start_message_id.message_index.wrapping_add(1);
				let initial_body = response_start.body.take();

				tokio::spawn(
					drain_http_response_stream(
						in_flight_req.clone(),
						msg_rx,
						drop_rx,
						http_response_abort_rx,
						stopped_sub,
						body_tx,
						initial_body,
						expected_message_index,
						self.actor_id,
						idle_timeout,
						tunnel_ping_interval,
						response_consumed_bytes.clone(),
						terminal_error,
						flow_metrics.clone(),
					)
					.in_current_span(),
				);
				tokio::spawn(
					super::response::send_http_response_window_updates(
						in_flight_req.clone(),
						response_consumed_rx,
					)
					.in_current_span(),
				);

				let egress_bytes = egress_bytes.clone();
				let consumption_flow_metrics = flow_metrics.clone();
				response_builder.body(response_body.with_consumption(move |bytes| {
					let Ok(bytes) = u64::try_from(bytes) else {
						return;
					};
					let Ok(previous) = response_consumed_bytes.fetch_update(
						Ordering::AcqRel,
						Ordering::Acquire,
						|current| current.checked_add(bytes),
					) else {
						return;
					};
					let consumed = previous + bytes;
					egress_bytes.fetch_add(bytes, Ordering::AcqRel);
					consumption_flow_metrics.consume(bytes as usize);
					let _ = response_consumed_tx.send(consumed);
				}))?
			} else {
				let body = response_start.body.unwrap_or_default();
				egress_bytes.fetch_add(body.len() as u64, Ordering::AcqRel);
				let response =
					response_builder.body(ResponseBody::Full(Full::new(Bytes::from(body))))?;

				in_flight_req.stop(RequestStopResult::Success).await;
				response
			};

			Ok(response)
		};
		tokio::pin!(exchange);
		let (res, client_disconnected) = tokio::select! {
			biased;
			_ = client_disconnect.cancelled() => (
				Err(anyhow!("client disconnected before the HTTP response started")),
				true,
			),
			res = &mut exchange => (res, false),
		};

		if client_disconnected {
			send_http_request_abort(
				&in_flight_req,
				protocol::HttpStreamAbortReasonKind::Cancelled,
				Some("client disconnected before the HTTP response started".to_owned()),
			)
			.await;
			in_flight_req
				.stop(RequestStopResult::ClientDisconnect)
				.await;
		} else if res.is_err() {
			send_http_request_abort(
				&in_flight_req,
				protocol::HttpStreamAbortReasonKind::InternalError,
				Some("gateway stopped the HTTP request before completion".to_owned()),
			)
			.await;
			in_flight_req.stop(RequestStopResult::EnvoyError).await;
		}
		client_disconnect_guard.disarm();

		res
	}
}

async fn send_to_envoy_or_actor_stopped(
	in_flight_req: &crate::shared_state::InFlightRequestHandle,
	stopped_sub: &mut message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	actor_id: Id,
	phase: &'static str,
	message: protocol::ToEnvoyTunnelMessageKind,
	ephemeral: bool,
) -> Result<()> {
	tokio::select! {
		biased;
		_ = stopped_sub.next() => {
			tracing::debug!("actor stopped while sending request");
			Err(ActorStoppedWhileWaiting {
				actor_id: actor_id.to_string(),
				phase: phase.to_owned(),
			}
			.build())
		}
		res = in_flight_req.send_message(message, ephemeral) => res,
	}
}

/// Aborts the actor-side request if gateway processing exits before response ownership transfers.
struct HttpClientDisconnectGuard {
	in_flight_req: Option<crate::shared_state::InFlightRequestHandle>,
}

impl HttpClientDisconnectGuard {
	fn new(in_flight_req: crate::shared_state::InFlightRequestHandle) -> Self {
		Self {
			in_flight_req: Some(in_flight_req),
		}
	}

	fn disarm(&mut self) {
		self.in_flight_req = None;
	}
}

impl Drop for HttpClientDisconnectGuard {
	fn drop(&mut self) {
		let Some(in_flight_req) = self.in_flight_req.take() else {
			return;
		};
		tokio::spawn(async move {
			send_http_request_abort(
				&in_flight_req,
				protocol::HttpStreamAbortReasonKind::Cancelled,
				Some("client disconnected before the HTTP response started".to_owned()),
			)
			.await;
			in_flight_req
				.stop(RequestStopResult::ClientDisconnect)
				.await;
		});
	}
}
