use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::Response;
use hyper::StatusCode;
use hyper::header::HeaderName;
use parking_lot::Mutex;
use rivet_api_builder::{ErrorResponse, RawErrorResponse};
use rivet_error::{INTERNAL_ERROR, RivetError};
use rivet_metrics::{GaugeGuardExt, IntGaugeGuard};
use rivet_runner_protocol as protocol;
use rivet_util::throttle::{RateLimitMethod, RateLimiter};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
use url::Url;

use crate::metrics;
use crate::proxy_service::{X_FORWARDED_FOR, X_RIVET_ERROR};
use crate::response_body::ResponseBody;
use crate::{request_context::RequestContext, route::RouteTarget};

const X_RIVET_TARGET: HeaderName = HeaderName::from_static("x-rivet-target");
const X_RIVET_ACTOR: HeaderName = HeaderName::from_static("x-rivet-actor");
const X_RIVET_TOKEN: HeaderName = HeaderName::from_static("x-rivet-token");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionRejection {
	RateLimit,
	MaxInFlight,
}

/// Optional admission state for a single client IP. The cache containing this state is not created
/// when both admission controls are disabled.
pub(crate) struct ClientState {
	rate_limiter: Option<RateLimiter>,
	in_flight: Option<InFlightCounter>,
}

impl ClientState {
	pub(crate) fn new(rate_limit: Option<(u64, Duration)>, max_in_flight: Option<usize>) -> Self {
		Self {
			rate_limiter: rate_limit.map(|(requests, period)| {
				RateLimiter::new(RateLimitMethod::FixedWindow { requests, period })
			}),
			in_flight: max_in_flight.map(InFlightCounter::new),
		}
	}

	/// Consumes one rate limit token and one in-flight slot when those controls are configured. A
	/// rate limit token is still consumed when the in-flight limit rejects the request.
	pub(crate) fn try_admit(&mut self) -> std::result::Result<(), AdmissionRejection> {
		if let Some(rate_limiter) = &mut self.rate_limiter
			&& !rate_limiter.try_acquire()
		{
			return Err(AdmissionRejection::RateLimit);
		}

		if let Some(in_flight) = &mut self.in_flight
			&& !in_flight.try_acquire()
		{
			return Err(AdmissionRejection::MaxInFlight);
		}

		Ok(())
	}

	pub(crate) fn release_in_flight(&mut self) {
		if let Some(in_flight) = &mut self.in_flight {
			in_flight.release();
		}
	}
}

/// Owns an optional slot in a client's in-flight counter together with the request id registered in
/// the global in-flight request set and the matching increment on `IN_FLIGHT_REQUEST_COUNT`. All
/// configured resources are released in `Drop`, so a cancelled or panicking request cannot leak
/// any of them.
///
/// The permit is held behind an `Arc` on `RequestContext`. Tasks that outlive the initial response,
/// such as a proxied websocket, clone the context and therefore keep the slot and request id
/// reserved for as long as they are still using them.
pub(crate) struct InFlightPermit {
	client_state: Option<Arc<Mutex<ClientState>>>,
	in_flight_requests: Arc<scc::HashSet<protocol::RequestId>>,
	request_id: protocol::RequestId,
	_in_flight_metric: IntGaugeGuard,
}

impl InFlightPermit {
	/// Takes ownership of an optional slot already acquired from `client_state` and a request id
	/// already inserted into `in_flight_requests`.
	pub(crate) fn new(
		client_state: Option<Arc<Mutex<ClientState>>>,
		in_flight_requests: Arc<scc::HashSet<protocol::RequestId>>,
		request_id: protocol::RequestId,
	) -> Self {
		Self {
			client_state,
			in_flight_requests,
			request_id,
			_in_flight_metric: metrics::IN_FLIGHT_REQUEST_COUNT.inc_guard(),
		}
	}

