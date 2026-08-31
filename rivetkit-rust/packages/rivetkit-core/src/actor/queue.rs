use std::collections::BTreeSet;
use std::fmt;
use std::future::pending;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::time::{Instant, SystemTime, UNIX_EPOCH, sleep};

use anyhow::{Context, Result};
use rivet_error::RivetError;
use rivetkit_actor_persist::{generated::v4 as persist_v4, versioned as persist_versioned};
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use tokio::runtime::{Builder, Handle};
use tokio::sync::{futures::Notified, oneshot};
use tokio_util::sync::CancellationToken;

use crate::actor::config::ActorConfig;
use crate::actor::context::ActorContext;
use crate::actor::internal_storage;
use crate::actor::persist::{
	decode_latest_with_embedded_version, encode_latest_with_embedded_version,
};
use crate::actor::task_types::UserTaskKind;
#[cfg(target_arch = "wasm32")]
use crate::error::ActorRuntime;

#[derive(Clone, Debug, Default)]
pub struct QueueNextOpts {
	pub names: Option<Vec<String>>,
	pub timeout: Option<Duration>,
	pub signal: Option<CancellationToken>,
	pub completable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct QueueWaitOpts {
	pub timeout: Option<Duration>,
	pub signal: Option<CancellationToken>,
	pub completable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EnqueueAndWaitOpts {
	pub timeout: Option<Duration>,
	pub signal: Option<CancellationToken>,
}

#[derive(Clone, Debug)]
pub struct QueueNextBatchOpts {
	pub names: Option<Vec<String>>,
	pub count: u32,
	pub timeout: Option<Duration>,
	pub signal: Option<CancellationToken>,
	pub completable: bool,
}

impl Default for QueueNextBatchOpts {
	fn default() -> Self {
		Self {
			names: None,
			count: 1,
			timeout: None,
			signal: None,
			completable: false,
		}
	}
}

#[derive(Clone, Debug, Default)]
pub struct QueueTryNextOpts {
	pub names: Option<Vec<String>>,
	pub completable: bool,
}

#[derive(Clone, Debug)]
pub struct QueueTryNextBatchOpts {
	pub names: Option<Vec<String>>,
	pub count: u32,
	pub completable: bool,
}

impl Default for QueueTryNextBatchOpts {
	fn default() -> Self {
		Self {
			names: None,
			count: 1,
			completable: false,
		}
	}
}

pub(super) type QueueWaitActivityCallback = Arc<dyn Fn() + Send + Sync>;
pub(super) type QueueInspectorUpdateCallback = Arc<dyn Fn(u32) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct QueueMessage {
	pub id: u64,
	pub name: String,
	pub body: Vec<u8>,
	pub created_at: i64,
	completion: Option<CompletionHandle>,
}

#[derive(Clone, Debug)]
pub struct CompletableQueueMessage {
	pub id: u64,
	pub name: String,
	pub body: Vec<u8>,
	pub created_at: i64,
	completion: CompletionHandle,
}

#[derive(Clone)]
struct CompletionHandle(Arc<CompletionHandleInner>);

struct CompletionHandleInner {
	ctx: ActorContext,
	message_id: u64,
	completed: std::sync::atomic::AtomicBool,
}

pub(crate) type QueueMetadata = persist_v4::QueueMetadata;
pub(crate) type PersistedQueueMessage = persist_v4::QueueMessage;

#[cfg(test)]
pub(crate) fn encode_queue_metadata(metadata: &QueueMetadata) -> Result<Vec<u8>> {
	encode_latest_with_embedded_version::<persist_versioned::QueueMetadata>(
		metadata.clone(),
		rivetkit_actor_persist::CURRENT_VERSION,
		"queue metadata",
	)
}

pub(crate) fn decode_queue_metadata(payload: &[u8]) -> Result<QueueMetadata> {
	let metadata = decode_latest_with_embedded_version::<persist_versioned::QueueMetadata>(
		payload,
		"queue metadata",
	)?;
	Ok(metadata)
}

pub(crate) fn encode_queue_message(message: &PersistedQueueMessage) -> Result<Vec<u8>> {
	encode_latest_with_embedded_version::<persist_versioned::QueueMessage>(
		message.clone(),
		rivetkit_actor_persist::CURRENT_VERSION,
		"queue message",
	)
}

pub(crate) fn decode_queue_message(payload: &[u8]) -> Result<PersistedQueueMessage> {
	let message = decode_latest_with_embedded_version::<persist_versioned::QueueMessage>(
		payload,
		"queue message",
	)?;
	Ok(message)
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"queue",
	"full",
	"Queue is full",
	"Queue is full. Limit is {limit} messages."
)]
struct QueueFull {
	limit: u32,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"queue",
	"message_too_large",
	"Queue message is too large",
	"Queue message too large ({size} bytes). Limit is {limit} bytes."
)]
struct QueueMessageTooLarge {
	size: usize,
	limit: u32,
}

