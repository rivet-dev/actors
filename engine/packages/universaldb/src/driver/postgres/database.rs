use std::{
	path::PathBuf,
	sync::{
		Arc,
		atomic::{AtomicI32, Ordering},
	},
	time::Duration,
};

use anyhow::{Context, Result};
use deadpool_postgres::{Config, ManagerConfig, Pool, PoolConfig, RecyclingMethod, Runtime};
use rivet_postgres_util::build_tls_config;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_postgres_rustls::MakeRustlsConnect;
use url::Url;
use uuid::Uuid;

use crate::{
	RetryableTransaction, Transaction,
	driver::{BoxFut, DatabaseDriver, Erased},
	error::DatabaseError,
	metrics,
	transaction::TXN_TIMEOUT,
	utils::{MaybeCommitted, calculate_tx_retry_backoff},
};

use super::{
	nats::{self, NatsConfig, NatsTransport, Subjects},
	resolver::{self, ResolverInput},
	shared::PostgresShared,
	transaction::PostgresTransactionDriver,
	transport::{COMMIT_QUEUE_BOUND, Transport},
};

const GC_INTERVAL: Duration = Duration::from_secs(30);
/// Default size of the follower connection pool, which serves ordinary transactions, dedup GC, and
/// the lease cache refresh.
const DEFAULT_POOL_MAX_SIZE: usize = 64;
/// Size of the leader connection pool. The leader path only ever needs a connection for the drain
/// batch and, concurrently, lease renewal, so two slots reserved away from follower traffic are
/// enough to keep a leader alive under any amount of follower load.
const LEADER_POOL_MAX_SIZE: usize = 2;
/// How often pool occupancy is sampled into the pool gauges.
const POOL_METRICS_INTERVAL: Duration = Duration::from_secs(1);
/// Failover dedup rows older than this are garbage collected. Must be well beyond the longest a
/// follower could spend resending a commit across a leader failover, so a dedup record is never
/// deleted while a resend that needs it could still arrive.
const DEDUP_ROW_MAX_AGE_SECS: i64 = 120;

