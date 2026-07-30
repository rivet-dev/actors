use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use rivet_envoy_client::config::{HttpRequestBodyStream, ResponseChunk};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::actor::connection::ConnHandle;
use crate::actor::lifecycle_hooks::Reply;
use crate::actor::schedule::ScheduledFireInfo;
use crate::actor::task_types::ShutdownKind;
use crate::error::ProtocolError;
use crate::types::ConnId;
use crate::websocket::WebSocket;

#[derive(Clone, Debug)]
pub struct Request {
	inner: http::Request<Vec<u8>>,
	body_stream: Option<Arc<Mutex<Option<HttpRequestBodyStream>>>>,
	cancel_token: CancellationToken,
}

impl Request {
	pub fn new(body: Vec<u8>) -> Self {
		Self {
			inner: http::Request::new(body),
			body_stream: None,
			cancel_token: CancellationToken::new(),
		}
	}

	pub fn from_parts(
		method: &str,
		uri: &str,
		headers: HashMap<String, String>,
		body: Vec<u8>,
	) -> Result<Self> {
		Self::from_parts_with_stream(method, uri, headers, body, None)
	}

	pub fn from_parts_with_stream(
		method: &str,
		uri: &str,
		headers: HashMap<String, String>,
		body: Vec<u8>,
		body_stream: Option<HttpRequestBodyStream>,
	) -> Result<Self> {
		let method = method
			.parse::<http::Method>()
			.map_err(|error| invalid_http_request("method", format!("{method}: {error}")))?;
		let uri = uri
			.parse::<http::Uri>()
			.map_err(|error| invalid_http_request("uri", format!("{uri}: {error}")))?;
		let mut request = http::Request::builder()
			.method(method)
			.uri(uri)
			.body(body)?;

		for (name, value) in headers {
			let header_name: http::header::HeaderName = name
				.parse()
				.map_err(|error| invalid_http_request("header name", format!("{name}: {error}")))?;
			let header_value: http::header::HeaderValue = value.parse().map_err(|error| {
				invalid_http_request("header value", format!("{name}: {error}"))
			})?;
			request.headers_mut().insert(header_name, header_value);
		}

		Ok(Self {
			inner: request,
			body_stream: body_stream.map(|rx| Arc::new(Mutex::new(Some(rx)))),
			cancel_token: CancellationToken::new(),
		})
	}

	pub fn to_parts(&self) -> (String, String, HashMap<String, String>, Vec<u8>) {
		(
			self.method().to_string(),
			self.uri().to_string(),
			self.headers()
				.iter()
				.map(|(name, value)| {
					(
						name.to_string(),
						String::from_utf8_lossy(value.as_bytes()).into_owned(),
					)
				})
				.collect(),
			self.body().clone(),
		)
	}

	pub fn has_body_stream(&self) -> bool {
		self.body_stream
			.as_ref()
			.is_some_and(|body_stream| body_stream.lock().is_some())
	}

	pub fn take_body_stream(&self) -> Option<HttpRequestBodyStream> {
		self.body_stream
			.as_ref()
			.and_then(|body_stream| body_stream.lock().take())
	}

	pub fn cancellation_token(&self) -> CancellationToken {
		self.cancel_token.clone()
	}

	pub async fn into_buffered(mut self) -> Result<Self> {
		if let Some(body_stream) = &self.body_stream {
			let mut body_stream = body_stream.lock().take();
			if let Some(mut body_stream) = body_stream.take() {
				while let Some(chunk) = body_stream.recv().await? {
					self.inner.body_mut().extend_from_slice(&chunk);
				}
			}
		}
		self.body_stream = None;
		Ok(self)
	}

	pub fn into_inner(self) -> http::Request<Vec<u8>> {
		self.inner
	}

	pub fn into_body(self) -> Vec<u8> {
		self.inner.into_body()
	}
}

impl Default for Request {
	fn default() -> Self {
		Self::new(Vec::new())
	}
}

impl Deref for Request {
	type Target = http::Request<Vec<u8>>;

	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}

impl DerefMut for Request {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.inner
	}
}

impl From<http::Request<Vec<u8>>> for Request {
	fn from(value: http::Request<Vec<u8>>) -> Self {
		Self {
			inner: value,
			body_stream: None,
			cancel_token: CancellationToken::new(),
		}
	}
}

