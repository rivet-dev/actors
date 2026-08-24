use std::{
	fmt::{Debug, Display},
	future::Future,
	time::Duration,
};

use anyhow::Result;
use futures_util::StreamExt;
use itertools::{Either, Itertools};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::broadcast;

use super::*;
use crate::{errors::Error, metrics};

/// How long to wait for an in flight cache req before proceeding to execute the same req anyway.
const IN_FLIGHT_TIMEOUT: Duration = Duration::from_secs(5);

/// Config specifying how cached values will behave.
#[derive(Clone)]
pub struct RequestConfig {
	pub(super) cache: Cache,
	ttl: i64,
}

impl Debug for RequestConfig {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RequestConfig")
			.field("cache", &self.cache)
			.field("ttl", &self.ttl)
			.finish()
	}
}

impl RequestConfig {
	pub(crate) fn new(cache: Cache) -> Self {
		RequestConfig {
			cache,
			ttl: rivet_util::duration::hours(2),
		}
	}

	/// Sets the TTL for the keys in ms.
	///
	/// Defaults to 2 hours.
	pub fn ttl(mut self, ttl: i64) -> Self {
		self.ttl = ttl;
		self
	}
}

// MARK: Fetch
impl RequestConfig {
	#[tracing::instrument(err, skip_all, fields(%base_key))]
	async fn fetch_all_convert<Key, Value, Getter, Fut, Encoder, Decoder>(
		self,
		base_key: impl Display + Debug,
		keys: impl IntoIterator<Item = Key>,
		getter: Getter,
		encoder: Encoder,
		decoder: Decoder,
	) -> Result<Vec<(Key, Value)>>
	where
		Key: CacheKey + Send + Sync,
		Value: Debug + Send + Sync,
		Getter: Fn(GetterCtx<Key, Value>, Vec<Key>) -> Fut + Clone,
		Fut: Future<Output = Result<GetterCtx<Key, Value>>>,
		Encoder: Fn(&Value) -> Result<Vec<u8>> + Clone,
		Decoder: Fn(&[u8]) -> Result<Value> + Clone,
	{
		let base_key = base_key.to_string();
		let keys = keys.into_iter().collect::<Vec<Key>>();

		// Ignore empty keys
		if keys.is_empty() {
			return Ok(Vec::new());
		}

		metrics::CACHE_REQUEST_TOTAL
			.with_label_values(&[base_key.as_str()])
			.inc();
		metrics::CACHE_VALUE_TOTAL
			.with_label_values(&[base_key.as_str()])
			.inc_by(keys.len() as u64);

		let mut ctx = GetterCtx::new(keys);

		// No driver (cache disabled)
		let Some(driver) = &self.cache.driver else {
			let keys = ctx.unresolved_keys();
			let ctx = getter(ctx, keys).await.map_err(Error::Getter)?;

			metrics::CACHE_VALUE_EMPTY_TOTAL
				.with_label_values(&[&base_key])
				.inc_by(ctx.unresolved_keys().len() as u64);

			return Ok(ctx.into_values());
		};

		// Build driver-specific cache keys
		let (keys, cache_keys): (Vec<_>, Vec<_>) = ctx
			.entries()
			.map(|(key, _)| (key.clone(), driver.process_key(&base_key, key)))
			.unzip();
		let cache_keys_len = cache_keys.len();

		// Attempt to fetch value from cache, fall back to getter
		match driver.get(&base_key, &cache_keys).await {
			Ok(cached_values) => {
				debug_assert_eq!(
					cache_keys_len,
					cached_values.len(),
					"cache returned wrong number of values"
				);

				// Resolve the cached values
				for (key, value) in keys.iter().zip(cached_values.into_iter()) {
					if let Some(value_bytes) = value {
						// Try to decode the value using the driver
						match decoder(&value_bytes) {
							Ok(value) => {
								ctx.resolve_from_cache(key, value);
							}
							Err(err) => {
								tracing::error!(?err, "Failed to decode value");
							}
						}
					}
				}

				// Fetch remaining values and add to the cached list
				if !ctx.all_entries_have_value() {
					// Call the getter
					let remaining_keys = ctx.unresolved_keys();
					let unresolved_len = remaining_keys.len();

					metrics::CACHE_VALUE_MISS_TOTAL
						.with_label_values(&[base_key.as_str()])
						.inc_by(unresolved_len as u64);

					let mut waiting_keys = Vec::new();
					let mut leased_keys = Vec::new();
					let (broadcast_tx, _) = broadcast::channel::<()>(16);

					// Determine which keys are currently being fetched and not
					for key in remaining_keys {
						let cache_key = driver.process_key(&base_key, &key);
						match self.cache.in_flight().entry_async(cache_key).await {
							scc::hash_map::Entry::Occupied(broadcast) => {
								waiting_keys.push((key, broadcast.subscribe()));
							}
							scc::hash_map::Entry::Vacant(entry) => {
								entry.insert_entry(broadcast_tx.clone());
								leased_keys.push(key);
							}
						}
					}

					let getter2 = getter.clone();
					let ctx2 = GetterCtx::new(leased_keys.clone());
					let base_key2 = base_key.clone();
					let leased_keys2 = leased_keys.clone();
					let (ctx2, ctx3) = tokio::try_join!(
						async move {
							if leased_keys2.is_empty() {
								Ok(ctx2)
							} else {
								getter2(ctx2, leased_keys2).await.map_err(Error::Getter)
							}
						},
						async move {
							let ctx3 = GetterCtx::new(
								waiting_keys.iter().map(|(key, _)| key.clone()).collect(),
							);

							// Wait on keys that are being fetched by another cache req
							let (succeeded_keys, failed_keys): (Vec<_>, Vec<_>) =
								futures_util::stream::iter(waiting_keys)
									.map(|(key, mut rx)| async move {
										(
											key,
											tokio::time::timeout(IN_FLIGHT_TIMEOUT, rx.recv())
												.await
												.ok()
												.map(|x| x.ok())
												.flatten()
												.is_some(),
										)
									})
									.buffer_unordered(1024)
									.collect::<Vec<_>>()
									.await
									.into_iter()
									.partition_map(|(key, succeeded)| {
										if succeeded {
											let cache_key = driver.process_key(&base_key2, &key);
											Either::Left((key, cache_key))
										} else {
											Either::Right(key)
										}
									});
							let (succeeded_keys, succeeded_cache_keys): (Vec<_>, Vec<_>) =
								succeeded_keys.into_iter().unzip();

							let (cached_values_res, ctx3_res) = tokio::join!(
								async {
									if succeeded_cache_keys.is_empty() {
										Ok(Vec::new())
									} else {
										driver.get(&base_key2, &succeeded_cache_keys).await
									}
								},
								async {
									if failed_keys.is_empty() {
										Ok(ctx3)
									} else {
										getter(ctx3, failed_keys).await.map_err(Error::Getter)
									}
								},
							);
							let mut ctx3 = ctx3_res?;

							match cached_values_res {
								Ok(cached_values) => {
									for (key, value) in
										succeeded_keys.iter().zip(cached_values.into_iter())
									{
										if let Some(value_bytes) = value {
											// Try to decode the value using the driver
											match decoder(&value_bytes) {
												Ok(value) => {
													ctx3.resolve_from_cache(key, value);
												}
												Err(err) => {
													tracing::error!(?err, "Failed to decode value");
												}
											}
										}
									}
								}
								Err(err) => {
									tracing::error!(?err, "failed to read batch keys from cache");

									metrics::CACHE_REQUEST_ERRORS
										.with_label_values(&[&base_key2])
										.inc();
								}
							}

							Ok(ctx3)
						}
					)?;

					ctx.merge(ctx2);
					ctx.merge(ctx3);

					// Write the values to cache
					let expire_at = rivet_util::timestamp::now() + self.ttl;
					let entries_needing_cache_write = ctx.entries_needing_cache_write();

					tracing::trace!(
						unresolved_len,
						fetched_len = entries_needing_cache_write.len(),
						"writing new values to cache"
					);

					// Convert values to cache bytes
					let entries_values = entries_needing_cache_write
						.into_iter()
						.filter_map(|(key, value)| {
							// Process the key with the appropriate driver
							let cache_key = driver.process_key(&base_key, key);
							// Try to decode the value using the driver
							match encoder(value) {
								Ok(value_bytes) => Some((cache_key, value_bytes, expire_at)),
								Err(err) => {
									tracing::error!(?err, "Failed to encode value");

									None
								}
							}
						})
						.collect::<Vec<_>>();

					if !entries_values.is_empty() {
						let base_key_clone = base_key.clone();

						if let Err(err) = driver.set(&base_key_clone, entries_values).await {
							tracing::error!(?err, "failed to write to cache");
						}

						let _ = broadcast_tx.send(());
					}

					// Release leases
					for key in leased_keys {
						let cache_key = driver.process_key(&base_key, &key);
						self.cache.in_flight().remove_async(&cache_key).await;
					}
				}

				metrics::CACHE_VALUE_EMPTY_TOTAL
					.with_label_values(&[&base_key])
					.inc_by(ctx.unresolved_keys().len() as u64);

				Ok(ctx.into_values())
			}
			Err(err) => {
				tracing::error!(
					?err,
					"failed to read batch keys from cache, falling back to getter"
				);

				let keys = ctx.unresolved_keys();

				metrics::CACHE_REQUEST_ERRORS
					.with_label_values(&[&base_key])
					.inc();
				metrics::CACHE_VALUE_MISS_TOTAL
					.with_label_values(&[base_key.as_str()])
					.inc_by(keys.len() as u64);

				// Fall back to the getter since we can't fetch the value from
				// the cache
				let ctx = getter(ctx, keys).await.map_err(Error::Getter)?;

				metrics::CACHE_VALUE_EMPTY_TOTAL
					.with_label_values(&[&base_key])
					.inc_by(ctx.unresolved_keys().len() as u64);

				Ok(ctx.into_values())
			}
		}
	}

