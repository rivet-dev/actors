use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rocksdb::{
	OptimisticTransactionDB, ReadOptions, SnapshotWithThreadMode,
	Transaction as RocksDbTransaction, WriteOptions,
};
use tokio::sync::{mpsc, oneshot};

use crate::{
	atomic::apply_atomic_op,
	conflict_tracker::TransactionConflictTracker,
	error::DatabaseError,
	options::{ConflictRangeType, MutationType},
	tx_ops::{self, Operation},
	value::{KeyValue, Slice, Values},
	versionstamp::{generate_versionstamp, substitute_raw_versionstamp},
};

/// Copy bytes borrowed from a rocksdb iterator into an owned `Vec`.
///
/// RocksDB hands back a null data pointer for zero-length keys and values.
/// Copying from a null pointer (which `<[u8]>::to_vec` does internally via
/// `ptr::copy_nonoverlapping`) violates its non-null precondition and aborts the
/// process on builds compiled with UB checks (debug assertions) enabled, even
/// though the copy length is zero. This is also why the boxing `DBIterator`
/// adapter must be avoided here: its `Iterator::next` unconditionally boxes the
/// value via `Box::<[u8]>::from(&[])`, hitting the same null-pointer copy. Guard
/// the empty case so we never copy from the null pointer.
fn iter_bytes_to_vec(bytes: &[u8]) -> Vec<u8> {
	if bytes.is_empty() {
		Vec::new()
	} else {
		bytes.to_vec()
	}
}

/// The point-in-time view a single UDB transaction reads from.
type Snapshot<'a> = SnapshotWithThreadMode<'a, OptimisticTransactionDB>;

pub enum TransactionCommand {
	Get {
		key: Vec<u8>,
		response: oneshot::Sender<Result<Option<Slice>>>,
	},
	GetKey {
		key: Vec<u8>,
		or_equal: bool,
		offset: i32,
		response: oneshot::Sender<Result<Option<Slice>>>,
	},
	GetRange {
		begin: Vec<u8>,
		begin_or_equal: bool,
		begin_offset: i32,
		end: Vec<u8>,
		end_or_equal: bool,
		end_offset: i32,
		limit: Option<usize>,
		reverse: bool,
		response: oneshot::Sender<Result<Values>>,
	},
	Commit {
		start_version: u64,
		operations: Vec<Operation>,
		conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
		response: oneshot::Sender<Result<()>>,
	},
	GetEstimatedRangeSize {
		begin: Vec<u8>,
		end: Vec<u8>,
		response: oneshot::Sender<Result<i64>>,
	},
}

// This task may be used for multiple rocksdb txns, in contrast to how postgres is written. This is solely to
// save on spawning new tasks.
pub struct TransactionTask {
	db: Arc<OptimisticTransactionDB>,
	txn_conflict_tracker: TransactionConflictTracker,
	receiver: mpsc::UnboundedReceiver<TransactionCommand>,
}

impl TransactionTask {
	pub fn new(
		db: Arc<OptimisticTransactionDB>,
		txn_conflict_tracker: TransactionConflictTracker,
		receiver: mpsc::UnboundedReceiver<TransactionCommand>,
	) -> Self {
		TransactionTask {
			db,
			txn_conflict_tracker,
			receiver,
		}
	}

