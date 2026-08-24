use rivet_error::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

static ACTION_NOT_FOUND_SCHEMA: RivetErrorSchema = RivetErrorSchema {
	group: "actor",
	code: "action_not_found",
	default_message: "Action not found",
	meta_type: None,
	_macro_marker: MacroMarker { _private: () },
};

pub fn action_not_found(name: impl Into<String>) -> anyhow::Error {
	let name = name.into();
	anyhow::Error::new(RivetError {
		kind: RivetErrorKind::Static(&ACTION_NOT_FOUND_SCHEMA),
		meta: None,
		message: Some(format!("Action `{name}` was not found.")),
		actor: None,
	})
}

pub fn public_error_status_code(group: &str, code: &str) -> Option<u16> {
	match (group, code) {
		("auth", "forbidden") => Some(403),
		("actor", "not_found") => Some(404),
		("actor", "action_not_found") => Some(404),
		("actor", "method_not_allowed") => Some(405),
		("actor", "action_timed_out") => Some(408),
		("actor", "aborted") => Some(400),
		(
			"actor_runtime_socket",
			"unsupported" | "not_enabled" | "closed" | "database_unavailable",
		) => Some(400),
		("message", "incoming_too_long" | "outgoing_too_long") => Some(400),
		("schedule", _) => Some(400),
		(
			"queue",
			"full"
			| "message_too_large"
			| "message_invalid"
			| "invalid_payload"
			| "invalid_completion_payload"
			| "already_completed"
			| "previous_message_not_completed"
			| "complete_not_configured"
			| "timed_out",
		) => Some(400),
		("kv", _) => Some(400),
		("user", _) => Some(400),
		_ => None,
	}
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("schedule")]
pub enum ScheduleRuntimeError {
	#[error(
		"invalid_name",
		"Schedule name is invalid.",
		"Schedule name is invalid: {reason}"
	)]
	InvalidName { reason: String },

	#[error(
		"invalid_cron_expression",
		"Cron expression is invalid.",
		"Cron expression is invalid: {reason}"
	)]
	InvalidCronExpression { reason: String },

	#[error(
		"invalid_timezone",
		"Schedule timezone is invalid.",
		"Schedule timezone '{timezone}' is invalid."
	)]
	InvalidTimezone { timezone: String },

	#[error(
		"invalid_interval",
		"Schedule interval is invalid.",
		"Schedule interval must be at least {minimum_ms} ms; received {interval_ms} ms."
	)]
	InvalidInterval { interval_ms: i64, minimum_ms: i64 },

	#[error(
		"invalid_max_history",
		"Schedule history limit is invalid.",
		"Schedule maxHistory must be between 0 and {maximum}; received {max_history}."
	)]
	InvalidMaxHistory { max_history: i64, maximum: i64 },

	#[error(
		"max_schedules_exceeded",
		"Actor schedule limit reached.",
		"Actor has reached its limit of {maximum} pending schedules."
	)]
	MaxSchedulesExceeded { maximum: u32 },

	#[error(
		"invalid_schedule_row",
		"Stored schedule data is invalid.",
		"Stored schedule '{schedule_id}' is invalid: {reason}"
	)]
	InvalidScheduleRow { schedule_id: String, reason: String },

	#[error(
		"interrupted",
		"Scheduled action was interrupted.",
		"Scheduled action was interrupted before completion."
	)]
	Interrupted,
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("kv")]
pub(crate) enum KvRuntimeError {
	#[error(
		"value_too_large",
		"KV value is too large.",
		"KV value too large ({size} bytes). Limit is {limit} bytes."
	)]
	ValueTooLarge { size: usize, limit: usize },
	#[error(
		"key_too_large",
		"KV key is too large.",
		"KV key too large ({size} bytes). Limit is {limit} bytes."
	)]
	KeyTooLarge { size: usize, limit: usize },
}

pub(crate) fn is_internal_error(group: &str, code: &str) -> bool {
	(group == "core" || group == "rivetkit") && code == "internal_error"
}

pub(crate) fn is_client_error_public(group: &str, code: &str) -> bool {
	public_error_status_code(group, code).is_some()
}

/// Masks private error messages before serializing them to clients because they
/// may contain private implementation details or user data.
pub(crate) fn client_error_message<'a>(group: &str, code: &str, message: &'a str) -> &'a str {
	if is_client_error_public(group, code) {
		message
	} else {
		INTERNAL_ERROR.default_message
	}
}