	#[tracing::instrument(err, skip_all, fields(%base_key))]
	pub async fn purge<Key>(
		self,
		base_key: impl Display + Debug,
		keys: impl IntoIterator<Item = Key>,
	) -> Result<()>
	where
		Key: CacheKey + Send + Sync,
	{
		// Cache disabled
		let Some(driver) = &self.cache.driver else {
			return Ok(());
		};

		// Build keys
		let base_key = base_key.to_string();
		let cache_keys = keys
			.into_iter()
			.map(|key| driver.process_key(&base_key, &key))
			.collect::<Vec<_>>();

		if cache_keys.is_empty() {
			return Ok(());
		}

		// Publish cache purge message to all services via UPS
		if let Some(ups) = &self.cache.ups {
			let message = CachePurgeMessage {
				base_key: base_key.clone(),
				keys: cache_keys.clone(),
			};

			let payload = rivet_util::serde::json_to_vec!(&message)?;

			if let Err(err) = ups
				.publish(
					CachePurgeSubject,
					&payload,
					universalpubsub::PublishOpts::broadcast(),
				)
				.await
			{
				tracing::error!(?err, "failed to publish cache purge message");
			} else {
				tracing::debug!(
					base_key,
					keys_count = cache_keys.len(),
					"published cache purge message"
				);
			}
		}

		// Delete keys locally
		self.purge_local(&base_key, cache_keys).await
	}

