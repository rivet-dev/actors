use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use rivet_envoy_client::config::HttpRequest;

use crate::inspector::InspectorTabEntry;

const DEFAULT_STATE_SAVE_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_CREATE_VARS_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CREATE_CONN_STATE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_ON_BEFORE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_ON_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_ON_MIGRATE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_ACTION_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_SLEEP_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SLEEP_GRACE_PERIOD: Duration = Duration::from_secs(15);
const DEFAULT_CONNECTION_LIVENESS_TIMEOUT: Duration = Duration::from_millis(2500);
const DEFAULT_CONNECTION_LIVENESS_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_MAX_QUEUE_SIZE: u32 = 1000;
pub const DEFAULT_MAX_SCHEDULES: u32 = 1_000;
const DEFAULT_MAX_QUEUE_MESSAGE_SIZE: u32 = 65_536;
const DEFAULT_MAX_INCOMING_MESSAGE_SIZE: u32 = 65_536;
const DEFAULT_MAX_OUTGOING_MESSAGE_SIZE: u32 = 1_048_576;
pub(crate) const MAX_SQLITE_TRANSACTION_TRACE_STATEMENTS: usize = 32;

#[derive(Clone)]
pub enum CanHibernateWebSocket {
	Bool(bool),
	Callback(Arc<dyn Fn(&HttpRequest) -> bool + Send + Sync>),
}

impl fmt::Debug for CanHibernateWebSocket {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Bool(value) => f.debug_tuple("Bool").field(value).finish(),
			Self::Callback(_) => f.write_str("Callback(..)"),
		}
	}
}

impl Default for CanHibernateWebSocket {
	fn default() -> Self {
		Self::Bool(false)
	}
}

