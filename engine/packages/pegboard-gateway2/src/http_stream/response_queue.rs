use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

use rivet_envoy_protocol::ToRivetTunnelMessageKind;

/// Bounds response data waiting between pubsub delivery and the downstream HTTP client.
///
/// Permits are attached to queued messages and release their capacity on delivery or drop. The
/// byte limit applies to concurrently buffered data, not the total size of a streaming response.
#[derive(Debug)]
pub(crate) struct HttpResponseQueueBudget {
	messages: AtomicUsize,
	bytes: AtomicUsize,
	buffered_response_max_bytes: usize,
	max_messages: usize,
	streaming_response_max_bytes: usize,
}

impl HttpResponseQueueBudget {
	pub(crate) fn new(
		buffered_response_max_bytes: usize,
		max_messages: usize,
		streaming_response_max_bytes: usize,
	) -> Self {
		Self {
			messages: AtomicUsize::new(0),
			bytes: AtomicUsize::new(0),
			buffered_response_max_bytes,
			max_messages,
			streaming_response_max_bytes,
		}
	}

	fn try_reserve(
		self: &Arc<Self>,
		bytes: usize,
		streaming: bool,
	) -> Option<HttpResponseQueuePermit> {
		let max_bytes = if streaming {
			self.streaming_response_max_bytes
		} else {
			self.buffered_response_max_bytes
		};
		if bytes > max_bytes {
			return None;
		}

		let previous_messages = self.messages.fetch_add(1, Ordering::AcqRel);
		if previous_messages >= self.max_messages {
			self.messages.fetch_sub(1, Ordering::AcqRel);
			return None;
		}

		let previous_bytes = self.bytes.fetch_add(bytes, Ordering::AcqRel);
		if previous_bytes.saturating_add(bytes) > max_bytes {
			self.bytes.fetch_sub(bytes, Ordering::AcqRel);
			self.messages.fetch_sub(1, Ordering::AcqRel);
			return None;
		}

		Some(HttpResponseQueuePermit {
			budget: self.clone(),
			bytes,
		})
	}

	pub(crate) fn try_reserve_message(
		self: &Arc<Self>,
		message: &ToRivetTunnelMessageKind,
	) -> Result<HttpResponseQueuePermit, HttpResponseQueueOverloaded> {
		let (bytes, streaming) = match message {
			ToRivetTunnelMessageKind::ToRivetResponseStart(response) => {
				(response.body.as_ref().map_or(0, Vec::len), response.stream)
			}
			ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => (chunk.body.len(), true),
			_ => (0, true),
		};

		self.try_reserve(bytes, streaming)
			.ok_or(HttpResponseQueueOverloaded { bytes })
	}
}

pub(crate) struct HttpResponseQueueOverloaded {
	pub(crate) bytes: usize,
}

#[derive(Debug)]
pub(crate) struct HttpResponseQueuePermit {
	budget: Arc<HttpResponseQueueBudget>,
	bytes: usize,
}

impl Drop for HttpResponseQueuePermit {
	fn drop(&mut self) {
		self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
		self.budget.messages.fetch_sub(1, Ordering::AcqRel);
	}
}

#[cfg(test)]
#[path = "../../tests/support/http_response_queue.rs"]
mod tests;
