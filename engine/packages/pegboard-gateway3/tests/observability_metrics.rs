use pegboard_gateway3::metrics;

#[test]
fn gateway_retry_metric_accepts_all_bucket_labels() {
	let buckets = ["1", "2", "3", "4+"];

	for bucket in buckets {
		metrics::REQUEST_RETRIES_TOTAL
			.with_label_values(&["namespace", "pool", "http", bucket])
			.inc();
	}
}

#[test]
fn gateway_in_flight_metrics_accept_all_labels() {
	let results = [
		"success",
		"client_disconnect",
		"actor_ready_timeout",
		"request_timeout",
		"envoy_error",
	];

	metrics::IN_FLIGHT
		.with_label_values(&["namespace", "pool", "http"])
		.set(1);
	metrics::IN_FLIGHT_DROPPED_TOTAL
		.with_label_values(&["namespace", "pool", "http", "client_disconnect"])
		.inc();

	for result in results {
		metrics::REQUEST_DURATION_SECONDS
			.with_label_values(&["namespace", "pool", "http", result])
			.observe(0.0);
	}
}

#[test]
fn gateway_http_streaming_metrics_accept_only_the_bounded_label_shapes() {
	for direction in ["request", "response"] {
		metrics::HTTP_BODY_WINDOW_EXHAUSTED_TOTAL
			.with_label_values(&[direction])
			.inc();
		metrics::HTTP_BODY_WINDOW_STALL_DURATION_SECONDS
			.with_label_values(&[direction])
			.observe(0.0);
		metrics::HTTP_BODY_BUFFERED_BYTES
			.with_label_values(&[direction])
			.set(0);
		metrics::HTTP_BODY_WINDOW_BYTES
			.with_label_values(&[direction])
			.set(0);
	}

	for (direction, initiator) in [("request", "gateway"), ("response", "actor")] {
		for kind in ["unknown", "cancelled", "handler_error", "internal_error"] {
			metrics::HTTP_STREAM_ABORT_TOTAL
				.with_label_values(&[direction, initiator, kind])
				.inc();
		}
	}

	for (direction, reason) in [
		("request", "invalid_credit"),
		("response", "message_index"),
		("response", "unexpected_message"),
		("response", "window_exceeded"),
	] {
		metrics::HTTP_PROTOCOL_VIOLATION_TOTAL
			.with_label_values(&[direction, reason])
			.inc();
	}

	for reason in [
		"ups_error",
		"handoff_timeout",
		"subscription_closed",
		"no_responder",
	] {
		metrics::HTTP_POST_COMMIT_FAILURE_TOTAL
			.with_label_values(&[reason])
			.inc();
	}
}
