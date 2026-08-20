use std::time::Duration;

use napi::bindgen_prelude::{BigInt, Buffer};
use napi_derive::napi;
use parking_lot::Mutex;
use rivetkit_core::{
	ActorContext as CoreActorContext, EnqueueAndWaitOpts, QueueMessage as CoreQueueMessage,
	QueueNextBatchOpts, QueueNextOpts, QueueTryNextBatchOpts, QueueTryNextOpts, QueueWaitOpts,
};

use crate::cancellation_token::CancellationToken;
use crate::{NapiInvalidArgument, NapiInvalidState, napi_anyhow_error};

#[napi(object)]
pub struct JsQueueNextOptions {
	pub names: Option<Vec<String>>,
	pub timeout_ms: Option<i64>,
	pub completable: Option<bool>,
}

#[napi(object)]
pub struct JsQueueNextBatchOptions {
	pub names: Option<Vec<String>>,
	pub count: Option<u32>,
	pub timeout_ms: Option<i64>,
	pub completable: Option<bool>,
}

#[napi(object)]
pub struct JsQueueWaitOptions {
	pub timeout_ms: Option<i64>,
	pub completable: Option<bool>,
}

#[napi(object)]
pub struct JsQueueEnqueueAndWaitOptions {
	pub timeout_ms: Option<i64>,
}

#[napi(object)]
pub struct JsQueueTryNextOptions {
	pub names: Option<Vec<String>>,
	pub completable: Option<bool>,
}

#[napi(object)]
pub struct JsQueueTryNextBatchOptions {
	pub names: Option<Vec<String>>,
	pub count: Option<u32>,
	pub completable: Option<bool>,
}

#[napi]
pub struct Queue {
	inner: CoreActorContext,
}

#[napi]
pub struct QueueMessage {
	// Completes are exposed through sync N-API object state; hold only for
	// take/restore, never across the queue completion await.
	inner: Mutex<Option<CoreQueueMessage>>,
	id: u64,
	name: String,
	body: Vec<u8>,
	created_at: i64,
	is_completable: bool,
}

impl Queue {
	pub(crate) fn new(inner: CoreActorContext) -> Self {
		Self { inner }
	}
}

impl QueueMessage {
	fn from_core(message: CoreQueueMessage) -> Self {
		tracing::debug!(
			class = "QueueMessage",
			message_id = message.id,
			name = %message.name,
			body_bytes = message.body.len(),
			completable = message.is_completable(),
			"constructed napi class"
		);
		Self {
			id: message.id,
			name: message.name.clone(),
			body: message.body.clone(),
			created_at: message.created_at,
			is_completable: message.is_completable(),
			inner: Mutex::new(Some(message)),
		}
	}
}

impl Drop for QueueMessage {
	fn drop(&mut self) {
		tracing::debug!(
			class = "QueueMessage",
			message_id = self.id,
			name = %self.name,
			completable = self.is_completable,
			"dropped napi class"
		);
	}
}

