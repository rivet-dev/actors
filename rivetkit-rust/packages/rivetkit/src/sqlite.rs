use std::{future::Future, time::Duration};

use anyhow::Result;
use rivetkit_core::{SqliteDb, SqliteTransaction};

#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteTransactionOptions<'a> {
	pub name: Option<&'a str>,
	pub timeout: Option<Duration>,
}

/// Ergonomic commit-on-success transaction helper for the high-level Rust API.
/// Coordination and profiling remain owned by `rivetkit-core`.
pub trait SqliteDbExt {
	fn transaction<'a, T, F, Fut>(
		&'a self,
		callback: F,
		options: SqliteTransactionOptions<'a>,
	) -> impl Future<Output = Result<T>> + Send + 'a
	where
		T: Send + 'a,
		F: FnOnce(SqliteTransaction) -> Fut + Send + 'a,
		Fut: Future<Output = Result<T>> + Send + 'a;
}

impl SqliteDbExt for SqliteDb {
	async fn transaction<'a, T, F, Fut>(
		&'a self,
		callback: F,
		options: SqliteTransactionOptions<'a>,
	) -> Result<T>
	where
		T: Send + 'a,
		F: FnOnce(SqliteTransaction) -> Fut + Send + 'a,
		Fut: Future<Output = Result<T>> + Send + 'a,
	{
		let transaction = self
			.begin_named_transaction(options.name, options.timeout)
			.await?;
		match callback(transaction.clone()).await {
			Ok(value) => {
				if let Err(error) = transaction.commit().await {
					let _ = transaction.rollback().await;
					return Err(error);
				}
				Ok(value)
			}
			Err(error) => {
				let _ = transaction.rollback().await;
				Err(error)
			}
		}
	}
}
