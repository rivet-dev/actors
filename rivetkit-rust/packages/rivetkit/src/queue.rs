use std::future::Future;
use std::io::Cursor;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use rivetkit_core::{
	ActorContext, EnqueueAndWaitOpts, QueueMessage as CoreQueueMessage, QueueNextBatchOpts,
	QueueNextOpts, QueueTryNextBatchOpts, QueueTryNextOpts,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{actor::Actor, context::Ctx};
pub(crate) type BoxQueueFuture = Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send>>;

pub trait QueueMessage: Serialize + DeserializeOwned + Send + Sync + 'static {
	type Reply: Serialize + DeserializeOwned + Send + 'static;

	const NAME: &'static str;
}

pub trait HandlesQueue<M: QueueMessage>: Actor + Sized {
	type Future: Future<Output = Result<M::Reply>> + Send + 'static;

	fn handle_queue(self: Arc<Self>, ctx: Ctx<Self>, message: M) -> Self::Future;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueEntry<A: Actor> {
	pub name: &'static str,
	_p: PhantomData<fn() -> A>,
}

impl<A: Actor> QueueEntry<A> {
	pub const fn new(name: &'static str) -> Self {
		Self {
			name,
			_p: PhantomData,
		}
	}
}

pub trait QueueSet<A: Actor>: Send + Sync + 'static {
	fn entries() -> Vec<QueueEntry<A>>;
	fn dispatch(actor: Arc<A>, ctx: Ctx<A>, name: &str, body: &[u8]) -> Option<BoxQueueFuture>;
}

impl<A: Actor> QueueSet<A> for () {
	fn entries() -> Vec<QueueEntry<A>> {
		Vec::new()
	}

	fn dispatch(_actor: Arc<A>, _ctx: Ctx<A>, _name: &str, _body: &[u8]) -> Option<BoxQueueFuture> {
		None
	}
}

macro_rules! impl_queue_set {
	($($message:ident),+) => {
		impl<Act, $($message),+> QueueSet<Act> for ($($message,)+)
		where
			Act: Actor + $(HandlesQueue<$message> +)+,
			$($message: QueueMessage,)+
		{
			fn entries() -> Vec<QueueEntry<Act>> {
				vec![$(QueueEntry::new(<$message as QueueMessage>::NAME)),+]
			}

			fn dispatch(
				actor: Arc<Act>,
				ctx: Ctx<Act>,
				name: &str,
				body: &[u8],
			) -> Option<BoxQueueFuture> {
				$(
					if name == <$message as QueueMessage>::NAME {
						let body = body.to_vec();
						return Some(Box::pin(async move {
							let message = decode_cbor::<$message>(&body, "queue message body")
								.with_context(|| format!("decode queue message '{}'", <$message as QueueMessage>::NAME))?;
							let reply = <Act as HandlesQueue<$message>>::handle_queue(actor, ctx, message).await?;
							Ok(Some(encode_cbor(&reply, "queue message reply")?))
						}));
					}
				)+
				None
			}
		}
	};
}

impl_queue_set!(M0);
impl_queue_set!(M0, M1);
impl_queue_set!(M0, M1, M2);
impl_queue_set!(M0, M1, M2, M3);
impl_queue_set!(M0, M1, M2, M3, M4);
impl_queue_set!(M0, M1, M2, M3, M4, M5);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7, M8);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7, M8, M9);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11, M12);
impl_queue_set!(M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11, M12, M13);
impl_queue_set!(
	M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11, M12, M13, M14
);
impl_queue_set!(
	M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11, M12, M13, M14, M15
);

/// Typed handle over the actor message queue, returned by [`crate::Ctx::queue`].
///
/// This is a thin typed facade over the core queue API. Send helpers CBOR-encode
/// the message body; the `*_raw` variants pass bytes through unchanged. Received
/// [`QueueMessage`] bodies are raw bytes that the caller decodes against their own
/// schema, matching how queued messages arrive through the event loop as
/// `RuntimeEvent::QueueSend`.
pub struct Queue<'a, A: Actor> {
	inner: &'a ActorContext,
	_p: PhantomData<fn() -> A>,
}

#[derive(Debug, Clone)]
pub struct TypedQueueMessage<M: QueueMessage> {
	inner: CoreQueueMessage,
	body: M,
}

impl<M: QueueMessage> TypedQueueMessage<M> {
	pub fn id(&self) -> u64 {
		self.inner.id
	}

	pub fn name(&self) -> &str {
		&self.inner.name
	}

	pub fn body(&self) -> &M {
		&self.body
	}

	pub fn created_at(&self) -> i64 {
		self.inner.created_at
	}

