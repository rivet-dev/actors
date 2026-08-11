use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use std::{
	future::Future,
	sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::{Context, Result, anyhow};
use futures_util::FutureExt;
use rivet_tracing_utils::CustomInstrumentExt;

use crate::{
	driver::{DatabaseDriverHandle, Erased},
	metrics,
	transaction::{RetryableTransaction, Transaction},
};

/// Returns the simulated latency duration read from UDB_SIMULATED_LATENCY_MS at startup.
fn simulated_latency() -> Option<Duration> {
	static LATENCY: OnceLock<Option<Duration>> = OnceLock::new();
	*LATENCY.get_or_init(|| {
		let ms: u64 = std::env::var("UDB_SIMULATED_LATENCY_MS")
			.ok()?
			.parse()
			.ok()?;
		if ms == 0 {
			return None;
		}
		tracing::debug!(latency_ms = ms, "udb simulated latency enabled");
		Some(Duration::from_millis(ms))
	})
}

#[derive(Clone)]
pub struct Database {
	driver: DatabaseDriverHandle,
}

impl Database {
	pub fn new(driver: DatabaseDriverHandle) -> Self {
		Database { driver }
	}

	/// Run a closure with automatic retry logic and a name.
	#[tracing::instrument(skip_all)]
	pub async fn txn<'a, F, Fut, T>(&'a self, name: &'static str, closure: F) -> Result<T>
	where
		F: Fn(RetryableTransaction) -> Fut + Send + Sync,
		Fut: Future<Output = Result<T>> + Send,
		T: Send + 'a + 'static,
	{
		if let Some(delay) = simulated_latency() {
			tokio::time::sleep(delay).await;
		}

		let start = Instant::now();
		let attempts = AtomicUsize::new(0);
		metrics::TRANSACTION_TOTAL.with_label_values(&[name]).inc();
		metrics::TRANSACTION_PENDING
			.with_label_values(&[name])
			.inc();

		let closure = &closure;
		let res = self
			.driver
			.run(Box::new(|tx| {
				let tx = tx.with_name(name);
				async move { closure(tx).await.map(|value| Box::new(value) as Erased) }
					.custom_instrument(tracing::info_span!("txn_attempt"))
					.boxed()
			}))
			.await
			.and_then(|res| {
				res.downcast::<T>()
					.map(|x| *x)
					.map_err(|_| anyhow!("failed to downcast `run` return type"))
			})
			.context("transaction failed");

		let final_attempts = attempts.load(Ordering::Acquire);
		let duration = start.elapsed();
		metrics::TRANSACTION_ATTEMPTS
			.with_label_values(&[name])
			.observe(final_attempts as f64);
		metrics::TRANSACTION_PENDING
			.with_label_values(&[name])
			.dec();
		metrics::TRANSACTION_DURATION
			.with_label_values(&[name])
			.observe(duration.as_secs_f64());

		res
	}

	/// Creates a new txn instance.
	pub fn create_txn(&self) -> Result<Transaction> {
		self.driver.create_txn()
	}

	pub fn txn_retry_limit(&self, limit: i32) -> Result<()> {
		self.driver.txn_retry_limit(limit)
	}

	/// Create a consistent point-in-time snapshot of the database at the given path.
	pub fn checkpoint(&self, path: &Path) -> Result<()> {
		self.driver.checkpoint(path)
	}

	/// Gracefully release process-wide driver resources before shutdown.
	pub async fn shutdown(&self) {
		self.driver.shutdown().await;
	}
}