#[derive(RivetError)]
#[error("queue", "already_completed", "Queue message was already completed")]
struct QueueAlreadyCompleted;

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"queue",
	"complete_not_configured",
	"Queue message does not support completion",
	"Queue '{name}' does not support completion responses."
)]
struct QueueCompleteNotConfigured {
	name: String,
}
#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"queue",
	"message_identity_mismatch",
	"Queue message identity does not match",
	"Queue message {message_id} is named '{actual_name}', not '{expected_name}'."
)]
struct QueueMessageIdentityMismatch {
	message_id: u64,
	expected_name: String,
	actual_name: String,
}

#[derive(RivetError)]
#[error("actor", "aborted", "Actor aborted")]
struct QueueActorAborted;

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"queue",
	"timed_out",
	"Queue wait timed out",
	"Queue wait timed out after {timeout_ms} ms."
)]
struct QueueWaitTimedOut {
	timeout_ms: u64,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"queue",
	"completion_waiter_conflict",
	"Queue completion waiter conflict",
	"Queue completion waiter is already registered for message {message_id}."
)]
struct QueueCompletionWaiterConflict {
	message_id: u64,
}

#[derive(RivetError)]
#[error(
	"queue",
	"completion_waiter_dropped",
	"Queue completion waiter dropped before response"
)]
struct QueueCompletionWaiterDropped;

impl ActorContext {
	pub async fn send(&self, name: &str, body: &[u8]) -> Result<QueueMessage> {
		self.enqueue_message(name, body, None).await
	}

	pub async fn enqueue_and_wait(
		&self,
		name: &str,
		body: &[u8],
		opts: EnqueueAndWaitOpts,
	) -> Result<Option<Vec<u8>>> {
		let (sender, receiver) = oneshot::channel();
		let message = self.enqueue_message(name, body, Some(sender)).await?;
		let result = self
			.wait_for_completion_response(message.id, receiver, opts.timeout, opts.signal.as_ref())
			.await;
		self.remove_completion_waiter(message.id).await;
		result
	}

	async fn enqueue_message(
		&self,
		name: &str,
		body: &[u8],
		completion_waiter: Option<oneshot::Sender<Option<Vec<u8>>>>,
	) -> Result<QueueMessage> {
		self.ensure_initialized().await?;

		let created_at = current_timestamp_ms()?;
		let persisted = PersistedQueueMessage {
			name: name.to_owned(),
			body: body.to_vec(),
			created_at,
			failure_count: None,
			available_at: None,
			in_flight: None,
			in_flight_at: None,
		};
		let encoded_message = encode_queue_message(&persisted).context("encode queue message")?;

		let config = self.config();
		if encoded_message.len() > config.max_queue_message_size as usize {
			return Err(QueueMessageTooLarge {
				size: encoded_message.len(),
				limit: config.max_queue_message_size,
			}
			.build());
		}

		let mut metadata = self.0.queue_metadata.lock().await;
		if metadata.size >= config.max_queue_size {
			return Err(QueueFull {
				limit: config.max_queue_size,
			}
			.build());
		}

		let id = if metadata.next_id == 0 {
			1
		} else {
			metadata.next_id
		};
		metadata.next_id = id.saturating_add(1);
		metadata.size = metadata.size.saturating_add(1);
		let registered_completion_waiter = if let Some(waiter) = completion_waiter {
			if self
				.0
				.queue_completion_waiters
				.insert_async(id, waiter)
				.await
				.is_err()
			{
				metadata.next_id = id;
				metadata.size = metadata.size.saturating_sub(1);
				return Err(QueueCompletionWaiterConflict { message_id: id }.build());
			}
			true
		} else {
			false
		};

		let persist_result =
			internal_storage::persist_queue_message(self.sql(), id, metadata.next_id, &persisted)
				.await;

		if let Err(error) = persist_result {
			metadata.next_id = id;
			metadata.size = metadata.size.saturating_sub(1);
			if registered_completion_waiter {
				self.remove_completion_waiter(id).await;
			}
			return Err(error).context("persist queue message");
		}

		let queue_size = metadata.size;
		drop(metadata);
		self.0.metrics.add_queue_messages_sent(1);
		self.0
			.metrics
			.set_queue_depth(self.0.queue_metadata.lock().await.size);
		self.notify_inspector_update(queue_size);
		self.0.queue_notify.notify_waiters();

		Ok(QueueMessage {
			id,
			name: name.to_owned(),
			body: body.to_vec(),
			created_at,
			completion: None,
		})
	}

