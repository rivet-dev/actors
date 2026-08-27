use rivet_util::Id;
use serde::{Deserialize, Serialize};

/// Keep in sync with `website/src/content/docs/actors/troubleshooting.mdx` and
/// `frontend/src/components/actors/actor-status-label.tsx`.
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RunnerPoolError {
	/// Serverless: SSE returned non-200 status code (e.g., 404, 500)
	ServerlessHttpError { status_code: u16, body: String },

	/// Serverless: SSE stream ended unexpectedly before runner initialized
	ServerlessStreamEndedEarly,

	/// Serverless: SSE connection or network error
	ServerlessConnectionError { message: String },

	/// Serverless: the configured URL is not a destination the engine is allowed to reach
	ServerlessDestinationBlocked { reason: String },

	/// Serverless: Runner sent invalid payload
	ServerlessInvalidSsePayload {
		message: String,
		raw_payload: Option<String>,
	},

	/// RivetKit was downgraded to a version after being upgraded to use envoys
	Downgrade,

	/// Internal error
	InternalError,
}

/// Keep in sync with `website/src/content/docs/actors/troubleshooting.mdx` and
/// `frontend/src/components/actors/actor-status-label.tsx`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorError {
	/// Runner pool-related errors
	RunnerPoolError(RunnerPoolError),
	/// No runners available matching the runner name
	NoCapacity,
	/// Runner was allocated but never started the actor (GC timeout)
	RunnerNoResponse { runner_id: Id },
	/// Runner connection was lost (no recent ping, network issue, or crash)
	RunnerConnectionLost { runner_id: Id },
	/// Runner was draining but actor didn't stop in time
	RunnerDrainingTimeout { runner_id: Id },
	/// Actor exited with an error and is now sleeping
	Crashed { message: Option<String> },
	/// Actor has an internal error
	InternalError,

	/// Actor cannot allocate due to the concurrent actor limit.
	ConcurrentActorLimitReached,
	/// No envoys available matching the pool name
	NoEnvoys,
	/// Envoy was allocated but never started the actor (GC timeout)
	EnvoyNoResponse { envoy_key: Option<String> },
	/// Envoy connection was lost (no recent ping, network issue, or crash)
	EnvoyConnectionLost { envoy_key: String },
}
