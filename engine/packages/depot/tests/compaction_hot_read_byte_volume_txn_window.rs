#![cfg(feature = "test-faults")]

//! Genuine byte-volume txn-window gate for the REAL compaction hot-input read
//! (`~/.agents/todo/depot-large-db-harness-byte-scale.md`).
//!
//! `compaction_byte_volume_txn_window.rs` gates the scan *helpers* in isolation. This gate drives the
//! real `read_hot_input_snapshot` (via `test_hooks::hot_input`) end to end, which is strictly stronger:
//! it proves the localization that ships (deriving the PIDX entries from the selected slice's deltas
//! and point-reading them, rather than scanning the whole `branch_pidx_prefix`) is what keeps the read
//! inside the FDB 5s transaction window on a branch whose PIDX keyspace is millions of rows.
//!
//! Seeding: one real `Db::commit` writes a small valid slice (COMMITS + DELTA + PIDX for the touched
//! pages) so the read reaches its PIDX phase with a real selected slice; then a direct-FDB bulk write
//! inflates the branch PIDX prefix to byte scale. Reaching byte scale through real commits is
//! impractical, and the padding rows are exactly what the pre-localization full-prefix scan
//! materialized (and aged out on) but the localized read never touches.
//!
//! The read is row-count driven, not literally byte driven: the test UDB (RocksDB) wraps every txn
//! closure in `tokio::time::timeout(TXN_TIMEOUT = 5s)` and returns `TransactionTooOld` on expiry, and
//! that cost is per-row iterate + alloc, so "byte volume" here means millions of PIDX rows. Real FDB
//! enforces the window server-side regardless.
//!
//! One committed test, two self-verifying modes so the same file proves both halves of the
//! before/after:
//! - default: assert the read COMPLETES with a bounded PIDX-entry count (the localized read shipping
//!   at the top of stack).
//! - `HOT_READ_EXPECT_AGE_OUT=1`: assert the read AGES OUT with a txn-window error in the chain (run in
//!   a worktree at a pre-localization revision, where the read scans the whole PIDX prefix).
//!
//! `#[ignore]` by default: seeding a window-blowing PIDX keyspace plus a deliberate ~5s age-out is
//! slow. Run with `cargo test -p depot --features test-faults --test \
//! compaction_hot_read_byte_volume_txn_window -- --ignored --nocapture --test-threads=1`.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use depot::{
	conveyer::{Db, branch},
	keys::{self, PAGE_SIZE},
	types::{BucketId, DatabaseBranchId, DirtyPage},
	workflows::compaction::test_hooks,
};
use gas::prelude::Id;
use rivet_pools::NodeId;
use universaldb::error::DatabaseError;
use universaldb::utils::IsolationLevel::Serializable;
use uuid::Uuid;

use common::test_db_with_dir;

/// PIDX rows to pad the branch prefix with. The measured crossover where one unbounded prefix scan
/// crosses the 5s window is ~1.2M tiny rows; 2.5M gives comfortable margin across machines. These are
/// the rows the pre-localization read scanned and the localized read never touches. Overridable.
fn seed_rows() -> u64 {
	std::env::var("HOT_READ_BYTE_VOLUME_ROWS")
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(2_500_000)
}

/// Whether to assert the read ages out (run in a pre-localization worktree) instead of completing.
fn expect_age_out() -> bool {
	std::env::var("HOT_READ_EXPECT_AGE_OUT")
		.map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
		.unwrap_or(false)
}

/// PIDX rows per seeding transaction. Each seed txn must commit well under the 5s window.
const PER_TXN: u64 = 50_000;

/// Pages the real seed commit touches. The localized read point-reads exactly these, so the bounded
/// PIDX-entry count equals this. Small and well under `MAX_COMMIT_DIRTY_PAGES`.
const COMMITTED_PAGES: u32 = 128;

/// Fixed wall clock shared by the seed commit and the read so PITR interval selection is deterministic.
const NOW_MS: i64 = 1_700_000_000_000;

fn test_bucket() -> Id {
	Id::v1(Uuid::from_u128(0x9ac3), 1)
}

fn dirty_page(pgno: u32) -> DirtyPage {
	DirtyPage {
		pgno,
		bytes: vec![(pgno % 251) as u8; PAGE_SIZE as usize],
	}
}

async fn resolve_branch(db: &universaldb::Database, database_id: &str) -> Result<DatabaseBranchId> {
	let database_id = database_id.to_string();
	db.txn("hot_read_byte_vol_branch", move |tx| {
		let database_id = database_id.clone();
		async move {
			branch::resolve_database_branch(
				&tx,
				BucketId::from_gas_id(test_bucket()),
				&database_id,
				Serializable,
			)
			.await?
			.context("database branch should exist after commit")
		}
	})
	.await
}

