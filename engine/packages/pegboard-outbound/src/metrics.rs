use rivet_metrics::{BUCKETS, MICRO_BUCKETS, REGISTRY, prometheus::*};

lazy_static::lazy_static! {
	pub static ref REQ_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_outbound_req_total",
		"Count of serverless outbound requests made.",
		&["namespace_id", "runner_name"],
		*REGISTRY
	).unwrap();
	pub static ref REQ_BY_GENERATION_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_outbound_req_by_generation_total",
		"Count of serverless outbound requests by actor generation bucket.",
		&["namespace_id", "runner_name", "generation_bucket"],
		*REGISTRY
	).unwrap();
	pub static ref REQ_ERROR_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_outbound_req_error_total",
		"Count of serverless outbound request failures by reason.",
		&["namespace_id", "pool_name", "error", "status"],
		*REGISTRY
	).unwrap();
	pub static ref REQ_DURATION_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_outbound_req_duration_seconds",
		"Full serverless outbound request lifetime by terminal result.",
		&["namespace_id", "pool_name", "result", "drain_reason"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref REQ_ACTIVE: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"pegboard_outbound_req_active",
		"Count of serverless outbound requests currently active.",
		&["namespace_id", "runner_name"],
		*REGISTRY
	).unwrap();

	pub static ref REQ_SSE_OPEN_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_outbound_req_sse_open_duration",
		"Time from starting a serverless outbound request to the SSE stream opening in seconds.",
		&["namespace_id", "runner_name"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
}