impl From<Request> for http::Request<Vec<u8>> {
	fn from(value: Request) -> Self {
		value.inner
	}
}

#[derive(Clone, Debug)]
pub struct Response(http::Response<Vec<u8>>);

impl Response {
	pub fn new(body: Vec<u8>) -> Self {
		Self(http::Response::new(body))
	}

	pub fn from_parts(
		status: u16,
		headers: HashMap<String, String>,
		body: Vec<u8>,
	) -> Result<Self> {
		let mut response = http::Response::new(body);
		*response.status_mut() = status
			.try_into()
			.map_err(|error| invalid_http_response("status", format!("{status}: {error}")))?;

		for (name, value) in headers {
			let header_name: http::header::HeaderName = name.parse().map_err(|error| {
				invalid_http_response("header name", format!("{name}: {error}"))
			})?;
			let header_value: http::header::HeaderValue = value.parse().map_err(|error| {
				invalid_http_response("header value", format!("{name}: {error}"))
			})?;
			response.headers_mut().insert(header_name, header_value);
		}

		Ok(Self(response))
	}

	pub fn to_parts(&self) -> (u16, HashMap<String, String>, Vec<u8>) {
		(
			self.status().as_u16(),
			self.headers()
				.iter()
				.map(|(name, value)| {
					(
						name.to_string(),
						String::from_utf8_lossy(value.as_bytes()).into_owned(),
					)
				})
				.collect(),
			self.body().clone(),
		)
	}

	pub fn into_inner(self) -> http::Response<Vec<u8>> {
		self.0
	}

	pub fn into_body(self) -> Vec<u8> {
		self.0.into_body()
	}
}

fn invalid_http_request(field: &str, reason: String) -> anyhow::Error {
	ProtocolError::InvalidHttpRequest {
		field: field.to_owned(),
		reason,
	}
	.build()
}

fn invalid_http_response(field: &str, reason: String) -> anyhow::Error {
	ProtocolError::InvalidHttpResponse {
		field: field.to_owned(),
		reason,
	}
	.build()
}

impl Default for Response {
	fn default() -> Self {
		Self::new(Vec::new())
	}
}