	pub async fn next(&self, opts: QueueNextOpts) -> Result<Option<QueueMessage>> {
		let mut messages = self
			.next_batch(QueueNextBatchOpts {
				names: opts.names,
				count: 1,
				timeout: opts.timeout,
				signal: opts.signal,
				completable: opts.completable,
			})
			.await?;
		Ok(messages.pop())
	}

	pub async fn next_batch(&self, opts: QueueNextBatchOpts) -> Result<Vec<QueueMessage>> {
		self.ensure_initialized().await?;

		let count = opts.count.max(1);
		let deadline = opts.timeout.map(|timeout| Instant::now() + timeout);
		let names = normalize_names(opts.names);

		loop {
			let notified = self.0.queue_notify.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();
			let messages = self
				.try_receive_batch(names.as_ref(), count, opts.completable)
				.await?;
			if !messages.is_empty() {
				return Ok(messages);
			}

			let remaining_timeout =
				deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
			if matches!(remaining_timeout, Some(timeout) if timeout.is_zero()) {
				return Ok(Vec::new());
			}

			let wait_guard = ActiveQueueWaitGuard::new(self);
			let result = self
				.wait_for_message(notified.as_mut(), remaining_timeout, opts.signal.as_ref())
				.await;
			drop(wait_guard);

			match result {
				WaitOutcome::Notified => continue,
				WaitOutcome::TimedOut => return Ok(Vec::new()),
				WaitOutcome::Aborted => return Err(QueueActorAborted.build()),
			}
		}
	}

	pub async fn wait_for_names(
		&self,
		names: Vec<String>,
		opts: QueueWaitOpts,
	) -> Result<QueueMessage> {
		self.ensure_initialized().await?;

		let deadline = opts.timeout.map(|timeout| Instant::now() + timeout);
		let names = normalize_names(Some(names));

		loop {
			let notified = self.0.queue_notify.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();
			if let Some(message) = self
				.try_receive_batch(names.as_ref(), 1, opts.completable)
				.await?
				.into_iter()
				.next()
			{
				return Ok(message);
			}

			let remaining_timeout =
				deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
			if let Some(timeout) = remaining_timeout
				&& timeout.is_zero()
			{
				return Err(QueueWaitTimedOut {
					timeout_ms: opts.timeout.map(duration_ms).unwrap_or(0),
				}
				.build());
			}

			let wait_guard = ActiveQueueWaitGuard::new(self);
			let result = self
				.wait_for_message(notified.as_mut(), remaining_timeout, opts.signal.as_ref())
				.await;
			drop(wait_guard);

			match result {
				WaitOutcome::Notified => continue,
				WaitOutcome::TimedOut => {
					return Err(QueueWaitTimedOut {
						timeout_ms: opts.timeout.map(duration_ms).unwrap_or(0),
					}
					.build());
				}
				WaitOutcome::Aborted => return Err(QueueActorAborted.build()),
			}
		}
	}

