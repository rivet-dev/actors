//! Implementation of a workflow database driver with UniversalDB and UniversalPubSub.
// TODO: Move code to smaller functions for readability

use std::{
	collections::{HashMap, HashSet},
	hash::{DefaultHasher, Hash, Hasher},
	sync::Arc,
	time::{Duration, Instant},
};

use anyhow::{Context, Result};
use futures_util::{StreamExt, TryStreamExt, future::try_join_all, stream::BoxStream};
use rivet_util::Id;
use rivet_util::future::CustomInstrumentExt;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::Instrument;
use universaldb::prelude::*;
use universaldb::utils::end_of_key_range;

use super::{BumpSubSubject, Database, PulledWorkflowData, SignalData, WorkflowData};
use crate::{
	error::{WorkflowError, WorkflowResult},
	history::{
		event::{
			ActivityEvent, Event, EventData, EventType, LoopEvent, MessageSendEvent, RemovedEvent,
			SignalSendEvent, SignalsEvent, SleepEvent, SleepState, SubWorkflowEvent,
			VersionCheckEvent,
		},
		location::Location,
	},
	metrics,
	worker::PING_INTERVAL,
	workflow::PruneVariant,
};

mod debug;
mod keys;
mod subjects;
mod system;

/// How long before considering the leases of a given worker expired.
const WORKER_LOST_THRESHOLD_MS: i64 = rivet_util::duration::seconds(30);
/// How long before overwriting an existing metrics lock.
const METRICS_LOCK_TIMEOUT_MS: i64 = rivet_util::duration::seconds(30);
const EARLY_TXN_TIMEOUT: Duration = Duration::from_millis(2500);

pub struct DatabaseKv {
	config: rivet_config::Config,
	pools: rivet_pools::Pools,
	subspace: universaldb::utils::Subspace,
	system: Arc<Mutex<system::SystemInfo>>,
}

impl DatabaseKv {
	/// Spawns a new thread and gracefully publishes a bump message to pubsub.
	fn bump(&self, subject: BumpSubSubject) {
		let Ok(pubsub) = self.pools.ups() else {
			tracing::debug!("failed to acquire pubsub pool");
			return;
		};

		let spawn_res = tokio::task::Builder::new().name("bump").spawn(
			async move {
				// Fail gracefully
				if let Err(err) = pubsub
					.publish(
						subject,
						&Vec::new(),
						universalpubsub::PublishOpts::broadcast(),
					)
					.await
				{
					tracing::warn!(?err, "failed to publish bump message");
				}
			}
			.instrument(tracing::info_span!("bump_worker_publish")),
		);
		if let Err(err) = spawn_res {
			tracing::error!(?err, "failed to spawn bump task");
		}
	}
}

// MARK: UDB Helpers
impl DatabaseKv {
	fn write_signal_wake_idxs(
		&self,
		workflow_id: Id,
		wake_signals: &[&str],
		tx: &universaldb::Transaction,
	) -> Result<()> {
		for signal_name in wake_signals {
			// Write to wake signals list
			let wake_signal_key =
				keys::workflow::WakeSignalKey::new(workflow_id, signal_name.to_string());
			tx.set(
				&self.subspace.pack(&wake_signal_key),
				&wake_signal_key.serialize(())?,
			);
		}

		Ok(())
	}

	fn write_sub_workflow_wake_idx(
		&self,
		workflow_id: Id,
		workflow_name: &str,
		sub_workflow_id: Id,
		tx: &universaldb::Transaction,
	) -> Result<()> {
		let sub_workflow_wake_key =
			keys::wake::SubWorkflowWakeKey::new(sub_workflow_id, workflow_id);

		tx.set(
			&self.subspace.pack(&sub_workflow_wake_key),
			&sub_workflow_wake_key.serialize(workflow_name.to_string())?,
		);

		Ok(())
	}

	async fn publish_signal_inner(
		&self,
		ray_id: Id,
		workflow_id: Id,
		signal_id: Id,
		signal_name: &str,
		body: &serde_json::value::RawValue,
		tx: &universaldb::Transaction,
	) -> Result<()> {
		tracing::debug!(
			?ray_id,
			?workflow_id,
			?signal_id,
			?signal_name,
			"publishing signal"
		);

		let workflow_name_key = keys::workflow::NameKey::new(workflow_id);
		let wake_signal_key =
			keys::workflow::WakeSignalKey::new(workflow_id, signal_name.to_string());

		let (workflow_name_entry, wake_signal_entry) = tokio::try_join!(
			tx.get(&self.subspace.pack(&workflow_name_key), Serializable),
			tx.get(&self.subspace.pack(&wake_signal_key), Serializable),
		)?;

		// TODO: This does not check if the workflow is silenced
		// Check if the workflow exists
		let Some(workflow_name_entry) = workflow_name_entry else {
			return Err(WorkflowError::WorkflowNotFound.into());
		};

		let workflow_name = workflow_name_key.deserialize(&workflow_name_entry)?;

		// Write name
		let name_key = keys::signal::NameKey::new(signal_id);
		tx.set(
			&self.subspace.pack(&name_key),
			&name_key.serialize(signal_name.to_string())?,
		);

		let signal_body_key = keys::signal::BodyKey::new(signal_id);

		// Write signal body
		for (i, chunk) in signal_body_key.split_ref(body)?.into_iter().enumerate() {
			let chunk_key = signal_body_key.chunk(i);

			tx.set(&self.subspace.pack(&chunk_key), &chunk);
		}

		// Write pending key
		let pending_signal_key =
			keys::workflow::PendingSignalKey::new(workflow_id, signal_name.to_string(), signal_id);

		tx.set(
			&self.subspace.pack(&pending_signal_key),
			&pending_signal_key.serialize(())?,
		);

		// Write create ts
		let create_ts_key = keys::signal::CreateTsKey::new(signal_id);
		tx.set(
			&self.subspace.pack(&create_ts_key),
			&create_ts_key.serialize(pending_signal_key.ts)?,
		);

		// Write ray id
		let ray_id_key = keys::signal::RayIdKey::new(signal_id);
		tx.set(
			&self.subspace.pack(&ray_id_key),
			&ray_id_key.serialize(ray_id)?,
		);

		// Write workflow id
		let workflow_id_key = keys::signal::WorkflowIdKey::new(signal_id);
		tx.set(
			&self.subspace.pack(&workflow_id_key),
			&workflow_id_key.serialize(workflow_id)?,
		);

		// If the workflow currently has a wake signal key for this signal, wake it
		if wake_signal_entry.is_some() {
			let mut wake_condition_key = keys::wake::WorkflowWakeConditionKey::new(
				workflow_name,
				workflow_id,
				keys::wake::WakeCondition::Signal { signal_id },
			);
			wake_condition_key.ts = pending_signal_key.ts;

			// Add wake condition for workflow
			tx.set(
				&self.subspace.pack(&wake_condition_key),
				&wake_condition_key.serialize(())?,
			);
		}

		update_metric(
			&tx.with_subspace(self.subspace.clone()),
			None,
			Some(keys::metric::Metric::SignalPending2(
				signal_name.to_string(),
			)),
		);
		update_wf_metric(
			&tx.with_subspace(self.subspace.clone()),
			workflow_id,
			None,
			Some(keys::workflow::Metric::SignalPending(
				signal_name.to_string(),
			)),
		);

		Ok(())
	}

	async fn dispatch_workflow_inner(
		&self,
		ray_id: Id,
		workflow_id: Id,
		workflow_name: &str,
		tags: Option<&serde_json::Value>,
		input: &serde_json::value::RawValue,
		unique: bool,
		tx: &universaldb::Transaction,
	) -> Result<Id> {
		let tx = tx.with_subspace(self.subspace.clone());

		if unique {
			let empty_tags = json!({});

			if let Some(existing_workflow_id) = self
				.find_workflow_inner(workflow_name, tags.unwrap_or(&empty_tags), &tx)
				.await?
			{
				tracing::debug!(?existing_workflow_id, "found existing workflow");
				return Ok(existing_workflow_id);
			}
		}

		tx.write(
			&keys::workflow::CreateTsKey::new(workflow_id),
			rivet_util::timestamp::now(),
		)?;

		tx.write(
			&keys::workflow::NameKey::new(workflow_id),
			workflow_name.to_string(),
		)?;

		tx.write(&keys::workflow::RayIdKey::new(workflow_id), ray_id)?;

		// Write tags
		let tags = tags
			.map(|x| {
				x.as_object()
					.ok_or_else(|| WorkflowError::InvalidTags("must be an object".to_string()))
			})
			.transpose()?
			.into_iter()
			.flatten()
			.map(|(k, v)| Ok((k.clone(), value_to_str(v)?)))
			.collect::<WorkflowResult<Vec<_>>>()?;
		let mut created_tags = Vec::new();
		for (_, tag) in &tags {
			if !created_tags.contains(tag) {
				created_tags.push(tag.clone());
			}
		}

		for (k, v) in &tags {
			// Write tag key
			tx.write(
				&keys::workflow::TagKey::new(workflow_id, k.clone(), v.clone()),
				(),
			)?;

			// Write "by name and first tag" secondary index
			let rest_of_tags = tags
				.iter()
				.filter(|(k2, _)| k2 != k)
				.map(|(k, v)| (k.clone(), v.clone()))
				.collect();

			tx.write(
				&keys::workflow::ByNameAndTagKey::new(
					workflow_name.to_string(),
					k.clone(),
					v.clone(),
					workflow_id,
				),
				rest_of_tags,
			)?;
		}

		// Write null key for the "by name and first tag" secondary index (all workflows have this)
		tx.write(
			&keys::workflow::ByNameAndTagKey::null(workflow_name.to_string(), workflow_id),
			tags,
		)?;

		// Write input
		let input_key = keys::workflow::InputKey::new(workflow_id);

		for (i, chunk) in input_key.split_ref(input)?.into_iter().enumerate() {
			let chunk_key = input_key.chunk(i);

			tx.set(&self.subspace.pack(&chunk_key), &chunk);
		}

		// Write immediate wake condition
		tx.write(
			&keys::wake::WorkflowWakeConditionKey::new(
				workflow_name.to_string(),
				workflow_id,
				keys::wake::WakeCondition::Immediate,
			),
			(),
		)?;

		tx.write(&keys::workflow::HasWakeConditionKey::new(workflow_id), ())?;

		// Write metric
		update_metric(
			&tx,
			None,
			Some(keys::metric::Metric::WorkflowSleeping(
				workflow_name.to_string(),
			)),
		);

		Ok(workflow_id)
	}

	async fn find_workflow_inner(
		&self,
		workflow_name: &str,
		tags: &serde_json::Value,
		tx: &universaldb::Transaction,
	) -> Result<Option<Id>> {
		// Convert to flat vec of strings
		let mut tag_iter = tags
			.as_object()
			.ok_or_else(|| WorkflowError::InvalidTags("must be an object".to_string()))?
			.iter()
			.map(|(k, v)| Result::<_>::Ok((k.clone(), value_to_str(v)?)));
		let first_tag = tag_iter.next().transpose()?;
		let rest_of_tags = tag_iter.collect::<Result<Vec<_>, _>>()?;

		let workflow_by_name_and_tag_subspace =
			if let Some((first_tag_key, first_tag_value)) = first_tag {
				self.subspace
					.subspace(&keys::workflow::ByNameAndTagKey::subspace(
						workflow_name.to_string(),
						first_tag_key,
						first_tag_value,
					))
			} else {
				// No tags provided, use null subspace. Every workflow has a null key for its tags
				// under the `ByNameAndTagKey` subspace
				self.subspace
					.subspace(&keys::workflow::ByNameAndTagKey::null_subspace(
						workflow_name.to_string(),
					))
			};

		let mut stream = tx.get_ranges_keyvalues(
			universaldb::RangeOption {
				mode: StreamingMode::Iterator,
				..(&workflow_by_name_and_tag_subspace).into()
			},
			Serializable,
		);

		loop {
			let Some(entry) = stream.try_next().await? else {
				return Ok(None);
			};

			// Unpack key
			let workflow_by_name_and_tag_key = self
				.subspace
				.unpack::<keys::workflow::ByNameAndTagKey>(&entry.key())?;

			// Deserialize value
			let wf_rest_of_tags = workflow_by_name_and_tag_key.deserialize(entry.value())?;

			// Compute intersection between wf tags and input
			let tags_match = rest_of_tags.iter().all(|(k, v)| {
				wf_rest_of_tags
					.iter()
					.any(|(wf_k, wf_v)| k == wf_k && v == wf_v)
			});

			// Return first signal that matches the tags
			if tags_match {
				break Ok(Some(workflow_by_name_and_tag_key.workflow_id));
			}
		}
	}
}

