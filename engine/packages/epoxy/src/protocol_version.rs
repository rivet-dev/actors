//! Per-peer epoxy protocol version discovery.
//!
//! Every other runtime protocol negotiates a fleet-wide version through the datacenter's UniversalDB
//! heartbeat subspace. Epoxy cannot: its peers are replicas in *other* datacenters, which write to
//! their own databases, so a peer's version is never in our subspace. Instead each replica publishes
//! the version its own datacenter agreed on over HTTP, and we take the minimum of that and ours.
//!
//! The `epoxy_protocol_version` core service owns the probing. [`negotiate`] runs on every
//! cross-datacenter request and only reads what the service has already discovered, so a probe round
//! trip never lands inside a consensus request's timeout budget.

use std::{sync::LazyLock, time::Duration};

use epoxy_protocol::protocol::{self, ReplicaId};
use futures_util::stream::{FuturesUnordered, StreamExt};
use gas::prelude::*;
use serde::{Deserialize, Serialize};

use crate::utils;

/// How often every replica in the topology is re-probed.
///
/// A datacenter only moves its version once every process heartbeating the old one is gone, so this
/// does not need to be tight. It only has to be short enough that a peer which finished rolling out
/// is picked up in reasonable time.
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize)]
pub struct ProtocolVersionResponse {
	pub protocol_version: u16,
}

/// Versions discovered by the service, keyed by replica URL.
///
/// This is process state rather than a field on a long-lived struct because the epoxy HTTP client is
/// a set of free functions over a per-request `ApiCtx`, so there is no per-process owner to hang it
/// on. It holds one small entry per replica in the cluster topology.
static PEER_VERSIONS: LazyLock<scc::HashMap<String, u16>> = LazyLock::new(scc::HashMap::new);

/// The version to encode a request to `replica_url` at.
///
/// Returns the lower of our own negotiated version and the peer's most recently discovered version.
/// This performs no I/O.
///
/// A replica the service has not reached yet falls back to our own negotiated version, which
/// reproduces the behavior from before peer discovery existed rather than downgrading whenever a
/// peer is briefly unreachable.
pub async fn negotiate(config: &rivet_config::Config, replica_url: &str) -> u16 {
	let local_version = config.protocols().epoxy.version();

	match PEER_VERSIONS
		.read_async(replica_url, |_, version| *version)
		.await
	{
		Some(peer_version) => local_version.min(peer_version),
		None => local_version,
	}
}

/// Core service that keeps [`PEER_VERSIONS`] current for every replica in the cluster topology.
#[tracing::instrument(skip_all)]
pub async fn start(config: rivet_config::Config, pools: rivet_pools::Pools) -> Result<()> {
	let mut refresh_interval = tokio::time::interval(REFRESH_INTERVAL);
	refresh_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

	loop {
		refresh_interval.tick().await;

		let (replica_id, cluster_config) = match read_cluster_config(&config, &pools).await {
			Ok(res) => res,
			// A replica the coordinator has not configured yet has no topology to probe. That is a
			// normal startup state, not a failure, so it stays out of the warning stream.
			Err(err) => {
				tracing::debug!(
					?err,
					"epoxy cluster config not readable, skipping probe pass"
				);
				continue;
			}
		};

		refresh(replica_id, &cluster_config).await;
	}
}

async fn read_cluster_config(
	config: &rivet_config::Config,
	pools: &rivet_pools::Pools,
) -> Result<(ReplicaId, protocol::ClusterConfig)> {
	let replica_id = config.epoxy_replica_id();
	let cluster_config = pools
		.udb()?
		.txn(
			"epoxy_read_cluster_config_for_protocol_version",
			|tx| async move { utils::read_config(&tx, replica_id).await },
		)
		.await?;

	Ok((replica_id, cluster_config))
}

#[tracing::instrument(skip_all)]
async fn refresh(replica_id: ReplicaId, cluster_config: &protocol::ClusterConfig) {
	let peer_urls = cluster_config
		.replicas
		.iter()
		.filter(|replica| replica.replica_id != replica_id)
		.map(|replica| replica.api_peer_url.clone())
		.collect::<Vec<_>>();

	// Drop replicas that left the topology so the map cannot grow without bound.
	PEER_VERSIONS
		.retain_async(|url, _| peer_urls.iter().any(|peer_url| peer_url == url))
		.await;

	let mut probes = peer_urls
		.into_iter()
		.map(|url| async move {
			let result = probe(&url).await;
			(url, result)
		})
		.collect::<FuturesUnordered<_>>();

	while let Some((url, result)) = probes.next().await {
		match result {
			Ok(version) => {
				PEER_VERSIONS.upsert_async(url, version).await;
			}
			// A probe failure must not change which version we speak. The peer answered at its
			// stored version recently and a datacenter never moves its version backwards, so
			// leaving the entry alone keeps sending at a version the peer still accepts.
			Err(err) => {
				tracing::warn!(replica_url = %url, ?err, "epoxy peer protocol version probe failed");
			}
		}
	}
}

#[tracing::instrument(skip_all, fields(%replica_url))]
async fn probe(replica_url: &str) -> Result<u16> {
	let mut url = url::Url::parse(replica_url)?;
	url.set_path("/epoxy/protocol-version");

	let client = rivet_pools::reqwest::client().await?;
	let response = tokio::time::timeout(PROBE_TIMEOUT, client.get(url.to_string()).send())
		.await
		.context("epoxy peer protocol version probe timed out")??
		.error_for_status()?
		.json::<ProtocolVersionResponse>()
		.await?;

	Ok(response.protocol_version)
}
