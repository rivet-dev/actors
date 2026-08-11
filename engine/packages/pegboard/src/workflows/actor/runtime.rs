// runner wf see how signal fail handling
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use gas::prelude::*;
use rand::prelude::SliceRandom;
use rivet_runner_protocol::{
	self as protocol, PROTOCOL_MK1_VERSION, PROTOCOL_MK2_VERSION, versioned,
};
use rivet_types::actors::CrashPolicy;
use rivet_types::runner_configs::RunnerConfigKind;
use std::time::Instant;
use universaldb::prelude::*;
use universalpubsub::PublishOpts;
use vbare::OwnedVersionedData;

use super::FailureReason;

use crate::{keys, metrics};

use super::{Allocate, Destroy, Input, PendingAllocation, State, destroy};

#[derive(Debug, Deserialize, Serialize)]
pub struct LifecycleRunnerState {
	pub last_event_idx: i64,
	pub last_event_ack_idx: i64,
}

impl Default for LifecycleRunnerState {
	fn default() -> Self {
		LifecycleRunnerState {
			last_event_idx: -1,
			last_event_ack_idx: -1,
		}
	}
}

// TODO: Rewrite this as a series of nested structs/enums for better transparency of current state (likely
// requires actor wf v2)
#[derive(Deserialize, Serialize)]
pub struct LifecycleState {
	pub generation: u32,

	// Set when currently running (not rescheduling or sleeping)
	pub runner_id: Option<Id>,
	pub runner_workflow_id: Option<Id>,
	pub runner_protocol_version: Option<u16>,
	pub runner_state: Option<LifecycleRunnerState>,

	pub sleeping: bool,
	#[serde(default)]
	pub stopping: bool,
	#[serde(default)]
	pub going_away: bool,

	/// If a wake was received in between an actor's intent to sleep and actor stop.
	#[serde(default)]
	pub will_wake: bool,
	pub alarm_ts: Option<i64>,
	/// Handles cleaning up the actor if it does not receive a certain state before the timeout (ex.
	/// created -> running event, stop intent -> stop event). If the timeout is reached, the actor is
	/// considered lost.
	pub gc_timeout_ts: Option<i64>,

	pub reschedule_state: RescheduleState,
}

impl LifecycleState {
	pub fn new(
		runner_id: Id,
		runner_workflow_id: Id,
		runner_protocol_version: u16,
		actor_start_threshold: i64,
	) -> Self {
		LifecycleState {
			generation: 0,
			runner_id: Some(runner_id),
			runner_workflow_id: Some(runner_workflow_id),
			runner_protocol_version: Some(runner_protocol_version),
			runner_state: Some(LifecycleRunnerState::default()),
			sleeping: false,
			stopping: false,
			going_away: false,
			will_wake: false,
			alarm_ts: None,
			gc_timeout_ts: Some(util::timestamp::now() + actor_start_threshold),
			reschedule_state: RescheduleState::default(),
		}
	}

	pub fn new_sleeping() -> Self {
		LifecycleState {
			generation: 0,
			runner_id: None,
			runner_workflow_id: None,
			runner_protocol_version: None,
			runner_state: None,
			sleeping: true,
			stopping: false,
			going_away: false,
			will_wake: false,
			alarm_ts: None,
			gc_timeout_ts: None,
			reschedule_state: RescheduleState::default(),
		}
	}
}

