use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use vbare::OwnedVersionedData;

use super::ids::{BucketBranchId, DatabaseBranchId};
use super::restore_points::RestorePointId;
use super::serialization::SQLITE_STORAGE_META_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbHistoryPinKind {
	DatabaseFork,
	BucketFork,
	RestorePoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbHistoryPin {
	pub at_versionstamp: [u8; 16],
	pub at_txid: u64,
	pub kind: DbHistoryPinKind,
	pub owner_database_branch_id: Option<DatabaseBranchId>,
	pub owner_bucket_branch_id: Option<BucketBranchId>,
	pub owner_restore_point: Option<RestorePointId>,
	pub created_at_ms: i64,
}

enum VersionedDbHistoryPin {
	V1(DbHistoryPin),
}

impl OwnedVersionedData for VersionedDbHistoryPin {
	type Latest = DbHistoryPin;

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
			_ => bail!("invalid depot DbHistoryPin version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

pub fn encode_db_history_pin(pin: DbHistoryPin) -> Result<Vec<u8>> {
	VersionedDbHistoryPin::wrap_latest(pin)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite db history pin")
}

pub fn decode_db_history_pin(payload: &[u8]) -> Result<DbHistoryPin> {
	VersionedDbHistoryPin::deserialize_with_embedded_version(payload)
		.context("decode sqlite db history pin")
}