#[derive(Clone, Debug, Default)]
pub struct ActorConfigOverrides {
	pub sleep_grace_period: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct ActionDefinition {
	pub name: String,
}

/// Experimental SQLite profiling configuration.
///
/// This entire configuration surface, including every field, is subject to
/// change without notice.
#[derive(Clone, Debug)]
pub struct SqliteProfilingConfig {
	pub enabled: bool,
	pub max_tracked_statement_fingerprints: usize,
	pub max_tracked_transaction_fingerprints: usize,
	pub max_prometheus_series: usize,
	pub max_statements_per_transaction_trace: usize,
	pub max_get_pages_requests_per_trace: usize,
	pub max_transaction_name_bytes: usize,
	pub slow_operation_threshold_ms: u64,
	pub baseline_sample_rate: f64,
	pub max_diagnostic_events_per_minute: usize,
	pub diagnostic_event_queue_capacity: usize,
}

impl Default for SqliteProfilingConfig {
	fn default() -> Self {
		Self {
			enabled: true,
			max_tracked_statement_fingerprints: 128,
			max_tracked_transaction_fingerprints: 8,
			max_prometheus_series: 25_000,
			max_statements_per_transaction_trace: 32,
			max_get_pages_requests_per_trace: 16,
			max_transaction_name_bytes: 128,
			slow_operation_threshold_ms: 10,
			baseline_sample_rate: 0.001,
			max_diagnostic_events_per_minute: 120,
			diagnostic_event_queue_capacity: 256,
		}
	}
}

/// Sparse experimental SQLite profiling configuration used at runtime
/// boundaries.
///
/// This entire configuration surface, including every field, is subject to
/// change without notice.
#[derive(Clone, Debug, Default)]
pub struct SqliteProfilingConfigInput {
	pub enabled: Option<bool>,
	pub max_tracked_statement_fingerprints: Option<u32>,
	pub max_tracked_transaction_fingerprints: Option<u32>,
	pub max_prometheus_series: Option<u32>,
	pub max_statements_per_transaction_trace: Option<u32>,
	pub max_get_pages_requests_per_trace: Option<u32>,
	pub max_transaction_name_bytes: Option<u32>,
	pub slow_operation_threshold_ms: Option<u32>,
	pub baseline_sample_rate: Option<f64>,
	pub max_diagnostic_events_per_minute: Option<u32>,
	pub diagnostic_event_queue_capacity: Option<u32>,
}

impl SqliteProfilingConfig {
	fn from_input(input: SqliteProfilingConfigInput) -> Self {
		let mut config = Self::default();
		macro_rules! set_usize {
			($field:ident) => {
				if let Some(value) = input.$field {
					config.$field = value as usize;
				}
			};
		}
		if let Some(value) = input.enabled {
			config.enabled = value;
		}
		set_usize!(max_tracked_statement_fingerprints);
		set_usize!(max_tracked_transaction_fingerprints);
		set_usize!(max_prometheus_series);
		set_usize!(max_statements_per_transaction_trace);
		set_usize!(max_get_pages_requests_per_trace);
		set_usize!(max_transaction_name_bytes);
		if let Some(value) = input.slow_operation_threshold_ms {
			config.slow_operation_threshold_ms = u64::from(value);
		}
		if let Some(value) = input.baseline_sample_rate {
			config.baseline_sample_rate = value;
		}
		set_usize!(max_diagnostic_events_per_minute);
		set_usize!(diagnostic_event_queue_capacity);
		config
	}
}

#[derive(Clone, Debug)]
pub struct ActorConfig {
	pub name: Option<String>,
	pub icon: Option<String>,
	/// Whether the user declared a SQLite database for this actor (`db({...})`
	/// on the TS side). Gates the inspector database tab.
	pub has_database: bool,
	pub remote_sqlite: bool,
	pub sqlite_profiling: SqliteProfilingConfig,
	/// Enables the experimental Actor Runtime Socket.
	pub enable_actor_runtime_socket: bool,
	/// Whether the user declared actor state (`state: ...` or `createState`).
	/// Gates the inspector state tab and state-subscription messages.
	pub has_state: bool,
	pub can_hibernate_websocket: CanHibernateWebSocket,
	pub state_save_interval: Duration,
	pub create_vars_timeout: Duration,
	pub create_conn_state_timeout: Duration,
	pub on_before_connect_timeout: Duration,
	pub on_connect_timeout: Duration,
	pub on_migrate_timeout: Duration,
	pub action_timeout: Duration,
	pub sleep_timeout: Duration,
	pub no_sleep: bool,
	pub sleep_grace_period: Duration,
	pub sleep_grace_period_overridden: bool,
	pub connection_liveness_timeout: Duration,
	pub connection_liveness_interval: Duration,
	pub max_queue_size: u32,
	pub max_schedules: u32,
	pub max_queue_message_size: u32,
	pub max_incoming_message_size: u32,
	pub max_outgoing_message_size: u32,
	pub overrides: Option<ActorConfigOverrides>,
	pub actions: Vec<ActionDefinition>,
	/// Author-declared inspector tab entries (custom tabs + built-in
	/// hides). Validated upstream (Zod / builder).
	pub inspector_tabs: Vec<InspectorTabEntry>,
}

/// Sparse, serialization-friendly actor configuration. All fields are optional with millisecond integers instead of Duration. Used at runtime boundaries (NAPI, config files). Convert to ActorConfig via ActorConfig::from_input().
#[derive(Clone, Debug, Default)]
pub struct ActorConfigInput {
	pub name: Option<String>,
	pub icon: Option<String>,
	pub has_database: Option<bool>,
	pub remote_sqlite: Option<bool>,
	pub sqlite_profiling: Option<SqliteProfilingConfigInput>,
	pub enable_actor_runtime_socket: Option<bool>,
	pub has_state: Option<bool>,
	pub can_hibernate_websocket: Option<bool>,
	pub state_save_interval_ms: Option<u32>,
	pub create_vars_timeout_ms: Option<u32>,
	pub create_conn_state_timeout_ms: Option<u32>,
	pub on_before_connect_timeout_ms: Option<u32>,
	pub on_connect_timeout_ms: Option<u32>,
	pub on_migrate_timeout_ms: Option<u32>,
	pub action_timeout_ms: Option<u32>,
	pub sleep_timeout_ms: Option<u32>,
	pub no_sleep: Option<bool>,
	pub sleep_grace_period_ms: Option<u32>,
	pub connection_liveness_timeout_ms: Option<u32>,
	pub connection_liveness_interval_ms: Option<u32>,
	pub max_queue_size: Option<u32>,
	pub max_schedules: Option<u32>,
	pub max_queue_message_size: Option<u32>,
	pub max_incoming_message_size: Option<u32>,
	pub max_outgoing_message_size: Option<u32>,
	pub actions: Option<Vec<ActionDefinition>>,
	pub inspector_tabs: Option<Vec<InspectorTabEntry>>,
}

impl ActorConfig {
	pub fn from_input(config: ActorConfigInput) -> Self {
		let mut actor_config = Self {
			name: config.name,
			icon: config.icon,
			has_database: config.has_database.unwrap_or(false),
			remote_sqlite: config.remote_sqlite.unwrap_or(false),
			sqlite_profiling: config
				.sqlite_profiling
				.map(SqliteProfilingConfig::from_input)
				.unwrap_or_default(),
			enable_actor_runtime_socket: config.enable_actor_runtime_socket.unwrap_or(false),
			has_state: config.has_state.unwrap_or(false),
			..Self::default()
		};
		if let Some(can_hibernate_websocket) = config.can_hibernate_websocket {
			actor_config.can_hibernate_websocket =
				CanHibernateWebSocket::Bool(can_hibernate_websocket);
		}
		if let Some(value) = config.state_save_interval_ms {
			actor_config.state_save_interval = duration_ms(value);
		}
		if let Some(value) = config.create_vars_timeout_ms {
			actor_config.create_vars_timeout = duration_ms(value);
		}
		if let Some(value) = config.create_conn_state_timeout_ms {
			actor_config.create_conn_state_timeout = duration_ms(value);
		}
		if let Some(value) = config.on_before_connect_timeout_ms {
			actor_config.on_before_connect_timeout = duration_ms(value);
		}
		if let Some(value) = config.on_connect_timeout_ms {
			actor_config.on_connect_timeout = duration_ms(value);
		}
		if let Some(value) = config.on_migrate_timeout_ms {
			actor_config.on_migrate_timeout = duration_ms(value);
		}
		if let Some(value) = config.action_timeout_ms {
			actor_config.action_timeout = duration_ms(value);
		}
		if let Some(value) = config.sleep_timeout_ms {
			actor_config.sleep_timeout = duration_ms(value);
		}
		if let Some(value) = config.no_sleep {
			actor_config.no_sleep = value;
		}
		if let Some(value) = config.sleep_grace_period_ms {
			actor_config.sleep_grace_period = duration_ms(value);
			actor_config.sleep_grace_period_overridden = true;
		}
		if let Some(value) = config.connection_liveness_timeout_ms {
			actor_config.connection_liveness_timeout = duration_ms(value);
		}
		if let Some(value) = config.connection_liveness_interval_ms {
			actor_config.connection_liveness_interval = duration_ms(value);
		}
		if let Some(value) = config.max_queue_size {
			actor_config.max_queue_size = value;
		}
		if let Some(value) = config.max_schedules {
			actor_config.max_schedules = value;
		}
		if let Some(value) = config.max_queue_message_size {
			actor_config.max_queue_message_size = value;
		}
		if let Some(value) = config.max_incoming_message_size {
			actor_config.max_incoming_message_size = value;
		}
		if let Some(value) = config.max_outgoing_message_size {
			actor_config.max_outgoing_message_size = value;
		}
		if let Some(actions) = config.actions {
			actor_config.actions = actions;
		}
		if let Some(tabs) = config.inspector_tabs {
			actor_config.inspector_tabs = tabs;
		}

		actor_config
	}