#[async_trait::async_trait]
impl Database for DatabaseKv {
	fn worker_poll_interval(&self) -> Duration {
		Duration::from_secs(16)
	}

	fn signal_poll_interval(&self) -> Duration {
		Duration::from_millis(1500)
	}

	async fn new(
		config: rivet_config::Config,
		pools: rivet_pools::Pools,
	) -> anyhow::Result<Arc<Self>> {
		metrics::DB_INSTANCE.inc();
		Ok(Arc::new(DatabaseKv {
			config,
			pools,
			subspace: universaldb::utils::Subspace::new(&(RIVET, GASOLINE, KV)),
			system: system::SystemInfo::get(),
		}))
	}

	#[tracing::instrument(skip_all)]
	async fn bump_sub<'a, 'b>(
		&'a self,
		subject: BumpSubSubject,
	) -> WorkflowResult<BoxStream<'b, ()>> {
		let mut subscriber = self
			.pools
			.ups()
			.map_err(WorkflowError::PoolsGeneric)?
			.subscribe(subject)
			.await
			.context("failed to subscribe to bump sub")
			.map_err(|x| WorkflowError::CreateSubscription(x.into()))?;

		let stream = async_stream::stream! {
			loop {
				use universalpubsub::NextOutput;
				match subscriber.next().await {
					Ok(NextOutput::Message(_)) => yield (),
					Ok(NextOutput::Unsubscribed) => break,
					Ok(NextOutput::NoResponders) => break,
					Err(err) => {
						tracing::warn!(?err, "error in worker wake stream");
						break;
					}
				}
			}
		};