	pub fn is_completable(&self) -> bool {
		self.inner.is_completable()
	}

	pub fn into_body(self) -> M {
		self.body
	}

	pub fn as_core(&self) -> &CoreQueueMessage {
		&self.inner
	}

	pub fn into_core(self) -> CoreQueueMessage {
		self.inner
	}

	pub async fn complete(self, reply: M::Reply) -> Result<()> {
		self.complete_raw(Some(encode_cbor(&reply, "queue message reply")?))
			.await
	}

	pub async fn complete_raw(self, response: Option<Vec<u8>>) -> Result<()> {
		self.inner.complete(response).await
	}
}

impl<'a, A: Actor> Queue<'a, A> {
	pub(crate) fn new(inner: &'a ActorContext) -> Self {
		Self {
			inner,
			_p: PhantomData,
		}
	}

	/// Enqueues a message with a CBOR-encoded body.
	pub async fn send<T: Serialize>(&self, name: &str, body: &T) -> Result<CoreQueueMessage> {
		self.send_raw(name, &encode_cbor(body, "queue message body")?)
			.await
	}

	/// Enqueues a message with a raw byte body.
	pub async fn send_raw(&self, name: &str, body: &[u8]) -> Result<CoreQueueMessage> {
		self.inner.send(name, body).await
	}

	/// Enqueues a message with a CBOR-encoded body and waits for the consumer to
	/// complete it, returning the raw completion response if any.
	pub async fn enqueue_and_wait<T: Serialize>(
		&self,
		name: &str,
		body: &T,
		opts: EnqueueAndWaitOpts,
	) -> Result<Option<Vec<u8>>> {
		self.enqueue_and_wait_raw(name, &encode_cbor(body, "queue message body")?, opts)
			.await
	}

	/// Enqueues a raw-body message and waits for its completion response.
	pub async fn enqueue_and_wait_raw(
		&self,
		name: &str,
		body: &[u8],
		opts: EnqueueAndWaitOpts,
	) -> Result<Option<Vec<u8>>> {
		self.inner.enqueue_and_wait(name, body, opts).await
	}

	/// Awaits the next queued message, optionally bounded by the opts timeout.
	pub async fn next(&self, opts: QueueNextOpts) -> Result<Option<CoreQueueMessage>> {
		self.inner.next(opts).await
	}

	/// Awaits the next queued message matching `M::NAME` and decodes its CBOR body.
	pub async fn next_typed<M: QueueMessage>(
		&self,
		opts: QueueNextOpts,
	) -> Result<Option<TypedQueueMessage<M>>> {
		self.inner
			.next(typed_next_opts::<M>(opts))
			.await?
			.map(decode_core_message)
			.transpose()
	}

	/// Awaits up to `opts.count` queued messages.
	pub async fn next_batch(&self, opts: QueueNextBatchOpts) -> Result<Vec<CoreQueueMessage>> {
		self.inner.next_batch(opts).await
	}

	/// Awaits up to `opts.count` queued messages matching `M::NAME`.
	pub async fn next_batch_typed<M: QueueMessage>(
		&self,
		opts: QueueNextBatchOpts,
	) -> Result<Vec<TypedQueueMessage<M>>> {
		self.inner
			.next_batch(typed_next_batch_opts::<M>(opts))
			.await?
			.into_iter()
			.map(decode_core_message)
			.collect()
	}

	/// Returns the next queued message if one is immediately available.
	pub fn try_next(&self, opts: QueueTryNextOpts) -> Result<Option<CoreQueueMessage>> {
		self.inner.try_next(opts)
	}

	/// Returns the next queued message matching `M::NAME` if one is immediately available.
	pub fn try_next_typed<M: QueueMessage>(
		&self,
		opts: QueueTryNextOpts,
	) -> Result<Option<TypedQueueMessage<M>>> {
		self.inner
			.try_next(typed_try_next_opts::<M>(opts))?
			.map(decode_core_message)
			.transpose()
	}

	/// Returns immediately-available queued messages up to `opts.count`.
	pub fn try_next_batch(&self, opts: QueueTryNextBatchOpts) -> Result<Vec<CoreQueueMessage>> {
		self.inner.try_next_batch(opts)
	}

	/// Returns immediately-available queued messages matching `M::NAME`.
	pub fn try_next_batch_typed<M: QueueMessage>(
		&self,
		opts: QueueTryNextBatchOpts,
	) -> Result<Vec<TypedQueueMessage<M>>> {
		self.inner
			.try_next_batch(typed_try_next_batch_opts::<M>(opts))?
			.into_iter()
			.map(decode_core_message)
			.collect()
	}

