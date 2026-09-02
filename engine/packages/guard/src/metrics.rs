use lazy_static::lazy_static;
use rivet_metrics::{BUCKETS, REGISTRY, prometheus::*};

lazy_static! {
	pub static ref ROUTE_TOTAL: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"guard_route_total",
		"Total number of routing results handled.",
		&["router"],
		*REGISTRY
	)
	.unwrap();
	pub static ref PEGBOARD_GATEWAY_ROUTE_TOTAL: IntCounterVec =
		register_int_counter_vec_with_registry!(
			"pegboard_gateway_route_total",
			"Pegboard gateway selections by bounded routing decision.",
			&["gateway", "envoy_protocol", "request_kind", "mode", "decision"],
			*REGISTRY
		)
		.unwrap();
	pub static ref ROUTE_DISPATCH_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_route_dispatch_duration",
		"Time spent dispatching to a guard routing module in seconds.",
		&["namespace_id"],
		BUCKETS.to_vec(),
		*REGISTRY
	)
	.unwrap();
	pub static ref ROUTE_API_PUBLIC_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_route_api_public_duration",
		"Time spent resolving the api-public route in seconds.",
		&["namespace_id"],
		BUCKETS.to_vec(),
		*REGISTRY
	)
	.unwrap();
	pub static ref ROUTE_COMPUTE_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_route_compute_duration",
		"Time spent resolving a compute route in seconds.",
		&["namespace_id"],
		BUCKETS.to_vec(),
		*REGISTRY
	)
	.unwrap();
	pub static ref ROUTE_AUTH_CHECK_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_route_auth_check_duration",
		"Time spent checking guard route authorization in seconds.",
		&["namespace_id"],
		BUCKETS.to_vec(),
		*REGISTRY
	)
	.unwrap();
	pub static ref ROUTE_PEGBOARD_SUBSCRIBE_DURATION: HistogramVec =
		register_histogram_vec_with_registry!(
			"guard_route_pegboard_subscribe_duration",
			"Time spent subscribing to pegboard actor routing events in seconds.",
			&["namespace_id"],
			BUCKETS.to_vec(),
			*REGISTRY
		)
		.unwrap();
	pub static ref ROUTE_PEGBOARD_FETCH_ACTOR_DURATION: HistogramVec =
		register_histogram_vec_with_registry!(
			"guard_route_pegboard_fetch_actor_duration",
			"Time spent fetching pegboard actor routing state in seconds.",
			&["namespace_id"],
			BUCKETS.to_vec(),
			*REGISTRY
		)
		.unwrap();
	pub static ref ROUTE_PEGBOARD_AUTH_CHECK_DURATION: HistogramVec =
		register_histogram_vec_with_registry!(
			"guard_route_pegboard_auth_check_duration",
			"Time spent checking pegboard actor route authorization in seconds.",
			&["namespace_id"],
			BUCKETS.to_vec(),
			*REGISTRY
		)
		.unwrap();
	pub static ref ROUTE_PEGBOARD_WAKE_SIGNAL_DURATION: HistogramVec =
		register_histogram_vec_with_registry!(
			"guard_route_pegboard_wake_signal_duration",
			"Time spent sending pegboard actor wake signals in seconds.",
			&["namespace_id"],
			BUCKETS.to_vec(),
			*REGISTRY
		)
		.unwrap();
	pub static ref ROUTE_PEGBOARD_RESOLVE_QUERY_DURATION: HistogramVec =
		register_histogram_vec_with_registry!(
			"guard_route_pegboard_resolve_query_duration",
			"Time spent resolving pegboard actor query routes in seconds.",
			&["namespace_id"],
			BUCKETS.to_vec(),
			*REGISTRY
		)
		.unwrap();
	pub static ref ROUTE_PEGBOARD_READY_WAIT_DURATION: HistogramVec =
		register_histogram_vec_with_registry!(
			"guard_route_pegboard_ready_wait_duration",
			"Time the gateway spent waiting for the actor Ready signal after dispatching to pegboard_actor2.",
			&["namespace_id", "pool_name", "was_sleeping", "wake_retries_bucket", "outcome"],
			BUCKETS.to_vec(),
			*REGISTRY
		)
		.unwrap();
}
