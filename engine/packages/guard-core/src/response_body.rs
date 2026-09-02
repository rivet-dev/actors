use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming as BodyIncoming;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type ResponseBodyError = Box<dyn std::error::Error + Send + Sync>;

#[doc(hidden)]
pub struct CompletionGuard(Option<Box<dyn FnOnce() + Send + 'static>>);

impl CompletionGuard {
	fn complete(&mut self) {
		if let Some(callback) = self.0.take() {
			callback();
		}
	}
}

impl std::fmt::Debug for CompletionGuard {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("CompletionGuard").finish_non_exhaustive()
	}
}

impl Drop for CompletionGuard {
	fn drop(&mut self) {
		self.complete();
	}
}

pub struct ConsumptionCallback(std::sync::Arc<dyn Fn(usize) + Send + Sync + 'static>);

impl std::fmt::Debug for ConsumptionCallback {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ConsumptionCallback").finish_non_exhaustive()
	}
}

/// Out-of-band terminal error for a bounded response body channel. The drain task can record an
/// error even when every data slot is occupied; Hyper receives it after the queued data frames.
#[derive(Clone)]
pub struct ResponseBodyTerminal(Arc<Mutex<Option<ResponseBodyError>>>);

impl ResponseBodyTerminal {
	pub fn fail(&self, error: ResponseBodyError) {
		let mut terminal = self.0.lock().expect("response body terminal poisoned");
		if terminal.is_none() {
			*terminal = Some(error);
		}
	}
}

impl std::fmt::Debug for ResponseBodyTerminal {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ResponseBodyTerminal").finish_non_exhaustive()
	}
}

/// Response body type that can handle both streaming and buffered responses
#[derive(Debug)]
pub enum ResponseBody {
	/// Buffered response body
	Full(Full<Bytes>),
	/// Streaming response body
	Incoming(BodyIncoming),
	/// Channel-backed streaming response body
	Channel(mpsc::Receiver<Result<Bytes, ResponseBodyError>>),
	/// Bounded channel with an out-of-band terminal error slot.
	ChannelWithTerminal {
		body: mpsc::Receiver<Result<Bytes, ResponseBodyError>>,
		terminal: ResponseBodyTerminal,
	},
	/// Body carrying a callback that runs at EOF, error, or drop.
	WithCompletion {
		body: Box<ResponseBody>,
		completion: CompletionGuard,
	},
	/// Body carrying a callback invoked when a data frame is yielded downstream.
	WithConsumption {
		body: Box<ResponseBody>,
		callback: ConsumptionCallback,
	},
}

impl ResponseBody {
	pub fn channel_with_terminal(
		capacity: usize,
	) -> (
		mpsc::Sender<Result<Bytes, ResponseBodyError>>,
		Self,
		ResponseBodyTerminal,
	) {
		let (tx, rx) = mpsc::channel(capacity);
		let terminal = ResponseBodyTerminal(Arc::new(Mutex::new(None)));
		(
			tx,
			Self::ChannelWithTerminal {
				body: rx,
				terminal: terminal.clone(),
			},
			terminal,
		)
	}

	#[doc(hidden)]
	/// Runs `callback` exactly once when the body reaches EOF, errors, or is dropped.
	pub fn with_completion(self, callback: impl FnOnce() + Send + 'static) -> Self {
		Self::WithCompletion {
			body: Box::new(self),
			completion: CompletionGuard(Some(Box::new(callback))),
		}
	}

	#[doc(hidden)]
	pub fn with_consumption(
		self,
		callback: impl Fn(usize) + Send + Sync + 'static,
	) -> Self {
		Self::WithConsumption {
			body: Box::new(self),
			callback: ConsumptionCallback(std::sync::Arc::new(callback)),
		}
	}
}

impl http_body::Body for ResponseBody {
	type Data = Bytes;
	type Error = ResponseBodyError;

