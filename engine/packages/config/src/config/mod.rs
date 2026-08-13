use anyhow::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub mod api_peer;
pub mod auth;
pub mod cache;
pub mod clickhouse;
pub mod db;
pub mod guard;
pub mod logs;
pub mod metrics;
pub mod pegboard;
pub mod pubsub;
pub mod pyroscope;
pub mod runtime;
pub mod sqlite;
pub mod telemetry;
pub mod topology;

pub use api_peer::*;
pub use auth::*;
pub use cache::*;
pub use clickhouse::*;
pub use db::Database;
pub use guard::*;
pub use logs::*;
pub use metrics::*;
pub use pegboard::*;
pub use pubsub::PubSub;
pub use pyroscope::*;
pub use runtime::*;
pub use sqlite::*;
pub use telemetry::*;
pub use topology::*;

// IMPORTANT:
//
// Do not use Vec unless it is `Vec<String>`. Use a `HashMap` instead.
//
// This is because all values need to be able to be configured using environment variables.
// config-rs can only parse `Vec<String>` from the environment.
//
// IMPORTANT:
//
// Everything at the root should be `Option`.
//
// This is because we use the `config` crate. If we were to provide a default value to `config` of
// `Root::default` (without options), it'll try to merge the keys by key-value instead of
// intelligently "reassigning" structs.
//
// This means that a lot of properties like enums cannot be overridden.
//
// For example:
//
// Default config: { "memory": {} }
// User config: { "file_system": {} }
// Will merge to: { "memory: {}, "file_system": {} }
//
// This is because `Config` is not intelligent enough to know that `Database` is an enum.
//
// By using `Option`, we can manually implement our own default using methods + `LazyLock` (for
// performance).

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Root {
	#[serde(default)]
	pub auth: Option<Auth>,

	#[serde(default)]
	pub guard: Option<Guard>,

	#[serde(default)]
	pub api_peer: Option<ApiPeer>,

	#[serde(default)]
	pub pegboard: Option<Pegboard>,

	#[serde(default)]
	pub logs: Option<Logs>,

	#[serde(default)]
	pub topology: Option<Topology>,

	#[serde(default, flatten)]
	pub database: Option<Database>,

	#[serde(default, flatten)]
	pub pubsub: Option<PubSub>,

	#[serde(default)]
	pub cache: Option<Cache>,

	#[serde(default)]
	pub clickhouse: Option<ClickHouse>,

	#[serde(default)]
	pub telemetry: Telemetry,

	#[serde(default)]
	pub runtime: Runtime,

	#[serde(default)]
	pub sqlite: Option<Sqlite>,

	#[serde(default)]
	pub metrics: Metrics,

	#[serde(default)]
	pub pyroscope: Option<Pyroscope>,
}

impl Default for Root {
	fn default() -> Self {
		Root {
			auth: None,
			guard: None,
			api_peer: None,
			pegboard: None,
			logs: None,
			topology: None,
			database: None,
			pubsub: None,
			cache: None,
			clickhouse: None,
			telemetry: Default::default(),
			runtime: Default::default(),
			sqlite: None,
			metrics: Default::default(),
			pyroscope: None,
		}
	}
}