#[derive(Clone, Debug)]
pub struct PostgresConfig {
	pub connection_string: String,
	pub ssl_config: Option<PostgresSslConfig>,
	/// When set, UniversalDB runs in multi-node mode and uses NATS for follower-to-leader commit
	/// transport. When `None`, it runs single-node with an in-process resolver.
	pub nats: Option<NatsConfig>,
	/// Size of the follower connection pool. Defaults to [`DEFAULT_POOL_MAX_SIZE`]. This is a test
	/// seam for forcing pool exhaustion with a handful of transactions, not an operator knob.
	pub pool_max_size: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct PostgresSslConfig {
	pub ssl_root_cert_path: Option<PathBuf>,
	pub ssl_client_cert_path: Option<PathBuf>,
	pub ssl_client_key_path: Option<PathBuf>,
}

impl PostgresConfig {
	/// Create a new PostgreSQL configuration with sane defaults (single-node).
	pub fn new(connection_string: String) -> Self {
		Self {
			connection_string,
			ssl_config: None,
			nats: None,
			pool_max_size: None,
		}
	}
}

/// Sentinel for "the transaction closure set no per-transaction retry limit", so the database-wide
/// limit applies.
pub const RETRY_LIMIT_UNSET: i32 = -1;

pub struct PostgresDatabaseDriver {
	shared: Arc<PostgresShared>,
	max_retries: AtomicI32,
	resolver_handle: JoinHandle<()>,
	gc_handle: JoinHandle<()>,
	pool_metrics_handle: JoinHandle<()>,
}

impl PostgresDatabaseDriver {
	/// Create a new PostgreSQL driver with custom configuration.
	///
	/// `rivet_config` is read for the negotiated commit protocol version, which the leader and
	/// follower wire formats are encoded at.
	pub async fn new_with_config(
		rivet_config: rivet_config::Config,
		config: PostgresConfig,
	) -> Result<Self> {
		tracing::debug!(
			connection_string = ?config.connection_string,
			"creating PostgresDatabaseDriver"
		);

		let ssl_disabled = if let Ok(url) = Url::parse(&config.connection_string) {
			url.query_pairs()
				.any(|(k, v)| k == "sslmode" && v == "disable")
		} else {
			false
		};

		let pool = Self::build_pool(
			&config,
			ssl_disabled,
			config.pool_max_size.unwrap_or(DEFAULT_POOL_MAX_SIZE),
		)?;
		// The leader path gets its own pool so a leader can always renew its lease and drain the
		// commit queue no matter how saturated the follower pool is.
		let leader_pool = Self::build_pool(&config, ssl_disabled, LEADER_POOL_MAX_SIZE)?;

		// Initialize the schema (idempotent).
		{
			let conn = pool
				.get()
				.await
				.context("failed to get connection from postgres pool")?;
			Self::init_schema(&conn).await?;
		}

		// Unique per-process node id (no hyphens). Names this node's NATS commit subject and is the
		// dedup `client_node_id`.
		let node_id = Uuid::new_v4().simple().to_string();

		let (shared, resolver_input) = match &config.nats {
			None => {
				// Single-node: the follower commit path hands jobs straight to the in-process leader
				// drain loop.
				let (commit_tx, commit_rx) = mpsc::channel(COMMIT_QUEUE_BOUND);
				let shared = PostgresShared::new(
					rivet_config.clone(),
					pool,
					leader_pool,
					node_id,
					Transport::SingleNode { commit_tx },
				);

				// Acquire the leader lease behind the correctness gate BEFORE spawning the resolver so
				// a failure to acquire fails startup loudly.
				let initial_epoch = resolver::acquire_single_node_gate(&shared).await?;

				(
					shared,
					ResolverInput::SingleNode {
						rx: commit_rx,
						initial_epoch,
					},
				)
			}
			Some(nats_config) => {
				// Multi-node: commits travel to the elected leader over NATS request/reply.
				let client = nats::connect(nats_config).await?;
				let subjects = Subjects::new(&config.connection_string);
				let shared = PostgresShared::new(
					rivet_config.clone(),
					pool,
					leader_pool,
					node_id,
					Transport::MultiNode(NatsTransport { client, subjects }),
				);

				(shared, ResolverInput::MultiNode)
			}
		};

		let resolver_handle = resolver::spawn(shared.clone(), resolver_input);
		let gc_handle = Self::spawn_gc(shared.clone());
		let pool_metrics_handle = Self::spawn_pool_metrics(shared.clone());

		Ok(PostgresDatabaseDriver {
			shared,
			max_retries: AtomicI32::new(10),
			resolver_handle,
			gc_handle,
			pool_metrics_handle,
		})
	}

	fn build_pool(config: &PostgresConfig, ssl_disabled: bool, max_size: usize) -> Result<Pool> {
		let mut pool_config = Config::new();
		pool_config.url = Some(config.connection_string.clone());
		pool_config.pool = Some(PoolConfig {
			max_size,
			..Default::default()
		});
		pool_config.manager = Some(ManagerConfig {
			recycling_method: RecyclingMethod::Fast,
		});

		if ssl_disabled {
			pool_config
				.create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls)
				.context("failed to create postgres connection pool")
		} else {
			let tls_config = build_tls_config(
				config
					.ssl_config
					.as_ref()
					.and_then(|c| c.ssl_root_cert_path.as_ref()),
				config
					.ssl_config
					.as_ref()
					.and_then(|c| c.ssl_client_cert_path.as_ref()),
				config
					.ssl_config
					.as_ref()
					.and_then(|c| c.ssl_client_key_path.as_ref()),
			)?;
			pool_config
				.create_pool(Some(Runtime::Tokio1), MakeRustlsConnect::new(tls_config))
				.context("failed to create postgres connection pool")
		}
	}

