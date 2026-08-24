use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use vbare::OwnedVersionedData;

use super::ids::{BucketBranchId, DatabaseBranchId};
use super::serialization::SQLITE_STORAGE_META_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRoot {
	pub schema_version: u32,
	pub manifest_generation: u64,
	pub hot_watermark_txid: u64,
	pub cold_watermark_txid: u64,
	pub cold_watermark_versionstamp: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqliteCmpDirty {
	pub observed_head_txid: u64,
	pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PitrIntervalCoverage {
	pub txid: u64,
	pub versionstamp: [u8; 16],
	pub wall_clock_ms: i64,
	pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketForkFact {
	pub source_bucket_branch_id: BucketBranchId,
	pub target_bucket_branch_id: BucketBranchId,
	pub fork_versionstamp: [u8; 16],
	pub parent_cap_versionstamp: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketCatalogDbFact {
	pub database_branch_id: DatabaseBranchId,
	pub bucket_branch_id: BucketBranchId,
	pub catalog_versionstamp: [u8; 16],
	pub tombstone_versionstamp: Option<[u8; 16]>,
}

macro_rules! impl_compaction_versioned_data {
	($versioned:ident, $latest:ty, $name:literal) => {
		impl OwnedVersionedData for $versioned {
			type Latest = $latest;

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
					_ => bail!("invalid depot {} version: {version}", $name),
				}
			}

			fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
				match self {
					Self::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
				}
			}
		}
	};
}

enum VersionedCompactionRoot {
	V1(CompactionRoot),
}

enum VersionedSqliteCmpDirty {
	V1(SqliteCmpDirty),
}

enum VersionedPitrIntervalCoverage {
	V1(PitrIntervalCoverage),
}

enum VersionedBucketForkFact {
	V1(BucketForkFact),
}

enum VersionedBucketCatalogDbFact {
	V1(BucketCatalogDbFact),
}

impl_compaction_versioned_data!(VersionedCompactionRoot, CompactionRoot, "CompactionRoot");
impl_compaction_versioned_data!(VersionedSqliteCmpDirty, SqliteCmpDirty, "SqliteCmpDirty");
impl_compaction_versioned_data!(
	VersionedPitrIntervalCoverage,
	PitrIntervalCoverage,
	"PitrIntervalCoverage"
);
impl_compaction_versioned_data!(VersionedBucketForkFact, BucketForkFact, "BucketForkFact");
impl_compaction_versioned_data!(
	VersionedBucketCatalogDbFact,
	BucketCatalogDbFact,
	"BucketCatalogDbFact"
);

pub fn encode_compaction_root(root: CompactionRoot) -> Result<Vec<u8>> {
	VersionedCompactionRoot::wrap_latest(root)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite compaction root")
}

pub fn decode_compaction_root(payload: &[u8]) -> Result<CompactionRoot> {
	VersionedCompactionRoot::deserialize_with_embedded_version(payload)
		.context("decode sqlite compaction root")
}

pub fn encode_sqlite_cmp_dirty(dirty: SqliteCmpDirty) -> Result<Vec<u8>> {
	VersionedSqliteCmpDirty::wrap_latest(dirty)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite compaction dirty marker")
}

pub fn decode_sqlite_cmp_dirty(payload: &[u8]) -> Result<SqliteCmpDirty> {
	VersionedSqliteCmpDirty::deserialize_with_embedded_version(payload)
		.context("decode sqlite compaction dirty marker")
}

pub fn encode_pitr_interval_coverage(coverage: PitrIntervalCoverage) -> Result<Vec<u8>> {
	VersionedPitrIntervalCoverage::wrap_latest(coverage)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite PITR interval coverage")
}

pub fn decode_pitr_interval_coverage(payload: &[u8]) -> Result<PitrIntervalCoverage> {
	VersionedPitrIntervalCoverage::deserialize_with_embedded_version(payload)
		.context("decode sqlite PITR interval coverage")
}

pub fn encode_bucket_fork_fact(fact: BucketForkFact) -> Result<Vec<u8>> {
	VersionedBucketForkFact::wrap_latest(fact)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite bucket fork fact")
}

pub fn decode_bucket_fork_fact(payload: &[u8]) -> Result<BucketForkFact> {
	VersionedBucketForkFact::deserialize_with_embedded_version(payload)
		.context("decode sqlite bucket fork fact")
}

pub fn encode_bucket_catalog_db_fact(fact: BucketCatalogDbFact) -> Result<Vec<u8>> {
	VersionedBucketCatalogDbFact::wrap_latest(fact)
		.serialize_with_embedded_version(SQLITE_STORAGE_META_VERSION)
		.context("encode sqlite bucket catalog db fact")
}

pub fn decode_bucket_catalog_db_fact(payload: &[u8]) -> Result<BucketCatalogDbFact> {
	VersionedBucketCatalogDbFact::deserialize_with_embedded_version(payload)
		.context("decode sqlite bucket catalog db fact")
}