		Ok(stream.boxed())
	}

	#[tracing::instrument(skip_all)]
	async fn clear_expired_leases(&self, _worker_id: Id) -> WorkflowResult<()> {
		let (lost_worker_ids, expired_workflow_count) = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_clear_expired_leases", |tx| {
				async move {
					let start = Instant::now();
					let now = rivet_util::timestamp::now();

					let mut last_ping_cache = HashMap::<Id, i64>::new();
					let mut lost_worker_ids = HashSet::new();
					let mut expired_workflow_count = 0;

					let lease_subspace = self
						.subspace
						.subspace(&keys::workflow::LeaseKey::subspace());

					// List all active leases
					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(&lease_subspace).into()
						},
						// Not Serializable because we don't want this to conflict with other queries which write
						// leases
						Snapshot,
					);

					loop {
						if start.elapsed() > EARLY_TXN_TIMEOUT {
							tracing::warn!("timed out processing expired leases");
							break;
						}

						let Some(lease_key_entry) = stream.try_next().await? else {
							break;
						};

						let lease_key = self
							.subspace
							.unpack::<keys::workflow::LeaseKey>(lease_key_entry.key())?;
						let (workflow_name, worker_id) =
							lease_key.deserialize(lease_key_entry.value())?;
						let last_ping_ts_key = keys::worker::LastPingTsKey::new(worker_id);

						// Get last ping of worker for this lease
						let last_ping_ts = if let Some(last_ping_ts) =
							last_ping_cache.get(&worker_id)
						{
							*last_ping_ts
						} else if let Some(last_ping_entry) = tx
							.get(
								&self.subspace.pack(&last_ping_ts_key),
								// Not Serializable because we don't want this to conflict
								Snapshot,
							)
							.await?
						{
							// Deserialize last ping value
							let last_ping_ts = last_ping_ts_key.deserialize(&last_ping_entry)?;

							// Update cache
							last_ping_cache.insert(worker_id, last_ping_ts);

							last_ping_ts
						} else {
							// Update cache
							last_ping_cache.insert(worker_id, 0);

							0
						};

						// Worker has not pinged within the threshold, meaning the lease is expired
						if last_ping_ts < now - WORKER_LOST_THRESHOLD_MS {
							// Check if the workflow is silenced and ignore
							let silence_ts_key =
								keys::workflow::SilenceTsKey::new(lease_key.workflow_id);
							if tx
								.get(&self.subspace.pack(&silence_ts_key), Serializable)
								.await?
								.is_some()
							{
								continue;
							}

							// NOTE: We add a read conflict here so this query conflicts with any other
							// `clear_expired_leases` queries running at the same time (will conflict with the
							// following `tx.clear`).
							tx.add_conflict_range(
								lease_key_entry.key(),
								&end_of_key_range(lease_key_entry.key()),
								ConflictRangeType::Read,
							)?;

							// Clear lease
							tx.clear(lease_key_entry.key());
							let worker_id_key =
								keys::workflow::WorkerIdKey::new(lease_key.workflow_id);
							tx.clear(&self.subspace.pack(&worker_id_key));

							// Add immediate wake for workflow
							let wake_condition_key = keys::wake::WorkflowWakeConditionKey::new(
								workflow_name.to_string(),
								lease_key.workflow_id,
								keys::wake::WakeCondition::Immediate,
							);
							tx.set(
								&self.subspace.pack(&wake_condition_key),
								&wake_condition_key.serialize(())?,
							);

							update_metric(
								&tx.with_subspace(self.subspace.clone()),
								Some(keys::metric::Metric::WorkflowActive(
									workflow_name.to_string(),
								)),
								Some(keys::metric::Metric::WorkflowSleeping(
									workflow_name.to_string(),
								)),
							);

							expired_workflow_count += 1;
							lost_worker_ids.insert(worker_id);

							tracing::debug!(?lease_key.workflow_id, "failed over wf");
						}
					}

					Ok((lost_worker_ids, expired_workflow_count))
				}
			})
			.custom_instrument(tracing::info_span!("clear_expired_leases_tx"))
			.await
			.context("failed to clear expired leases")
			.map_err(WorkflowError::Udb)?;

		if expired_workflow_count != 0 {
			tracing::info!(
				worker_ids=?lost_worker_ids,
				total_workflows=%expired_workflow_count,
				"handled failover",
			);

			self.bump(BumpSubSubject::Worker);
		}

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn publish_metrics(&self, worker_id: Id) -> WorkflowResult<()> {
		// Attempt to be the only worker publishing metrics by writing to the lock key
		let acquired_lock = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_acquire_metrics_lock", |tx| {
				async move {
					tx.tag(&format!("acquire_metrics_lock:{worker_id}"))?;

					let tx = tx.with_subspace(self.subspace.clone());

					// Read existing lock
					let lock_expired = if let Some(lock_ts) = tx
						.read_opt(&keys::worker::MetricsLockKey::new(), Serializable)
						.await?
					{
						lock_ts < rivet_util::timestamp::now() - METRICS_LOCK_TIMEOUT_MS
					} else {
						true
					};

					if lock_expired {
						// Write to lock key. UDB transactions guarantee that if multiple workers are running this
						// query at the same time only one will succeed which means only one will have the lock.
						tx.write(
							&keys::worker::MetricsLockKey::new(),
							rivet_util::timestamp::now(),
						)?;
					}

					Ok(lock_expired)
				}
			})
			.custom_instrument(tracing::info_span!("acquire_lock_tx"))
			.await
			.context("failed to acquire metrics lock")
			.map_err(WorkflowError::Udb)?;

		if acquired_lock {
			metrics::WORKER_LAST_METRICS_PUBLISH.set(rivet_util::timestamp::now());

			let entries = self
				.pools
				.udb()
				.map_err(WorkflowError::PoolsGeneric)?
				.txn("gas_read_metrics", |tx| async move {
					tx.tag(&format!("publish_metrics:{worker_id}"))?;

					let tx = tx.with_subspace(self.subspace.clone());

					let metrics_subspace =
						self.subspace.subspace(&keys::metric::MetricKey::subspace());
					tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(&metrics_subspace).into()
						},
						Serializable,
					)
					.map(|res| match res {
						Ok(entry) => tx.read_entry::<keys::metric::MetricKey>(&entry),
						Err(err) => Err(err.into()),
					})
					.try_collect::<Vec<_>>()
					.await
				})
				.custom_instrument(tracing::info_span!("read_metrics_tx"))
				.await
				.context("failed to read metrics")
				.map_err(WorkflowError::Udb)?;

			let mut total_workflow_counts: Vec<(String, i64)> = Vec::new();

			for (key, count) in entries {
				match key.metric {
					keys::metric::Metric::WorkflowActive(workflow_name) => {
						metrics::WORKFLOW_ACTIVE
							.with_label_values(&[workflow_name.as_str()])
							.set(count);

						if let Some(entry) = total_workflow_counts
							.iter_mut()
							.find(|(name, _)| name == &workflow_name)
						{
							entry.1 += count;
						} else {
							total_workflow_counts.push((workflow_name, count));
						}
					}
					keys::metric::Metric::WorkflowSleeping(workflow_name) => {
						metrics::WORKFLOW_SLEEPING
							.with_label_values(&[workflow_name.as_str()])
							.set(count);

						if let Some(entry) = total_workflow_counts
							.iter_mut()
							.find(|(name, _)| name == &workflow_name)
						{
							entry.1 += count;
						} else {
							total_workflow_counts.push((workflow_name, count));
						}
					}
					keys::metric::Metric::WorkflowDead(workflow_name, error) => {
						metrics::WORKFLOW_DEAD
							.with_label_values(&[workflow_name.as_str(), error.as_str()])
							.set(count);

						if let Some(entry) = total_workflow_counts
							.iter_mut()
							.find(|(name, _)| name == &workflow_name)
						{
							entry.1 += count;
						} else {
							total_workflow_counts.push((workflow_name, count));
						}
					}
					keys::metric::Metric::WorkflowComplete(workflow_name) => {
						if let Some(entry) = total_workflow_counts
							.iter_mut()
							.find(|(name, _)| name == &workflow_name)
						{
							entry.1 += count;
						} else {
							total_workflow_counts.push((workflow_name, count));
						}
					}
					keys::metric::Metric::SignalPending(_) => {}
					keys::metric::Metric::SignalPending2(signal_name) => {
						metrics::SIGNAL_PENDING
							.with_label_values(&[signal_name.as_str()])
							.set(count);
					}
				}
			}

			for (workflow_name, count) in total_workflow_counts {
				metrics::WORKFLOW_TOTAL
					.with_label_values(&[workflow_name.as_str()])
					.set(count);
			}

			// Clear lock
			self.pools
				.udb()
				.map_err(WorkflowError::PoolsGeneric)?
				.txn("gas_clear_metrics_lock", |tx| async move {
					let metrics_lock_key = keys::worker::MetricsLockKey::new();
					tx.clear(&self.subspace.pack(&metrics_lock_key));

					Ok(())
				})
				.custom_instrument(tracing::info_span!("clear_lock_tx"))
				.await
				.context("failed to release metrics lock")
				.map_err(WorkflowError::Udb)?;
		}

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn update_worker_ping(
		&self,
		worker_id: Id,
		worker_version: i64,
		update_active_idx: bool,
	) -> WorkflowResult<()> {
		// TODO: Temporarily don't record worker id to reduce metrics cardinality
		// let worker_id_str = worker_id.to_string();
		let worker_id_str = "worker".to_string();

		metrics::WORKER_LAST_PING
			.with_label_values(&[worker_id_str.as_str()])
			.set(rivet_util::timestamp::now());

		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_update_worker_ping", |tx| async move {
				let tx = tx.with_subspace(self.subspace.clone());

				let ping_ts = rivet_util::timestamp::now();
				let last_ping_ts_key = keys::worker::LastPingTsKey::new(worker_id);
				let last_active_ping_ts_key = keys::worker::LastActivePingTsKey::new(worker_id);
				let version_key = keys::worker::VersionKey::new(worker_id);

				if update_active_idx {
					// Delete old entry
					if let Some(last_active_ping_ts) =
						tx.read_opt(&last_active_ping_ts_key, Serializable).await?
					{
						let old_active_worker_idx_key = keys::worker::ActiveWorkerIdxKey::new(
							last_active_ping_ts,
							worker_version,
							worker_id,
						);
						tx.delete(&old_active_worker_idx_key);
					}

					// Write new entry
					let active_worker_idx_key =
						keys::worker::ActiveWorkerIdxKey::new(ping_ts, worker_version, worker_id);
					tx.write(&active_worker_idx_key, ())?;

					tx.write(&last_active_ping_ts_key, ping_ts)?;
				}

				tx.write(&last_ping_ts_key, ping_ts)?;
				tx.write(&version_key, worker_version)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("update_worker_ping_tx"))
			.await
			.context("failed to update worker ping")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn mark_worker_inactive(&self, worker_id: Id) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_mark_worker_inactive", |tx| async move {
				let last_active_ping_ts_key = keys::worker::LastActivePingTsKey::new(worker_id);
				let version_key = keys::worker::VersionKey::new(worker_id);

				if let (Some(last_active_ping_ts), Some(version)) = tokio::try_join!(
					tx.read_opt(&last_active_ping_ts_key, Serializable),
					tx.read_opt(&version_key, Serializable),
				)? {
					let active_worker_idx_key = keys::worker::ActiveWorkerIdxKey::new(
						last_active_ping_ts,
						version,
						worker_id,
					);
					tx.delete(&active_worker_idx_key);
				}

				Ok(())
			})
			.custom_instrument(tracing::info_span!("mark_worker_inactive_tx"))
			.await
			.context("failed to mark worker inactive")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(%workflow_id, %workflow_name, unique))]
	async fn dispatch_workflow(
		&self,
		ray_id: Id,
		workflow_id: Id,
		workflow_name: &str,
		tags: Option<&serde_json::Value>,
		input: &serde_json::value::RawValue,
		unique: bool,
	) -> WorkflowResult<Id> {
		let workflow_id = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_dispatch_workflow", |tx| async move {
				tx.tag(&format!("dispatch_workflow:{workflow_name}"))?;

				self.dispatch_workflow_inner(
					ray_id,
					workflow_id,
					workflow_name,
					tags,
					input,
					unique,
					&tx,
				)
				.await
			})
			.custom_instrument(tracing::info_span!("dispatch_workflow_tx"))
			.await
			.context("failed to dispatch workflow")
			.map_err(WorkflowError::Udb)?;

		self.bump(BumpSubSubject::Worker);

		Ok(workflow_id)
	}

	#[tracing::instrument(skip_all, fields(?workflow_ids))]
	async fn get_workflows(&self, workflow_ids: Vec<Id>) -> WorkflowResult<Vec<WorkflowData>> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_get_workflows", |tx| {
				let workflow_ids = workflow_ids.clone();
				async move {
					tx.tag("get_workflows")?;

					futures_util::stream::iter(workflow_ids)
						.map(|workflow_id| {
							let tx = tx.clone();
							async move {
								let name_key = keys::workflow::NameKey::new(workflow_id);
								let input_key = keys::workflow::InputKey::new(workflow_id);
								let input_subspace = self.subspace.subspace(&input_key);
								let state_key = keys::workflow::StateKey::new(workflow_id);
								let state_subspace = self.subspace.subspace(&state_key);
								let output_key = keys::workflow::OutputKey::new(workflow_id);
								let output_subspace = self.subspace.subspace(&output_key);
								let has_wake_condition_key =
									keys::workflow::HasWakeConditionKey::new(workflow_id);

								// Read input and output
								let (
									name_entry,
									input_chunks,
									state_chunks,
									output_chunks,
									has_wake_condition_entry,
								) = tokio::try_join!(
									tx.get(&self.subspace.pack(&name_key), Serializable),
									tx.get_ranges_keyvalues(
										universaldb::RangeOption {
											mode: StreamingMode::WantAll,
											..(&input_subspace).into()
										},
										Serializable,
									)
									.try_collect::<Vec<_>>(),
									tx.get_ranges_keyvalues(
										universaldb::RangeOption {
											mode: StreamingMode::WantAll,
											..(&state_subspace).into()
										},
										Serializable,
									)
									.try_collect::<Vec<_>>(),
									tx.get_ranges_keyvalues(
										universaldb::RangeOption {
											mode: StreamingMode::WantAll,
											..(&output_subspace).into()
										},
										Serializable,
									)
									.try_collect::<Vec<_>>(),
									tx.get(
										&self.subspace.pack(&has_wake_condition_key),
										Serializable
									),
								)?;

								if input_chunks.is_empty() {
									Ok(None)
								} else {
									let input = input_key.combine(input_chunks)?;

									let state = if state_chunks.is_empty() {
										serde_json::value::RawValue::NULL.to_owned()
									} else {
										state_key.combine(state_chunks)?
									};

									let output = if output_chunks.is_empty() {
										None
									} else {
										Some(output_key.combine(output_chunks)?)
									};

									Ok(Some(WorkflowData {
										workflow_id,
										name: name_key.deserialize(
											&name_entry.context("name key should exist")?,
										)?,
										input,
										state,
										output,
										has_wake_condition: has_wake_condition_entry.is_some(),
									}))
								}
							}
						})
						.buffered(256)
						.try_filter_map(|x| std::future::ready(Ok(x)))
						.try_collect::<Vec<_>>()
						.instrument(tracing::trace_span!("get_workflows"))
						.await
				}
			})
			.custom_instrument(tracing::info_span!("get_workflow_tx"))
			.await
			.context("failed to get workflows")
			.map_err(WorkflowError::Udb)
	}

	/// Returns the first incomplete workflow with the given name and tags, first meaning the one with the
	/// lowest id value (by internal representation) because its in a KV store. There is no way to get any other
	/// workflow besides the first.
	#[tracing::instrument(skip_all, fields(%workflow_name))]
	async fn find_workflow(
		&self,
		workflow_name: &str,
		tags: &serde_json::Value,
	) -> WorkflowResult<Option<Id>> {
		let start_instant = Instant::now();

		let workflow_id = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_find_workflow", |tx| async move {
				tx.tag(&format!("find_workflow:{workflow_name}"))?;
				self.find_workflow_inner(workflow_name, tags, &tx).await
			})
			.custom_instrument(tracing::info_span!("find_workflow_tx"))
			.await
			.context("failed to find workflow")
			.map_err(WorkflowError::Udb)?;

		let dt = start_instant.elapsed().as_secs_f64();
		metrics::FIND_WORKFLOWS_DURATION
			.with_label_values(&[workflow_name])
			.observe(dt);

		Ok(workflow_id)
	}

	#[tracing::instrument(skip_all)]
	async fn find_workflows(
		&self,
		queries: &[(&str, serde_json::Value)],
	) -> WorkflowResult<Vec<Option<Id>>> {
		let start_instant = Instant::now();

		let workflow_ids = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_find_workflows_batch", |tx| async move {
				tx.tag("find_workflows")?;

				let futures = queries.iter().map(|(workflow_name, tags)| {
					self.find_workflow_inner(workflow_name, tags, &tx)
				});
				try_join_all(futures).await
			})
			.custom_instrument(tracing::info_span!("find_workflows_batch_tx"))
			.await
			.context("failed to find workflows")
			.map_err(WorkflowError::Udb)?;

		let dt = start_instant.elapsed().as_secs_f64();
		metrics::FIND_WORKFLOWS_BATCH_DURATION.observe(dt);

		Ok(workflow_ids)
	}

	#[tracing::instrument(skip_all)]
	async fn pull_workflows(
		&self,
		worker_id: Id,
		worker_version: i64,
		filter: &[&str],
	) -> WorkflowResult<Vec<PulledWorkflowData>> {
		let start_instant = Instant::now();
		let owned_filter = filter
			.into_iter()
			.map(|x| x.to_string())
			.collect::<Vec<_>>();

		let leased_workflows = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_pull_workflows", |tx| {
				let owned_filter = owned_filter.clone();

				async move {
					tx.tag(&format!("pull_workflows:{worker_id}"))?;

					let tx = tx.with_subspace(self.subspace.clone());
					let now = rivet_util::timestamp::now();

					// All wake conditions with a timestamp before this timestamp will be pulled
					let pull_before = now + i64::try_from(self.worker_poll_interval().as_millis())?;
					// Only consider workers that have pinged within 2 ping intervals ago
					let active_workers_after = now - i64::try_from(PING_INTERVAL.as_millis() * 2)?;

					let cpu_usage_ratio = {
						self.system.lock().await.fetch_cpu_usage(
							self.config.runtime.worker_load_shedding_beta(),
							self.config.runtime.worker_cpu_max,
						)
					};
					let load_shed_curve = self.config.runtime.worker_load_shedding_curve();
					let load_shed_ratio_x1000 = calc_pull_ratio(
						(cpu_usage_ratio * 1000.0) as u64,
						load_shed_curve[0].0,
						load_shed_curve[0].1,
						load_shed_curve[1].0,
						load_shed_curve[1].1,
					);

					// Record load shedding ratio metric
					metrics::LOAD_SHEDDING_RATIO.observe(load_shed_ratio_x1000 as f64 / 1000.0);

					let active_worker_subspace_start = tx.pack(
						&keys::worker::ActiveWorkerIdxKey::subspace(active_workers_after),
					);
					let active_worker_subspace_end = self
						.subspace
						.subspace(&keys::worker::ActiveWorkerIdxKey::entire_subspace())
						.range()
						.1;

					// Pull all available wake conditions from all registered wf names
					let (mut active_workers, wake_keys) = tokio::try_join!(
						tx.get_ranges_keyvalues(
							universaldb::RangeOption {
								mode: StreamingMode::WantAll,
								..(active_worker_subspace_start, active_worker_subspace_end).into()
							},
							// This is Snapshot to reduce contention and exact timestamps are not important
							Snapshot,
						)
						.map(|res| tx.unpack::<keys::worker::ActiveWorkerIdxKey>(res?.key()))
						.try_collect::<Vec<_>>(),
						async {
							let start = Instant::now();
							let mut buffer = Vec::new();
							let mut stream = futures_util::stream::iter(owned_filter)
								.map(|wf_name| {
									let wake_subspace_start = end_of_key_range(&tx.pack(
										&keys::wake::WorkflowWakeConditionKey::subspace_without_ts(
											wf_name.clone(),
										),
									));
									let wake_subspace_end =
										tx.pack(&keys::wake::WorkflowWakeConditionKey::subspace(
											wf_name,
											pull_before,
										));

									tx.get_ranges_keyvalues(
										universaldb::RangeOption {
											mode: StreamingMode::WantAll,
											..(wake_subspace_start, wake_subspace_end).into()
										},
										// This is Snapshot to reduce contention with any new wake conditions
										// being inserted. Conflicts are handled by workflow leases.
										Snapshot,
									)
								})
								.flatten()
								.map(|res| {
									tx.unpack::<keys::wake::WorkflowWakeConditionKey>(res?.key())
								});

							loop {
								if start.elapsed() > EARLY_TXN_TIMEOUT {
									tracing::warn!("timed out pulling wake conditions");
									break;
								}

								let Some(wake_key) = stream.try_next().await? else {
									break;
								};

								buffer.push(wake_key);
							}

							anyhow::Ok(buffer)
						}
						.custom_instrument(tracing::debug_span!("read_wake_conditions"))
					)?;

					let highest_worker_version = active_workers
						.iter()
						.reduce(|acc, e| if acc.version > e.version { acc } else { e })
						.map(|e| e.version)
						.unwrap_or_default();

					// Do not pull if this worker is outdated. This ensures old workers do not pick up
					// workflows that may have already been run on a new worker
					if worker_version < highest_worker_version {
						tracing::info!("worker version is outdated, not pulling workflows");
						return Ok(Vec::new());
					}

					// Keep only highest version workers
					active_workers.retain(|w| w.version == highest_worker_version);

					// Sort for consistency across all workers
					active_workers.sort_by_key(|w| w.worker_id);

					// Get a globally unique idx for the current worker relative to all active workers
					let current_worker_idx = if let Some(current_worker_idx) = active_workers
						.iter()
						.enumerate()
						.find_map(|(i, worker)| (worker_id == worker.worker_id).then_some(i))
					{
						current_worker_idx as u64
					} else {
						tracing::error!(
							?worker_id,
							"current worker should exist in active worker idx, defaulting to worker index 0"
						);

						0
					};
					let active_worker_count = active_workers.len().max(1) as u64;

					// Collect name and deadline ts for each wf id
					let mut dedup_workflows = HashMap::<Id, MinimalPulledWorkflow>::new();
					let now = rivet_util::timestamp::now(); // More up to date now than prev var
					for wake_key in &wake_keys {
						let wake_age_ms = now.saturating_sub(wake_key.ts).max(0);

						// Record time difference between when the wake condition was created and when it was
						// pulled (here). We ignore deadline wake conditions because their ts value is not
						// representative of when it was created, but rather when it should wake.
						if !matches!(
							wake_key.condition,
							keys::wake::WakeCondition::Deadline { .. }
						) {
							// TODO: This will record metrics even if the txn fails, which is wrong
							metrics::WORKFLOW_WAKE_DELTA_DURATION
								.with_label_values(&[&wake_key.workflow_name])
								.observe(wake_age_ms as f64 / 1000.0);
						}

						let Some(wf) = dedup_workflows.get_mut(&wake_key.workflow_id) else {
							dedup_workflows.insert(
								wake_key.workflow_id,
								MinimalPulledWorkflow {
									workflow_id: wake_key.workflow_id,
									workflow_name: wake_key.workflow_name.clone(),
									wake_deadline_ts: wake_key.condition.deadline_ts(),
									earliest_wake_condition_ts: wake_key.ts,
								},
							);

							// Hard limit of 10k deduped workflows, this gets further limited to 1000 at
							// `assigned_workflows`
							if dedup_workflows.len() >= 10000 {
								break;
							}

							continue;
						};

						let key_wake_deadline_ts = wake_key.condition.deadline_ts();

						// Update wake condition ts if earlier
						if wake_key.ts < wf.earliest_wake_condition_ts {
							wf.earliest_wake_condition_ts = wake_key.ts;
						}

						// Update wake deadline ts if earlier
						if wf.wake_deadline_ts.is_none()
							|| key_wake_deadline_ts < wf.wake_deadline_ts
						{
							wf.wake_deadline_ts = key_wake_deadline_ts;
						}
					}

					// Filter workflows in a way that spreads all current pending workflows across all active
					// workers evenly
					let assigned_workflows = dedup_workflows
						.into_values()
						.filter(|wf| {
							let mut hasher = DefaultHasher::new();

							// Earliest wake condition ts is consistent for hashing purposes because when it
							// changes it means a worker has leased it
							wf.earliest_wake_condition_ts.hash(&mut hasher);
							let wf_hash = hasher.finish();

							let pseudorandom_value_x1000 = {
								// Add a little pizazz to the hash so its a different number than wf_hash but
								// still consistent
								1234i32.hash(&mut hasher);
								hasher.finish() % 1000 // 0-1000
							};

							if pseudorandom_value_x1000 > load_shed_ratio_x1000 {
								return false;
							}

							let wf_worker_idx = wf_hash % active_worker_count;

							// Every worker pulls workflows that match the current worker idx as well as the next
							// worker for redundancy. this results in increased txn conflicts but less chance of
							// orphaned workflows
							let next_worker_idx = (current_worker_idx + 1) % active_worker_count;

							wf_worker_idx == current_worker_idx || wf_worker_idx == next_worker_idx
						})
						// Hard limit of 1000 workflows per pull
						.take(1000);

					// Check leases
					let leased_workflows = futures_util::stream::iter(assigned_workflows)
						.map(|wf| {
							let tx = tx.clone();
							async move {
								let lease_key = keys::workflow::LeaseKey::new(wf.workflow_id);

								// Check lease
								if tx.exists(&lease_key, Serializable).await? {
									Result::<_>::Ok(None)
								} else {
									tx.write(&lease_key, (wf.workflow_name.clone(), worker_id))?;

									tx.write(
										&keys::workflow::WorkerIdKey::new(wf.workflow_id),
										worker_id,
									)?;

									update_metric(
										&tx,
										Some(keys::metric::Metric::WorkflowSleeping(
											wf.workflow_name.clone(),
										)),
										Some(keys::metric::Metric::WorkflowActive(
											wf.workflow_name.clone(),
										)),
									);

									Ok(Some(wf))
								}
							}
						})
						// TODO: How to get rid of this buffer?
						.buffer_unordered(1024)
						.try_filter_map(|x| std::future::ready(Ok(x)))
						.try_collect::<Vec<_>>()
						.custom_instrument(tracing::debug_span!("map_to_leased_workflows"))
						.await?;

					// Clear all wake conditions from workflows that we have leased
					for wake_key in &wake_keys {
						if !leased_workflows
							.iter()
							.any(|wf| wf.workflow_id == wake_key.workflow_id)
						{
							continue;
						}

						tx.delete(wake_key);
					}

					// NOTE: We don't read any workflow data in this txn since its only for acquiring leases.
					// The less operations we do in this txn the less contention there is with other workers.
					Ok(leased_workflows)
				}
			})
			.custom_instrument(tracing::info_span!("pull_workflows_tx"))
			.await
			.context("failed to lease workflows")
			.map_err(WorkflowError::Udb)?;

		// TODO: Temporarily don't record worker id to reduce metrics cardinality
		// let worker_id_str = worker_id.to_string();
		let worker_id_str = "worker".to_string();
		let dt = start_instant.elapsed().as_secs_f64();
		metrics::LAST_PULL_WORKFLOWS_DURATION
			.with_label_values(&[worker_id_str.as_str()])
			.set(dt);
		metrics::PULL_WORKFLOWS_DURATION
			.with_label_values(&[worker_id_str.as_str()])
			.observe(dt);

		if leased_workflows.is_empty() {
			return Ok(Vec::new());
		}

		let start_instant2 = Instant::now();

		// Collect metrics on lease counts
		let mut leased_metrics = HashMap::<&str, usize>::new();
		for leased_workflow in &leased_workflows {
			*leased_metrics
				.entry(leased_workflow.workflow_name.as_str())
				.or_default() += 1;
		}

		for (workflow_name, count) in &leased_metrics {
			metrics::WORKFLOW_LEASED
				.with_label_values(&[*workflow_name])
				.observe(*count as f64);
		}

		let leased_workflow_ids = leased_workflows
			.iter()
			.map(|wf| wf.workflow_id)
			.collect::<Vec<_>>();
		let clear_secondary_idx_fut = async move {
			self.pools
				.udb()
				.map_err(WorkflowError::PoolsGeneric)?
				.txn("gas_clear_workflow_secondary_idx", |tx| {
					let leased_workflow_ids = leased_workflow_ids.clone();

					async move {
						// Clear secondary indexes so that we don't get any new wake conditions inserted while
						// the workflow is running
						futures_util::stream::iter(leased_workflow_ids)
							.map(|workflow_id| {
								let tx = tx.clone();
								async move {
									// Clear sub workflow secondary idx
									let wake_sub_workflow_key =
										keys::workflow::WakeSubWorkflowKey::new(workflow_id);
									// NOTE: Snapshot read because we prefer having this txn not conflict rather
									// than unnecessarily insert wake conditions
									if let Some(entry) = tx
										.get(&self.subspace.pack(&wake_sub_workflow_key), Snapshot)
										.await?
									{
										let sub_workflow_id =
											wake_sub_workflow_key.deserialize(&entry)?;

										let sub_workflow_wake_key =
											keys::wake::SubWorkflowWakeKey::new(
												sub_workflow_id,
												workflow_id,
											);

										tx.clear(&self.subspace.pack(&sub_workflow_wake_key));
									}

									// Clear signals secondary index
									let wake_signals_subspace = self.subspace.subspace(
										&keys::workflow::WakeSignalKey::subspace(workflow_id),
									);
									tx.clear_subspace_range(&wake_signals_subspace);

									anyhow::Ok(())
								}
							})
							// TODO: How to get rid of this buffer?
							.buffer_unordered(1024)
							.try_collect::<()>()
							.await
					}
				})
				.custom_instrument(tracing::info_span!("clear_workflow_secondary_idx_tx"))
				.await
				.context("failed to clear workflow secondary indexes")
		};

		let pulled_workflows_fut = async move {
			self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_pull_workflow_history", |tx| {
				let leased_workflows = leased_workflows.clone();

				async move {
					tx.tag(&format!("pull_workflow_history:{worker_id}"))?;

					// Read required wf data for each leased wf
					futures_util::stream::iter(leased_workflows)
						.map(|wf| {
							let tx = tx.clone();
							async move {
								let create_ts_key =
									keys::workflow::CreateTsKey::new(wf.workflow_id);
								let ray_id_key = keys::workflow::RayIdKey::new(wf.workflow_id);
								let input_key = keys::workflow::InputKey::new(wf.workflow_id);
								let state_key = keys::workflow::StateKey::new(wf.workflow_id);
								let output_key = keys::workflow::OutputKey::new(wf.workflow_id);
								let silence_ts_key =
									keys::workflow::SilenceTsKey::new(wf.workflow_id);
								let input_subspace = self.subspace.subspace(&input_key);
								let state_subspace = self.subspace.subspace(&state_key);
								let output_subspace = self.subspace.subspace(&output_key);
								let active_history_subspace = self.subspace.subspace(
									&keys::history::HistorySubspaceKey::new(
										wf.workflow_id,
										keys::history::HistorySubspaceVariant::Active,
									),
								);

								let (
									create_ts_entry,
									ray_id_entry,
									input_chunks,
									state_chunks,
									has_output,
									silence_ts_entry,
									events,
								) = tokio::try_join!(
									tx.get(&self.subspace.pack(&create_ts_key), Serializable),
									tx.get(&self.subspace.pack(&ray_id_key), Serializable),
									tx.get_ranges_keyvalues(
										universaldb::RangeOption {
											mode: StreamingMode::WantAll,
											..(&input_subspace).into()
										},
										Serializable,
									)
									.try_collect::<Vec<_>>(),
									tx.get_ranges_keyvalues(
										universaldb::RangeOption {
											mode: StreamingMode::WantAll,
											..(&state_subspace).into()
										},
										Serializable,
									)
									.try_collect::<Vec<_>>(),
									async {
										tx.get_ranges_keyvalues(
											universaldb::RangeOption {
												mode: StreamingMode::Exact,
												limit: Some(1),
												..(&output_subspace).into()
											},
											Serializable,
										)
										.try_next()
										.await
										.map(|entry| entry.is_some())
									},
									tx.get(&self.subspace.pack(&silence_ts_key), Serializable),
									async {
										let mut events_by_location: HashMap<Location, Vec<Event>> =
											HashMap::new();
										let mut current_event =
											WorkflowHistoryEventBuilder::new(Location::empty());

										let mut stream = tx.get_ranges_keyvalues(
											universaldb::RangeOption {
												mode: StreamingMode::WantAll,
												..(&active_history_subspace).into()
											},
											Serializable,
										);

										loop {
											let Some(entry) = stream.try_next().await? else {
												break;
											};

											// Parse only the wf id and location of the current key
											let partial_key = self
												.subspace
												.unpack::<keys::history::PartialEventKey>(
												entry.key(),
											)?;

											if current_event.location != partial_key.location {
												if current_event.location.is_empty() {
													current_event =
														WorkflowHistoryEventBuilder::new(
															partial_key.location.clone(),
														);
												} else {
													// Insert current event builder to into wf events and
													// reset state
													let previous_event = std::mem::replace(
														&mut current_event,
														WorkflowHistoryEventBuilder::new(
															partial_key.location.clone(),
														),
													);

													let loc = previous_event.location.clone();

													match Event::try_from(previous_event) {
														Ok(event) => {
															events_by_location
																.entry(loc.root())
																.or_default()
																.push(event);
														}
														Err(err) => {
															tracing::error!(workflow_id=?wf.workflow_id, location=?loc, ?err, "failed building workflow history");
															return Ok(None);
														}
													}
												}
											}

											// Parse current key as any event key
											if let Ok(key) =
												self.subspace.unpack::<keys::history::EventTypeKey>(
													entry.key(),
												) {
												let event_type = key.deserialize(entry.value())?;

												current_event.event_type = Some(event_type);
											} else if let Ok(key) =
												self.subspace.unpack::<keys::history::VersionKey>(
													entry.key(),
												) {
												let version = key.deserialize(entry.value())?;

												current_event.version = Some(version);
											} else if let Ok(key) =
												self.subspace.unpack::<keys::history::CreateTsKey>(
													entry.key(),
												) {
												let create_ts = key.deserialize(entry.value())?;

												current_event.create_ts = Some(create_ts);
											} else if let Ok(key) =
												self.subspace
													.unpack::<keys::history::NameKey>(entry.key())
											{
												let name = key.deserialize(entry.value())?;

												current_event.name = Some(name);
											} else if let Ok(key) =
												self.subspace.unpack::<keys::history::SignalIdKey>(
													entry.key(),
												) {
												let signal_id = key.deserialize(entry.value())?;

												current_event.signal_id = Some(signal_id);
											} else if let Ok(key) = self
												.subspace
												.unpack::<keys::history::SubWorkflowIdKey>(
												entry.key(),
											) {
												let sub_workflow_id =
													key.deserialize(entry.value())?;

												current_event.sub_workflow_id =
													Some(sub_workflow_id);
											} else if let Ok(_key) = self
												.subspace
												.unpack::<keys::history::InputChunkKey>(
												entry.key(),
											) {
												current_event.input_chunks.push(entry);
											} else if let Ok(_key) = self
												.subspace
												.unpack::<keys::history::OutputChunkKey>(
												entry.key(),
											) {
												current_event.output_chunks.push(entry);
											} else if let Ok(_key) =
												self.subspace
													.unpack::<keys::history::ErrorKey>(entry.key())
											{
												current_event.error_count += 1;
											} else if let Ok(key) =
												self.subspace.unpack::<keys::history::IterationKey>(
													entry.key(),
												) {
												let iteration = key.deserialize(entry.value())?;

												current_event.iteration = Some(iteration);
											} else if let Ok(key) = self
												.subspace
												.unpack::<keys::history::DeadlineTsKey>(
												entry.key(),
											) {
												let deadline_ts = key.deserialize(entry.value())?;

												current_event.deadline_ts = Some(deadline_ts);
											} else if let Ok(key) = self
												.subspace
												.unpack::<keys::history::SleepStateKey>(
												entry.key(),
											) {
												let sleep_state = key.deserialize(entry.value())?;

												current_event.sleep_state = Some(sleep_state);
											} else if let Ok(key) = self
												.subspace
												.unpack::<keys::history::InnerEventTypeKey>(
												entry.key(),
											) {
												let inner_event_type =
													key.deserialize(entry.value())?;

												current_event.inner_event_type =
													Some(inner_event_type);
											} else if let Ok(key) = self
												.subspace
												.unpack::<keys::history::InnerVersionKey>(
												entry.key(),
											) {
												let inner_version =
													key.deserialize(entry.value())?;

												current_event.inner_version = Some(inner_version);
											} else if let Ok(key) = self
												.subspace
												.unpack::<keys::history::IndexedNameKey>(
												entry.key(),
											) {
												if current_event.indexed_names.len() != key.index {
													tracing::error!(
														?wf,
														location=?partial_key.location,
														expected=%current_event.indexed_names.len(),
														got=%key.index,
														"corrupt history, indexed name doesn't exist yet or is out of order"
													);
													return Ok(None);
												}

												let name = key.deserialize(entry.value())?;
												current_event.indexed_names.push(name);
											} else if let Ok(key) =
												self.subspace
													.unpack::<keys::history::IndexedInputChunkKey>(
														entry.key(),
													) {
												if let Some(input_chunks) = current_event
													.indexed_input_chunks
													.get_mut(key.index)
												{
													input_chunks.push(entry);
												} else if current_event.indexed_input_chunks.len()
													== key.index
												{
													current_event
														.indexed_input_chunks
														.push(vec![entry]);
												} else {
													tracing::error!(
														?wf,
														location=?partial_key.location,
														expected=%current_event.indexed_input_chunks.len(),
														got=%key.index,
														"corrupt history, indexed chunk doesn't exist yet or is out of order"
													);
													return Ok(None);
												}
											}

											// We ignore keys we don't need (like tags)
										}
										// Insert final event
										if !current_event.location.is_empty() {
											let loc = current_event.location.clone();

											match Event::try_from(current_event) {
												Ok(event) => {
													events_by_location
														.entry(loc.root())
														.or_default()
														.push(event);
												}
												Err(err) => {
													tracing::error!(workflow_id=?wf.workflow_id, location=?loc, ?err, "failed building workflow history");
													return Ok(None);
												}
											}
										}

										Ok(Some(events_by_location))
									}
								)?;
								let is_silenced = silence_ts_entry.is_some();

								if has_output {
									tracing::warn!(workflow_id=?wf.workflow_id, "workflow already completed, ignoring");
								} else if is_silenced {
									tracing::warn!(workflow_id=?wf.workflow_id, "workflow silenced, ignoring");
								}

								if has_output || is_silenced {
									// Clear lease
									let lease_key = keys::workflow::LeaseKey::new(wf.workflow_id);
									tx.clear(&self.subspace.pack(&lease_key));
									let worker_id_key =
										keys::workflow::WorkerIdKey::new(wf.workflow_id);
									tx.clear(&self.subspace.pack(&worker_id_key));

									return Ok(None);
								}

								let Some(events) = events else {
									return Ok(None);
								};

								let (Some(create_ts_entry), Some(ray_id_entry)) = (create_ts_entry, ray_id_entry) else {
									tracing::error!(workflow_id=?wf.workflow_id, "create_ts and ray_id keys should exist");
									return Ok(None);
								};

								let create_ts = create_ts_key
									.deserialize(&create_ts_entry)?;
								let ray_id = ray_id_key
									.deserialize(&ray_id_entry)?;
								let input = input_key.combine(input_chunks)?;
								let state = if state_chunks.is_empty() {
									serde_json::value::RawValue::NULL.to_owned()
								} else {
									state_key.combine(state_chunks)?
								};

								anyhow::Ok(Some(PulledWorkflowData {
									workflow_id: wf.workflow_id,
									workflow_name: wf.workflow_name,
									create_ts,
									ray_id,
									input,
									state,
									wake_deadline_ts: wf.wake_deadline_ts,
									events,
								}))
							}
						})
						// TODO: How to get rid of this buffer?
						.buffer_unordered(512)
						.try_filter_map(|x| std::future::ready(Ok(x)))
						.try_collect::<Vec<_>>()
						.instrument(tracing::trace_span!("map_to_partial_workflow"))
						.await
				}
			})
			.custom_instrument(tracing::info_span!("pull_workflow_history_tx"))
			.await
			.context("failed to pull workflow history")
		};

		let (_, pulled_workflows) =
			tokio::try_join!(clear_secondary_idx_fut, pulled_workflows_fut,)
				.map_err(WorkflowError::Udb)?;

		let dt2 = start_instant2.elapsed().as_secs_f64();
		let dt = start_instant.elapsed().as_secs_f64();
		metrics::LAST_PULL_WORKFLOWS_FULL_DURATION
			.with_label_values(&[worker_id_str.as_str()])
			.set(dt);
		metrics::PULL_WORKFLOWS_FULL_DURATION
			.with_label_values(&[worker_id_str.as_str()])
			.observe(dt);
		metrics::LAST_PULL_WORKFLOWS_HISTORY_DURATION
			.with_label_values(&[worker_id_str.as_str()])
			.set(dt2);
		metrics::PULL_WORKFLOWS_HISTORY_DURATION
			.with_label_values(&[worker_id_str.as_str()])
			.observe(dt2);

		Ok(pulled_workflows)
	}

	#[tracing::instrument(skip_all)]
	async fn complete_workflow(
		&self,
		workflow_id: Id,
		workflow_name: &str,
		output: &serde_json::value::RawValue,
		prune_variant: PruneVariant,
	) -> WorkflowResult<()> {
		let start_instant = Instant::now();

		let (wrote_to_wake_idx, pending_signal_cleared_count) = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_complete_workflow", |tx| {
				async move {
					let tx = tx.with_subspace(self.subspace.clone());

					let sub_workflow_wake_subspace = self
						.subspace
						.subspace(&keys::wake::SubWorkflowWakeKey::subspace(workflow_id));
					let tags_subspace = self
						.subspace
						.subspace(&keys::workflow::TagKey::subspace(workflow_id));
					let wake_deadline_key = keys::workflow::WakeDeadlineKey::new(workflow_id);

					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(&sub_workflow_wake_subspace).into()
						},
						// NOTE: Must be Serializable to conflict with `get_sub_workflow`
						Serializable,
					);

					let (wrote_to_wake_idx, tag_keys, wake_deadline) = tokio::try_join!(
						// Check for other workflows waiting on this one, wake all
						async {
							let mut wrote_to_wake_idx = false;

							while let Some(entry) = stream.try_next().await? {
								let (sub_workflow_wake_key, workflow_name) =
									tx.read_entry::<keys::wake::SubWorkflowWakeKey>(&entry)?;

								// Add wake condition for workflow
								tx.write(
									&keys::wake::WorkflowWakeConditionKey::new(
										workflow_name,
										sub_workflow_wake_key.workflow_id,
										keys::wake::WakeCondition::SubWorkflow {
											sub_workflow_id: workflow_id,
										},
									),
									(),
								)?;

								// Clear secondary index
								tx.delete(&sub_workflow_wake_key);

								wrote_to_wake_idx = true;
							}

							Ok(wrote_to_wake_idx)
						},
						// Read tags
						tx.get_ranges_keyvalues(
							universaldb::RangeOption {
								mode: StreamingMode::WantAll,
								..(&tags_subspace).into()
							},
							Serializable,
						)
						.map(|res| {
							tx.unpack::<keys::workflow::TagKey>(res?.key())
								.map_err(anyhow::Error::from)
						})
						.try_collect::<Vec<_>>(),
						tx.read_opt(&wake_deadline_key, Serializable),
					)?;

					for key in tag_keys {
						tx.delete(&keys::workflow::ByNameAndTagKey::new(
							workflow_name.to_string(),
							key.k,
							key.v,
							workflow_id,
						));
					}

					// Clear null key
					tx.delete(&keys::workflow::ByNameAndTagKey::null(
						workflow_name.to_string(),
						workflow_id,
					));

					// Get and clear the pending deadline wake condition, if any. This could be put in the
					// `pull_workflows` function (where we clear secondary indexes) but we chose to clear it
					// here and in `commit_workflow` because its not a secondary index so theres no worry of
					// it inserting more wake conditions. This reduces the load on `pull_workflows`. The
					// reason this isn't immediately cleared in `pull_workflows` along with the rest of the
					// wake conditions is because it might be in the future.
					if let Some(deadline_ts) = wake_deadline {
						tx.delete(&keys::wake::WorkflowWakeConditionKey::new(
							workflow_name.to_string(),
							workflow_id,
							keys::wake::WakeCondition::Deadline { deadline_ts },
						));
					}

					// Clear "has wake condition"
					tx.delete(&keys::workflow::HasWakeConditionKey::new(workflow_id));

					// Write output
					let output_key = keys::workflow::OutputKey::new(workflow_id);

					for (i, chunk) in output_key.split_ref(output)?.into_iter().enumerate() {
						let chunk_key = output_key.chunk(i);

						tx.set(&tx.pack(&chunk_key), &chunk);
					}

					// Clear lease
					tx.delete(&keys::workflow::LeaseKey::new(workflow_id));
					tx.delete(&keys::workflow::WorkerIdKey::new(workflow_id));

					// Clear pending signals metric for observability
					let metrics_subspace = self
						.subspace
						.subspace(&keys::workflow::MetricKey::subspace(workflow_id));
					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(&metrics_subspace).into()
						},
						Serializable,
					);

					let mut pending_signal_cleared_count = 0;
					loop {
						let Some(entry) = stream.try_next().await? else {
							break;
						};

						let (key, metric_count) =
							tx.read_entry::<keys::workflow::MetricKey>(&entry)?;

						// Ignore negatives and zero
						if metric_count as isize <= 0 {
							continue;
						}

						match key.metric {
							keys::workflow::Metric::SignalPending(signal_name) => {
								update_metric_by(
									&tx,
									Some(keys::metric::Metric::SignalPending2(signal_name)),
									None,
									metric_count,
								);
								pending_signal_cleared_count += metric_count;
							}
						}
					}

					// Insert into prune idx if applicable
					match prune_variant {
						PruneVariant::All | PruneVariant::History => {
							tx.write(
								&keys::workflow::PruneIdxKey::new(workflow_id, prune_variant),
								(),
							)?;
						}
						PruneVariant::None => {}
					}

					tx.write(
						&keys::workflow::CompleteTsKey::new(workflow_id),
						rivet_util::timestamp::now(),
					)?;

					update_metric(
						&tx,
						Some(keys::metric::Metric::WorkflowActive(
							workflow_name.to_string(),
						)),
						Some(keys::metric::Metric::WorkflowComplete(
							workflow_name.to_string(),
						)),
					);

					Ok((wrote_to_wake_idx, pending_signal_cleared_count))
				}
			})
			.custom_instrument(tracing::info_span!("complete_workflows_tx"))
			.await
			.context("failed to complete workflow")
			.map_err(WorkflowError::Udb)?;

		// Wake worker again in case some other workflow was waiting for this one to complete
		if wrote_to_wake_idx {
			self.bump(BumpSubSubject::WorkflowComplete { workflow_id });
			self.bump(BumpSubSubject::Worker);
		}

		if pending_signal_cleared_count != 0 {
			tracing::debug!(count=%pending_signal_cleared_count, "cleared pending signals after workflow completed");
		}

		let dt = start_instant.elapsed().as_secs_f64();
		metrics::COMPLETE_WORKFLOW_DURATION
			.with_label_values(&[workflow_name])
			.observe(dt);

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow(
		&self,
		workflow_id: Id,
		workflow_name: &str,
		wake_immediate: bool,
		wake_deadline_ts: Option<i64>,
		wake_signals: &[&str],
		wake_sub_workflow_id: Option<Id>,
		error: &str,
	) -> WorkflowResult<()> {
		let start_instant = Instant::now();

		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow", |tx| {
				async move {
					let wake_deadline_key = keys::workflow::WakeDeadlineKey::new(workflow_id);

					let wake_deadline_entry = tx
						.get(&self.subspace.pack(&wake_deadline_key), Serializable)
						.await?;

					// Add immediate wake for workflow
					if wake_immediate {
						let wake_condition_key = keys::wake::WorkflowWakeConditionKey::new(
							workflow_name.to_string(),
							workflow_id,
							keys::wake::WakeCondition::Immediate,
						);
						tx.set(
							&self.subspace.pack(&wake_condition_key),
							&wake_condition_key.serialize(())?,
						);
					}

					// Get and clear the pending deadline wake condition, if any. This could be put in the
					// `pull_workflows` function (where we clear secondary indexes) but we chose to clear it
					// here and in `complete_workflow` because its not a secondary index so theres no worry of
					// it inserting more wake conditions. This reduces the load on `pull_workflows`. The
					// reason this isn't immediately cleared in `pull_workflows` along with the rest of the
					// wake conditions is because it might be in the future.
					if let Some(raw) = wake_deadline_entry {
						let deadline_ts = wake_deadline_key.deserialize(&raw)?;

						let wake_condition_key = keys::wake::WorkflowWakeConditionKey::new(
							workflow_name.to_string(),
							workflow_id,
							keys::wake::WakeCondition::Deadline { deadline_ts },
						);

						tx.clear(&self.subspace.pack(&wake_condition_key));
					}

					// Write deadline wake index
					if let Some(deadline_ts) = wake_deadline_ts {
						let wake_condition_key = keys::wake::WorkflowWakeConditionKey::new(
							workflow_name.to_string(),
							workflow_id,
							keys::wake::WakeCondition::Deadline { deadline_ts },
						);

						// Add wake condition for workflow
						tx.set(
							&self.subspace.pack(&wake_condition_key),
							&wake_condition_key.serialize(())?,
						);

						// Write to wake deadline
						tx.set(
							&self.subspace.pack(&wake_deadline_key),
							&wake_deadline_key.serialize(deadline_ts)?,
						);
					}

					self.write_signal_wake_idxs(workflow_id, wake_signals, &tx)?;

					// Write sub workflow wake index
					if let Some(sub_workflow_id) = wake_sub_workflow_id {
						self.write_sub_workflow_wake_idx(
							workflow_id,
							workflow_name,
							sub_workflow_id,
							&tx,
						)?;
					}

					// Update "has wake condition"
					let has_wake_condition_key =
						keys::workflow::HasWakeConditionKey::new(workflow_id);
					let has_wake_condition = wake_immediate
						|| wake_deadline_ts.is_some()
						|| !wake_signals.is_empty()
						|| wake_sub_workflow_id.is_some();
					if has_wake_condition {
						tx.set(
							&self.subspace.pack(&has_wake_condition_key),
							&has_wake_condition_key.serialize(())?,
						);
					} else {
						tx.clear(&self.subspace.pack(&has_wake_condition_key));
					}

					// Write error
					let error_key = keys::workflow::ErrorKey::new(workflow_id);
					tx.set(
						&self.subspace.pack(&error_key),
						&error_key.serialize(error.to_string())?,
					);

					// Clear lease
					let lease_key = keys::workflow::LeaseKey::new(workflow_id);
					tx.clear(&self.subspace.pack(&lease_key));
					let worker_id_key = keys::workflow::WorkerIdKey::new(workflow_id);
					tx.clear(&self.subspace.pack(&worker_id_key));

					update_metric(
						&tx.with_subspace(self.subspace.clone()),
						Some(keys::metric::Metric::WorkflowActive(
							workflow_name.to_string(),
						)),
						Some(if has_wake_condition {
							keys::metric::Metric::WorkflowSleeping(workflow_name.to_string())
						} else {
							keys::metric::Metric::WorkflowDead(
								workflow_name.to_string(),
								error.to_string(),
							)
						}),
					);

					Ok(())
				}
			})
			.custom_instrument(tracing::info_span!("commit_workflow_tx"))
			.await
			.context("failed to commit workflow")
			.map_err(WorkflowError::Udb)?;

		// Always wake the worker immediately again. This is an IMPORTANT implementation detail to prevent
		// race conditions with workflow sleep. Imagine the scenario:
		//
		// 1. workflow is between user code and commit
		// 2. worker reads wake condition for said workflow but cannot run it because it is already leased
		// 3. workflow commits
		//
		// This will result in the workflow sleeping instead of immediately running again.
		//
		// Adding this bump_workers call ensures that if the workflow has a valid wake condition before commit
		// then it will immediately wake up again.
		//
		// This is simpler than having this commit_workflow fn read wake conditions because:
		// - the wake conditions are not indexed by wf id
		// - would involve informing the worker to restart the workflow in memory instead of the usual
		//   workflow lifecycle
		// - the worker is already designed to pull wake conditions frequently
		self.bump(BumpSubSubject::Worker);

		let dt = start_instant.elapsed().as_secs_f64();
		metrics::COMMIT_WORKFLOW_DURATION
			.with_label_values(&[workflow_name])
			.observe(dt);

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(?workflow_id, %location))]
	async fn pull_next_signals(
		&self,
		workflow_id: Id,
		workflow_name: &str,
		filter: &[&str],
		location: &Location,
		version: usize,
		_loop_location: Option<&Location>,
		mut limit: usize,
		last_attempt: bool,
		related_sleep_location: Option<&Location>,
	) -> WorkflowResult<Vec<SignalData>> {
		let owned_filter = filter
			.into_iter()
			.map(|x| x.to_string())
			.collect::<Vec<_>>();

		// Retry loop for limit
		loop {
			// Fetch signals from UDB
			let res = self
				.pools
				.udb()
				.map_err(WorkflowError::PoolsGeneric)?
				.txn("gas_pull_next_signals", |tx| {
					let owned_filter = owned_filter.clone();

					async move {
						tx.tag(&format!("pull_next_signals:{workflow_name}"))?;

						// Fetch signals from all streams at the same time
						let mut signals = futures_util::stream::iter(owned_filter.clone())
							.map(|signal_name| {
								let pending_signal_subspace = self.subspace.subspace(
									&keys::workflow::PendingSignalKey::subspace(
										workflow_id,
										signal_name.to_string(),
									),
								);

								tx.get_ranges_keyvalues(
									universaldb::RangeOption {
										mode: StreamingMode::Exact,
										// Each individual stream is limited to our max limit, we apply this
										// limit again after they are all aggregated further down
										limit: Some(limit),
										..(&pending_signal_subspace).into()
									},
									// NOTE: This is Serializable because any insert into this subspace
									// should cause a conflict and retry of this txn
									Serializable,
								)
							})
							.flatten()
							.map(|res| {
								let entry = res?;

								anyhow::Ok(
									self.subspace
										.unpack::<keys::workflow::PendingSignalKey>(&entry.key())?,
								)
							})
							.try_collect::<Vec<_>>()
							.instrument(tracing::trace_span!("map_signals"))
							.await?;

						if !signals.is_empty() {
							let now = rivet_util::timestamp::now();

							// Insert history event
							keys::history::insert::signals_event(
								&self.subspace,
								&tx,
								workflow_id,
								&location,
								version,
								now,
							)?;

							// Sort by ts after aggregating but before applying limit again. Signals are already
							// in order by ts in their individual streams so this should be cheap
							signals.sort_by_key(|key| key.ts);

							// Read signal data in parallel
							let signals = futures_util::stream::iter(
								signals.into_iter().take(limit).enumerate(),
							)
							.map(|(index, key)| {
								let tx = tx.clone();
								async move {
									let ack_ts_key = keys::signal::AckTsKey::new(key.signal_id);

									let packed_key = self.subspace.pack(&key);

									// Ack signal
									tx.add_conflict_range(
										&packed_key,
										&end_of_key_range(&packed_key),
										ConflictRangeType::Read,
									)?;
									tx.set(
										&self.subspace.pack(&ack_ts_key),
										&ack_ts_key.serialize(now)?,
									);

									update_metric(
										&tx.with_subspace(self.subspace.clone()),
										Some(keys::metric::Metric::SignalPending2(
											key.signal_name.clone(),
										)),
										None,
									);
									update_wf_metric(
										&tx.with_subspace(self.subspace.clone()),
										workflow_id,
										Some(keys::workflow::Metric::SignalPending(
											key.signal_name.clone(),
										)),
										None,
									);

									// Clear pending signal key
									tx.clear(&packed_key);

									// Read signal body
									let body_key = keys::signal::BodyKey::new(key.signal_id);
									let body_subspace = self.subspace.subspace(&body_key);

									let chunks = tx
										.get_ranges_keyvalues(
											universaldb::RangeOption {
												mode: StreamingMode::WantAll,
												..(&body_subspace).into()
											},
											Serializable,
										)
										.try_collect::<Vec<_>>()
										.await?;

									let body = body_key.combine(chunks)?;

									// Insert each signal body into the signals event
									keys::history::insert::signals_event_signal(
										&self.subspace,
										&tx,
										workflow_id,
										&location,
										index,
										key.signal_id,
										&key.signal_name,
										&body,
									)?;

									anyhow::Ok(SignalData {
										signal_id: key.signal_id,
										signal_name: key.signal_name,
										create_ts: key.ts,
										body,
									})
								}
							})
							// IMPORTANT: The signals need to stay in order
							.buffered(1024)
							.try_collect::<Vec<_>>()
							.await?;

							// Update the related sleep event and mark it as interrupted
							if let Some(related_sleep_location) = related_sleep_location {
								keys::history::insert::update_sleep_event(
									&self.subspace,
									&tx,
									workflow_id,
									related_sleep_location,
									SleepState::Interrupted,
								)?;
							}

							Ok(signals)
						}
						// No signals found
						else {
							// Write signal wake index if no signal was received. Normally this is done in
							// `commit_workflow` but without this code there would be a race condition if the
							// signal is published between after this transaction and before `commit_workflow`.
							// There is a possibility of `commit_workflow` NOT writing a signal secondary index
							// after this in which case there might be an unnecessary wake condition inserted
							// causing the workflow to wake up again, but this is not as big of an issue because
							// workflow wakes should be idempotent if no events happen.
							// It is important that this is only written on the last try to pull workflows
							// (the workflow engine internally retries a few times) because it should only
							// write signal wake indexes before going to sleep (with err `NoSignalFound`) and
							// not during a retry.
							if last_attempt {
								self.write_signal_wake_idxs(
									workflow_id,
									&owned_filter.iter().map(|x| x.as_str()).collect::<Vec<_>>(),
									&tx,
								)?;
							}

							Ok(Vec::new())
						}
					}
				})
				.custom_instrument(tracing::info_span!("pull_next_signals_tx"))
				.await;

			// If the error indicates the payload is too large, shrink the limit procedurally until it succeeds.
			match res {
				Ok(signals) => return Ok(signals),
				Err(err) => {
					if universaldb::utils::error_is_transaction_too_large(&err) {
						limit = (limit / 2).max(1);

						tracing::warn!(
							?limit,
							"failed pulling workflows due to large txn, trying again with lower limit"
						);
					} else {
						return Err(WorkflowError::Udb(err.context("failed to pull signals")));
					}
				}
			}
		}
	}

	#[tracing::instrument(skip_all)]
	async fn get_sub_workflow(
		&self,
		workflow_id: Id,
		workflow_name: &str,
		sub_workflow_id: Id,
	) -> WorkflowResult<Option<WorkflowData>> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_get_sub_workflow", |tx| {
				async move {
					let name_key = keys::workflow::NameKey::new(sub_workflow_id);
					let input_key = keys::workflow::InputKey::new(sub_workflow_id);
					let input_subspace = self.subspace.subspace(&input_key);
					let state_key = keys::workflow::StateKey::new(sub_workflow_id);
					let state_subspace = self.subspace.subspace(&state_key);
					let output_key = keys::workflow::OutputKey::new(sub_workflow_id);
					let output_subspace = self.subspace.subspace(&output_key);
					let has_wake_condition_key =
						keys::workflow::HasWakeConditionKey::new(sub_workflow_id);

					// Read input and output
					let (
						name_entry,
						input_chunks,
						state_chunks,
						output_chunks,
						has_wake_condition_entry,
					) = tokio::try_join!(
						tx.get(&self.subspace.pack(&name_key), Serializable),
						tx.get_ranges_keyvalues(
							universaldb::RangeOption {
								mode: StreamingMode::WantAll,
								..(&input_subspace).into()
							},
							Serializable,
						)
						.try_collect::<Vec<_>>(),
						tx.get_ranges_keyvalues(
							universaldb::RangeOption {
								mode: StreamingMode::WantAll,
								..(&state_subspace).into()
							},
							Serializable,
						)
						.try_collect::<Vec<_>>(),
						tx.get_ranges_keyvalues(
							universaldb::RangeOption {
								mode: StreamingMode::WantAll,
								..(&output_subspace).into()
							},
							Serializable,
						)
						.try_collect::<Vec<_>>(),
						tx.get(&self.subspace.pack(&has_wake_condition_key), Serializable),
					)?;

					if input_chunks.is_empty() {
						Ok(None)
					} else {
						let input = input_key.combine(input_chunks)?;

						let state = if state_chunks.is_empty() {
							serde_json::value::RawValue::NULL.to_owned()
						} else {
							state_key.combine(state_chunks)?
						};

						let output = if output_chunks.is_empty() {
							// Write sub workflow wake index if the sub workflow is not complete yet. Normally
							// this is done in `commit_workflow` but without this code there would be a race
							// condition if the sub workflow completes between after this transaction and
							// before `commit_workflow`. There is a possibility of `commit_workflow` NOT writing a
							// sub workflow secondary index after this in which case there might be an
							// unnecessary wake condition inserted causing the workflow to wake up again, but this
							// is not as big of an issue because workflow wakes should be idempotent if no events
							// happen.
							self.write_sub_workflow_wake_idx(
								workflow_id,
								workflow_name,
								sub_workflow_id,
								&tx,
							)?;

							None
						} else {
							Some(output_key.combine(output_chunks)?)
						};

						Ok(Some(WorkflowData {
							workflow_id: sub_workflow_id,
							name: name_key
								.deserialize(&name_entry.context("name key should exist")?)?,
							input,
							state,
							output,
							has_wake_condition: has_wake_condition_entry.is_some(),
						}))
					}
				}
			})
			.custom_instrument(tracing::info_span!("get_sub_workflow_tx"))
			.await
			.context("failed to get sub workflow")
			.map_err(WorkflowError::Udb)
	}

	#[tracing::instrument(skip_all)]
	async fn publish_signal(
		&self,
		ray_id: Id,
		workflow_id: Id,
		signal_id: Id,
		signal_name: &str,
		body: &serde_json::value::RawValue,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_publish_signal", |tx| async move {
				tx.tag("publish_signal")?;

				self.publish_signal_inner(ray_id, workflow_id, signal_id, signal_name, body, &tx)
					.await
			})
			.custom_instrument(tracing::info_span!("publish_signal_tx"))
			.await
			.context("failed to publish signal")
			.map_err(WorkflowError::Udb)?;

		self.bump(BumpSubSubject::SignalPublish {
			to_workflow_id: workflow_id,
		});
		self.bump(BumpSubSubject::Worker);

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn publish_signal_from_workflow(
		&self,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		ray_id: Id,
		to_workflow_id: Id,
		signal_id: Id,
		signal_name: &str,
		body: &serde_json::value::RawValue,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_publish_signal_from_workflow", |tx| async move {
				tx.tag(&format!("publish_signal:{from_workflow_id}"))?;

				self.publish_signal_inner(
					ray_id,
					to_workflow_id,
					signal_id,
					signal_name,
					body,
					&tx,
				)
				.await?;

				// Insert history event
				keys::history::insert::signal_send_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					&location,
					version,
					rivet_util::timestamp::now(),
					signal_id,
					&signal_name,
					&body,
					to_workflow_id,
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("publish_signal_from_workflow_tx"))
			.await
			.context("failed to publish signal from workflow")
			.map_err(WorkflowError::Udb)?;

		self.bump(BumpSubSubject::SignalPublish { to_workflow_id });
		self.bump(BumpSubSubject::Worker);

		Ok(())
	}

	#[tracing::instrument(skip_all, fields(%sub_workflow_id, %sub_workflow_name, unique))]
	async fn dispatch_sub_workflow(
		&self,
		ray_id: Id,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		sub_workflow_id: Id,
		sub_workflow_name: &str,
		tags: Option<&serde_json::Value>,
		input: &serde_json::value::RawValue,
		_loop_location: Option<&Location>,
		unique: bool,
	) -> WorkflowResult<Id> {
		let sub_workflow_id = self
			.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_dispatch_sub_workflow", |tx| async move {
				tx.tag(&format!("dispatch_workflow:{sub_workflow_name}"))?;

				let sub_workflow_id = self
					.dispatch_workflow_inner(
						ray_id,
						sub_workflow_id,
						sub_workflow_name,
						tags,
						input,
						unique,
						&tx,
					)
					.await?;

				// Insert history event
				keys::history::insert::sub_workflow_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					&location,
					version,
					rivet_util::timestamp::now(),
					sub_workflow_id,
					sub_workflow_name,
					tags,
					input,
				)?;

				Ok(sub_workflow_id)
			})
			.custom_instrument(tracing::info_span!("dispatch_sub_workflow_tx"))
			.await
			.context("failed to dispatch sub workflow")
			.map_err(WorkflowError::Udb)?;

		self.bump(BumpSubSubject::Worker);

		Ok(sub_workflow_id)
	}

	#[tracing::instrument(skip_all)]
	async fn update_workflow_state(
		&self,
		workflow_id: Id,
		state: &serde_json::value::RawValue,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_update_workflow_state", |tx| {
				async move {
					tx.tag(&format!("workflow_state_update:{workflow_id}"))?;

					let state_key = keys::workflow::StateKey::new(workflow_id);
					let state_subspace = self.subspace.subspace(&state_key);

					// Clear old state
					tx.clear_subspace_range(&state_subspace);

					// Write new state
					for (i, chunk) in state_key.split_ref(&state)?.into_iter().enumerate() {
						let chunk_key = state_key.chunk(i);

						tx.set(&self.subspace.pack(&chunk_key), &chunk);
					}

					Ok(())
				}
			})
			.custom_instrument(tracing::info_span!("update_workflow_state_tx"))
			.await
			.context("failed to update workflow state")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow_activity_event(
		&self,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		name: &str,
		create_ts: i64,
		input: &serde_json::value::RawValue,
		res: Result<&serde_json::value::RawValue, &str>,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow_activity_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::activity_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					version,
					create_ts,
					name,
					input,
					res,
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("commit_workflow_activity_event_tx"))
			.await
			.context("failed to commit activity event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow_message_send_event(
		&self,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		tags: &serde_json::Value,
		message_name: &str,
		body: &serde_json::value::RawValue,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow_message_send_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::message_send_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					version,
					rivet_util::timestamp::now(),
					tags,
					message_name,
					body,
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("commit_workflow_message_send_event_tx"))
			.await
			.context("failed to commit message send event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn upsert_workflow_loop_event(
		&self,
		from_workflow_id: Id,
		_workflow_name: &str,
		location: &Location,
		version: usize,
		iteration: usize,
		state: &serde_json::value::RawValue,
		output: Option<&serde_json::value::RawValue>,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_upsert_workflow_loop_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				if iteration == 0 {
					keys::history::insert::loop_event(
						&self.subspace,
						&tx,
						from_workflow_id,
						location,
						version,
						rivet_util::timestamp::now(),
						iteration,
						state,
						output,
					)?;
				} else {
					keys::history::insert::update_loop_event(
						&self.subspace,
						&tx,
						from_workflow_id,
						location,
						iteration,
						state,
						output,
					)?;

					let active_history_subspace =
						self.subspace
							.subspace(&keys::history::HistorySubspaceKey::new(
								from_workflow_id,
								keys::history::HistorySubspaceVariant::Active,
							));

					let forgotten_history_subspace =
						self.subspace
							.subspace(&keys::history::HistorySubspaceKey::new(
								from_workflow_id,
								keys::history::HistorySubspaceVariant::Forgotten,
							));

					// Start is {loop location, 0, ...}
					let loop_events_subspace_start = self
						.subspace
						.subspace(&keys::history::EventHistorySubspaceKey::entire(
							from_workflow_id,
							location.clone(),
							false,
						))
						.range()
						.0;
					// End is {loop location, iteration - 1, ...}
					let loop_events_subspace_end = self
						.subspace
						.subspace(&keys::history::EventHistorySubspaceKey::new(
							from_workflow_id,
							location.clone(),
							iteration.saturating_sub(1),
							false,
						))
						.range()
						.1;

					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(
								loop_events_subspace_start.as_slice(),
								loop_events_subspace_end.as_slice(),
							)
								.into()
						},
						Serializable,
					);

					// Move all events under this loop up to the current iteration to the forgotten history
					loop {
						let Some(entry) = stream.try_next().await? else {
							break;
						};

						if !active_history_subspace.is_start_of(entry.key()) {
							return Err(universaldb::tuple::PackError::BadPrefix.into());
						}

						// Truncate tuple up to ...ACTIVE and replace it with ...FORGOTTEN
						let truncated_key = &entry.key()[active_history_subspace.bytes().len()..];
						let forgotten_key =
							[forgotten_history_subspace.bytes(), truncated_key].concat();

						tx.set(&forgotten_key, entry.value());
					}

					tx.clear_range(&loop_events_subspace_start, &loop_events_subspace_end);

					// Only retain last 100 events in forgotten history
					if iteration > 100 {
						let old_forgotten_subspace_start =
							self.subspace
								.pack(&keys::history::EventHistorySubspaceKey::new(
									from_workflow_id,
									location.clone(),
									0,
									true,
								));
						let old_forgotten_subspace_end =
							self.subspace
								.pack(&keys::history::EventHistorySubspaceKey::new(
									from_workflow_id,
									location.clone(),
									iteration - 100,
									true,
								));

						tx.clear_range(&old_forgotten_subspace_start, &old_forgotten_subspace_end);
					}
				}

				Ok(())
			})
			.custom_instrument(tracing::info_span!("upsert_loop_event_tx"))
			.await
			.context("failed to upsert loop event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow_sleep_event(
		&self,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		deadline_ts: i64,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow_sleep_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::sleep_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					version,
					rivet_util::timestamp::now(),
					deadline_ts,
					SleepState::Normal,
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("commit_workflow_sleep_event_tx"))
			.await
			.context("failed to commit sleep event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn update_workflow_sleep_event_state(
		&self,
		from_workflow_id: Id,
		location: &Location,
		state: SleepState,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_update_workflow_sleep_state", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::update_sleep_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					state,
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("update_workflow_sleep_state_tx"))
			.await
			.context("failed to update sleep state")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow_branch_event(
		&self,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow_branch_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::branch_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					version,
					rivet_util::timestamp::now(),
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("commit_workflow_branch_event_tx"))
			.await
			.context("failed to commit branch event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow_removed_event(
		&self,
		from_workflow_id: Id,
		location: &Location,
		event_type: EventType,
		event_name: Option<&str>,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow_removed_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::removed_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					1, // Default
					rivet_util::timestamp::now(),
					event_type,
					event_name,
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!("commit_workflow_removed_event_tx"))
			.await
			.context("failed to commit removed event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}

	#[tracing::instrument(skip_all)]
	async fn commit_workflow_version_check_event(
		&self,
		from_workflow_id: Id,
		location: &Location,
		version: usize,
		inner_version: usize,
		_loop_location: Option<&Location>,
	) -> WorkflowResult<()> {
		self.pools
			.udb()
			.map_err(WorkflowError::PoolsGeneric)?
			.txn("gas_commit_workflow_version_check_event", |tx| async move {
				tx.tag(&format!("update_workflow_history:{from_workflow_id}"))?;

				keys::history::insert::version_check_event(
					&self.subspace,
					&tx,
					from_workflow_id,
					location,
					version,
					inner_version,
					rivet_util::timestamp::now(),
				)?;

				Ok(())
			})
			.custom_instrument(tracing::info_span!(
				"commit_workflow_version_check_event_tx"
			))
			.await
			.context("failed to commit version check event")
			.map_err(WorkflowError::Udb)?;

		Ok(())
	}
}