	pub async fn run(mut self) {
		// One pinned snapshot per UDB transaction, taken at the first read. FDB pins a read version at
		// the first read so every read in a transaction observes the same point in time; without this
		// a commit landing mid-transaction would be partially visible. It is released on commit so a
		// reused task takes a fresh snapshot for the next transaction.
		let mut snapshot: Option<SnapshotWithThreadMode<'_, OptimisticTransactionDB>> = None;

		while let Some(command) = self.receiver.recv().await {
			match command {
				TransactionCommand::Get { key, response } => {
					let snapshot = snapshot.get_or_insert_with(|| self.db.snapshot());
					let result = Self::handle_get(snapshot, &key);
					let _ = response.send(result);
				}
				TransactionCommand::GetKey {
					key,
					or_equal,
					offset,
					response,
				} => {
					let snapshot = snapshot.get_or_insert_with(|| self.db.snapshot());
					let result = Self::handle_get_key(snapshot, &key, or_equal, offset);
					let _ = response.send(result);
				}
				TransactionCommand::GetRange {
					begin,
					begin_or_equal,
					begin_offset,
					end,
					end_or_equal,
					end_offset,
					limit,
					reverse,
					response,
				} => {
					let snapshot = snapshot.get_or_insert_with(|| self.db.snapshot());
					let result = Self::handle_get_range(
						snapshot,
						begin,
						begin_or_equal,
						begin_offset,
						end,
						end_or_equal,
						end_offset,
						limit,
						reverse,
					);
					let _ = response.send(result);
				}
				TransactionCommand::Commit {
					start_version,
					operations,
					conflict_ranges,
					response,
				} => {
					// The commit reads the latest committed state, not this transaction's snapshot, so
					// release the snapshot before applying.
					snapshot = None;

					let result = self
						.handle_commit(start_version, operations, conflict_ranges)
						.await;
					let _ = response.send(result);
				}
				TransactionCommand::GetEstimatedRangeSize {
					begin,
					end,
					response,
				} => {
					let result = self.handle_get_estimated_range_size(&begin, &end);
					let _ = response.send(result);
				}
			}
		}
	}