#[derive(Serialize, Deserialize)]
pub struct LifecycleResult {
	pub generation: u32,
	#[serde(default)]
	pub migrate_to_v2: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub(crate) struct RescheduleState {
	last_retry_ts: i64,
	retry_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateRunnerInput {
	actor_id: Id,
	runner_id: Id,
	runner_workflow_id: Id,
}

// This is called when allocated by an outside source while the actor was pending.
#[activity(UpdateRunner)]
async fn update_runner(ctx: &ActivityCtx, input: &UpdateRunnerInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	state.sleep_ts = None;
	state.pending_allocation_ts = None;
	state.failure_reason = None;
	state.runner_id = Some(input.runner_id);
	state.runner_workflow_id = Some(input.runner_workflow_id);

	ctx.udb()?
		.txn("pegboard_actor_runtime_clear_sleep", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			// Set actor as not sleeping
			tx.delete(&keys::actor::SleepTsKey::new(input.actor_id));

			Ok(())
		})
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct AllocateActorInputV1 {
	actor_id: Id,
	generation: u32,
	force_allocate: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocateActorOutputV1 {
	Allocated {
		runner_id: Id,
		runner_workflow_id: Id,
	},
	Pending {
		pending_allocation_ts: i64,
	},
	Sleep,
}

#[activity(AllocateActor)]
async fn allocate_actor(
	ctx: &ActivityCtx,
	input: &AllocateActorInputV1,
) -> Result<AllocateActorOutputV1> {
	bail!("allocate actor v1 should never be called again")
}

#[derive(Debug, Serialize, Deserialize)]
struct AllocateActorInputV2 {
	actor_id: Id,
	generation: u32,
	force_allocate: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AllocateActorOutputV2 {
	status: AllocateActorStatus,
	serverless: bool,
}

impl From<AllocateActorOutputV1> for AllocateActorOutputV2 {
	fn from(value: AllocateActorOutputV1) -> Self {
		Self {
			serverless: false,
			status: match value {
				AllocateActorOutputV1::Allocated {
					runner_id,
					runner_workflow_id,
				} => AllocateActorStatus::Allocated {
					runner_id,
					runner_workflow_id,
					runner_protocol_version: None,
				},
				AllocateActorOutputV1::Pending {
					pending_allocation_ts,
				} => AllocateActorStatus::Pending {
					pending_allocation_ts,
				},
				AllocateActorOutputV1::Sleep => AllocateActorStatus::Sleep,
			},
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AllocateActorStatus {
	Allocated {
		runner_id: Id,
		runner_workflow_id: Id,
		#[serde(default)]
		runner_protocol_version: Option<u16>,
	},
	Pending {
		pending_allocation_ts: i64,
	},
	Sleep,
	MigrateToV2,
}

// If no availability, returns the timestamp of the actor's queue key
#[activity(AllocateActorV2)]
async fn allocate_actor_v2(
	ctx: &ActivityCtx,
	input: &AllocateActorInputV2,
) -> Result<AllocateActorOutputV2> {
	let start_instant = Instant::now();

	let mut state = ctx.state::<State>()?;
	let namespace_id = state.namespace_id;
	let crash_policy = state.crash_policy;
	let runner_name_selector = &state.runner_name_selector;

	let runner_eligible_threshold = ctx.config().pegboard().runner_eligible_threshold();
	let actor_allocation_candidate_sample_size = ctx
		.config()
		.pegboard()
		.actor_allocation_candidate_sample_size();

	let pool_res = ctx
		.op(crate::ops::runner_config::get::Input {
			runners: vec![(namespace_id, runner_name_selector.clone())],
			bypass_cache: false,
		})
		.await?;
	let pool = pool_res.into_iter().next();
	let for_serverless = pool
		.as_ref()
		.map(|pool| matches!(pool.config.kind, RunnerConfigKind::Serverless { .. }))
		.unwrap_or(false);

	// Protocol version is set or this is a serverless pool
	if pool.and_then(|p| p.protocol_version).is_some() {
		return Ok(AllocateActorOutputV2 {
			status: AllocateActorStatus::MigrateToV2,
			serverless: false,
		});
	}

	// NOTE: This txn should closely resemble the one found in the allocate_pending_actors activity of the
	// client wf
	let res = ctx
		.udb()?
		.txn("pegboard_actor_runtime_allocate_actor", |tx| async move {
			let ping_threshold_ts = util::timestamp::now() - runner_eligible_threshold;

			let tx = tx.with_subspace(keys::subspace());

			// Check if a queue exists
			let pending_actor_subspace = keys::subspace().subspace(
				&keys::ns::PendingActorByRunnerNameSelectorKey::subspace(
					namespace_id,
					runner_name_selector.clone(),
				),
			);

			let mut queue_stream = tx.get_ranges_keyvalues(
				universaldb::RangeOption {
					mode: StreamingMode::Exact,
					limit: Some(1),
					..(&pending_actor_subspace).into()
				},
				// NOTE: This is not Serializable because we don't want to conflict with other
				// inserts/clears to this range
				Snapshot,
			);
			let queue_exists = queue_stream.next().await.is_some();

			if for_serverless {
				tx.atomic_op(
					&rivet_types::keys::pegboard::ns::ServerlessDesiredSlotsKey::new(
						namespace_id,
						runner_name_selector.clone(),
					),
					&1i64.to_le_bytes(),
					MutationType::Add,
				);
			}

			if !queue_exists {
				let runner_alloc_subspace =
					keys::subspace().subspace(&keys::ns::RunnerAllocIdxKey::subspace(
						namespace_id,
						runner_name_selector.clone(),
					));

				let mut stream = tx.get_ranges_keyvalues(
					universaldb::RangeOption {
						mode: StreamingMode::Iterator,
						..(&runner_alloc_subspace).into()
					},
					// NOTE: This is not Serializable because we don't want to conflict with all of the
					// keys, just the one we choose
					Snapshot,
				);

				let mut highest_version = None;
				let mut candidates = Vec::with_capacity(actor_allocation_candidate_sample_size);

				// Select valid runner candidates for allocation
				loop {
					let Some(entry) = stream.try_next().await? else {
						break;
					};

					let (old_runner_alloc_key, old_runner_alloc_key_data) =
						tx.read_entry::<keys::ns::RunnerAllocIdxKey>(&entry)?;

					if let Some(highest_version) = highest_version {
						// We have passed all of the runners with the highest version. This is reachable if
						// the ping of the highest version workers makes them ineligible
						if old_runner_alloc_key.version < highest_version {
							break;
						}
					} else {
						highest_version = Some(old_runner_alloc_key.version);
					}

					// An empty runner means we have reached the end of the runners with the highest version
					if old_runner_alloc_key.remaining_millislots == 0 {
						break;
					}

					// Ignore runners without valid ping
					if old_runner_alloc_key.last_ping_ts < ping_threshold_ts {
						continue;
					}

					candidates.push((old_runner_alloc_key, old_runner_alloc_key_data));

					// Max candidate size reached
					if candidates.len() >= actor_allocation_candidate_sample_size {
						break;
					}
				}

				if !candidates.is_empty() {
					// Select a candidate at random, weighted by remaining slots
					let (old_runner_alloc_key, old_runner_alloc_key_data) = candidates
						.choose_weighted(&mut rand::thread_rng(), |(key, _)| {
							key.remaining_millislots
						})?;

					// Add read conflict only for this key
					tx.add_conflict_key(&old_runner_alloc_key, ConflictRangeType::Read)?;

					// Clear old entry
					tx.delete(&old_runner_alloc_key);

					let new_remaining_slots =
						old_runner_alloc_key_data.remaining_slots.saturating_sub(1);
					let new_remaining_millislots =
						(new_remaining_slots * 1000) / old_runner_alloc_key_data.total_slots;

					// Write new allocation key with 1 less slot
					tx.write(
						&keys::ns::RunnerAllocIdxKey::new(
							namespace_id,
							runner_name_selector.clone(),
							old_runner_alloc_key.version,
							new_remaining_millislots,
							old_runner_alloc_key.last_ping_ts,
							old_runner_alloc_key.runner_id,
						),
						rivet_data::converted::RunnerAllocIdxKeyData {
							workflow_id: old_runner_alloc_key_data.workflow_id,
							remaining_slots: new_remaining_slots,
							total_slots: old_runner_alloc_key_data.total_slots,
							protocol_version: old_runner_alloc_key_data.protocol_version,
						},
					)?;

					// Update runner record
					tx.write(
						&keys::runner::RemainingSlotsKey::new(old_runner_alloc_key.runner_id),
						new_remaining_slots,
					)?;

					// Set runner id of actor
					tx.write(
						&keys::actor::RunnerIdKey::new(input.actor_id),
						old_runner_alloc_key.runner_id,
					)?;

					// Insert actor index key
					tx.write(
						&keys::runner::ActorKey::new(
							old_runner_alloc_key.runner_id,
							input.actor_id,
						),
						input.generation,
					)?;

					// Set actor as not sleeping
					tx.delete(&keys::actor::SleepTsKey::new(input.actor_id));

					return Ok(AllocateActorOutputV2 {
						serverless: for_serverless,
						status: AllocateActorStatus::Allocated {
							runner_id: old_runner_alloc_key.runner_id,
							runner_workflow_id: old_runner_alloc_key_data.workflow_id,
							runner_protocol_version: Some(
								old_runner_alloc_key_data.protocol_version,
							),
						},
					});
				}
			}

			// At this point in the txn there is no availability

			match (crash_policy, input.force_allocate, for_serverless) {
				(CrashPolicy::Sleep, false, false) => Ok(AllocateActorOutputV2 {
					serverless: false,
					status: AllocateActorStatus::Sleep,
				}),
				// Write the actor to the alloc queue to wait
				_ => {
					let pending_allocation_ts = util::timestamp::now();

					// NOTE: This will conflict with serializable reads to the alloc queue, which is the behavior we
					// want. If a runner reads from the queue while this is being inserted, one of the two txns will
					// retry and we ensure the actor does not end up in queue limbo.
					tx.write(
						&keys::ns::PendingActorByRunnerNameSelectorKey::new(
							namespace_id,
							runner_name_selector.clone(),
							pending_allocation_ts,
							input.actor_id,
						),
						input.generation,
					)?;

					Ok(AllocateActorOutputV2 {
						serverless: for_serverless,
						status: AllocateActorStatus::Pending {
							pending_allocation_ts,
						},
					})
				}
			}
		})
		.custom_instrument(tracing::info_span!("actor_allocate_tx"))
		.await?;

	let dt = start_instant.elapsed().as_secs_f64();
	metrics::ACTOR_ALLOCATE_DURATION
		.with_label_values(&[
			if res.serverless {
				"serverless"
			} else {
				"serverful"
			},
			match res.status {
				AllocateActorStatus::Allocated { .. } => "allocated",
				AllocateActorStatus::Pending { .. } => "pending",
				AllocateActorStatus::Sleep { .. } => "sleep",
				AllocateActorStatus::MigrateToV2 => bail!("should not be migrate_to_v2"),
			},
		])
		.observe(dt);

	state.for_serverless = res.serverless;
	state.allocated_serverless_slot = res.serverless;
	state.reschedule_ts = None;

	match &res.status {
		AllocateActorStatus::Allocated {
			runner_id,
			runner_workflow_id,
			..
		} => {
			state.sleep_ts = None;
			state.pending_allocation_ts = None;
			state.failure_reason = None;
			state.runner_id = Some(*runner_id);
			state.runner_workflow_id = Some(*runner_workflow_id);
		}
		AllocateActorStatus::Pending {
			pending_allocation_ts,
			..
		} => {
			tracing::debug!(
				actor_id=?input.actor_id,
				"failed to allocate (no availability), waiting for allocation",
			);

			state.pending_allocation_ts = Some(*pending_allocation_ts);
			if state.failure_reason.is_none() {
				state.failure_reason = Some(super::FailureReason::NoCapacity);
			}
		}
		AllocateActorStatus::Sleep => {
			if state.failure_reason.is_none() {
				state.failure_reason = Some(super::FailureReason::NoCapacity);
			}
		}
		AllocateActorStatus::MigrateToV2 => bail!("should not be migrate_to_v2"),
	}

	Ok(res)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetNotConnectableInput {
	pub actor_id: Id,
}

#[activity(SetNotConnectable)]
pub async fn set_not_connectable(ctx: &ActivityCtx, input: &SetNotConnectableInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	ctx.udb()?
		.txn(
			"pegboard_actor_runtime_set_not_connectable",
			|tx| async move {
				let tx = tx.with_subspace(keys::subspace());

				let connectable_key = keys::actor::ConnectableKey::new(input.actor_id);
				tx.clear(&keys::subspace().pack(&connectable_key));

				Ok(())
			},
		)
		.custom_instrument(tracing::info_span!("actor_set_not_connectable_tx"))
		.await?;

	state.connectable_ts = None;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeallocateInput {
	pub actor_id: Id,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeallocateOutput {
	pub for_serverless: bool,
}

#[activity(Deallocate)]
pub async fn deallocate(ctx: &ActivityCtx, input: &DeallocateInput) -> Result<DeallocateOutput> {
	let mut state = ctx.state::<State>()?;
	let namespace_id = state.namespace_id;
	let runner_name_selector = &state.runner_name_selector;
	let runner_id = state.runner_id;
	let allocated_serverless_slot = state.allocated_serverless_slot;

	ctx.udb()?
		.txn("pegboard_actor_runtime_deallocate", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			tx.delete(&keys::actor::ConnectableKey::new(input.actor_id));

			destroy::clear_slot(
				input.actor_id,
				namespace_id,
				runner_name_selector,
				runner_id,
				allocated_serverless_slot,
				&tx,
			)
			.await?;

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_deallocate_tx"))
		.await?;

	state.connectable_ts = None;
	state.runner_id = None;
	state.runner_workflow_id = None;
	state.runner_state = None;
	// Slot was cleared by the above txn
	state.allocated_serverless_slot = false;

	Ok(DeallocateOutput {
		for_serverless: state.for_serverless,
	})
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AllocationOverride {
	#[default]
	None,
	/// Forces actors with CrashPolicy::Sleep to pend instead of sleep.
	DontSleep { pending_timeout: Option<i64> },
	/// If an allocation results in pending, it will be put to sleep if it is not allocated after this
	/// timeout.
	PendingTimeout { pending_timeout: i64 },
}

#[derive(Debug)]
pub enum SpawnActorOutput {
	Allocated {
		runner_id: Id,
		runner_workflow_id: Id,
		runner_protocol_version: u16,
	},
	Sleep,
	Destroy,
	MigrateToV2,
}

/// Wrapper around `allocate_actor` that handles pending state.
pub async fn spawn_actor(
	ctx: &mut WorkflowCtx,
	input: &Input,
	generation: u32,
	allocation_override: AllocationOverride,
) -> Result<SpawnActorOutput> {
	// Attempt allocation
	let allocate_res: AllocateActorOutputV2 = match ctx.check_version(2).await? {
		1 => ctx
			.activity(AllocateActorInputV1 {
				actor_id: input.actor_id,
				generation,
				force_allocate: matches!(
					&allocation_override,
					AllocationOverride::DontSleep { .. }
				),
			})
			.await?
			.into(),
		_latest => {
			ctx.v(2)
				.activity(AllocateActorInputV2 {
					actor_id: input.actor_id,
					generation,
					force_allocate: matches!(
						&allocation_override,
						AllocationOverride::DontSleep { .. }
					),
				})
				.await?
		}
	};

	match allocate_res.status {
		AllocateActorStatus::Allocated {
			runner_id,
			runner_workflow_id,
			runner_protocol_version,
		} => {
			let runner_protocol_version = runner_protocol_version.unwrap_or(PROTOCOL_MK1_VERSION);

			ctx.removed::<Message<super::BumpServerlessAutoscalerStub>>()
				.await?;

			// Bump the pool so it can scale up
			if allocate_res.serverless {
				let res = ctx
					.v(2)
					.signal(crate::workflows::runner_pool::Bump::default())
					.to_workflow::<crate::workflows::runner_pool::Workflow>()
					.tag("namespace_id", input.namespace_id)
					.tag("runner_name", input.runner_name_selector.clone())
					.send()
					.await;

				if let Some(WorkflowError::WorkflowNotFound) = res
					.as_ref()
					.err()
					.and_then(|x| x.chain().find_map(|x| x.downcast_ref::<WorkflowError>()))
				{
					tracing::warn!(
						namespace_id=%input.namespace_id,
						runner_name=%input.runner_name_selector,
						"serverless pool workflow not found, respective runner config likely deleted"
					);
				} else {
					res?;
				}
			}

			if protocol::is_mk2(runner_protocol_version) {
				ctx.activity(InsertAndSendCommandsInput {
					actor_id: input.actor_id,
					generation,
					runner_id,
					commands: vec![protocol::mk2::Command::CommandStartActor(
						protocol::mk2::CommandStartActor {
							config: protocol::mk2::ActorConfig {
								name: input.name.clone(),
								key: input.key.clone(),
								// HACK: We should not use dynamic timestamp here, but we don't validate if signal data
								// changes (like activity inputs) so this is fine for now.
								create_ts: util::timestamp::now(),
								input: input
									.input
									.as_ref()
									.and_then(|x| BASE64_STANDARD.decode(x).ok()),
							},
							// Empty because request ids are ephemeral. This is intercepted by guard and
							// populated before it reaches the runner
							hibernating_requests: Vec::new(),
						},
					)],
				})
				.await?;
			} else {
				ctx.signal(crate::workflows::runner::Command {
					inner: protocol::Command::CommandStartActor(protocol::CommandStartActor {
						actor_id: input.actor_id.to_string(),
						generation,
						config: protocol::ActorConfig {
							name: input.name.clone(),
							key: input.key.clone(),
							// HACK: We should not use dynamic timestamp here, but we don't validate if signal data
							// changes (like activity inputs) so this is fine for now.
							create_ts: util::timestamp::now(),
							input: input
								.input
								.as_ref()
								.map(|x| BASE64_STANDARD.decode(x))
								.transpose()?,
						},
						// Empty because request ids are ephemeral. This is intercepted by guard and
						// populated before it reaches the runner
						hibernating_requests: Vec::new(),
					}),
				})
				.to_workflow_id(runner_workflow_id)
				.send()
				.await?;
			}

			Ok(SpawnActorOutput::Allocated {
				runner_id,
				runner_workflow_id,
				runner_protocol_version,
			})
		}
		AllocateActorStatus::Pending {
			pending_allocation_ts,
		} => {
			ctx.removed::<Message<super::BumpServerlessAutoscalerStub>>()
				.await?;

			// Bump the pool so it can scale up
			if allocate_res.serverless {
				let res = ctx
					.v(2)
					.signal(crate::workflows::runner_pool::Bump::default())
					.to_workflow::<crate::workflows::runner_pool::Workflow>()
					.tag("namespace_id", input.namespace_id)
					.tag("runner_name", input.runner_name_selector.clone())
					.send()
					.await;

				if let Some(WorkflowError::WorkflowNotFound) = res
					.as_ref()
					.err()
					.and_then(|x| x.chain().find_map(|x| x.downcast_ref::<WorkflowError>()))
				{
					tracing::warn!(
						namespace_id=%input.namespace_id,
						runner_name=%input.runner_name_selector,
						"serverless pool workflow not found, respective runner config likely deleted"
					);
				} else {
					res?;
				}
			}

			let signal = match allocation_override {
				AllocationOverride::DontSleep {
					pending_timeout: Some(timeout),
				}
				| AllocationOverride::PendingTimeout {
					pending_timeout: timeout,
				} => {
					ctx.listen_with_timeout::<PendingAllocation>(timeout)
						.await?
				}
				_ => Some(ctx.listen::<PendingAllocation>().await?),
			};

			// If allocation fails, the allocate txn already inserted this actor into the queue. Now we wait for
			// an `Allocate` signal
			match signal {
				Some(PendingAllocation::Allocate(sig)) => {
					let runner_protocol_version =
						sig.runner_protocol_version.unwrap_or(PROTOCOL_MK1_VERSION);

					ctx.activity(UpdateRunnerInput {
						actor_id: input.actor_id,
						runner_id: sig.runner_id,
						runner_workflow_id: sig.runner_workflow_id,
					})
					.await?;

					if protocol::is_mk2(runner_protocol_version) {
						ctx.activity(InsertAndSendCommandsInput {
							actor_id: input.actor_id,
							generation,
							runner_id: sig.runner_id,
							commands: vec![protocol::mk2::Command::CommandStartActor(
								protocol::mk2::CommandStartActor {
									config: protocol::mk2::ActorConfig {
										name: input.name.clone(),
										key: input.key.clone(),
										create_ts: util::timestamp::now(),
										input: input
											.input
											.as_ref()
											.map(|x| BASE64_STANDARD.decode(x))
											.transpose()?,
									},
									// Empty because request ids are ephemeral. This is intercepted by guard and
									// populated before it reaches the runner
									hibernating_requests: Vec::new(),
								},
							)],
						})
						.await?;
					} else {
						ctx.signal(crate::workflows::runner::Command {
							inner: protocol::Command::CommandStartActor(
								protocol::CommandStartActor {
									actor_id: input.actor_id.to_string(),
									generation,
									config: protocol::ActorConfig {
										name: input.name.clone(),
										key: input.key.clone(),
										create_ts: util::timestamp::now(),
										input: input
											.input
											.as_ref()
											.map(|x| BASE64_STANDARD.decode(x))
											.transpose()?,
									},
									// Empty because request ids are ephemeral. This is intercepted by guard and
									// populated before it reaches the runner
									hibernating_requests: Vec::new(),
								},
							),
						})
						.to_workflow_id(sig.runner_workflow_id)
						.send()
						.await?;
					}

					Ok(SpawnActorOutput::Allocated {
						runner_id: sig.runner_id,
						runner_workflow_id: sig.runner_workflow_id,
						runner_protocol_version,
					})
				}
				Some(PendingAllocation::Destroy(_)) => {
					tracing::debug!(actor_id=?input.actor_id, "destroying before actor allocated");

					let cleared = ctx
						.activity(ClearPendingAllocationInput {
							actor_id: input.actor_id,
							namespace_id: input.namespace_id,
							runner_name_selector: input.runner_name_selector.clone(),
							pending_allocation_ts,
						})
						.await?;

					// If this actor was no longer present in the queue it means it was allocated. We must now
					// wait for the allocated signal to prevent a race condition.
					if !cleared {
						let sig = ctx.listen::<Allocate>().await?;

						ctx.activity(UpdateRunnerInput {
							actor_id: input.actor_id,
							runner_id: sig.runner_id,
							runner_workflow_id: sig.runner_workflow_id,
						})
						.await?;
					}
					// Bump the pool so it can scale down
					else if allocate_res.serverless {
						let res = ctx
							.v(2)
							.signal(crate::workflows::runner_pool::Bump::default())
							.to_workflow::<crate::workflows::runner_pool::Workflow>()
							.tag("namespace_id", input.namespace_id)
							.tag("runner_name", input.runner_name_selector.clone())
							.send()
							.await;

						if let Some(WorkflowError::WorkflowNotFound) = res
							.as_ref()
							.err()
							.and_then(|x| x.chain().find_map(|x| x.downcast_ref::<WorkflowError>()))
						{
							tracing::warn!(
								namespace_id=%input.namespace_id,
								runner_name=%input.runner_name_selector,
								"serverless pool workflow not found, respective runner config likely deleted"
							);
						} else {
							res?;
						}
					}

					Ok(SpawnActorOutput::Destroy)
				}
				None => {
					tracing::debug!(actor_id=?input.actor_id, "timed out before actor allocated");

					let cleared = ctx
						.activity(ClearPendingAllocationInput {
							actor_id: input.actor_id,
							namespace_id: input.namespace_id,
							runner_name_selector: input.runner_name_selector.clone(),
							pending_allocation_ts,
						})
						.await?;

					// If this actor was no longer present in the queue it means it was allocated. We must now
					// wait for the allocated signal to prevent a race condition.
					if !cleared {
						let sig = ctx.listen::<Allocate>().await?;
						let runner_protocol_version =
							sig.runner_protocol_version.unwrap_or(PROTOCOL_MK1_VERSION);

						ctx.activity(UpdateRunnerInput {
							actor_id: input.actor_id,
							runner_id: sig.runner_id,
							runner_workflow_id: sig.runner_workflow_id,
						})
						.await?;

						if protocol::is_mk2(runner_protocol_version) {
							ctx.activity(InsertAndSendCommandsInput {
								actor_id: input.actor_id,
								generation,
								runner_id: sig.runner_id,
								commands: vec![protocol::mk2::Command::CommandStartActor(
									protocol::mk2::CommandStartActor {
										config: protocol::mk2::ActorConfig {
											name: input.name.clone(),
											key: input.key.clone(),
											create_ts: util::timestamp::now(),
											input: input
												.input
												.as_ref()
												.map(|x| BASE64_STANDARD.decode(x))
												.transpose()?,
										},
										// Empty because request ids are ephemeral. This is intercepted by guard and
										// populated before it reaches the runner
										hibernating_requests: Vec::new(),
									},
								)],
							})
							.await?;
						} else {
							ctx.signal(crate::workflows::runner::Command {
								inner: protocol::Command::CommandStartActor(
									protocol::CommandStartActor {
										actor_id: input.actor_id.to_string(),
										generation,
										config: protocol::ActorConfig {
											name: input.name.clone(),
											key: input.key.clone(),
											create_ts: util::timestamp::now(),
											input: input
												.input
												.as_ref()
												.map(|x| BASE64_STANDARD.decode(x))
												.transpose()?,
										},
										// Empty because request ids are ephemeral. This is intercepted by guard and
										// populated before it reaches the runner
										hibernating_requests: Vec::new(),
									},
								),
							})
							.to_workflow_id(sig.runner_workflow_id)
							.send()
							.await?;
						}

						Ok(SpawnActorOutput::Allocated {
							runner_id: sig.runner_id,
							runner_workflow_id: sig.runner_workflow_id,
							runner_protocol_version,
						})
					} else {
						// Bump the pool so it can scale down
						if allocate_res.serverless {
							let res = ctx
								.v(2)
								.signal(crate::workflows::runner_pool::Bump::default())
								.to_workflow::<crate::workflows::runner_pool::Workflow>()
								.tag("namespace_id", input.namespace_id)
								.tag("runner_name", input.runner_name_selector.clone())
								.send()
								.await;

							if let Some(WorkflowError::WorkflowNotFound) =
								res.as_ref().err().and_then(|x| {
									x.chain().find_map(|x| x.downcast_ref::<WorkflowError>())
								}) {
								tracing::warn!(
									namespace_id=%input.namespace_id,
									runner_name=%input.runner_name_selector,
									"serverless pool workflow not found, respective runner config likely deleted"
								);
							} else {
								res?;
							}
						}

						Ok(SpawnActorOutput::Sleep)
					}
				}
			}
		}
		AllocateActorStatus::Sleep => Ok(SpawnActorOutput::Sleep),
		AllocateActorStatus::MigrateToV2 => Ok(SpawnActorOutput::MigrateToV2),
	}
}

/// Wrapper around `spawn_actor` that handles rescheduling retries. Returns true if the actor should be
/// destroyed.
pub async fn reschedule_actor(
	ctx: &mut WorkflowCtx,
	input: &Input,
	state: &mut LifecycleState,
	metrics_workflow_id: Id,
	allocation_override: AllocationOverride,
) -> Result<SpawnActorOutput> {
	tracing::debug!(actor_id=?input.actor_id, "rescheduling actor");

	// Determine next backoff sleep duration
	let mut backoff = reschedule_backoff(
		state.reschedule_state.retry_count,
		ctx.config().pegboard().base_retry_timeout(),
		ctx.config().pegboard().reschedule_backoff_max_exponent(),
	);

	let (now, reset) = ctx
		.v(2)
		.activity(CompareRetryInput {
			retry_count: state.reschedule_state.retry_count,
			last_retry_ts: state.reschedule_state.last_retry_ts,
		})
		.await?;

	state.reschedule_state.retry_count = if reset {
		0
	} else {
		state.reschedule_state.retry_count + 1
	};
	state.reschedule_state.last_retry_ts = now;

	// Don't sleep for first retry
	if state.reschedule_state.retry_count > 0 {
		let next = backoff.step().expect("should not have max retry");

		// Sleep for backoff or destroy early
		if let Some(_sig) = ctx
			.listen_with_timeout::<Destroy>(Instant::from(next) - Instant::now())
			.await?
		{
			tracing::debug!("destroying before actor start");

			return Ok(SpawnActorOutput::Destroy);
		}
	}

	let next_generation = state.generation + 1;
	let spawn_res = spawn_actor(ctx, &input, next_generation, allocation_override).await?;

	if let SpawnActorOutput::Allocated {
		runner_id,
		runner_workflow_id,
		runner_protocol_version,
	} = &spawn_res
	{
		state.generation = next_generation;
		state.runner_id = Some(*runner_id);
		state.runner_workflow_id = Some(*runner_workflow_id);
		state.runner_protocol_version = Some(*runner_protocol_version);

		// Reset gc timeout once allocated
		state.gc_timeout_ts =
			Some(util::timestamp::now() + ctx.config().pegboard().actor_start_threshold());

		// Resume periodic metrics workflow
		ctx.v(2)
			.signal(super::metrics::Resume {
				ts: util::timestamp::now(),
			})
			.to_workflow_id(metrics_workflow_id)
			.send()
			.await?;
	}

	Ok(spawn_res)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClearPendingAllocationInput {
	actor_id: Id,
	namespace_id: Id,
	runner_name_selector: String,
	pending_allocation_ts: i64,
}

#[activity(ClearPendingAllocation)]
pub async fn clear_pending_allocation(
	ctx: &ActivityCtx,
	input: &ClearPendingAllocationInput,
) -> Result<bool> {
	let mut state = ctx.state::<State>()?;

	let allocated_serverless_slot = state.allocated_serverless_slot;

	// Clear self from alloc queue
	let cleared = ctx
		.udb()?
		.txn(
			"pegboard_actor_runtime_clear_alloc_queue",
			|tx| async move {
				let tx = tx.with_subspace(keys::subspace());

				let pending_alloc_key = keys::ns::PendingActorByRunnerNameSelectorKey::new(
					input.namespace_id,
					input.runner_name_selector.clone(),
					input.pending_allocation_ts,
					input.actor_id,
				);
				let exists = tx.exists(&pending_alloc_key, Serializable).await?;

				if exists {
					tx.delete(&pending_alloc_key);

					// If the pending actor key still exists, we must clear its desired slot because after this
					// activity the actor will go to sleep or be destroyed. We don't clear the slot if the key
					// doesn't exist because the actor may either be allocated or destroyed.
					if allocated_serverless_slot {
						tx.atomic_op(
							&rivet_types::keys::pegboard::ns::ServerlessDesiredSlotsKey::new(
								input.namespace_id,
								input.runner_name_selector.clone(),
							),
							&(-1i64).to_le_bytes(),
							MutationType::Add,
						);
					}
				}

				Ok(exists)
			},
		)
		.custom_instrument(tracing::info_span!("actor_clear_pending_alloc_tx"))
		.await?;

	// Only mark allocated_serverless_slot as false if it was allocated before and cleared now
	if allocated_serverless_slot && cleared {
		state.allocated_serverless_slot = false;
	}

	Ok(cleared)
}

#[derive(Debug, Serialize, Deserialize)]
struct CompareRetryInput {
	#[serde(default)]
	retry_count: usize,
	last_retry_ts: i64,
}

#[activity(CompareRetry)]
async fn compare_retry(ctx: &ActivityCtx, input: &CompareRetryInput) -> Result<(i64, bool)> {
	let mut state = ctx.state::<State>()?;

	let now = util::timestamp::now();

	// If the last retry ts is more than RETRY_RESET_DURATION_MS ago, reset retry count
	let reset = input.last_retry_ts < now - ctx.config().pegboard().retry_reset_duration();

	if reset {
		state.reschedule_ts = None;
	} else {
		let backoff = reschedule_backoff(
			input.retry_count,
			ctx.config().pegboard().base_retry_timeout(),
			ctx.config().pegboard().reschedule_backoff_max_exponent(),
		);
		state.reschedule_ts = Some(now + i64::try_from(backoff.current_duration())?);
	}

	Ok((now, reset))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetStartedInput {
	pub actor_id: Id,
}

#[activity(SetStarted)]
pub async fn set_started(ctx: &ActivityCtx, input: &SetStartedInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;
	let now = util::timestamp::now();

	if state.start_ts.is_none() {
		state.start_ts = Some(now);
	}
	state.connectable_ts = Some(now);

	ctx.udb()?
		.txn("pegboard_actor_runtime_set_started", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			let connectable_key = keys::actor::ConnectableKey::new(input.actor_id);
			tx.set(
				&keys::subspace().pack(&connectable_key),
				&connectable_key.serialize(())?,
			);

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_set_started_tx"))
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetSleepingInput {
	pub actor_id: Id,
}

#[activity(SetSleeping)]
pub async fn set_sleeping(ctx: &ActivityCtx, input: &SetSleepingInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;
	let now = util::timestamp::now();

	state.sleep_ts = Some(now);
	state.connectable_ts = None;

	ctx.udb()?
		.txn("pegboard_actor_runtime_set_sleeping", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			// Make not connectable
			tx.delete(&keys::actor::ConnectableKey::new(input.actor_id));
			tx.write(&keys::actor::SleepTsKey::new(input.actor_id), now)?;

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_set_sleeping_tx"))
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetCompleteInput {}

#[activity(SetComplete)]
pub async fn set_complete(ctx: &ActivityCtx, input: &SetCompleteInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	state.complete_ts = Some(util::timestamp::now());

	Ok(())
}

fn reschedule_backoff(
	retry_count: usize,
	base_retry_timeout: usize,
	max_exponent: usize,
) -> util::throttle::Backoff {
	util::throttle::Backoff::new_at(max_exponent, None, base_retry_timeout, 500, retry_count)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsertAndSendCommandsInput {
	pub actor_id: Id,
	pub generation: u32,
	pub runner_id: Id,
	pub commands: Vec<protocol::mk2::Command>,
}

#[activity(InsertAndSendCommands)]
pub async fn insert_and_send_commands(
	ctx: &ActivityCtx,
	input: &InsertAndSendCommandsInput,
) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	let runner_state = state.runner_state.get_or_insert_default();
	let old_last_command_idx = runner_state.last_command_idx;
	runner_state.last_command_idx += input.commands.len() as i64;

	// This does not have to be part of its own activity because the txn is idempotent
	let last_command_idx = runner_state.last_command_idx;
	ctx.udb()?
		.txn(
			"pegboard_actor_runtime_insert_and_send_commands",
			|tx| async move {
				let tx = tx.with_subspace(keys::subspace());

				tx.write(
					&keys::runner::ActorLastCommandIdxKey::new(
						input.runner_id,
						input.actor_id,
						input.generation,
					),
					last_command_idx,
				)?;

				for (i, command) in input.commands.iter().enumerate() {
					tx.write(
						&keys::runner::ActorCommandKey::new(
							input.runner_id,
							input.actor_id,
							input.generation,
							old_last_command_idx + i as i64 + 1,
						),
						match command {
							protocol::mk2::Command::CommandStartActor(x) => {
								protocol::mk2::ActorCommandKeyData::CommandStartActor(x.clone())
							}
							protocol::mk2::Command::CommandStopActor => {
								protocol::mk2::ActorCommandKeyData::CommandStopActor
							}
						},
					)?;
				}

				Ok(())
			},
		)
		.await?;

	let receiver_subject = crate::pubsub_subjects::RunnerReceiverSubject::new(input.runner_id);

	let message_serialized =
		versioned::ToRunnerMk2::wrap_latest(protocol::mk2::ToRunner::ToClientCommands(
			input
				.commands
				.iter()
				.enumerate()
				.map(|(i, command)| protocol::mk2::CommandWrapper {
					checkpoint: protocol::mk2::ActorCheckpoint {
						actor_id: input.actor_id.to_string(),
						generation: input.generation,
						index: old_last_command_idx + i as i64 + 1,
					},
					inner: command.clone(),
				})
				.collect(),
		))
		.serialize_with_embedded_version(PROTOCOL_MK2_VERSION)?;

	ctx.ups()?
		.publish(&receiver_subject, &message_serialized, PublishOpts::one())
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendMessagesToRunnerInput {
	pub runner_id: Id,
	pub messages: Vec<protocol::mk2::ToRunner>,
}

#[activity(SendMessagesToRunner)]
pub async fn send_messages_to_runner(
	ctx: &ActivityCtx,
	input: &SendMessagesToRunnerInput,
) -> Result<()> {
	let receiver_subject = crate::pubsub_subjects::RunnerReceiverSubject::new(input.runner_id);

	for message in &input.messages {
		let message_serialized = versioned::ToRunnerMk2::wrap_latest(message.clone())
			.serialize_with_embedded_version(PROTOCOL_MK2_VERSION)?;

		ctx.ups()?
			.publish(&receiver_subject, &message_serialized, PublishOpts::one())
			.await?;
	}

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckRunnersStubInput {}

#[activity(CheckRunnersStub)]
pub async fn check_runners(ctx: &ActivityCtx, input: &CheckRunnersStubInput) -> Result<()> {
	unreachable!();
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetFailureReasonInput {
	pub failure_reason: FailureReason,
}

/// Sets the failure reason on the actor workflow state.
#[activity(SetFailureReason)]
pub async fn set_failure_reason(ctx: &ActivityCtx, input: &SetFailureReasonInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	// Runner-related errors are never overwritten, as they represent the root cause of the failure
	// and should not be masked by subsequent errors like `Crashed`.
	if let Some(existing) = &state.failure_reason
		&& existing.is_runner_failure()
	{
		tracing::debug!(
			?existing,
			new=?input.failure_reason,
			"preserving existing runner failure error"
		);
		return Ok(());
	}

	state.failure_reason = Some(input.failure_reason.clone());
	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordEventMetricsInput {
	pub namespace_id: Id,
	pub name: String,
	pub alarms_set: usize,
}

#[activity(RecordEventMetrics)]
pub async fn record_event_metrics(
	ctx: &ActivityCtx,
	input: &RecordEventMetricsInput,
) -> Result<()> {
	ctx.udb()?
		.txn(
			"pegboard_actor_runtime_record_event_metrics",
			|tx| async move {
				let tx = tx.with_subspace(keys::subspace());

				namespace::keys::metric::inc(
					&tx.with_subspace(namespace::keys::subspace()),
					input.namespace_id,
					namespace::keys::metric::Metric::AlarmsSet(input.name.clone()),
					input.alarms_set as i64,
				);

				Ok(())
			},
		)
		.await?;

	Ok(())
}