	pub async fn wait_for_names_available(
		&self,
		names: Vec<String>,
		opts: QueueWaitOpts,
	) -> Result<()> {
		self.ensure_initialized().await?;

		let deadline = opts.timeout.map(|timeout| Instant::now() + timeout);
		let names = normalize_names(Some(names));

		loop {
			let notified = self.0.queue_notify.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();
			if internal_storage::has_queue_messages(self.sql(), names.as_ref())
				.await
				.context("check for matching sqlite queue messages")?
			{
				return Ok(());
			}

			let remaining_timeout =
				deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
			if let Some(timeout) = remaining_timeout
				&& timeout.is_zero()
			{
				return Err(QueueWaitTimedOut {
					timeout_ms: opts.timeout.map(duration_ms).unwrap_or(0),
				}
				.build());
			}

			let wait_guard = ActiveQueueWaitGuard::new(self);
			let result = self
				.wait_for_message(notified.as_mut(), remaining_timeout, opts.signal.as_ref())
				.await;
			drop(wait_guard);

			match result {
				WaitOutcome::Notified => continue,
				WaitOutcome::TimedOut => {
					return Err(QueueWaitTimedOut {
						timeout_ms: opts.timeout.map(duration_ms).unwrap_or(0),
					}
					.build());
				}
				WaitOutcome::Aborted => return Err(QueueActorAborted.build()),
			}
		}
	}
	/// Completes a persisted message by durable identity. Missing or already
	/// completed IDs are idempotent; an expected-name mismatch leaves the row
	/// untouched.
	pub async fn complete_persisted_message(
		&self,
		message_id: u64,
		expected_name: &str,
		response: Option<Vec<u8>>,
	) -> Result<bool> {
		self.ensure_initialized().await?;
		let _receive_guard = self.0.queue_receive_lock.lock().await;
		let Some(_) = self
			.verify_persisted_message_identity_unlocked(message_id, expected_name)
			.await?
		else {
			return Ok(false);
		};
		self.complete_message_by_id_unlocked(message_id, response)
			.await?;
		Ok(true)
	}

	async fn verify_persisted_message_identity_unlocked(
		&self,
		message_id: u64,
		expected_name: &str,
	) -> Result<Option<String>> {
		let Some(actual_name) =
			internal_storage::load_queue_message_name(self.sql(), message_id).await?
		else {
			return Ok(None);
		};
		if actual_name != expected_name {
			return Err(QueueMessageIdentityMismatch {
				message_id,
				expected_name: expected_name.to_owned(),
				actual_name: actual_name.clone(),
			}
			.build());
		}
		Ok(Some(actual_name))
	}

	pub fn try_next(&self, opts: QueueTryNextOpts) -> Result<Option<QueueMessage>> {
		let mut messages = self.try_next_batch(QueueTryNextBatchOpts {
			names: opts.names,
			count: 1,
			completable: opts.completable,
		})?;
		Ok(messages.pop())
	}

	pub fn try_next_batch(&self, opts: QueueTryNextBatchOpts) -> Result<Vec<QueueMessage>> {
		self.block_on(async {
			self.ensure_initialized().await?;
			self.try_receive_batch(
				normalize_names(opts.names).as_ref(),
				opts.count.max(1),
				opts.completable,
			)
			.await
		})
	}

	pub async fn inspect_messages(&self) -> Result<Vec<QueueMessage>> {
		self.ensure_initialized().await?;
		self.list_messages().await
	}

	pub fn max_size(&self) -> u32 {
		self.config().max_queue_size
	}

	/// Removes all messages from the queue and resets the size counter.
	pub async fn reset(&self) -> Result<()> {
		self.ensure_initialized().await?;

		// Serialize against receivers before touching metadata. Lock order matches
		// try_receive_batch (receive lock then metadata) so there is no deadlock.
		let _receive_guard = self.0.queue_receive_lock.lock().await;

		let mut metadata = self.0.queue_metadata.lock().await;

		internal_storage::reset_queue(self.sql())
			.await
			.context("delete all sqlite queue messages")?;

		metadata.size = 0;

		self.0.queue_completion_waiters.clear_async().await;

		drop(metadata);

		self.0.metrics.set_queue_depth(0);
		self.notify_inspector_update(0);
		self.0.queue_notify.notify_waiters();

		Ok(())
	}

	pub(crate) fn configure_queue(&self, config: ActorConfig) {
		*self.0.queue_config.lock() = config;
	}

	pub(crate) fn set_wait_activity_callback(&self, callback: Option<Arc<dyn Fn() + Send + Sync>>) {
		*self.0.queue_wait_activity_callback.lock() = callback;
	}

	pub(crate) fn set_inspector_update_callback(
		&self,
		callback: Option<Arc<dyn Fn(u32) + Send + Sync>>,
	) {
		*self.0.queue_inspector_update_callback.lock() = callback;
	}

