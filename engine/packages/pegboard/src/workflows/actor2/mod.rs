use futures_util::FutureExt;
use gas::prelude::*;
use rivet_data::converted::ActorByKeyKeyData;
use rivet_envoy_protocol as protocol;
use universaldb::prelude::*;

use crate::errors;

mod alloc_serverful;
mod keys;
pub mod metrics;
mod runtime;
mod sqlite;

use runtime::{StoppedResult, Transition};

/// Batch size of how many events to ack.
const EVENT_ACK_BATCH_SIZE: i64 = 250;

// NOTE: Assumes input is validated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
	pub actor_id: Id,
	pub name: String,
	pub pool_name: String,
	pub key: Option<String>,

	pub namespace_id: Id,

	/// Arbitrary user-provided binary data encoded in base64.
	pub input: Option<String>,
	pub from_v1: bool,
}

#[derive(Deserialize, Serialize)]
pub struct State {
	pub actor_id: Id,
	pub name: String,
	pub pool_name: String,
	pub key: Option<String>,
	pub namespace_id: Id,

	pub acquired_slot: bool,
	pub envoy_last_command_idx: i64,
	// Used as a cache
	pub envoy_key: Option<String>,

	pub create_ts: i64,
	pub create_complete_ts: Option<i64>,
	pub allocate_ts: Option<i64>,
	/// Set only on first allocation.
	pub start_ts: Option<i64>,
	// NOTE: This is not the alarm ts, this is when the actor started sleeping. See `LifecycleState` for alarm
	pub sleep_ts: Option<i64>,
	pub connectable_ts: Option<i64>,
	pub reschedule_ts: Option<i64>,
	pub destroy_ts: Option<i64>,

	/// Explains why the actor is NOT healthy, either due to failure to allocate or a failed
	/// envoy.
	///
	/// # When error is cleared
	///
	/// - When actor is allocated
	/// - When actor becomes connectable
	pub error: Option<ActorError>,
}

impl State {
	pub fn new(
		actor_id: Id,
		name: String,
		pool_name: String,
		key: Option<String>,
		namespace_id: Id,
		create_ts: i64,
	) -> Self {
		State {
			actor_id,
			name,
			pool_name,
			key,
			namespace_id,

			acquired_slot: false,
			envoy_last_command_idx: -1,
			envoy_key: None,

			create_ts,
			create_complete_ts: None,
			allocate_ts: None,
			start_ts: None,
			sleep_ts: None,
			connectable_ts: None,
			reschedule_ts: None,
			destroy_ts: None,

			error: None,
		}
	}
}

#[doc(hidden)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HashAllocatorReadStats {
	pub last_ping_ts_reads: usize,
	pub slots_reads: usize,
}

#[doc(hidden)]
pub async fn allocate_hash_for_tests(
	namespace_id: Id,
	pool_name: &str,
	tx: &universaldb::Transaction,
	pools: &rivet_pools::PoolsHandle,
	now: i64,
	envoy_eligible_threshold: i64,
	samples: u8,
	max_scan: u32,
	slot_jitter: u8,
	use_snapshot_read: bool,
	pivots: Vec<[u8; 16]>,
	rng_seed: u64,
) -> Result<(Option<String>, HashAllocatorReadStats)> {
	alloc_serverful::allocate_hash_for_tests(
		namespace_id,
		pool_name,
		tx,
		pools,
		now,
		envoy_eligible_threshold,
		samples,
		max_scan,
		slot_jitter,
		use_snapshot_read,
		pivots,
		rng_seed,
	)
	.await
}

