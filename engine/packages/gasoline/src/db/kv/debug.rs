use std::{
	collections::HashMap,
	ops::Deref,
	result::Result::{Err, Ok},
	time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use futures_util::{StreamExt, TryStreamExt, stream::FuturesUnordered};
use rivet_util::Id;
use serde::Serialize;
use tracing::Instrument;
use universaldb::utils::{FormalChunkedKey, FormalKey, IsolationLevel::*, end_of_key_range};
use universaldb::{
	KeySelector, RangeOption,
	options::{ConflictRangeType, StreamingMode},
	tuple::{PackResult, TupleDepth, TupleUnpack},
	value::Value,
};
use uuid::Uuid;

use super::{DatabaseKv, keys, update_metric};
use crate::{
	db::{
		BumpSubSubject,
		debug::{
			ActivityError, ActivityEvent, DatabaseDebug, Event, EventData, HistoryData, LoopEvent,
			MessageSendEvent, RepairInspection, RepairOutcome, RepairVariant, RepairVerification,
			SignalData, SignalEvent, SignalSendEvent, SignalState, SignalsEvent, SubWorkflowEvent,
			WorkflowData, WorkflowState,
		},
	},
	error::{WorkflowError, WorkflowResult},
	history::{
		event::{EventType, RemovedEvent, SleepEvent, SleepState, VersionCheckEvent},
		location::Location,
	},
	workflow::PruneVariant,
};

const EARLY_TXN_TIMEOUT: Duration = Duration::from_secs(3);

// These tests seed private workflow keys and exercise the real RocksDB-backed debug operation.
#[cfg(test)]
#[path = "../../../tests/modules/dead_workflow_backfill.rs"]
mod dead_workflow_backfill_tests;

impl DatabaseKv {
	#[tracing::instrument(level = "debug", skip_all)]
	async fn get_workflows_inner(
		&self,
		workflow_ids: Vec<Id>,
		tx: &universaldb::RetryableTransaction,
	) -> Result<Vec<WorkflowData>> {
		let mut res = Vec::new();

		// TODO: Parallelize
		for workflow_id in workflow_ids {
			// TODO: Do a single range read for all keys under workflow/data
			let name_key = keys::workflow::NameKey::new(workflow_id);
			let tags_subspace_key = keys::workflow::TagKey::subspace(workflow_id);
			let tags_subspace = self.subspace.subspace(&tags_subspace_key);
			let create_ts_key = keys::workflow::CreateTsKey::new(workflow_id);
			let input_key = keys::workflow::InputKey::new(workflow_id);
			let input_subspace = self.subspace.subspace(&input_key);
			let state_key = keys::workflow::StateKey::new(workflow_id);
			let state_subspace = self.subspace.subspace(&state_key);
			let output_key = keys::workflow::OutputKey::new(workflow_id);
			let output_subspace = self.subspace.subspace(&output_key);
			let error_key = keys::workflow::ErrorKey::new(workflow_id);
			let has_wake_condition_key = keys::workflow::HasWakeConditionKey::new(workflow_id);
			let worker_id_key = keys::workflow::WorkerIdKey::new(workflow_id);
			let silence_ts_key = keys::workflow::SilenceTsKey::new(workflow_id);

			let (
				tags,
				name_entry,
				create_ts_entry,
				input_chunks,
				state_chunks,
				output_chunks,
				error_entry,
				has_wake_condition_entry,
				worker_id_entry,
				silence_ts_entry,
			) = tokio::try_join!(
				tx.get_ranges_keyvalues(
					RangeOption {
						mode: StreamingMode::WantAll,
						..(&tags_subspace).into()
					},
					Snapshot,
				)
				.map(|res| {
					let key = self.subspace.unpack::<keys::workflow::TagKey>(res?.key())?;
					let v = serde_json::Value::String(key.v.clone());

					Ok((key.k, v))
				})
				.try_collect::<serde_json::Map<_, _>>(),
				tx.get(&self.subspace.pack(&name_key), Snapshot),
				tx.get(&self.subspace.pack(&create_ts_key), Snapshot),
				tx.get_ranges_keyvalues(
					RangeOption {
						mode: StreamingMode::WantAll,
						..(&input_subspace).into()
					},
					Snapshot,
				)
				.try_collect::<Vec<_>>(),
				tx.get_ranges_keyvalues(
					RangeOption {
						mode: StreamingMode::WantAll,
						..(&state_subspace).into()
					},
					Snapshot,
				)
				.try_collect::<Vec<_>>(),
				tx.get_ranges_keyvalues(
					RangeOption {
						mode: StreamingMode::WantAll,
						..(&output_subspace).into()
					},
					Snapshot,
				)
				.try_collect::<Vec<_>>(),
				tx.get(&self.subspace.pack(&error_key), Snapshot),
				tx.get(&self.subspace.pack(&has_wake_condition_key), Snapshot),
				tx.get(&self.subspace.pack(&worker_id_key), Snapshot),
				tx.get(&self.subspace.pack(&silence_ts_key), Snapshot),
			)?;

			let Some(create_ts_entry) = &create_ts_entry else {
				tracing::warn!(?workflow_id, "workflow not found");
				continue;
			};

			let create_ts = create_ts_key.deserialize(&create_ts_entry)?;

			let workflow_name = name_key.deserialize(&name_entry.context("key should exist")?)?;

			let input = input_key.combine(input_chunks)?;

			let data = if state_chunks.is_empty() {
				serde_json::value::RawValue::NULL.to_owned()
			} else {
				state_key.combine(state_chunks)?
			};

			let output = if output_chunks.is_empty() {
				None
			} else {
				Some(output_key.combine(output_chunks)?)
			};

			let error = if let Some(error_entry) = error_entry {
				Some(error_key.deserialize(&error_entry)?)
			} else {
				None
			};

			let state = if silence_ts_entry.is_some() {
				WorkflowState::Silenced
			} else if output.is_some() {
				WorkflowState::Complete
			} else if worker_id_entry.is_some() {
				WorkflowState::Running
			} else if has_wake_condition_entry.is_some() {
				WorkflowState::Sleeping
			} else {
				WorkflowState::Dead
			};

			res.push(WorkflowData {
				workflow_id,
				workflow_name,
				tags: serde_json::Value::Object(tags),
				create_ts,
				input: serde_json::from_str(input.get())?,
				data: serde_json::from_str(data.get())?,
				output: output.map(|x| serde_json::from_str(x.get())).transpose()?,
				error,
				state,
			});
		}

		Ok(res)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn get_signals_inner(
		&self,
		signal_ids: Vec<Id>,
		tx: &universaldb::RetryableTransaction,
	) -> Result<Vec<SignalData>> {
		let mut res = Vec::new();

		// TODO: Parallelize
		for signal_id in signal_ids {
			let name_key = keys::signal::NameKey::new(signal_id);
			let create_ts_key = keys::signal::CreateTsKey::new(signal_id);
			let body_key = keys::signal::BodyKey::new(signal_id);
			let body_subspace = self.subspace.subspace(&body_key);
			let ack_ts_key = keys::signal::AckTsKey::new(signal_id);
			let silence_ts_key = keys::signal::SilenceTsKey::new(signal_id);
			let workflow_id_key = keys::signal::WorkflowIdKey::new(signal_id);

			let (
				name_entry,
				workflow_id_entry,
				create_ts_entry,
				body_chunks,
				ack_ts_entry,
				silence_ts_entry,
			) = tokio::try_join!(
				tx.get(&self.subspace.pack(&name_key), Snapshot),
				tx.get(&self.subspace.pack(&workflow_id_key), Snapshot),
				tx.get(&self.subspace.pack(&create_ts_key), Snapshot),
				async {
					tx.get_ranges_keyvalues(
						RangeOption {
							mode: StreamingMode::WantAll,
							..(&body_subspace).into()
						},
						Snapshot,
					)
					.try_collect::<Vec<_>>()
					.await
				},
				tx.get(&self.subspace.pack(&ack_ts_key), Snapshot),
				tx.get(&self.subspace.pack(&silence_ts_key), Snapshot),
			)?;

			let Some(create_ts_entry) = &create_ts_entry else {
				tracing::warn!(?signal_id, "signal not found");
				continue;
			};

			let create_ts = create_ts_key.deserialize(&create_ts_entry)?;

			let signal_name = name_key.deserialize(&name_entry.context("key should exist")?)?;

			let body = body_key.combine(body_chunks)?;

			let workflow_id = if let Some(workflow_id_entry) = workflow_id_entry {
				Some(workflow_id_key.deserialize(&workflow_id_entry)?)
			} else {
				None
			};

			let ack_ts = if let Some(ack_ts_entry) = ack_ts_entry {
				Some(ack_ts_key.deserialize(&ack_ts_entry)?)
			} else {
				None
			};

			let state = if silence_ts_entry.is_some() {
				SignalState::Silenced
			} else if ack_ts.is_some() {
				SignalState::Acked
			} else {
				SignalState::Pending
			};

			res.push(SignalData {
				signal_id,
				signal_name,
				tags: None,
				workflow_id,
				create_ts,
				ack_ts,
				body: serde_json::from_str(body.get())?,
				state,
			});
		}

		Ok(res)
	}
}

// NOTE: Most of the reads here are Snapshot because we don't want this to conflict with the actual wf engine.
// Its just for debugging
#[async_trait::async_trait]
impl DatabaseDebug for DatabaseKv {
	#[tracing::instrument(level = "debug", skip_all)]
	async fn get_workflows(&self, workflow_ids: Vec<Id>) -> Result<Vec<WorkflowData>> {
		self.pools
			.udb()?
			.txn("gas_debug_get_workflows", |tx| {
				let workflow_ids = workflow_ids.clone();
				async move { self.get_workflows_inner(workflow_ids, &tx).await }
			})
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn find_workflows(
		&self,
		tags: &[(String, String)],
		name: Option<&str>,
		state: Option<WorkflowState>,
	) -> Result<Vec<WorkflowData>> {
		// NOTE: this does a full scan of all keys under workflow/data and filters in memory
		self.pools
			.udb()?
			.txn("gas_debug_find_workflows", |tx| {
				let name = name.clone();
				async move {
					let mut workflow_ids = Vec::new();

					let data_subspace = self
						.subspace
						.subspace(&keys::workflow::DataSubspaceKey::new());

					let mut stream = tx.get_ranges_keyvalues(
						RangeOption {
							mode: StreamingMode::Iterator,
							..(&data_subspace).into()
						},
						Snapshot,
					);

					let mut current_workflow_id = None;
					let mut matching_tags = 0;
					let mut name_matches = name.is_none();
					let mut state_matches = state.is_none() || state == Some(WorkflowState::Dead);

					while let Some(entry) = stream.try_next().await? {
						let workflow_id = *self.subspace.unpack::<JustId>(entry.key())?;

						if let Some(curr) = current_workflow_id {
							if workflow_id != curr {
								// Save if matches query
								if matching_tags == tags.len() && name_matches && state_matches {
									workflow_ids.push(curr);

									if workflow_ids.len() >= 100 {
										current_workflow_id = None;
										break;
									}
								}

								// Reset state
								matching_tags = 0;
								name_matches = name.is_none();
								state_matches =
									state.is_none() || state == Some(WorkflowState::Dead);
							}
						}

						current_workflow_id = Some(workflow_id);

						if let Ok(tag_key) =
							self.subspace.unpack::<keys::workflow::TagKey>(entry.key())
						{
							if tags.iter().any(|(k, v)| &tag_key.k == k && &tag_key.v == v) {
								matching_tags += 1;
							}
						} else if let Ok(name_key) =
							self.subspace.unpack::<keys::workflow::NameKey>(entry.key())
						{
							if let Some(name) = &name {
								let workflow_name = name_key.deserialize(entry.value())?;

								name_matches = &workflow_name == name;
							}
						} else if let Ok(_) = self
							.subspace
							.unpack::<keys::workflow::OutputChunkKey>(entry.key())
						{
							// Has output
							match state {
								Some(WorkflowState::Complete) => state_matches = true,
								Some(_) => state_matches = false,
								None => {}
							}
						} else if let Ok(_) = self
							.subspace
							.unpack::<keys::workflow::WorkerIdKey>(entry.key())
						{
							match state {
								Some(WorkflowState::Running) => state_matches = true,
								Some(WorkflowState::Sleeping | WorkflowState::Dead) => {
									state_matches = false
								}
								_ => {}
							}
						} else if let Ok(_) = self
							.subspace
							.unpack::<keys::workflow::HasWakeConditionKey>(entry.key())
						{
							match state {
								Some(WorkflowState::Sleeping) => state_matches = true,
								Some(WorkflowState::Dead) => state_matches = false,
								_ => {}
							}
						} else if let Ok(_) = self
							.subspace
							.unpack::<keys::workflow::SilenceTsKey>(entry.key())
						{
							match state {
								Some(WorkflowState::Silenced) => state_matches = true,
								_ => state_matches = false,
							}
						}
					}

					if let (Some(workflow_id), true) = (
						current_workflow_id,
						matching_tags == tags.len() && name_matches && state_matches,
					) {
						workflow_ids.push(workflow_id);
					}

					let workflows = self.get_workflows_inner(workflow_ids, &tx).await?;

					Ok(workflows)
				}
			})
			.instrument(tracing::debug_span!("find_workflows_tx"))
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn silence_workflows(&self, workflow_ids: Vec<Id>) -> Result<()> {
		self.pools
			.udb()?
			.txn("gas_debug_silence_workflows", |tx| {
				let workflow_ids = workflow_ids.clone();

				async move {
					// TODO: Parallelize
					for workflow_id in workflow_ids {
						let sub_workflow_wake_subspace = self
							.subspace
							.subspace(&keys::wake::SubWorkflowWakeKey::subspace(workflow_id));
						let tags_subspace = self
							.subspace
							.subspace(&keys::workflow::TagKey::subspace(workflow_id));
						let name_key = keys::workflow::NameKey::new(workflow_id);
						let worker_id_key = keys::workflow::WorkerIdKey::new(workflow_id);
						let output_key = keys::workflow::OutputKey::new(workflow_id);
						let output_subspace = self.subspace.subspace(&output_key);
						let has_wake_condition_key =
							keys::workflow::HasWakeConditionKey::new(workflow_id);
						let silence_ts_key = keys::workflow::SilenceTsKey::new(workflow_id);
						let wake_sub_workflow_key =
							keys::workflow::WakeSubWorkflowKey::new(workflow_id);
						let error_key = keys::workflow::ErrorKey::new(workflow_id);

						let Some(name_entry) =
							tx.get(&self.subspace.pack(&name_key), Serializable).await?
						else {
							tracing::warn!(?workflow_id, "workflow not found");
							continue;
						};

						let workflow_name = name_key.deserialize(&name_entry)?;

						let wake_conditions_subspace = self.subspace.subspace(
							&keys::wake::WorkflowWakeConditionKey::subspace_without_ts(
								workflow_name.clone(),
							),
						);

						let (
							sub_workflow_wake_keys,
							tag_keys,
							wake_condition_keys,
							is_running,
							has_output,
							has_wake_condition,
							is_silenced,
							wake_sub_workflow_entry,
							error_entry,
						) = tokio::try_join!(
							// Read sub workflow wake conditions
							tx.get_ranges_keyvalues(
								RangeOption {
									mode: StreamingMode::WantAll,
									..(&sub_workflow_wake_subspace).into()
								},
								Serializable,
							)
							.map(|res| self
								.subspace
								.unpack::<keys::wake::SubWorkflowWakeKey>(res?.key())
								.map_err(Into::into))
							.try_collect::<Vec<_>>(),
							// Read tags
							tx.get_ranges_keyvalues(
								RangeOption {
									mode: StreamingMode::WantAll,
									..(&tags_subspace).into()
								},
								Serializable,
							)
							.map(|res| self
								.subspace
								.unpack::<keys::workflow::TagKey>(res?.key())
								.map_err(Into::into))
							.try_collect::<Vec<_>>(),
							// Read wake conditions
							tx.get_ranges_keyvalues(
								RangeOption {
									mode: StreamingMode::WantAll,
									..(&wake_conditions_subspace).into()
								},
								Snapshot,
							)
							.map(|res| {
								let entry = res?;

								Ok((
									entry.key().to_vec(),
									self.subspace
										.unpack::<keys::wake::WorkflowWakeConditionKey>(
											entry.key(),
										)?,
								))
							})
							.try_collect::<Vec<_>>(),
							async {
								tx.get(&self.subspace.pack(&worker_id_key), Serializable)
									.await
									.map(|x| x.is_some())
							},
							async {
								tx.get_ranges_keyvalues(
									RangeOption {
										mode: StreamingMode::Exact,
										limit: Some(1),
										..(&output_subspace).into()
									},
									Snapshot,
								)
								.try_next()
								.await
								.map(|x| x.is_some())
							},
							async {
								tx.get(&self.subspace.pack(&has_wake_condition_key), Serializable)
									.await
									.map(|x| x.is_some())
							},
							async {
								tx.get(&self.subspace.pack(&silence_ts_key), Serializable)
									.await
									.map(|x| x.is_some())
							},
							tx.get(&self.subspace.pack(&wake_sub_workflow_key), Serializable),
							tx.get(&self.subspace.pack(&error_key), Serializable),
						)?;

						if is_silenced {
							continue;
						}

						ensure!(!is_running, "cannot silence a running workflow");

						for key in sub_workflow_wake_keys {
							tracing::warn!(
								"workflow {} is being waited on by sub workflow {}, silencing anyway",
								key.workflow_id,
								key.sub_workflow_id
							);
						}

						for key in tag_keys {
							let by_name_and_tag_key = keys::workflow::ByNameAndTagKey::new(
								workflow_name.clone(),
								key.k,
								key.v,
								workflow_id,
							);
							tx.clear(&self.subspace.pack(&by_name_and_tag_key));
						}

						// Clear null key
						{
							let by_name_and_tag_key = keys::workflow::ByNameAndTagKey::null(
								workflow_name.clone(),
								workflow_id,
							);
							tx.clear(&self.subspace.pack(&by_name_and_tag_key));
						}

						// Clear wake conditions
						for (raw_key, key) in wake_condition_keys {
							if key.workflow_id != workflow_id {
								continue;
							}

							tx.add_conflict_range(
								&raw_key,
								&end_of_key_range(&raw_key),
								ConflictRangeType::Read,
							)?;

							tx.clear(&raw_key);
						}

						// Clear sub workflow secondary idx
						if let Some(entry) = wake_sub_workflow_entry {
							let sub_workflow_id = wake_sub_workflow_key.deserialize(&entry)?;

							let sub_workflow_wake_key =
								keys::wake::SubWorkflowWakeKey::new(sub_workflow_id, workflow_id);

							tx.clear(&self.subspace.pack(&sub_workflow_wake_key));
						}

						// Clear signals secondary index
						let wake_signals_subspace = self
							.subspace
							.subspace(&keys::workflow::WakeSignalKey::subspace(workflow_id));
						tx.clear_subspace_range(&wake_signals_subspace);

						// Clear "has wake condition"
						tx.clear(&self.subspace.pack(&has_wake_condition_key));

						tx.set(
							&self.subspace.pack(&silence_ts_key),
							&silence_ts_key.serialize(rivet_util::timestamp::now())?,
						);

						// Clear metric
						let metric = if has_output {
							keys::metric::Metric::WorkflowComplete(workflow_name.clone())
						} else if has_wake_condition {
							keys::metric::Metric::WorkflowSleeping(workflow_name.clone())
						} else {
							// Workflow is dead
							let error =
								error_key.deserialize(&error_entry.context("key should exist")?)?;

							// Clear dead idx
							tx.clear(&self.subspace.pack(&keys::workflow::DeadIdxKey::new(
								workflow_name.clone(),
								error.clone(),
								workflow_id,
							)));

							keys::metric::Metric::WorkflowDead(workflow_name.clone(), error)
						};

						update_metric(&tx.with_subspace(self.subspace.clone()), Some(metric), None);
					}

					Ok(())
				}
			})
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn wake_workflows(&self, workflow_ids: Vec<Id>) -> Result<()> {
		self.pools
			.udb()?
			.txn("gas_debug_wake_workflows", |tx| {
				let workflow_ids = workflow_ids.clone();
				async move {
					tx.tag("wake_workflows")?;

					let tx = tx.with_subspace(self.subspace.clone());

					for workflow_id in workflow_ids {
						let name_key = keys::workflow::NameKey::new(workflow_id);
						let worker_id_key = keys::workflow::WorkerIdKey::new(workflow_id);
						let has_wake_condition_key =
							keys::workflow::HasWakeConditionKey::new(workflow_id);
						let error_key = keys::workflow::ErrorKey::new(workflow_id);
						let silence_ts_key = keys::workflow::SilenceTsKey::new(workflow_id);
						let output_key = keys::workflow::OutputKey::new(workflow_id);
						let output_subspace = self.subspace.subspace(&output_key);

						let (
							workflow_name_entry,
							is_running,
							has_wake_condition,
							is_silenced,
							has_output,
							error,
						) = tokio::try_join!(
							tx.read_opt(&name_key, Serializable),
							tx.exists(&worker_id_key, Serializable),
							tx.exists(&has_wake_condition_key, Serializable),
							tx.exists(&silence_ts_key, Serializable),
							async {
								tx.get_ranges_keyvalues(
									RangeOption {
										mode: StreamingMode::Exact,
										limit: Some(1),
										..(&output_subspace).into()
									},
									Snapshot,
								)
								.try_next()
								.await
								.map_err(Into::into)
								.map(|x| x.is_some())
							},
							tx.read_opt(&error_key, Serializable),
						)?;

						let Some(workflow_name) = workflow_name_entry else {
							tracing::warn!("workflow {workflow_id} does not exist");
							continue;
						};

						if is_running || is_silenced {
							continue;
						}

						if has_output {
							tracing::warn!("cannot wake a completed workflow");
							continue;
						}

						tx.write(
							&keys::wake::WorkflowWakeConditionKey::new(
								workflow_name.clone(),
								workflow_id,
								keys::wake::WakeCondition::Immediate,
							),
							(),
						)?;

						tx.write(&has_wake_condition_key, ())?;

						if !has_wake_condition {
							let error = error.context("key should exist")?;

							tx.delete(&keys::workflow::DeadIdxKey::new(
								workflow_name.clone(),
								error.clone(),
								workflow_id,
							));

							update_metric(
								&tx,
								Some(keys::metric::Metric::WorkflowDead(
									workflow_name.clone(),
									error,
								)),
								Some(keys::metric::Metric::WorkflowSleeping(workflow_name)),
							);
						}
					}

					Ok(())
				}
			})
			.instrument(tracing::debug_span!("wake_workflows_tx"))
			.await?;

		self.bump(BumpSubSubject::Worker);

		Ok(())
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn get_workflow_history(
		&self,
		workflow_id: Id,
		include_forgotten: bool,
	) -> Result<Option<HistoryData>> {
		self.pools
			.udb()?
			.txn("gas_debug_get_workflow_history", |tx| {
				async move {
					let history_subspace =
						self.subspace
							.subspace(&keys::history::HistorySubspaceKey::new(
								workflow_id,
								if include_forgotten {
									keys::history::HistorySubspaceVariant::All
								} else {
									keys::history::HistorySubspaceVariant::Active
								},
							));

					let (wf, events) = tokio::try_join!(
						async {
							self.get_workflows(vec![workflow_id])
								.await
								.map(|wfs| wfs.into_iter().next())
						},
						async {
							let mut events_by_location: HashMap<Location, Vec<Event>> =
								HashMap::new();
							let mut current_event =
								WorkflowHistoryEventBuilder::new(Location::empty(), false);

							let mut stream = tx.get_ranges_keyvalues(
								RangeOption {
									mode: StreamingMode::WantAll,
									..(&history_subspace).into()
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
									.unpack::<keys::history::PartialEventKey>(entry.key())?;

								if current_event.location != partial_key.location {
									if current_event.location.is_empty() {
										current_event = WorkflowHistoryEventBuilder::new(
											partial_key.location.clone(),
											partial_key.forgotten,
										);
									} else {
										// Insert current event builder to into wf events and
										// reset state
										let previous_event = std::mem::replace(
											&mut current_event,
											WorkflowHistoryEventBuilder::new(
												partial_key.location.clone(),
												partial_key.forgotten,
											),
										);
										events_by_location
											.entry(previous_event.location.root())
											.or_default()
											.push(Event::try_from(previous_event)?);
									}
								}

								// Parse current key as any event key
								if let Ok(key) = self
									.subspace
									.unpack::<keys::history::EventTypeKey>(entry.key())
								{
									let event_type = key.deserialize(entry.value())?;

									current_event.event_type = Some(event_type);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::VersionKey>(entry.key())
								{
									let version = key.deserialize(entry.value())?;

									current_event.version = Some(version);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::CreateTsKey>(entry.key())
								{
									let create_ts = key.deserialize(entry.value())?;

									current_event.create_ts = Some(create_ts);
								} else if let Ok(key) =
									self.subspace.unpack::<keys::history::NameKey>(entry.key())
								{
									let name = key.deserialize(entry.value())?;

									current_event.name = Some(name);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::SignalIdKey>(entry.key())
								{
									let signal_id = key.deserialize(entry.value())?;

									current_event.signal_id = Some(signal_id);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::SubWorkflowIdKey>(entry.key())
								{
									let sub_workflow_id = key.deserialize(entry.value())?;

									current_event.sub_workflow_id = Some(sub_workflow_id);
								} else if let Ok(_key) = self
									.subspace
									.unpack::<keys::history::InputChunkKey>(entry.key())
								{
									current_event.input_chunks.push(entry);
								} else if let Ok(_key) = self
									.subspace
									.unpack::<keys::history::OutputChunkKey>(entry.key())
								{
									current_event.output_chunks.push(entry);
								} else if let Ok(key) =
									self.subspace.unpack::<keys::history::ErrorKey>(entry.key())
								{
									if let Some(err) = current_event
										.errors
										.iter_mut()
										.find(|err| err.error == key.error)
									{
										err.count += 1;
										err.latest_ts = err.latest_ts.max(key.ts);
									} else {
										current_event.errors.push(ActivityError {
											error: key.error,
											count: 1,
											latest_ts: key.ts,
										});
									}
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::IterationKey>(entry.key())
								{
									let iteration = key.deserialize(entry.value())?;

									current_event.iteration = Some(iteration);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::DeadlineTsKey>(entry.key())
								{
									let deadline_ts = key.deserialize(entry.value())?;

									current_event.deadline_ts = Some(deadline_ts);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::SleepStateKey>(entry.key())
								{
									let sleep_state = key.deserialize(entry.value())?;

									current_event.sleep_state = Some(sleep_state);
								} else if let Ok(key) =
									self.subspace
										.unpack::<keys::history::InnerEventTypeKey>(entry.key())
								{
									let inner_event_type = key.deserialize(entry.value())?;

									current_event.inner_event_type = Some(inner_event_type);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::InnerVersionKey>(entry.key())
								{
									let inner_version = key.deserialize(entry.value())?;

									current_event.inner_version = Some(inner_version);
								} else if let Ok(key) =
									self.subspace
										.unpack::<keys::history::IndexedSignalIdKey>(entry.key())
								{
									ensure!(
										current_event.indexed_signal_ids.len() == key.index,
										"corrupt history location={} expected={} got={}, indexed signal doesn't exist yet or is out of order",
										partial_key.location,
										current_event.indexed_signal_ids.len(),
										key.index,
									);

									let signal_id = key.deserialize(entry.value())?;
									current_event
										.indexed_signal_ids
										.insert(key.index, signal_id);
								} else if let Ok(key) = self
									.subspace
									.unpack::<keys::history::IndexedNameKey>(entry.key())
								{
									ensure!(
										current_event.indexed_names.len() == key.index,
										"corrupt history location={} expected={} got={}, indexed name doesn't exist yet or is out of order",
										partial_key.location,
										current_event.indexed_names.len(),
										key.index,
									);

									let name = key.deserialize(entry.value())?;
									current_event.indexed_names.insert(key.index, name);
								} else if let Ok(key) =
									self.subspace
										.unpack::<keys::history::IndexedInputChunkKey>(entry.key())
								{
									if let Some(input_chunks) =
										current_event.indexed_input_chunks.get_mut(key.index)
									{
										input_chunks.push(entry);
									} else if current_event.indexed_input_chunks.len()
										== key.index
									{
										current_event
										.indexed_input_chunks
										.push(vec![entry]);
									} else {
										bail!(
											"corrupt history location={} expected={} got={}, indexed chunk doesn't exist yet or is out of order",
											partial_key.location,
											current_event.indexed_input_chunks.len(),
											key.index,
										);
									}
								}

								// We ignore keys we don't need (like tags)
							}
							// Insert final event
							if !current_event.location.is_empty() {
								events_by_location
									.entry(current_event.location.root())
									.or_default()
									.push(Event::try_from(current_event)?);
							}

							Ok(events_by_location)
						}
					)?;

					let Some(wf) = wf else {
						return Ok(None);
					};

					let mut flat_events =
						events.into_iter().flat_map(|(_, v)| v).collect::<Vec<_>>();
					flat_events.sort_by(|a, b| a.location.cmp(&b.location));

					Ok(Some(HistoryData {
						wf,
						events: flat_events,
					}))
				}
			})
			.instrument(tracing::debug_span!("pull_workflow_history_tx"))
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn get_signals(&self, signal_ids: Vec<Id>) -> Result<Vec<SignalData>> {
		self.pools
			.udb()?
			.txn("gas_debug_get_signals", |tx| {
				let signal_ids = signal_ids.clone();
				async move { self.get_signals_inner(signal_ids, &tx).await }
			})
			.instrument(tracing::debug_span!("get_signals_tx"))
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn find_signals(
		&self,
		_tags: &[(String, String)],
		workflow_id: Option<Id>,
		name: Option<&str>,
		state: Option<SignalState>,
	) -> Result<Vec<SignalData>> {
		// NOTE: this does a full scan of all keys under signal/data and filters in memory
		self.pools
			.udb()?
			.txn("gas_debug_find_signals", |tx| {
				let name = name.clone();
				let workflow_id = workflow_id.clone();
				async move {
					let mut signal_ids = Vec::new();

					let data_subspace = self
						.subspace
						.subspace(&keys::signal::DataSubspaceKey::new());

					let mut stream = tx.get_ranges_keyvalues(
						RangeOption {
							mode: StreamingMode::Iterator,
							..(&data_subspace).into()
						},
						Snapshot,
					);

					let mut current_signal_id = None;
					let mut name_matches = name.is_none();
					let mut workflow_id_matches = workflow_id.is_none();
					let mut state_matches = state.is_none() || state == Some(SignalState::Pending);

					while let Some(entry) = stream.try_next().await? {
						let signal_id = *self.subspace.unpack::<JustId>(entry.key())?;

						if let Some(curr) = current_signal_id {
							if signal_id != curr {
								// Save if matches query
								if name_matches && workflow_id_matches && state_matches {
									signal_ids.push(curr);

									if signal_ids.len() >= 100 {
										current_signal_id = None;
										break;
									}
								}

								// Reset state
								name_matches = name.is_none();
								workflow_id_matches = workflow_id.is_none();
								state_matches =
									state.is_none() || state == Some(SignalState::Pending);
							}
						}

						current_signal_id = Some(signal_id);

						if let Ok(name_key) =
							self.subspace.unpack::<keys::signal::NameKey>(entry.key())
						{
							if let Some(name) = &name {
								let signal_name = name_key.deserialize(entry.value())?;

								name_matches = &signal_name == name;
							}
						} else if let Ok(workflow_id_key) = self
							.subspace
							.unpack::<keys::signal::WorkflowIdKey>(entry.key())
						{
							if let Some(workflow_id) = &workflow_id {
								let signal_workflow_id =
									workflow_id_key.deserialize(entry.value())?;

								workflow_id_matches = &signal_workflow_id == workflow_id;
							}
						} else if let Ok(_) =
							self.subspace.unpack::<keys::signal::AckTsKey>(entry.key())
						{
							// Has ack timestamp
							match state {
								Some(SignalState::Acked) => state_matches = true,
								Some(SignalState::Pending) => state_matches = false,
								_ => {}
							}
						} else if let Ok(_) = self
							.subspace
							.unpack::<keys::signal::SilenceTsKey>(entry.key())
						{
							match state {
								Some(SignalState::Silenced) => state_matches = true,
								_ => state_matches = false,
							}
						}
					}

					if let (Some(signal_id), true) = (
						current_signal_id,
						name_matches && workflow_id_matches && state_matches,
					) {
						signal_ids.push(signal_id);
					}

					let signals = self.get_signals_inner(signal_ids, &tx).await?;

					Ok(signals)
				}
			})
			.instrument(tracing::debug_span!("find_signals_tx"))
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn silence_signals(&self, signal_ids: Vec<Id>) -> Result<()> {
		self.pools
			.udb()?
			.txn("gas_debug_silence_signals", |tx| {
				let signal_ids = signal_ids.clone();

				async move {
					// TODO: Parallelize
					for signal_id in signal_ids {
						let signal_name_key = keys::signal::NameKey::new(signal_id);
						let create_ts_key = keys::signal::CreateTsKey::new(signal_id);
						let workflow_id_key = keys::signal::WorkflowIdKey::new(signal_id);
						let silence_ts_key = keys::signal::SilenceTsKey::new(signal_id);
						let ack_ts_key = keys::signal::AckTsKey::new(signal_id);

						let (
							signal_name_entry,
							create_ts_entry,
							workflow_id_entry,
							silence_ts_entry,
							ack_ts_entry,
						) = tokio::try_join!(
							tx.get(&self.subspace.pack(&signal_name_key), Serializable),
							tx.get(&self.subspace.pack(&create_ts_key), Serializable),
							tx.get(&self.subspace.pack(&workflow_id_key), Serializable),
							tx.get(&self.subspace.pack(&silence_ts_key), Serializable),
							tx.get(&self.subspace.pack(&ack_ts_key), Serializable),
						)?;

						if silence_ts_entry.is_some() {
							continue;
						}

						let Some(signal_name_entry) = signal_name_entry else {
							tracing::warn!(?signal_id, "signal not found");
							continue;
						};

						let signal_name = signal_name_key.deserialize(&signal_name_entry)?;

						let create_ts = create_ts_key
							.deserialize(&create_ts_entry.context("key should exist")?)?;

						let workflow_id = workflow_id_key
							.deserialize(&workflow_id_entry.context("key should exist")?)?;

						let workflow_name_key = keys::workflow::NameKey::new(workflow_id);

						let workflow_name_entry = tx
							.get(&self.subspace.pack(&workflow_name_key), Serializable)
							.await?;

						let workflow_name = workflow_name_key
							.deserialize(&workflow_name_entry.context("key should exist")?)?;

						// Clear pending key
						let mut pending_signal_key = keys::workflow::PendingSignalKey::new(
							workflow_id,
							signal_name.clone(),
							signal_id,
						);
						pending_signal_key.ts = create_ts;
						tx.clear(&self.subspace.pack(&pending_signal_key));

						// Clear wake condition
						let mut wake_condition_key = keys::wake::WorkflowWakeConditionKey::new(
							workflow_name,
							workflow_id,
							keys::wake::WakeCondition::Signal { signal_id },
						);
						wake_condition_key.ts = create_ts;
						tx.clear(&self.subspace.pack(&wake_condition_key));

						tx.set(
							&self.subspace.pack(&silence_ts_key),
							&silence_ts_key.serialize(rivet_util::timestamp::now())?,
						);

						// An acked signal was already inserted into the prune idx when it was acked
						if ack_ts_entry.is_none() {
							// Insert into prune idx so the pruner can reclaim this signal once it
							// is old enough
							let prune_idx_key = keys::signal::PruneIdxKey::new(signal_id);
							tx.set(
								&self.subspace.pack(&prune_idx_key),
								&prune_idx_key.serialize(())?,
							);

							update_metric(
								&tx.with_subspace(self.subspace.clone()),
								Some(keys::metric::Metric::SignalPending(signal_name)),
								None,
							);
						}
					}

					Ok(())
				}
			})
			.await
			.map_err(Into::into)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn revive_workflows(
		&self,
		names: &[&str],
		error_like: &[&str],
		dry_run: bool,
	) -> Result<usize> {
		let mut futs = FuturesUnordered::new();

		for name in names {
			futs.push(self.revive_workflows_inner(name, error_like, dry_run));
		}

		let mut total = 0;
		while let Some(res) = futs.next().await {
			total += res?;
		}

		tracing::info!(?total, "workflows revived");

		Ok(total)
	}

	async fn backfill_dead_workflows(
		&self,
		limit: usize,
		last_key: Option<&[u8]>,
	) -> Result<(usize, Option<Vec<u8>>)> {
		ensure!(limit > 0, "backfill workflow limit must be positive");
		let last_key = last_key.map(|x| x.to_vec());

		self.pools
			.udb()?
			.txn("gas_debug_backfill_dead_workflows", |tx| {
				let last_key = &last_key;
				async move {
					let tx = tx.with_subspace(self.subspace.clone());
					let (start, end) = self
						.subspace
						.subspace(&keys::workflow::DataSubspaceKey::new())
						.range();
					let mut total = 0;
					// This remains a raw range boundary, accepting previously persisted first-key cursors.
					let mut new_last_key = Some(last_key.clone().unwrap_or(start));

					let fut = async {
						while total < limit {
							let Some(start) = &new_last_key else { break };
							// A RocksDB range stream materializes all values before yielding. Seek only
							// the next key so input, state, and output payloads never enter this scan.
							let first_key = tx
								.get_key(
									&KeySelector::first_greater_or_equal(start.as_slice()),
									Snapshot,
								)
								.await?;
							if first_key.is_empty() || first_key.as_slice() >= end.as_slice() {
								new_last_key = None;
								break;
							}
							let workflow_id = *self.subspace.unpack::<JustId>(&first_key)?;
							let output_range = self
								.subspace
								.subspace(&keys::workflow::OutputKey::new(workflow_id))
								.range();
							// get_key tracks the returned key, not the empty seek gap. Cover the entire
							// output prefix so a concurrent completion cannot leave a stale dead index.
							tx.add_conflict_range(
								&output_range.0,
								&output_range.1,
								ConflictRangeType::Read,
							)?;
							let name_key = keys::workflow::NameKey::new(workflow_id);
							let error_key = keys::workflow::ErrorKey::new(workflow_id);
							let worker_key = keys::workflow::WorkerIdKey::new(workflow_id);
							let wake_key = keys::workflow::HasWakeConditionKey::new(workflow_id);
							let silence_key = keys::workflow::SilenceTsKey::new(workflow_id);
							let output_selector =
								KeySelector::first_greater_or_equal(output_range.0.as_slice());
							let (name, error, worker, wake, silenced, output_key) = tokio::try_join!(
								tx.read_opt(&name_key, Serializable),
								tx.read_opt(&error_key, Serializable),
								tx.exists(&worker_key, Serializable),
								tx.exists(&wake_key, Serializable),
								tx.exists(&silence_key, Serializable),
								tx.get_key(&output_selector, Snapshot),
							)?;
							let has_output = !output_key.is_empty()
								&& output_key.as_slice() < output_range.1.as_slice();
							if let (Some(name), Some(error)) = (name, error)
								&& !worker && !wake && !silenced
								&& !has_output
							{
								tx.write(
									&keys::workflow::DeadIdxKey::new(name, error, workflow_id),
									(),
								)?;
							}
							// Do not await between indexing and advancing. A timeout leaves an unfinished
							// workflow at the cursor, while completed writes and progress commit together.
							new_last_key = Some(
								self.subspace
									.subspace(
										&keys::workflow::DataSubspaceKey::new_with_workflow_id(
											workflow_id,
										),
									)
									.range()
									.1,
							);
							total += 1;
						}
						anyhow::Ok(())
					};

					match tokio::time::timeout(EARLY_TXN_TIMEOUT, fut).await {
						Ok(res) => res?,
						Err(_) => tracing::debug!("timed out reading workflows"),
					}
					Ok((total, new_last_key))
				}
			})
			.instrument(tracing::debug_span!("backfill_dead_workflows_tx"))
			.await
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn prune_workflows(
		&self,
		before_ts: i64,
		limit: usize,
		last_key: Option<&[u8]>,
	) -> Result<(usize, Option<Vec<u8>>)> {
		let last_key = last_key.map(|x| x.to_vec());
		let dc_name = &self.config.dc_name()?;

		#[derive(clickhouse::Row, Serialize)]
		struct WorkflowTombstone<'a> {
			datacenter: &'a str,
			workflow_id: Id,
			created_at: i64,
			deleted_at: i64,
		}

		let (prune_count, inserter, new_last_key) = self
			.pools
			.udb()?
			.txn("gas_debug_prune_workflows", |tx| {
				let last_key = &last_key;
				async move {
					tx.tag("prune_workflows")?;

					let start = Instant::now();
					let tx = tx.with_subspace(self.subspace.clone());
					let now = rivet_util::timestamp::now();
					let mut prune_count = 0;
					let mut new_last_key = None;
					let mut inserter = if let Some(ch) = self.pools.clickhouse_option() {
						Some(
							ch.clone()
								.with_database("db_gasoline")
								.inserter::<WorkflowTombstone>("workflow_tombstones"),
						)
					} else {
						None
					};

					let subspace_start = self
						.subspace
						.subspace(&keys::workflow::PruneIdxKey::entire_subspace())
						.range()
						.0;
					let subspace_end = self
						.subspace
						.subspace(&keys::workflow::PruneIdxKey::subspace(before_ts))
						.range()
						.1;

					let range_start = if let Some(last_key) = last_key {
						last_key
					} else {
						&subspace_start
					};
					let range_end = &subspace_end;

					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(range_start.as_slice(), range_end.as_slice()).into()
						},
						Serializable,
					);

					loop {
						if start.elapsed() > EARLY_TXN_TIMEOUT {
							tracing::warn!("timed out pruning workflows");
							break;
						}

						let Some(entry) = stream.try_next().await? else {
							new_last_key = None;
							break;
						};

						let prune_key = tx.unpack::<keys::workflow::PruneIdxKey>(entry.key())?;

						// Remove prune idx entry
						tx.delete(&prune_key);

						match prune_key.variant {
							PruneVariant::All => {
								tx.delete_key_subspace(
									&keys::workflow::DataSubspaceKey::new_with_workflow_id(
										prune_key.workflow_id,
									),
								);

								tx.delete_key_subspace(
									&keys::workflow::PendingSignalKey::entire_subspace(
										prune_key.workflow_id,
									),
								);

								if let (Some(inserter), Some(create_ts)) = (
									&mut inserter,
									tx.read_opt(
										&keys::workflow::CreateTsKey::new(prune_key.workflow_id),
										Snapshot,
									)
									.await?,
								) {
									inserter
										.write(&WorkflowTombstone {
											datacenter: dc_name,
											workflow_id: prune_key.workflow_id,
											created_at: create_ts,
											deleted_at: now,
										})
										.await?;
								}
							}
							PruneVariant::History => {
								let history_subspace = keys::history::HistorySubspaceKey::new(
									prune_key.workflow_id,
									keys::history::HistorySubspaceVariant::All,
								);

								tx.delete_key_subspace(&history_subspace);
								tx.write(
									&keys::workflow::PruneTsKey::new(prune_key.workflow_id),
									now,
								)?;
							}
							// Should be unreachable, we don't insert into the idx if prune variant
							// is `none`
							PruneVariant::None => continue,
						}

						prune_count += 1;
						new_last_key = Some([entry.key(), &[0xff]].concat());

						if prune_count >= limit {
							break;
						}
					}

					Ok((prune_count, inserter, new_last_key))
				}
			})
			.instrument(tracing::debug_span!("prune_workflows_tx"))
			.await?;

		if let Some(mut inserter) = inserter {
			inserter.force_commit().await?;
			inserter.end().await?;
		}

		Ok((prune_count, new_last_key))
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn prune_signals(
		&self,
		before_ts: i64,
		limit: usize,
		last_key: Option<&[u8]>,
	) -> Result<(usize, Option<Vec<u8>>)> {
		let last_key = last_key.map(|x| x.to_vec());

		self.pools
			.udb()?
			.txn("gas_debug_prune_signals", |tx| {
				let last_key = &last_key;
				async move {
					tx.tag("prune_signals")?;

					let start = Instant::now();
					let tx = tx.with_subspace(self.subspace.clone());
					let mut prune_count = 0;
					let mut new_last_key = None;

					let subspace_start = self
						.subspace
						.subspace(&keys::signal::PruneIdxKey::entire_subspace())
						.range()
						.0;
					let subspace_end = self
						.subspace
						.subspace(&keys::signal::PruneIdxKey::subspace(before_ts))
						.range()
						.1;

					let range_start = if let Some(last_key) = last_key {
						last_key
					} else {
						&subspace_start
					};
					let range_end = &subspace_end;

					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(range_start.as_slice(), range_end.as_slice()).into()
						},
						Serializable,
					);

					loop {
						if start.elapsed() > EARLY_TXN_TIMEOUT {
							tracing::warn!("timed out pruning signals");
							break;
						}

						let Some(entry) = stream.try_next().await? else {
							new_last_key = None;
							break;
						};

						let prune_key = tx.unpack::<keys::signal::PruneIdxKey>(entry.key())?;

						// Remove prune idx entry
						tx.delete(&prune_key);

						tx.delete_key_subspace(&keys::signal::DataSubspaceKey::new_with_signal_id(
							prune_key.signal_id,
						));

						prune_count += 1;
						new_last_key = Some([entry.key(), &[0xff]].concat());

						if prune_count >= limit {
							break;
						}
					}

					Ok((prune_count, new_last_key))
				}
			})
			.instrument(tracing::debug_span!("prune_signals_tx"))
			.await
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn prune_complete_workflow_history(
		&self,
		names: &[&str],
		before_ts: i64,
		dry_run: bool,
		parallelization: u16,
	) -> Result<usize> {
		ensure!(parallelization > 0);
		ensure!(parallelization < 1024);

		let chunk_size = u128::MAX / parallelization as u128;
		let mut futs = FuturesUnordered::new();

		for i in 0..parallelization as u128 {
			let start = i * chunk_size;
			futs.push(self.prune_workflow_history_inner(
				names,
				before_ts,
				dry_run,
				start,
				start + chunk_size,
			));
		}

		let mut total = 0;
		while let Some(res) = futs.next().await {
			total += res?;
		}

		tracing::info!(?total, "workflows deleted");

		Ok(total)
	}

	#[tracing::instrument(level = "debug", skip_all)]
	async fn prune_acked_signals(
		&self,
		names: &[&str],
		before_ts: i64,
		dry_run: bool,
		parallelization: u16,
		max_per_txn: Option<usize>,
	) -> Result<usize> {
		ensure!(parallelization > 0);
		ensure!(parallelization < 1024);

		let chunk_size = u128::MAX / parallelization as u128;
		let mut futs = FuturesUnordered::new();

		for i in 0..parallelization as u128 {
			let start = i * chunk_size;
			futs.push(self.prune_signals_inner(
				names,
				before_ts,
				dry_run,
				start,
				start + chunk_size,
				max_per_txn,
			));
		}

		let mut total = 0;
		while let Some(res) = futs.next().await {
			total += res?;
		}

		tracing::info!(?total, "signals deleted");

		Ok(total)
	}

	#[tracing::instrument(skip_all)]
	async fn inspect_workflow_repair(
		&self,
		workflow_id: Id,
		variant: RepairVariant,
		location: Option<Location>,
	) -> Result<RepairInspection> {
		self.inspect_workflow_repair_inner(workflow_id, variant, location)
			.await
	}

	#[tracing::instrument(skip_all)]
	async fn repair_workflow(
		&self,
		workflow_id: Id,
		variant: RepairVariant,
		location: Option<Location>,
	) -> Result<RepairOutcome> {
		self.repair_workflow_inner(workflow_id, variant, location)
			.await
	}

	#[tracing::instrument(skip_all)]
	async fn verify_workflow_repair(
		&self,
		workflow_id: Id,
		variant: RepairVariant,
		location: Location,
	) -> Result<RepairVerification> {
		self.verify_workflow_repair_inner(workflow_id, variant, location)
			.await
	}
}

impl DatabaseKv {
	pub async fn revive_workflows_inner(
		&self,
		name: &str,
		error_like: &[&str],
		dry_run: bool,
	) -> Result<usize> {
		let mut total = 0;
		let mut last_key = None;

		loop {
			let (new_last_key, workflow_ids) = self.pools
				.udb()?
				.txn("gas_debug_revive_workflows", |tx| {
					let mut last_key = last_key.clone();

					async move {
						tx.tag(&format!("revive_workflows:{name}"))?;

						let mut workflow_ids = Vec::new();

						let entire_subspace_key = keys::workflow::DeadIdxKey::subspace(name.to_string());
						let start = if let Some(last_key) = last_key.take() {
							last_key
						} else {
							self.subspace.subspace(&entire_subspace_key).range().0
						};
						let end = self.subspace.subspace(&entire_subspace_key).range().1;

						let mut stream = tx.get_ranges_keyvalues(
							RangeOption {
								mode: StreamingMode::WantAll,
								..(start, end).into()
							},
							Snapshot,
						);

						let mut workflows_processed = 0;

						let fut = async {
							while let Some(entry) = stream.try_next().await? {
								let dead_idx_key = self.subspace.unpack::<keys::workflow::DeadIdxKey>(entry.key())?;

								if error_like.is_empty() || error_like.iter().any(|err| dead_idx_key.error.to_lowercase().contains(err)) {
									workflow_ids.push(dead_idx_key.workflow_id);
								}

								workflows_processed += 1;
								last_key = Some(entry.key().to_vec());
							}

							last_key = None;

							anyhow::Ok(())
						};

						match tokio::time::timeout(EARLY_TXN_TIMEOUT, fut).await {
							Ok(res) => res?,
							Err(_) => tracing::debug!("timed out reading workflows"),
						}

						tracing::info!(?workflows_processed, matching_workflows=?workflow_ids.len(), "batch processed workflows");

						Ok((last_key, workflow_ids))
					}
				})
				.instrument(tracing::debug_span!("find_dead_workflows_tx"))
				.await?;

			last_key = new_last_key;
			total += workflow_ids.len();

			if !dry_run {
				self.wake_workflows(workflow_ids).await?;
			}

			if last_key.is_none() {
				tracing::info!("reached end of workflows");
				break;
			}
		}

		Ok(total)
	}

	pub async fn prune_workflow_history_inner(
		&self,
		names: &[&str],
		before_ts: i64,
		dry_run: bool,
		start: u128,
		end: u128,
	) -> Result<usize> {
		let mut total = 0;
		let mut current_workflow_id = Some(Id::v1(Uuid::from_u128(start), self.config.dc_label()));
		let end_workflow_id = Id::v1(Uuid::from_u128(end), self.config.dc_label());

		loop {
			let (new_current_workflow_id, workflow_ids) = self.pools
				.udb()?
				.txn("gas_debug_fetch_prune_workflows", |tx| {
					async move {
						tx.tag("fetch_prune_workflows")?;

						let mut workflow_ids = Vec::new();

						let key_end = keys::workflow::DataSubspaceKey::new_with_workflow_id(end_workflow_id);
						let end = self.subspace.subspace(&key_end).range().1;

						let start = if let Some(current_workflow_id) = current_workflow_id {
							let key_start = keys::workflow::DataSubspaceKey::new_with_workflow_id(current_workflow_id);
							let mut bytes = self.subspace.subspace(&key_start).range().0;

							if let Some(b) = bytes.last_mut() {
								*b = 255;
							}

							bytes
						} else {
							let entire_subspace_key = keys::workflow::DataSubspaceKey::new();

							self.subspace.subspace(&entire_subspace_key).range().0
						};

						let mut stream = tx.get_ranges_keyvalues(
							RangeOption {
								mode: StreamingMode::WantAll,
								..(start, end).into()
							},
							Snapshot,
						);

						let mut workflows_processed = 0;
						let mut current_workflow_id = None;
						let mut name_matches = names.is_empty();
						let mut state_matches = false;
						let mut ts_matches = false;
						let mut history_matches = false;

						let fut = async {
							while let Some(entry) = stream.try_next().await? {
								let workflow_id = *self.subspace.unpack::<JustId>(entry.key())?;

								if let Some(curr) = current_workflow_id {
									if workflow_id != curr {
										// Save if matches query
										if name_matches && state_matches && ts_matches && history_matches {
											workflow_ids.push(curr);
										}

										workflows_processed += 1;

										// Reset state
										name_matches = names.is_empty();
										state_matches = false;
										ts_matches = false;
										history_matches = false;
									}
								}

								current_workflow_id = Some(workflow_id);

								if let Ok(name_key) =
									self.subspace.unpack::<keys::workflow::NameKey>(entry.key())
								{
									let workflow_name = name_key.deserialize(entry.value())?;

									name_matches = names.is_empty() || names.iter().any(|name| &workflow_name == name);
								} else if let Ok(_) = self
									.subspace
									.unpack::<keys::workflow::OutputChunkKey>(entry.key())
								{
									state_matches = true;
								} else if let Ok(_) = self
									.subspace
									.unpack::<keys::workflow::WorkerIdKey>(entry.key())
								{
									state_matches = false;
								} else if let Ok(_) = self
									.subspace
									.unpack::<keys::workflow::HasWakeConditionKey>(entry.key())
								{
									state_matches = false;
								} else if let Ok(_) = self
									.subspace
									.unpack::<keys::workflow::PruneTsKey>(entry.key())
								{
									state_matches = false;
								} else if let Ok(create_ts_key) = self
									.subspace
									.unpack::<keys::workflow::CreateTsKey>(entry.key())
								{
									let create_ts = create_ts_key.deserialize(entry.value())?;

									ts_matches = create_ts < before_ts;
								} else if let Ok(_) = self
									.subspace
									.unpack::<keys::history::PartialEventKey>(entry.key())
								{
									history_matches = true;
								}
							}

							if let (Some(workflow_id), true) = (
								current_workflow_id,
								name_matches && state_matches && ts_matches && history_matches
							) {
								workflow_ids.push(workflow_id);
								current_workflow_id = None;
							}

							if current_workflow_id.is_some() {
								workflows_processed += 1;
							}

							anyhow::Ok(())
						};

						match tokio::time::timeout(EARLY_TXN_TIMEOUT, fut).await {
							Ok(res) => res?,
							Err(_) => tracing::debug!("timed out reading workflows"),
						}

						tracing::info!(?workflows_processed, matching_workflows=?workflow_ids.len(), "batch processed workflows");

						Ok((current_workflow_id, workflow_ids))
					}
				})
				.instrument(tracing::debug_span!("find_complete_workflows_tx"))
				.await?;

			current_workflow_id = new_current_workflow_id;
			total += workflow_ids.len();

			if !dry_run {
				let workflow_ids = &workflow_ids;
				self.pools
					.udb()?
					.txn("gas_debug_prune_workflow_history", |tx| async move {
						tx.tag("prune_workflows")?;

						let tx = tx.with_subspace(self.subspace.clone());
						let now = rivet_util::timestamp::now();

						for workflow_id in workflow_ids {
							let history_subspace = keys::history::HistorySubspaceKey::new(
								*workflow_id,
								keys::history::HistorySubspaceVariant::All,
							);

							tx.delete_key_subspace(&history_subspace);

							tx.write(&keys::workflow::PruneTsKey::new(*workflow_id), now)?;
						}

						Ok(())
					})
					.await?;
			}

			if current_workflow_id.is_none() {
				tracing::info!("reached end of workflows");
				break;
			}
		}

		Ok(total)
	}

	pub async fn prune_signals_inner(
		&self,
		names: &[&str],
		before_ts: i64,
		dry_run: bool,
		start: u128,
		end: u128,
		max_per_txn: Option<usize>,
	) -> Result<usize> {
		let mut total = 0;
		let mut current_signal_id = Some(Id::v1(Uuid::from_u128(start), self.config.dc_label()));
		let end_signal_id = Id::v1(Uuid::from_u128(end), self.config.dc_label());

		loop {
			let (new_current_signal_id, signal_ids) = self.pools
				.udb()?
				.txn("gas_debug_fetch_prune_signals", |tx| {
					async move {
						tx.tag("fetch_prune_signals")?;

						let mut signal_ids = Vec::new();

						let key_end = keys::signal::DataSubspaceKey::new_with_signal_id(end_signal_id);
						let end = self.subspace.subspace(&key_end).range().1;

						let start = if let Some(current_signal_id) = current_signal_id {
							let key_start = keys::signal::DataSubspaceKey::new_with_signal_id(current_signal_id);
							let mut bytes = self.subspace.subspace(&key_start).range().0;

							if let Some(b) = bytes.last_mut() {
								*b = 255;
							}

							bytes
						} else {
							let entire_subspace_key = keys::signal::DataSubspaceKey::new();

							self.subspace.subspace(&entire_subspace_key).range().0
						};

						let mut stream = tx.get_ranges_keyvalues(
							RangeOption {
								mode: StreamingMode::WantAll,
								..(start, end).into()
							},
							Snapshot,
						);

						let mut signals_processed = 0;
						let mut current_signal_id = None;
						let mut name_matches = names.is_empty();
						let mut state_matches = false;
						let mut ts_matches = false;

						let fut = async {
							loop {
								let Some(entry) = stream.try_next().await? else {
									break;
								};

								let signal_id = *self.subspace.unpack::<JustId>(entry.key())?;

								if let Some(curr) = current_signal_id {
									if signal_id != curr {
										// Save if matches query
										if name_matches && state_matches && ts_matches {
											signal_ids.push(curr);

											if let Some(max_per_txn) = max_per_txn {
												if signal_ids.len() >= max_per_txn {
													return anyhow::Ok(());
												}
											}
										}

										signals_processed += 1;

										// Reset state
										name_matches = names.is_empty();
										state_matches = false;
										ts_matches = false;
									}
								}

								current_signal_id = Some(signal_id);

								if let Ok(name_key) =
									self.subspace.unpack::<keys::signal::NameKey>(entry.key())
								{
									let signal_name = name_key.deserialize(entry.value())?;

									name_matches = names.is_empty() || names.iter().any(|name| &signal_name == name);
								} else if let Ok(_) = self
									.subspace
									.unpack::<keys::signal::AckTsKey>(entry.key())
								{
									state_matches = true;
								} else if let Ok(create_ts_key) = self
									.subspace
									.unpack::<keys::signal::CreateTsKey>(entry.key())
								{
									let create_ts = create_ts_key.deserialize(entry.value())?;

									ts_matches = create_ts < before_ts;
								}
							}

							if let (Some(signal_id), true) = (
								current_signal_id,
								name_matches && state_matches && ts_matches
							) {
								signal_ids.push(signal_id);
								current_signal_id = None;
							}

							if current_signal_id.is_some() {
								signals_processed += 1;
							}

							anyhow::Ok(())
						};

						match tokio::time::timeout(EARLY_TXN_TIMEOUT, fut).await {
							Ok(res) => res?,
							Err(_) => tracing::debug!("timed out reading signals"),
						}

						tracing::info!(?signals_processed, matching_signals=?signal_ids.len(), "batch processed signals");

						Ok((current_signal_id, signal_ids))
					}
				})
				.instrument(tracing::debug_span!("find_acked_signals_tx"))
				.await?;

			current_signal_id = new_current_signal_id;
			total += signal_ids.len();

			if !dry_run {
				let signal_ids = &signal_ids;
				self.pools
					.udb()?
					.txn("gas_debug_delete_signals", |tx| async move {
						tx.tag("prune_signals")?;

						let tx = tx.with_subspace(self.subspace.clone());

						for signal_id in signal_ids {
							let data_subspace =
								keys::signal::DataSubspaceKey::new_with_signal_id(*signal_id);

							tx.delete_key_subspace(&data_subspace);
						}

						Ok(())
					})
					.await?;
			}

			if current_signal_id.is_none() {
				tracing::info!("reached end of signals");
				break;
			}
		}

		Ok(total)
	}
}

// Parses Id in third position, ignores the rest
pub(crate) struct JustId(Id);

impl<'de> TupleUnpack<'de> for JustId {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (_, _, id)) = <(usize, usize, Id)>::unpack(input, tuple_depth)?;
		let v = JustId(id);

		Ok((&input[0..0], v))
	}
}

impl Deref for JustId {
	type Target = Id;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

struct WorkflowHistoryEventBuilder {
	location: Location,
	event_type: Option<EventType>,
	version: Option<usize>,
	create_ts: Option<i64>,
	forgotten: bool,
	name: Option<String>,
	signal_id: Option<Id>,
	sub_workflow_id: Option<Id>,
	input_chunks: Vec<Value>,
	output_chunks: Vec<Value>,
	tags: Vec<(String, String)>,
	errors: Vec<ActivityError>,
	iteration: Option<usize>,
	deadline_ts: Option<i64>,
	sleep_state: Option<SleepState>,
	inner_event_type: Option<EventType>,
	inner_version: Option<usize>,

	indexed_signal_ids: Vec<Id>,
	indexed_names: Vec<String>,
	indexed_input_chunks: Vec<Vec<Value>>,
}

impl WorkflowHistoryEventBuilder {
	fn new(location: Location, forgotten: bool) -> Self {
		WorkflowHistoryEventBuilder {
			location,
			event_type: None,
			version: None,
			create_ts: None,
			forgotten,
			name: None,
			signal_id: None,
			sub_workflow_id: None,
			input_chunks: Vec::new(),
			output_chunks: Vec::new(),
			tags: Vec::new(),
			errors: Vec::new(),
			iteration: None,
			deadline_ts: None,
			sleep_state: None,
			inner_event_type: None,
			inner_version: None,

			indexed_signal_ids: Vec::new(),
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
			location: value.location.clone(),
			version: value
				.version
				.ok_or(WorkflowError::MissingEventData("version"))?,
			create_ts: value
				.create_ts
				.ok_or(WorkflowError::MissingEventData("create_ts"))?,
			forgotten: value.forgotten,
			data: match event_type {
				EventType::Activity => EventData::Activity(value.try_into()?),
				EventType::Signal => EventData::Signal(value.try_into()?),
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
			input: {
				if value.input_chunks.is_empty() {
					return Err(WorkflowError::MissingEventData("input"));
				} else {
					// workflow_id not needed
					let input_key = keys::history::InputKey::new(Id::nil(), value.location.clone());
					let v = input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					serde_json::from_str(v.get())
						.map_err(Into::into)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
			output: {
				if value.output_chunks.is_empty() {
					None
				} else {
					// workflow_id not needed
					let output_key =
						keys::history::OutputKey::new(Id::nil(), value.location.clone());
					let v = output_key
						.combine(value.output_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					Some(
						serde_json::from_str(v.get())
							.map_err(Into::into)
							.map_err(WorkflowError::DeserializeEventData)?,
					)
				}
			},
			errors: value.errors,
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for SignalEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(SignalEvent {
			signal_id: value
				.signal_id
				.ok_or(WorkflowError::MissingEventData("signal_id"))?,
			name: value.name.ok_or(WorkflowError::MissingEventData("name"))?,
			body: {
				if value.input_chunks.is_empty() {
					return Err(WorkflowError::MissingEventData("input"));
				} else {
					// workflow_id not needed
					let input_key = keys::history::InputKey::new(Id::nil(), value.location);
					let v = input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					serde_json::from_str(v.get())
						.map_err(Into::into)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
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
			workflow_id: value.sub_workflow_id,
			tags: None,
			body: {
				if value.input_chunks.is_empty() {
					return Err(WorkflowError::MissingEventData("input"));
				} else {
					// workflow_id not needed
					let input_key = keys::history::InputKey::new(Id::nil(), value.location);
					let v = input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					serde_json::from_str(v.get())
						.map_err(Into::into)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
		})
	}
}

impl TryFrom<WorkflowHistoryEventBuilder> for MessageSendEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(MessageSendEvent {
			name: value.name.ok_or(WorkflowError::MissingEventData("name"))?,
			tags: serde_json::Value::Object(
				value
					.tags
					.into_iter()
					.map(|(k, v)| (k, serde_json::Value::String(v)))
					.collect(),
			),
			body: {
				if value.input_chunks.is_empty() {
					return Err(WorkflowError::MissingEventData("input"));
				} else {
					// workflow_id not needed
					let input_key = keys::history::InputKey::new(Id::nil(), value.location);
					let v = input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					serde_json::from_str(v.get())
						.map_err(Into::into)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
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
			tags: serde_json::Value::Object(
				value
					.tags
					.into_iter()
					.map(|(k, v)| (k, serde_json::Value::String(v)))
					.collect(),
			),
			input: {
				if value.input_chunks.is_empty() {
					return Err(WorkflowError::MissingEventData("input"));
				} else {
					// workflow_id not needed
					let input_key = keys::history::InputKey::new(Id::nil(), value.location);
					let v = input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					serde_json::from_str(v.get())
						.map_err(Into::into)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
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
					let v = input_key
						.combine(value.input_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					serde_json::from_str(v.get())
						.map_err(Into::into)
						.map_err(WorkflowError::DeserializeEventData)?
				}
			},
			output: {
				if value.output_chunks.is_empty() {
					None
				} else {
					// workflow_id not needed
					let output_key =
						keys::history::OutputKey::new(Id::nil(), value.location.clone());
					let v = output_key
						.combine(value.output_chunks)
						.map_err(WorkflowError::DeserializeEventData)?;

					Some(
						serde_json::from_str(v.get())
							.map_err(Into::into)
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

impl TryFrom<WorkflowHistoryEventBuilder> for SignalsEvent {
	type Error = WorkflowError;

	fn try_from(value: WorkflowHistoryEventBuilder) -> WorkflowResult<Self> {
		Ok(SignalsEvent {
			signal_ids: if value.indexed_signal_ids.is_empty() {
				return Err(WorkflowError::MissingEventData("signal_id"));
			} else {
				value.indexed_signal_ids
			},
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
						let v = input_key
							.combine(input_chunks)
							.map_err(WorkflowError::DeserializeEventData)?;

						serde_json::from_str::<serde_json::Value>(v.get())
							.map_err(Into::into)
							.map_err(WorkflowError::DeserializeEventData)
					})
					.collect::<std::result::Result<_, _>>()?
			},
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
