use rivet_metrics::{BUCKETS, MICRO_BUCKETS, REGISTRY, prometheus::*};

lazy_static::lazy_static! {
	pub static ref SUBSCRIBER_COUNT: IntCounterVec = register_int_counter_vec_with_registry!(
		"ups_subscriber_count",
		"Total number of subscribers ever created by subject.",
		&["subject"],
		*REGISTRY
	).unwrap();
	pub static ref ACTIVE_SUBSCRIBER_COUNT: IntGaugeVec = register_int_gauge_vec_with_registry!(
		"ups_active_subscriber_count",
		"Number of active subscribers by subject.",
		&["subject"],
		*REGISTRY
	).unwrap();
	// PubSub layer metrics
	pub static ref LOCAL_SUBSCRIBER_COUNT: IntGauge = register_int_gauge_with_registry!(
		"ups_local_subscriber_count",
		"Number of local in-memory subscriber entries.",
		*REGISTRY
	).unwrap();
	// Memory driver metrics
	pub static ref MEMORY_SUBSCRIBER_COUNT: IntGauge = register_int_gauge_with_registry!(
		"ups_memory_subscriber_count",
		"Number of subject entries in the memory driver.",
		*REGISTRY
	).unwrap();
	// Message metrics
	pub static ref MESSAGE_RECV_COUNT: IntCounterVec = register_int_counter_vec_with_registry!(
		"ups_message_recv_count",
		"Total number of messages ever received by subject.",
		&["subject"],
		*REGISTRY
	).unwrap();
	pub static ref MESSAGE_SEND_COUNT: IntCounterVec = register_int_counter_vec_with_registry!(
		"ups_message_send_count",
		"Total number of messages ever sent by subject.",
		&["kind", "subject"],
		*REGISTRY
	).unwrap();
	pub static ref PUBLISH_ATTEMPT_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"ups_publish_attempt_duration_seconds",
		"Duration of each individual driver publish attempt.",
		&["subject_root"],
		vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
		*REGISTRY
	).unwrap();
	pub static ref PUBLISH_RETRY_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"ups_publish_retry_total",
		"Total number of retried driver publish attempts.",
		&["subject_root"],
		*REGISTRY
	).unwrap();
	pub static ref NATS_CLIENT_IN_MESSAGES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"nats_client_in_messages_total",
		"Total number of messages received by the async-nats client.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_CLIENT_OUT_MESSAGES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"nats_client_out_messages_total",
		"Total number of messages sent by the async-nats client.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_CLIENT_IN_BYTES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"nats_client_in_bytes_total",
		"Total number of bytes received by the async-nats client.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_CLIENT_OUT_BYTES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"nats_client_out_bytes_total",
		"Total number of bytes sent by the async-nats client.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_SLOW_CONSUMER_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
		"nats_slow_consumer_total",
		"Total number of async-nats slow-consumer drops by subject root.",
		&["subject_root"],
		*REGISTRY
	).unwrap();
	pub static ref NATS_SUBSCRIPTION_PENDING_MESSAGES: IntGauge = register_int_gauge_with_registry!(
		"nats_subscription_pending_messages",
		"Current number of messages buffered in async-nats subscription channels.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_SUBSCRIPTION_PENDING_BYTES: IntGauge = register_int_gauge_with_registry!(
		"nats_subscription_pending_bytes",
		"Current number of bytes buffered in async-nats subscription channels.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_ACTIVE_SUBSCRIPTIONS: IntGauge = register_int_gauge_with_registry!(
		"nats_active_subscriptions",
		"Current number of active async-nats subscription handles.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_ACTIVE_SUBSCRIPTION_CAPACITY: IntGauge = register_int_gauge_with_registry!(
		"nats_active_subscription_capacity",
		"Total message capacity across active async-nats subscription channels.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_SUBSCRIPTION_DROPPED_MESSAGES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"nats_subscription_dropped_messages_total",
		"Total number of messages dropped because async-nats subscription channels were full.",
		*REGISTRY
	).unwrap();
	pub static ref NATS_SUBSCRIPTION_DROPPED_BYTES_TOTAL: IntCounter = register_int_counter_with_registry!(
		"nats_subscription_dropped_bytes_total",
		"Total number of bytes dropped because async-nats subscription channels were full.",
		*REGISTRY
	).unwrap();
	pub static ref BYTES_PER_MESSAGE: HistogramVec = register_histogram_vec_with_registry!(
		"ups_bytes_per_message",
		"Amount of bytes per message received.",
		&["subject"],
		vec![16.0, 32.0, 64.0, 128.0, 256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0, 16777216.0],
		*REGISTRY
	).unwrap();

	pub static ref MESSAGE_RECV_LAG: HistogramVec = register_histogram_vec_with_registry!(
		"ups_message_recv_lag",
		"Duration from msg send to msg recv.",
		&["subject"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	// Request metrics
	pub static ref REQUEST_RESPONSE_LAG: HistogramVec = register_histogram_vec_with_registry!(
		"ups_request_response_lag",
		"Time between request start and response recv.",
		&["subject"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref REQUEST_TIMEOUT_COUNT: IntCounterVec = register_int_counter_vec_with_registry!(
		"ups_request_timeout_count",
		"Total number of requests that timed out.",
		&["subject"],
		*REGISTRY
	).unwrap();
}
