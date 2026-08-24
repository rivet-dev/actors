use std::time::{SystemTime, UNIX_EPOCH};

use rivet_util::Id;
use tokio::time::Instant;

use crate::ctx::common::RETRY_TIMEOUT_MS;

pub type WorkflowResult<T> = Result<T, WorkflowError>;

#[derive(thiserror::Error, Debug)]
pub enum WorkflowError {
	#[error("workflow {0} failed: {1:?}")]
	WorkflowFailure(&'static str, #[source] anyhow::Error),

	// Includes error count
	#[error("activity {0} failed: {1:?}")]
	ActivityFailure(&'static str, #[source] anyhow::Error, usize),

	#[error("activity {0} failed, max retries reached: {1:?}")]
	ActivityMaxFailuresReached(&'static str, #[source] anyhow::Error),

	#[error("operation {0} failed: {1:?}")]
	OperationFailure(&'static str, #[source] anyhow::Error),

	#[error("workflow missing from registry: {0}")]
	WorkflowMissingFromRegistry(String),

	#[error("workflow not found")]
	WorkflowNotFound,

	#[error("workflow evicted")]
	WorkflowEvicted,

	#[error("history diverged: {0}")]
	HistoryDiverged(String),

	#[error("latent history found: {0}")]
	LatentHistoryFound(String),

	#[error("serialize workflow input: {0}")]
	SerializeWorkflowInput(#[source] serde_json::Error),

	#[error("deserialize workflow input: {0}")]
	DeserializeWorkflowInput(#[source] serde_json::Error),

	#[error("serialize workflow output: {0}")]
	SerializeWorkflowOutput(#[source] serde_json::Error),

	#[error("deserialize workflow output: {0}")]
	DeserializeWorkflowOutput(#[source] serde_json::Error),

	#[error("deserialize workflow state: {0}")]
	DeserializeWorkflowState(#[source] serde_json::Error),

	#[error("state inaccessible: {0}")]
	WorkflowStateInaccessible(&'static str),

	#[error("serialize activity input: {0}")]
	SerializeActivityInput(#[source] serde_json::Error),

	#[error("serialize activity output: {0}")]
	SerializeActivityOutput(#[source] serde_json::Error),

	#[error("deserialize activity output: {0}")]
	DeserializeActivityOutput(#[source] serde_json::Error),

	#[error("serialize signal body: {0}")]
	SerializeSignalBody(#[source] serde_json::Error),

	#[error("deserialize signal body: {0}")]
	DeserializeSignalBody(#[source] serde_json::Error),

	#[error("serialize message body: {0}")]
	SerializeMessageBody(#[source] serde_json::Error),

	#[error("deserialize message body: {0}")]
	DeserializeMessageBody(#[source] serde_json::Error),

	#[error("serialize message: {0}")]
	SerializeMessage(#[source] serde_json::Error),

	#[error("deserialize message: {0}")]
	DeserializeMessage(#[source] serde_json::Error),

	#[error("failed to serialize cjson tags: {0:?}")]
	CjsonSerializeTags(cjson::Error),

	#[error("failed to serialize tags: {0:?}")]
	SerializeTags(#[source] serde_json::Error),

	#[error("failed to deserialize tags: {0}")]
	DeserializeTags(#[source] serde_json::Error),

	#[error("invalid tags: {0}")]
	InvalidTags(String),

	#[error("failed to serialize loop state: {0}")]
	SerializeLoopState(#[source] serde_json::Error),

	#[error("failed to deserialize loop state: {0}")]
	DeserializeLoopState(#[source] serde_json::Error),

	#[error("failed to serialize loop output: {0}")]
	SerializeLoopOutput(#[source] serde_json::Error),

	#[error("failed to deserialize loop output: {0}")]
	DeserializeLoopOutput(#[source] serde_json::Error),

	#[error("failed to create subscription: {0}")]
	CreateSubscription(#[source] anyhow::Error),

	#[error("failed to flush pubsub: {0}")]
	FlushPubsub(#[source] anyhow::Error),

	#[error("failed to publish message: {0}")]
	PublishMessage(#[source] anyhow::Error),

	#[error("subscription unsubscribed")]
	SubscriptionUnsubscribed,

	#[error("missing message data")]
	MissingMessageData,

	#[error("no signal found: {0:?}")]
	NoSignalFound(Box<[&'static str]>),

	#[error("sub workflow incomplete: {0:?}")]
	SubWorkflowIncomplete(Id),

	#[error("integer conversion failed")]
	IntegerConversion,

	#[error("missing event data: {0}")]
	MissingEventData(&'static str),

	#[error("failed to deserialize event data: {0}")]
	DeserializeEventData(#[source] anyhow::Error),

	#[error("udb error: {0}")]
	Udb(#[source] anyhow::Error),

	#[error("pools error: {0}")]
	Pools(#[from] rivet_pools::Error),

	#[error("pools error: {0}")]
	PoolsGeneric(#[source] anyhow::Error),

	#[error("config error: {0}")]
	Config(#[source] anyhow::Error),

	// Includes error count
	#[error("activity {0} timed out")]
	ActivityTimeout(&'static str, usize),

	// Includes error count
	#[error("operation {0} timed out")]
	OperationTimeout(&'static str, usize),

	#[error("duplicate registered workflow: {0}")]
	DuplicateRegisteredWorkflow(String),

	#[error("sleeping until {0}")]
	Sleep(i64),

	#[error("no signal found: {0:?}. sleeping until {1}")]
	NoSignalFoundAndSleep(Box<[&'static str]>, i64),

	#[error("`ListenCtx` has already been used once (`listen_any` called)")]
	ListenCtxUsed,

	#[error("int conversion error: {0}")]
	TryFromIntError(#[from] std::num::TryFromIntError),

	#[error("failed to serialize location: {0}")]
	SerializeLocation(#[source] serde_json::Error),

	#[error("invalid version: {0}")]
	InvalidVersion(String),

	#[error("flush channel closed")]
	FlushChannelClosed,
}

impl WorkflowError {
	pub(crate) fn wake_immediate(&self) -> bool {
		matches!(self, WorkflowError::WorkflowEvicted)
	}

	/// Returns the next deadline for a workflow to be woken up again based on the error.
	pub(crate) fn deadline_ts(&self) -> Option<i64> {
		match self {
			WorkflowError::ActivityFailure(_, _, error_count)
			| WorkflowError::ActivityTimeout(_, error_count)
			| WorkflowError::OperationTimeout(_, error_count) => {
				// NOTE: Max retry is handled in `WorkflowCtx::activity`
				let mut backoff = rivet_util::throttle::Backoff::new_at(
					8,
					None,
					RETRY_TIMEOUT_MS,
					500,
					*error_count,
				);
				let next = backoff.step().expect("should not have max retry");

				// Calculate timestamp based on the backoff
				let duration_until = next.duration_since(Instant::now());
				let deadline_ts = (SystemTime::now() + duration_until)
					.duration_since(UNIX_EPOCH)
					.unwrap_or_else(|err| unreachable!("time is broken: {}", err))
					.as_millis()
					.try_into()
					.expect("doesn't fit in i64");

				Some(deadline_ts)
			}
			WorkflowError::Sleep(ts) | WorkflowError::NoSignalFoundAndSleep(_, ts) => Some(*ts),
			// Always retry udb errors, they're usually caused by ephemeral outages and can be retried
			WorkflowError::Udb(_) => Some(rivet_util::timestamp::now() + 30_000),
			_ => None,
		}
	}

	/// Any error that the workflow can continue on with its execution from.
	pub(crate) fn is_recoverable(&self) -> bool {
		match self {
			WorkflowError::ActivityFailure(_, _, _)
			| WorkflowError::ActivityTimeout(_, _)
			| WorkflowError::OperationTimeout(_, _)
			| WorkflowError::NoSignalFound(_)
			| WorkflowError::NoSignalFoundAndSleep(_, _)
			| WorkflowError::SubWorkflowIncomplete(_)
			| WorkflowError::Sleep(_)
			| WorkflowError::WorkflowEvicted => true,
			_ => false,
		}
	}

	/// Any error that the workflow can try again on a fixed number of times. Only used for printing.
	pub(crate) fn is_retryable(&self) -> bool {
		match self {
			WorkflowError::ActivityFailure(_, _, _)
			| WorkflowError::ActivityTimeout(_, _)
			| WorkflowError::OperationTimeout(_, _) => true,
			_ => false,
		}
	}

	pub(crate) fn signals(&self) -> &[&'static str] {
		match self {
			WorkflowError::NoSignalFound(signals)
			| WorkflowError::NoSignalFoundAndSleep(signals, _) => signals,
			_ => &[],
		}
	}

	pub(crate) fn sub_workflow(&self) -> Option<Id> {
		if let WorkflowError::SubWorkflowIncomplete(sub_workflow_id) = self {
			Some(*sub_workflow_id)
		} else {
			None
		}
	}
}
