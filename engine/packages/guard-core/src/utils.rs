use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::Response;
use hyper::StatusCode;
use hyper::header::HeaderName;
use rivet_api_builder::{ErrorResponse, RawErrorResponse};
use rivet_error::{INTERNAL_ERROR, RivetError};
use rivet_util::Id;
use std::time::Duration;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
use url::Url;

use crate::proxy_service::{X_FORWARDED_FOR, X_RIVET_ERROR};
use crate::response_body::ResponseBody;
use crate::{request_context::RequestContext, route::RouteTarget};

const X_RIVET_TARGET: HeaderName = HeaderName::from_static("x-rivet-target");
const X_RIVET_ACTOR: HeaderName = HeaderName::from_static("x-rivet-actor");
const X_RIVET_TOKEN: HeaderName = HeaderName::from_static("x-rivet-token");

// In-flight requests counter (semaphore)
pub(crate) struct InFlightCounter {
	count: usize,
	max: usize,
}

impl InFlightCounter {
	pub(crate) fn new(max: usize) -> Self {
		Self { count: 0, max }
	}

	pub(crate) fn try_acquire(&mut self) -> bool {
		if self.count < self.max {
			self.count += 1;
			true
		} else {
			false
		}
	}

	pub(crate) fn release(&mut self) {
		self.count = self.count.saturating_sub(1);
	}
}

// Calculate backoff duration for a given retry attempt
pub(crate) fn calculate_backoff(attempt: u32, initial_interval: u64) -> Duration {
	Duration::from_millis(initial_interval * 2u64.pow(attempt - 1))
}

/// Modifies the incoming request before it is proxied.
pub(crate) fn proxied_request_builder(
	req_parts: &hyper::http::request::Parts,
	req_ctx: &RequestContext,
	target: &RouteTarget,
) -> Result<hyper::http::request::Builder> {
	let scheme = if target.port == 443 { "https" } else { "http" };

	// Bracket raw IPv6 hosts
	let host = if target.host.contains(':') && !target.host.starts_with('[') {
		format!("[{}]", target.host)
	} else {
		target.host.clone()
	};

	// Ensure path starts with a leading slash
	let path = if target.path.starts_with('/') {
		target.path.clone()
	} else {
		format!("/{}", target.path)
	};

	let url = Url::parse(&format!("{scheme}://{host}:{}{}", target.port, path))
		.context("invalid scheme/host/port when building URL")?;

	// Build the proxied request
	let mut builder = hyper::Request::builder()
		.method(req_parts.method.clone())
		.uri(url.to_string());

	// Modify proxy headers
	let headers = builder
		.headers_mut()
		.expect("request builder unexpectedly in error state");

	headers.remove(X_RIVET_TARGET);
	headers.remove(X_RIVET_ACTOR);
	headers.remove(X_RIVET_TOKEN);

	add_proxy_headers_with_addr(headers, &req_ctx)?;

	Ok(builder)
}

pub(crate) fn add_proxy_headers_with_addr(
	headers: &mut hyper::HeaderMap,
	req_ctx: &RequestContext,
) -> Result<()> {
	// Copy headers except Host
	for (key, value) in &req_ctx.headers {
		if key != hyper::header::HOST {
			headers.insert(key.clone(), value.clone());
		}
	}

	// Add X-Forwarded-For header
	if let Some(existing) = req_ctx.headers.get(X_FORWARDED_FOR) {
		if let Ok(forwarded) = existing.to_str() {
			if !forwarded.contains(&req_ctx.remote_addr.ip().to_string()) {
				headers.insert(
					X_FORWARDED_FOR,
					hyper::header::HeaderValue::from_str(&format!(
						"{}, {}",
						forwarded,
						req_ctx.remote_addr.ip()
					))?,
				);
			}
		}
	} else {
		headers.insert(
			X_FORWARDED_FOR,
			hyper::header::HeaderValue::from_str(&req_ctx.remote_addr.ip().to_string())?,
		);
	}

	Ok(())
}

