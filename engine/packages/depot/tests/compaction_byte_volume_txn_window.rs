#![cfg(feature = "test-faults")]

//! Genuine byte-volume txn-window gate (`~/.agents/todo/depot-large-db-harness-byte-scale.md`).
//!
//! The `compaction_large_db_bounded` harness proves the remediated reads materialize a bounded *row
//! count*, but FDB ages a transaction out by **wall clock**, not row count: the test UDB (RocksDB)
//! wraps every transaction closure in `tokio::time::timeout(TXN_TIMEOUT, ..)` with `TXN_TIMEOUT = 5s`
//! and returns `TransactionTooOld` on expiry. A read that is bounded in rows but unbounded in the
//! keyspace it scans would still age out on a large enough database. `UDB_SIMULATED_LATENCY_MS` cannot
//! manufacture this: it is a single pre-sleep at the top of `Database::txn`, paid once per txn and
//! outside the timeout wrapper.
//!
//! This gate seeds a genuinely window-blowing PIDX keyspace, then drives the two real
//! `compaction/shared.rs` scan helpers over it (exposed via `test_hooks::scan_helpers`): the unbounded
//! full-prefix scan must age out the 5s window, and the bounded `<= CMP_FDB_BATCH_MAX_KEYS`-row scan
//! must complete well under it. A regression that swaps a remediated read back to the unbounded form
//! dies here even though every row-count assertion still passes.
//!
//! `#[ignore]` by default: seeding ~2.5M rows plus a deliberate ~5s age-out makes the binary slow. Run
//! with `cargo test -p depot --features test-faults --test compaction_byte_volume_txn_window -- \
//! --ignored --nocapture --test-threads=1`.

mod common;

use std::time::{Duration, Instant};

use anyhow::Result;
use depot::{
	CMP_FDB_BATCH_MAX_KEYS, keys, types::DatabaseBranchId, workflows::compaction::test_hooks,
};
use universaldb::error::DatabaseError;
use universaldb::utils::IsolationLevel::Serializable;

use common::test_db_with_dir;
use test_hooks::scan_helpers;

/// Rows to seed. The measured crossover where one unbounded prefix scan crosses the 5s window is
/// ~1.2M tiny rows; 2.5M gives comfortable margin across machines. Overridable for tuning.
fn seed_rows() -> u64 {
	std::env::var("BYTE_VOLUME_ROWS")
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(2_500_000)
}

/// Rows per seeding transaction. Each seed txn must commit well under the 5s window.
const PER_TXN: u64 = 50_000;

/// Smallest key strictly greater than every key carrying `prefix`, i.e. the exclusive end bound that
/// covers the whole prefix range for a bounded scan.
fn strinc(prefix: &[u8]) -> Vec<u8> {
	let mut end = prefix.to_vec();
	while let Some(&last) = end.last() {
		if last == 0xff {
			end.pop();
		} else {
			*end.last_mut().unwrap() += 1;
			break;
		}
	}
	end
}

/// Writes `rows` PIDX entries (`branch_pidx_key(branch_id, pgno)` -> big-endian txid) straight into
/// UDB, bypassing `Db::commit`. Reaching byte volume through real commits is impractical; a direct
/// bulk write inflates the exact keyspace the hot read scans in seconds.
async fn seed_pidx(
	db: &universaldb::Database,
	branch_id: DatabaseBranchId,
	rows: u64,
) -> Result<()> {
	let mut next = 1u64;
	while next <= rows {
		let end = (next + PER_TXN - 1).min(rows);
		db.txn("byte_vol_seed", move |tx| async move {
			let informal = tx.informal();
			for pgno in next..=end {
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
#[ignore = "byte-volume: seeds ~2.5M rows and deliberately ages out a 5s txn; run with --ignored --nocapture --test-threads=1"]
async fn unbounded_compaction_read_ages_out_bounded_survives() -> Result<()> {
	let (db, _dir) = test_db_with_dir("byte-vol-txn-window-").await?;
	let branch_id = DatabaseBranchId::new_v4();
	let rows = seed_rows();

	let seed_start = Instant::now();
	seed_pidx(&db, branch_id, rows).await?;
	eprintln!("seeded {rows} PIDX rows in {:?}", seed_start.elapsed());

	// Cap retries to 1 so the aged-out unbounded scan fails after a single ~5s attempt instead of
	// re-running until the retry limit. Set after seeding so seed txns keep the normal retry budget.
	db.txn_retry_limit(1)?;

	let prefix = keys::branch_pidx_prefix(branch_id);

	// 1) The unbounded full-prefix scan (the read the hot path used before it was localized to the
	// requested pages) must age out the 5s window on this keyspace.
	let unbounded_prefix = prefix.clone();
	let unbounded_start = Instant::now();
	let unbounded = db
		.txn("byte_vol_unbounded", move |tx| {
			let prefix = unbounded_prefix.clone();
			async move { scan_helpers::scan_prefix_unbounded(&tx, &prefix, Serializable).await }
		})
		.await;
	let unbounded_elapsed = unbounded_start.elapsed();
	eprintln!(
		"unbounded scan returned in {unbounded_elapsed:?}: {:?}",
		unbounded.as_ref().map(Vec::len)
	);
	let err = unbounded.expect_err(&format!(
		"unbounded prefix scan over {rows} PIDX rows must age out the 5s txn window, but it completed \
		 in {unbounded_elapsed:?}",
	));
	// Confirm the failure is the window aging out (retry-capped at 1, so `TransactionTooOld` surfaces
	// as `MaxRetriesReached`), not an unrelated error that would pass a bare `is_err` check.
	let aged_out = err.chain().any(|cause| {
		matches!(
			cause.downcast_ref::<DatabaseError>(),
			Some(DatabaseError::TransactionTooOld | DatabaseError::MaxRetriesReached(_))
		)
	});
	assert!(
		aged_out,
		"unbounded scan failed but not with a txn-window error: {err:?}",
	);

	// 2) The bounded scan (the remediated read) must cap at the budget and complete well under the
	// window on the same keyspace.
	let end = strinc(&prefix);
	let bounded_prefix = prefix.clone();
	let bounded_start = Instant::now();
	let bounded = db
		.txn("byte_vol_bounded", move |tx| {
			let start = bounded_prefix.clone();
			let end = end.clone();
			async move {
				scan_helpers::scan_range_limited(
					&tx,
					&start,
					&end,
					CMP_FDB_BATCH_MAX_KEYS,
					Serializable,
				)
				.await
			}
		})
		.await?;
	let bounded_elapsed = bounded_start.elapsed();
	eprintln!(
		"bounded scan returned {} rows in {bounded_elapsed:?}",
		bounded.len()
	);
	assert_eq!(
		bounded.len(),
		CMP_FDB_BATCH_MAX_KEYS,
		"bounded scan over {rows} rows must cap at the budget",
	);
	assert!(
		bounded_elapsed < Duration::from_secs(5),
		"bounded scan must complete well under the 5s window, took {bounded_elapsed:?}",
	);

	Ok(())
}