	fn poll_frame(
		self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
		match self.get_mut() {
			ResponseBody::Full(body) => {
				let pin = std::pin::Pin::new(body);
				match pin.poll_frame(cx) {
					std::task::Poll::Ready(Some(Ok(frame))) => {
						std::task::Poll::Ready(Some(Ok(frame)))
					}
					std::task::Poll::Ready(Some(Err(e))) => {
						std::task::Poll::Ready(Some(Err(Box::new(e))))
					}
					std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
					std::task::Poll::Pending => std::task::Poll::Pending,
				}
			}
			ResponseBody::Incoming(body) => {
				let pin = std::pin::Pin::new(body);
				match pin.poll_frame(cx) {
					std::task::Poll::Ready(Some(Ok(frame))) => {
						std::task::Poll::Ready(Some(Ok(frame)))
					}
					std::task::Poll::Ready(Some(Err(e))) => {
						std::task::Poll::Ready(Some(Err(Box::new(e))))
					}
					std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
					std::task::Poll::Pending => std::task::Poll::Pending,
				}
			}
			ResponseBody::Channel(rx) => match rx.poll_recv(cx) {
				std::task::Poll::Ready(Some(Ok(bytes))) => {
					std::task::Poll::Ready(Some(Ok(http_body::Frame::data(bytes))))
				}
				std::task::Poll::Ready(Some(Err(err))) => std::task::Poll::Ready(Some(Err(err))),
				std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
				std::task::Poll::Pending => std::task::Poll::Pending,
			},
			ResponseBody::ChannelWithTerminal { body, terminal } => match body.poll_recv(cx) {
				std::task::Poll::Ready(Some(Ok(bytes))) => {
					std::task::Poll::Ready(Some(Ok(http_body::Frame::data(bytes))))
				}
				std::task::Poll::Ready(Some(Err(err))) => std::task::Poll::Ready(Some(Err(err))),
				std::task::Poll::Ready(None) => {
					let error = terminal
						.0
						.lock()
						.expect("response body terminal poisoned")
						.take();
					match error {
						Some(error) => std::task::Poll::Ready(Some(Err(error))),
						None => std::task::Poll::Ready(None),
					}
				}
				std::task::Poll::Pending => std::task::Poll::Pending,
			},
			ResponseBody::WithCompletion { body, completion } => {
				let result = std::pin::Pin::new(body.as_mut()).poll_frame(cx);
				if matches!(
					&result,
					std::task::Poll::Ready(None) | std::task::Poll::Ready(Some(Err(_)))
				) {
					completion.complete();
				}
				result
			}
			ResponseBody::WithConsumption { body, callback } => {
				let result = std::pin::Pin::new(body.as_mut()).poll_frame(cx);
				if let std::task::Poll::Ready(Some(Ok(frame))) = &result
					&& let Some(data) = frame.data_ref()
				{
					(callback.0)(data.len());
				}
				result
			}
		}
	}

	fn is_end_stream(&self) -> bool {
		match self {
			ResponseBody::Full(body) => body.is_end_stream(),
			ResponseBody::Incoming(body) => body.is_end_stream(),
			ResponseBody::Channel(rx) => rx.is_closed() && rx.is_empty(),
			ResponseBody::ChannelWithTerminal { body, terminal } => {
				body.is_closed()
					&& body.is_empty()
					&& terminal
						.0
						.lock()
						.expect("response body terminal poisoned")
						.is_none()
			}
			ResponseBody::WithCompletion { body, .. } => body.is_end_stream(),
			ResponseBody::WithConsumption { body, .. } => body.is_end_stream(),
		}
	}

	fn size_hint(&self) -> http_body::SizeHint {
		match self {
			ResponseBody::Full(body) => body.size_hint(),
			ResponseBody::Incoming(body) => body.size_hint(),
			ResponseBody::Channel(_) => http_body::SizeHint::default(),
			ResponseBody::ChannelWithTerminal { .. } => http_body::SizeHint::default(),
			ResponseBody::WithCompletion { body, .. } => body.size_hint(),
			ResponseBody::WithConsumption { body, .. } => body.size_hint(),
		}
	}
}

#[cfg(test)]
#[path = "../tests/support/response_body.rs"]
mod tests;
