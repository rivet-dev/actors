//! Structured, read-only snapshot of a database branch's stored history rows.
//!
//! Tests and tooling use this to assert exactly which DELTA, COMMITS, VTX,
//! PIDX, SHARD, PITR_INTERVAL, and DB_PIN rows survive compaction and reclaim.
//! Every set is decoded from raw UDB rows inside one transaction so partial
//! deletes cannot hide behind in-process caches or derived state.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt;
use universaldb::{RangeOption, options::StreamingMode, utils::IsolationLevel::Snapshot};

use crate::conveyer::{
	keys,
	types::{
		CommitRow, CompactionRoot, DBHead, DatabaseBranchId, DbHistoryPin, PitrIntervalCoverage,
		decode_commit_row, decode_compaction_root, decode_db_head, decode_db_history_pin,
		decode_pitr_interval_coverage,
	},
};

/// Exact stored-history state for one database branch.
#[derive(Debug, Clone, Default)]
pub struct BranchHistorySnapshot {
	pub head: Option<DBHead>,
	pub head_at_fork: Option<DBHead>,
	pub compaction_root: Option<CompactionRoot>,
	/// Distinct txids that still have at least one DELTA chunk row.
	pub delta_txids: BTreeSet<u64>,
	/// Total DELTA chunk rows, including multi-chunk blobs.
	pub delta_chunk_rows: usize,
	pub commits: BTreeMap<u64, CommitRow>,
	/// VTX rows as versionstamp -> txid.
	pub vtx: BTreeMap<[u8; 16], u64>,
	/// PIDX rows as page number -> owning delta txid.
	pub pidx: BTreeMap<u32, u64>,
	/// Reader-visible shard versions as shard id -> ascending as_of txids.
	pub shard_versions: BTreeMap<u32, Vec<u64>>,
	/// PITR interval rows as bucket start ms -> coverage.
	pub pitr_intervals: BTreeMap<i64, PitrIntervalCoverage>,
	pub pins: Vec<DbHistoryPin>,
}

impl BranchHistorySnapshot {
	pub fn hot_watermark_txid(&self) -> u64 {
		self.compaction_root
			.as_ref()
			.map(|root| root.hot_watermark_txid)
			.unwrap_or(0)
	}

	pub fn commit_txids(&self) -> BTreeSet<u64> {
		self.commits.keys().copied().collect()
	}

	pub fn vtx_txids(&self) -> BTreeSet<u64> {
		self.vtx.values().copied().collect()
	}

	pub fn pidx_txids(&self) -> BTreeSet<u64> {
		self.pidx.values().copied().collect()
	}

	pub fn pitr_interval_txids(&self) -> BTreeSet<u64> {
		self.pitr_intervals
			.values()
			.map(|coverage| coverage.txid)
			.collect()
	}

	pub fn pin_txids(&self) -> BTreeSet<u64> {
		self.pins.iter().map(|pin| pin.at_txid).collect()
	}
}

pub async fn branch_history_snapshot(
	udb: &universaldb::Database,
	branch_id: DatabaseBranchId,
) -> Result<BranchHistorySnapshot> {
	udb.txn("depot_history_snapshot", move |tx| async move {
		branch_history_snapshot_tx(&tx, branch_id).await
	})
	.await
}