	pub fn effective_sleep_grace_period(&self) -> Duration {
		cap_duration(
			self.sleep_grace_period,
			self.overrides
				.as_ref()
				.and_then(|overrides| overrides.sleep_grace_period),
		)
	}

	/// Runtime authority for rejecting malformed config that bypassed the
	/// TypeScript Zod layer (direct Rust builders, NAPI shim corruption,
	/// etc.). Call this before constructing an `ActorFactory` from this
	/// config so the actor never starts with garbage state.
	pub fn validate(&self) -> anyhow::Result<()> {
		crate::inspector::validate_inspector_tabs(&self.inspector_tabs)?;
		anyhow::ensure!(
			self.sqlite_profiling.baseline_sample_rate.is_finite()
				&& (0.0..=1.0).contains(&self.sqlite_profiling.baseline_sample_rate),
			"SQLite profiling baselineSampleRate must be between 0 and 1"
		);
		anyhow::ensure!(
			self.sqlite_profiling.max_get_pages_requests_per_trace <= 16,
			"SQLite profiling maxGetPagesRequestsPerTrace must be at most 16"
		);
		anyhow::ensure!(
			self.sqlite_profiling.max_statements_per_transaction_trace
				<= MAX_SQLITE_TRANSACTION_TRACE_STATEMENTS,
			"SQLite profiling maxStatementsPerTransactionTrace must be at most 32"
		);
		anyhow::ensure!(
			self.sqlite_profiling.max_transaction_name_bytes > 0,
			"SQLite profiling maxTransactionNameBytes must be greater than zero"
		);
		Ok(())
	}
}

impl Default for ActorConfig {
	fn default() -> Self {
		Self {
			name: None,
			icon: None,
			has_database: false,
			remote_sqlite: false,
			sqlite_profiling: SqliteProfilingConfig::default(),
			enable_actor_runtime_socket: false,
			has_state: false,
			can_hibernate_websocket: CanHibernateWebSocket::default(),
			state_save_interval: DEFAULT_STATE_SAVE_INTERVAL,
			create_vars_timeout: DEFAULT_CREATE_VARS_TIMEOUT,
			create_conn_state_timeout: DEFAULT_CREATE_CONN_STATE_TIMEOUT,
			on_before_connect_timeout: DEFAULT_ON_BEFORE_CONNECT_TIMEOUT,
			on_connect_timeout: DEFAULT_ON_CONNECT_TIMEOUT,
			on_migrate_timeout: DEFAULT_ON_MIGRATE_TIMEOUT,
			action_timeout: DEFAULT_ACTION_TIMEOUT,
			sleep_timeout: DEFAULT_SLEEP_TIMEOUT,
			no_sleep: false,
			sleep_grace_period: DEFAULT_SLEEP_GRACE_PERIOD,
			sleep_grace_period_overridden: false,
			connection_liveness_timeout: DEFAULT_CONNECTION_LIVENESS_TIMEOUT,
			connection_liveness_interval: DEFAULT_CONNECTION_LIVENESS_INTERVAL,
			max_queue_size: DEFAULT_MAX_QUEUE_SIZE,
			max_schedules: DEFAULT_MAX_SCHEDULES,
			max_queue_message_size: DEFAULT_MAX_QUEUE_MESSAGE_SIZE,
			max_incoming_message_size: DEFAULT_MAX_INCOMING_MESSAGE_SIZE,
			max_outgoing_message_size: DEFAULT_MAX_OUTGOING_MESSAGE_SIZE,
			overrides: None,
			actions: Vec::new(),
			inspector_tabs: Vec::new(),
		}
	}
}

fn cap_duration(duration: Duration, override_duration: Option<Duration>) -> Duration {
	if let Some(override_duration) = override_duration {
		duration.min(override_duration)
	} else {
		duration
	}
}

fn duration_ms(value: u32) -> Duration {
	Duration::from_millis(u64::from(value))
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/config.rs"]
mod tests;
