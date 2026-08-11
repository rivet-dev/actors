use std::{
	fmt::{self, Debug},
	marker::PhantomData,
	sync::Arc,
};

use rivet_pools::UpsPool;
use rivet_util::Id;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::Instrument;
use universalpubsub::{NextOutput, Subscriber};

use crate::{
	error::{WorkflowError, WorkflowResult},
	message::{Message, PubsubMessage, PubsubMessageWrapper},
	pubsub_subjects::MessageSubject,
};

#[derive(Clone)]
pub struct MessageCtx {
	/// The connection used to communicate with pubsub.
	pubsub: UpsPool,

	ray_id: Id,

	config: rivet_config::Config,
}

impl MessageCtx {
	#[tracing::instrument(skip_all, fields(%ray_id))]
	pub fn new(
		config: &rivet_config::Config,
		pools: &rivet_pools::Pools,
		_cache: &rivet_cache::Cache,
		ray_id: Id,
	) -> WorkflowResult<Self> {
		Ok(MessageCtx {
			pubsub: pools.ups().map_err(WorkflowError::PoolsGeneric)?,
			ray_id,
			config: config.clone(),
		})
	}
}

// MARK: Publishing messages
impl MessageCtx {
	/// Publishes a message to pubsub and to a durable message stream if a topic is
	/// set.
	///
	/// Use `subscribe` to consume these messages ephemerally and `tail` to read
	/// the most recently sent message.
	///
	/// This spawns a background task that calls `message_wait` internally and does not wait for the message to
	/// finish publishing. This is done since there are very few cases where a
	/// service should need to wait or fail if a message does not publish
	/// successfully.
	#[tracing::instrument(skip_all, fields(message=M::NAME, %topic))]
	pub async fn message<M>(&self, topic: &str, message_body: M) -> WorkflowResult<()>
	where
		M: Message,
	{
		let client = self.clone();
		let topic = topic.to_string();

		tokio::task::Builder::new()
			.name("gasoline::message_async")
			.spawn(
				async move {
					match client.message_wait::<M>(&topic, message_body).await {
						Ok(_) => {}
						Err(err) => {
							tracing::error!(?err, "failed to publish message");
						}
					}
				}
				.instrument(tracing::info_span!("message_bg")),
			)
			.map_err(|err| WorkflowError::PublishMessage(err.into()))
			.map(|_| ())
	}

	/// Same as `message` but waits for the message to successfully publish.
	///
	/// This is useful in scenarios where we need to publish a large amount of
	/// messages at once so we put the messages in a queue instead of submitting
	/// a large number of tasks to Tokio at once.
	#[tracing::instrument(skip_all, fields(message = M::NAME, %topic))]
	pub async fn message_wait<M>(&self, topic: &str, message_body: M) -> WorkflowResult<()>
	where
		M: Message,
	{
		let subject = MessageSubject::<M>::new(topic);
		let duration_since_epoch = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_else(|err| unreachable!("time is broken: {}", err));
		let ts = duration_since_epoch.as_millis() as i64;

		// Serialize the body
		let body_buf = rivet_util::serde::json_to_string!(&message_body)
			.map_err(WorkflowError::SerializeMessage)?;
		let body_buf_len = body_buf.len();
		let body_buf = serde_json::value::RawValue::from_string(body_buf)
			.map_err(WorkflowError::SerializeMessage)?;

		// Serialize message
		let req_id = Id::new_v1(self.config.dc_label());
		let message = PubsubMessageWrapper {
			req_id,
			ray_id: self.ray_id,
			topic: topic.to_string(),
			ts,
			body: &body_buf,
		};
		let message_buf =
			rivet_util::serde::json_to_vec!(&message).map_err(WorkflowError::SerializeMessage)?;

		tracing::debug!(
			%subject,
			body_bytes = ?body_buf_len,
			message_bytes = ?message_buf.len(),
			"publish message"
		);

		// It's important to write to the stream as fast as possible in order to
		// ensure messages are handled quickly.
		let message_buf = Arc::new(message_buf);
		self.message_publish_pubsub::<M>(subject, message_buf).await;

		Ok(())
	}

	/// Publishes the message to pubsub.
	#[tracing::instrument(level = "debug", skip_all)]
	async fn message_publish_pubsub<M>(
		&self,
		subject: MessageSubject<'_, M>,
		message_buf: Arc<Vec<u8>>,
	) where
		M: Message,
	{
		// Infinite backoff since we want to wait until the service reboots.
		let mut backoff = rivet_util::throttle::Backoff::default_infinite();
		loop {
			// Ignore for infinite backoff
			backoff.tick().await;

			tracing::trace!(
				%subject,
				message_len = message_buf.len(),
				"publishing message to pubsub"
			);
			if let Err(err) = self
				.pubsub
				.publish(
					subject.clone(),
					&(*message_buf),
					universalpubsub::PublishOpts::broadcast(),
				)
				.await
			{
				tracing::debug!(?err, "publish message failed, trying again");
				continue;
			}

			tracing::debug!("publish pubsub message succeeded");
			break;
		}
	}
}