#[napi]
impl Queue {
	#[napi]
	pub async fn send(&self, name: String, body: Buffer) -> napi::Result<QueueMessage> {
		self.inner
			.send(&name, body.as_ref())
			.await
			.map(QueueMessage::from_core)
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn next(
		&self,
		options: Option<JsQueueNextOptions>,
		signal: Option<&CancellationToken>,
	) -> napi::Result<Option<QueueMessage>> {
		self.inner
			.next(queue_next_opts(options, signal)?)
			.await
			.map(|message| message.map(QueueMessage::from_core))
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn next_batch(
		&self,
		options: Option<JsQueueNextBatchOptions>,
		signal: Option<&CancellationToken>,
	) -> napi::Result<Vec<QueueMessage>> {
		self.inner
			.next_batch(queue_next_batch_opts(options, signal)?)
			.await
			.map(|messages| messages.into_iter().map(QueueMessage::from_core).collect())
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn wait_for_names(
		&self,
		names: Vec<String>,
		options: Option<JsQueueWaitOptions>,
		signal: Option<&CancellationToken>,
	) -> napi::Result<QueueMessage> {
		self.inner
			.wait_for_names(names, queue_wait_opts(options, signal)?)
			.await
			.map(QueueMessage::from_core)
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn wait_for_names_available(
		&self,
		names: Vec<String>,
		options: Option<JsQueueWaitOptions>,
		signal: Option<&CancellationToken>,
	) -> napi::Result<()> {
		self.inner
			.wait_for_names_available(names, queue_wait_opts(options, signal)?)
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn complete_persisted(
		&self,
		message_id: BigInt,
		expected_name: String,
		response: Option<Buffer>,
	) -> napi::Result<bool> {
		let (negative, message_id, lossless) = message_id.get_u64();
		if negative || !lossless {
			return Err(napi_anyhow_error(
				NapiInvalidArgument {
					argument: "messageId".to_owned(),
					reason: "must be a non-negative 64-bit bigint".to_owned(),
				}
				.build(),
			));
		}
		self.inner
			.complete_persisted_message(
				message_id,
				&expected_name,
				response.map(|value| value.to_vec()),
			)
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn enqueue_and_wait(
		&self,
		name: String,
		body: Buffer,
		options: Option<JsQueueEnqueueAndWaitOptions>,
		signal: Option<&CancellationToken>,
	) -> napi::Result<Option<Buffer>> {
		self.inner
			.enqueue_and_wait(
				&name,
				body.as_ref(),
				enqueue_and_wait_opts(options, signal)?,
			)
			.await
			.map(|response| response.map(Buffer::from))
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn try_next(
		&self,
		options: Option<JsQueueTryNextOptions>,
	) -> napi::Result<Option<QueueMessage>> {
		self.inner
			.try_next(queue_try_next_opts(options))
			.map(|message| message.map(QueueMessage::from_core))
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn try_next_batch(
		&self,
		options: Option<JsQueueTryNextBatchOptions>,
	) -> napi::Result<Vec<QueueMessage>> {
		self.inner
			.try_next_batch(queue_try_next_batch_opts(options))
			.map(|messages| messages.into_iter().map(QueueMessage::from_core).collect())
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn max_size(&self) -> u32 {
		self.inner.max_size()
	}

	#[napi]
	pub async fn reset(&self) -> napi::Result<()> {
		self.inner.reset().await.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn inspect_messages(&self) -> napi::Result<Vec<JsQueueInspectMessage>> {
		self.inner
			.inspect_messages()
			.await
			.map(|messages| {
				messages
					.into_iter()
					.map(|m| JsQueueInspectMessage {
						id: u64_to_i64(m.id),
						name: m.name,
						created_at_ms: m.created_at,
					})
					.collect()
			})
			.map_err(napi_anyhow_error)
	}
}

#[napi(object)]
pub struct JsQueueInspectMessage {
	/// Queue message id. Stored as the raw u64 reinterpreted as i64 so JS
	/// sees a plain number; ids are monotonic and fit comfortably in i64.
	pub id: i64,
	pub name: String,
	pub created_at_ms: i64,
}

fn u64_to_i64(value: u64) -> i64 {
	i64::try_from(value).unwrap_or(i64::MAX)
}

#[napi]
impl QueueMessage {
	#[napi]
	pub fn id(&self) -> u64 {
		self.id
	}

	#[napi]
	pub fn name(&self) -> String {
		self.name.clone()
	}

	#[napi]
	pub fn body(&self) -> Buffer {
		Buffer::from(self.body.clone())
	}

	#[napi]
	pub fn created_at(&self) -> i64 {
		self.created_at
	}

	#[napi]
	pub fn is_completable(&self) -> bool {
		self.is_completable
	}

	#[napi]
	pub async fn complete(&self, response: Option<Buffer>) -> napi::Result<()> {
		tracing::debug!(
			class = "QueueMessage",
			message_id = self.id,
			name = %self.name,
			response_bytes = response.as_ref().map(|response| response.len()).unwrap_or(0),
			"completing queue message"
		);
		let message = {
			let mut guard = self.inner.lock();
			guard.take().ok_or_else(|| {
				napi_anyhow_error(
					NapiInvalidState {
						state: "queue message".to_owned(),
						reason: "already completed".to_owned(),
					}
					.build(),
				)
			})?
		};

		if let Err(error) = message
			.clone()
			.complete(response.map(|response| response.to_vec()))
			.await
		{
			let mut guard = self.inner.lock();
			*guard = Some(message);
			return Err(napi_anyhow_error(error));
		}

		Ok(())
	}
}

fn queue_next_opts(
	options: Option<JsQueueNextOptions>,
	signal: Option<&CancellationToken>,
) -> napi::Result<QueueNextOpts> {
	let options = options.unwrap_or(JsQueueNextOptions {
		names: None,
		timeout_ms: None,
		completable: None,
	});

	Ok(QueueNextOpts {
		names: options.names,
		timeout: timeout_duration(options.timeout_ms)?,
		signal: signal.map(|signal| signal.inner().clone()),
		completable: options.completable.unwrap_or(false),
	})
}

fn queue_next_batch_opts(
	options: Option<JsQueueNextBatchOptions>,
	signal: Option<&CancellationToken>,
) -> napi::Result<QueueNextBatchOpts> {
	let options = options.unwrap_or(JsQueueNextBatchOptions {
		names: None,
		count: None,
		timeout_ms: None,
		completable: None,
	});

	Ok(QueueNextBatchOpts {
		names: options.names,
		count: options.count.unwrap_or(1),
		timeout: timeout_duration(options.timeout_ms)?,
		signal: signal.map(|signal| signal.inner().clone()),
		completable: options.completable.unwrap_or(false),
	})
}

fn queue_wait_opts(
	options: Option<JsQueueWaitOptions>,
	signal: Option<&CancellationToken>,
) -> napi::Result<QueueWaitOpts> {
	let options = options.unwrap_or(JsQueueWaitOptions {
		timeout_ms: None,
		completable: None,
	});

	Ok(QueueWaitOpts {
		timeout: timeout_duration(options.timeout_ms)?,
		signal: signal.map(|signal| signal.inner().clone()),
		completable: options.completable.unwrap_or(false),
	})
}

fn enqueue_and_wait_opts(
	options: Option<JsQueueEnqueueAndWaitOptions>,
	signal: Option<&CancellationToken>,
) -> napi::Result<EnqueueAndWaitOpts> {
	let options = options.unwrap_or(JsQueueEnqueueAndWaitOptions { timeout_ms: None });

	Ok(EnqueueAndWaitOpts {
		timeout: timeout_duration(options.timeout_ms)?,
		signal: signal.map(|signal| signal.inner().clone()),
	})
}

fn queue_try_next_opts(options: Option<JsQueueTryNextOptions>) -> QueueTryNextOpts {
	let options = options.unwrap_or(JsQueueTryNextOptions {
		names: None,
		completable: None,
	});

	QueueTryNextOpts {
		names: options.names,
		completable: options.completable.unwrap_or(false),
	}
}

fn queue_try_next_batch_opts(options: Option<JsQueueTryNextBatchOptions>) -> QueueTryNextBatchOpts {
	let options = options.unwrap_or(JsQueueTryNextBatchOptions {
		names: None,
		count: None,
		completable: None,
	});

	QueueTryNextBatchOpts {
		names: options.names,
		count: options.count.unwrap_or(1),
		completable: options.completable.unwrap_or(false),
	}
}

fn timeout_duration(timeout_ms: Option<i64>) -> napi::Result<Option<Duration>> {
	match timeout_ms {
		Some(timeout_ms) if timeout_ms < 0 => Err(napi_anyhow_error(
			NapiInvalidArgument {
				argument: "timeoutMs".to_owned(),
				reason: "must be non-negative".to_owned(),
			}
			.build(),
		)),
		Some(timeout_ms) => Ok(Some(Duration::from_millis(
			u64::try_from(timeout_ms).map_err(|_| {
				napi_anyhow_error(
					NapiInvalidArgument {
						argument: "timeoutMs".to_owned(),
						reason: "exceeds u64 range".to_owned(),
					}
					.build(),
				)
			})?,
		))),
		None => Ok(None),
	}
}
