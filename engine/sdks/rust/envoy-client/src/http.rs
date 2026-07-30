use std::collections::HashMap;

use rivet_envoy_protocol as protocol;
use tokio::sync::{mpsc, watch};

pub const HTTP_BODY_STREAM_CHANNEL_CAPACITY: usize = 16;
pub const HTTP_BODY_MAX_CHUNK_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct HttpRequestBodyError {
	pub reason: protocol::HttpStreamAbortReason,
}

impl std::fmt::Display for HttpRequestBodyError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match &self.reason.detail {
			Some(detail) => write!(f, "{:?}: {detail}", self.reason.kind),
			None => write!(f, "{:?}", self.reason.kind),
		}
	}
}

impl std::error::Error for HttpRequestBodyError {}

#[derive(Debug)]
pub struct HttpRequestBodyStream {
	rx: mpsc::Receiver<Vec<u8>>,
	abort_rx: watch::Receiver<Option<HttpRequestBodyError>>,
}

impl HttpRequestBodyStream {
	pub fn new(
		rx: mpsc::Receiver<Vec<u8>>,
		abort_rx: watch::Receiver<Option<HttpRequestBodyError>>,
	) -> Self {
		Self { rx, abort_rx }
	}

	pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, HttpRequestBodyError> {
		loop {
			if let Some(error) = self.abort_rx.borrow().clone() {
				return Err(error);
			}

			tokio::select! {
				biased;
				changed = self.abort_rx.changed() => {
					if changed.is_ok() {
						continue;
					}
					return Ok(self.rx.recv().await);
				}
				chunk = self.rx.recv() => return Ok(chunk),
			}
		}
	}
}

/// HTTP request/response types used by the envoy client.
pub struct HttpRequest {
	pub method: String,
	pub path: String,
	pub headers: HashMap<String, String>,
	pub body: Option<Vec<u8>>,
	/// If the request is streamed, body chunks arrive on this channel.
	pub body_stream: Option<HttpRequestBodyStream>,
}

pub struct HttpResponse {
	pub status: u16,
	pub headers: HashMap<String, String>,
	pub body: Option<Vec<u8>>,
	/// If set, the response is streamed. The envoy client reads chunks and sends
	/// `ToRivetResponseChunk` for each one.
	pub body_stream: Option<HttpResponseBodyStream>,
}

/// A chunk in a streaming HTTP response.
pub enum ResponseChunk {
	Data { data: Vec<u8>, finish: bool },
	Error(String),
}

pub struct HttpResponseBodyStream {
	rx: mpsc::Receiver<ResponseChunk>,
	on_drop: Option<Box<dyn FnOnce() + Send>>,
}

impl HttpResponseBodyStream {
	pub fn set_on_drop(&mut self, on_drop: impl FnOnce() + Send + 'static) {
		self.on_drop = Some(Box::new(on_drop));
	}

	pub async fn recv(&mut self) -> Option<ResponseChunk> {
		self.rx.recv().await
	}
}

impl From<mpsc::Receiver<ResponseChunk>> for HttpResponseBodyStream {
	fn from(rx: mpsc::Receiver<ResponseChunk>) -> Self {
		Self { rx, on_drop: None }
	}
}

impl Drop for HttpResponseBodyStream {
	fn drop(&mut self) {
		if let Some(on_drop) = self.on_drop.take() {
			on_drop();
		}
	}
}
