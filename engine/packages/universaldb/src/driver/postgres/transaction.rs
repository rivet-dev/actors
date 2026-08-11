use std::{
	future::Future,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use anyhow::{Context, Result};
use tokio::sync::{OnceCell, mpsc, oneshot};

use crate::{
	driver::TransactionDriver,
	key_selector::KeySelector,
	options::{ConflictRangeType, MutationType},
	range_option::RangeOption,
	tx_ops::TransactionOperations,
	utils::IsolationLevel,
	value::{Slice, Value, Values},
};

use super::{
	shared::PostgresShared,
	transaction_task::{TransactionCommand, TransactionTask},
};

pub struct PostgresTransactionDriver {
	shared: Arc<PostgresShared>,
	operations: TransactionOperations,
	committed: AtomicBool,
	tx_sender: OnceCell<mpsc::UnboundedSender<TransactionCommand>>,
}

impl PostgresTransactionDriver {
	pub fn new(shared: Arc<PostgresShared>) -> Self {
		PostgresTransactionDriver {
			shared,
			operations: TransactionOperations::default(),
			committed: AtomicBool::new(false),
			tx_sender: OnceCell::new(),
		}
	}

	/// Get or create the transaction task that owns this transaction's read snapshot.
	async fn ensure_transaction(&self) -> Result<&mpsc::UnboundedSender<TransactionCommand>> {
		self.tx_sender
			.get_or_try_init(|| async {
				let (sender, receiver) = mpsc::unbounded_channel();

				let task = TransactionTask::new(self.shared.clone(), receiver);
				tokio::spawn(task.run());

				anyhow::Ok(sender)
			})
			.await
			.context("failed to initialize postgres transaction task")
	}
}

impl TransactionDriver for PostgresTransactionDriver {
	fn atomic_op(&self, key: &[u8], param: &[u8], op_type: MutationType) {
		self.operations.atomic_op(key, param, op_type);
	}

	fn get<'a>(
		&'a self,
		key: &[u8],
		isolation_level: IsolationLevel,
	) -> Pin<Box<dyn Future<Output = Result<Option<Slice>>> + Send + 'a>> {
		let key = key.to_vec();

		Box::pin(async move {
			self.operations
				.get_with_callback(&key, isolation_level, || async {
					let tx_sender = self.ensure_transaction().await?;

					// Send query command
					let (response_tx, response_rx) = oneshot::channel();
					tx_sender
						.send(TransactionCommand::Get {
							key: key.clone(),
							response: response_tx,
						})
						.context("failed to send postgres transaction command")?;

					// Wait for response
					response_rx
						.await
						.context("failed to receive postgres response")?
				})
				.await
		})
	}

	fn get_key<'a>(
		&'a self,
		selector: &KeySelector<'a>,
		isolation_level: IsolationLevel,
	) -> Pin<Box<dyn Future<Output = Result<Slice>> + Send + 'a>> {
		let selector = selector.clone();

		Box::pin(async move {
			let key = selector.key().to_vec();
			let offset = selector.offset();
			let or_equal = selector.or_equal();

			self.operations
				.get_key(&selector, isolation_level, || async {
					let tx_sender = self.ensure_transaction().await?;

					// Send query command
					let (response_tx, response_rx) = oneshot::channel();
					tx_sender
						.send(TransactionCommand::GetKey {
							key: key.clone(),
							or_equal,
							offset,
							response: response_tx,
						})
						.context("failed to send postgres transaction command")?;

					// Wait for response
					let result_key = response_rx
						.await
						.context("failed to receive postgres key selector response")??;

					// Return the key if found, or empty vector if not
					Ok(result_key.unwrap_or_else(Slice::new))
				})
				.await
		})
	}

	fn get_range<'a>(
		&'a self,
		opt: &RangeOption<'a>,
		_iteration: usize,
		isolation_level: IsolationLevel,
	) -> Pin<Box<dyn Future<Output = Result<Values>> + Send + 'a>> {
		let opt = opt.clone();

		Box::pin(async move {
			let begin = opt.begin.key().to_vec();
			let begin_or_equal = opt.begin.or_equal();
			let begin_offset = opt.begin.offset();
			let end = opt.end.key().to_vec();
			let end_or_equal = opt.end.or_equal();
			let end_offset = opt.end.offset();
			let limit = opt.limit;
			let reverse = opt.reverse;

			self.operations
				.get_range(&opt, isolation_level, || async {
					let tx_sender = self.ensure_transaction().await?;

					// Send query command
					let (response_tx, response_rx) = oneshot::channel();
					tx_sender
						.send(TransactionCommand::GetRange {
							begin: begin.clone(),
							begin_or_equal,
							begin_offset,
							end: end.clone(),
							end_or_equal,
							end_offset,
							limit,
							reverse,
							response: response_tx,
						})
						.context("failed to send postgres transaction command")?;

					// Wait for response
					response_rx
						.await
						.context("failed to receive postgres range response")?
				})
				.await
		})
	}

	fn get_ranges_keyvalues<'a>(
		&'a self,
		opt: RangeOption<'a>,
		isolation_level: IsolationLevel,
	) -> crate::value::Stream<'a, Value> {
		use futures_util::{StreamExt, stream};

		// Convert the range result into a stream
		let fut = async move {
			match self.get_range(&opt, 1, isolation_level).await {
				Ok(values) => values
					.into_iter()
					.map(|kv| Ok(Value::from_keyvalue(kv)))
					.collect::<Vec<_>>(),
				Err(e) => vec![Err(e)],
			}
		};

		Box::pin(stream::once(fut).flat_map(stream::iter))
	}

	fn set(&self, key: &[u8], value: &[u8]) {
		self.operations.set(key, value);
	}

	fn clear(&self, key: &[u8]) {
		self.operations.clear(key);
	}

	fn clear_range(&self, begin: &[u8], end: &[u8]) {
		self.operations.clear_range(begin, end);
	}

	fn commit(self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
		Box::pin(async move {
			if self.committed.load(Ordering::SeqCst) {
				return Ok(());
			}
			self.committed.store(true, Ordering::SeqCst);

			let (operations, conflict_ranges) = self.operations.consume();

			let tx_sender = self.ensure_transaction().await?;

			// Send commit command
			let (response_tx, response_rx) = oneshot::channel();
			tx_sender
				.send(TransactionCommand::Commit {
					operations,
					conflict_ranges,
					response: response_tx,
				})
				.context("failed to send postgres transaction command")?;

			// Wait for commit response
			response_rx
				.await
				.context("failed to receive postgres commit response")??;

			Ok(())
		})
	}

	fn reset(&mut self) {
		self.operations.clear_all();
		self.committed.store(false, Ordering::SeqCst);

		// Replace tx sender to get a new txn version
		self.tx_sender = OnceCell::new();
	}

	fn cancel(&self) {
		self.operations.clear_all();
		self.committed.store(true, Ordering::SeqCst); // Prevent future commits

		// Transaction will be rolled back when dropped
	}

	fn add_conflict_range(
		&self,
		begin: &[u8],
		end: &[u8],
		conflict_type: ConflictRangeType,
	) -> Result<()> {
		self.operations
			.add_conflict_range(begin, end, conflict_type);

		Ok(())
	}

	fn get_estimated_range_size_bytes<'a>(
		&'a self,
		begin: &'a [u8],
		end: &'a [u8],
	) -> Pin<Box<dyn Future<Output = Result<i64>> + Send + 'a>> {
		let begin = begin.to_vec();
		let end = end.to_vec();

		Box::pin(async move {
			let tx_sender = self.ensure_transaction().await?;

			// Send query command
			let (response_tx, response_rx) = oneshot::channel();
			tx_sender
				.send(TransactionCommand::GetEstimatedRangeSize {
					begin,
					end,
					response: response_tx,
				})
				.context("failed to send postgres command")?;

			// Wait for response
			let size = response_rx
				.await
				.context("failed to receive postgres size response")??;

			Ok(size)
		})
	}

	fn commit_ref(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
		Box::pin(async move {
			if self.committed.load(Ordering::SeqCst) {
				return Ok(());
			}
			self.committed.store(true, Ordering::SeqCst);

			let (operations, conflict_ranges) = self.operations.consume();

			// We have operations but no transaction - create one just for commit
			let tx_sender = self.ensure_transaction().await?;

			// Send commit command
			let (response_tx, response_rx) = oneshot::channel();
			tx_sender
				.send(TransactionCommand::Commit {
					operations,
					conflict_ranges,
					response: response_tx,
				})
				.context("failed to send postgres transaction command")?;

			// Wait for commit response
			response_rx
				.await
				.context("failed to receive postgres commit response")??;

			Ok(())
		})
	}
}