	fn create_transaction(&self) -> RocksDbTransaction<'_, OptimisticTransactionDB> {
		let write_opts = WriteOptions::default();
		let mut txn_opts = rocksdb::OptimisticTransactionOptions::default();
		txn_opts.set_snapshot(true);
		self.db.transaction_opt(&write_opts, &txn_opts)
	}

	fn handle_get(snapshot: &Snapshot<'_>, key: &[u8]) -> Result<Option<Slice>> {
		Ok(snapshot
			.get(key)
			.context("failed to read key from rocksdb")?
			.map(|v| v.into()))
	}

	fn handle_get_key(
		snapshot: &Snapshot<'_>,
		key: &[u8],
		or_equal: bool,
		offset: i32,
	) -> Result<Option<Slice>> {
		// Based on PostgreSQL's interpretation:
		// (false, 1) => first_greater_or_equal
		// (true, 1) => first_greater_than
		// (false, 0) => last_less_than
		// (true, 0) => last_less_or_equal

		match (or_equal, offset) {
			(false, 1) => {
				// first_greater_or_equal: find first key >= search_key
				let mut iter = snapshot.raw_iterator();
				iter.seek(key);
				let result = iter.key().map(iter_bytes_to_vec);
				iter.status()
					.context("failed to iterate rocksdb for first_greater_or_equal")?;
				Ok(result.map(Into::into))
			}
			(true, 1) => {
				// first_greater_than: find first key > search_key
				let mut iter = snapshot.raw_iterator();
				iter.seek(key);
				while iter.valid() {
					let k = iter.key().expect("iterator should be valid");
					// Skip if it's the exact key
					if k == key {
						iter.next();
						continue;
					}
					return Ok(Some(iter_bytes_to_vec(k).into()));
				}
				iter.status()
					.context("failed to iterate rocksdb for first_greater_than")?;
				Ok(None)
			}
			(false, 0) => {
				// last_less_than: find last key < search_key
				// Use reverse iterator starting just before the key
				let mut iter = snapshot.raw_iterator();
				iter.seek_for_prev(key);
				while iter.valid() {
					let k = iter.key().expect("iterator should be valid");
					// We want strictly less than
					if k < key {
						return Ok(Some(iter_bytes_to_vec(k).into()));
					}
					iter.prev();
				}
				iter.status()
					.context("failed to iterate rocksdb for last_less_than")?;
				Ok(None)
			}
			(true, 0) => {
				// last_less_or_equal: find last key <= search_key
				// Use reverse iterator starting from the key
				let mut iter = snapshot.raw_iterator();
				iter.seek_for_prev(key);
				while iter.valid() {
					let k = iter.key().expect("iterator should be valid");
					// We want less than or equal
					if k <= key {
						return Ok(Some(iter_bytes_to_vec(k).into()));
					}
					iter.prev();
				}
				iter.status()
					.context("failed to iterate rocksdb for last_less_or_equal")?;
				Ok(None)
			}
			_ => {
				// For other offset values, return an error
				bail!("invalid key selector offset")
			}
		}
	}

	async fn handle_commit(
		&self,
		start_version: u64,
		operations: Vec<Operation>,
		conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
	) -> Result<()> {
		// A read-only transaction is never committed, matching FDB. Its reads all came from one pinned
		// snapshot, so there is nothing to validate, and running the conflict check anyway would abort
		// a transaction that cannot have observed anything inconsistent.
		if tx_ops::is_read_only(&operations, &conflict_ranges) {
			return Ok(());
		}

		// Create a new transaction for this commit
		let txn = self.create_transaction();
		let transaction_versionstamp = generate_versionstamp(0);

		// Apply all operations to the transaction
		for op in operations {
			match op {
				Operation::SetValue { key, value } => {
					txn.put(key, &value)
						.context("failed to set key in rocksdb")?;
				}
				Operation::Clear { key } => {
					txn.delete(key)
						.context("failed to delete key from rocksdb")?;
				}
				Operation::ClearRange { begin, end } => {
					// RocksDB doesn't have a native clear_range, so we need to iterate and delete.
					// Collect the in-range keys first, then delete after dropping the iterator, so
					// we never mutate the transaction's write batch while iterating it.
					let read_opts = ReadOptions::default();
					let mut iter = txn.raw_iterator_opt(read_opts);
					iter.seek(&begin);

					let mut keys_to_delete: Vec<Vec<u8>> = Vec::new();
					while iter.valid() {
						let k = iter.key().expect("iterator should be valid");
						if k >= end.as_slice() {
							break;
						}
						keys_to_delete.push(iter_bytes_to_vec(k));
						iter.next();
					}
					iter.status()
						.context("failed to iterate rocksdb for clear range")?;
					drop(iter);

					for key in keys_to_delete {
						txn.delete(&key)
							.context("failed to delete key in range from rocksdb")?;
					}
				}
				Operation::AtomicOp {
					key,
					param,
					op_type,
				} => {
					if matches!(op_type, MutationType::SetVersionstampedKey) {
						let key = substitute_raw_versionstamp(key, &transaction_versionstamp)
							.map_err(anyhow::Error::msg)
							.context("failed substituting versionstamped key")?;
						txn.put(key, &param)
							.context("failed to set versionstamped key in rocksdb")?;
						continue;
					}

					if matches!(op_type, MutationType::SetVersionstampedValue) {
						let value = substitute_raw_versionstamp(param, &transaction_versionstamp)
							.map_err(anyhow::Error::msg)
							.context("failed substituting versionstamped value")?;
						txn.put(key, &value)
							.context("failed to set versionstamped value in rocksdb")?;
						continue;
					}

					// Get the current value from the database
					let read_opts = ReadOptions::default();
					let current_value = txn
						.get_opt(&key, &read_opts)
						.context("failed to get current value for atomic operation")?;

					// Apply the atomic operation
					let current_slice = current_value.as_deref();
					let new_value = apply_atomic_op(current_slice, &param, op_type);

					// Store the result
					if let Some(new_value) = &new_value {
						txn.put(key, new_value)
							.context("failed to set atomic operation result")?;
					} else {
						txn.delete(key)
							.context("failed to delete key after atomic operation")?;
					}
				}
			}
		}

		// rocksdb generates both start and commit versions from the in-process counter.
		let commit_version = self.txn_conflict_tracker.next_global_version();
		if self
			.txn_conflict_tracker
			.check_and_insert(start_version, commit_version, conflict_ranges)
			.await
		{
			return Err(DatabaseError::NotCommitted.into());
		}

		// Commit the transaction (this consumes txn)
		match txn.commit() {
			Ok(_) => Ok(()),
			Err(e) => {
				// If the txn failed due to a rocksdb error, remove it from the conflict tracker
				self.txn_conflict_tracker.remove(commit_version).await;

				let err_str = e.to_string();

				// Check if this is a conflict error
				if err_str.contains("conflict") || err_str.contains("Resource busy") {
					// Return retryable error
					Err(DatabaseError::NotCommitted.into())
				} else {
					Err(e).context("rocksdb commit error")
				}
			}
		}
	}

	fn handle_get_range(
		snapshot: &Snapshot<'_>,
		begin: Vec<u8>,
		begin_or_equal: bool,
		begin_offset: i32,
		end: Vec<u8>,
		end_or_equal: bool,
		end_offset: i32,
		limit: Option<usize>,
		reverse: bool,
	) -> Result<Values> {
		// Resolve the begin selector
		let resolved_begin =
			Self::resolve_key_selector_for_range(snapshot, &begin, begin_or_equal, begin_offset)?;

		// Resolve the end selector
		let resolved_end =
			Self::resolve_key_selector_for_range(snapshot, &end, end_or_equal, end_offset)?;

		let mut results = Vec::new();
		let limit = limit.unwrap_or(usize::MAX);

		// When reversing, iterate descending from the end so that `limit` selects
		// the highest keys in range (matching FDB semantics). Applying `limit`
		// during a forward scan and reversing afterward would instead return the
		// lowest keys, which is wrong for reverse range reads.
		if reverse {
			let mut iter = snapshot.raw_iterator();
			iter.seek_for_prev(&resolved_end);

			while iter.valid() {
				let k = iter.key().expect("iterator should be valid");
				// The end key is exclusive, so skip anything at or above it.
				if k >= resolved_end.as_slice() {
					iter.prev();
					continue;
				}
				// The begin key is inclusive; once we drop below it we are done.
				if k < resolved_begin.as_slice() {
					break;
				}

				let key = iter_bytes_to_vec(k);
				let value = iter.value().map(iter_bytes_to_vec).unwrap_or_default();
				results.push(KeyValue::new(key, value));

				if results.len() >= limit {
					break;
				}
				iter.prev();
			}
			iter.status()
				.context("failed to iterate rocksdb for get range")?;
		} else {
			let mut iter = snapshot.raw_iterator();
			iter.seek(&resolved_begin);

			while iter.valid() {
				let k = iter.key().expect("iterator should be valid");
				// Check if we've reached the end key
				if k >= resolved_end.as_slice() {
					break;
				}

				let key = iter_bytes_to_vec(k);
				let value = iter.value().map(iter_bytes_to_vec).unwrap_or_default();
				results.push(KeyValue::new(key, value));

				if results.len() >= limit {
					break;
				}
				iter.next();
			}
			iter.status()
				.context("failed to iterate rocksdb for get range")?;
		}

		Ok(Values::new(results))
	}

	fn resolve_key_selector_for_range(
		snapshot: &Snapshot<'_>,
		key: &[u8],
		or_equal: bool,
		offset: i32,
	) -> Result<Vec<u8>> {
		// Based on PostgreSQL's interpretation:
		// (false, 1) => first_greater_or_equal
		// (true, 1) => first_greater_than
		// (false, 0) => last_less_than
		// (true, 0) => last_less_or_equal

		match (or_equal, offset) {
			(false, 1) => {
				// first_greater_or_equal: find first key >= search_key
				let mut iter = snapshot.raw_iterator();
				iter.seek(key);
				let result = iter.key().map(iter_bytes_to_vec);
				iter.status().context(
					"failed to iterate rocksdb for range selector first_greater_or_equal",
				)?;
				// If no key found, return a key that will make the range empty
				Ok(result.unwrap_or_else(|| vec![0xff; 255]))
			}
			(true, 1) => {
				// first_greater_than: find first key > search_key
				let mut iter = snapshot.raw_iterator();
				iter.seek(key);
				while iter.valid() {
					let k = iter.key().expect("iterator should be valid");
					// Skip if it's the exact key
					if k == key {
						iter.next();
						continue;
					}
					return Ok(iter_bytes_to_vec(k));
				}
				iter.status()
					.context("failed to iterate rocksdb for range selector first_greater_than")?;
				// If no key found, return a key that will make the range empty
				Ok(vec![0xff; 255])
			}
			_ => {
				// For other cases, just use the key as-is for now
				// This is a simplification - full implementation would handle all cases
				Ok(key.to_vec())
			}
		}
	}

	fn handle_get_estimated_range_size(&self, begin: &[u8], end: &[u8]) -> Result<i64> {
		let range = rocksdb::Range::new(begin, end);

		Ok(self
			.db
			.get_approximate_sizes(&[range])
			.first()
			.copied()
			.unwrap_or(0) as i64)
	}
}