impl Root {
	pub fn guard(&self) -> &Guard {
		static DEFAULT: LazyLock<Guard> = LazyLock::new(Guard::default);
		self.guard.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn api_peer(&self) -> &ApiPeer {
		static DEFAULT: LazyLock<ApiPeer> = LazyLock::new(ApiPeer::default);
		self.api_peer.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn pegboard(&self) -> &Pegboard {
		static DEFAULT: LazyLock<Pegboard> = LazyLock::new(Pegboard::default);
		self.pegboard.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn logs(&self) -> &Logs {
		static DEFAULT: LazyLock<Logs> = LazyLock::new(Logs::default);
		self.logs.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn topology(&self) -> &Topology {
		static DEFAULT: LazyLock<Topology> = LazyLock::new(Topology::default);
		self.topology.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn database(&self) -> &Database {
		static DEFAULT: LazyLock<Database> = LazyLock::new(Database::default);
		self.database.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn pubsub(&self) -> &PubSub {
		static DEFAULT: LazyLock<PubSub> = LazyLock::new(PubSub::default);
		self.pubsub.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn sqlite(&self) -> &Sqlite {
		static DEFAULT: LazyLock<Sqlite> = LazyLock::new(Sqlite::default);
		self.sqlite.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn cache(&self) -> &Cache {
		static DEFAULT: LazyLock<Cache> = LazyLock::new(Cache::default);
		self.cache.as_ref().unwrap_or(&DEFAULT)
	}

	pub fn clickhouse(&self) -> Option<&ClickHouse> {
		self.clickhouse.as_ref()
	}

	pub fn validate_and_set_defaults(&mut self) -> Result<()> {
		// When UDB runs on Postgres without an explicit NATS config, inherit the UPS NATS config if
		// one is set. The presence of NATS is what selects UDB multi-node mode, so this lets a
		// single `pubsub: nats` config drive both UPS and UDB across nodes.
		if let Some(PubSub::Nats(nats)) = self.pubsub.clone()
			&& let Some(Database::Postgres(pg)) = &mut self.database
			&& pg.nats.is_none()
		{
			pg.nats = Some(nats);
		}

		self.pegboard().validate()?;

		// Validate that all datacenters have valid_hosts configured when there's more than one datacenter
		let topology = self.topology();
		if topology.datacenters.len() > 1 {
			for dc in &topology.datacenters {
				if dc.valid_hosts.is_none() {
					bail!(
						"datacenter '{}' must have valid_hosts configured when there is more than one datacenter",
						dc.name
					);
				}
			}
		}

		// Set name property for map topology
		if let Some(topology) = &mut self.topology {
			match &mut topology.datacenters {
				DatacentersRepr::Map(m) => {
					for (name, dc) in m {
						if !dc.name.is_empty() {
							bail!(
								"datacenter '{}' cannot have the `name` property set because it is automatically derived from key",
								dc.name
							);
						}

						dc.name = name.clone();
					}
				}
				DatacentersRepr::List(l) => {
					for (i, dc) in l.iter().enumerate() {
						if dc.name.is_empty() {
							bail!("datacenter at index {} must have a name", i);
						}
					}
				}
			}
		}

		// Validate force_shutdown_duration covers worker and guard shutdown durations
		let worker = self.runtime.worker_shutdown_duration();
		let guard = self.runtime.guard_shutdown_duration();
		let force = self.runtime.force_shutdown_duration();
		let max_graceful = worker.max(guard);
		if force < max_graceful {
			bail!(
				"force_shutdown_duration ({force:?}) must be greater than or equal to both \
				worker_shutdown_duration ({worker:?}) and guard_shutdown_duration ({guard:?})"
			);
		}

		Ok(())
	}

	/// Alias of `dc_label`. This is for convenience & clarify when reading code.
	pub fn epoxy_replica_id(&self) -> u64 {
		self.dc_label() as u64
	}

	/// Current datacenter's label.
	pub fn dc_label(&self) -> u16 {
		self.topology().datacenter_label
	}

	pub fn dc_name(&self) -> Result<&str> {
		Ok(&self.topology().current_dc()?.name)
	}

	pub fn dc_for_label(&self, label: u16) -> Option<&topology::Datacenter> {
		self.topology().dc_for_label(label)
	}

	pub fn dc_for_name(&self, name: &str) -> Option<&topology::Datacenter> {
		self.topology().dc_for_name(name)
	}

	pub fn leader_dc(&self) -> Result<&topology::Datacenter> {
		self.topology().leader_dc()
	}

	pub fn is_leader(&self) -> bool {
		self.topology().is_leader()
	}
}
