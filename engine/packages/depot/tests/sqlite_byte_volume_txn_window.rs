#![cfg(feature = "test-faults")]

//! Genuine byte-volume txn-window gate for the SQLite read path
//! (`~/.agents/todo/depot-large-db-harness-byte-scale.md`).
//!
//! `Db::get_pages` resolves every requested page in ONE `depot_get_pages` FDB transaction (PIDX
//! resolve + DELTA/SHARD decode); it does not chunk internally. The test UDB (RocksDB) wraps each txn
//! closure in `tokio::time::timeout(TXN_TIMEOUT = 5s)` and returns `TransactionTooOld` on expiry, so a
//! large enough page request ages the read out. The production contract is therefore that callers
//! (VFS / preload) must chunk page reads; this gate proves the failure mode is real and that a small
//! request stays well under the window.
//!
//! Caveat baked into the seed size: `tokio::time::timeout` only observes the deadline when the inner
//! future yields at an `.await`. `get_pages` has a CPU-bound page-assembly tail after its FDB reads,
//! and that tail evades the timer, so the age-out must occur during the FDB READ phase, not total
//! wall clock. Empirically the read phase crosses 5s around ~600k pages on the dev box (an
//! intermediate ~300k seed ran 8.4s total yet completed, because its read phase stayed under 5s and
//! the overage was synchronous assembly). The default seed is set well past that so the read phase
//! ages out with margin on faster machines too; real FDB enforces the window server-side and would
//! reject the read regardless of client CPU.
//!
//! Seeds a real multi-page database via `Db::commit`, then drives the real `get_pages`: requesting the
//! whole database in one call ages out, a small slice completes fast. Unlike the compaction PIDX gate,
//! this exercises the full production read (PIDX + DELTA decode) end to end on real page bytes.
//!
//! `#[ignore]` by default: seeding a window-blowing page count plus a deliberate ~5s age-out is slow.
//! Run with `cargo test -p depot --features test-faults --test sqlite_byte_volume_txn_window -- \
//! --ignored --nocapture --test-threads=1`. Tune the seed with `SQLITE_BYTE_VOLUME_PAGES`.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use depot::{conveyer::Db, keys::PAGE_SIZE, types::DirtyPage};
use gas::prelude::Id;
use rivet_pools::NodeId;
use universaldb::error::DatabaseError;
use uuid::Uuid;

use common::test_db_with_dir;

/// Pages to seed. `get_pages` over the whole database in one txn must cross the 5s window; the
/// per-page cost (PIDX resolve + DELTA decode) is far higher than a raw scan, so this needs fewer
/// entities than the raw-scan crossover. Tunable for hardware differences.
fn seed_pages() -> u32 {
	std::env::var("SQLITE_BYTE_VOLUME_PAGES")
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(1_000_000)
}

/// Pages per seeding commit (`MAX_COMMIT_DIRTY_PAGES` is 320; stay under it).
const PAGES_PER_COMMIT: u32 = 256;

/// Pages a bounded request reads. The VFS reads a handful per query; this stands in for the bounded
/// caller pattern that must complete well under the window.
const BOUNDED_PAGES: u32 = 256;

fn test_bucket() -> Id {
	Id::v1(Uuid::from_u128(0x9ac2), 1)
}

fn dirty_page(pgno: u32) -> DirtyPage {
	DirtyPage {
		pgno,
		bytes: vec![(pgno % 251) as u8; PAGE_SIZE as usize],
	}
}

async fn seed_pages_into(db: &Db, total: u32) -> Result<()> {
	let mut pgno = 1u32;
	while pgno <= total {
		let end = (pgno + PAGES_PER_COMMIT - 1).min(total);
		let dirty = (pgno..=end).map(dirty_page).collect::<Vec<_>>();
		db.commit(dirty, total + 1, 1_000 + pgno as i64).await?;
		pgno = end + 1;
	}
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "byte-volume: seeds a large db and deliberately ages out a 5s get_pages txn; run with --ignored --nocapture --test-threads=1"]
async fn get_pages_ages_out_unbounded_bounded_survives() -> Result<()> {
	let (udb, _dir) = test_db_with_dir("sqlite-byte-vol-").await?;
	let database_id = "sqlite-byte-volume".to_string();
	let db = Db::new(Arc::clone(&udb), test_bucket(), database_id, NodeId::new());
	let total = seed_pages();

	let seed_start = Instant::now();
	seed_pages_into(&db, total).await?;
	eprintln!("seeded {total} pages in {:?}", seed_start.elapsed());

	// Bounded request at the normal retry budget: a small slice must complete well under the window.
	let bounded_pgnos = (1..=BOUNDED_PAGES.min(total)).collect::<Vec<_>>();
	let bounded_start = Instant::now();
	let bounded = db.get_pages(bounded_pgnos.clone()).await?;
	let bounded_elapsed = bounded_start.elapsed();
	eprintln!(
		"bounded get_pages({}) returned {} pages in {bounded_elapsed:?}",
		bounded_pgnos.len(),
		bounded.len()
	);
	assert_eq!(
		bounded.len(),
		bounded_pgnos.len(),
		"bounded read must return every requested page"
	);
	assert!(
		bounded_elapsed < Duration::from_secs(5),
		"bounded get_pages must complete well under the 5s window, took {bounded_elapsed:?}",
	);

	// Cap retries to 1 so the aged-out unbounded read fails after a single ~5s attempt.
	udb.txn_retry_limit(1)?;

	// Unbounded request: the whole database in one `get_pages` txn must age out the 5s window.
	let all_pgnos = (1..=total).collect::<Vec<_>>();
	let unbounded_start = Instant::now();
	let unbounded = db.get_pages(all_pgnos).await;
	let unbounded_elapsed = unbounded_start.elapsed();
	eprintln!(
		"unbounded get_pages({total}) returned in {unbounded_elapsed:?}: {:?}",
		unbounded.as_ref().map(Vec::len)
	);
	let err = unbounded.expect_err(&format!(
		"get_pages over all {total} pages must age out the 5s txn window, but it completed in \
		 {unbounded_elapsed:?}",
	));
	let aged_out = err.chain().any(|cause| {
		matches!(
			cause.downcast_ref::<DatabaseError>(),
			Some(DatabaseError::TransactionTooOld | DatabaseError::MaxRetriesReached(_))
		)
	});
	assert!(
		aged_out,
		"unbounded get_pages failed but not with a txn-window error: {err:?}",
	);

	Ok(())
}
