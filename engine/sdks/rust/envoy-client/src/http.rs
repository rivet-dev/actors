use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use anyhow::{Context, Result as AnyhowResult, ensure};
use parking_lot::Mutex as SyncMutex;
use rivet_envoy_protocol as protocol;
use tokio::sync::{Mutex, Notify, mpsc, watch};

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
	source: HttpRequestBodySource,
	event_tx: Option<mpsc::UnboundedSender<RequestBodyEvent>>,
	terminal: bool,
}

#[derive(Debug)]
enum HttpRequestBodySource {
	Legacy {
		rx: mpsc::Receiver<Vec<u8>>,
		abort_rx: watch::Receiver<Option<HttpRequestBodyError>>,
	},
	FlowControlled(Arc<HttpRequestBodyQueue>),
}

#[derive(Debug)]
struct HttpRequestBodyQueueState {
	chunks: VecDeque<Vec<u8>>,
	finished: bool,
	abort: Option<HttpRequestBodyError>,
	receiver_closed: bool,
}

/// Byte-windowed request bodies use a coalescing queue instead of a frame-count bounded channel.
/// The producer is the synchronous actor inbox, so this is a deliberately short forced-sync lock;
/// it is never held across an await. Byte credit separately caps the total queued payload.
#[derive(Debug)]
pub(crate) struct HttpRequestBodyQueue {
	state: SyncMutex<HttpRequestBodyQueueState>,
	ready: Notify,
}

impl HttpRequestBodyQueue {
	pub(crate) fn new() -> Arc<Self> {
		Arc::new(Self {
			state: SyncMutex::new(HttpRequestBodyQueueState {
				chunks: VecDeque::new(),
				finished: false,
				abort: None,
				receiver_closed: false,
			}),
			ready: Notify::new(),
		})
	}

	pub(crate) fn push(&self, mut data: Vec<u8>) -> bool {
		let mut state = self.state.lock();
		if state.finished || state.abort.is_some() || state.receiver_closed {
			return false;
		}

		if let Some(last) = state.chunks.back_mut() {
			let available = HTTP_BODY_MAX_CHUNK_SIZE.saturating_sub(last.len());
			let append = available.min(data.len());
			last.extend_from_slice(&data[..append]);
			data.drain(..append);
		}
		for chunk in data.chunks(HTTP_BODY_MAX_CHUNK_SIZE) {
			state.chunks.push_back(chunk.to_vec());
		}
		drop(state);
		self.ready.notify_one();
		true
	}

	pub(crate) fn finish(&self) {
		let mut state = self.state.lock();
		if state.abort.is_none() {
			state.finished = true;
		}
		drop(state);
		self.ready.notify_waiters();
	}

	pub(crate) fn abort(&self, error: HttpRequestBodyError) {
		let mut state = self.state.lock();
		if state.abort.is_none() && !state.finished {
			state.chunks.clear();
			state.abort = Some(error);
		}
		drop(state);
		self.ready.notify_waiters();
	}

	fn close_receiver(&self) {
		let mut state = self.state.lock();
		state.receiver_closed = true;
		state.chunks.clear();
		drop(state);
		self.ready.notify_waiters();
	}

	async fn recv(&self) -> Result<Option<Vec<u8>>, HttpRequestBodyError> {
		loop {
			let notified = self.ready.notified();
			{
				let mut state = self.state.lock();
				if let Some(error) = &state.abort {
					return Err(error.clone());
				}
				if let Some(chunk) = state.chunks.pop_front() {
					return Ok(Some(chunk));
				}
				if state.finished || state.receiver_closed {
					return Ok(None);
				}
			}
			notified.await;
		}
	}
}

#[derive(Debug)]
pub(crate) enum RequestBodyEvent {
	Consumed(u64),
	Cancelled,
}

impl HttpRequestBodyStream {
	fn handle_chunk(
		&mut self,
		chunk: Option<Vec<u8>>,
	) -> Result<Option<Vec<u8>>, HttpRequestBodyError> {
		match chunk {
			Some(chunk) => {
				if let Some(event_tx) = &self.event_tx {
					let _ = event_tx.send(RequestBodyEvent::Consumed(chunk.len() as u64));
				}
				Ok(Some(chunk))
			}
			None => {
				self.terminal = true;
				Ok(None)
			}
		}
	}

	pub fn new(
		rx: mpsc::Receiver<Vec<u8>>,
		abort_rx: watch::Receiver<Option<HttpRequestBodyError>>,
	) -> Self {
		Self {
			source: HttpRequestBodySource::Legacy { rx, abort_rx },
			event_tx: None,
			terminal: false,
		}
	}

	pub(crate) fn new_with_flow_control(
		queue: Arc<HttpRequestBodyQueue>,
		event_tx: mpsc::UnboundedSender<RequestBodyEvent>,
	) -> Self {
		Self {
			source: HttpRequestBodySource::FlowControlled(queue),
			event_tx: Some(event_tx),
			terminal: false,
		}
	}

	pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, HttpRequestBodyError> {
		let chunk = match &mut self.source {
			HttpRequestBodySource::Legacy { rx, abort_rx } => loop {
				if let Some(error) = abort_rx.borrow().clone() {
					self.terminal = true;
					return Err(error);
				}

				tokio::select! {
					biased;
					changed = abort_rx.changed() => {
						if changed.is_ok() {
							continue;
						}
						break rx.recv().await;
					}
					chunk = rx.recv() => break chunk,
				}
			},
			HttpRequestBodySource::FlowControlled(queue) => match queue.recv().await {
				Ok(chunk) => chunk,
				Err(error) => {
					self.terminal = true;
					return Err(error);
				}
			},
		};
		self.handle_chunk(chunk)
	}
}

