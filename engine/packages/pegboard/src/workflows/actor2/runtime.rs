use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use futures_util::TryStreamExt;
use gas::{prelude::*, workflow::StateGuard};
use rivet_envoy_protocol::{self as protocol, PROTOCOL_VERSION, versioned};
use rivet_types::runner_configs::RunnerConfigKind;
use std::{fmt, time::Instant};
use universaldb::prelude::*;
use universalpubsub::PublishOpts;
use vbare::OwnedVersionedData;

use super::{ActorError, Input, LostReason, State, Stopped, metrics};
use crate::keys;

#[derive(Deserialize, Serialize)]
pub struct LifecycleState {
	pub generation: u32,
	pub transition: Transition,
	pub alarm_ts: Option<i64>,
	pub retry_backoff_state: RetryBackoffState,
}

impl LifecycleState {
	pub fn new() -> Self {
		LifecycleState {
			generation: 0,
			transition: Transition::Allocating {
				destroy_after_start: false,
				lost_timeout_ts: 0,
			},
			alarm_ts: None,
			retry_backoff_state: RetryBackoffState::default(),
		}
	}
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Transition {
	Allocating {
		destroy_after_start: bool,
		/// Handles cleaning up the actor if it does not receive a certain state before the timeout (ex.
		/// created -> running event, stop intent -> stop event). If the timeout is reached, the actor is
		/// considered lost.
		lost_timeout_ts: i64,
	},
	Starting {
		destroy_after_start: bool,
		lost_timeout_ts: i64,
	},
	Running {
		envoy: EnvoyState,
		last_liveness_check_ts: i64,
	},
	SleepIntent {
		envoy: EnvoyState,
		lost_timeout_ts: i64,
		/// If a wake was received in between an actor's intent to sleep and actor stop.
		rewake_after_stop: bool,
	},
	StopIntent {
		envoy: EnvoyState,
		lost_timeout_ts: i64,
	},
	GoingAway {
		envoy: EnvoyState,
		lost_timeout_ts: i64,
	},
	Sleeping,
	Reallocating {
		since_ts: i64,
	},
	Destroying {
		envoy: EnvoyState,
		lost_timeout_ts: i64,
	},
}

impl Transition {
	pub(crate) fn envoy(&mut self) -> Option<&mut EnvoyState> {
		match self {
			Transition::Running { envoy, .. }
			| Transition::SleepIntent { envoy, .. }
			| Transition::StopIntent { envoy, .. }
			| Transition::GoingAway { envoy, .. }
			| Transition::Destroying { envoy, .. } => Some(envoy),
			Transition::Allocating { .. }
			| Transition::Starting { .. }
			| Transition::Sleeping
			| Transition::Reallocating { .. } => None,
		}
	}
}

impl fmt::Display for Transition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Transition::Allocating { .. } => write!(f, "allocating"),
			Transition::Starting { .. } => write!(f, "starting"),
			Transition::Running { .. } => write!(f, "running"),
			Transition::SleepIntent { .. } => write!(f, "sleep_intent"),
			Transition::StopIntent { .. } => write!(f, "stop_intent"),
			Transition::GoingAway { .. } => write!(f, "going_away"),
			Transition::Sleeping => write!(f, "sleeping"),
			Transition::Reallocating { .. } => write!(f, "reallocating"),
			Transition::Destroying { .. } => write!(f, "destroying"),
		}
	}
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EnvoyState {
	pub envoy_key: String,
	pub last_event_idx: i64,
	pub last_event_ack_idx: i64,
}

impl EnvoyState {
	pub fn new(envoy_key: String) -> Self {
		EnvoyState {
			envoy_key,
			last_event_idx: -1,
			last_event_ack_idx: -1,
		}
	}
}

// Used for `mem::take`
impl Default for EnvoyState {
	fn default() -> Self {
		EnvoyState {
			envoy_key: String::new(),
			last_event_idx: -1,
			last_event_ack_idx: -1,
		}
	}
}

