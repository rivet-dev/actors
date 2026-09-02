use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Instant,
};

use lazy_static::lazy_static;
use rivet_metrics::{BUCKETS, REGISTRY, prometheus::*};

/// Keeps the protocol dimension bounded across mixed-version Envoy rollouts.
pub fn pegboard_envoy_protocol_label(version: Option<u16>) -> &'static str {
	match version {
		Some(6) => "6",
		Some(7) => "7",
		_ => "unknown",
	}
}

#[derive(Clone, Copy)]
pub enum PegboardGatewayVersion {
	V2,
	V3,
}

impl PegboardGatewayVersion {
	fn label(self) -> &'static str {
		match self {
			Self::V2 => "2",
			Self::V3 => "3",
		}
	}
}

#[derive(Clone, Copy)]
pub enum PegboardGatewayRequestKind {
	Http,
	WebSocket,
}

impl PegboardGatewayRequestKind {
	fn label(self) -> &'static str {
		match self {
			Self::Http => "http",
			Self::WebSocket => "websocket",
		}
	}
}

#[derive(Clone, Copy)]
pub enum PegboardGatewayResult {
	Success,
	ClientDisconnect,
	ActorReadyTimeout,
	RequestTimeout,
	EnvoyError,
	PreDispatchFailure,
}

impl PegboardGatewayResult {
	fn label(self) -> &'static str {
		match self {
			Self::Success => "success",
			Self::ClientDisconnect => "client_disconnect",
			Self::ActorReadyTimeout => "actor_ready_timeout",
			Self::RequestTimeout => "request_timeout",
			Self::EnvoyError => "envoy_error",
			Self::PreDispatchFailure => "pre_dispatch_failure",
		}
	}
}

struct PegboardGatewayLifecycleInner {
	gateway: PegboardGatewayVersion,
	envoy_protocol: &'static str,
	request_kind: PegboardGatewayRequestKind,
	started_at: Instant,
	finished: AtomicBool,
}

impl PegboardGatewayLifecycleInner {
	fn finish(&self, result: PegboardGatewayResult) {
		if self.finished.swap(true, Ordering::AcqRel) {
			return;
		}
		let labels = [
			self.gateway.label(),
			self.envoy_protocol,
			self.request_kind.label(),
			result.label(),
		];
		PEGBOARD_GATEWAY_REQUEST_TOTAL
			.with_label_values(&labels)
			.inc();
		PEGBOARD_GATEWAY_REQUEST_DURATION_SECONDS
			.with_label_values(&labels)
			.observe(self.started_at.elapsed().as_secs_f64());
	}
}

impl Drop for PegboardGatewayLifecycleInner {
	fn drop(&mut self) {
		self.finish(PegboardGatewayResult::PreDispatchFailure);
	}
}

#[derive(Clone)]
pub struct PegboardGatewayLifecycle(Arc<PegboardGatewayLifecycleInner>);

impl PegboardGatewayLifecycle {
	pub fn new(
		gateway: PegboardGatewayVersion,
		envoy_protocol: Option<u16>,
		request_kind: PegboardGatewayRequestKind,
	) -> Self {
		Self(Arc::new(PegboardGatewayLifecycleInner {
			gateway,
			envoy_protocol: pegboard_envoy_protocol_label(envoy_protocol),
			request_kind,
			started_at: Instant::now(),
			finished: AtomicBool::new(false),
		}))
	}

	pub fn finish(&self, result: PegboardGatewayResult) {
		self.0.finish(result);
	}
}

