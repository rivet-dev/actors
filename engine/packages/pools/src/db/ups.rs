use std::{
	str::FromStr,
	sync::{Arc, atomic::Ordering},
	time::Duration,
};

use anyhow::*;
use rivet_config::{Config, config};
use universalpubsub as ups;

use crate::Error;

pub type UpsPool = ups::PubSub;

#[tracing::instrument(skip(config))]
pub async fn setup(config: &Config, client_name: &str) -> Result<UpsPool> {
	let driver = match config.pubsub() {
		config::PubSub::Nats(nats) => {
			// Parse nodes
			let server_addrs = nats
				.addresses
				.iter()
				.map(|addr| format!("nats://{addr}"))
				.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
				.collect::<Result<Vec<_>, _>>()
				.map_err(Error::BuildNatsIo)?;

			let mut options =
				if let (Some(username), Some(password)) = (&nats.username, &nats.password) {
					async_nats::ConnectOptions::with_user_and_password(
						username.clone(),
						password.read().clone(),
					)
				} else {
					async_nats::ConnectOptions::new()
				};

			options = options
				.client_capacity(nats.client_capacity)
				.subscription_capacity(nats.subscription_capacity)
				.event_callback({
					let server_addrs = server_addrs.clone();
					move |event| {
						let server_addrs = server_addrs.clone();
						async move {
							match event {
								async_nats::Event::Connected => {
									tracing::info!(?server_addrs, "nats reconnected");
								}
								async_nats::Event::Disconnected => {
									tracing::warn!(?server_addrs, "nats disconnected");
								}
								async_nats::Event::LameDuckMode => {
									tracing::warn!(?server_addrs, "nats lame duck mode");
								}
								async_nats::Event::Draining => {
									tracing::warn!(?server_addrs, "nats draining");
								}
								async_nats::Event::Closed => {
									// Engine is shutting down, not an error
									tracing::info!(?server_addrs, "nats closed");
								}
								async_nats::Event::SlowConsumer(slow_consumer) => {
									let root = ups::subject::subject_root_from_str(
										slow_consumer.subject.as_str(),
									);
									ups::metrics::NATS_SLOW_CONSUMER_TOTAL
										.with_label_values(&[root])
										.inc();
									tracing::warn!(
										?server_addrs,
										sid = ?slow_consumer.sid,
										subject = %slow_consumer.subject,
										root,
										"nats slow consumer"
									);
								}
								async_nats::Event::ServerError(err) => {
									tracing::error!(?server_addrs, ?err, "nats server error");
								}
								async_nats::Event::ClientError(err) => {
									tracing::error!(?server_addrs, ?err, "nats client error");
								}
							}
						}
					}
				});

			// NATS has built in backoff with jitter (with max of 4s), so
			// once the connection is established, we never have to worry
			// about disconnections that aren't handled by NATS.
			let driver = ups::driver::nats::NatsDriver::connect(options, &server_addrs[..]).await?;
			spawn_nats_statistics_task(driver.statistics());

			Arc::new(driver) as ups::PubSubDriverHandle
		}
		config::PubSub::Memory(memory) => {
			tracing::debug!(channel=%memory.channel, "creating memory pubsub driver");
			Arc::new(ups::driver::memory::MemoryDriver::new(
				memory.channel.clone(),
			)) as ups::PubSubDriverHandle
		}
	};

	let disable_memory_optimization = match config.pubsub() {
		config::PubSub::Nats(nats) => nats.disable_memory_optimization,
		config::PubSub::Memory(memory) => memory.disable_memory_optimization,
	};
	Ok(ups::PubSub::new_with_memory_optimization(
		driver,
		!disable_memory_optimization,
	))
}

fn spawn_nats_statistics_task(statistics: Arc<async_nats::Statistics>) {
	tokio::spawn(async move {
		let mut interval = tokio::time::interval(Duration::from_secs(10));
		interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

		let mut last_in_messages = 0;
		let mut last_out_messages = 0;
		let mut last_in_bytes = 0;
		let mut last_out_bytes = 0;
		let mut last_dropped_messages = 0;
		let mut last_dropped_bytes = 0;

		loop {
			interval.tick().await;

			let in_messages = statistics.in_messages.load(Ordering::Relaxed);
			let out_messages = statistics.out_messages.load(Ordering::Relaxed);
			let in_bytes = statistics.in_bytes.load(Ordering::Relaxed);
			let out_bytes = statistics.out_bytes.load(Ordering::Relaxed);
			let pending_messages = statistics
				.subscription_pending_messages
				.load(Ordering::Relaxed);
			let pending_bytes = statistics
				.subscription_pending_bytes
				.load(Ordering::Relaxed);
			let dropped_messages = statistics
				.subscription_dropped_messages
				.load(Ordering::Relaxed);
			let dropped_bytes = statistics
				.subscription_dropped_bytes
				.load(Ordering::Relaxed);
			let active_subscriptions = statistics.active_subscriptions.load(Ordering::Relaxed);
			let active_subscription_capacity = statistics
				.active_subscription_capacity
				.load(Ordering::Relaxed);

			ups::metrics::NATS_CLIENT_IN_MESSAGES_TOTAL
				.inc_by(in_messages.saturating_sub(last_in_messages));
			ups::metrics::NATS_CLIENT_OUT_MESSAGES_TOTAL
				.inc_by(out_messages.saturating_sub(last_out_messages));
			ups::metrics::NATS_CLIENT_IN_BYTES_TOTAL.inc_by(in_bytes.saturating_sub(last_in_bytes));
			ups::metrics::NATS_CLIENT_OUT_BYTES_TOTAL
				.inc_by(out_bytes.saturating_sub(last_out_bytes));
			ups::metrics::NATS_SUBSCRIPTION_PENDING_MESSAGES.set(u64_to_i64(pending_messages));
			ups::metrics::NATS_SUBSCRIPTION_PENDING_BYTES.set(u64_to_i64(pending_bytes));
			ups::metrics::NATS_ACTIVE_SUBSCRIPTIONS.set(u64_to_i64(active_subscriptions));
			ups::metrics::NATS_ACTIVE_SUBSCRIPTION_CAPACITY
				.set(u64_to_i64(active_subscription_capacity));
			ups::metrics::NATS_SUBSCRIPTION_DROPPED_MESSAGES_TOTAL
				.inc_by(dropped_messages.saturating_sub(last_dropped_messages));
			ups::metrics::NATS_SUBSCRIPTION_DROPPED_BYTES_TOTAL
				.inc_by(dropped_bytes.saturating_sub(last_dropped_bytes));

			last_in_messages = in_messages;
			last_out_messages = out_messages;
			last_in_bytes = in_bytes;
			last_out_bytes = out_bytes;
			last_dropped_messages = dropped_messages;
			last_dropped_bytes = dropped_bytes;
		}
	});
}

fn u64_to_i64(value: u64) -> i64 {
	value.min(i64::MAX as u64) as i64
}
