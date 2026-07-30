use std::{
	collections::HashMap,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use anyhow::{Result, anyhow};
use bytes::Bytes;
use gas::prelude::*;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Method, Request, Response, StatusCode, body::Body};
use rivet_envoy_protocol as protocol;
use rivet_guard_core::{
	ResponseBody,
	errors::{ActorStoppedWhileWaiting, InvalidRequestBody},
	request_context::RequestContext,
};
use tokio::sync::{mpsc, watch};
use tracing::Instrument;

use super::{
	ResponseBodyError,
	request::{
		should_stream_http_request_body_hint, stream_http_request_and_wait_for_response,
		wait_for_http_response_start,
	},
	response::drain_http_response_stream,
	send_http_request_abort,
};
use crate::{
	PegboardGateway2, metrics_task,
	request_metrics::{RequestKind, RequestMetrics},
	shared_state::{InFlightRequestCtx, RequestProtocol, RequestStopResult},
};

const PHASE_PRE_REQUEST: &str = "pre_request";

impl PegboardGateway2 {
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
		let actor_id = self.actor_id.to_string();
		let request_id = req_ctx.in_flight_request_id()?;

		// Buffer small and explicitly sized requests. Unknown or large bodies are forwarded with
		// backpressure while still enforcing the same total request-size limit.
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
		let request_stream = !matches!(req_ctx.method(), &Method::GET | &Method::HEAD)
			&& should_stream_http_request_body_hint(&request_body_size_hint);
		let (req_parts, body) = req.into_parts();
		let headers = req_parts
			.headers
			.iter()
			.filter_map(|(name, value)| {
				value
					.to_str()
					.ok()
					.map(|value_str| (name.to_string(), value_str.to_string()))
			})
			.collect::<HashMap<_, _>>();
		let (body_bytes, streaming_body) = if request_stream {
			(Bytes::new(), Some(body))
		} else {
			(
				Limited::new(body, max_request_body_size)
					.collect()
					.await
					.map_err(|error| anyhow!("failed to read body: {error}"))?
					.to_bytes(),
				None,
			)
		};
		ingress_bytes.fetch_add(body_bytes.len() as u64, Ordering::AcqRel);

		let mut stopped_sub = ctx
			.subscribe::<pegboard::workflows::actor2::Stopped>(("actor_id", self.actor_id))
			.await?;

		// Open the stopped subscription before resolving the envoy key so actor reallocation cannot
		// race between the lookup and request registration.
		let res = ctx
			.op(pegboard::ops::actor::get_for_gateway::Input {
				actor_id: self.actor_id,
			})
			.await?;
		let Some(envoy_key) = res.and_then(|x| x.envoy_key) else {
			return Err(ActorStoppedWhileWaiting {
				actor_id: self.actor_id.to_string(),
				phase: PHASE_PRE_REQUEST.to_owned(),
			}
			.build());
		};

		if self.envoy_key != envoy_key {
			tracing::debug!(
				gateway_envoy_key=%self.envoy_key,
				new_envoy_key=%envoy_key,
				"actor changed envoy while waiting for HTTP response",
			);
			return Err(ActorStoppedWhileWaiting {
				actor_id: self.actor_id.to_string(),
				phase: PHASE_PRE_REQUEST.to_owned(),
			}
			.build());
		}

		let tunnel_subject = pegboard::pubsub_subjects::EnvoyReceiverSubject::new(
			self.namespace_id,
			self.envoy_key.clone(),
		)
		.to_string();

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
		// Keep this armed until response ownership has safely transferred to either the returned body
		// or the explicit error path.
		let mut client_disconnect_guard = HttpClientDisconnectGuard::new(in_flight_req.clone());

		let res = async {
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
					stream: request_stream,
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
				let (body_tx, body_rx) =
					mpsc::channel::<Result<Bytes, ResponseBodyError>>(body_channel_capacity);
				let idle_timeout = self
					.ctx
					.config()
					.pegboard()
					.gateway_response_chunk_idle_timeout_ms()
					.map(|timeout_ms| Duration::from_millis(timeout_ms.max(1)));
				let expected_message_index =
					response_start_message_id.message_index.wrapping_add(1);
				let initial_body = response_start.body.take();

				tokio::spawn(
					drain_http_response_stream(
						in_flight_req.clone(),
						msg_rx,
						drop_rx,
						stopped_sub,
						body_tx,
						initial_body,
						expected_message_index,
						self.actor_id,
						idle_timeout,
						egress_bytes,
					)
					.in_current_span(),
				);

				response_builder.body(ResponseBody::Channel(body_rx))?
			} else {
				let body = response_start.body.unwrap_or_default();
				egress_bytes.fetch_add(body.len() as u64, Ordering::AcqRel);
				let response =
					response_builder.body(ResponseBody::Full(Full::new(Bytes::from(body))))?;

				in_flight_req.stop(RequestStopResult::Success).await;
				response
			};

			Ok(response)
		}
		.await;

		if res.is_err() {
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
				protocol::HttpStreamAbortReasonKind::ClientDisconnect,
				Some("client disconnected before the HTTP response started".to_owned()),
			)
			.await;
			in_flight_req
				.stop(RequestStopResult::ClientDisconnect)
				.await;
		});
	}
}