	async fn init_schema(conn: &deadpool_postgres::Client) -> Result<()> {
		// Durable latest-value store.
		conn.batch_execute(
			"CREATE TABLE IF NOT EXISTS kv (
				key BYTEA PRIMARY KEY,
				value BYTEA NOT NULL
			);

			CREATE TABLE IF NOT EXISTS udb_lease (
				id              INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
				epoch           BIGINT NOT NULL,
				leader_addr     TEXT   NOT NULL,
				durable_version BIGINT NOT NULL DEFAULT 0,
				expires_at      TIMESTAMPTZ NOT NULL
			);

			CREATE SEQUENCE IF NOT EXISTS udb_version_seq AS BIGINT
				START WITH 1 INCREMENT BY 1 MINVALUE 1;

			CREATE TABLE IF NOT EXISTS udb_applied (
				client_node_id BYTEA  NOT NULL,
				client_seq     BIGINT NOT NULL,
				commit_version BIGINT NOT NULL,
				created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
				PRIMARY KEY (client_node_id, client_seq)
			);

			CREATE INDEX IF NOT EXISTS udb_applied_created_at_idx
				ON udb_applied (created_at);",
		)
		.await
		.context("failed to initialize postgres schema")?;

		Ok(())
	}

	/// Garbage-collect old failover dedup records. A dedup row only needs to outlive the longest a
	/// follower could spend resending a single commit across a leader failover, so terminal rows past
	/// [`DEDUP_ROW_MAX_AGE_SECS`] are safe to drop. (Single-node never writes this table.)
	fn spawn_gc(shared: Arc<PostgresShared>) -> JoinHandle<()> {
		tokio::spawn(async move {
			let mut interval = tokio::time::interval(GC_INTERVAL);
			interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

			loop {
				interval.tick().await;

				let conn = match shared.pool.get().await {
					Ok(conn) => conn,
					Err(err) => {
						tracing::debug!(?err, "failed to get connection for dedup gc");
						continue;
					}
				};

				if let Err(err) = conn
					.execute(
						"DELETE FROM udb_applied
						 WHERE created_at < now() - ($1::bigint * interval '1 second')",
						&[&DEDUP_ROW_MAX_AGE_SECS],
					)
					.await
				{
					tracing::error!(?err, "failed postgres dedup gc");
				}
			}
		})
	}

	/// Sample both pools' occupancy into the pool gauges. `available == 0` with `waiting > 0` on the
	/// follower pool is the signature of pool starvation.
	fn spawn_pool_metrics(shared: Arc<PostgresShared>) -> JoinHandle<()> {
		tokio::spawn(async move {
			let mut interval = tokio::time::interval(POOL_METRICS_INTERVAL);
			interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

			loop {
				interval.tick().await;

				for (label, pool) in [("follower", &shared.pool), ("leader", &shared.leader_pool)] {
					let status = pool.status();
					metrics::POSTGRES_POOL_SIZE
						.with_label_values(&[label])
						.set(status.size as i64);
					metrics::POSTGRES_POOL_AVAILABLE
						.with_label_values(&[label])
						.set(status.available as i64);
					metrics::POSTGRES_POOL_WAITING
						.with_label_values(&[label])
						.set(status.waiting as i64);
				}
			}
		})
	}
}

impl DatabaseDriver for PostgresDatabaseDriver {
	fn create_txn(&self) -> Result<Transaction> {
		Ok(Transaction::new(Arc::new(PostgresTransactionDriver::new(
			self.shared.clone(),
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
				let tx = Transaction::new(Arc::new(PostgresTransactionDriver::with_retry_limit(
					self.shared.clone(),
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

	fn shutdown<'a>(&'a self) -> BoxFut<'a, ()> {
		Box::pin(async move {
			// Stop renewing the lease before releasing it so a racing renew cannot re-extend it.
			self.resolver_handle.abort();
			self.gc_handle.abort();
			self.pool_metrics_handle.abort();

			// Hand off leadership immediately if we hold it, instead of waiting out the lease TTL.
			resolver::handoff(&self.shared).await;
		})
	}
}

impl Drop for PostgresDatabaseDriver {
	fn drop(&mut self) {
		// Abort the resolver so a dropped node stops renewing its lease; the lease then expires and
		// another node can take over. Without this a dropped leader would renew its lease forever.
		self.resolver_handle.abort();
		self.gc_handle.abort();
		self.pool_metrics_handle.abort();
	}
}
