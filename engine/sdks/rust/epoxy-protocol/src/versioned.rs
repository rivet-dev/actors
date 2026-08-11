use anyhow::{Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::{v2, v3};

pub enum CommittedValue {
	V2(v2::CommittedValue),
	V3(v3::CommittedValue),
}

impl OwnedVersionedData for CommittedValue {
	type Latest = v3::CommittedValue;

	fn wrap_latest(latest: v3::CommittedValue) -> Self {
		CommittedValue::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let CommittedValue::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			2 => Ok(CommittedValue::V2(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			3 => Ok(CommittedValue::V3(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			CommittedValue::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			CommittedValue::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Ok]
	}
}

impl CommittedValue {
	fn v2_to_v3(self) -> Result<Self> {
		if let CommittedValue::V2(x) = self {
			Ok(CommittedValue::V3(v3::CommittedValue {
				value: Some(x.value),
				version: x.version,
				mutable: x.mutable,
			}))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		bail!("cannot downgrade committed value from v3 to v2");
	}
}

pub enum CachedValue {
	V2(v2::CachedValue),
	V3(v3::CachedValue),
}

impl OwnedVersionedData for CachedValue {
	type Latest = v3::CachedValue;

	fn wrap_latest(latest: v3::CachedValue) -> Self {
		CachedValue::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let CachedValue::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			2 => Ok(CachedValue::V2(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			3 => Ok(CachedValue::V3(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			CachedValue::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			CachedValue::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Ok]
	}
}

impl CachedValue {
	fn v2_to_v3(self) -> Result<Self> {
		if let CachedValue::V2(x) = self {
			Ok(CachedValue::V3(v3::CachedValue {
				// v2 None (not found) maps to v3 None (not found).
				// v2 Some(bytes) (found) maps to v3 Some(Some(bytes)) (value set).
				value: x.value.map(Some),
				version: x.version,
			}))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		bail!("cannot downgrade cached epoxy from v3 to v2");
	}
}

pub enum AcceptedValue {
	V2(v2::AcceptedValue),
	V3(v3::AcceptedValue),
}

impl OwnedVersionedData for AcceptedValue {
	type Latest = v3::AcceptedValue;

	fn wrap_latest(latest: v3::AcceptedValue) -> Self {
		AcceptedValue::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let AcceptedValue::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			2 => Ok(AcceptedValue::V2(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			3 => Ok(AcceptedValue::V3(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			AcceptedValue::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			AcceptedValue::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Ok]
	}
}

impl AcceptedValue {
	fn v2_to_v3(self) -> Result<Self> {
		if let AcceptedValue::V2(x) = self {
			Ok(AcceptedValue::V3(v3::AcceptedValue {
				value: Some(x.value),
				ballot: convert_ballot_v2_to_v3(x.ballot),
				version: x.version,
				mutable: x.mutable,
			}))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		bail!("cannot downgrade accepted value from v3 to v2");
	}
}

pub enum Request {
	V2(v2::Request),
	V3(v3::Request),
}

impl OwnedVersionedData for Request {
	type Latest = v3::Request;

	fn wrap_latest(latest: v3::Request) -> Self {
		Request::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let Request::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			2 => Ok(Request::V2(rivet_util::serde::bare_from_slice!(payload)?)),
			3 => Ok(Request::V3(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Request::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			Request::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Ok]
	}
}

impl Request {
	fn v2_to_v3(self) -> Result<Self> {
		if let Request::V2(x) = self {
			Ok(Request::V3(v3::Request {
				from_replica_id: x.from_replica_id,
				to_replica_id: x.to_replica_id,
				kind: match x.kind {
					v2::RequestKind::UpdateConfigRequest(req) => {
						v3::RequestKind::UpdateConfigRequest(v3::UpdateConfigRequest {
							config: convert_cluster_config_v2_to_v3(req.config),
						})
					}
					v2::RequestKind::PrepareRequest(req) => {
						v3::RequestKind::PrepareRequest(v3::PrepareRequest {
							key: req.key,
							ballot: convert_ballot_v2_to_v3(req.ballot),
							mutable: req.mutable,
							version: req.version,
						})
					}
					v2::RequestKind::PreAcceptRequest(req) => {
						v3::RequestKind::PreAcceptRequest(v3::PreAcceptRequest {
							key: req.key,
							value: Some(req.value),
							ballot: convert_ballot_v2_to_v3(req.ballot),
							mutable: req.mutable,
							version: req.version,
						})
					}
					v2::RequestKind::CommitRequest(req) => {
						v3::RequestKind::CommitRequest(v3::CommitRequest {
							key: req.key,
							value: Some(req.value),
							ballot: convert_ballot_v2_to_v3(req.ballot),
							mutable: req.mutable,
							version: req.version,
						})
					}
					v2::RequestKind::ChangelogReadRequest(req) => {
						v3::RequestKind::ChangelogReadRequest(v3::ChangelogReadRequest {
							after_versionstamp: req.after_versionstamp,
							count: req.count,
						})
					}
					v2::RequestKind::HealthCheckRequest => v3::RequestKind::HealthCheckRequest,
					v2::RequestKind::CoordinatorUpdateReplicaStatusRequest(req) => {
						v3::RequestKind::CoordinatorUpdateReplicaStatusRequest(
							v3::CoordinatorUpdateReplicaStatusRequest {
								replica_id: req.replica_id,
								status: convert_replica_status_v2_to_v3(req.status),
							},
						)
					}
					v2::RequestKind::BeginLearningRequest(req) => {
						v3::RequestKind::BeginLearningRequest(v3::BeginLearningRequest {
							config: convert_cluster_config_v2_to_v3(req.config),
						})
					}
					v2::RequestKind::KvGetRequest(req) => {
						v3::RequestKind::KvGetRequest(v3::KvGetRequest {
							key: req.key,
							caching_behavior: convert_caching_behavior_v2_to_v3(
								req.caching_behavior,
							),
						})
					}
					v2::RequestKind::KvPurgeCacheRequest(req) => {
						v3::RequestKind::KvPurgeCacheRequest(v3::KvPurgeCacheRequest {
							entries: req
								.entries
								.into_iter()
								.map(|e| v3::KvPurgeCacheEntry {
									key: e.key,
									version: e.version,
								})
								.collect(),
						})
					}
				},
			}))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		bail!("cannot downgrade request from v3 to v2");
	}
}

fn convert_ballot_v2_to_v3(b: v2::Ballot) -> v3::Ballot {
	v3::Ballot {
		counter: b.counter,
		replica_id: b.replica_id,
	}
}

fn convert_replica_status_v2_to_v3(s: v2::ReplicaStatus) -> v3::ReplicaStatus {
	match s {
		v2::ReplicaStatus::Joining => v3::ReplicaStatus::Joining,
		v2::ReplicaStatus::Learning => v3::ReplicaStatus::Learning,
		v2::ReplicaStatus::Active => v3::ReplicaStatus::Active,
	}
}

fn convert_replica_config_v2_to_v3(c: v2::ReplicaConfig) -> v3::ReplicaConfig {
	v3::ReplicaConfig {
		replica_id: c.replica_id,
		status: convert_replica_status_v2_to_v3(c.status),
		api_peer_url: c.api_peer_url,
		guard_url: c.guard_url,
	}
}

fn convert_cluster_config_v2_to_v3(c: v2::ClusterConfig) -> v3::ClusterConfig {
	v3::ClusterConfig {
		coordinator_replica_id: c.coordinator_replica_id,
		epoch: c.epoch,
		replicas: c
			.replicas
			.into_iter()
			.map(convert_replica_config_v2_to_v3)
			.collect(),
	}
}

fn convert_caching_behavior_v2_to_v3(b: v2::CachingBehavior) -> v3::CachingBehavior {
	match b {
		v2::CachingBehavior::Optimistic => v3::CachingBehavior::Optimistic,
		v2::CachingBehavior::SkipCache => v3::CachingBehavior::SkipCache,
	}
}