	/// Purges keys from the local cache without publishing to NATS.
	/// This is used by the cache-purge service to avoid recursive publishing.
	#[tracing::instrument(err, skip_all, fields(%base_key))]
	pub async fn purge_local(
		self,
		base_key: impl Display + Debug,
		keys: Vec<RawCacheKey>,
	) -> Result<()> {
		// Cache disabled
		let Some(driver) = &self.cache.driver else {
			return Ok(());
		};

		let base_key = base_key.to_string();

		if keys.is_empty() {
			return Ok(());
		}

		metrics::CACHE_PURGE_REQUEST_TOTAL
			.with_label_values(&[&base_key])
			.inc();
		metrics::CACHE_PURGE_VALUE_TOTAL
			.with_label_values(&[&base_key])
			.inc_by(keys.len() as u64);

		// Delete keys locally
		match driver.delete(&base_key, keys).await {
			Ok(_) => {
				tracing::trace!("successfully deleted keys");
			}
			Err(err) => {
				tracing::error!(?err, "failed to delete from cache, proceeding regardless")
			}
		}

		Ok(())
	}
}

// MARK: JSON fetch
impl RequestConfig {
	pub async fn fetch_one_json<Key, Value, Getter, Fut>(
		self,
		base_key: impl Display + Debug,
		key: Key,
		getter: Getter,
	) -> Result<Option<Value>>
	where
		Key: CacheKey + Send + Sync,
		Value: Serialize + DeserializeOwned + Debug + Send + Sync,
		Getter: Fn(GetterCtx<Key, Value>, Key) -> Fut + Clone,
		Fut: Future<Output = Result<GetterCtx<Key, Value>>>,
	{
		let values = self
			.fetch_all_json_with_keys(base_key, [key], move |cache, keys| {
				let getter = getter.clone();
				async move {
					debug_assert_eq!(1, keys.len());
					if let Some(key) = keys.into_iter().next() {
						getter(cache, key).await
					} else {
						tracing::error!("no keys provided to fetch one");
						Ok(cache)
					}
				}
			})
			.await?;
		Ok(values.into_iter().next().map(|(_, v)| v))
	}