impl Drop for HttpRequestBodyStream {
	fn drop(&mut self) {
		if let HttpRequestBodySource::FlowControlled(queue) = &self.source {
			queue.close_receiver();
		}
		if !self.terminal
			&& let Some(event_tx) = &self.event_tx
		{
			let _ = event_tx.send(RequestBodyEvent::Cancelled);
		}
	}
}

#[derive(Debug)]
struct HttpBodySendWindowState {
	sent_bytes: u64,
	consumed_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct HttpBodySendWindow {
	state: Mutex<HttpBodySendWindowState>,
	credit_available: Notify,
}

impl HttpBodySendWindow {
	pub(crate) fn new() -> Arc<Self> {
		Arc::new(Self {
			state: Mutex::new(HttpBodySendWindowState {
				sent_bytes: 0,
				consumed_bytes: 0,
			}),
			credit_available: Notify::new(),
		})
	}

	pub(crate) async fn reserve(&self, bytes: u64) -> AnyhowResult<()> {
		ensure!(
			bytes <= protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES,
			"HTTP body frame exceeds the flow-control window"
		);
		if bytes == 0 {
			return Ok(());
		}

		loop {
			let notified = self.credit_available.notified();
			{
				let mut state = self.state.lock().await;
				let outstanding = state
					.sent_bytes
					.checked_sub(state.consumed_bytes)
					.context("HTTP body flow-control accounting underflow")?;
				let available = protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES
					.checked_sub(outstanding)
					.context("HTTP body flow-control window exceeded")?;
				if bytes <= available {
					state.sent_bytes = state
						.sent_bytes
						.checked_add(bytes)
						.context("HTTP body sent-byte counter overflow")?;
					return Ok(());
				}
			}
			notified.await;
		}
	}

	pub(crate) async fn update_consumed(&self, consumed_bytes: u64) -> AnyhowResult<()> {
		let mut state = self.state.lock().await;
		ensure!(
			consumed_bytes >= state.consumed_bytes,
			"HTTP body consumed-byte counter moved backwards"
		);
		ensure!(
			consumed_bytes <= state.sent_bytes,
			"HTTP body consumed-byte counter exceeds sent bytes"
		);
		if consumed_bytes == state.consumed_bytes {
			return Ok(());
		}
		state.consumed_bytes = consumed_bytes;
		drop(state);
		self.credit_available.notify_waiters();
		Ok(())
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

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;

	#[tokio::test]
	async fn request_body_returns_credit_only_when_consumed() {
		let queue = HttpRequestBodyQueue::new();
		let (event_tx, mut event_rx) = mpsc::unbounded_channel();
		let mut body = HttpRequestBodyStream::new_with_flow_control(queue.clone(), event_tx);
		assert!(queue.push(vec![1, 2, 3]));

		assert!(event_rx.try_recv().is_err());
		assert_eq!(body.recv().await.expect("read body"), Some(vec![1, 2, 3]));
		assert!(matches!(
			event_rx.recv().await,
			Some(RequestBodyEvent::Consumed(3))
		));
	}

	#[tokio::test]
	async fn dropping_unfinished_request_body_emits_upload_cancel() {
		let queue = HttpRequestBodyQueue::new();
		let (event_tx, mut event_rx) = mpsc::unbounded_channel();
		let body = HttpRequestBodyStream::new_with_flow_control(queue, event_tx);

		drop(body);

		assert!(matches!(
			event_rx.recv().await,
			Some(RequestBodyEvent::Cancelled)
		));
	}

	#[tokio::test]
	async fn request_body_coalesces_tiny_frames_within_the_byte_window() {
		let queue = HttpRequestBodyQueue::new();
		let (event_tx, mut event_rx) = mpsc::unbounded_channel();
		let mut body = HttpRequestBodyStream::new_with_flow_control(queue.clone(), event_tx);
		for byte in 0..=255u8 {
			assert!(queue.push(vec![byte]));
		}
		queue.finish();

		let chunk = body.recv().await.expect("read coalesced body").unwrap();
		assert_eq!(chunk, (0..=255u8).collect::<Vec<_>>());
		assert!(matches!(
			event_rx.recv().await,
			Some(RequestBodyEvent::Consumed(256))
		));
		assert_eq!(body.recv().await.expect("read request eof"), None);
	}

	#[tokio::test]
	async fn response_body_window_blocks_until_consumption_is_acknowledged() {
		let window = HttpBodySendWindow::new();
		window
			.reserve(protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES)
			.await
			.expect("reserve initial window");
		let blocked = tokio::spawn({
			let window = window.clone();
			async move { window.reserve(1).await }
		});
		assert!(
			tokio::time::timeout(Duration::from_millis(20), async {
				while !blocked.is_finished() {
					tokio::task::yield_now().await;
				}
			})
			.await
			.is_err()
		);

		window
			.update_consumed(1)
			.await
			.expect("return one byte of credit");
		blocked
			.await
			.expect("join blocked reservation")
			.expect("reserve after credit");
	}

	#[tokio::test]
	async fn response_body_window_rejects_regression_and_over_credit() {
		let window = HttpBodySendWindow::new();
		window.reserve(10).await.expect("reserve bytes");
		window.update_consumed(5).await.expect("consume bytes");
		assert!(window.update_consumed(4).await.is_err());
		assert!(window.update_consumed(11).await.is_err());
		window
			.update_consumed(5)
			.await
			.expect("duplicate cumulative ack");
	}
}
