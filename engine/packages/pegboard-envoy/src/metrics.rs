use rivet_metrics::{
	BUCKETS, LIFETIME_BUCKETS, MICRO_BUCKETS, PAGE_COUNT_BUCKETS, REGISTRY, prometheus::*,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EnvoyState {
	Starting,
	Connected,
	Stopping,
	Disconnected,
	Lost,
	Stopped,
}

impl EnvoyState {
	pub const ALL: [Self; 6] = [
		Self::Starting,
		Self::Connected,
		Self::Stopping,
		Self::Disconnected,
		Self::Lost,
		Self::Stopped,
	];

	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Starting => "starting",
			Self::Connected => "connected",
			Self::Stopping => "stopping",
			Self::Disconnected => "disconnected",
			Self::Lost => "lost",
			Self::Stopped => "stopped",
		}
	}
}

lazy_static::lazy_static! {
	pub static ref CONNECTION_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"envoy_connection_total",
		"Count of envoy connections opened.",
		&["namespace_id", "pool_name", "protocol_version"],
		*REGISTRY
	).unwrap();

	pub static ref EVICTION_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"envoy_eviction_total",
		"Count of envoy connections evicted.",
		&["namespace_id", "pool_name", "protocol_version"],
		*REGISTRY
	).unwrap();

	pub static ref CONNECTION_ACTIVE: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"envoy_connection_active",
		"Count of envoy connections currently active.",
		&["namespace_id", "pool_name", "protocol_version"],
		*REGISTRY
	).unwrap();
	pub static ref ENVOY_CONNECTIONS_BY_STATE: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"pegboard_envoy_connections_by_state",
		"Current envoy WebSocket connections by lifecycle state. Each connection should contribute to exactly one state.",
		&["namespace_id", "pool_name", "protocol_version", "envoy_state"],
		*REGISTRY
	).unwrap();
	pub static ref ENVOY_STATE_TRANSITION_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_envoy_state_transition_total",
		"Count of envoy WebSocket lifecycle state transitions.",
		&["namespace_id", "pool_name", "protocol_version", "envoy_state", "reason"],
		*REGISTRY
	).unwrap();
	pub static ref ENVOY_CONNECTED: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"pegboard_envoy_connected",
		"Count of currently connected envoy WebSocket connections.",
		&["namespace_id", "pool_name"],
		*REGISTRY
	).unwrap();
	pub static ref ENVOY_LIFETIME_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_lifetime_seconds",
		"Lifetime of envoy WebSocket connections.",
		&["namespace_id", "pool_name"],
		LIFETIME_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref ENVOY_PING_LAG_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_ping_lag_seconds",
		"Round-trip time from engine envoy ping to pong.",
		&["namespace_id", "pool_name"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref ENVOY_TIME_SINCE_LAST_PONG_SECONDS: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_time_since_last_pong_seconds",
		"Time since the last pong was received from this envoy, observed on each ping_task tick. Diverges over time when an envoy stops responding; reaches `envoy_ping_timeout` right before the engine closes the WS with ws.timed_out.",
		&["namespace_id", "pool_name"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref WS_MESSAGE_PROCESSING_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_ws_message_processing_duration_seconds",
		"Wall-clock duration spent inside ws_to_tunnel_task::handle_message per message kind. The task processes envoy WS messages serially, so long durations head-of-line block every subsequent message (including pings, state updates, and other actors' KV ops) on the same envoy connection.",
		&["namespace_id", "pool_name", "message_kind"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref WS_MESSAGE_SLOW_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_envoy_ws_message_slow_total",
		"Count of ws_to_tunnel_task::handle_message invocations that exceeded the slow-handle warning threshold (head-of-line blocking risk).",
		&["namespace_id", "pool_name", "message_kind"],
		*REGISTRY
	).unwrap();

	pub static ref TUNNEL_PUBLISH_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_envoy_tunnel_publish_total",
		"Tunnel messages published from envoy to gateway, by outcome.",
		&["namespace_id", "pool_name", "result"],
		*REGISTRY
	).unwrap();

	pub static ref TUNNEL_TASKS_ACTIVE: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"pegboard_envoy_tunnel_tasks_active",
		"Live gateway/request tunnel message task entries on this pod.",
		&["namespace_id", "pool_name"],
		*REGISTRY
	).unwrap();

	pub static ref WS_RESPONSES_IN_FLIGHT: IntGauge = register_int_gauge_with_registry!(
		"pegboard_envoy_ws_responses_in_flight",
		"Pod-wide count of responses currently queued on or being written to envoy WebSockets via WebSocketHandle::send.",
		*REGISTRY
	).unwrap();

	pub static ref ACTOR_TASKS_ACTIVE: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"pegboard_envoy_actor_tasks_active",
		"Pod-wide count of per-actor pegboard-envoy task queues by kind. Growth indicates per-actor task backpressure.",
		&["task_kind"],
		*REGISTRY
	).unwrap();

	pub static ref WS_TO_TUNNEL_BRANCH_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_ws_to_tunnel_task_branch_duration_seconds",
		"Duration of each tokio::select! branch body inside ws_to_tunnel_task::task_inner. Lets operators see which branch dominates the loop.",
		&["branch"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref RECEIVE_INIT_PACKET_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"envoy_receive_init_packet_duration",
		"Duration to receive the init packet for a envoy connection.",
		&["namespace_id", "pool_name"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref EVENT_DEMUXER_COUNT: IntGauge = register_int_gauge_with_registry!(
		"pegboard_envoy_event_demuxer_count",
		"Number of active actor event demultiplexers.",
		*REGISTRY
	).unwrap();

	pub static ref INGESTED_EVENTS_TOTAL: IntCounter = register_int_counter_with_registry!(
		"envoy_ingested_events_total",
		"Count of actor events.",
		*REGISTRY
	).unwrap();

	pub static ref ACTOR_WAKE_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_actor_wake_duration_seconds",
		"Envoy-side actor wake duration. Time from when pegboard-envoy forwards ToEnvoyWebSocketOpen to the envoy over WS until the matching ToRivetWebSocketOpen reply arrives back from the envoy. Mirrors the gateway-side `pegboard_gateway_websocket_open_wait_seconds` so operators can split engine-side vs envoy-side wake latency.",
		// TODO: Add `was_cached` label once envoy reports whether the actor was cold-started or already warm. pegboard-envoy currently has no visibility into the envoy-side actor cache state.
		&["namespace_id", "pool_name", "result"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_COMMIT_ENVOY_DISPATCH_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"sqlite_commit_envoy_dispatch_duration_seconds",
		"Duration from sqlite commit frame arrival until depot dispatch.",
		&["namespace_id", "pool_name"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_COMMIT_ENVOY_RESPONSE_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"sqlite_commit_envoy_response_duration_seconds",
		"Duration from depot commit return until the websocket response frame is sent.",
		&["namespace_id", "pool_name"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_REQUEST_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_envoy_sqlite_request_total",
		"Total SQLite requests handled by pegboard envoy.",
		&["namespace_id", "pool_name", "type", "result"],
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_REQUEST_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_sqlite_request_duration_seconds",
		"Duration of SQLite requests handled by pegboard envoy.",
		&["namespace_id", "pool_name", "type", "result"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_REQUEST_PAGES: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_sqlite_request_pages",
		"SQLite pages requested or returned by pegboard envoy.",
		&["namespace_id", "pool_name", "type", "direction"],
		PAGE_COUNT_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_REQUEST_DIRTY_PAGES: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_sqlite_request_dirty_pages",
		"SQLite dirty pages committed by pegboard envoy.",
		&["namespace_id", "pool_name", "type"],
		PAGE_COUNT_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_REQUEST_PAYLOAD_BYTES: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_envoy_sqlite_request_payload_bytes",
		"SQLite request and response payload bytes handled by pegboard envoy.",
		&["namespace_id", "pool_name", "type", "direction"],
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_MIGRATION_ATTEMPTS_TOTAL: IntCounter = register_int_counter_with_registry!(
		"envoy_sqlite_migration_attempts_total",
		"Total number of sqlite v1 to v2 migration attempts.",
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_MIGRATION_SUCCESSES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"envoy_sqlite_migration_successes_total",
		"Total number of sqlite v1 to v2 migrations that completed successfully.",
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_MIGRATION_FAILURES_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"envoy_sqlite_migration_failures_total",
		"Total number of sqlite v1 to v2 migration failures by phase.",
		&["phase"],
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_MIGRATION_DURATION: Histogram = register_histogram_with_registry!(
		"envoy_sqlite_migration_duration_seconds",
		"Duration of sqlite v1 to v2 migrations.",
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SQLITE_MIGRATION_PAGES: Histogram = register_histogram_with_registry!(
		"envoy_sqlite_migration_pages",
		"Number of pages imported during sqlite v1 to v2 migration.",
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref ACK_MSG_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_ack_msg_duration",
		"Time to deserialize and reply to an incoming ToEnvoyConn msg.",
		&["namespace_id", "pool_name"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref PROCESS_MSG_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"pegboard_envoy_process_msg_duration",
		"Time to process an incoming ToEnvoyConn msg.",
		&["namespace_id", "pool_name"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref MSG_PROCESSED_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"pegboard_envoy_msg_processed_total",
		"Count of total tunnel messages processed.",
		&["namespace_id", "pool_name"],
		*REGISTRY
	).unwrap();
}

pub fn inc_envoy_connection_state(
	namespace_id: &str,
	pool_name: &str,
	protocol_version: &str,
	state: EnvoyState,
	reason: &'static str,
) {
	ENVOY_CONNECTIONS_BY_STATE
		.with_label_values(&[namespace_id, pool_name, protocol_version, state.as_str()])
		.inc();
	ENVOY_STATE_TRANSITION_TOTAL
		.with_label_values(&[
			namespace_id,
			pool_name,
			protocol_version,
			state.as_str(),
			reason,
		])
		.inc();
}

pub fn dec_envoy_connection_state(
	namespace_id: &str,
	pool_name: &str,
	protocol_version: &str,
	state: EnvoyState,
) {
	ENVOY_CONNECTIONS_BY_STATE
		.with_label_values(&[namespace_id, pool_name, protocol_version, state.as_str()])
		.dec();
}

pub fn transition_envoy_connection_state(
	namespace_id: &str,
	pool_name: &str,
	protocol_version: &str,
	from: EnvoyState,
	to: EnvoyState,
	reason: &'static str,
) {
	if from == to {
		ENVOY_STATE_TRANSITION_TOTAL
			.with_label_values(&[
				namespace_id,
				pool_name,
				protocol_version,
				to.as_str(),
				reason,
			])
			.inc();
		return;
	}

	dec_envoy_connection_state(namespace_id, pool_name, protocol_version, from);
	inc_envoy_connection_state(namespace_id, pool_name, protocol_version, to, reason);
}

pub fn set_envoy_connection_state(
	namespace_id: &str,
	pool_name: &str,
	protocol_version: &str,
	from: Option<EnvoyState>,
	to: Option<EnvoyState>,
	reason: &'static str,
) {
	match (from, to) {
		(Some(from), Some(to)) => {
			transition_envoy_connection_state(
				namespace_id,
				pool_name,
				protocol_version,
				from,
				to,
				reason,
			);
		}
		(Some(from), None) => {
			dec_envoy_connection_state(namespace_id, pool_name, protocol_version, from);
		}
		(None, Some(to)) => {
			inc_envoy_connection_state(namespace_id, pool_name, protocol_version, to, reason);
		}
		(None, None) => {}
	}
}
