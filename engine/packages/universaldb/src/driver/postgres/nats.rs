use std::str::FromStr;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use super::{
	codec,
	transport::{CommitJob, DedupKey, Responder},
};

/// NATS connection settings for UniversalDB multi-node mode. Built from the resolved
/// `database.postgres.nats` config (which may be inherited from the UPS NATS config).
#[derive(Clone, Debug)]
pub struct NatsConfig {
	/// `host:port` server addresses.
	pub addresses: Vec<String>,
	pub username: Option<String>,
	pub password: Option<String>,
	pub client_capacity: usize,
	pub subscription_capacity: usize,
}

/// The multi-node transport handle: the NATS client plus the cluster-scoped subject names.
pub struct NatsTransport {
	pub client: async_nats::Client,
	pub subjects: Subjects,
}

/// Cluster-scoped UniversalDB NATS subjects. The cluster prefix is derived from the Postgres
/// connection string so two separate clusters that happen to share one NATS deployment do not
/// cross-deliver watermark/election broadcasts or commits.
#[derive(Clone)]
pub struct Subjects {
	prefix: String,
}

impl Subjects {
	pub fn new(connection_string: &str) -> Self {
		Subjects {
			prefix: format!("udb.{:016x}", fnv1a_64(connection_string.as_bytes())),
		}
	}

	/// Subject a follower sends a commit request to, and the elected leader subscribes to. Namespaced
	/// by the leader's node id so only the current leader receives commits.
	pub fn commit(&self, leader_id: &str) -> String {
		format!("{}.commit.{leader_id}", self.prefix)
	}

	/// Subject the leader publishes each watermark advance to; every node subscribes.
	pub fn watermark(&self) -> String {
		format!("{}.watermark", self.prefix)
	}

	/// Subject a departing leader publishes to so standby candidates elect immediately.
	pub fn election(&self) -> String {
		format!("{}.election", self.prefix)
	}
}

/// Connect a NATS client for the UniversalDB multi-node transport.
pub async fn connect(config: &NatsConfig) -> Result<async_nats::Client> {
	let server_addrs = config
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(&url))
		.collect::<Result<Vec<_>, _>>()
		.context("failed to parse udb nats addresses")?;

	let mut options = match (&config.username, &config.password) {
		(Some(username), Some(password)) => {
			async_nats::ConnectOptions::with_user_and_password(username.clone(), password.clone())
		}
		_ => async_nats::ConnectOptions::new(),
	};
	options = options
		.client_capacity(config.client_capacity)
		.subscription_capacity(config.subscription_capacity);

	options
		.connect(&server_addrs[..])
		.await
		.context("failed to connect udb nats client")
}

/// Leader-side task: subscribe to this leader's commit subject, decode each request into a
/// [`CommitJob`], and forward it into the drain loop's job queue. Returns when the subscription ends
/// (client closed) or the drain loop's receiver is dropped (step-down).
pub async fn run_commit_subscriber(
	client: async_nats::Client,
	subject: String,
	jobs_tx: mpsc::Sender<CommitJob>,
) -> Result<()> {
	let mut sub = client
		.subscribe(subject.clone())
		.await
		.with_context(|| format!("failed to subscribe to udb commit subject {subject}"))?;

	while let Some(msg) = sub.next().await {
		let Some(reply) = msg.reply.clone() else {
			tracing::warn!("udb commit request missing reply subject; dropping");
			continue;
		};

		let decoded = match codec::decode_commit_request(&msg.payload) {
			Ok(decoded) => decoded,
			Err(err) => {
				tracing::warn!(?err, "failed to decode udb commit request; dropping");
				continue;
			}
		};

		let job = CommitJob {
			read_version: decoded.read_version,
			conflict_ranges: decoded.conflict_ranges,
			operations: decoded.operations,
			dedup_key: Some(DedupKey {
				client_node_id: decoded.client_node_id,
				client_seq: decoded.client_seq as i64,
			}),
			responder: Responder::Nats {
				client: client.clone(),
				reply,
			},
		};

		// A full queue applies backpressure; a closed queue means the drain loop stepped down.
		if jobs_tx.send(job).await.is_err() {
			break;
		}
	}

	Ok(())
}

/// FNV-1a 64-bit hash. Deterministic across processes (unlike `DefaultHasher`), used only to derive a
/// stable cluster subject prefix.
fn fnv1a_64(bytes: &[u8]) -> u64 {
	const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
	const PRIME: u64 = 0x0000_0100_0000_01b3;
	let mut hash = OFFSET;
	for &b in bytes {
		hash ^= b as u64;
		hash = hash.wrapping_mul(PRIME);
	}
	hash
}
