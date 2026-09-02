use pegboard_gateway3::metrics;

#[test]
fn gateway_retry_metric_accepts_all_bucket_labels() {
	let buckets = ["1", "2", "3", "4+"];

	for bucket in buckets {
		metrics::REQUEST_RETRIES_TOTAL
			.with_label_values(&[bucket])
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

	metrics::IN_FLIGHT.with_label_values(&["namespace"]).set(1);
	metrics::IN_FLIGHT_DROPPED_TOTAL
		.with_label_values(&["namespace", "client_disconnect"])
		.inc();

	for result in results {
		metrics::REQUEST_DURATION_SECONDS
			.with_label_values(&["namespace", result])
			.observe(0.0);
	}
}
