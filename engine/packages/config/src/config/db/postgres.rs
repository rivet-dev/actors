use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::secret::Secret;

use super::super::pubsub::Nats;

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PostgresSsl {
	/// Path to the root certificate file for verifying the server's certificate
	///
	/// Required when using custom certificate authorities (e.g., Supabase)
	/// Equivalent to PostgreSQL's `sslrootcert` parameter
	#[serde(default)]
	pub root_cert_path: Option<PathBuf>,

	/// Path to the client certificate file
	///
	/// Used for client certificate authentication
	/// Equivalent to PostgreSQL's `sslcert` parameter
	#[serde(default)]
	pub client_cert_path: Option<PathBuf>,

	/// Path to the client private key file
	///
	/// Used for client certificate authentication
	/// Equivalent to PostgreSQL's `sslkey` parameter
	#[serde(default)]
	pub client_key_path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Postgres {
	/// URL to connect to Postgres with
	///
	/// Supports standard PostgreSQL connection parameters including `sslmode`.
	/// Supported sslmode values: `disable`, `prefer` (default), `require`.
	/// To verify server certificates, use `sslmode=require` with `ssl.root_cert_path`.
	///
	/// Example with sslmode: `postgresql://user:pass@host:5432/db?sslmode=require`
	///
	/// See: https://docs.rs/postgres/0.19.10/postgres/config/struct.Config.html#url
	pub url: Secret<String>,

	/// SSL configuration options
	#[serde(default)]
	pub ssl: Option<PostgresSsl>,

	/// NATS configuration for UniversalDB multi-node mode.
	///
	/// When set, UniversalDB runs in multi-node mode and uses NATS for follower-to-leader commit
	/// transport instead of an in-process resolver. When absent, UniversalDB runs single-node.
	/// If unset but the UPS pubsub is configured for NATS, this is inherited from that config at
	/// startup (see `Root::validate_and_set_defaults`).
	#[serde(default)]
	pub nats: Option<Nats>,
}

impl Default for Postgres {
	fn default() -> Self {
		Self {
			url: Secret::new("postgresql://postgres:postgres@127.0.0.1:5432/postgres".into()),
			ssl: None,
			nats: None,
		}
	}
}
