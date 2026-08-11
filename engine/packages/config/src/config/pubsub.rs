use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::secret::Secret;

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PubSub {
	Nats(Nats),
	Memory(Memory),
}

impl Default for PubSub {
	fn default() -> Self {
		PubSub::Memory(Memory::default())
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Nats {
	pub addresses: Vec<String>,
	pub port: Option<u16>,
	/// Capacity of the async-nats client command queue.
	///
	/// When this fills, client operations such as subscribe, publish, and drain wait until the
	/// connection task catches up.
	#[serde(default = "Nats::default_client_capacity")]
	pub client_capacity: usize,
	/// Capacity of each individual NATS subscriber message buffer.
	///
	/// When this fills, async-nats drops the message and emits `SlowConsumer`. Rivet logs this as
	/// `nats slow consumer`.
	#[serde(default = "Nats::default_subscription_capacity")]
	pub subscription_capacity: usize,
	#[serde(default)]
	pub username: Option<String>,
	#[serde(default)]
	pub password: Option<Secret<String>>,
	/// When true, force every UPS publish to round-trip through NATS instead of taking the
	/// in-process fast path for subjects that have a local subscriber on the same engine pod.
	/// Opt-in diagnostic; default false.
	#[serde(default)]
	pub disable_memory_optimization: bool,
}

impl Default for Nats {
	fn default() -> Self {
		Self {
			addresses: vec!["127.0.0.1:4222".to_string()],
			port: None,
			client_capacity: Self::default_client_capacity(),
			subscription_capacity: Self::default_subscription_capacity(),
			username: None,
			password: None,
			disable_memory_optimization: false,
		}
	}
}

impl Nats {
	pub fn port(&self) -> u16 {
		self.port.unwrap_or(4222)
	}

	fn default_client_capacity() -> usize {
		// Keep this large because subscription churn can bottleneck async-nats' client command queue.
		1_048_576
	}

	fn default_subscription_capacity() -> usize {
		262_144
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Memory {
	#[serde(default = "Memory::default_channel")]
	pub channel: String,
	/// When true, force every UPS publish to round-trip through the memory driver instead of
	/// taking the in-process fast path for subjects that have a local subscriber on the same
	/// engine pod. Opt-in diagnostic; default false.
	#[serde(default)]
	pub disable_memory_optimization: bool,
}

impl Default for Memory {
	fn default() -> Self {
		Self {
			channel: Self::default_channel(),
			disable_memory_optimization: false,
		}
	}
}

impl Memory {
	fn default_channel() -> String {
		"default".to_string()
	}
}