#[derive(Serialize, Deserialize, Default)]
pub struct RetryBackoffState {
	pub last_retry_ts: i64,
	pub retry_count: usize,
}

impl RetryBackoffState {
	pub async fn get_next_retry_ts(&mut self, ctx: &mut WorkflowCtx) -> Result<Option<i64>> {
		let (reschedule_ts, now, reset) = ctx
			.activity(CompareRetryInput {
				retry_count: self.retry_count,
				last_retry_ts: self.last_retry_ts,
			})
			.await?;

		self.retry_count = if reset { 0 } else { self.retry_count + 1 };
		self.last_retry_ts = now;

		Ok(reschedule_ts)
	}
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocateInput {
	// Added property, must use default
	#[serde(default)]
	generation: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Allocation {
	Serverless,
	Serverful { envoy_key: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocateOutput {
	pub allocation: Option<Allocation>,
	pub now: i64,
}

#[activity(Allocate)]
pub async fn allocate(ctx: &ActivityCtx, input: &AllocateInput) -> Result<AllocateOutput> {
	let start = Instant::now();
	let mut state = ctx.state::<State>()?;

	// NOTE: Assuming cache is populated, this is faster than reading the runner config in the fdb txn below
	let pool_res = ctx
		.op(crate::ops::runner_config::get::Input {
			runners: vec![(state.namespace_id, state.pool_name.clone())],
			bypass_cache: false,
		})
		.await?;
	let pool = pool_res.into_iter().next();
	let is_serverless = pool
		.as_ref()
		.map(|pool| matches!(pool.config.kind, RunnerConfigKind::Serverless { .. }))
		.unwrap_or(false);
	let max_concurrent_actors = pool.as_ref().and_then(|pool| match &pool.config.kind {
		RunnerConfigKind::Serverless {
			max_concurrent_actors,
			..
		} => Some(*max_concurrent_actors),
		_ => None,
	});

	let actor_id = state.actor_id;
	let namespace_id = state.namespace_id;
	let pool_name = &state.pool_name;
	let envoy_eligible_threshold = ctx.config().pegboard().envoy_eligible_threshold();
	let envoy_load_balancer = ctx.config().pegboard().envoy_load_balancer();
	let pools = ctx.pools().clone();

	// Check if limit has been reached and choose an envoy if serverful
	let (acquired_slot, allocation, error) = ctx
		.udb()?
		.txn("pegboard_actor2_acquire_slot", |tx| {
			let pools = pools.clone();
			async move {
				let now = util::timestamp::now();
				let tx = tx.with_subspace(keys::subspace());

				let actor_slots_key = keys::ns::ActorSlotsKey::new(namespace_id, pool_name.clone());

				let acquired_slot = if let Some(max_concurrent_actors) = max_concurrent_actors {
					if tx.read_opt(&actor_slots_key, Snapshot).await?.unwrap_or(0)
						< max_concurrent_actors as i64
					{
						tx.atomic_op(&actor_slots_key, &1i64.to_le_bytes(), MutationType::Add);

						true
					} else {
						false
					}
				} else {
					tx.atomic_op(&actor_slots_key, &1i64.to_le_bytes(), MutationType::Add);

					true
				};

				let (allocation, error) = if acquired_slot {
					if is_serverless {
						(Some(Allocation::Serverless), None)
					} else {
						let allocation = super::alloc_serverful::allocate_serverful(
							namespace_id,
							&pool_name,
							&tx,
							&pools,
							now,
							envoy_eligible_threshold,
							&envoy_load_balancer,
						)
						.await?;

						if allocator_debug_enabled() {
							let ping_threshold_ts = now - envoy_eligible_threshold;
							let dump = dump_envoy_lb_idx_eligible(
								&tx,
								namespace_id,
								&pool_name,
								ping_threshold_ts,
							)
							.await?;
							tracing::info!(
								?namespace_id,
								%pool_name,
								?actor_id,
								now,
								ping_threshold_ts,
								eligible_count = dump.len(),
								chosen_envoy_key = ?allocation,
								entries = ?dump,
								"pegboard allocator subspace dump"
							);
						}

						let error = allocation.is_none().then_some(super::ActorError::NoEnvoys);

						(
							allocation.map(|envoy_key| Allocation::Serverful { envoy_key }),
							error,
						)
					}
				} else {
					(None, Some(super::ActorError::ConcurrentActorLimitReached))
				};

				if allocation.is_some() {
					// Set not sleeping
					tx.delete(&keys::actor::SleepTsKey::new(actor_id));
					tx.write(&keys::actor::GenerationKey::new(actor_id), input.generation)?;
				}

				Ok((acquired_slot, allocation, error))
			}
		})
		.custom_instrument(tracing::info_span!("actor_allocate_tx"))
		.await?;

	state.acquired_slot = acquired_slot;
	state.error = error;
	state.envoy_last_command_idx = 0;

	let now = util::timestamp::now();

	if allocation.is_some() {
		state.sleep_ts = None;
		state.reschedule_ts = None;
	}

	let dt = start.elapsed().as_secs_f64();
	crate::metrics::ACTOR_ALLOCATE_DURATION
		.with_label_values(&[
			if is_serverless {
				"serverless"
			} else {
				"serverful"
			},
			if allocation.is_some() {
				"allocated"
			} else {
				"not_allocated"
			},
		])
		.observe(dt);

	Ok(AllocateOutput { allocation, now })
}

fn allocator_debug_enabled() -> bool {
	static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
	*CACHED.get_or_init(|| {
		std::env::var("RIVET_DEBUG_PEGBOARD_ALLOCATOR")
			.map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
			.unwrap_or(false)
	})
}

#[derive(Debug)]
#[allow(dead_code)]
struct LbIdxDumpEntry {
	version: u32,
	last_ping_ts: i64,
	envoy_key: String,
	gap_before_ms: i64,
}

// Reads the full envoy LB idx subspace for (namespace_id, pool_name), keeps only entries with
// last_ping_ts >= ping_threshold_ts, and annotates each entry with the gap to the previous entry
// of the same version. Uses Snapshot so the scan does not contribute to the txn read-conflict
// range. Intended for debug-only logging; do not call on a hot path.
async fn dump_envoy_lb_idx_eligible(
	tx: &universaldb::Transaction,
	namespace_id: Id,
	pool_name: &str,
	ping_threshold_ts: i64,
) -> Result<Vec<LbIdxDumpEntry>> {
	let subspace = keys::subspace().subspace(&keys::ns::EnvoyLoadBalancerIdxKey::subspace(
		namespace_id,
		pool_name.to_string(),
	));
	let mut stream = tx.get_ranges_keyvalues(
		RangeOption {
			mode: StreamingMode::WantAll,
			..(&subspace).into()
		},
		Snapshot,
	);

	let mut out = Vec::new();
	let mut prev_ping_ts: Option<i64> = None;
	let mut prev_version: Option<u32> = None;

	while let Some(entry) = stream.try_next().await? {
		let (lb_key, _) = tx.read_entry::<keys::ns::EnvoyLoadBalancerIdxKey>(&entry)?;
		if lb_key.last_ping_ts < ping_threshold_ts {
			continue;
		}

		let gap_before_ms = match (prev_version, prev_ping_ts) {
			(Some(pv), Some(pt)) if pv == lb_key.version => lb_key.last_ping_ts - pt,
			_ => 0,
		};
		prev_ping_ts = Some(lb_key.last_ping_ts);
		prev_version = Some(lb_key.version);

		out.push(LbIdxDumpEntry {
			version: lb_key.version,
			last_ping_ts: lb_key.last_ping_ts,
			envoy_key: lb_key.envoy_key,
			gap_before_ms,
		});
	}

	Ok(out)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendOutboundInput {
	pub generation: u32,
	pub input: Option<String>,
	pub allocation: Allocation,
}

#[activity(SendOutbound)]
pub async fn send_outbound(ctx: &ActivityCtx, input: &SendOutboundInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	match &input.allocation {
		Allocation::Serverless => {
			let message_serialized = versioned::ToOutbound::wrap_latest(
				protocol::ToOutbound::ToOutboundActorStart(protocol::ToOutboundActorStart {
					namespace_id: state.namespace_id.to_string(),
					pool_name: state.pool_name.clone(),
					checkpoint: protocol::ActorCheckpoint {
						actor_id: state.actor_id.to_string(),
						generation: input.generation,
						// It is guaranteed that this is the first command because every new allocation has a
						// unique generation
						index: 0,
					},
					actor_config: protocol::ActorConfig {
						name: state.name.clone(),
						key: state.key.clone(),
						create_ts: util::timestamp::now(),
						input: input
							.input
							.as_ref()
							.and_then(|x| BASE64_STANDARD.decode(x).ok()),
					},
				}),
			)
			.serialize_with_embedded_version(PROTOCOL_VERSION)?;

			ctx.ups()?
				.publish(
					crate::pubsub_subjects::ServerlessOutboundSubject,
					&message_serialized,
					PublishOpts::one(),
				)
				.await?;
		}
		Allocation::Serverful { envoy_key } => {
			let command = protocol::Command::CommandStartActor(protocol::CommandStartActor {
				config: protocol::ActorConfig {
					name: state.name.clone(),
					key: state.key.clone(),
					create_ts: util::timestamp::now(),
					input: input
						.input
						.as_ref()
						.and_then(|x| BASE64_STANDARD.decode(x).ok()),
				},
				// Request ids are ephemeral. Pegboard-envoy refreshes this on the
				// WebSocket send path immediately before the actor start reaches envoy.
				hibernating_requests: Vec::new(),
				preloaded_kv: None,
			});

			insert_and_send_commands_inner(
				ctx,
				&InsertAndSendCommandsInput {
					generation: input.generation,
					envoy_key: envoy_key.clone(),
					commands: vec![command],
				},
				&mut state,
			)
			.await?;
		}
	}

	// We record allocate ts only after sending the outbound msg so that when we measure actor start time it
	// has nothing to do with actor workflow operations
	state.allocate_ts = Some(util::timestamp::now());

	Ok(())
}

pub async fn reschedule_actor(
	ctx: &mut WorkflowCtx,
	input: &Input,
	state: &mut LifecycleState,
	metrics_workflow_id: Id,
) -> Result<()> {
	let next_gen = state.generation + 1;
	let allocate_res = ctx
		.activity(AllocateInput {
			generation: next_gen,
		})
		.await?;

	if let Some(allocation) = allocate_res.allocation {
		state.generation += 1;

		match &allocation {
			Allocation::Serverless => {
				// Transition to allocating
				state.transition = Transition::Allocating {
					destroy_after_start: false,
					lost_timeout_ts: allocate_res.now
						+ ctx.config().pegboard().actor_allocation_threshold(),
				};
			}
			Allocation::Serverful { .. } => {
				// Transition to starting
				state.transition = Transition::Starting {
					destroy_after_start: false,
					lost_timeout_ts: allocate_res.now
						+ ctx.config().pegboard().actor_start_threshold(),
				};
			}
		}

		ctx.activity(SendOutboundInput {
			generation: state.generation,
			input: input.input.clone(),
			allocation,
		})
		.await?;
	} else {
		// NOTE: Cannot return `StoppedResult::Destroy` if provided `StoppedVariant::FailedAllocation` so we
		// can ignore it
		handle_stopped(
			ctx,
			&input,
			state,
			metrics_workflow_id,
			StoppedVariant::FailedAllocation,
		)
		.await?;
	}

	Ok(())
}

#[derive(Debug)]
pub enum StoppedVariant {
	FailedAllocation,
	Stopped {
		code: protocol::StopCode,
		message: Option<String>,
	},
	Lost {
		reason: LostReason,
	},
}

// What the workflow should do after `handle_stopped`
pub enum StoppedResult {
	Continue,
	Destroy,
}

pub async fn handle_stopped(
	ctx: &mut WorkflowCtx,
	input: &Input,
	state: &mut LifecycleState,
	metrics_workflow_id: Id,
	variant: StoppedVariant,
) -> Result<StoppedResult> {
	tracing::debug!(?variant, ?state.transition, "actor stopped");

	// Save error to state
	match &variant {
		StoppedVariant::FailedAllocation => {}
		StoppedVariant::Stopped {
			code: protocol::StopCode::Ok,
			..
		} => {}
		StoppedVariant::Stopped {
			code: protocol::StopCode::Error,
			message,
		} => {
			ctx.activity(SetErrorInput {
				error: ActorError::Crashed {
					message: message.clone(),
				},
			})
			.await?;
		}
		StoppedVariant::Lost { reason } => {
			// Build error from lost reason
			match reason {
				LostReason::EnvoyNoResponse => {
					let error = ActorError::EnvoyNoResponse {
						envoy_key: state.transition.envoy().map(|x| x.envoy_key.clone()),
					};

					ctx.activity(SetErrorInput { error }).await?;
				}
				LostReason::EnvoyConnectionLost => {
					if let Some(envoy) = state.transition.envoy() {
						let error = ActorError::EnvoyConnectionLost {
							envoy_key: envoy.envoy_key.clone(),
						};
						ctx.activity(SetErrorInput { error }).await?;
					}
				}
			}
		}
	}

	ctx.activity(DeallocateInput {}).await?;

	// Pause periodic metrics workflow
	ctx.signal(metrics::Pause {
		ts: util::timestamp::now(),
	})
	.to_workflow_id(metrics_workflow_id)
	.send()
	.await?;

	// We don't know the state of the previous generation of this actor if it becomes lost, send stop
	// command anyway in case it ended up allocating
	if let (StoppedVariant::Lost { .. }, Some(envoy)) = (&variant, state.transition.envoy()) {
		ctx.activity(InsertAndSendCommandsInput {
			generation: state.generation,
			envoy_key: envoy.envoy_key.clone(),
			commands: vec![protocol::Command::CommandStopActor(
				protocol::CommandStopActor {
					reason: protocol::StopActorReason::Lost,
				},
			)],
		})
		.await?;
	}

	enum Decision {
		Reallocate,
		Backoff,
		Sleep,
		Destroy,
	}

	let mut decision = match &state.transition {
		Transition::SleepIntent {
			rewake_after_stop: true,
			..
		} => Decision::Reallocate,
		Transition::SleepIntent {
			rewake_after_stop: false,
			..
		} => Decision::Sleep,
		Transition::GoingAway { .. } => Decision::Reallocate,
		Transition::Destroying { .. } => Decision::Destroy,
		_ => match variant {
			StoppedVariant::FailedAllocation => Decision::Backoff,
			// An actor stopping with `StopCode::Ok` indicates a graceful exit
			StoppedVariant::Stopped {
				code: protocol::StopCode::Ok,
				..
			} => Decision::Destroy,
			StoppedVariant::Stopped {
				code: protocol::StopCode::Error,
				..
			}
			| StoppedVariant::Lost { .. } => Decision::Sleep,
		},
	};

	// Check alarm
	if let (Decision::Sleep, Some(alarm_ts)) = (&decision, state.alarm_ts) {
		let now = ctx.activity(GetTsInput {}).await?;

		if now >= alarm_ts {
			state.alarm_ts = None;
			decision = Decision::Reallocate;
		}
	}

	let stopped_res = match decision {
		Decision::Reallocate => {
			let next_gen = state.generation + 1;
			let allocate_res = ctx
				.activity(AllocateInput {
					generation: next_gen,
				})
				.await?;

			if let Some(allocation) = allocate_res.allocation {
				state.generation += 1;

				match &allocation {
					Allocation::Serverless => {
						state.transition = Transition::Allocating {
							destroy_after_start: false,
							lost_timeout_ts: allocate_res.now
								+ ctx.config().pegboard().actor_allocation_threshold(),
						};
					}
					Allocation::Serverful { envoy_key: _ } => {
						state.transition = Transition::Starting {
							destroy_after_start: false,
							lost_timeout_ts: allocate_res.now
								+ ctx.config().pegboard().actor_start_threshold(),
						};
					}
				}

				ctx.activity(SendOutboundInput {
					generation: state.generation,
					input: input.input.clone(),
					allocation,
				})
				.await?;
			} else {
				// Transition to retry backoff
				state.transition = Transition::Reallocating {
					since_ts: allocate_res.now,
				};
			}

			StoppedResult::Continue
		}
		Decision::Backoff => {
			let now = ctx.activity(GetTsInput {}).await?;

			state.transition = Transition::Reallocating { since_ts: now };

			StoppedResult::Continue
		}
		Decision::Sleep => {
			// Transition to sleeping
			state.transition = Transition::Sleeping;

			StoppedResult::Continue
		}
		Decision::Destroy => StoppedResult::Destroy,
	};

	match state.transition {
		Transition::Sleeping | Transition::Reallocating { .. } => {
			ctx.activity(SetSleepingInput {}).await?;
		}
		_ => {}
	}

	ctx.msg(Stopped {})
		.topic(("actor_id", input.actor_id))
		.send()
		.await?;

	Ok(stopped_res)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetTsInput {}

#[activity(GetTs)]
pub async fn get_ts(ctx: &ActivityCtx, input: &GetTsInput) -> Result<i64> {
	Ok(util::timestamp::now())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetErrorInput {
	pub error: ActorError,
}

/// Sets the error on the actor workflow state.
#[activity(SetError)]
pub async fn set_error(ctx: &ActivityCtx, input: &SetErrorInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	// Envoy-related errors are never overwritten, as they represent the root cause of the failure
	// and should not be masked by subsequent errors like `Crashed`.
	if let Some(existing) = &state.error
		&& existing.is_envoy_failure()
	{
		tracing::debug!(
			?existing,
			new=?input.error,
			"preserving existing envoy failure error"
		);
		return Ok(());
	}

	state.error = Some(input.error.clone());
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
		.txn("pegboard_actor2_record_event_metrics", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				input.namespace_id,
				namespace::keys::metric::Metric::AlarmsSet(input.name.clone()),
				input.alarms_set as i64,
			);

			Ok(())
		})
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct CompareRetryInput {
	retry_count: usize,
	last_retry_ts: i64,
}

#[activity(CompareRetry)]
async fn compare_retry(
	ctx: &ActivityCtx,
	input: &CompareRetryInput,
) -> Result<(Option<i64>, i64, bool)> {
	let mut state = ctx.state::<State>()?;

	let now = util::timestamp::now();

	// If the last retry ts is more than `retry_reset_duration` ago, reset retry count
	let reset = input.last_retry_ts < now - ctx.config().pegboard().retry_reset_duration();

	if reset {
		state.reschedule_ts = None;
	} else {
		let backoff = util::throttle::Backoff::new_at(
			ctx.config().pegboard().reschedule_backoff_max_exponent(),
			None,
			ctx.config().pegboard().base_retry_timeout(),
			500,
			input.retry_count,
		);
		state.reschedule_ts = Some(now + i64::try_from(backoff.current_duration())?);
	}

	Ok((state.reschedule_ts, now, reset))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetConnectableInput {
	pub envoy_key: String,
	pub generation: u32,
}

#[activity(SetConnectable)]
pub async fn set_connectable(ctx: &ActivityCtx, input: &SetConnectableInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;
	let now = util::timestamp::now();

	// Record start duration metric. Technically measures all of:
	// - ups msg from actor wf (SendOutbound activity) -> pb envoy
	// - pb envoy forward msg -> ws
	// - network time from pb envoy -> envoy client
	// - envoy client startup procedure for actor
	// - network time from envoy client -> pb envoy
	// - ws msg from pb envoy -> actor demuxer
	// - actor demuxer send signal -> actor wf
	// - gas worker wake actor wf (if sleeping)
	// - actor wf recv signal
	// - this code here
	if let Some(allocate_ts) = state.allocate_ts {
		let pool_res = ctx
			.op(crate::ops::runner_config::get::Input {
				runners: vec![(state.namespace_id, state.pool_name.clone())],
				bypass_cache: false,
			})
			.await?;
		let pool = pool_res.into_iter().next();
		let is_serverless = pool
			.as_ref()
			.map(|pool| matches!(pool.config.kind, RunnerConfigKind::Serverless { .. }))
			.unwrap_or(false);

		let dt = now.saturating_sub(allocate_ts) as f64 / 1000.0;
		crate::metrics::ACTOR_START_DURATION
			.with_label_values(&[
				state.namespace_id.to_string().as_str(),
				state.pool_name.as_str(),
				if is_serverless {
					"serverless"
				} else {
					"serverful"
				},
			])
			.observe(dt);
	}

	if state.start_ts.is_none() {
		state.start_ts = Some(now);
	}
	state.connectable_ts = Some(now);
	state.envoy_key = Some(input.envoy_key.clone());

	let namespace_id = state.namespace_id;
	let actor_id = state.actor_id;
	ctx.udb()?
		.txn("pegboard_actor2_set_connectable", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			tx.write(&keys::actor::ConnectableKey::new(actor_id), ())?;
			tx.write(
				&keys::actor::EnvoyKeyKey::new(actor_id),
				input.envoy_key.clone(),
			)?;

			tx.atomic_op(
				&keys::envoy::SlotsKey::new(namespace_id, input.envoy_key.clone()),
				&1i64.to_le_bytes(),
				MutationType::Add,
			);
			// Insert actor into envoy list
			tx.write(
				&keys::envoy::ActorKey::new(namespace_id, input.envoy_key.clone(), actor_id),
				input.generation,
			)?;

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_set_connectable_tx"))
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetNotConnectableInput {}

/// Make the actor not connectable. It is not deallocated yet.
#[activity(SetNotConnectable)]
pub async fn set_not_connectable(ctx: &ActivityCtx, input: &SetNotConnectableInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	let actor_id = state.actor_id;
	ctx.udb()?
		.txn("pegboard_actor2_set_not_connectable", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			let connectable_key = keys::actor::ConnectableKey::new(actor_id);
			tx.clear(&keys::subspace().pack(&connectable_key));

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_set_not_connectable_tx"))
		.await?;

	state.connectable_ts = None;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetSleepingInput {}

/// Make the actor not connectable and set as sleeping. It is not deallocated yet.
#[activity(SetSleeping)]
pub async fn set_sleeping(ctx: &ActivityCtx, input: &SetSleepingInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;
	let now = util::timestamp::now();

	let actor_id = state.actor_id;
	ctx.udb()?
		.txn("pegboard_actor2_set_sleeping", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			// Make not connectable
			tx.delete(&keys::actor::ConnectableKey::new(actor_id));
			tx.write(&keys::actor::SleepTsKey::new(actor_id), now)?;

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_set_sleeping_tx"))
		.await?;

	state.sleep_ts = Some(now);
	state.connectable_ts = None;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeallocateInput {}

#[activity(Deallocate)]
pub async fn deallocate(ctx: &ActivityCtx, input: &DeallocateInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	let namespace_id = state.namespace_id;
	let acquired_slot = state.acquired_slot;
	let pool_name = &state.pool_name;
	let actor_id = state.actor_id;
	let envoy_key = &state.envoy_key;
	ctx.udb()?
		.txn("pegboard_actor2_deallocate", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			tx.delete(&keys::actor::ConnectableKey::new(actor_id));

			// Remove slot
			if acquired_slot {
				tx.delete(&keys::actor::EnvoyKeyKey::new(actor_id));
				tx.atomic_op(
					&keys::ns::ActorSlotsKey::new(namespace_id, pool_name.clone()),
					&(-1i64).to_le_bytes(),
					MutationType::Add,
				);

				if let Some(envoy_key) = envoy_key {
					tx.atomic_op(
						&keys::envoy::SlotsKey::new(namespace_id, envoy_key.clone()),
						&(-1i64).to_le_bytes(),
						MutationType::Add,
					);
					tx.delete(&keys::envoy::ActorKey::new(
						namespace_id,
						envoy_key.clone(),
						actor_id,
					));
				}
			}

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_deallocate_tx"))
		.await?;

	state.allocate_ts = None;
	state.connectable_ts = None;
	state.envoy_key = None;
	state.acquired_slot = false;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InsertAndSendCommandsInput {
	pub generation: u32,
	pub envoy_key: String,
	pub commands: Vec<protocol::Command>,
}

#[activity(InsertAndSendCommands)]
pub async fn insert_and_send_commands(
	ctx: &ActivityCtx,
	input: &InsertAndSendCommandsInput,
) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	insert_and_send_commands_inner(ctx, input, &mut state).await
}

async fn insert_and_send_commands_inner(
	ctx: &ActivityCtx,
	input: &InsertAndSendCommandsInput,
	state: &mut StateGuard<'_, State>,
) -> Result<()> {
	// This does not have to be part of its own activity because the txn is idempotent
	let old_last_command_idx = state.envoy_last_command_idx;
	let namespace_id = state.namespace_id;
	let actor_id = state.actor_id;
	ctx.udb()?
		.txn("pegboard_actor2_envoy_send_commands", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			for (i, command) in input.commands.iter().enumerate() {
				tx.write(
					&keys::envoy::ActorCommandKey::new(
						namespace_id,
						input.envoy_key.clone(),
						actor_id,
						input.generation,
						old_last_command_idx + i as i64 + 1,
					),
					match command {
						protocol::Command::CommandStartActor(x) => {
							protocol::ActorCommandKeyData::CommandStartActor(x.clone())
						}
						protocol::Command::CommandStopActor(x) => {
							protocol::ActorCommandKeyData::CommandStopActor(x.clone())
						}
					},
				)?;
			}

			tx.write(
				&keys::envoy::ActorLastCommandIdxKey::new(
					namespace_id,
					input.envoy_key.clone(),
					actor_id,
					input.generation,
				),
				old_last_command_idx + input.commands.len() as i64,
			)?;

			Ok(())
		})
		.await?;

	state.envoy_last_command_idx += input.commands.len() as i64;

	let receiver_subject = crate::pubsub_subjects::EnvoyReceiverSubject::new(
		state.namespace_id,
		input.envoy_key.clone(),
	);

	let message_serialized =
		versioned::ToEnvoyConn::wrap_latest(protocol::ToEnvoyConn::ToEnvoyCommands(
			input
				.commands
				.iter()
				.enumerate()
				.map(|(i, command)| protocol::CommandWrapper {
					checkpoint: protocol::ActorCheckpoint {
						actor_id: state.actor_id.to_string(),
						generation: input.generation,
						index: old_last_command_idx + i as i64 + 1,
					},
					inner: command.clone(),
				})
				.collect(),
		))
		.serialize_with_embedded_version(PROTOCOL_VERSION)?;

	ctx.ups()?
		.publish(&receiver_subject, &message_serialized, PublishOpts::one())
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendMessagesToEnvoyInput {
	// The reason we pass namespace id in the input is because reading from state is not allowed when
	// joining activities
	pub namespace_id: Id,
	pub envoy_key: String,
	pub messages: Vec<protocol::ToEnvoyConn>,
}

#[activity(SendMessagesToEnvoy)]
pub async fn send_messages_to_envoy(
	ctx: &ActivityCtx,
	input: &SendMessagesToEnvoyInput,
) -> Result<()> {
	let receiver_subject = crate::pubsub_subjects::EnvoyReceiverSubject::new(
		input.namespace_id,
		input.envoy_key.clone(),
	);

	for message in &input.messages {
		let message_serialized = versioned::ToEnvoyConn::wrap_latest(message.clone())
			.serialize_with_embedded_version(PROTOCOL_VERSION)?;

		ctx.ups()?
			.publish(&receiver_subject, &message_serialized, PublishOpts::one())
			.await?;
	}

	Ok(())
}
