use anyhow::*;
use base64::Engine;
use epoxy::ops::propose::{Command, CommandKind, Proposal, SetCommand};
use epoxy_protocol::protocol::ReplicaId;
use futures_util::TryStreamExt;
use gas::prelude::*;
use indexmap::IndexMap;
use rivet_api_builder::ApiCtx;
use rivet_profiling::pubsub_subjects::{ProfileConfigSubject, SetProfileConfigMessage};
use rivet_tracing_reconfigure::pubsub_subjects::TracingConfigSubject;
use serde::{Deserialize, Serialize};
use universaldb::{
	RangeOption,
	options::StreamingMode,
	utils::{FormalKey, IsolationLevel::Serializable, keys::CHANGELOG},
};
use universalpubsub::PublishOpts;

#[derive(Serialize, Deserialize)]
pub struct CachePurgeRequest {
	pub base_key: String,
	pub keys: Vec<rivet_cache::RawCacheKey>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachePurgeResponse {}

pub async fn cache_purge(
	ctx: ApiCtx,
	_path: (),
	_query: (),
	body: CachePurgeRequest,
) -> Result<CachePurgeResponse> {
	ctx.cache()
		.clone()
		.request()
		.purge(&body.base_key, body.keys)
		.await?;

	Ok(CachePurgeResponse {})
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTracingConfigRequest {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub filter: Option<Option<String>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub sampler_ratio: Option<Option<f64>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTracingConfigResponse {}

#[tracing::instrument(skip_all)]
pub async fn set_tracing_config(
	ctx: ApiCtx,
	_path: (),
	_query: (),
	body: SetTracingConfigRequest,
) -> Result<SetTracingConfigResponse> {
	// Broadcast message to all services via UPS
	let message = rivet_util::serde::json_to_vec!(&body)?;

	ctx.ups()?
		.publish(TracingConfigSubject, &message, PublishOpts::broadcast())
		.await?;

	tracing::info!(
		filter = ?body.filter,
		sampler_ratio = ?body.sampler_ratio,
		"broadcasted tracing config update"
	);

	Ok(SetTracingConfigResponse {})
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetProfileConfigRequest {
	pub enabled: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub sample_rate: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetProfileConfigResponse {}

#[tracing::instrument(skip_all)]
pub async fn set_profiling_config(
	ctx: ApiCtx,
	_path: (),
	_query: (),
	body: SetProfileConfigRequest,
) -> Result<SetProfileConfigResponse> {
	// Broadcast message to all services via UPS
	let message = serde_json::to_vec(&SetProfileConfigMessage {
		enabled: body.enabled,
		sample_rate: body.sample_rate,
	})?;

	ctx.ups()?
		.publish(ProfileConfigSubject, &message, PublishOpts::broadcast())
		.await?;

	tracing::info!(
		enabled = body.enabled,
		sample_rate = ?body.sample_rate,
		"broadcasted profile config update"
	);

	Ok(SetProfileConfigResponse {})
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaReconfigureRequest {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaReconfigureResponse {}

/// Triggers the epoxy coordinator to reconfigure all replicas.
///
/// Useful when a replica's configuration is outdated for any reason and needs to be re-notified of
/// changes.
///
/// This should never need to be called manually if everything is operating correctly.
pub async fn epoxy_replica_reconfigure(
	ctx: ApiCtx,
	_path: (),
	_query: (),
	_body: ReplicaReconfigureRequest,
) -> Result<ReplicaReconfigureResponse> {
	if ctx.config().is_leader() {
		ctx.signal(epoxy::workflows::coordinator::ReplicaReconfigure {})
			.to_workflow::<epoxy::workflows::coordinator::Workflow>()
			.tag("replica", ctx.config().epoxy_replica_id())
			.send()
			.await?;
	}

	Ok(ReplicaReconfigureResponse {})
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetEpoxyStateResponse {
	pub config: epoxy::types::ClusterConfig,
}

/// Returns the current epoxy coordinator cluster configuration.
///
/// Useful for inspecting the current state of the epoxy cluster, including all replicas and their statuses.
pub async fn get_epoxy_state(ctx: ApiCtx, _path: (), _query: ()) -> Result<GetEpoxyStateResponse> {
	let workflow_id = ctx
		.find_workflow::<epoxy::workflows::coordinator::Workflow>((
			"replica",
			ctx.config().epoxy_replica_id(),
		))
		.await?
		.ok_or_else(|| anyhow!("epoxy coordinator workflow not found"))?;

	let wfs = ctx.get_workflows(vec![workflow_id]).await?;
	let wf = wfs.first().ok_or_else(|| anyhow!("workflow not found"))?;

	let state: epoxy::workflows::coordinator::State =
		wf.parse_state().context("failed to parse workflow state")?;

	Ok(GetEpoxyStateResponse {
		config: state.config,
	})
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetEpoxyStateRequest {
	pub config: epoxy::types::ClusterConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetEpoxyStateResponse {}

/// Overrides the epoxy coordinator cluster configuration and triggers reconfiguration.
///
/// Useful for manually adjusting the cluster state in case the replica status drifts from the
/// state in the coordinator. This will automatically trigger a reconfigure when called.
///
/// This should never need to be called manually if everything is operating correctly.
pub async fn set_epoxy_state(
	ctx: ApiCtx,
	_path: (),
	_query: (),
	body: SetEpoxyStateRequest,
) -> Result<SetEpoxyStateResponse> {
	ensure!(
		body.config.coordinator_replica_id == ctx.config().epoxy_replica_id(),
		"config coordinator_replica_id ({}) does not match current replica id ({})",
		body.config.coordinator_replica_id,
		ctx.config().epoxy_replica_id()
	);

	if ctx.config().is_leader() {
		ctx.signal(epoxy::workflows::coordinator::OverrideState {
			config: body.config,
		})
		.to_workflow::<epoxy::workflows::coordinator::Workflow>()
		.tag("replica", ctx.config().epoxy_replica_id())
		.send()
		.await?;
	}

	Ok(SetEpoxyStateResponse {})
}

#[derive(Serialize, Deserialize)]
pub struct GetEpoxyReplicaDebugResponse {
	pub config: epoxy::types::ClusterConfig,
	pub computed: EpoxyReplicaDebugComputed,
}

#[derive(Serialize, Deserialize)]
pub struct EpoxyReplicaDebugComputed {
	pub this_replica_id: ReplicaId,
	pub this_replica_status: Option<epoxy::types::ReplicaStatus>,
	pub is_coordinator: bool,
	pub quorum_members: Vec<ReplicaId>,
	pub quorum_sizes: EpoxyQuorumSizes,
	pub replica_counts: EpoxyReplicaCounts,
}

#[derive(Serialize, Deserialize)]
pub struct EpoxyQuorumSizes {
	pub fast: usize,
	pub slow: usize,
	pub all: usize,
	pub any: usize,
}

#[derive(Serialize, Deserialize)]
pub struct EpoxyReplicaCounts {
	pub active: usize,
	pub learning: usize,
	pub joining: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyBallot {
	pub counter: u64,
	pub replica_id: ReplicaId,
}

/// Returns debug information for this epoxy replica including cluster config, computed values, and state.
///
/// This reads from the replica's local UDB state, which may differ from the coordinator's
/// view if the replica hasn't been reconfigured yet.
pub async fn get_epoxy_replica_debug(
	ctx: ApiCtx,
	_path: (),
	_query: (),
) -> Result<GetEpoxyReplicaDebugResponse> {
	let replica_id = ctx.config().epoxy_replica_id();

	let config = ctx
		.udb()?
		.txn("api_peer_read_epoxy_config", |tx| async move {
			let config = epoxy::utils::read_config(&tx, replica_id).await?;
			Result::Ok(config)
		})
		.await?;

	// Compute derived values from config
	let this_replica_status = config
		.replicas
		.iter()
		.find(|r| r.replica_id == replica_id)
		.map(|r| epoxy::types::ReplicaStatus::from(r.status.clone()));
	let is_coordinator = config.coordinator_replica_id == replica_id;

	let quorum_members = epoxy::utils::get_quorum_members(&config);
	let quorum_member_count = quorum_members.len();

	let quorum_sizes = EpoxyQuorumSizes {
		fast: epoxy::utils::calculate_quorum(quorum_member_count, epoxy::utils::QuorumType::Fast),
		slow: epoxy::utils::calculate_quorum(quorum_member_count, epoxy::utils::QuorumType::Slow),
		all: epoxy::utils::calculate_quorum(quorum_member_count, epoxy::utils::QuorumType::All),
		any: epoxy::utils::calculate_quorum(quorum_member_count, epoxy::utils::QuorumType::Any),
	};

	let replica_counts = EpoxyReplicaCounts {
		active: config
			.replicas
			.iter()
			.filter(|r| matches!(r.status, epoxy_protocol::protocol::ReplicaStatus::Active))
			.count(),
		learning: config
			.replicas
			.iter()
			.filter(|r| matches!(r.status, epoxy_protocol::protocol::ReplicaStatus::Learning))
			.count(),
		joining: config
			.replicas
			.iter()
			.filter(|r| matches!(r.status, epoxy_protocol::protocol::ReplicaStatus::Joining))
			.count(),
	};

	Ok(GetEpoxyReplicaDebugResponse {
		config: config.into(),
		computed: EpoxyReplicaDebugComputed {
			this_replica_id: replica_id,
			this_replica_status,
			is_coordinator,
			quorum_members,
			quorum_sizes,
			replica_counts,
		},
	})
}

// MARK: Key debug
#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyKeyDebugPath {
	pub key: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetEpoxyKeyDebugResponse {
	pub replica_id: ReplicaId,
	pub key: String,
	pub state: EpoxyKeyState,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyKeyState {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub current_ballot: Option<EpoxyBallot>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub accepted: Option<EpoxyAcceptedValue>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub committed: Option<EpoxyCommittedValue>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub optimistic_cache: Option<EpoxyCachedValue>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub legacy_committed: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub changelog: Vec<EpoxyChangelogEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyCommittedValue {
	/// Base64-encoded value bytes.
	pub value: Option<String>,
	pub version: u64,
	pub mutable: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyAcceptedValue {
	/// Base64-encoded value bytes.
	pub value: Option<String>,
	pub ballot: EpoxyBallot,
	pub version: u64,
	pub mutable: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyCachedValue {
	/// Base64-encoded value bytes.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub value: Option<String>,
	pub version: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct EpoxyChangelogEntry {
	/// Base64-encoded value bytes.
	pub value: Option<String>,
	/// Base64-encoded raw versionstamp bytes.
	pub versionstamp: String,
	pub version: u64,
	pub mutable: bool,
}

/// Returns debug information for a specific key on this replica.
///
/// V2 stores per-key Fast Paxos state rather than an EPaxos-style instance log, so this route
/// exposes the concrete key state that actually exists in UDB.
pub async fn get_epoxy_key_debug(
	ctx: ApiCtx,
	path: EpoxyKeyDebugPath,
	_query: (),
) -> Result<GetEpoxyKeyDebugResponse> {
	let replica_id = ctx.config().epoxy_replica_id();

	let key_bytes = base64::engine::general_purpose::STANDARD
		.decode(&path.key)
		.context("invalid base64 key")?;
	let state = read_epoxy_key_state(ctx.clone(), replica_id, key_bytes).await?;

	Ok(GetEpoxyKeyDebugResponse {
		replica_id,
		key: path.key,
		state,
	})
}

#[derive(Serialize, Deserialize)]
pub struct GetEpoxyKeyDebugFanoutResponse {
	pub replicas: IndexMap<ReplicaId, GetEpoxyKeyDebugResponse>,
	pub errors: IndexMap<ReplicaId, String>,
}

/// Returns debug information for a specific key across all replicas in the cluster.
///
/// Fans out to all replicas and aggregates their responses.
pub async fn get_epoxy_key_debug_fanout(
	ctx: ApiCtx,
	path: EpoxyKeyDebugPath,
	_query: (),
) -> Result<GetEpoxyKeyDebugFanoutResponse> {
	let replica_id = ctx.config().epoxy_replica_id();

	// Get cluster config to find all replicas
	let config: epoxy::types::ClusterConfig = ctx
		.udb()?
		.txn("api_peer_read_epoxy_cluster_config", |tx| async move {
			let config = epoxy::utils::read_config(&tx, replica_id).await?;
			Result::Ok(config.into())
		})
		.await?;

	// Get local response first
	let local_response = get_epoxy_key_debug(ctx.clone(), path.clone(), ()).await?;

	let mut replicas = IndexMap::new();
	let mut errors = IndexMap::new();

	// Add local response
	replicas.insert(replica_id, local_response);

	// Fan out to other replicas
	let client = rivet_pools::reqwest::client().await?;

	for replica_config in &config.replicas {
		if replica_config.replica_id == replica_id {
			continue;
		}

		let url = format!(
			"{}/epoxy/replica/key/{}",
			replica_config.api_peer_url, path.key
		);

		let response_result = client.get(&url).send().await;
		match response_result {
			std::result::Result::Ok(response) => {
				if response.status().is_success() {
					match response.json::<GetEpoxyKeyDebugResponse>().await {
						std::result::Result::Ok(resp) => {
							replicas.insert(replica_config.replica_id, resp);
						}
						std::result::Result::Err(e) => {
							errors.insert(
								replica_config.replica_id,
								format!("failed to parse response: {}", e),
							);
						}
					}
				} else {
					errors.insert(
						replica_config.replica_id,
						format!("request failed with status: {}", response.status()),
					);
				}
			}
			std::result::Result::Err(e) => {
				errors.insert(replica_config.replica_id, format!("request failed: {}", e));
			}
		}
	}

	Ok(GetEpoxyKeyDebugFanoutResponse { replicas, errors })
}

fn debug_ballot(ballot: epoxy_protocol::protocol::Ballot) -> EpoxyBallot {
	EpoxyBallot {
		counter: ballot.counter,
		replica_id: ballot.replica_id,
	}
}

async fn read_epoxy_key_state(
	ctx: ApiCtx,
	replica_id: ReplicaId,
	key: Vec<u8>,
) -> Result<EpoxyKeyState> {
	ctx.udb()?
		.txn("api_peer_read_epoxy_key_state", move |tx| {
			let key = key.clone();
			async move {
				let replica_subspace = epoxy::keys::subspace(replica_id);
				let legacy_subspace = epoxy::keys::legacy_subspace(replica_id);

				let value_key = epoxy::keys::KvValueKey::new(key.clone());
				let ballot_key = epoxy::keys::KvBallotKey::new(key.clone());
				let accepted_key = epoxy::keys::KvAcceptedKey::new(key.clone());
				let accepted2_key = epoxy::keys::KvAccepted2Key::new(key.clone());
				let cache_key = epoxy::keys::KvOptimisticCacheKey::new(key.clone());
				let legacy_value_key = epoxy::keys::LegacyCommittedValueKey::new(key.clone());

				let v2_tx = tx.with_subspace(replica_subspace.clone());
				let legacy_tx = tx.with_subspace(legacy_subspace);

				let (
					committed,
					current_ballot,
					accepted,
					accepted2,
					optimistic_cache,
					legacy_committed,
				) = tokio::try_join!(
					v2_tx.read_opt(&value_key, Serializable),
					v2_tx.read_opt(&ballot_key, Serializable),
					v2_tx.read_opt(&accepted_key, Serializable),
					v2_tx.read_opt(&accepted2_key, Serializable),
					v2_tx.read_opt(&cache_key, Serializable),
					legacy_tx.read_opt(&legacy_value_key, Serializable),
				)?;

				let changelog_subspace = replica_subspace.subspace(&(CHANGELOG,));
				let mut range: RangeOption<'static> = (&changelog_subspace).into();
				range.mode = StreamingMode::WantAll;

				let mut changelog = Vec::new();
				let mut stream = tx.get_ranges_keyvalues(range, Serializable);
				while let Some(entry) = stream.try_next().await? {
					let changelog_key =
						replica_subspace.unpack::<epoxy::keys::ChangelogKey>(entry.key())?;
					let changelog_entry = changelog_key.deserialize(entry.value())?;

					if changelog_entry.key != key {
						continue;
					}

					changelog.push(EpoxyChangelogEntry {
						value: changelog_entry
							.value
							.as_ref()
							.map(|x| base64::engine::general_purpose::STANDARD.encode(x)),
						versionstamp: base64::engine::general_purpose::STANDARD
							.encode(changelog_key.versionstamp().as_bytes()),
						version: changelog_entry.version,
						mutable: changelog_entry.mutable,
					});
				}

				Ok(EpoxyKeyState {
					current_ballot: current_ballot.map(debug_ballot),
					accepted: accepted2
						.map(|accepted| EpoxyAcceptedValue {
							value: accepted
								.value
								.as_ref()
								.map(|x| base64::engine::general_purpose::STANDARD.encode(x)),
							ballot: debug_ballot(accepted.ballot),
							version: accepted.version,
							mutable: accepted.mutable,
						})
						.or_else(|| {
							accepted.map(|accepted| EpoxyAcceptedValue {
								value: Some(
									base64::engine::general_purpose::STANDARD
										.encode(accepted.value),
								),
								ballot: debug_ballot(accepted.ballot),
								version: accepted.version,
								mutable: accepted.mutable,
							})
						}),
					committed: committed.map(|value| EpoxyCommittedValue {
						value: value
							.value
							.as_ref()
							.map(|x| base64::engine::general_purpose::STANDARD.encode(x)),
						version: value.version,
						mutable: value.mutable,
					}),
					optimistic_cache: optimistic_cache.map(|value| EpoxyCachedValue {
						value: value
							.value
							.flatten()
							.map(|value| base64::engine::general_purpose::STANDARD.encode(&value)),
						version: value.version,
					}),
					legacy_committed: legacy_committed
						.map(|value| base64::engine::general_purpose::STANDARD.encode(&value)),
					changelog,
				})
			}
		})
		.await
}

// MARK: KV get/set
#[derive(Serialize, Deserialize)]
pub struct GetEpoxyKvResponse {
	pub exists: bool,
	/// Base64-encoded value bytes
	#[serde(skip_serializing_if = "Option::is_none")]
	pub value: Option<String>,
}

/// Gets a value from epoxy KV (local replica only). Key and value are base64-encoded.
pub async fn get_epoxy_kv_local(
	ctx: ApiCtx,
	path: EpoxyKeyDebugPath,
	_query: (),
) -> Result<GetEpoxyKvResponse> {
	use base64::Engine;

	let replica_id = ctx.config().epoxy_replica_id();

	let key_bytes = base64::engine::general_purpose::STANDARD
		.decode(&path.key)
		.context("invalid base64 key")?;

	let output = ctx
		.op(epoxy::ops::kv::get_local::Input {
			replica_id,
			key: key_bytes,
		})
		.await?;

	Ok(GetEpoxyKvResponse {
		exists: output.is_some(),
		value: output
			.and_then(|v| v.value)
			.map(|v| base64::engine::general_purpose::STANDARD.encode(&v)),
	})
}

/// Gets a value from epoxy KV with optimistic read (local + cache + fanout).
/// Key and value are base64-encoded.
pub async fn get_epoxy_kv_optimistic(
	ctx: ApiCtx,
	path: EpoxyKeyDebugPath,
	_query: (),
) -> Result<GetEpoxyKvResponse> {
	use base64::Engine;

	let replica_id = ctx.config().epoxy_replica_id();

	let key_bytes = base64::engine::general_purpose::STANDARD
		.decode(&path.key)
		.context("invalid base64 key")?;

	let output = ctx
		.op(epoxy::ops::kv::get_optimistic::Input {
			replica_id,
			key: key_bytes,
			caching_behavior: epoxy_protocol::protocol::CachingBehavior::Optimistic,
			target_replicas: None,
			save_empty: false,
		})
		.await?;

	Ok(GetEpoxyKvResponse {
		exists: output.value.is_some(),
		value: output
			.value
			.map(|v| base64::engine::general_purpose::STANDARD.encode(&v)),
	})
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetEpoxyKvRequest {
	/// Base64-encoded value bytes. If null, deletes the key.
	pub value: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SetEpoxyKvResponse {
	pub result: String,
}

/// Sets a value in epoxy KV through consensus. Key and value are base64-encoded.
pub async fn set_epoxy_kv(
	ctx: ApiCtx,
	path: EpoxyKeyDebugPath,
	_query: (),
	body: SetEpoxyKvRequest,
) -> Result<SetEpoxyKvResponse> {
	use base64::Engine;

	let key_bytes = base64::engine::general_purpose::STANDARD
		.decode(&path.key)
		.context("invalid base64 key")?;

	let value_bytes = body
		.value
		.map(|v| {
			base64::engine::general_purpose::STANDARD
				.decode(&v)
				.context("invalid base64 value")
		})
		.transpose()?;

	let value_bytes = value_bytes.ok_or_else(|| {
		anyhow!("epoxy v2 debug set only supports immutable set-if-absent writes")
	})?;

	let result = ctx
		.op(epoxy::ops::propose::Input {
			proposal: Proposal {
				commands: vec![Command {
					kind: CommandKind::SetCommand(SetCommand {
						key: key_bytes,
						value: Some(value_bytes),
					}),
				}],
			},
			mutable: false,
			purge_cache: true,
			target_replicas: None,
		})
		.await?;

	let result_str = match result {
		epoxy::ops::propose::ProposalResult::Committed => "committed".to_string(),
		epoxy::ops::propose::ProposalResult::ConsensusFailed { reason } => {
			format!("consensus_failed: {reason:?}")
		}
	};

	Ok(SetEpoxyKvResponse { result: result_str })
}