	pub(crate) fn request_id(&self) -> protocol::RequestId {
		self.request_id
	}
}

impl Drop for InFlightPermit {
	fn drop(&mut self) {
		if let Some(client_state) = &self.client_state {
			client_state.lock().release_in_flight();
		}
		self.in_flight_requests.remove_sync(&self.request_id);
	}
}

// In-flight requests counter (semaphore)
struct InFlightCounter {
	count: usize,
	max: usize,
}

impl InFlightCounter {
	fn new(max: usize) -> Self {
		Self { count: 0, max }
	}

	fn try_acquire(&mut self) -> bool {
		if self.count < self.max {
			self.count += 1;
			true
		} else {
			false
		}
	}

	fn release(&mut self) {
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

/// Renders a request URI for logs with the values of credential query parameters replaced.
pub(crate) fn redact_uri_for_logs(uri: &hyper::Uri) -> String {
	let path_and_query = uri
		.path_and_query()
		.map(|path_and_query| path_and_query.as_str())
		.unwrap_or_else(|| uri.path());

	match (uri.scheme_str(), uri.authority()) {
		(Some(scheme), Some(authority)) => {
			format!(
				"{scheme}://{authority}{}",
				redact_path_for_logs(path_and_query)
			)
		}
		_ => redact_path_for_logs(path_and_query),
	}
}

/// Renders a request path and query string for logs with the values of credential query
/// parameters replaced.
pub fn redact_path_for_logs(path_and_query: &str) -> String {
	let Some((path, query)) = path_and_query.split_once('?') else {
		return path_and_query.to_string();
	};

	let redacted_query = query
		.split('&')
		.map(|pair| {
			let raw_key = pair.split('=').next().unwrap_or_default();
			let is_credential = url::form_urlencoded::parse(raw_key.as_bytes())
				.next()
				.is_some_and(|(key, _)| key.to_ascii_lowercase().ends_with("token"));

			if is_credential {
				format!("{raw_key}=REDACTED")
			} else {
				pair.to_string()
			}
		})
		.collect::<Vec<_>>()
		.join("&");

	format!("{path}?{redacted_query}")
}

/// Headers that are scoped to a single connection and must not be forwarded to the upstream
/// service. See RFC 9110 section 7.6.1. The outgoing request generates its own versions of these
/// where they are needed, including the `Connection` and `Upgrade` headers of a websocket
/// handshake.
const HOP_BY_HOP_HEADERS: [HeaderName; 8] = [
	hyper::header::CONNECTION,
	HeaderName::from_static("keep-alive"),
	hyper::header::PROXY_AUTHENTICATE,
	hyper::header::PROXY_AUTHORIZATION,
	hyper::header::TE,
	hyper::header::TRAILER,
	hyper::header::TRANSFER_ENCODING,
	hyper::header::UPGRADE,
];

/// Headers belonging to the client's websocket handshake with guard. Guard terminates that
/// handshake and opens its own to the upstream service, so the outgoing request keeps the key it
/// generated for itself rather than echoing the client's.
const CLIENT_HANDSHAKE_HEADERS: [HeaderName; 2] = [
	hyper::header::SEC_WEBSOCKET_KEY,
	hyper::header::SEC_WEBSOCKET_VERSION,
];

pub(crate) fn add_proxy_headers_with_addr(
	headers: &mut hyper::HeaderMap,
	req_ctx: &RequestContext,
) -> Result<()> {
	// Copy every header the upstream service is allowed to see. Appending rather than inserting
	// keeps all the values of a header the client sent as multiple field lines, such as a cookie
	// split across lines by an HTTP/2 client.
	for (key, value) in &req_ctx.headers {
		if key == hyper::header::HOST
			|| HOP_BY_HOP_HEADERS.contains(key)
			|| CLIENT_HANDSHAKE_HEADERS.contains(key)
		{
			continue;
		}

		headers.append(key.clone(), value.clone());
	}

	// Record this hop in X-Forwarded-For unless the client already claims to have come from this
	// address. Appending keeps any existing chain intact, including a chain the client sent as
	// multiple field lines.
	let client_ip = req_ctx.remote_addr.ip().to_string();
	let already_forwarded = req_ctx
		.headers
		.get_all(X_FORWARDED_FOR)
		.into_iter()
		.filter_map(|value| value.to_str().ok())
		.flat_map(|value| value.split(','))
		.any(|entry| entry.trim() == client_ip);

	if !already_forwarded {
		headers.append(
			X_FORWARDED_FOR,
			hyper::header::HeaderValue::from_str(&client_ip)?,
		);
	}

	Ok(())
}

/// Label used when an error does not match any of the types known to `error_type_name`.
const UNKNOWN_ERROR_LABEL: &str = "unknown";

/// Expands to the name of the first type in the list that `$err` downcasts to. The written path is
/// used rather than `type_name` because `type_name` exposes private module paths such as
/// `std::io::error::Error`.
macro_rules! match_error_type {
	($err:expr, $($ty:ty),* $(,)?) => {{
		let err = $err;
		None$(.or_else(|| err.downcast_ref::<$ty>().map(|_| stringify!($ty))))*
	}};
}

/// Returns the type name of an error if it is one of the foreign error types guard commonly
/// encounters.
fn error_type_name(err: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
	match_error_type!(
		err,
		std::io::Error,
		tokio_tungstenite::tungstenite::Error,
		hyper_tungstenite::tungstenite::Error,
		hyper::Error,
		hyper_util::client::legacy::Error,
		hyper::http::Error,
		hyper::header::InvalidHeaderValue,
		hyper::header::ToStrError,
		hyper::header::InvalidHeaderName,
		serde_json::Error,
		url::ParseError,
		tokio::time::error::Elapsed,
	)
}

/// Builds a bounded metric label for an error. Formal errors become `{group}.{code}`. Anything else
/// falls back to the error's type name, since error messages frequently embed request specific
/// values such as ids, hosts, and paths that would make the label unbounded.
pub(crate) fn error_metric_label(err: &anyhow::Error) -> String {
	if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
		return format!("{}.{}", rivet_err.group(), rivet_err.code());
	}

	if let Some(raw_err) = err
		.chain()
		.find_map(|x| x.downcast_ref::<RawErrorResponse>())
	{
		return format!("{}.{}", raw_err.1.group, raw_err.1.code);
	}

	err.chain()
		.find_map(error_type_name)
		.unwrap_or(UNKNOWN_ERROR_LABEL)
		.to_string()
}

pub(crate) fn err_into_response(err: anyhow::Error) -> Result<Response<ResponseBody>> {
	let (status, error_response) =
		if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
			let status = match (rivet_err.group(), rivet_err.code()) {
				("api", "not_found") => StatusCode::NOT_FOUND,
				("api", "unauthorized") => StatusCode::UNAUTHORIZED,
				("api", "forbidden") => StatusCode::FORBIDDEN,
				("acl", "token_not_found") => StatusCode::UNAUTHORIZED,
				("acl", "token_expired") => StatusCode::UNAUTHORIZED,
				("acl", "insufficient_permissions") => StatusCode::FORBIDDEN,
				("guard", "rate_limit") => StatusCode::TOO_MANY_REQUESTS,
				("guard", "upstream_error") => StatusCode::BAD_GATEWAY,
				("guard", "routing_error") => StatusCode::BAD_GATEWAY,
				("guard", "request_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "route_dispatch_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "route_api_public_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "route_compute_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "route_auth_check_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "retry_attempts_exceeded") => StatusCode::BAD_GATEWAY,
				("pegboard", "route_subscribe_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("pegboard", "route_fetch_actor_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("pegboard", "route_auth_check_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("pegboard", "route_wake_signal_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("pegboard", "route_resolve_query_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "service_unavailable") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "actor_wake_retries_exceeded") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "actor_stopped_while_waiting") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "tunnel_request_aborted") => StatusCode::SERVICE_UNAVAILABLE,
				("guard", "tunnel_message_timeout") => StatusCode::GATEWAY_TIMEOUT,
				("guard", "request_delivery_unconfirmed") => StatusCode::SERVICE_UNAVAILABLE,
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
	let error_code = format!("{}.{}", error_response.group, error_response.code);

	Response::builder()
		.status(status)
		.header(hyper::header::CONTENT_TYPE, "application/json")
		.header(X_RIVET_ERROR, error_code)
		.body(ResponseBody::Full(Full::new(bytes)))
		.map_err(Into::into)
}

pub(crate) fn should_retry_request(res: &Result<Response<ResponseBody>>) -> bool {
	match res {
		Ok(resp) => should_retry_request_inner(resp.status(), resp.headers()),
		Err(err) => should_retry_error(err),
	}
}

pub(crate) fn should_retry_error(err: &anyhow::Error) -> bool {
	if let Some(rivet_err) = err.chain().find_map(|x| x.downcast_ref::<RivetError>()) {
		rivet_err.group() == "guard" && is_retryable_guard_http_error(rivet_err.code())
	} else {
		false
	}
}

fn is_retryable_guard_http_error(code: &str) -> bool {
	matches!(
		code,
		"service_unavailable"
			| "actor_ready_timeout"
			| "actor_wake_retries_exceeded"
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
		rivet_err.group() == "guard"
			&& matches!(
				rivet_err.code(),
				"websocket_closed_before_open"
					| "actor_stopped_while_waiting_for_websocket_open"
					| "websocket_open_dropped"
					| "websocket_open_response_closed"
					| "websocket_open_timeout"
					| "websocket_tunnel_subscription_closed"
			)
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

pub(crate) fn err_to_close_frame(err: anyhow::Error, ray_id: &str) -> CloseFrame {
	metrics::WEBSOCKET_CLOSE_ERROR_TOTAL
		.with_label_values(&[&error_metric_label(&err)])
		.inc();

	let rivet_err = err
		.chain()
		.find_map(|x| x.downcast_ref::<RivetError>())
		.cloned()
		.unwrap_or_else(|| RivetError::from(&INTERNAL_ERROR));

	let code = match (rivet_err.group(), rivet_err.code()) {
		("ws", "connection_closed") | ("ws", "eviction") => CloseCode::Normal,
		_ => CloseCode::Error,
	};

	// Log the error
	match code {
		CloseCode::Normal => tracing::debug!("websocket closed"),
		_ => {
			// Downgrade log if error is `ResetWithoutClosingHandshake` or `SendAfterClosing`
			if err.chain().any(|x| {
				match x.downcast_ref::<tokio_tungstenite::tungstenite::Error>() {
					Some(tokio_tungstenite::tungstenite::Error::AlreadyClosed)
					| Some(tokio_tungstenite::tungstenite::Error::Protocol(
						ProtocolError::ResetWithoutClosingHandshake,
					))
					| Some(tokio_tungstenite::tungstenite::Error::Protocol(
						ProtocolError::SendAfterClosing,
					)) => true,
					Some(tokio_tungstenite::tungstenite::Error::Io(io_err))
						if io_err.to_string().contains("connection reset by peer") =>
					{
						true
					}
					Some(tokio_tungstenite::tungstenite::Error::Io(io_err))
						if io_err.kind() == std::io::ErrorKind::BrokenPipe =>
					{
						true
					}
					_ => false,
				}
			}) {
				tracing::warn!(?err, "websocket failed");
			} else if rivet_err.group() == "core" && rivet_err.code() == "internal_error" {
				tracing::error!(?err, "websocket failed");
			} else {
				tracing::warn!(?err, "websocket failed");
			}
		}
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