pub(crate) fn err_into_response(err: anyhow::Error) -> Result<Response<ResponseBody>> {
	let (status, error_response) =
		if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
			let status = match (rivet_err.group(), rivet_err.code()) {
				("api", "not_found") => StatusCode::NOT_FOUND,
				("api", "unauthorized") => StatusCode::UNAUTHORIZED,
				("api", "forbidden") => StatusCode::FORBIDDEN,
				("guard", "rate_limit") => StatusCode::TOO_MANY_REQUESTS,
				("guard", "upstream_error") => StatusCode::BAD_GATEWAY,
				("guard", "routing_error") => StatusCode::BAD_GATEWAY,
				("guard", "request_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "retry_attempts_exceeded") => StatusCode::BAD_GATEWAY,
				("guard", "service_unavailable") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "actor_stopped_while_waiting") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "tunnel_request_aborted") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "tunnel_message_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "tunnel_response_closed") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "gateway_response_start_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "actor_ready_timeout") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "no_route") => StatusCode::NOT_FOUND,
				("guard", "invalid_request_body") => StatusCode::PAYLOAD_TOO_LARGE,
				("guard", "invalid_response_body") => StatusCode::BAD_GATEWAY,
				("actor", "creation_rate_limit") => StatusCode::TOO_MANY_REQUESTS,
				("actor", "not_found") => StatusCode::NOT_FOUND,
				_ => StatusCode::BAD_REQUEST,
			};

			(status, ErrorResponse::from(rivet_err))
		} else if let Some(raw_err) = err
			.chain()
			.find_map(|x| x.downcast_ref::<RawErrorResponse>())
		{
			(raw_err.0, raw_err.1.clone())
		} else {
			(
				StatusCode::INTERNAL_SERVER_ERROR,
				ErrorResponse::from(&RivetError {
					kind: rivet_error::RivetErrorKind::Static(&rivet_error::INTERNAL_ERROR),
					meta: None,
					message: None,
					actor: None,
				}),
			)
		};

	let body_json = serde_json::to_vec(&error_response)?;
	let bytes = Bytes::from(body_json);

	Response::builder()
		.status(status)
		.header(hyper::header::CONTENT_TYPE, "application/json")
		.body(ResponseBody::Full(Full::new(bytes)))
		.map_err(Into::into)
}

pub(crate) fn should_retry_request(res: &Result<Response<ResponseBody>>) -> bool {
	match res {
		Ok(resp) => should_retry_request_inner(resp.status(), resp.headers()),
		Err(err) => {
			if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
				rivet_err.group() == "guard" && is_retryable_guard_http_error(rivet_err.code())
			} else {
				false
			}
		}
	}
}

fn is_retryable_guard_http_error(code: &str) -> bool {
	matches!(
		code,
		"service_unavailable"
			| "actor_ready_timeout"
			| "actor_stopped_while_waiting"
			| "tunnel_request_aborted"
			| "tunnel_message_timeout"
			| "tunnel_response_closed"
			| "gateway_response_start_timeout"
	)
}

// Determine if a response should trigger a retry: transient status and x-rivet-error.
pub(crate) fn should_retry_request_inner(status: StatusCode, headers: &hyper::HeaderMap) -> bool {
	(status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::GATEWAY_TIMEOUT)
		&& headers
			.get(X_RIVET_ERROR)
			.and_then(|value| value.to_str().ok())
			.and_then(|value| value.split_once('.'))
			.is_some_and(|(group, code)| group == "guard" && is_retryable_guard_http_error(code))
}

// Determine if a websocket error is retryable (e.g., transient UPS/tunnel issues)
pub(crate) fn is_retryable_ws_error(err: &anyhow::Error) -> bool {
	if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
		rivet_err.group() == "guard" && rivet_err.code() == "websocket_service_unavailable"
	} else {
		false
	}
}

pub fn is_ws_hibernate(err: &anyhow::Error) -> bool {
	if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
		rivet_err.group() == "guard" && rivet_err.code() == "websocket_service_hibernate"
	} else {
		false
	}
}

pub(crate) fn err_to_close_frame(err: anyhow::Error, ray_id: Id) -> CloseFrame {
	let rivet_err = err
		.chain()
		.find_map(|x| x.downcast_ref::<RivetError>())
		.cloned()
		.unwrap_or_else(|| RivetError::from(&INTERNAL_ERROR));

	let code = match (rivet_err.group(), rivet_err.code()) {
		("ws", "connection_closed") | ("ws", "eviction") => CloseCode::Normal,
		_ => CloseCode::Error,
	};

	match code {
		CloseCode::Normal => tracing::debug!("websocket closed"),
		_ => tracing::error!(?err, "websocket failed"),
	}

	let reason = format!("{}.{}#{}", rivet_err.group(), rivet_err.code(), ray_id);

	// NOTE: reason cannot be more than 123 bytes as per the WS protocol
	let reason = rivet_util::safe_slice(&reason, 0, 123).into();

	CloseFrame { code, reason }
}

pub(crate) fn to_hyper_close(frame: Option<CloseFrame>) -> hyper_tungstenite::tungstenite::Message {
	if let Some(frame) = frame {
		// Manual conversion to handle different tungstenite versions
		let code_num: u16 = frame.code.into();
		let reason = frame.reason.clone();

		tokio_tungstenite::tungstenite::Message::Close(Some(
			tokio_tungstenite::tungstenite::protocol::CloseFrame {
				code: code_num.into(),
				reason,
			},
		))
	} else {
		tokio_tungstenite::tungstenite::Message::Close(Some(
			tokio_tungstenite::tungstenite::protocol::CloseFrame {
				code: CloseCode::Normal,
				reason: "ws.closed".into(),
			},
		))
	}
}

#[cfg(test)]
#[path = "utils/tests.rs"]
mod tests;