impl MessageCtx {
	pub fn config(&self) -> &rivet_config::Config {
		&self.config
	}
}

// MARK: Subscriptions
impl MessageCtx {
	/// Listens for gasoline messages globally on pubsub.
	#[tracing::instrument(skip_all, fields(message = M::NAME, %topic))]
	pub async fn subscribe<M>(&self, topic: &str) -> WorkflowResult<SubscriptionHandle<M>>
	where
		M: Message,
	{
		self.subscribe_opt::<M>(SubscribeOpts {
			topic: topic.to_string(),
			flush: true,
		})
		.in_current_span()
		.await
	}

	/// Listens for gasoline messages globally on pubsub.
	#[tracing::instrument(skip_all, fields(message = M::NAME))]
	pub async fn subscribe_opt<M>(
		&self,
		opts: SubscribeOpts,
	) -> WorkflowResult<SubscriptionHandle<M>>
	where
		M: Message,
	{
		let subject = MessageSubject::<M>::new(&opts.topic);

		// Create subscription and flush immediately.
		tracing::debug!(%subject, "creating subscription");
		let subscription = self
			.pubsub
			.subscribe(&subject)
			.await
			.map_err(|x| WorkflowError::CreateSubscription(x.into()))?;
		if opts.flush {
			self.pubsub
				.flush()
				.await
				.map_err(|x| WorkflowError::FlushPubsub(x.into()))?;
		}

		// Return handle
		let subscription = SubscriptionHandle::new(subject.to_string(), subscription);
		Ok(subscription)
	}
}

#[derive(Debug)]
pub struct SubscribeOpts {
	pub topic: String,
	pub flush: bool,
}

/// Used to receive messages from other contexts.
///
/// This subscription will automatically close when dropped.
pub struct SubscriptionHandle<M>
where
	M: Message,
{
	_message: PhantomData<M>,
	_guard: DropGuard,
	subject: String,
	subscription: Subscriber,
}

impl<M> Debug for SubscriptionHandle<M>
where
	M: Message,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("SubscriptionHandle")
			.field("subject", &self.subject)
			.finish()
	}
}

impl<M> SubscriptionHandle<M>
where
	M: Message,
{
	#[tracing::instrument(level = "debug", skip_all)]
	fn new(subject: String, subscription: Subscriber) -> Self {
		let token = CancellationToken::new();

		{
			let token = token.clone();
			let spawn_res = tokio::task::Builder::new()
				.name("gasoline::message_wait_drop")
				.spawn(
					async move {
						token.cancelled().await;

						tracing::trace!("closing subscription");

						// We don't worry about calling `subscription.drain()` since the
						// entire subscription wrapper is dropped anyways, so we can't
						// call `.recv()`.
					}
					.instrument(tracing::trace_span!("subscription_wait_drop")),
				);
			if let Err(err) = spawn_res {
				tracing::error!(?err, "failed to spawn message_wait_drop task");
			}
		}

		SubscriptionHandle {
			_message: Default::default(),
			_guard: token.drop_guard(),
			subject,
			subscription,
		}
	}

	/// Waits for the next message in the subscription.
	///
	/// This future can be safely dropped.
	#[tracing::instrument(name="message_next", skip_all, fields(message = M::NAME))]
	pub async fn next(&mut self) -> WorkflowResult<PubsubMessage<M>> {
		tracing::debug!("waiting for message");

		// Poll the subscription.
		//
		// Use blocking threads instead of `try_next`, since I'm not sure
		// try_next works as intended.
		let message = match self.subscription.next().await {
			Ok(NextOutput::Message(msg)) => msg,
			Ok(NextOutput::Unsubscribed) => {
				tracing::debug!("unsubscribed");
				return Err(WorkflowError::SubscriptionUnsubscribed);
			}
			// Should be unreachable
			Ok(NextOutput::NoResponders) => {
				tracing::warn!("no responders");
				return Err(WorkflowError::CreateSubscription(
					anyhow::anyhow!("no responders from gas subscription").into(),
				));
			}
			Err(err) => {
				tracing::warn!(?err, "subscription error");
				return Err(WorkflowError::CreateSubscription(err.into()));
			}
		};

		let message_wrapper = PubsubMessage::<M>::deserialize_wrapper(&message.payload)?;

		let message = PubsubMessage::<M>::deserialize_from_wrapper(message_wrapper)?;
		tracing::debug!(?message, "received message");

		Ok(message)
	}

	/// Converts the subscription in to a stream.
	pub fn into_stream(self) -> impl futures_util::Stream<Item = WorkflowResult<PubsubMessage<M>>> {
		futures_util::stream::try_unfold(self, |mut sub| {
			async move {
				let message = sub.next().await?;
				Ok(Some((message, sub)))
			}
			.in_current_span()
		})
	}
}
