#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
	#[error("transaction not committed due to conflict with another transaction")]
	NotCommitted,

	// TODO: Implement in rocksdb and postgres drivers
	#[error("transaction is too old to perform reads or be committed")]
	TransactionTooOld,

	// Stores the last error. The alternate format prints the whole context chain, so the cause the
	// context names is reported alongside the underlying variant.
	#[error("max number of transaction retries reached, last error: {0:#}")]
	MaxRetriesReached(anyhow::Error),

	#[error("operation issued while a commit was outstanding")]
	UsedDuringCommit,

	#[error("driver does not support a per-transaction retry limit")]
	RetryLimitUnsupported,
}

impl DatabaseError {
	pub fn is_retryable(&self) -> bool {
		use DatabaseError::*;

		match self {
			NotCommitted | TransactionTooOld | MaxRetriesReached(_) => true,
			_ => false,
		}
	}

	pub fn is_maybe_committed(&self) -> bool {
		false
	}
}
