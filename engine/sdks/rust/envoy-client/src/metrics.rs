//! Process-wide envoy-client metrics.
//!
//! These metrics live in the actor pod and are intended to make the
//! single-WebSocket transport between the actor and the engine debuggable
//! during incidents (e.g. pod-local handshake latency spikes where sent
//! requests get abandoned on disconnect).
//!
//! All labels here are bounded by code-defined enums. No `actor_id`,
//! `actor_key`, `runner_id`, `request_id`, or other unbounded values may be
//! added here.

use std::sync::LazyLock;

use rivet_metrics::prometheus::{
	Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, Opts, Registry,
};

const SQLITE_REQUEST_EXPIRED_LABELS: &[&str] = &["kind", "was_sent"];
const ENVOY_LOOP_ITER_LABELS: &[&str] = &["branch"];
const WS_TX_LOCK_LABELS: &[&str] = &["message_kind"];
const WS_TX_ADMISSION_LABELS: &[&str] = &["lane", "reason"];
const SQLITE_SEND_LABELS: &[&str] = &["kind"];
const WS_RECONNECT_LABELS: &[&str] = &["reason"];

pub struct EnvoyClientMetrics {
	pub sqlite_request_expired_total: IntCounterVec,
	pub sqlite_requests_inflight: IntGauge,
	pub remote_sqlite_requests_inflight: IntGauge,
	pub kv_requests_inflight: IntGauge,
	pub envoy_loop_iteration_duration_seconds: HistogramVec,
	pub ws_tx_lock_wait_duration_seconds: HistogramVec,
	pub ws_tx_lock_hold_duration_seconds: HistogramVec,
	pub ws_tx_admission_failures_total: IntCounterVec,
	pub sqlite_request_total_duration_seconds: HistogramVec,
	pub sqlite_request_submit_duration_seconds: HistogramVec,
	pub sqlite_request_wait_duration_seconds: HistogramVec,
	pub ws_reconnect_total: IntCounterVec,
	pub ws_session_duration_seconds: Histogram,
	pub envoy_tx_depth: IntGauge,
	pub ws_read_handler_duration: Histogram,
	pub ws_decode_duration: Histogram,
	pub ws_rx_bytes_total: IntCounter,
	pub ws_rx_messages_total: IntCounter,
}

impl EnvoyClientMetrics {
	fn new() -> Self {
		let sqlite_request_expired_total = IntCounterVec::new(
			Opts::new(
				"rivetkit_envoy_client_sqlite_request_expired_total",
				"total VFS sqlite requests expired by cleanup (smoking-gun signal: was_sent=true means a request was abandoned without resolution)",
			),
			SQLITE_REQUEST_EXPIRED_LABELS,
		)
		.expect("create envoy_client_sqlite_request_expired_total counter");

		let sqlite_requests_inflight = IntGauge::new(
			"rivetkit_envoy_client_sqlite_requests_inflight",
			"current in-flight VFS sqlite requests tracked in envoy ctx",
		)
		.expect("create envoy_client_sqlite_requests_inflight gauge");

		let remote_sqlite_requests_inflight = IntGauge::new(
			"rivetkit_envoy_client_remote_sqlite_requests_inflight",
			"current in-flight remote sqlite (exec/execute) requests tracked in envoy ctx",
		)
		.expect("create envoy_client_remote_sqlite_requests_inflight gauge");

		let kv_requests_inflight = IntGauge::new(
			"rivetkit_envoy_client_kv_requests_inflight",
			"current in-flight KV requests tracked in envoy ctx",
		)
		.expect("create envoy_client_kv_requests_inflight gauge");

		let envoy_loop_iteration_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_envoy_client_envoy_loop_iteration_duration_seconds",
				"duration of one envoy_loop select branch in seconds; long tails indicate the single fan-in loop stalled",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
			ENVOY_LOOP_ITER_LABELS,
		)
		.expect("create envoy_client_envoy_loop_iteration_duration_seconds histogram");