	async fn ensure_initialized(&self) -> Result<()> {
		self.0
			.queue_initialize
			.get_or_try_init(|| async {
				let metadata = internal_storage::load_queue_metadata(self.sql())
					.await
					.context("load queue metadata from sqlite")?;
				let mut state = self.0.queue_metadata.lock().await;
				*state = metadata;
				self.0.metrics.set_queue_depth(state.size);
				Ok(())
			})
			.await
			.map(|_| ())
	}

	async fn try_receive_batch(
		&self,
		names: Option<&BTreeSet<String>>,
		count: u32,
		completable: bool,
	) -> Result<Vec<QueueMessage>> {
		let _receive_guard = self.0.queue_receive_lock.lock().await;

		let selected = self.list_messages_matching(names, count).await?;

		if selected.is_empty() {
			return Ok(Vec::new());
		}

		if completable {
			let queue_size = self.0.queue_metadata.lock().await.size;
			self.0
				.metrics
				.add_queue_messages_received(selected.len().try_into().unwrap_or(u64::MAX));
			self.notify_inspector_update(queue_size);
			return Ok(selected
				.into_iter()
				.map(|message| self.attach_completion(message))
				.collect());
		}

		self.remove_messages(selected.iter().map(|message| message.id).collect())
			.await?;
		self.0
			.metrics
			.add_queue_messages_received(selected.len().try_into().unwrap_or(u64::MAX));

		Ok(selected)
	}

	async fn list_messages(&self) -> Result<Vec<QueueMessage>> {
		let messages: Vec<QueueMessage> = internal_storage::load_queue_messages(self.sql())
			.await
			.context("list sqlite queue messages")?
			.into_iter()
			.map(queue_message_from_row)
			.collect();

		let actual_size = messages.len().try_into().unwrap_or(u32::MAX);
		let mut metadata = self.0.queue_metadata.lock().await;
		if metadata.size != actual_size {
			metadata.size = actual_size;
		}
		if metadata.next_id == 0 {
			metadata.next_id = messages
				.last()
				.map(|message| message.id.saturating_add(1))
				.unwrap_or(1);
		}

		Ok(messages)
	}

	async fn list_messages_matching(
		&self,
		names: Option<&BTreeSet<String>>,
		limit: u32,
	) -> Result<Vec<QueueMessage>> {
		internal_storage::load_queue_messages_matching(self.sql(), names, limit)
			.await
			.context("list matching sqlite queue messages")
			.map(|rows| rows.into_iter().map(queue_message_from_row).collect())
	}

	fn attach_completion(&self, mut message: QueueMessage) -> QueueMessage {
		message.completion = Some(CompletionHandle::new(self.clone(), message.id));
		message
	}

	async fn remove_messages(&self, message_ids: Vec<u64>) -> Result<()> {
		if message_ids.is_empty() {
			return Ok(());
		}

		let deleted_count = internal_storage::delete_queue_messages(self.sql(), &message_ids)
			.await
			.context("delete sqlite queue messages")?;

		let queue_size = {
			let mut metadata = self.0.queue_metadata.lock().await;
			metadata.size = metadata.size.saturating_sub(deleted_count);
			metadata.size
		};
		self.0
			.metrics
			.set_queue_depth(self.0.queue_metadata.lock().await.size);
		self.notify_inspector_update(queue_size);
		Ok(())
	}

	async fn complete_message_by_id(
		&self,
		message_id: u64,
		response: Option<Vec<u8>>,
	) -> Result<()> {
		let _receive_guard = self.0.queue_receive_lock.lock().await;
		self.complete_message_by_id_unlocked(message_id, response)
			.await
	}

	async fn complete_message_by_id_unlocked(
		&self,
		message_id: u64,
		response: Option<Vec<u8>>,
	) -> Result<()> {
		self.remove_messages(vec![message_id]).await?;
		if let Some(waiter) = self.remove_completion_waiter(message_id).await {
			let _ = waiter.send(response);
		}
		Ok(())
	}

	async fn remove_completion_waiter(
		&self,
		message_id: u64,
	) -> Option<oneshot::Sender<Option<Vec<u8>>>> {
		self.0
			.queue_completion_waiters
			.remove_async(&message_id)
			.await
			.map(|(_, waiter)| waiter)
	}