	/// Lists the currently persisted queue messages without consuming them.
	pub async fn inspect_messages(&self) -> Result<Vec<CoreQueueMessage>> {
		self.inner.inspect_messages().await
	}

	/// Returns the configured maximum queue size.
	pub fn max_size(&self) -> u32 {
		self.inner.max_size()
	}
}

fn decode_core_message<M: QueueMessage>(message: CoreQueueMessage) -> Result<TypedQueueMessage<M>> {
	if message.name != M::NAME {
		anyhow::bail!(
			"expected queue message '{}', received '{}'",
			M::NAME,
			message.name
		);
	}

	let body = decode_cbor::<M>(&message.body, "queue message body")
		.with_context(|| format!("decode queue message '{}'", M::NAME))?;

	Ok(TypedQueueMessage {
		inner: message,
		body,
	})
}

fn typed_next_opts<M: QueueMessage>(opts: QueueNextOpts) -> QueueNextOpts {
	QueueNextOpts {
		names: Some(vec![M::NAME.to_owned()]),
		timeout: opts.timeout,
		signal: opts.signal,
		completable: opts.completable,
	}
}

fn typed_next_batch_opts<M: QueueMessage>(opts: QueueNextBatchOpts) -> QueueNextBatchOpts {
	QueueNextBatchOpts {
		names: Some(vec![M::NAME.to_owned()]),
		count: opts.count,
		timeout: opts.timeout,
		signal: opts.signal,
		completable: opts.completable,
	}
}

fn typed_try_next_opts<M: QueueMessage>(opts: QueueTryNextOpts) -> QueueTryNextOpts {
	QueueTryNextOpts {
		names: Some(vec![M::NAME.to_owned()]),
		completable: opts.completable,
	}
}

fn typed_try_next_batch_opts<M: QueueMessage>(
	opts: QueueTryNextBatchOpts,
) -> QueueTryNextBatchOpts {
	QueueTryNextBatchOpts {
		names: Some(vec![M::NAME.to_owned()]),
		count: opts.count,
		completable: opts.completable,
	}
}

fn encode_cbor<T: Serialize>(value: &T, label: &str) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded)
		.with_context(|| format!("encode {label} as cbor"))?;
	Ok(encoded)
}

fn decode_cbor<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
	ciborium::from_reader(Cursor::new(bytes)).with_context(|| format!("decode {label} from cbor"))
}

#[cfg(test)]
mod tests {
	use std::future::{Ready, ready};
	use std::sync::Arc;

	use anyhow::Result;
	use serde::{Deserialize, Serialize};

	use super::{HandlesQueue, QueueMessage, QueueSet};
	use crate::{action, actor::Actor, context::Ctx};

	struct TestActor;

	impl Actor for TestActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;
	}

	#[derive(Debug, Serialize, Deserialize)]
	struct FirstMessage;

	impl QueueMessage for FirstMessage {
		type Reply = ();

		const NAME: &'static str = "first";
	}

	#[derive(Debug, Serialize, Deserialize)]
	struct SecondMessage;

	impl QueueMessage for SecondMessage {
		type Reply = ();

		const NAME: &'static str = "second";
	}

	impl HandlesQueue<FirstMessage> for TestActor {
		type Future = Ready<Result<()>>;

		fn handle_queue(self: Arc<Self>, _ctx: Ctx<Self>, _message: FirstMessage) -> Self::Future {
			ready(Ok(()))
		}
	}

	impl HandlesQueue<SecondMessage> for TestActor {
		type Future = Ready<Result<()>>;

		fn handle_queue(self: Arc<Self>, _ctx: Ctx<Self>, _message: SecondMessage) -> Self::Future {
			ready(Ok(()))
		}
	}

	#[test]
	fn queue_set_unit_registers_nothing() {
		assert!(<() as QueueSet<TestActor>>::entries().is_empty());
	}

	#[test]
	fn queue_set_tuple_registers_names_in_order() {
		let entries = <(FirstMessage, SecondMessage) as QueueSet<TestActor>>::entries();

		assert_eq!(
			entries.iter().map(|entry| entry.name).collect::<Vec<_>>(),
			["first", "second",]
		);
	}

	#[test]
	fn queue_set_tuple_supports_one_and_max_arity() {
		assert_eq!(
			<(FirstMessage,) as QueueSet<TestActor>>::entries()
				.iter()
				.map(|entry| entry.name)
				.collect::<Vec<_>>(),
			["first"]
		);

		type MaxMessages = (
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
			FirstMessage,
		);
		let entries = <MaxMessages as QueueSet<TestActor>>::entries();

		assert_eq!(entries.len(), action::TUPLE_ARITY_MAX);
		assert!(entries.iter().all(|entry| entry.name == "first"));
	}
}
