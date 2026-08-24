use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use deadpool_postgres::Transaction;
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::IsolationLevel;

use crate::{
	options::ConflictRangeType,
	tx_ops::Operation,
	value::{KeyValue, Slice, Values},
};

use super::{commit, shared::PostgresShared};

pub enum TransactionCommand {
	// Read operations
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
	// Transaction control
	Commit {
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

/// TransactionTask runs in a separate tokio task to own a single pinned PostgreSQL `REPEATABLE READ`
/// snapshot connection for the lifetime of a follower transaction.
///
/// Reads go directly against this snapshot (they never involve the leader). Commits delegate to
/// [`commit::submit`], which enqueues the request on the leader and awaits the result. The
/// `read_version` is captured from the cached watermark before the snapshot is opened, so no write
/// with `commit_version <= read_version` can be invisible to the snapshot.
pub struct TransactionTask {
	shared: Arc<PostgresShared>,
	receiver: mpsc::UnboundedReceiver<TransactionCommand>,
}

impl TransactionTask {
	pub fn new(
		shared: Arc<PostgresShared>,
		receiver: mpsc::UnboundedReceiver<TransactionCommand>,
	) -> Self {
		Self { shared, receiver }
	}

	pub async fn run(mut self) {
		// Capture the read version BEFORE opening the snapshot so the snapshot reflects every write
		// with commit_version <= read_version.
		let read_version = self.shared.read_version();

		let mut conn = match self.shared.pool.get().await {
			Ok(conn) => conn,
			Err(_) => {
				self.fail_receiver().await;
				return;
			}
		};

		let tx = match conn
			.build_transaction()
			.isolation_level(IsolationLevel::RepeatableRead)
			.read_only(true)
			.start()
			.await
		{
			Ok(tx) => tx,
			Err(_) => {
				self.fail_receiver().await;
				return;
			}
		};

		while let Some(cmd) = self.receiver.recv().await {
			match cmd {
				TransactionCommand::Get { key, response } => {
					let result = self.handle_get(&tx, &key).await;
					let _ = response.send(result);
				}
				TransactionCommand::GetKey {
					key,
					or_equal,
					offset,
					response,
				} => {
					let result = self.handle_get_key(&tx, &key, or_equal, offset).await;
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
					let result = self
						.handle_get_range(
							&tx,
							begin,
							begin_or_equal,
							begin_offset,
							end,
							end_or_equal,
							end_offset,
							limit,
							reverse,
						)
						.await;
					let _ = response.send(result);
				}
				TransactionCommand::Commit {
					operations,
					conflict_ranges,
					response,
				} => {
					// The read snapshot is read-only; release it and submit the commit to the leader.
					let _ = tx.commit().await;
					let result =
						commit::submit(&self.shared, read_version, operations, conflict_ranges)
							.await;
					let _ = response.send(result);
					return;
				}
				TransactionCommand::GetEstimatedRangeSize {
					begin,
					end,
					response,
				} => {
					let result = self
						.handle_get_estimated_range_size(&tx, &begin, &end)
						.await;
					let _ = response.send(result);
				}
			}
		}

		// If the channel is closed, the snapshot transaction is rolled back when dropped.
	}

	async fn handle_get(&mut self, tx: &Transaction<'_>, key: &[u8]) -> Result<Option<Slice>> {
		let query = "SELECT value FROM kv WHERE key = $1";
		let stmt = tx.prepare_cached(query).await.map_err(map_postgres_error)?;

		tx.query_opt(&stmt, &[&key])
			.await
			.map(|row| row.map(|r| r.get::<_, Vec<u8>>(0).into()))
			.map_err(map_postgres_error)
	}

	async fn handle_get_key(
		&mut self,
		tx: &Transaction<'_>,
		key: &[u8],
		or_equal: bool,
		offset: i32,
	) -> Result<Option<Slice>> {
		// Determine selector type and build appropriate query
		let query = match (or_equal, offset) {
			// first_greater_or_equal
			(false, 1) => "SELECT key FROM kv WHERE key >= $1 ORDER BY key LIMIT 1",
			// first_greater_than
			(true, 1) => "SELECT key FROM kv WHERE key > $1 ORDER BY key LIMIT 1",
			// last_less_than
			(false, 0) => "SELECT key FROM kv WHERE key < $1 ORDER BY key DESC LIMIT 1",
			// last_less_or_equal
			(true, 0) => "SELECT key FROM kv WHERE key <= $1 ORDER BY key DESC LIMIT 1",
			_ => bail!("invalid or_equal + offset combo"),
		};

		let stmt = tx.prepare_cached(query).await.map_err(map_postgres_error)?;

		tx.query_opt(&stmt, &[&key])
			.await
			.map(|row| row.map(|r| r.get::<_, Vec<u8>>(0).into()))
			.map_err(map_postgres_error)
	}

	async fn handle_get_range(
		&mut self,
		tx: &Transaction<'_>,
		begin_key: Vec<u8>,
		begin_or_equal: bool,
		begin_offset: i32,
		end_key: Vec<u8>,
		end_or_equal: bool,
		end_offset: i32,
		limit: Option<usize>,
		reverse: bool,
	) -> Result<Values> {
		// Determine SQL operators based on key selector types
		let begin_op = if begin_offset == 1 {
			if begin_or_equal { ">" } else { ">=" }
		} else {
			">="
		};

		let end_op = if end_offset == 1 {
			if end_or_equal { "<=" } else { "<" }
		} else {
			"<"
		};

		let query = if reverse {
			if let Some(limit) = limit {
				format!(
					"SELECT key, value FROM kv WHERE key {begin_op} $1 AND key {end_op} $2 ORDER BY key DESC LIMIT {limit}"
				)
			} else {
				format!(
					"SELECT key, value FROM kv WHERE key {begin_op} $1 AND key {end_op} $2 ORDER BY key DESC"
				)
			}
		} else if let Some(limit) = limit {
			format!(
				"SELECT key, value FROM kv WHERE key {begin_op} $1 AND key {end_op} $2 ORDER BY key LIMIT {limit}"
			)
		} else {
			format!(
				"SELECT key, value FROM kv WHERE key {begin_op} $1 AND key {end_op} $2 ORDER BY key"
			)
		};

		let stmt = tx
			.prepare_cached(&query)
			.await
			.map_err(map_postgres_error)?;

		tx.query(&stmt, &[&begin_key, &end_key])
			.await
			.map(|rows| {
				rows.into_iter()
					.map(|row| {
						let key: Vec<u8> = row.get(0);
						let value: Vec<u8> = row.get(1);
						KeyValue::new(key, value)
					})
					.collect()
			})
			.map(Values::new)
			.map_err(map_postgres_error)
	}

	async fn handle_get_estimated_range_size(
		&mut self,
		tx: &Transaction<'_>,
		begin: &[u8],
		end: &[u8],
	) -> Result<i64> {
		// Sample 1% of the range.
		let query = "
			WITH range_stats AS (
				SELECT
					COUNT(*) as estimated_count,
					COALESCE(SUM(pg_column_size(key) + pg_column_size(value)), 0) as sample_size
				FROM kv TABLESAMPLE SYSTEM(1)
				WHERE key >= $1 AND key < $2
			),
			table_stats AS (
				SELECT reltuples::bigint as total_rows
				FROM pg_class
				WHERE relname = 'kv' AND relkind = 'r'
			)
			SELECT
				CASE
					WHEN r.estimated_count = 0 THEN 0
					ELSE (r.sample_size * 100)::bigint
				END as estimated_size
			FROM range_stats r, table_stats t";
		let stmt = tx.prepare_cached(query).await.map_err(map_postgres_error)?;

		tx.query_opt(&stmt, &[&begin, &end])
			.await
			.map(|row| row.map(|r| r.get::<_, i64>(0)).unwrap_or(0))
			.map_err(map_postgres_error)
	}

	async fn fail_receiver(&mut self) {
		while let Some(cmd) = self.receiver.recv().await {
			match cmd {
				TransactionCommand::Get { response, .. } => {
					let _ = response.send(Err(anyhow!("postgres transaction connection failed")));
				}
				TransactionCommand::GetKey { response, .. } => {
					let _ = response.send(Err(anyhow!("postgres transaction connection failed")));
				}
				TransactionCommand::GetRange { response, .. } => {
					let _ = response.send(Err(anyhow!("postgres transaction connection failed")));
				}
				TransactionCommand::Commit { response, .. } => {
					let _ = response.send(Err(anyhow!("postgres transaction connection failed")));
				}
				TransactionCommand::GetEstimatedRangeSize { response, .. } => {
					let _ = response.send(Err(anyhow!("postgres transaction connection failed")));
				}
			}
		}
	}
}

/// Maps a PostgreSQL error from the read path to a `DatabaseError` where appropriate.
fn map_postgres_error(err: tokio_postgres::Error) -> anyhow::Error {
	let error_str = err.to_string();

	if error_str.contains("serialization failure")
		|| error_str.contains("could not serialize")
		|| error_str.contains("deadlock detected")
	{
		crate::error::DatabaseError::NotCommitted.into()
	} else if error_str.contains("current transaction is aborted") {
		crate::error::DatabaseError::NotCommitted.into()
	} else {
		tracing::error!(%err, "postgres error");
		anyhow::Error::new(err)
	}
}