lazy_static! {
	// MARK: Internal
	pub static ref ROUTE_CACHE_COUNT: IntGauge = register_int_gauge_with_registry!(
		"guard_route_cache_count",
		"Number of entries in the route cache",
		*REGISTRY
	).unwrap();
	pub static ref RATE_LIMITER_COUNT: IntGauge = register_int_gauge_with_registry!(
		"guard_rate_limiter_count",
		"Number of active rate limiters",
		*REGISTRY
	).unwrap();
	pub static ref IN_FLIGHT_COUNTER_COUNT: IntGauge = register_int_gauge_with_registry!(
		"guard_in_flight_counter_count",
		"Number of active in-flight counters",
		*REGISTRY
	).unwrap();
	pub static ref IN_FLIGHT_REQUEST_COUNT: IntGauge = register_int_gauge_with_registry!(
		"guard_in_flight_request_count",
		"Number of active in-flight requests",
		*REGISTRY
	).unwrap();
	pub static ref PEGBOARD_GATEWAY_REQUEST_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_gateway_request_total",
		"Completed Pegboard gateway requests by router, Envoy protocol, kind, and result.",
		&["gateway", "envoy_protocol", "request_kind", "result"],
		*REGISTRY
	).unwrap();
	pub static ref PEGBOARD_GATEWAY_REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_gateway_request_duration_seconds",
		"Full Pegboard gateway request lifecycle duration by router, Envoy protocol, kind, and result.",
		&["gateway", "envoy_protocol", "request_kind", "result"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	// MARK: TCP
	pub static ref TCP_CONNECTION_TOTAL: IntCounter = register_int_counter_with_registry!(
		"guard_tcp_connection_total",
		"Total number of TCP connections ever",
		*REGISTRY
	).unwrap();
	pub static ref TCP_CONNECTION_PENDING: IntGauge = register_int_gauge_with_registry!(
		"guard_tcp_connection_pending",
		"Total number of open TCP connections",
		*REGISTRY
	).unwrap();
	pub static ref TCP_CONNECTION_DURATION: Histogram = register_histogram_with_registry!(
		"guard_tcp_connection_duration",
		"TCP connection duration in seconds",
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	// MARK: Pre-proxy
	pub static ref RESOLVE_ROUTE_DURATION: Histogram = register_histogram_with_registry!(
		"guard_resolve_route_duration",
		"Time to resolve request route in seconds",
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	// MARK: Proxy requests
	pub static ref PROXY_REQUEST_TOTAL: IntCounter = register_int_counter_with_registry!(
		"guard_proxy_request_total",
		"Total number of requests to actor",
		*REGISTRY
	).unwrap();
	pub static ref PROXY_REQUEST_PENDING: IntGauge = register_int_gauge_with_registry!(
		"guard_proxy_request_pending",
		"Number of pending requests to actor",
		*REGISTRY
	).unwrap();
	pub static ref PROXY_REQUEST_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_proxy_request_duration",
		"Request duration in seconds",
		&["status"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref PROXY_REQUEST_ERROR_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"guard_proxy_request_errors_total",
		"Total number of errors when proxying requests to actor",
		&["error"],
		*REGISTRY
	).unwrap();

	// MARK: WebSockets
	pub static ref WEBSOCKET_SEND_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_websocket_send_duration",
		"Time to send a WebSocket message through a shared WebSocketHandle in seconds.",
		&["message_kind"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_SEND_LOCK_WAIT_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_core_websocket_send_lock_wait_duration_seconds",
		"Time spent awaiting the per-connection ws_tx mutex inside WebSocketHandle::send. High tails indicate contention from other senders on the same connection.",
		&["message_kind"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_SEND_WRITE_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"guard_core_websocket_send_write_duration_seconds",
		"Time spent inside the network write (lock held) of WebSocketHandle::send.",
		&["message_kind"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_WRITE_WOULD_BLOCK_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"guard_websocket_write_would_block_total",
		"Total number of WebSocket write or flush attempts that hit WouldBlock.",
		&["message_kind"],
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_WRITE_BUFFER_FULL_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"guard_websocket_write_buffer_full_total",
		"Total number of WebSocket messages rejected because the tungstenite write buffer was full.",
		&["message_kind"],
		*REGISTRY
	).unwrap();
	pub static ref WEBSOCKET_WRITE_BACKPRESSURE_EVENTS_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"guard_websocket_write_backpressure_events_total",
		"Total number of transitions from write-ready to write-backpressured.",
		&["message_kind"],
		*REGISTRY
	).unwrap();
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn pegboard_gateway_lifecycle_records_exactly_one_terminal_result() {
		let success_labels = ["3", "7", "http", "success"];
		let fallback_labels = ["2", "6", "websocket", "pre_dispatch_failure"];
		let success_before = PEGBOARD_GATEWAY_REQUEST_TOTAL
			.with_label_values(&success_labels)
			.get();
		let fallback_before = PEGBOARD_GATEWAY_REQUEST_TOTAL
			.with_label_values(&fallback_labels)
			.get();

		let lifecycle = PegboardGatewayLifecycle::new(
			PegboardGatewayVersion::V3,
			Some(7),
			PegboardGatewayRequestKind::Http,
		);
		lifecycle.finish(PegboardGatewayResult::Success);
		lifecycle.finish(PegboardGatewayResult::EnvoyError);
		drop(lifecycle);

		let unfinished = PegboardGatewayLifecycle::new(
			PegboardGatewayVersion::V2,
			Some(6),
			PegboardGatewayRequestKind::WebSocket,
		);
		drop(unfinished);

		assert_eq!(
			PEGBOARD_GATEWAY_REQUEST_TOTAL
				.with_label_values(&success_labels)
				.get(),
			success_before + 1,
		);
		assert_eq!(
			PEGBOARD_GATEWAY_REQUEST_TOTAL
				.with_label_values(&fallback_labels)
				.get(),
			fallback_before + 1,
		);
	}

	#[test]
	fn pegboard_envoy_protocol_label_is_bounded() {
		assert_eq!(pegboard_envoy_protocol_label(Some(6)), "6");
		assert_eq!(pegboard_envoy_protocol_label(Some(7)), "7");
		assert_eq!(pegboard_envoy_protocol_label(None), "unknown");
		assert_eq!(pegboard_envoy_protocol_label(Some(8)), "unknown");
	}
}