	async fn wait_for_message(
		&self,
		mut notified: Pin<&mut Notified<'_>>,
		timeout: Option<Duration>,
		signal: Option<&CancellationToken>,
	) -> WaitOutcome {
		let actor_abort_signal = self.0.queue_abort_signal.lock().clone();
		if signal.is_some_and(CancellationToken::is_cancelled) {
			return WaitOutcome::Aborted;
		}
		if actor_abort_signal.is_cancelled() {
			return WaitOutcome::Aborted;
		}

		let actor_aborted = async {
			actor_abort_signal.cancelled().await;
		};
		let external_aborted = async {
			if let Some(signal) = signal {
				signal.cancelled().await;
			} else {
				pending::<()>().await;
			}
		};

		match timeout {
			Some(timeout) => {
				tokio::select! {
					_ = &mut notified => WaitOutcome::Notified,
					_ = actor_aborted => WaitOutcome::Aborted,
					_ = external_aborted => WaitOutcome::Aborted,
					_ = sleep(timeout) => WaitOutcome::TimedOut,
				}
			}
			None => {
				tokio::select! {
					_ = &mut notified => WaitOutcome::Notified,
					_ = actor_aborted => WaitOutcome::Aborted,
					_ = external_aborted => WaitOutcome::Aborted,
				}
			}
		}
	}

	/// TS parity: queue-manager.ts keeps `enqueueAndWait` completion waits
	/// alive across actor aborts; the surrounding tracked user task owns
	/// shutdown cancellation.
	async fn wait_for_completion_response(
		&self,
		message_id: u64,
		mut receiver: oneshot::Receiver<Option<Vec<u8>>>,
		timeout: Option<Duration>,
		signal: Option<&CancellationToken>,
	) -> Result<Option<Vec<u8>>> {
		if signal.is_some_and(CancellationToken::is_cancelled) {
			return Err(QueueActorAborted.build());
		}

		let external_aborted = async {
			if let Some(signal) = signal {
				signal.cancelled().await;
			} else {
				pending::<()>().await;
			}
		};

		let wait_result = match timeout {
			Some(timeout) => {
				tokio::select! {
					response = &mut receiver => CompletionWaitOutcome::Response(response),
					_ = external_aborted => CompletionWaitOutcome::Aborted,
					_ = sleep(timeout) => CompletionWaitOutcome::TimedOut,
				}
			}
			None => {
				tokio::select! {
					response = &mut receiver => CompletionWaitOutcome::Response(response),
					_ = external_aborted => CompletionWaitOutcome::Aborted,
				}
			}
		};

		match wait_result {
			CompletionWaitOutcome::Response(Ok(response)) => Ok(response),
			CompletionWaitOutcome::Response(Err(_)) => Err(QueueCompletionWaiterDropped.build())
				.context(format!("wait for queue completion on message {message_id}")),
			CompletionWaitOutcome::TimedOut => Err(QueueWaitTimedOut {
				timeout_ms: timeout.map(duration_ms).unwrap_or(0),
			}
			.build()),
			CompletionWaitOutcome::Aborted => Err(QueueActorAborted.build()),
		}
	}

	fn block_on<T>(&self, future: impl std::future::Future<Output = Result<T>>) -> Result<T> {
		#[cfg(not(target_arch = "wasm32"))]
		{
			if let Ok(handle) = Handle::try_current() {
				tokio::task::block_in_place(|| handle.block_on(future))
			} else {
				Builder::new_current_thread()
					.enable_all()
					.build()
					.context("build temporary runtime for queue operation")?
					.block_on(future)
			}
		}

		#[cfg(target_arch = "wasm32")]
		{
			drop(future);
			Err(ActorRuntime::InvalidOperation {
				operation: "queue.try_next_batch".to_owned(),
				reason: "synchronous queue receive requires native runtime support".to_owned(),
			}
			.build())
		}
	}

	fn config(&self) -> ActorConfig {
		self.0.queue_config.lock().clone()
	}

	#[cfg(test)]
	pub(crate) fn queue_config_for_tests(&self) -> ActorConfig {
		self.config()
	}

	fn notify_wait_activity(&self) {
		if let Some(callback) = self.0.queue_wait_activity_callback.lock().clone() {
			callback();
		}
	}

	fn notify_inspector_update(&self, queue_size: u32) {
		if let Some(callback) = self.0.queue_inspector_update_callback.lock().clone() {
			callback(queue_size);
		}
	}
}

