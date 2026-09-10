use std::{
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicI32, Ordering},
	},
};

use anyhow::{Context, Result};
use rocksdb::{OptimisticTransactionDB, Options, checkpoint::Checkpoint};

use crate::{
	RetryableTransaction, Transaction,
	driver::{BoxFut, DatabaseDriver, Erased},
	error::DatabaseError,
	transaction::TXN_TIMEOUT,
	utils::{MaybeCommitted, calculate_tx_retry_backoff},
};

use crate::conflict_tracker::TransactionConflictTracker;

use super::transaction::RocksDbTransactionDriver;

/// Sentinel for "the transaction closure set no per-transaction retry limit", so the database-wide
/// limit applies.
pub const RETRY_LIMIT_UNSET: i32 = -1;

pub struct RocksDbDatabaseDriver {
	db: Arc<OptimisticTransactionDB>,
	max_retries: AtomicI32,
	txn_conflict_tracker: TransactionConflictTracker,
}

impl RocksDbDatabaseDriver {
	pub async fn new(db_path: PathBuf) -> Result<Self> {
		tracing::info!(db_path=%db_path.display(), "starting file system driver");

		// Create directory if it doesn't exist
		std::fs::create_dir_all(&db_path).context("failed to create database directory")?;

		// Configure RocksDB options
		let mut opts = Options::default();
		opts.create_if_missing(true);
		opts.set_max_open_files(10000);
		opts.set_keep_log_file_num(10);
		opts.set_max_total_wal_size(64 * 1024 * 1024); // 64MiB
		opts.set_write_buffer_size(256 * 1024 * 1024); // 256MiB for conflict detection

		// Open the OptimisticTransactionDB
		tracing::debug!(path=%db_path.display(), "opening rocksdb");
		let db = OptimisticTransactionDB::open(&opts, db_path).context("failed to open rocksdb")?;

		Ok(RocksDbDatabaseDriver {
			db: Arc::new(db),
			max_retries: AtomicI32::new(10),
			txn_conflict_tracker: TransactionConflictTracker::new(),
		})
	}
}

impl DatabaseDriver for RocksDbDatabaseDriver {
	fn create_txn(&self) -> Result<Transaction> {
		Ok(Transaction::new(Arc::new(RocksDbTransactionDriver::new(
			self.db.clone(),
			self.txn_conflict_tracker.clone(),
		))))
	}

	fn run<'a>(
		&'a self,
		closure: Box<dyn Fn(RetryableTransaction) -> BoxFut<'a, Result<Erased>> + Send + Sync + 'a>,
	) -> BoxFut<'a, Result<Erased>> {
		Box::pin(async move {
			let mut maybe_committed = MaybeCommitted(false);
			let max_retries = self.max_retries.load(Ordering::SeqCst);
			// Owned here rather than per attempt: each attempt builds a fresh transaction driver, so a
			// limit the closure sets has to outlive the attempt that set it to bound the next one.
			let retry_limit = Arc::new(AtomicI32::new(RETRY_LIMIT_UNSET));

			let mut attempt = 0;
			loop {
				let tx = Transaction::new(Arc::new(RocksDbTransactionDriver::with_retry_limit(
					self.db.clone(),
					self.txn_conflict_tracker.clone(),
					retry_limit.clone(),
				)));
				let mut retryable = RetryableTransaction::new(tx);
				retryable.maybe_committed = maybe_committed;

				// Execute transaction
				let error =
					match tokio::time::timeout(TXN_TIMEOUT, closure(retryable.clone())).await {
						Ok(Ok(res)) => match retryable.inner.driver.commit_ref().await {
							Ok(_) => return Ok(res),
							Err(e) => e,
						},
						Ok(Err(e)) => e,
						Err(_) => anyhow::Error::from(DatabaseError::TransactionTooOld),
					};

				let chain = error
					.chain()
					.find_map(|x| x.downcast_ref::<DatabaseError>());

				if let Some(db_error) = chain {
					// Handle retry or return error
					if db_error.is_retryable() {
						if db_error.is_maybe_committed() {
							maybe_committed = MaybeCommitted(true);
						}

						// Re-read every iteration. Nothing has called `retry_limit` before the first
						// attempt; from then on the closure's limit wins over the database-wide one.
						// The check runs after an attempt failed, so both values bound retries rather
						// than total attempts.
						let limit = retry_limit.load(Ordering::SeqCst);
						let retry_budget = if limit == RETRY_LIMIT_UNSET {
							max_retries
						} else {
							limit
						};
						if attempt >= retry_budget {
							return Err(DatabaseError::MaxRetriesReached(error).into());
						}

						attempt += 1;

						let backoff_ms = calculate_tx_retry_backoff(attempt as usize);
						tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
					} else {
						return Err(error);
					}
				} else {
					return Err(error);
				}
			}
		})
	}

	fn txn_retry_limit(&self, limit: i32) -> Result<()> {
		self.max_retries.store(limit, Ordering::SeqCst);
		Ok(())
	}

	fn checkpoint(&self, path: &Path) -> Result<()> {
		let cp = Checkpoint::new(&*self.db).context("failed to create checkpoint handle")?;
		cp.create_checkpoint(path)
			.context("failed to create rocksdb checkpoint")?;
		Ok(())
	}
}

impl Drop for RocksDbDatabaseDriver {
	fn drop(&mut self) {
		self.db.cancel_all_background_work(true);
	}
}