impl Drop for DatabaseKv {
	fn drop(&mut self) {
		metrics::DB_INSTANCE.dec();
	}
}

#[derive(Debug, Clone)]
struct MinimalPulledWorkflow {
	workflow_id: Id,
	workflow_name: String,
	wake_deadline_ts: Option<i64>,
	earliest_wake_condition_ts: i64,
}

fn update_metric(
	tx: &universaldb::Transaction,
	previous: Option<keys::metric::Metric>,
	current: Option<keys::metric::Metric>,
) {
	update_metric_by(tx, previous, current, 1)
}

fn update_metric_by(
	tx: &universaldb::Transaction,
	previous: Option<keys::metric::Metric>,
	current: Option<keys::metric::Metric>,
	by: i64,
) {
	if &previous == &current {
		return;
	}

	if let Some(previous) = previous {
		tx.atomic_op(
			&keys::metric::MetricKey::new(previous),
			&(by * -1).to_le_bytes(),
			MutationType::Add,
		);
	}

	if let Some(current) = current {
		tx.atomic_op(
			&keys::metric::MetricKey::new(current),
			&by.to_le_bytes(),
			MutationType::Add,
		);
	}
}

fn update_wf_metric(
	tx: &universaldb::Transaction,
	workflow_id: Id,
	previous: Option<keys::workflow::Metric>,
	current: Option<keys::workflow::Metric>,
) {
	if &previous == &current {
		return;
	}

	if let Some(previous) = previous {
		tx.atomic_op(
			&keys::workflow::MetricKey::new(workflow_id, previous),
			&(-1i64).to_le_bytes(),
			MutationType::Add,
		);
	}

	if let Some(current) = current {
		tx.atomic_op(
			&keys::workflow::MetricKey::new(workflow_id, current),
			&1i64.to_le_bytes(),
			MutationType::Add,
		);
	}
}

