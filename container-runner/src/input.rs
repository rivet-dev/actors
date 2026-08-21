//! The actor input payload and persisted state for the child game server.
//!
//! [`ActorInput`] carries the command, args, env, and port from the engine's
//! create-time `input` (CBOR per RivetKit); anything omitted falls back to the CLI
//! template (`rivet-container-runner -- <command...>`). [`ActorState`] is what
//! persists across sleep: the launch spec plus lifecycle bookkeeping.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Persisted actor state, restored on wake. The launch spec is flattened in so an
/// actor persisted before `started_once` existed (state was a bare [`ActorInput`])
/// still decodes.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActorState {
	#[serde(flatten)]
	pub input: ActorInput,
	/// Set once the actor has performed its real start. The reject-second-start guard
	/// self-sleeps a repeat start when this is already set. See `RIVET_REJECT_SECOND_START`.
	#[serde(default)]
	pub started_once: bool,
}

/// Shape of the actor `input` payload. Unknown fields are ignored, not rejected:
/// it nests in the persisted [`ActorState`], and a strict decode would break waking
/// actors after a rollback to a binary predating a new field.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActorInput {
	/// Overrides the CLI command template entirely (program + fixed args).
	#[serde(default)]
	pub command: Option<Vec<String>>,
	/// Extra args appended after the command template / `command`.
	#[serde(default)]
	pub args: Vec<String>,
	/// Extra environment variables for the child process.
	#[serde(default)]
	pub env: HashMap<String, String>,
	/// Local port the child listens on; also exported to the child as `PORT`.
	/// Falls back to the runner's `--child-port` when omitted.
	#[serde(default)]
	pub port: Option<u16>,
}

#[cfg(test)]
#[path = "../tests/inline/input.rs"]
mod tests;