/// Drops private error metadata before serializing it to clients because it may
/// contain private implementation details or user data.
pub(crate) fn client_error_metadata<'a>(
	group: &str,
	code: &str,
	metadata: Option<&'a JsonValue>,
) -> Option<&'a JsonValue> {
	if is_client_error_public(group, code) {
		metadata
	} else {
		None
	}
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("actor")]
pub enum ActorLifecycle {
	#[error("starting", "Actor is starting.")]
	Starting,

	#[error("not_ready", "Actor is not ready.")]
	NotReady,

	#[error("stopping", "Actor is stopping.")]
	Stopping,

	#[error("destroying", "Actor is destroying.")]
	Destroying,

	#[error("shutdown_timeout", "Actor shutdown timed out.")]
	ShutdownTimeout,

	#[error("dropped_reply", "Actor reply channel was dropped without a response.")]
	DroppedReply,

	#[error(
		"overloaded",
		"Actor is overloaded.",
		"Actor channel '{channel}' is overloaded while attempting to {operation} (capacity {capacity})."
	)]
	Overloaded {
		channel: String,
		capacity: usize,
		operation: String,
	},
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("actor")]
pub enum ActorRuntime {
	#[error(
		"not_configured",
		"Actor capability is not configured.",
		"Actor capability '{component}' is not configured."
	)]
	NotConfigured { component: String },

	#[error(
		"not_found",
		"Actor resource was not found.",
		"Actor {resource} '{id}' was not found."
	)]
	NotFound { resource: String, id: String },

	#[error(
		"not_registered",
		"Actor factory is not registered.",
		"Actor factory '{actor_name}' is not registered."
	)]
	NotRegistered { actor_name: String },

	#[error("missing_input", "Actor input is missing.")]
	MissingInput,

	#[error(
		"invalid_operation",
		"Actor operation is invalid.",
		"Actor operation '{operation}' is invalid: {reason}"
	)]
	InvalidOperation { operation: String, reason: String },

	#[error(
		"panicked",
		"Actor task panicked.",
		"Actor task panicked while running {operation}."
	)]
	Panicked { operation: String },
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("protocol")]
pub(crate) enum ProtocolError {
	#[error(
		"invalid_http_request",
		"Invalid HTTP request.",
		"Invalid HTTP request {field}: {reason}"
	)]
	InvalidHttpRequest { field: String, reason: String },

	#[error(
		"invalid_http_response",
		"Invalid HTTP response.",
		"Invalid HTTP response {field}: {reason}"
	)]
	InvalidHttpResponse { field: String, reason: String },

	#[error(
		"invalid_actor_connect_request",
		"Invalid actor-connect request.",
		"Invalid actor-connect request {field}: {reason}"
	)]
	InvalidActorConnectRequest { field: String, reason: String },

	#[error(
		"invalid_persisted_data",
		"Invalid persisted actor data.",
		"Invalid persisted {label}: {reason}"
	)]
	InvalidPersistedData { label: String, reason: String },

	#[error(
		"unsupported_encoding",
		"Unsupported protocol encoding.",
		"Unsupported protocol encoding '{encoding}'."
	)]
	UnsupportedEncoding { encoding: String },
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("sqlite")]
pub(crate) enum SqliteRuntimeError {
	#[error(
		"unavailable",
		"SQLite is unavailable.",
		"Actor database is not available because rivetkit-core was built without the sqlite feature."
	)]
	Unavailable,

	#[error("closed", "SQLite database is closed.")]
	Closed,

	#[error(
		"not_configured",
		"SQLite is not configured.",
		"SQLite {component} is not configured."
	)]
	NotConfigured { component: String },

	#[error(
		"invalid_bind_parameter",
		"Invalid SQLite bind parameter.",
		"Invalid SQLite bind parameter {name}: {reason}"
	)]
	InvalidBindParameter { name: String, reason: String },

	#[error(
		"writer_busy",
		"SQLite writer is busy.",
		"SQLite writer is busy because a transaction is already open."
	)]
	WriterBusy,

	#[error(
		"remote_unavailable",
		"Remote SQLite is unavailable.",
		"Remote SQLite is unavailable: {reason}"
	)]
	RemoteUnavailable { reason: String },

	#[error(
		"remote_execution_failed",
		"Remote SQLite execution failed.",
		"Remote SQLite execution failed: {message}"
	)]
	RemoteExecutionFailed { message: String },

	#[error(
		"remote_indeterminate_result",
		"Remote SQLite result is indeterminate.",
		"Remote SQLite {operation} may have completed, but the envoy disconnected before returning a result."
	)]
	RemoteIndeterminateResult { operation: String },

	#[error(
		"remote_fence_mismatch",
		"Remote SQLite generation is stale.",
		"Remote SQLite generation is stale: {reason}"
	)]
	RemoteFenceMismatch { reason: String },
}