	pub async fn fetch_all_json<Key, Value, Getter, Fut>(
		self,
		base_key: impl Display + Debug,
		keys: impl IntoIterator<Item = Key>,
		getter: Getter,
	) -> Result<Vec<Value>>
	where
		Key: CacheKey + Send + Sync,
		Value: Serialize + DeserializeOwned + Debug + Send + Sync,
		Getter: Fn(GetterCtx<Key, Value>, Vec<Key>) -> Fut + Clone,
		Fut: Future<Output = Result<GetterCtx<Key, Value>>>,
	{
		self.fetch_all_json_with_keys::<Key, Value, Getter, Fut>(base_key, keys, getter)
			.await
			// TODO: Find a way to not allocate another vec here
			.map(|x| x.into_iter().map(|(_, v)| v).collect::<Vec<_>>())
	}

	pub async fn fetch_all_json_with_keys<Key, Value, Getter, Fut>(
		self,
		base_key: impl Display + Debug,
		keys: impl IntoIterator<Item = Key>,
		getter: Getter,
	) -> Result<Vec<(Key, Value)>>
	where
		Key: CacheKey + Send + Sync,
		Value: Serialize + DeserializeOwned + Debug + Send + Sync,
		Getter: Fn(GetterCtx<Key, Value>, Vec<Key>) -> Fut + Clone,
		Fut: Future<Output = Result<GetterCtx<Key, Value>>>,
	{
		self.fetch_all_convert(
			base_key,
			keys,
			getter,
			|value: &Value| -> Result<Vec<u8>> {
				rivet_util::serde::json_to_vec!(&value)
					.map_err(Error::SerdeEncode)
					.map_err(Into::into)
			},
			|value: &[u8]| -> Result<Value> {
				rivet_util::serde::json_from_slice!(value)
					.map_err(Error::SerdeDecode)
					.map_err(Into::into)
			},
		)
		.await
	}
}
