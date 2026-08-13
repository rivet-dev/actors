use rivet_metrics::{BUCKETS, MICRO_BUCKETS, REGISTRY, prometheus::*};

lazy_static::lazy_static! {
	pub static ref TUNNEL_PING_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"gateway2_tunnel_ping_duration",
		"RTT of messages from gateway to pegboard.",
		&["namespace_id", "pool_name", "protocol"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref LAST_PONG_AGE_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway2_last_pong_age_seconds",
		"Age of last received pong at every tunnel ping check; the tail tracks how close requests are to the tunnel_ping_timeout cliff.",
		&["namespace_id", "pool_name", "protocol"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref REQUEST_RETRIES_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway_request_retries_total",
		"Total gateway request-apply retries after no responders.",
		&["namespace_id", "pool_name", "protocol", "attempt_bucket"],
		*REGISTRY
	).unwrap();
	pub static ref IN_FLIGHT: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"gateway2_in_flight",
		"Count of currently active in-flight gateway requests.",
		&["namespace_id", "pool_name", "protocol"],
		*REGISTRY
	).unwrap();
	pub static ref IN_FLIGHT_DROPPED_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway2_in_flight_dropped_total",
		"Count of gateway tunnel messages dropped because the in-flight request is gone.",
		&["namespace_id", "pool_name", "protocol", "reason"],
		*REGISTRY
	).unwrap();
	pub static ref REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway2_request_duration_seconds",
		"Full gateway request lifecycle duration.",
		&["namespace_id", "pool_name", "protocol", "result"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_OPEN_WAIT_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"gateway2_websocket_open_wait_seconds",
		"Time spent waiting for ToRivetWebSocketOpen after sending ToEnvoyWebSocketOpen.",
		&["namespace_id", "pool_name", "result"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref CLOSE_SENT_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway2_close_sent_total",
		"ToEnvoyWebSocketClose messages emitted by gateway, by reason.",
		&["namespace_id", "pool_name", "protocol", "reason"],
		*REGISTRY
	).unwrap();
	pub static ref MSG_SENT_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"gateway2_msg_sent_total",
		"Count of total of tunnel messages sent.",
		&["namespace_id", "pool_name", "kind"],
		*REGISTRY
	).unwrap();
}
