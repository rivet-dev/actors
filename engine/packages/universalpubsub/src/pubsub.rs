use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rivet_perf::{perf_finish, perf_start};
use scc::HashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

use rivet_util::throttle::Backoff;

use crate::chunking::{ChunkTracker, FastPath, encode_chunk, split_payload_into_chunks};
use crate::driver::{PubSubDriverHandle, PublishOpts, SubscriberDriverHandle};
use crate::errors;
use crate::metrics;
use crate::subject::{InboxSubject, Subject};

const GC_INTERVAL: Duration = Duration::from_secs(60);

pub struct PubSubInner {
	driver: PubSubDriverHandle,
	chunk_tracker: ChunkTracker,
	// Local in-memory subscribers by subject (shared across all drivers)
	local_subscribers: HashMap<String, broadcast::Sender<Vec<u8>>>,
	// Enables/disables local fast-path across all drivers
	memory_optimization: bool,
}

#[derive(Clone)]
pub struct PubSub(Arc<PubSubInner>);

impl Deref for PubSub {
	type Target = PubSubInner;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl PubSub {
	pub fn new(driver: PubSubDriverHandle) -> Self {
		Self::new_with_memory_optimization(driver, true)
	}

	pub fn new_with_memory_optimization(
		driver: PubSubDriverHandle,
		memory_optimization: bool,
	) -> Self {
		let inner = Arc::new(PubSubInner {
			driver,
			chunk_tracker: ChunkTracker::new(),
			local_subscribers: HashMap::new(),
			memory_optimization,
		});

		// Spawn GC task for chunk buffers and local subscribers
		let inner2 = Arc::downgrade(&inner);
		tokio::spawn(async move {
			let mut interval = tokio::time::interval(GC_INTERVAL);
			interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

			loop {
				interval.tick().await;
				if let Some(inner) = inner2.upgrade() {
					// Clean up chunk buffers
					inner.chunk_tracker.gc().await;

					// Clean up local subscribers with no receivers
					inner
						.local_subscribers
						.retain_async(|_, sender| sender.receiver_count() > 0)
						.await;
					metrics::LOCAL_SUBSCRIBER_COUNT.set(inner.local_subscribers.len() as i64);
				} else {
					break;
				}
			}
		});

		Self(inner)
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	pub async fn subscribe<T: Subject>(&self, subject: T) -> Result<Subscriber> {
		self.subscribe_inner(subject, None).await
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	async fn subscribe_inner<T: Subject>(
		&self,
		subject: T,
		reply_id: Option<Uuid>,
	) -> Result<Subscriber> {
		// Underlying driver subscription
		let driver = self.driver.subscribe(&subject.as_cow(), reply_id).await?;

		if !self.memory_optimization {
			return Ok(Subscriber::new(
				driver,
				self.clone(),
				false,
				subject.to_string(),
				subject.subject_root().map(|x| x.to_string()),
			));
		}

		// Ensure a local broadcast channel exists for this subject
		let local_rx = {
			let rx = self
				.local_subscribers
				.entry_async(subject.to_string())
				.await
				.or_insert_with(|| broadcast::channel(1024).0)
				.subscribe();
			metrics::LOCAL_SUBSCRIBER_COUNT.set(self.local_subscribers.len() as i64);
			rx
		};

		// Wrap the driver
		let optimized_driver: SubscriberDriverHandle = Box::new(LocalOptimizedSubscriberDriver {
			subject: subject.to_string(),
			driver,
			local_rx,
		});

		Ok(Subscriber::new(
			optimized_driver,
			self.clone(),
			true,
			subject.to_string(),
			subject.subject_root().map(|x| x.to_string()),
		))
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	pub async fn queue_subscribe<T: Subject>(&self, subject: T, queue: &str) -> Result<Subscriber> {
		// Underlying driver subscription
		let driver = self
			.driver
			.queue_subscribe(&subject.as_cow(), queue)
			.await?;

		return Ok(Subscriber::new(
			driver,
			self.clone(),
			false,
			subject.to_string(),
			subject.subject_root().map(|x| x.to_string()),
		));
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	pub async fn publish(
		&self,
		subject: impl Subject,
		payload: &[u8],
		opts: PublishOpts,
	) -> Result<Uuid> {
		self.publish_inner(subject, payload, None::<&str>, opts, None)
			.await
	}

	#[tracing::instrument(skip_all, fields(%subject, %reply_subject))]
	pub async fn publish_with_reply(
		&self,
		subject: impl Subject,
		payload: &[u8],
		reply_subject: impl Subject,
		opts: PublishOpts,
	) -> Result<Uuid> {
		self.publish_inner(subject, payload, Some(reply_subject), opts, None)
			.await
	}

	#[tracing::instrument(skip_all, fields(%subject, ?opts, message_id = tracing::field::Empty))]
	async fn publish_inner<T: Subject>(
		&self,
		subject: T,
		payload: &[u8],
		reply_subject: Option<impl Subject>,
		opts: PublishOpts,
		request_deadline_at: Option<i64>,
	) -> Result<Uuid> {
		let message_id = Uuid::new_v4();
		tracing::Span::current().record("message_id", message_id.to_string());

		let chunks = split_payload_into_chunks(
			payload,
			self.driver.max_message_size(),
			message_id,
			reply_subject.as_ref().map(|x| x.as_cow()).as_deref(),
			request_deadline_at,
		)?;
		let chunk_count = chunks.len() as u32;

		let use_local = self
			.should_use_local_subscriber(&subject, opts.behavior)
			.await;

		let subject_cow = subject.as_cow();
		let reply_subject = reply_subject.as_ref().map(|x| x.as_cow());
		let subject_root = subject.subject_root();
		let subject_root = subject_root.as_deref().unwrap_or("unknown");

		for (chunk_idx, chunk_payload) in chunks.into_iter().enumerate() {
			let encoded = encode_chunk(
				chunk_payload,
				chunk_idx as u32,
				chunk_count,
				message_id,
				reply_subject.clone(),
				request_deadline_at,
			)?;

			if use_local {
				if let Some(sender) = self.local_subscribers.get_async(&*subject_cow).await {
					let _ = sender.send(encoded);
				} else {
					tracing::warn!(%subject, "local subscriber disappeared");
					break;
				}
			} else {
				// Use backoff when publishing through the driver
				let subject = subject.as_cow();

				let mut backoff = Backoff::default();
				loop {
					let measure = perf_start!(
						&metrics::PUBLISH_ATTEMPT_DURATION,
						slow_ms = 50,
						"ups_publish_attempt",
						labels: { subject_root = %subject_root },
						fields: { subject = %subject },
					);
					let res = self
						.driver
						.publish(&subject, &encoded, reply_subject.as_deref())
						.await;
					perf_finish!(measure, fields: { result = %res.is_ok() });

					match res {
						Result::Ok(_) => {
							break;
						}
						Err(err) if !backoff.tick().await => {
							metrics::PUBLISH_RETRY_TOTAL
								.with_label_values(&[subject_root])
								.inc();
							tracing::warn!(?err, "error publishing, cannot retry again");
							return Err(errors::Ups::PublishFailed.build().into());
						}
						Err(err) => {
							metrics::PUBLISH_RETRY_TOTAL
								.with_label_values(&[subject_root])
								.inc();
							tracing::debug!(?err, "error publishing, retrying");
							// Continue retrying
						}
					}
				}
			}
		}

		if use_local {
			metrics::MESSAGE_SEND_COUNT
				.with_label_values(&["local", subject_root])
				.inc();
		} else {
			metrics::MESSAGE_SEND_COUNT
				.with_label_values(&["driver", subject_root])
				.inc();
		}

		Ok(message_id)
	}

	#[tracing::instrument(skip_all)]
	pub async fn flush(&self) -> Result<()> {
		self.driver.flush().await
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	pub async fn request(&self, subject: impl Subject, payload: &[u8]) -> Result<NextOutput> {
		self.request_with_timeout(subject, payload, Duration::from_secs(30))
			.await
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	pub async fn request_with_timeout(
		&self,
		subject: impl Subject,
		payload: &[u8],
		timeout: Duration,
	) -> Result<NextOutput> {
		self.request_with_timeout_inner(subject, payload, timeout)
			.await
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	pub async fn request_with_timeout_inner<T: Subject>(
		&self,
		subject: T,
		payload: &[u8],
		timeout: Duration,
	) -> Result<NextOutput> {
		let start = Instant::now();
		let reply_subject = self.driver.new_inbox();
		let now = rivet_util::timestamp::now();
		let request_deadline_at = i64::try_from(timeout.as_millis())
			.ok()
			.and_then(|timeout_ms| now.checked_add(timeout_ms));

		// Subscribe to the reply subject (use local-aware subscribe)
		let mut reply_subscriber = self
			.subscribe_inner(reply_subject.clone(), Some(reply_subject.id))
			.await?;

		// Send the request with the reply subject, using local fast-path
		self.publish_inner(
			subject,
			payload,
			Some(&reply_subject),
			PublishOpts::one(),
			request_deadline_at,
		)
		.await?;

		// Wait for response with timeout
		match tokio::time::timeout(timeout, reply_subscriber.next()).await {
			std::result::Result::Ok(std::result::Result::Ok(output)) => {
				let subject_str = T::root();
				let subject_str = subject_str.as_deref().unwrap_or("unknown");
				metrics::REQUEST_RESPONSE_LAG
					.with_label_values(&[subject_str])
					.observe(start.elapsed().as_secs_f64());

				Ok(output)
			}
			std::result::Result::Ok(std::result::Result::Err(_)) => {
				Err(errors::Ups::RequestTimeout.build().into())
			}
			std::result::Result::Err(_) => {
				let subject_str = T::root();
				let subject_str = subject_str.as_deref().unwrap_or("unknown");
				metrics::REQUEST_TIMEOUT_COUNT
					.with_label_values(&[subject_str])
					.inc();

				Err(errors::Ups::RequestTimeout.build().into())
			}
		}
	}

	#[tracing::instrument(skip_all, fields(%subject))]
	async fn should_use_local_subscriber(
		&self,
		subject: &impl Subject,
		behavior: crate::driver::PublishBehavior,
	) -> bool {
		// Local fast-path for one-subscriber behavior:
		// - When memory_optimization is enabled and behavior == OneSubscriber, deliver directly
		//   to any in-process subscribers via the subject's broadcast channel and skip calling
		//   the underlying driver (avoids network hops and driver overhead).
		// - For Broadcast, always publish via the driver so remote subscribers (and other
		//   processes) receive the message; local subscribers will also receive via the driver.
		// - If there are no local receivers at the time of publish (or the channel disappears),
		//   fall back to the driver publish path.

		if !self.memory_optimization {
			return false;
		}
		if !matches!(behavior, crate::driver::PublishBehavior::OneSubscriber) {
			return false;
		}
		if let Some(sender) = self.local_subscribers.get_async(&*subject.as_cow()).await {
			sender.receiver_count() > 0
		} else {
			false
		}
	}
}

pub struct Subscriber {
	driver: SubscriberDriverHandle,
	pubsub: PubSub,
	memory_optimization: bool,
	subject: String,
	root_subject: Option<String>,
}

impl Subscriber {
	fn new(
		driver: SubscriberDriverHandle,
		pubsub: PubSub,
		memory_optimization: bool,
		subject: String,
		root_subject: Option<String>,
	) -> Self {
		let subject_str = if let Some(root_subject) = &root_subject {
			root_subject.as_str()
		} else {
			"unknown"
		};
		metrics::SUBSCRIBER_COUNT
			.with_label_values(&[subject_str])
			.inc();
		metrics::ACTIVE_SUBSCRIBER_COUNT
			.with_label_values(&[subject_str])
			.inc();

		Self {
			driver,
			pubsub,
			memory_optimization,
			subject,
			root_subject,
		}
	}

	#[tracing::instrument(skip_all, fields(subject=%self.subject, message_id = tracing::field::Empty))]
	pub async fn next(&mut self) -> Result<NextOutput> {
		loop {
			match self.driver.next().await? {
				DriverOutput::Message {
					subject: _,
					payload,
				} => {
					// Sync fast path skips the scc::HashMap entry for single-chunk messages.
					let decoded = match self.pubsub.chunk_tracker.try_process_chunk_fast(&payload) {
						std::result::Result::Ok(FastPath::Decoded(decoded)) => decoded,
						std::result::Result::Ok(FastPath::Multi(message)) => {
							match self.pubsub.chunk_tracker.process_chunk_async(message).await {
								std::result::Result::Ok(Some(decoded)) => decoded,
								std::result::Result::Ok(None) => continue, // Waiting for more chunks
								std::result::Result::Err(e) => {
									tracing::warn!(?e, "failed to process chunk");
									continue;
								}
							}
						}
						std::result::Result::Err(e) => {
							tracing::warn!(?e, "failed to process chunk");
							continue;
						}
					};

					let secs = rivet_util::timestamp::now().saturating_sub(decoded.timestamp)
						as f64 / 1000.0;
					metrics::MESSAGE_RECV_LAG
						.with_label_values(&[if let Some(root_subject) = &self.root_subject {
							root_subject.as_str()
						} else {
							"unknown"
						}])
						.observe(secs);
					metrics::MESSAGE_RECV_COUNT
						.with_label_values(&[if let Some(root_subject) = &self.root_subject {
							root_subject.as_str()
						} else {
							"unknown"
						}])
						.inc();

					metrics::BYTES_PER_MESSAGE
						.with_label_values(&[if let Some(root_subject) = &self.root_subject {
							root_subject.as_str()
						} else {
							"unknown"
						}])
						.observe(decoded.payload.len() as f64);

					tracing::Span::current().record("message_id", decoded.message_id.to_string());

					return Ok(NextOutput::Message(Message {
						message_id: decoded.message_id,
						pubsub: self.pubsub.clone(),
						payload: decoded.payload,
						reply: decoded.reply_subject,
						request_deadline_at: decoded.request_deadline_at,
					}));
				}
				DriverOutput::Unsubscribed => return Ok(NextOutput::Unsubscribed),
				DriverOutput::NoResponders => return Ok(NextOutput::NoResponders),
			}
		}
	}
}

impl Drop for Subscriber {
	fn drop(&mut self) {
		metrics::ACTIVE_SUBSCRIBER_COUNT
			.with_label_values(&[if let Some(root_subject) = &self.root_subject {
				root_subject.as_str()
			} else {
				"unknown"
			}])
			.dec();

		// Clean up local subscriber entry immediately if memory_optimization was enabled
		if self.memory_optimization {
			let pubsub = self.pubsub.clone();
			let subject = self.subject.clone();
			tokio::spawn(async move {
				if let Some(sender) = pubsub.local_subscribers.get_async(&subject).await {
					if sender.receiver_count() == 0 {
						let _ = sender.remove();
						metrics::LOCAL_SUBSCRIBER_COUNT.set(pubsub.local_subscribers.len() as i64);
					}
				}
			});
		}
	}
}

// Output from drivers (raw binary messages)
pub enum DriverOutput {
	Message { subject: String, payload: Vec<u8> },
	Unsubscribed,
	NoResponders,
}

// Output from subscriber (after chunking/decoding)
pub enum NextOutput {
	Message(Message),
	Unsubscribed,
	NoResponders,
}

impl From<NextOutput> for Option<Message> {
	fn from(value: NextOutput) -> Self {
		match value {
			NextOutput::Message(msg) => Some(msg),
			NextOutput::Unsubscribed | NextOutput::NoResponders => None,
		}
	}
}

pub struct Message {
	pub message_id: Uuid,
	pub pubsub: PubSub,
	pub payload: Vec<u8>,
	pub reply: Option<String>,
	pub request_deadline_at: Option<i64>,
}

impl Message {
	#[tracing::instrument(skip_all, fields(message_id=?self.message_id, reply_subject=?self.reply, request_deadline_at=?self.request_deadline_at))]
	pub async fn reply(&self, payload: &[u8]) -> Result<()> {
		if let Some(ref reply_subject) = self.reply {
			if self.is_request_expired() {
				return Err(errors::Ups::RequestTimeout.build().into());
			}

			// Replies expect exactly one subscriber and should use local fast-path
			if let Some(reply_subject) = InboxSubject::from_existing(reply_subject) {
				self.pubsub
					.publish(reply_subject, payload, PublishOpts::one())
					.await?;
			} else {
				self.pubsub
					.publish(reply_subject, payload, PublishOpts::one())
					.await?;
			}
		}
		Ok(())
	}

	pub fn is_request_expired(&self) -> bool {
		self.request_deadline_at
			.is_some_and(|deadline_at| rivet_util::timestamp::now() >= deadline_at)
	}
}

/// Internal composite subscriber that merges driver messages with local in-memory messages
struct LocalOptimizedSubscriberDriver {
	subject: String,
	driver: SubscriberDriverHandle,
	local_rx: broadcast::Receiver<Vec<u8>>,
}

#[async_trait::async_trait]
impl crate::driver::SubscriberDriver for LocalOptimizedSubscriberDriver {
	#[tracing::instrument(skip_all)]
	async fn next(&mut self) -> Result<DriverOutput> {
		loop {
			tokio::select! {
				biased;
				// Prefer local messages to reduce latency
				res = self.local_rx.recv() => {
					match res {
						std::result::Result::Ok(payload) => {
							return Ok(DriverOutput::Message { subject: self.subject.clone(), payload });
						}
						std::result::Result::Err(broadcast::error::RecvError::Lagged(_)) => {
							// Skip lagged and continue
							continue;
						}
						std::result::Result::Err(broadcast::error::RecvError::Closed) => {
							// Local channel closed; fall back to driver only
							// Replace with a closed receiver to avoid busy loop
							// We simply continue and rely on driver
						}
					}
				}
				res = self.driver.next() => {
					return res;
				}
			}
		}
	}
}