/// Pads the branch PIDX prefix with `rows` entries (`branch_pidx_key(branch_id, pgno)` -> big-endian
/// txid) straight into UDB, starting above the committed slice so it never overwrites the real slice's
/// PIDX. This is the exact keyspace the pre-localization hot read scanned in full.
async fn seed_pidx_padding(
	db: &universaldb::Database,
	branch_id: DatabaseBranchId,
	start_pgno: u64,
	rows: u64,
) -> Result<()> {
	let end_pgno = start_pgno + rows - 1;
	let mut next = start_pgno;
	while next <= end_pgno {
		let end = (next + PER_TXN - 1).min(end_pgno);
		db.txn("hot_read_byte_vol_seed", move |tx| async move {
			let informal = tx.informal();
			for pgno in next..=end {
				// Owner txid is irrelevant to the scan cost; the pre-localization read materializes
				// every row before filtering by the slice window.
				let value = pgno.to_be_bytes();
				informal.set(&keys::branch_pidx_key(branch_id, pgno as u32), &value);
			}
			Ok(())
		})
		.await?;
		next = end + 1;
	}
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "byte-volume: seeds a window-blowing PIDX keyspace and drives the real hot-input read; run with --ignored --nocapture --test-threads=1"]
async fn hot_input_read_stays_bounded_at_byte_scale() -> Result<()> {
	let (udb, _dir) = test_db_with_dir("hot-read-byte-vol-").await?;
	let database_id = "hot-read-byte-volume".to_string();
	let db = Db::new(
		Arc::clone(&udb),
		test_bucket(),
		database_id.clone(),
		NodeId::new(),
	);

	// Real slice: one commit of a small page set writes valid COMMITS + DELTA + PIDX so the hot read
	// selects a real slice and reaches its PIDX phase.
	let slice_pages = (1..=COMMITTED_PAGES).map(dirty_page).collect::<Vec<_>>();
	db.commit(slice_pages, COMMITTED_PAGES, NOW_MS)
		.await
		.context("seed commit")?;

	let branch_id = resolve_branch(&udb, &database_id).await?;

	// Inflate the branch PIDX prefix to byte scale above the committed pages.
	let rows = seed_rows();
	let seed_start = Instant::now();
	seed_pidx_padding(&udb, branch_id, COMMITTED_PAGES as u64 + 1, rows).await?;
	eprintln!(
		"committed {COMMITTED_PAGES} slice pages + seeded {rows} padding PIDX rows in {:?}",
		seed_start.elapsed()
	);

	// Cap retries to 1 so an aged-out read fails after a single ~5s attempt. Set after seeding so seed
	// txns keep the normal retry budget.
	udb.txn_retry_limit(1)?;

	let read_start = Instant::now();
	let result = udb
		.txn("hot_read_byte_vol_read", move |tx| async move {
			test_hooks::hot_input::read_hot_input_pidx_entry_count(
				&tx,
				branch_id,
				NOW_MS,
				Serializable,
			)
			.await
		})
		.await;
	let read_elapsed = read_start.elapsed();

	if expect_age_out() {
		eprintln!(
			"hot-input read (expecting age-out) returned in {read_elapsed:?}: {:?}",
			result.as_ref().map(|count| *count)
		);
		let err = result.expect_err(&format!(
			"pre-localization hot read over {rows} PIDX rows must age out the 5s txn window, but it \
			 completed in {read_elapsed:?}",
		));
		let aged_out = err.chain().any(|cause| {
			matches!(
				cause.downcast_ref::<DatabaseError>(),
				Some(DatabaseError::TransactionTooOld | DatabaseError::MaxRetriesReached(_))
			)
		});
		assert!(
			aged_out,
			"hot read failed but not with a txn-window error: {err:?}",
		);
	} else {
		let count = result.context("localized hot read must complete within the txn window")?;
		eprintln!(
			"hot-input read materialized {count} PIDX entries in {read_elapsed:?} (padding rows: {rows})",
		);
		assert_eq!(
			count, COMMITTED_PAGES as usize,
			"localized hot read must materialize exactly the selected slice's pages, not the padded \
			 keyspace",
		);
		assert!(
			read_elapsed < Duration::from_secs(5),
			"localized hot read must complete well under the 5s window, took {read_elapsed:?}",
		);
	}

	Ok(())
}
