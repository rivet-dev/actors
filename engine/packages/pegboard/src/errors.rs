use gas::prelude::*;
use rivet_error::*;
use serde::{Deserialize, Serialize};

use crate::ops::serverless_metadata::fetch::ServerlessMetadataErrorEnvelope;

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("actor")]
pub enum Actor {
	#[error("not_found", "The actor does not exist or was destroyed.")]
	NotFound,

	#[error("namespace_not_found", "The namespace does not exist.")]
	NamespaceNotFound,

	#[error(
		"creation_rate_limit",
		"Too many actors created at once. Try again later."
	)]
	CreationRateLimit,

	#[error(
		"input_too_large",
		"Actor input too large.",
		"Input too large (max {max_size})."
	)]
	InputTooLarge { max_size: usize },

	#[error("empty_key", "Key label cannot be empty.")]
	EmptyKey,

	#[error(
		"key_too_large",
		"Key label too large.",
		"Key label too large (max {max_size} bytes): {key_preview}"
	)]
	KeyTooLarge {
		max_size: usize,
		key_preview: String,
	},

	#[error(
		"duplicate_key",
		"Actor key already in use.",
		"Actor key '{key}' already in use for actor '{existing_actor_id}'"
	)]
	DuplicateKey { key: String, existing_actor_id: Id },

	#[error("destroyed_during_creation", "Actor was destroyed during creation.")]
	DestroyedDuringCreation,

	#[error(
		"destroyed_while_waiting_for_ready",
		"Actor was destroyed while waiting for ready state."
	)]
	DestroyedWhileWaitingForReady,

	#[error(
		"key_reserved_in_different_datacenter",
		"Actor key is already reserved in a different datacenter. Either remove the datacenter constraint to automatically create this actor in the correct datacenter or provide the datacenter that matches.",
		"Actor key is already reserved in the datacenter '{datacenter_label}'. Either remove the datacenter constraint to automatically create this actor in the correct datacenter or provide the datacenter that matches."
	)]
	KeyReservedInDifferentDatacenter { datacenter_label: u16 },

	#[error(
		"no_runner_config_configured",
		"No runner config configured in any datacenter. Validate a provider is listed that matches requested pool name.",
		"No runner config with name '{pool_name}' are available in any datacenter for the namespace '{namespace}'. Validate a provider is listed that matches the requested pool name."
	)]
	NoRunnerConfigConfigured {
		namespace: String,
		pool_name: String,
	},

	#[error("kv_key_not_found", "The KV key does not exist for this actor.")]
	KvKeyNotFound,

	#[error(
		"kv_storage_quota_exceeded",
		"Not enough space left in storage.",
		"Not enough space left in storage ({remaining} bytes remaining, current payload is {payload_size} bytes)."
	)]
	KvStorageQuotaExceeded {
		remaining: usize,
		payload_size: usize,
	},
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error(
	"pegboard",
	"route_subscribe_timeout",
	"Timed out subscribing to actor routing events.",
	"Timed out subscribing to actor routing events after {elapsed_ms}ms (timeout {timeout_ms}ms)."
)]
pub struct RouteSubscribeTimeout {
	pub elapsed_ms: u64,
	pub timeout_ms: u64,
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error(
	"pegboard",
	"route_fetch_actor_timeout",
	"Timed out fetching actor routing state.",
	"Timed out fetching actor routing state for actor {actor_id} after {elapsed_ms}ms (timeout {timeout_ms}ms)."
)]
pub struct RouteFetchActorTimeout {
	pub actor_id: String,
	pub elapsed_ms: u64,
	pub timeout_ms: u64,
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error(
	"pegboard",
	"route_auth_check_timeout",
	"Timed out checking actor route authorization.",
	"Timed out checking actor route authorization for actor {actor_id} after {elapsed_ms}ms (timeout {timeout_ms}ms)."
)]
pub struct RouteAuthCheckTimeout {
	pub actor_id: String,
	pub elapsed_ms: u64,
	pub timeout_ms: u64,
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error(
	"pegboard",
	"route_wake_signal_timeout",
	"Timed out sending actor wake signal.",
	"Timed out sending actor wake signal for actor {actor_id} after {elapsed_ms}ms (timeout {timeout_ms}ms)."
)]
pub struct RouteWakeSignalTimeout {
	pub actor_id: String,
	pub elapsed_ms: u64,
	pub timeout_ms: u64,
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error(
	"pegboard",
	"route_resolve_query_timeout",
	"Timed out resolving actor query route.",
	"Timed out resolving actor query route after {elapsed_ms}ms (timeout {timeout_ms}ms)."
)]
pub struct RouteResolveQueryTimeout {
	pub elapsed_ms: u64,
	pub timeout_ms: u64,
}

#[derive(RivetError, Debug, Clone, Deserialize, Serialize)]
#[error("runner")]
pub enum Runner {
	#[error("not_found", "The runner does not exist.")]
	NotFound,
}

#[derive(RivetError, Debug, Deserialize, Serialize)]
#[error("runner_config")]
pub enum RunnerConfig {
	#[error("invalid", "Invalid runner config.", "Invalid runner config: {reason}")]
	Invalid { reason: String },

	#[error("not_found", "No config for this runner exists.")]
	NotFound,
}

#[derive(RivetError, Debug, Deserialize, Serialize)]
#[error("serverless_runner_pool")]
pub enum ServerlessRunnerPool {
	#[error("not_found", "No serverless pool for this runner exists.")]
	NotFound,
	#[error(
		"failed_to_fetch_metadata",
		"Failed to fetch serverless metadata: {reason}."
	)]
	FailedToFetchMetadata {
		reason: ServerlessMetadataErrorEnvelope,
	},
}