#[workflow]
#[prune = history] // Don't prune state, required for actor::list_for_ns
pub async fn pegboard_actor2(ctx: &mut WorkflowCtx, input: &Input) -> Result<()> {
	// Actor creation follows a careful sequence to prevent race conditions:
	//
	// 1. **Add actor to UDB with no indexes** This ensures any services attempting on this actor
	//    by ID will find it exists, even before creation is complete.
	//
	// 2. **Reserve the key with Epoxy** This is slow as it traverses datacenters globally to
	//    ensure key is unique across the entire system. We do this before adding to indexes to
	//    prevent showing the actor in API requests before the creation is complete.
	//
	// 3. **Add actor to relevant indexes** Only done after confirming Epoxy key is reserved. If
	//    we added to indexes before Epoxy validation, actors could appear in lists with duplicate
	//    key (since reservation wasn't confirmed yet).

	ctx.activity(InitStateAndUdbInput {
		actor_id: input.actor_id,
		name: input.name.clone(),
		pool_name: input.pool_name.clone(),
		key: input.key.clone(),
		namespace_id: input.namespace_id,
		create_ts: ctx.create_ts(),
		from_v1: input.from_v1,
	})
	.await?;

	if input.from_v1 {
		// When this workflow is dispatched from the v1 actor workflow's migrate-to-v2 path, run the
		// sqlite v1->v2 migration exactly once before any allocation. Skipped for fresh v2 actors.
		ctx.v(2)
			.activity(sqlite::MigrateSqliteV1ToV2Input {
				actor_id: input.actor_id,
				namespace_id: input.namespace_id,
				name: input.name.clone(),
			})
			.await?;
		ctx.v(2).activity(keys::UpdateKeyIndexInput {}).await?;
		ctx.v(2).activity(PopulateIndexesInput {}).await?;
	} else {
		if let Some(key) = &input.key {
			match keys::reserve_key(
				ctx,
				input.namespace_id,
				&input.name,
				&key,
				input.actor_id,
				&input.pool_name,
			)
			.await?
			{
				keys::ReserveKeyOutput::Success => {}
				keys::ReserveKeyOutput::ForwardToDatacenter { dc_label } => {
					ctx.msg(Failed {
						error: errors::Actor::KeyReservedInDifferentDatacenter {
							datacenter_label: dc_label,
						},
					})
					.topic(("actor_id", input.actor_id))
					.send()
					.await?;

					// Destroyed early
					destroy(ctx, input).await?;

					return Ok(());
				}
				keys::ReserveKeyOutput::KeyExists { existing_actor_id } => {
					ctx.msg(Failed {
						error: errors::Actor::DuplicateKey {
							key: key.clone(),
							existing_actor_id,
						},
					})
					.topic(("actor_id", input.actor_id))
					.send()
					.await?;

					// Destroyed early
					destroy(ctx, input).await?;

					return Ok(());
				}
			}
		}

		ctx.activity(PopulateIndexesInput {}).await?;

		ctx.msg(CreateComplete {})
			.topic(("actor_id", input.actor_id))
			.send()
			.await?;
	}

	// Spawn adjacent workflows
	let metrics_workflow_id = ctx
		.workflow(metrics::Input {
			actor_id: input.actor_id,
			namespace_id: input.namespace_id,
			name: input.name.clone(),
		})
		.tag("actor_id", input.actor_id)
		.dispatch()
		.await?;

	let mut lifecycle_state = runtime::LifecycleState::new();

	// Attempt initial allocation
	runtime::reschedule_actor(ctx, input, &mut lifecycle_state, metrics_workflow_id).await?;

	ctx.lupe()
		// TODO: Temp set to 1 as part of a two phase actor wf fix for amp
		.commit_interval(1)
		.with_state(lifecycle_state)
		.run(|ctx, state| {
			let input = input.clone();

			async move {
				let signals = listen_for_signals(ctx, &input, state, metrics_workflow_id).await?;

				for sig in signals {
					if let Loop::Break(()) =
						process_signal(ctx, &input, state, metrics_workflow_id, sig).await?
					{
						return Ok(Loop::Break(()));
					}
				}

				Ok(Loop::Continue)
			}
			.boxed()
		})
		.await?;

	// Destroy adjacent workflows
	ctx.signal(metrics::Destroy {
		ts: util::timestamp::now(),
	})
	.to_workflow_id(metrics_workflow_id)
	.send()
	.await?;

	destroy(ctx, input).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitStateAndUdbInput {
	pub actor_id: Id,
	pub name: String,
	pub key: Option<String>,
	pub namespace_id: Id,
	pub pool_name: String,
	pub create_ts: i64,
	pub from_v1: bool,
}

#[activity(InitStateAndDb)]
pub async fn insert_state_and_db(ctx: &ActivityCtx, input: &InitStateAndUdbInput) -> Result<()> {
	let mut state = ctx.state::<Option<State>>()?;

	let create_ts = ctx
		.udb()?
		.txn("pegboard_actor2_insert_state_and_db", |tx| async move {
			let tx = tx.with_subspace(crate::keys::subspace());

			let create_ts = if input.from_v1 {
				tx.read(
					&crate::keys::actor::CreateTsKey::new(input.actor_id),
					Serializable,
				)
				.await?
			} else {
				tx.write(
					&crate::keys::actor::CreateTsKey::new(input.actor_id),
					input.create_ts,
				)?;

				input.create_ts
			};
			tx.write(
				&crate::keys::actor::WorkflowIdKey::new(input.actor_id),
				ctx.workflow_id(),
			)?;
			tx.write(
				&crate::keys::actor::NamespaceIdKey::new(input.actor_id),
				input.namespace_id,
			)?;
			tx.write(
				&crate::keys::actor::PoolNameKey::new(input.actor_id),
				input.pool_name.clone(),
			)?;
			tx.write(
				&crate::keys::actor::NameKey::new(input.actor_id),
				input.name.clone(),
			)?;
			tx.write(&crate::keys::actor::VersionKey::new(input.actor_id), 2)?;
			// Generation is reset to 0 even when upgrading from v1 actor wf
			tx.write(&crate::keys::actor::GenerationKey::new(input.actor_id), 0)?;

			if let Some(key) = &input.key {
				tx.write(
					&crate::keys::actor::KeyKey::new(input.actor_id),
					key.clone(),
				)?;
			}

			if !input.from_v1 {
				// Update metrics
				namespace::keys::metric::inc(
					&tx.with_subspace(namespace::keys::subspace()),
					input.namespace_id,
					namespace::keys::metric::Metric::TotalActors(input.name.clone()),
					1,
				);
			}

			Ok(create_ts)
		})
		.custom_instrument(tracing::info_span!("actor_insert_tx"))
		.await?;

	*state = Some(State::new(
		input.actor_id,
		input.name.clone(),
		input.pool_name.clone(),
		input.key.clone(),
		input.namespace_id,
		create_ts,
	));

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PopulateIndexesInput {}

#[activity(PopulateIndexes)]
pub async fn populate_indexes(ctx: &ActivityCtx, input: &PopulateIndexesInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	// Set create complete
	state.create_complete_ts = Some(util::timestamp::now());

	let namespace_id = state.namespace_id;
	let actor_id = state.actor_id;
	let name = &state.name;
	let create_ts = state.create_ts;

	// Populate indexes
	ctx.udb()?
		.txn("pegboard_actor2_populate_indexes", |tx| {
			async move {
				let tx = tx.with_subspace(crate::keys::subspace());

				// Populate indexes
				tx.write(
					&crate::keys::ns::ActiveActorKey::new(
						namespace_id,
						name.clone(),
						create_ts,
						actor_id,
					),
					ctx.workflow_id(),
				)?;

				tx.write(
					&crate::keys::ns::AllActorKey::new(
						namespace_id,
						name.clone(),
						create_ts,
						actor_id,
					),
					ctx.workflow_id(),
				)?;

				// NOTE: keys::ns::ActorByKeyKey is written in actor_keys.rs when reserved by epoxy

				Ok(())
			}
		})
		.custom_instrument(tracing::info_span!("actor_populate_indexes_tx"))
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckEnvoyLivenessInput {
	envoy_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckEnvoyLivenessOutput {
	expired: bool,
	now: i64,
}

#[activity(CheckEnvoyLiveness)]
async fn check_envoy_liveness(
	ctx: &ActivityCtx,
	input: &CheckEnvoyLivenessInput,
) -> Result<CheckEnvoyLivenessOutput> {
	let state = ctx.state::<State>()?;
	let envoy_lost_threshold = ctx.config().pegboard().envoy_lost_threshold();

	let namespace_id = state.namespace_id;
	ctx.udb()?
		.txn("pegboard_actor2_check_envoy_liveness", |tx| async move {
			let tx = tx.with_subspace(crate::keys::subspace());

			let last_ping_ts = tx
				.read(
					&crate::keys::envoy::LastPingTsKey::new(namespace_id, input.envoy_key.clone()),
					Serializable,
				)
				.await?;

			let now = util::timestamp::now();
			let expired = last_ping_ts < now - envoy_lost_threshold;

			Ok(CheckEnvoyLivenessOutput { expired, now })
		})
		.custom_instrument(tracing::info_span!("actor_check_envoy_liveness_tx"))
		.await
		.map_err(Into::into)
}

async fn listen_for_signals(
	ctx: &mut WorkflowCtx,
	input: &Input,
	state: &mut runtime::LifecycleState,
	metrics_workflow_id: Id,
) -> Result<Vec<Main>> {
	// Listen for signals based on transition
	let signals = match &mut state.transition {
		Transition::Allocating {
			lost_timeout_ts, ..
		}
		| Transition::Starting {
			lost_timeout_ts, ..
		}
		| Transition::SleepIntent {
			lost_timeout_ts, ..
		}
		| Transition::StopIntent {
			lost_timeout_ts, ..
		}
		| Transition::GoingAway {
			lost_timeout_ts, ..
		}
		| Transition::Destroying {
			lost_timeout_ts, ..
		} => {
			// Listen for signals with a timeout. if a timeout happens, it means this actor is lost
			let signals = ctx.listen_n_until::<Main>(*lost_timeout_ts, 256).await?;
			if signals.is_empty() {
				tracing::warn!(actor_id=?input.actor_id, transition=%state.transition, "actor lost");

				// Fake signal
				vec![Main::Lost(Lost {
					generation: state.generation,
					reason: LostReason::EnvoyNoResponse,
				})]
			} else {
				signals
			}
		}
		Transition::Running {
			envoy,
			last_liveness_check_ts,
		} => {
			// Listen for signals with periodic liveness check timeout
			let signals = ctx
				.listen_n_until::<Main>(
					*last_liveness_check_ts + ctx.config().pegboard().envoy_lost_threshold(),
					256,
				)
				.await?;

			// Perform liveness check
			if signals.is_empty() {
				let res = ctx
					.activity(CheckEnvoyLivenessInput {
						envoy_key: envoy.envoy_key.clone(),
					})
					.await?;

				*last_liveness_check_ts = res.now;

				if res.expired {
					vec![Main::Lost(Lost {
						generation: state.generation,
						reason: LostReason::EnvoyConnectionLost,
					})]
				} else {
					vec![]
				}
			} else {
				signals
			}
		}
		Transition::Sleeping => {
			if let Some(alarm_ts) = state.alarm_ts {
				// Listen for signals with timeout. if a timeout happens, it means this actor should
				// wake up
				let signals = ctx.listen_n_until::<Main>(alarm_ts, 256).await?;
				if signals.is_empty() {
					tracing::debug!(actor_id=?input.actor_id, "actor wake");

					// Fake signal
					vec![Main::Wake(Wake {})]
				} else {
					signals
				}
			} else {
				// Listen for signals with no timeout
				ctx.listen_n::<Main>(256).await?
			}
		}
		Transition::Reallocating { since_ts } => {
			let next_retry_ts = state.retry_backoff_state.get_next_retry_ts(ctx).await?;

			// If the actor has been retrying for too long, set it to sleep
			if state.retry_backoff_state.last_retry_ts
				> *since_ts + ctx.config().pegboard().actor_retry_duration_threshold()
			{
				state.transition = Transition::Sleeping;

				Vec::new()
			} else {
				let signals = if let Some(next_retry_ts) = next_retry_ts {
					// Listen for signals with timeout
					ctx.listen_n_until::<Main>(next_retry_ts, 256).await?
				} else {
					Vec::new()
				};

				// Attempt reallocation
				if signals.is_empty() {
					runtime::reschedule_actor(ctx, &input, state, metrics_workflow_id).await?;
				}

				signals
			}
		}
	};

	Ok(signals)
}

async fn process_signal(
	ctx: &mut WorkflowCtx,
	input: &Input,
	state: &mut runtime::LifecycleState,
	metrics_workflow_id: Id,
	sig: Main,
) -> Result<Loop<()>> {
	match sig {
		Main::Allocated(sig) => {
			// Ignore signals for previous generations
			if sig.generation != state.generation {
				return Ok(Loop::Continue);
			}

			if let Transition::Allocating {
				destroy_after_start,
				..
			} = &state.transition
			{
				let now = ctx.activity(runtime::GetTsInput {}).await?;

				// Transition to starting
				state.transition = Transition::Starting {
					destroy_after_start: *destroy_after_start,
					lost_timeout_ts: now + ctx.config().pegboard().actor_start_threshold(),
				};
			}

			ctx.signal(metrics::Resume {
				ts: util::timestamp::now(),
			})
			.to_workflow_id(metrics_workflow_id)
			.send()
			.await?;
		}
		Main::Events(sig) => {
			// Ignore the events signal based on current transition state
			match &state.transition {
				Transition::Starting { .. } => {}
				Transition::Running { envoy, .. }
				| Transition::SleepIntent { envoy, .. }
				| Transition::StopIntent { envoy, .. }
				| Transition::GoingAway { envoy, .. }
				| Transition::Destroying { envoy, .. } => {
					if &sig.envoy_key != &envoy.envoy_key {
						tracing::debug!("events not from current envoy, ignoring");
						return Ok(Loop::Continue);
					}
				}
				Transition::Allocating { .. }
				| Transition::Sleeping
				| Transition::Reallocating { .. } => {
					tracing::warn!(?sig, "actor not allocated, ignoring events");
					return Ok(Loop::Continue);
				}
			}

			let now = ctx.activity(runtime::GetTsInput {}).await?;

			// Fetch the last event index for the current envoy or default to -1 (if still starting)
			let last_event_idx = state
				.transition
				.envoy()
				.map(|e| e.last_event_idx)
				.unwrap_or(-1);

			let mut new_last_event_idx = None;
			let mut alarms_set = 0;

			for event in sig.events {
				// Filter events from previous generations and already received events
				if event.checkpoint.generation != state.generation
					|| event.checkpoint.index <= last_event_idx
				{
					tracing::debug!(?event, "ignored event due to generation/index filter");
					continue;
				}

				new_last_event_idx = Some(event.checkpoint.index);

				match &event.inner {
					protocol::Event::EventActorIntent(protocol::EventActorIntent {
						intent,
						..
					}) => {
						match intent {
							protocol::ActorIntent::ActorIntentSleep => {
								if let Transition::Running { envoy, .. } = &mut state.transition {
									let envoy = std::mem::take(envoy);
									// Transition to sleep intent
									state.transition = Transition::SleepIntent {
										envoy,
										lost_timeout_ts: now
											+ ctx.config().pegboard().actor_stop_threshold(),
										rewake_after_stop: false,
									};

									ctx.activity(runtime::SetSleepingInput {}).await?;

									ctx.activity(runtime::InsertAndSendCommandsInput {
										generation: state.generation,
										envoy_key: sig.envoy_key.clone(),
										commands: vec![protocol::Command::CommandStopActor(
											protocol::CommandStopActor {
												reason: protocol::StopActorReason::SleepIntent,
											},
										)],
									})
									.await?;
								}
							}
							protocol::ActorIntent::ActorIntentStop => {
								if let Transition::Running { envoy, .. } = &mut state.transition {
									let envoy = std::mem::take(envoy);
									// Transition to stop intent
									state.transition = Transition::StopIntent {
										envoy,
										lost_timeout_ts: now
											+ ctx.config().pegboard().actor_stop_threshold(),
									};

									ctx.activity(runtime::SetNotConnectableInput {}).await?;

									ctx.activity(runtime::InsertAndSendCommandsInput {
										generation: state.generation,
										envoy_key: sig.envoy_key.clone(),
										commands: vec![protocol::Command::CommandStopActor(
											protocol::CommandStopActor {
												reason: protocol::StopActorReason::StopIntent,
											},
										)],
									})
									.await?;
								}
							}
						}
					}
					protocol::Event::EventActorStateUpdate(protocol::EventActorStateUpdate {
						state: actor_state,
						..
					}) => match actor_state {
						protocol::ActorState::ActorStateRunning => {
							if let Transition::Starting {
								destroy_after_start,
								..
							} = &mut state.transition
							{
								if *destroy_after_start {
									// Transition to destroying
									state.transition = Transition::Destroying {
										envoy: runtime::EnvoyState::new(sig.envoy_key.clone()),
										lost_timeout_ts: now
											+ ctx.config().pegboard().actor_stop_threshold(),
									};

									ctx.activity(runtime::InsertAndSendCommandsInput {
										generation: state.generation,
										envoy_key: sig.envoy_key.clone(),
										commands: vec![protocol::Command::CommandStopActor(
											protocol::CommandStopActor {
												reason: protocol::StopActorReason::Destroy,
											},
										)],
									})
									.await?;
								} else {
									// Transition to starting
									state.transition = Transition::Running {
										envoy: runtime::EnvoyState::new(sig.envoy_key.clone()),
										last_liveness_check_ts: now,
									};

									ctx.activity(runtime::SetConnectableInput {
										envoy_key: sig.envoy_key.clone(),
										generation: state.generation,
									})
									.await?;

									ctx.msg(Ready {
										envoy_key: sig.envoy_key.clone(),
									})
									.topic(("actor_id", input.actor_id))
									.send()
									.await?;
								}
							}
						}
						protocol::ActorState::ActorStateStopped(protocol::ActorStateStopped {
							code,
							message,
						}) => {
							if let StoppedResult::Destroy = runtime::handle_stopped(
								ctx,
								&input,
								state,
								metrics_workflow_id,
								runtime::StoppedVariant::Stopped {
									code: code.clone(),
									message: message.clone(),
								},
							)
							.await?
							{
								return Ok(Loop::Break(()));
							}
						}
					},
					protocol::Event::EventActorSetAlarm(protocol::EventActorSetAlarm {
						alarm_ts,
						..
					}) => {
						state.alarm_ts = *alarm_ts;
						alarms_set += 1;
					}
				}
			}

			let send_ack_act = if let (Some(envoy), Some(new_last_event_idx)) =
				(state.transition.envoy(), new_last_event_idx)
			{
				// Update last event idx
				envoy.last_event_idx = envoy.last_event_idx.max(new_last_event_idx);

				// Ack events in batch
				if envoy.last_event_idx
					> envoy
						.last_event_ack_idx
						.saturating_add(EVENT_ACK_BATCH_SIZE)
				{
					envoy.last_event_ack_idx = envoy.last_event_idx;

					// Send ack events msg to envoy
					Some(activity(runtime::SendMessagesToEnvoyInput {
						namespace_id: input.namespace_id,
						envoy_key: envoy.envoy_key.clone(),
						messages: vec![protocol::ToEnvoyConn::ToEnvoyAckEvents(
							protocol::ToEnvoyAckEvents {
								last_event_checkpoints: vec![protocol::ActorCheckpoint {
									actor_id: input.actor_id.to_string(),
									generation: state.generation,
									index: envoy.last_event_ack_idx,
								}],
							},
						)],
					}))
				} else {
					None
				}
			} else {
				None
			};

			ctx.join((
				send_ack_act,
				// Record alarm metrics
				(alarms_set > 0).then(|| {
					activity(runtime::RecordEventMetricsInput {
						namespace_id: input.namespace_id,
						name: input.name.clone(),
						alarms_set,
					})
				}),
			))
			.await?;
		}
		Main::Wake(_) => {
			// Clear alarm
			if let Some(alarm_ts) = state.alarm_ts {
				let now = ctx.activity(runtime::GetTsInput {}).await?;

				if now >= alarm_ts {
					state.alarm_ts = None;
				}
			}

			match &mut state.transition {
				Transition::Sleeping => {
					runtime::reschedule_actor(ctx, &input, state, metrics_workflow_id).await?;
				}
				Transition::SleepIntent {
					rewake_after_stop, ..
				} => {
					if !*rewake_after_stop {
						*rewake_after_stop = true;

						tracing::debug!(
							actor_id=?input.actor_id,
							"cannot wake an actor that intends to sleep but has not stopped yet, deferring wake until after stop",
						);
					}
				}
				_ => {
					tracing::debug!(
						actor_id=?input.actor_id,
						"cannot wake actor that is not sleeping",
					);
				}
			}
		}
		Main::Sleep(_) => {
			match &mut state.transition {
				Transition::Allocating { .. }
				| Transition::Starting { .. }
				| Transition::GoingAway { .. } => {
					// TODO: Set to sleep after allocation is complete
				}
				Transition::Running { envoy, .. } => {
					let envoy_key = envoy.envoy_key.clone();
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy = std::mem::take(envoy);

					// Transition to sleep intent
					state.transition = Transition::SleepIntent {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
						rewake_after_stop: false,
					};

					ctx.activity(runtime::SetSleepingInput {}).await?;

					ctx.activity(runtime::InsertAndSendCommandsInput {
						generation: state.generation,
						envoy_key,
						commands: vec![protocol::Command::CommandStopActor(
							protocol::CommandStopActor {
								reason: protocol::StopActorReason::SleepIntent,
							},
						)],
					})
					.await?;
				}
				Transition::Reallocating { .. } => {
					// Stop reallocating
					state.transition = Transition::Sleeping;
				}
				Transition::SleepIntent { .. }
				| Transition::StopIntent { .. }
				| Transition::Sleeping
				| Transition::Destroying { .. } => {}
			}
		}
		Main::Reschedule(_) => {
			match &mut state.transition {
				Transition::Running { envoy, .. } => {
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy_key = envoy.envoy_key.clone();
					let envoy = std::mem::take(envoy);

					// Transition to going away
					state.transition = Transition::GoingAway {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
					};

					ctx.activity(runtime::InsertAndSendCommandsInput {
						generation: state.generation,
						envoy_key,
						commands: vec![protocol::Command::CommandStopActor(
							protocol::CommandStopActor {
								reason: protocol::StopActorReason::GoingAway,
							},
						)],
					})
					.await?;
				}
				Transition::SleepIntent { envoy, .. } | Transition::StopIntent { envoy, .. } => {
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy = std::mem::take(envoy);

					state.transition = Transition::GoingAway {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
					};

					// Stop command was already sent
				}
				Transition::Allocating { .. }
				| Transition::Starting { .. }
				| Transition::Sleeping
				| Transition::Reallocating { .. } => {
					// Do nothing, already mid allocation
				}
				Transition::GoingAway { .. } | Transition::Destroying { .. } => {}
			}
		}
		Main::Lost(sig) => {
			// Ignore signals for previous generations
			if sig.generation != state.generation {
				return Ok(Loop::Continue);
			}

			if let StoppedResult::Destroy = runtime::handle_stopped(
				ctx,
				&input,
				state,
				metrics_workflow_id,
				runtime::StoppedVariant::Lost { reason: sig.reason },
			)
			.await?
			{
				return Ok(Loop::Break(()));
			}
		}
		Main::GoingAway(sig) => {
			// Ignore signals for previous generations
			if sig.generation != state.generation {
				return Ok(Loop::Continue);
			}

			match &mut state.transition {
				Transition::Running { envoy, .. } => {
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy_key = envoy.envoy_key.clone();
					let envoy = std::mem::take(envoy);

					// Transition to going away
					state.transition = Transition::GoingAway {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
					};

					match ctx.check_version(2).await? {
						1 => {}
						_latest => {
							ctx.v(2)
								.activity(runtime::SetNotConnectableInput {})
								.await?;
						}
					}

					ctx.activity(runtime::InsertAndSendCommandsInput {
						generation: state.generation,
						envoy_key,
						commands: vec![protocol::Command::CommandStopActor(
							protocol::CommandStopActor {
								reason: protocol::StopActorReason::GoingAway,
							},
						)],
					})
					.await?;
				}
				Transition::SleepIntent { envoy, .. } | Transition::StopIntent { envoy, .. } => {
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy = std::mem::take(envoy);

					state.transition = Transition::GoingAway {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
					};

					// Stop command was already sent
				}
				Transition::Allocating { .. } => {
					tracing::warn!(transition=?state.transition, "should not be reachable");
				}
				Transition::Starting { .. } => {
					// TODO: Reallocate after start
				}
				Transition::Sleeping
				| Transition::Reallocating { .. }
				| Transition::GoingAway { .. }
				| Transition::Destroying { .. } => {}
			}
		}
		Main::Destroy(_) => {
			match &mut state.transition {
				Transition::Running { envoy, .. } => {
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy_key = envoy.envoy_key.clone();
					let envoy = std::mem::take(envoy);

					// Transition to destroying
					state.transition = Transition::Destroying {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
					};

					ctx.activity(runtime::InsertAndSendCommandsInput {
						generation: state.generation,
						envoy_key,
						commands: vec![protocol::Command::CommandStopActor(
							protocol::CommandStopActor {
								reason: protocol::StopActorReason::Destroy,
							},
						)],
					})
					.await?;
				}
				Transition::SleepIntent { envoy, .. }
				| Transition::StopIntent { envoy, .. }
				| Transition::GoingAway { envoy, .. } => {
					let now = ctx.activity(runtime::GetTsInput {}).await?;
					let envoy = std::mem::take(envoy);

					state.transition = Transition::Destroying {
						envoy,
						lost_timeout_ts: now + ctx.config().pegboard().actor_stop_threshold(),
					};

					// Stop command was already sent
				}
				Transition::Allocating {
					destroy_after_start,
					..
				} => {
					*destroy_after_start = true;
				}
				Transition::Starting {
					destroy_after_start,
					..
				} => {
					*destroy_after_start = true;
				}
				Transition::Sleeping | Transition::Reallocating { .. } => {
					return Ok(Loop::Break(()));
				}
				Transition::Destroying { .. } => {}
			}
		}
	}

	Ok(Loop::Continue)
}

/// Reason why an actor failed to allocate or run.
///
/// Distinct from `errors::Actor` which represents user-facing API errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorError {
	/// Actor cannot allocate due to the concurrent actor limit. Only set if `error`
	/// is currently `None` (envoy failures take precedence as root causes).
	ConcurrentActorLimitReached,
	/// No envoys connected to serverful pool.
	NoEnvoys,
	/// Envoy did not respond with expected events (lost timeout).
	EnvoyNoResponse { envoy_key: Option<String> },
	/// Envoy connection was lost (no recent ping, network issue, or crash).
	EnvoyConnectionLost { envoy_key: String },
	/// Actor crashed during execution.
	Crashed { message: Option<String> },
}

impl ActorError {
	/// Used to determine the category of this error.
	///
	/// Actor errors will not override envoy errors.
	pub fn is_envoy_failure(&self) -> bool {
		match self {
			ActorError::ConcurrentActorLimitReached | ActorError::Crashed { .. } => false,
			ActorError::EnvoyNoResponse { .. }
			| ActorError::EnvoyConnectionLost { .. }
			| ActorError::NoEnvoys => true,
		}
	}
}

async fn destroy(ctx: &mut WorkflowCtx, input: &Input) -> Result<()> {
	ctx.msg(DestroyStarted {})
		.topic(("actor_id", input.actor_id))
		.send()
		.await?;

	ctx.activity(UpdateStateAndDbInput {}).await?;
	ctx.activity(ClearKvInput {}).await?;

	ctx.msg(DestroyComplete {})
		.topic(("actor_id", input.actor_id))
		.send()
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateStateAndDbInput {}

#[activity(UpdateStateAndDb)]
async fn update_state_and_db(ctx: &ActivityCtx, input: &UpdateStateAndDbInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	let destroy_ts = util::timestamp::now();
	state.destroy_ts = Some(destroy_ts);

	let namespace_id = state.namespace_id;
	let actor_id = state.actor_id;
	let name = &state.name;
	let create_ts = state.create_ts;
	let key = &state.key;
	ctx.udb()?
		.txn("pegboard_actor2_destroy", |tx| {
			async move {
				let tx = tx.with_subspace(crate::keys::subspace());

				tx.write(&crate::keys::actor::DestroyTsKey::new(actor_id), destroy_ts)?;

				// Update namespace indexes
				tx.delete(&crate::keys::ns::ActiveActorKey::new(
					namespace_id,
					name.clone(),
					create_ts,
					actor_id,
				));

				if let Some(key) = &key {
					tx.write(
						&crate::keys::ns::ActorByKeyKey::new(
							namespace_id,
							name.clone(),
							key.clone(),
							create_ts,
							actor_id,
						),
						ActorByKeyKeyData {
							workflow_id: ctx.workflow_id(),
							is_destroyed: true,
						},
					)?;
				}

				// Update metrics
				namespace::keys::metric::inc(
					&tx.with_subspace(namespace::keys::subspace()),
					namespace_id,
					namespace::keys::metric::Metric::TotalActors(name.clone()),
					-1,
				);

				Ok(())
			}
		})
		.custom_instrument(tracing::info_span!("actor_destroy_tx"))
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct ClearKvInput {}

#[derive(Debug, Serialize, Deserialize)]
struct ClearKvOutput {
	/// Simply an estimate, not accurate under 3MiB
	final_size: i64,
}

#[activity(ClearKv)]
async fn clear_kv(ctx: &ActivityCtx, input: &ClearKvInput) -> Result<ClearKvOutput> {
	let state = ctx.state::<State>()?;

	let actor_id = state.actor_id;
	let namespace_id = state.namespace_id;
	let udb = ctx.udb()?;
	let final_size = udb
		.txn("pegboard_actor2_clear_kv", |tx| async move {
			let subspace = crate::keys::actor_kv::subspace(actor_id);

			let (start, end) = subspace.range();
			let final_size = tx.get_estimated_range_size_bytes(&start, &end).await?;

			// Matches `delete_all` from actor kv
			tx.clear_subspace_range(&subspace);
			crate::actor_sqlite::clear_v2_storage_for_destroy(&tx, actor_id);

			Ok(final_size)
		})
		.custom_instrument(tracing::info_span!("actor_clear_kv_tx"))
		.await?;

	// Keep outside txn - branch delete has its own txn.
	crate::actor_sqlite::clear_branch_storage_for_destroy(&udb, namespace_id, actor_id).await?;

	Ok(ClearKvOutput { final_size })
}

#[message("pegboard_actor2_create_complete")]
pub struct CreateComplete {}

#[message("pegboard_actor2_failed")]
pub struct Failed {
	pub error: errors::Actor,
}

#[message("pegboard_actor2_ready")]
pub struct Ready {
	pub envoy_key: String,
}

#[message("pegboard_actor2_stopped")]
pub struct Stopped {}

#[derive(Debug)]
#[signal("pegboard_actor2_events")]
pub struct Events {
	pub envoy_key: String,
	pub events: Vec<protocol::EventWrapper>,
}

#[signal("pegboard_actor2_wake")]
pub struct Wake {}

#[signal("pegboard_actor2_sleep")]
pub struct Sleep {}

#[signal("pegboard_actor2_reschedule")]
pub struct Reschedule {}

/// Ack response from outbound req handler service.
#[signal("pegboard_actor2_allocated")]
pub struct Allocated {
	pub generation: u32,
}

/// Reason why an actor was lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LostReason {
	/// Envoy did not respond with expected events (lost timeout).
	EnvoyNoResponse,
	/// Envoy connection was lost (no recent ping, network issue, or crash).
	EnvoyConnectionLost,
}

#[derive(Debug)]
#[signal("pegboard_actor2_lost")]
pub struct Lost {
	pub generation: u32,
	/// Why the actor was lost.
	pub reason: LostReason,
}

#[derive(Debug)]
#[signal("pegboard_actor2_going_away")]
pub struct GoingAway {
	pub generation: u32,
}

#[signal("pegboard_actor2_destroy")]
pub struct Destroy {}

#[message("pegboard_actor2_destroy_started")]
pub struct DestroyStarted {}

#[message("pegboard_actor2_destroy_complete")]
pub struct DestroyComplete {}

join_signal!(Main {
	Allocated,
	Events,
	Wake,
	Sleep,
	Reschedule,
	Lost,
	GoingAway,
	Destroy,
	// Comment to prevent invalid formatting
});
