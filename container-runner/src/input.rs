//! The actor input payload describing how to launch the child game server.
//!
//! The command, args, env, and port are carried in the actor's create-time `input`
//! (CBOR per RivetKit); anything omitted falls back to the CLI template
//! (`rivet-container-runner -- <command...>`). This is also the actor's persisted
//! state, so a woken actor restores the same launch spec.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Shape of the actor `input` payload. Unknown fields are ignored, not rejected:
/// this is also the persisted state, and a strict decode would break waking actors
/// after a rollback to a binary predating a new field.
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