impl QueueMessage {
	pub async fn complete(self, response: Option<Vec<u8>>) -> Result<()> {
		let completable = self.into_completable()?;
		completable.complete(response).await
	}

	pub fn into_completable(self) -> Result<CompletableQueueMessage> {
		let completion = self.completion.clone().ok_or_else(|| {
			QueueCompleteNotConfigured {
				name: self.name.clone(),
			}
			.build()
		})?;

		Ok(CompletableQueueMessage {
			id: self.id,
			name: self.name,
			body: self.body,
			created_at: self.created_at,
			completion,
		})
	}

	pub fn is_completable(&self) -> bool {
		self.completion.is_some()
	}
}

impl CompletableQueueMessage {
	pub async fn complete(self, response: Option<Vec<u8>>) -> Result<()> {
		self.completion.complete(response).await
	}

	pub fn into_message(self) -> QueueMessage {
		QueueMessage {
			id: self.id,
			name: self.name,
			body: self.body,
			created_at: self.created_at,
			completion: Some(self.completion),
		}
	}
}

impl CompletionHandle {
	fn new(ctx: ActorContext, message_id: u64) -> Self {
		Self(Arc::new(CompletionHandleInner {
			ctx,
			message_id,
			completed: std::sync::atomic::AtomicBool::new(false),
		}))
	}

	async fn complete(&self, response: Option<Vec<u8>>) -> Result<()> {
		if self.0.completed.swap(true, Ordering::SeqCst) {
			return Err(QueueAlreadyCompleted.build());
		}

		if let Err(error) = self
			.0
			.ctx
			.complete_message_by_id(self.0.message_id, response)
			.await
		{
			self.0.completed.store(false, Ordering::SeqCst);
			return Err(error);
		}

		Ok(())
	}
}

impl fmt::Debug for CompletionHandle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("CompletionHandle")
			.field("message_id", &self.0.message_id)
			.field("completed", &self.0.completed.load(Ordering::SeqCst))
			.finish()
	}
}

struct ActiveQueueWaitGuard<'a> {
	ctx: &'a ActorContext,
	started_at: Instant,
}

impl<'a> ActiveQueueWaitGuard<'a> {
	fn new(ctx: &'a ActorContext) -> Self {
		ctx.0.active_queue_wait_count.fetch_add(1, Ordering::SeqCst);
		ctx.0.metrics.begin_user_task(UserTaskKind::QueueWait);
		ctx.notify_wait_activity();
		Self {
			ctx,
			started_at: Instant::now(),
		}
	}
}

impl Drop for ActiveQueueWaitGuard<'_> {
	fn drop(&mut self) {
		self.ctx
			.0
			.metrics
			.end_user_task(UserTaskKind::QueueWait, self.started_at.elapsed());
		let previous = self
			.ctx
			.0
			.active_queue_wait_count
			.fetch_sub(1, Ordering::SeqCst);
		if previous == 0 {
			self.ctx
				.0
				.active_queue_wait_count
				.store(0, Ordering::SeqCst);
		}
		self.ctx.notify_wait_activity();
	}
}

enum WaitOutcome {
	Notified,
	TimedOut,
	Aborted,
}

enum CompletionWaitOutcome {
	Response(Result<Option<Vec<u8>>, oneshot::error::RecvError>),
	TimedOut,
	Aborted,
}

fn normalize_names(names: Option<Vec<String>>) -> Option<BTreeSet<String>> {
	names.and_then(|names| {
		let normalized = names.into_iter().collect::<BTreeSet<_>>();
		if normalized.is_empty() {
			None
		} else {
			Some(normalized)
		}
	})
}

fn queue_message_from_row(row: internal_storage::QueueMessageRow) -> QueueMessage {
	QueueMessage {
		id: row.id,
		name: row.message.name,
		body: row.message.body,
		created_at: row.message.created_at,
		completion: None,
	}
}

fn current_timestamp_ms() -> Result<i64> {
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.context("current time is before unix epoch")?;
	i64::try_from(now.as_millis()).context("queue timestamp exceeds i64")
}

fn duration_ms(duration: Duration) -> u64 {
	duration.as_millis().try_into().unwrap_or(u64::MAX)
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/queue.rs"]
mod tests;
