use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use vbare::OwnedVersionedData;

use super::ids::{BucketBranchId, BucketIdUuid, DatabaseBranchId, DatabaseIdStr};
use super::restore_points::RestorePointRef;
use super::serialization::{SQLITE_DATABASE_BRANCH_RECORD_VERSION, SQLITE_STORAGE_META_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchState {
	Live,
	Frozen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketBranchRecord {
	pub branch_id: BucketBranchId,
	pub parent: Option<BucketBranchId>,
	pub parent_versionstamp: Option<[u8; 16]>,
	pub root_versionstamp: [u8; 16],
	pub fork_depth: u8,
	pub created_at_ms: i64,
	pub created_from_restore_point: Option<RestorePointRef>,
	pub state: BranchState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseBranchRecord {
	pub branch_id: DatabaseBranchId,
	pub bucket_branch: BucketBranchId,
	pub parent: Option<DatabaseBranchId>,
	pub parent_versionstamp: Option<[u8; 16]>,
	pub root_versionstamp: [u8; 16],
	pub fork_depth: u8,
	pub created_at_ms: i64,
	pub created_from_restore_point: Option<RestorePointRef>,
	pub state: BranchState,
	pub lifecycle_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketPointer {
	pub current_branch: BucketBranchId,
	pub last_swapped_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabasePointer {
	pub current_branch: DatabaseBranchId,
	pub last_swapped_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerSnapshot {
	pub schema_version: u32,
	pub pass_versionstamp: [u8; 16],
	pub databases: Vec<(DatabaseIdStr, BucketBranchId, DatabaseBranchId)>,
	pub buckets: Vec<(BucketIdUuid, BucketBranchId)>,
}

enum VersionedDatabaseBranchRecord {
	Current(DatabaseBranchRecord),
}

impl OwnedVersionedData for VersionedDatabaseBranchRecord {
	type Latest = DatabaseBranchRecord;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::Current(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::Current(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::Current(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid depot DatabaseBranchRecord version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::Current(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

enum VersionedDatabasePointer {
	V1(DatabasePointer),
}

impl OwnedVersionedData for VersionedDatabasePointer {
	type Latest = DatabasePointer;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid depot DatabasePointer version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

enum VersionedBucketBranchRecord {
	V1(BucketBranchRecord),
}

impl OwnedVersionedData for VersionedBucketBranchRecord {
	type Latest = BucketBranchRecord;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid depot BucketBranchRecord version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

enum VersionedBucketPointer {
	V1(BucketPointer),
}

impl OwnedVersionedData for VersionedBucketPointer {
	type Latest = BucketPointer;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid depot BucketPointer version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

enum VersionedPointerSnapshot {
	V1(PointerSnapshot),
}

impl OwnedVersionedData for VersionedPointerSnapshot {
	type Latest = PointerSnapshot;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid depot PointerSnapshot version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

pub fn encode_database_branch_record(record: DatabaseBranchRecord) -> Result<Vec<u8>> {
	VersionedDatabaseBranchRecord::wrap_latest(record)
		.serialize_with_embedded_version(SQLITE_DATABASE_BRANCH_RECORD_VERSION)
		.context("encode sqlite database branch record")
}

pub fn decode_database_branch_record(payload: &[u8]) -> Result<DatabaseBranchRecord> {
	VersionedDatabaseBranchRecord::deserialize_with_embedded_version(payload)
		.context("decode sqlite database branch record")
}

pub fn encode_database_pointer(pointer: DatabasePointer) -> Result<Vec<u8>> {
	VersionedDatabasePointer::wrap_latest(pointer)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite database pointer")
}

pub fn decode_database_pointer(payload: &[u8]) -> Result<DatabasePointer> {
	VersionedDatabasePointer::deserialize_with_embedded_version(payload)
		.context("decode sqlite database pointer")
}

pub fn encode_bucket_branch_record(record: BucketBranchRecord) -> Result<Vec<u8>> {
	VersionedBucketBranchRecord::wrap_latest(record)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite bucket branch record")
}

pub fn decode_bucket_branch_record(payload: &[u8]) -> Result<BucketBranchRecord> {
	VersionedBucketBranchRecord::deserialize_with_embedded_version(payload)
		.context("decode sqlite bucket branch record")
}

pub fn encode_bucket_pointer(pointer: BucketPointer) -> Result<Vec<u8>> {
	VersionedBucketPointer::wrap_latest(pointer)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite bucket pointer")
}

pub fn decode_bucket_pointer(payload: &[u8]) -> Result<BucketPointer> {
	VersionedBucketPointer::deserialize_with_embedded_version(payload)
		.context("decode sqlite bucket pointer")
}

pub fn encode_pointer_snapshot(snapshot: PointerSnapshot) -> Result<Vec<u8>> {
	VersionedPointerSnapshot::wrap_latest(snapshot)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite pointer snapshot")
}

pub fn decode_pointer_snapshot(payload: &[u8]) -> Result<PointerSnapshot> {
	VersionedPointerSnapshot::deserialize_with_embedded_version(payload)
		.context("decode sqlite pointer snapshot")
}
