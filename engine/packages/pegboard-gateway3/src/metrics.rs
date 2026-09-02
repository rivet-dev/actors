use rivet_metrics::{BUCKETS, MICRO_BUCKETS, REGISTRY, prometheus::*};

pub(crate) struct HttpResponseFlowMetrics {
	buffered_bytes: std::sync::atomic::AtomicU64,
}

impl HttpResponseFlowMetrics {
	pub(crate) fn new() -> std::sync::Arc<Self> {
		HTTP_BODY_WINDOW_BYTES
			.with_label_values(&["response"])
			.add(rivet_envoy_protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as i64);
		std::sync::Arc::new(Self {
			buffered_bytes: std::sync::atomic::AtomicU64::new(0),
		})
	}

	pub(crate) fn enqueue(&self, bytes: usize) {
		self.buffered_bytes
			.fetch_add(bytes as u64, std::sync::atomic::Ordering::AcqRel);
		HTTP_BODY_BUFFERED_BYTES
			.with_label_values(&["response"])
			.add(bytes as i64);
	}

	pub(crate) fn consume(&self, bytes: usize) {
		let result = self.buffered_bytes.fetch_update(
			std::sync::atomic::Ordering::AcqRel,
			std::sync::atomic::Ordering::Acquire,
			|buffered| buffered.checked_sub(bytes as u64),
		);
		if result.is_ok() {
			HTTP_BODY_BUFFERED_BYTES
				.with_label_values(&["response"])
				.sub(bytes as i64);
		}
	}
}

impl Drop for HttpResponseFlowMetrics {
	fn drop(&mut self) {
		let buffered = self
			.buffered_bytes
			.swap(0, std::sync::atomic::Ordering::AcqRel);
		HTTP_BODY_BUFFERED_BYTES
			.with_label_values(&["response"])
			.sub(buffered as i64);
		HTTP_BODY_WINDOW_BYTES
			.with_label_values(&["response"])
			.sub(rivet_envoy_protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as i64);
	}
}

lazy_static::lazy_static! {
	pub static ref TUNNEL_PING_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"gateway3_tunnel_ping_duration",
		"RTT of messages from gateway to pegboard.",
		&["namespace_id", "pool_name", "protocol"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref LAST_PONG_AGE_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway3_last_pong_age_seconds",
		"Age of last received pong at every tunnel ping check; the tail tracks how close requests are to the tunnel_ping_timeout cliff.",
		&["namespace_id", "pool_name", "protocol"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref REQUEST_RETRIES_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_request_retries_total",
		"Total gateway request-apply retries after no responders.",
		&["namespace_id", "pool_name", "protocol", "attempt_bucket"],
		*REGISTRY
	).unwrap();
	pub static ref IN_FLIGHT: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"gateway3_in_flight",
		"Count of currently active in-flight gateway requests.",
		&["namespace_id", "pool_name", "protocol"],
		*REGISTRY
	).unwrap();
	pub static ref IN_FLIGHT_DROPPED_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_in_flight_dropped_total",
		"Count of gateway tunnel messages dropped because the in-flight request is gone.",
		&["namespace_id", "pool_name", "protocol", "reason"],
		*REGISTRY
	).unwrap();
	pub static ref REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway3_request_duration_seconds",
		"Full gateway request lifecycle duration.",
		&["namespace_id", "pool_name", "protocol", "result"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_OPEN_WAIT_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway3_websocket_open_wait_seconds",
		"Time spent waiting for ToRivetWebSocketOpen after sending ToEnvoyWebSocketOpen.",
		&["namespace_id", "pool_name", "result"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref CLOSE_SENT_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_close_sent_total",
		"ToEnvoyWebSocketClose messages emitted by gateway, by reason.",
		&["namespace_id", "pool_name", "protocol", "reason"],
		*REGISTRY
	).unwrap();
	pub static ref MSG_SENT_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_msg_sent_total",
		"Count of total of tunnel messages sent.",
		&["namespace_id", "pool_name", "kind"],
		*REGISTRY
	).unwrap();
	pub static ref HTTP_BODY_WINDOW_EXHAUSTED_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_http_body_window_exhausted_total",
		"Transitions to exhausted HTTP streaming credit.",
		&["direction"],
		*REGISTRY
	).unwrap();
	pub static ref HTTP_BODY_WINDOW_STALL_DURATION_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway3_http_body_window_stall_duration_seconds",
		"Continuous time blocked on HTTP streaming credit.",
		&["direction"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref HTTP_BODY_BUFFERED_BYTES: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"gateway3_http_body_buffered_bytes",
		"Aggregate bytes buffered by active HTTP streams.",
		&["direction"],
		*REGISTRY
	).unwrap();
	pub static ref HTTP_BODY_WINDOW_BYTES: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"gateway3_http_body_window_bytes",
		"Aggregate configured HTTP streaming window bytes.",
		&["direction"],
		*REGISTRY
	).unwrap();
	pub static ref HTTP_STREAM_ABORT_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_http_stream_abort_total",
		"HTTP stream aborts by bounded direction, initiator, and kind.",
		&["direction", "initiator", "kind"],
		*REGISTRY
	).unwrap();
	pub static ref HTTP_PROTOCOL_VIOLATION_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_http_protocol_violation_total",
		"HTTP streaming protocol violations by bounded direction and reason.",
		&["direction", "reason"],
		*REGISTRY
	).unwrap();
	pub static ref HTTP_POST_COMMIT_FAILURE_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway3_http_post_commit_failure_total",
		"Indeterminate HTTP failures after request-start commitment.",
		&["reason"],
		*REGISTRY
	).unwrap();
}