		let ws_tx_lock_wait_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_envoy_client_ws_tx_lock_wait_duration_seconds",
				"time spent waiting to acquire the ws_tx mutex in seconds",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
			WS_TX_LOCK_LABELS,
		)
		.expect("create envoy_client_ws_tx_lock_wait_duration_seconds histogram");

		let ws_tx_lock_hold_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_envoy_client_ws_tx_lock_hold_duration_seconds",
				"time the ws_tx mutex guard was held in seconds (covers encode + send)",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
			WS_TX_LOCK_LABELS,
		)
		.expect("create envoy_client_ws_tx_lock_hold_duration_seconds histogram");

		let ws_tx_admission_failures_total = IntCounterVec::new(
			Opts::new(
				"rivetkit_envoy_client_ws_tx_admission_failures_total",
				"messages rejected before entering the bounded websocket writer queues",
			),
			WS_TX_ADMISSION_LABELS,
		)
		.expect("create envoy_client_ws_tx_admission_failures_total counter");

		let sqlite_request_total_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_envoy_client_sqlite_request_total_duration_seconds",
				"end-to-end duration from send_sqlite_request call to oneshot resolution in seconds",
			)
			.buckets(rivet_metrics::BUCKETS.to_vec()),
			SQLITE_SEND_LABELS,
		)
		.expect("create envoy_client_sqlite_request_total_duration_seconds histogram");

		let sqlite_request_submit_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_envoy_client_sqlite_request_submit_duration_seconds",
				"time from send_sqlite_request entry to envoy_tx.send return in seconds",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
			SQLITE_SEND_LABELS,
		)
		.expect("create envoy_client_sqlite_request_submit_duration_seconds histogram");

		let sqlite_request_wait_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_envoy_client_sqlite_request_wait_duration_seconds",
				"time from envoy_tx.send return to oneshot resolution in seconds",
			)
			.buckets(rivet_metrics::BUCKETS.to_vec()),
			SQLITE_SEND_LABELS,
		)
		.expect("create envoy_client_sqlite_request_wait_duration_seconds histogram");

		let ws_reconnect_total = IntCounterVec::new(
			Opts::new(
				"rivetkit_envoy_client_ws_reconnect_total",
				"total websocket reconnect events by reason",
			),
			WS_RECONNECT_LABELS,
		)
		.expect("create envoy_client_ws_reconnect_total counter");

		let ws_session_duration_seconds = Histogram::with_opts(
			HistogramOpts::new(
				"rivetkit_envoy_client_ws_session_duration_seconds",
				"duration of a single websocket session from connect to disconnect in seconds",
			)
			.buckets(rivet_metrics::BUCKETS.to_vec()),
		)
		.expect("create envoy_client_ws_session_duration_seconds histogram");

		let envoy_tx_depth = IntGauge::new(
			"rivetkit_envoy_client_envoy_tx_depth",
			"current depth of the unbounded envoy_tx mpsc between WS read task and envoy_loop",
		)
		.expect("create envoy_client_envoy_tx_depth gauge");

		let ws_read_handler_duration = Histogram::with_opts(
			HistogramOpts::new(
				"rivetkit_envoy_client_ws_read_handler_duration",
				"time the websocket read loop spends decoding and dispatching one frame, during which the socket is not being drained",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
		)
		.expect("create envoy_client_ws_read_handler_duration histogram");

		let ws_decode_duration = Histogram::with_opts(
			HistogramOpts::new(
				"rivetkit_envoy_client_ws_decode_duration",
				"time to deserialize one inbound websocket frame",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
		)
		.expect("create envoy_client_ws_decode_duration histogram");

		let ws_rx_bytes_total = IntCounter::new(
			"rivetkit_envoy_client_ws_rx_bytes_total",
			"total bytes read from the engine websocket; the rate of this is how fast this pod consumes websocket data",
		)
		.expect("create envoy_client_ws_rx_bytes_total counter");

		let ws_rx_messages_total = IntCounter::new(
			"rivetkit_envoy_client_ws_rx_messages_total",
			"total inbound websocket frames read from the engine",
		)
		.expect("create envoy_client_ws_rx_messages_total counter");

		register(
			&rivet_metrics::REGISTRY,
			sqlite_request_expired_total.clone(),
		);
		register(&rivet_metrics::REGISTRY, sqlite_requests_inflight.clone());
		register(
			&rivet_metrics::REGISTRY,
			remote_sqlite_requests_inflight.clone(),
		);
		register(&rivet_metrics::REGISTRY, kv_requests_inflight.clone());
		register(
			&rivet_metrics::REGISTRY,
			envoy_loop_iteration_duration_seconds.clone(),
		);
		register(
			&rivet_metrics::REGISTRY,
			ws_tx_lock_wait_duration_seconds.clone(),
		);
		register(
			&rivet_metrics::REGISTRY,
			ws_tx_lock_hold_duration_seconds.clone(),
		);
		register(
			&rivet_metrics::REGISTRY,
			ws_tx_admission_failures_total.clone(),
		);
		register(
			&rivet_metrics::REGISTRY,
			sqlite_request_total_duration_seconds.clone(),
		);
		register(
			&rivet_metrics::REGISTRY,
			sqlite_request_submit_duration_seconds.clone(),
		);
		register(
			&rivet_metrics::REGISTRY,
			sqlite_request_wait_duration_seconds.clone(),
		);
		register(&rivet_metrics::REGISTRY, ws_reconnect_total.clone());
		register(
			&rivet_metrics::REGISTRY,
			ws_session_duration_seconds.clone(),
		);
		register(&rivet_metrics::REGISTRY, envoy_tx_depth.clone());
		register(&rivet_metrics::REGISTRY, ws_read_handler_duration.clone());
		register(&rivet_metrics::REGISTRY, ws_decode_duration.clone());
		register(&rivet_metrics::REGISTRY, ws_rx_bytes_total.clone());
		register(&rivet_metrics::REGISTRY, ws_rx_messages_total.clone());

		Self {
			sqlite_request_expired_total,
			sqlite_requests_inflight,
			remote_sqlite_requests_inflight,
			kv_requests_inflight,
			envoy_loop_iteration_duration_seconds,
			ws_tx_lock_wait_duration_seconds,
			ws_tx_lock_hold_duration_seconds,
			ws_tx_admission_failures_total,
			sqlite_request_total_duration_seconds,
			sqlite_request_submit_duration_seconds,
			sqlite_request_wait_duration_seconds,
			ws_reconnect_total,
			ws_session_duration_seconds,
			envoy_tx_depth,
			ws_read_handler_duration,
			ws_decode_duration,
			ws_rx_bytes_total,
			ws_rx_messages_total,
		}
	}
}

pub static METRICS: LazyLock<EnvoyClientMetrics> = LazyLock::new(EnvoyClientMetrics::new);

fn register<M>(registry: &Registry, metric: M)
where
	M: rivet_metrics::prometheus::core::Collector + Clone + Send + Sync + 'static,
{
	if let Err(error) = registry.register(Box::new(metric)) {
		tracing::warn!(
			?error,
			"envoy-client metric registration failed, using existing collector"
		);
	}
}