struct WorkflowHistoryEventBuilder {
	location: Location,
	event_type: Option<EventType>,
	version: Option<usize>,
	create_ts: Option<i64>,
	name: Option<String>,
	signal_id: Option<Id>,
	sub_workflow_id: Option<Id>,
	input_chunks: Vec<Value>,
	output_chunks: Vec<Value>,
	error_count: usize,
	iteration: Option<usize>,
	deadline_ts: Option<i64>,
	sleep_state: Option<SleepState>,
	inner_event_type: Option<EventType>,
	inner_version: Option<usize>,

	indexed_names: Vec<String>,
	indexed_input_chunks: Vec<Vec<Value>>,
}

impl WorkflowHistoryEventBuilder {
	fn new(location: Location) -> Self {
		WorkflowHistoryEventBuilder {
			location,
			event_type: None,
			version: None,
			create_ts: None,
			name: None,
			signal_id: None,
			sub_workflow_id: None,
			input_chunks: Vec::new(),
			output_chunks: Vec::new(),
			error_count: 0,
			iteration: None,
			deadline_ts: None,
			sleep_state: None,
			inner_event_type: None,
			inner_version: None,

			indexed_names: Vec::new(),
			indexed_input_chunks: Vec::new(),
		}
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for Event {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		let event_type = value
			.event_type
			.ok_or(WorkflowError::MissingEventData("event_type"))?;

		Ok(Event {
			coordinate: value
				.location
				.tail()
				.cloned()
				.ok_or(WorkflowError::MissingEventData("location"))?,
			version: value
				.version
				.ok_or(WorkflowError::MissingEventData("version"))?,
			data: match event_type {
				EventType::Activity => EventData::Activity(value.try_into()?),
				// Deprecated, manually convert to newer type
				EventType::Signal => {
					EventData::Signals(SignalsEvent {
						names: vec![value.name.ok_or(WorkflowError::MissingEventData("name"))?],
						bodies: vec![if value.input_chunks.is_empty() {
							return Err(WorkflowError::MissingEventData("input"));
						} else {
							// workflow_id not needed
							let input_key = keys::history::InputKey::new(Id::nil(), value.location);
							input_key
								.combine(value.input_chunks)
								.map_err(WorkflowError::DeserializeEventData)?
						}],
					})
				}
				EventType::SignalSend => EventData::SignalSend(value.try_into()?),
				EventType::MessageSend => EventData::MessageSend(value.try_into()?),
				EventType::SubWorkflow => EventData::SubWorkflow(value.try_into()?),
				EventType::Loop => EventData::Loop(value.try_into()?),
				EventType::Sleep => EventData::Sleep(value.try_into()?),
				EventType::Branch => EventData::Branch,
				EventType::Removed => EventData::Removed(value.try_into()?),
				EventType::VersionCheck => EventData::VersionCheck(value.try_into()?),
				EventType::Signals => EventData::Signals(value.try_into()?),
			},
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for ActivityEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(ActivityEvent {
			name: value.name.ok_or(WorkflowError::MissingEventData("name"))?,
			create_ts: value
				.create_ts
				.ok_or(WorkflowError::MissingEventData("create_ts"))?,
			output: {
				if value.output_chunks.is_empty() {
					None
				} else {
					// workflow_id not needed
					let output_key = keys::history::OutputKey::new(Id::nil(), value.location);
					Some(
						output_key
							.combine(value.output_chunks)
							.map_err(WorkflowError::DeserializeEventData)?,
					)
				}
			},
			error_count: value.error_count,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for SignalSendEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(SignalSendEvent {
			signal_id: value
				.signal_id
				.ok_or(WorkflowError::MissingEventData("signal_id"))?,
			name: value.name.ok_or(WorkflowError::MissingEventData("name"))?,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for MessageSendEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(MessageSendEvent {
			name: value.name.ok_or(WorkflowError::MissingEventData("name"))?,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for SubWorkflowEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(SubWorkflowEvent {
			sub_workflow_id: value
				.sub_workflow_id
				.ok_or(WorkflowError::MissingEventData("sub_workflow_id"))?,
			name: value.name.ok_or(WorkflowError::MissingEventData("name"))?,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for LoopEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(LoopEvent {
			state: {
				if value.input_chunks.is_empty() {
					return Err(WorkflowError::MissingEventData("input"));
				} else {
					// workflow_id not needed
					let input_key = keys::history::InputKey::new(Id::nil(), value.location.clone());
					input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
			output: {
				if value.output_chunks.is_empty() {
					None
				} else {
					// workflow_id not needed
					let output_key = keys::history::OutputKey::new(Id::nil(), value.location);
					Some(
						output_key
							.combine(value.output_chunks)
							.map_err(WorkflowError::DeserializeEventData)?,
					)
				}
			},
			iteration: value
				.iteration
				.ok_or(WorkflowError::MissingEventData("iteration"))?
				.try_into()
				.map_err(|_| WorkflowError::IntegerConversion)?,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for SleepEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(SleepEvent {
			deadline_ts: value
				.deadline_ts
				.ok_or(WorkflowError::MissingEventData("deadline_ts"))?,
			state: value
				.sleep_state
				.ok_or(WorkflowError::MissingEventData("sleep_state"))?,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for RemovedEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(RemovedEvent {
			name: value.name,
			event_type: value
				.inner_event_type
				.ok_or(WorkflowError::MissingEventData("inner_event_type"))?,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for VersionCheckEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(VersionCheckEvent {
			// Fallback to event version for old events that don't have inner version
			inner_version: value.inner_version.unwrap_or(
				value
					.version
					.ok_or(WorkflowError::MissingEventData("version"))?,
			),
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for SignalsEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(SignalsEvent {
			names: if value.indexed_names.is_empty() {
				return Err(WorkflowError::MissingEventData("name"));
			} else {
				value.indexed_names
			},
			bodies: if value.indexed_input_chunks.is_empty() {
				return Err(WorkflowError::MissingEventData("input"));
			} else {
				value
					.indexed_input_chunks
					.into_iter()
					.map(|input_chunks| {
						// workflow_id not needed
						let input_key =
							keys::history::InputKey::new(Id::nil(), value.location.clone());
						input_key
							.combine(input_chunks)
							.map_err(WorkflowError::DeserializeEventData)
					})
					.collect::<std::result::Result<_, _>>()?
			},
		})
	}
}

fn value_to_str(v: &serde_json::Value) -> WorkflowResult<String> {
	match v {
		serde_json::Value::String(s) => Ok(s.clone()),
		_ => cjson::to_string(&v).map_err(WorkflowError::CjsonSerializeTags),
	}
}

fn calc_pull_ratio(x: u64, ax: u64, ay: u64, bx: u64, by: u64) -> u64 {
	// Must have neg slope, inversely proportional
	assert!(ax < bx);
	assert!(ay > by);

	// Bound domain
	let x = x.max(ax).min(bx);

	let dx = bx - ax;
	let neg_dy = ay - by;
	let b = ay + ax * neg_dy / dx;

	return b.saturating_sub(x * neg_dy / dx);
}