impl Deref for Response {
	type Target = http::Response<Vec<u8>>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for Response {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl From<http::Response<Vec<u8>>> for Response {
	fn from(value: http::Response<Vec<u8>>) -> Self {
		Self(value)
	}
}

impl From<Response> for http::Response<Vec<u8>> {
	fn from(value: Response) -> Self {
		value.0
	}
}

pub struct StreamingResponse {
	status: u16,
	headers: HashMap<String, String>,
	body_stream: mpsc::Receiver<ResponseChunk>,
}

impl StreamingResponse {
	pub fn from_parts(
		status: u16,
		headers: HashMap<String, String>,
		body_stream: mpsc::Receiver<ResponseChunk>,
	) -> Result<Self> {
		let status_code: http::StatusCode = status
			.try_into()
			.map_err(|error| invalid_http_response("status", format!("{status}: {error}")))?;
		for (name, value) in &headers {
			let _: http::header::HeaderName = name.parse().map_err(|error| {
				invalid_http_response("header name", format!("{name}: {error}"))
			})?;
			let _: http::header::HeaderValue = value.parse().map_err(|error| {
				invalid_http_response("header value", format!("{name}: {error}"))
			})?;
		}

		Ok(Self {
			status: status_code.as_u16(),
			headers,
			body_stream,
		})
	}

	pub fn into_parts(self) -> (u16, HashMap<String, String>, mpsc::Receiver<ResponseChunk>) {
		(self.status, self.headers, self.body_stream)
	}
}

pub enum ActorHttpResponse {
	Buffered(Response),
	Stream(StreamingResponse),
}

impl From<Response> for ActorHttpResponse {
	fn from(value: Response) -> Self {
		Self::Buffered(value)
	}
}

impl From<StreamingResponse> for ActorHttpResponse {
	fn from(value: StreamingResponse) -> Self {
		Self::Stream(value)
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StateDelta {
	ActorState(Vec<u8>),
	ConnHibernation { conn: ConnId, bytes: Vec<u8> },
	ConnHibernationRemoved(ConnId),
}

/// One workflow-engine KV write that must commit atomically with its actor
/// state snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowKvWrite {
	pub key: Vec<u8>,
	pub value: Vec<u8>,
}

impl StateDelta {
	pub(crate) fn payload_len(&self) -> usize {
		match self {
			Self::ActorState(bytes) | Self::ConnHibernation { bytes, .. } => bytes.len(),
			Self::ConnHibernationRemoved(_) => 0,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SerializeStateReason {
	Save,
	Inspector,
}

impl SerializeStateReason {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Save => "save",
			Self::Inspector => "inspector",
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueSendStatus {
	Completed,
	TimedOut,
}

impl QueueSendStatus {
	pub(crate) fn as_str(&self) -> &'static str {
		match self {
			Self::Completed => "completed",
			Self::TimedOut => "timedOut",
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueSendResult {
	pub status: QueueSendStatus,
	pub response: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum ActorEvent {
	Action {
		name: String,
		args: Vec<u8>,
		conn: Option<ConnHandle>,
		scheduled_fire: Option<ScheduledFireInfo>,
		reply: Reply<Vec<u8>>,
	},
	HttpRequest {
		request: Request,
		reply: Reply<ActorHttpResponse>,
	},
	QueueSend {
		name: String,
		body: Vec<u8>,
		conn: ConnHandle,
		request: Request,
		wait: bool,
		timeout_ms: Option<u64>,
		reply: Reply<QueueSendResult>,
	},
	WebSocketOpen {
		conn: ConnHandle,
		ws: WebSocket,
		request: Option<Request>,
		reply: Reply<()>,
	},
	ConnectionPreflight {
		conn: ConnHandle,
		params: Vec<u8>,
		request: Option<Request>,
		reply: Reply<()>,
	},
	ConnectionOpen {
		conn: ConnHandle,
		request: Option<Request>,
		reply: Reply<()>,
	},
	ConnectionClosed {
		conn: ConnHandle,
	},
	SubscribeRequest {
		conn: ConnHandle,
		event_name: String,
		reply: Reply<()>,
	},
	SerializeState {
		reason: SerializeStateReason,
		reply: Reply<Vec<StateDelta>>,
	},
	RunGracefulCleanup {
		reason: ShutdownKind,
		reply: Reply<()>,
	},
	DisconnectConn {
		conn_id: ConnId,
		reply: Reply<()>,
	},
	#[cfg(test)]
	BeginSleep,
	#[cfg(test)]
	FinalizeSleep {
		reply: Reply<()>,
	},
	#[cfg(test)]
	Destroy {
		reply: Reply<()>,
	},
	WorkflowHistoryRequested {
		reply: Reply<Option<Vec<u8>>>,
	},
	WorkflowReplayRequested {
		entry_id: Option<String>,
		reply: Reply<Option<Vec<u8>>>,
	},
	/// The persisted logical run deadline became due. Foreign runtimes ensure
	/// their run callback is active without restarting the actor runtime itself.
	RunWake {
		wake_at: i64,
		wake_revision: u64,
		reply: Reply<()>,
	},
}

impl ActorEvent {
	pub(crate) fn kind(&self) -> &'static str {
		match self {
			Self::Action { .. } => "action",
			Self::HttpRequest { .. } => "http_request",
			Self::QueueSend { .. } => "queue_send",
			Self::WebSocketOpen { .. } => "websocket_open",
			Self::ConnectionPreflight { .. } => "connection_preflight",
			Self::ConnectionOpen { .. } => "connection_open",
			Self::ConnectionClosed { .. } => "connection_closed",
			Self::SubscribeRequest { .. } => "subscribe_request",
			Self::SerializeState { reason, .. } => match reason {
				SerializeStateReason::Save => "serialize_state_save",
				SerializeStateReason::Inspector => "serialize_state_inspector",
			},
			Self::RunGracefulCleanup { reason, .. } => match reason {
				ShutdownKind::Sleep => "run_sleep_cleanup",
				ShutdownKind::Destroy => "run_destroy_cleanup",
			},
			Self::DisconnectConn { .. } => "disconnect_conn",
			#[cfg(test)]
			Self::BeginSleep => "begin_sleep",
			#[cfg(test)]
			Self::FinalizeSleep { .. } => "finalize_sleep",
			#[cfg(test)]
			Self::Destroy { .. } => "destroy",
			Self::WorkflowHistoryRequested { .. } => "workflow_history_requested",
			Self::WorkflowReplayRequested { .. } => "workflow_replay_requested",
			Self::RunWake { .. } => "run_wake",
		}
	}
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/messages.rs"]
mod tests;
