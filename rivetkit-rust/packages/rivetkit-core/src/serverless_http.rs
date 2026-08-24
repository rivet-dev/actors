use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use anyhow::{Context, Result};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::any;
use futures::Stream;
use futures::StreamExt;
use futures::future::BoxFuture;
use http_body_util::LengthLimitError;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::CancellationToken;
use tower_http::services::ServeDir;

use crate::serverless::{CoreServerlessRuntime, ServerlessRequest, ServerlessResponse};

#[derive(Clone)]
pub struct ListenerConfig {
	/// Host to bind; accepts numeric IPs or DNS names. Defaults to `0.0.0.0`.
	pub host: Option<String>,
	pub port: u16,
	pub public_dir: Option<PathBuf>,
	/// Optional application handler for requests not owned by RivetKit.
	pub application: Option<ApplicationFetch>,
}

#[derive(Debug)]
pub struct ApplicationRequest {
	pub method: String,
	pub url: String,
	pub headers: HashMap<String, String>,
	pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct ApplicationResponse {
	pub status: u16,
	pub headers: HashMap<String, String>,
	pub body: Vec<u8>,
}

pub type ApplicationFetch = Arc<
	dyn Fn(ApplicationRequest) -> BoxFuture<'static, Result<ApplicationResponse>> + Send + Sync,
>;

#[derive(Clone)]
struct AppState {
	runtime: CoreServerlessRuntime,
	application: Option<ApplicationFetch>,
	shutdown_token: CancellationToken,
}

#[derive(Clone)]
struct ApplicationState {
	application: ApplicationFetch,
}

/// Bind a TCP listener and serve `runtime` over HTTP until `shutdown` fires.
pub async fn serve(
	runtime: CoreServerlessRuntime,
	listener: ListenerConfig,
	shutdown: CancellationToken,
) -> Result<()> {
	let host = listener.host.as_deref().unwrap_or("0.0.0.0");
	let port = listener.port;

	let state = AppState {
		runtime,
		application: listener.application.clone(),
		shutdown_token: shutdown.clone(),
	};

	let forward_service = any(forward_request).with_state(state);

	let router = match listener.public_dir.as_ref() {
		Some(dir) => Router::new().fallback_service(
			ServeDir::new(dir)
				.call_fallback_on_method_not_allowed(true)
				.fallback(forward_service),
		),
		None => Router::new().fallback_service(forward_service),
	};

	let tcp = tokio::net::TcpListener::bind((host, port))
		.await
		.with_context(|| format!("bind tcp listener on {host}:{port}"))?;
	let bound = tcp
		.local_addr()
		.context("read local address of bound listener")?;
	tracing::info!(host = %bound.ip(), port = bound.port(), "rivetkit server listening");

	let shutdown_fut = {
		let shutdown = shutdown.clone();
		async move { shutdown.cancelled().await }
	};

	axum::serve(tcp, router.into_make_service())
		.with_graceful_shutdown(shutdown_fut)
		.await
		.context("axum::serve returned an error")?;

	Ok(())
}

/// Bind a TCP listener that forwards every request to an application handler.
///
/// This listener is independent of the serverless runtime and can run beside a
/// normal serverful envoy using the same registry shutdown token.
pub async fn serve_application(
	listener: ListenerConfig,
	application: ApplicationFetch,
	max_body_bytes: usize,
	shutdown: CancellationToken,
) -> Result<()> {
	let host = listener.host.as_deref().unwrap_or("0.0.0.0");
	let port = listener.port;
	let forward_service = any(forward_application_request)
		.with_state((ApplicationState { application }, max_body_bytes));
	let router = match listener.public_dir.as_ref() {
		Some(dir) => Router::new().fallback_service(
			ServeDir::new(dir)
				.call_fallback_on_method_not_allowed(true)
				.fallback(forward_service),
		),
		None => Router::new().fallback_service(forward_service),
	};
	let tcp = tokio::net::TcpListener::bind((host, port))
		.await
		.with_context(|| format!("bind application tcp listener on {host}:{port}"))?;
	let bound = tcp
		.local_addr()
		.context("read application listener local address")?;
	tracing::info!(host = %bound.ip(), port = bound.port(), "application server listening");

	axum::serve(tcp, router.into_make_service())
		.with_graceful_shutdown(async move { shutdown.cancelled().await })
		.await
		.context("application axum::serve returned an error")?;
	Ok(())
}

async fn forward_application_request(
	State((state, max_body_bytes)): State<(ApplicationState, usize)>,
	request: Request,
) -> axum::response::Response {
	let (parts, body) = request.into_parts();
	let body = match axum::body::to_bytes(body, max_body_bytes).await {
		Ok(body) => body,
		Err(error) if is_length_limit_error(&error) => {
			tracing::warn!(max_body_bytes, "application request body exceeded limit");
			return (
				StatusCode::PAYLOAD_TOO_LARGE,
				[("content-type", "text/plain; charset=utf-8")],
				"Payload Too Large",
			)
				.into_response();
		}
		Err(error) => {
			tracing::warn!(?error, "failed to read application request body");
			return (
				StatusCode::BAD_REQUEST,
				[("content-type", "text/plain; charset=utf-8")],
				"Bad Request",
			)
				.into_response();
		}
	};
	let request = application_request_from_parts(parts, body);
	match (state.application)(request).await {
		Ok(response) => into_application_response(response),
		Err(error) => {
			tracing::error!(?error, "application request handler failed");
			(
				StatusCode::INTERNAL_SERVER_ERROR,
				[("content-type", "text/plain; charset=utf-8")],
				"Internal Server Error",
			)
				.into_response()
		}
	}
}

async fn forward_request(
	State(state): State<AppState>,
	request: Request,
) -> axum::response::Response {
	let (parts, body) = request.into_parts();
	let body_limit = state.runtime.max_request_body_bytes();
	let request_token = state.shutdown_token.child_token();
	let body_bytes = match axum::body::to_bytes(body, body_limit).await {
		Ok(bytes) => bytes,
		Err(error) if is_length_limit_error(&error) => {
			tracing::warn!(body_limit, "request body exceeded limit");
			return into_axum_response(state.runtime.incoming_too_long_response(), request_token);
		}
		Err(error) => {
			tracing::warn!(?error, "failed to read request body");
			return into_axum_response(
				state
					.runtime
					.invalid_request_response("failed to read request body"),
				request_token,
			);
		}
	};

	let application_request = application_request_from_parts(parts, body_bytes);
	let req = ServerlessRequest {
		method: application_request.method,
		url: application_request.url,
		headers: application_request.headers,
		body: application_request.body,
		cancel_token: request_token.clone(),
	};

	if state.runtime.handles_listener_request(&req.url) || state.application.is_none() {
		return into_axum_response(state.runtime.handle_request(req).await, request_token);
	}

	let application = state
		.application
		.as_ref()
		.expect("application checked above");
	match application(ApplicationRequest {
		method: req.method,
		url: req.url,
		headers: req.headers,
		body: req.body,
	})
	.await
	{
		Ok(response) => into_application_response(response),
		Err(error) => {
			tracing::error!(?error, "application request handler failed");
			(
				StatusCode::INTERNAL_SERVER_ERROR,
				[("content-type", "text/plain; charset=utf-8")],
				"Internal Server Error",
			)
				.into_response()
		}
	}
}

fn application_request_from_parts(
	parts: axum::http::request::Parts,
	body: Bytes,
) -> ApplicationRequest {
	let path_and_query = parts
		.uri
		.path_and_query()
		.map(|pq| pq.as_str())
		.unwrap_or("/");
	let url = format!("http://internal{path_and_query}");

	// Repeated header names get comma-joined per RFC 9110 §5.3.
	let mut headers: HashMap<String, String> = HashMap::new();
	for (name, value) in parts.headers.iter() {
		let Ok(value_str) = value.to_str() else {
			continue;
		};
		let key = name.as_str().to_ascii_lowercase();
		headers
			.entry(key)
			.and_modify(|existing| {
				existing.push_str(", ");
				existing.push_str(value_str);
			})
			.or_insert_with(|| value_str.to_owned());
	}

	ApplicationRequest {
		method: parts.method.as_str().to_owned(),
		url,
		headers,
		body: body.to_vec(),
	}
}

fn into_application_response(response: ApplicationResponse) -> axum::response::Response {
	let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
	let mut header_map = HeaderMap::with_capacity(response.headers.len());
	for (name, value) in response.headers {
		if let (Ok(name), Ok(value)) = (
			HeaderName::try_from(name.as_str()),
			HeaderValue::from_str(&value),
		) {
			header_map.append(name, value);
		}
	}
	(status, header_map, response.body).into_response()
}

fn into_axum_response(
	response: ServerlessResponse,
	request_token: CancellationToken,
) -> axum::response::Response {
	let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
	let mut header_map = HeaderMap::with_capacity(response.headers.len());
	for (name, value) in response.headers {
		if let (Ok(name), Ok(value)) = (
			HeaderName::try_from(name.as_str()),
			HeaderValue::from_str(&value),
		) {
			header_map.append(name, value);
		}
	}

	let stream = UnboundedReceiverStream::new(response.body).map(|chunk| match chunk {
		Ok(bytes) => Ok::<Bytes, std::io::Error>(Bytes::from(bytes)),
		Err(error) => {
			tracing::warn!(?error, "serverless stream error");
			Err(std::io::Error::other(format!(
				"{}.{}: {}",
				error.group, error.code, error.message
			)))
		}
	});

	// Cancel the runtime task when the response body is dropped.
	let guarded = CancelOnDropStream {
		inner: stream,
		_guard: CancelOnDrop {
			token: request_token,
		},
	};

	(status, header_map, Body::from_stream(guarded)).into_response()
}

fn is_length_limit_error(error: &axum::Error) -> bool {
	let mut source: Option<&dyn std::error::Error> = Some(error);
	while let Some(err) = source {
		if err.is::<LengthLimitError>() {
			return true;
		}
		source = err.source();
	}
	false
}

struct CancelOnDrop {
	token: CancellationToken,
}

impl Drop for CancelOnDrop {
	fn drop(&mut self) {
		self.token.cancel();
	}
}

struct CancelOnDropStream<S> {
	inner: S,
	_guard: CancelOnDrop,
}

impl<S: Stream + Unpin> Stream for CancelOnDropStream<S> {
	type Item = S::Item;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
		Pin::new(&mut self.inner).poll_next(cx)
	}
}