pub async fn branch_history_snapshot_tx(
	tx: &universaldb::Transaction,
	branch_id: DatabaseBranchId,
) -> Result<BranchHistorySnapshot> {
	let mut snapshot = BranchHistorySnapshot::default();

	snapshot.head = read_value(tx, &keys::branch_meta_head_key(branch_id))
		.await?
		.as_deref()
		.map(decode_db_head)
		.transpose()
		.context("decode sqlite head for history snapshot")?;
	snapshot.head_at_fork = read_value(tx, &keys::branch_meta_head_at_fork_key(branch_id))
		.await?
		.as_deref()
		.map(decode_db_head)
		.transpose()
		.context("decode sqlite fork head for history snapshot")?;
	snapshot.compaction_root = read_value(tx, &keys::branch_compaction_root_key(branch_id))
		.await?
		.as_deref()
		.map(decode_compaction_root)
		.transpose()
		.context("decode sqlite compaction root for history snapshot")?;

	for (key, _) in scan_prefix(tx, &keys::branch_delta_prefix(branch_id)).await? {
		let txid = keys::decode_branch_delta_chunk_txid(branch_id, &key)?;
		snapshot.delta_txids.insert(txid);
		snapshot.delta_chunk_rows += 1;
	}

	for (key, value) in scan_prefix(tx, &keys::branch_commit_prefix(branch_id)).await? {
		let txid = decode_be_u64_suffix(&keys::branch_commit_prefix(branch_id), &key)
			.context("decode sqlite commit txid for history snapshot")?;
		let commit =
			decode_commit_row(&value).context("decode sqlite commit row for history snapshot")?;
		snapshot.commits.insert(txid, commit);
	}

	for (key, value) in scan_prefix(tx, &keys::branch_vtx_prefix(branch_id)).await? {
		let prefix = keys::branch_vtx_prefix(branch_id);
		let suffix = key
			.strip_prefix(prefix.as_slice())
			.context("branch VTX key did not start with expected prefix")?;
		let versionstamp: [u8; 16] = suffix
			.try_into()
			.map_err(|_| anyhow::anyhow!("branch VTX suffix should be 16 bytes"))?;
		let txid = decode_be_u64_value(&value)
			.context("decode sqlite VTX txid value for history snapshot")?;
		snapshot.vtx.insert(versionstamp, txid);
	}

	for (key, value) in scan_prefix(tx, &keys::branch_pidx_prefix(branch_id)).await? {
		let prefix = keys::branch_pidx_prefix(branch_id);
		let suffix = key
			.strip_prefix(prefix.as_slice())
			.context("branch PIDX key did not start with expected prefix")?;
		let pgno_bytes: [u8; 4] = suffix
			.try_into()
			.map_err(|_| anyhow::anyhow!("branch PIDX suffix should be 4 bytes"))?;
		let txid = decode_be_u64_value(&value)
			.context("decode sqlite PIDX txid value for history snapshot")?;
		snapshot.pidx.insert(u32::from_be_bytes(pgno_bytes), txid);
	}

	for (key, _) in scan_prefix(tx, &keys::branch_shard_prefix(branch_id)).await? {
		let prefix = keys::branch_shard_prefix(branch_id);
		let suffix = key
			.strip_prefix(prefix.as_slice())
			.context("branch SHARD key did not start with expected prefix")?;
		ensure!(
			suffix.len() == 4 + 1 + 8 && suffix[4] == b'/',
			"branch SHARD key suffix had unexpected layout"
		);
		let shard_id = u32::from_be_bytes(
			suffix[..4]
				.try_into()
				.context("branch SHARD id should decode as u32")?,
		);
		let as_of_txid = u64::from_be_bytes(
			suffix[5..]
				.try_into()
				.context("branch SHARD as_of txid should decode as u64")?,
		);
		snapshot
			.shard_versions
			.entry(shard_id)
			.or_default()
			.push(as_of_txid);
	}

	for (key, value) in scan_prefix(tx, &keys::branch_pitr_interval_prefix(branch_id)).await? {
		let bucket_start_ms = keys::decode_branch_pitr_interval_bucket(branch_id, &key)?;
		let coverage = decode_pitr_interval_coverage(&value)
			.context("decode sqlite PITR interval coverage for history snapshot")?;
		snapshot.pitr_intervals.insert(bucket_start_ms, coverage);
	}

	for (_, value) in scan_prefix(tx, &keys::db_pin_prefix(branch_id)).await? {
		let pin =
			decode_db_history_pin(&value).context("decode sqlite db pin for history snapshot")?;
		snapshot.pins.push(pin);
	}

	Ok(snapshot)
}

async fn read_value(tx: &universaldb::Transaction, key: &[u8]) -> Result<Option<Vec<u8>>> {
	Ok(tx.informal().get(key, Snapshot).await?.map(Vec::<u8>::from))
}

async fn scan_prefix(
	tx: &universaldb::Transaction,
	prefix: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
	let prefix_subspace =
		universaldb::Subspace::from(universaldb::tuple::Subspace::from_bytes(prefix.to_vec()));
	let informal = tx.informal();
	let mut stream = informal.get_ranges_keyvalues(
		RangeOption {
			mode: StreamingMode::WantAll,
			..RangeOption::from(&prefix_subspace)
		},
		Snapshot,
	);
	let mut rows = Vec::new();

	while let Some(entry) = stream.try_next().await? {
		rows.push((entry.key().to_vec(), entry.value().to_vec()));
	}

	Ok(rows)
}

fn decode_be_u64_suffix(prefix: &[u8], key: &[u8]) -> Result<u64> {
	let suffix = key
		.strip_prefix(prefix)
		.context("key did not start with expected prefix")?;
	let bytes: [u8; 8] = suffix
		.try_into()
		.map_err(|_| anyhow::anyhow!("key suffix should be 8 bytes"))?;

	Ok(u64::from_be_bytes(bytes))
}

fn decode_be_u64_value(value: &[u8]) -> Result<u64> {
	let bytes: [u8; 8] = value
		.try_into()
		.map_err(|_| anyhow::anyhow!("value should be 8 bytes"))?;

	Ok(u64::from_be_bytes(bytes))
}
