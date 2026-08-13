use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use vbare::OwnedVersionedData;

use super::ids::DatabaseBranchId;
use super::serialization::SQLITE_STORAGE_META_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct RestorePointId(String);

impl RestorePointId {
	pub fn new(restore_point: impl Into<String>) -> Result<Self> {
		let restore_point = restore_point.into();
		let bytes = restore_point.as_bytes();
		if bytes.len() != 33
			|| bytes[16] != b'-'
			|| !bytes[..16].iter().all(|byte| byte.is_ascii_hexdigit())
			|| !bytes[17..].iter().all(|byte| byte.is_ascii_hexdigit())
		{
			bail!("sqlite restore point id must match 0000000000000000-0000000000000000");
		}

		Ok(Self(restore_point))
	}

	pub fn format(ts_ms: i64, txid: u64) -> Result<Self> {
		if ts_ms < 0 {
			bail!("sqlite restore point timestamp must be non-negative");
		}

		Self::new(format!("{:016x}-{txid:016x}", ts_ms as u64))
	}

	pub fn parse(&self) -> Result<(i64, u64)> {
		let ts_ms = u64::from_str_radix(&self.0[..16], 16)
			.context("parse sqlite restore point timestamp")?;
		let txid =
			u64::from_str_radix(&self.0[17..], 16).context("parse sqlite restore point txid")?;
		let ts_ms =
			i64::try_from(ts_ms).context("sqlite restore point timestamp exceeds i64 range")?;

		Ok((ts_ms, txid))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}

	pub fn into_string(self) -> String {
		self.0
	}
}

impl<'de> Deserialize<'de> for RestorePointId {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let restore_point = String::deserialize(deserializer)?;
		Self::new(restore_point).map_err(serde::de::Error::custom)
	}
}

impl TryFrom<String> for RestorePointId {
	type Error = anyhow::Error;

	fn try_from(value: String) -> Result<Self> {
		Self::new(value)
	}
}

impl TryFrom<&str> for RestorePointId {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		Self::new(value)
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePointRef {
	pub restore_point: RestorePointId,
	pub resolved_versionstamp: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedVersionstamp {
	pub versionstamp: [u8; 16],
	pub restore_point: Option<RestorePointRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotSelector {
	Latest,
	AtTimestamp { timestamp_ms: i64 },
	RestorePoint { restore_point: RestorePointId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotKind {
	Latest,
	AtTimestamp,
	RestorePoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRestoreTarget {
	pub database_branch_id: DatabaseBranchId,
	pub txid: u64,
	pub versionstamp: [u8; 16],
	pub wall_clock_ms: i64,
	pub kind: SnapshotKind,
	pub restore_point: Option<RestorePointRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinStatus {
	Pending,
	/// The restore point has an FDB `DB_PIN` history record.
	Ready,
	Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePointIndexEntry {
	pub schema_version: u32,
	pub restore_point_id: RestorePointId,
	pub pin_object_key: Option<String>,
	pub pin_status: PinStatus,
	pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePointRecord {
	pub restore_point_id: RestorePointId,
	pub database_branch_id: DatabaseBranchId,
	pub versionstamp: [u8; 16],
	pub status: PinStatus,
	/// Current restore_points are FDB history pins, so this is `None`.
	pub pin_object_key: Option<String>,
	pub created_at_ms: i64,
	pub updated_at_ms: i64,
}

enum VersionedRestorePointRecord {
	V1(RestorePointRecord),
}

impl OwnedVersionedData for VersionedRestorePointRecord {
	type Latest = RestorePointRecord;

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
			_ => bail!("invalid depot RestorePointRecord version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}
}

pub fn encode_restore_point_record(record: RestorePointRecord) -> Result<Vec<u8>> {
	VersionedRestorePointRecord::wrap_latest(record)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite restore point record")
}

pub fn decode_restore_point_record(payload: &[u8]) -> Result<RestorePointRecord> {
	VersionedRestorePointRecord::deserialize_with_embedded_version(payload)
		.context("decode sqlite restore point record")
}
