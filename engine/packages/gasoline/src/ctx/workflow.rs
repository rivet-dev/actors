use std::{
	ops::Deref,
	sync::Arc,
	time::{Duration, Instant},
};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use opentelemetry::trace::SpanContext;
use rivet_util::Id;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{Mutex, watch};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::{
	activity::{Activity, ActivityInput},
	builder::{WorkflowRepr, workflow as builder},
	ctx::{ActivityCtx, ListenCtx, MessageCtx, VersionedWorkflowCtx},
	db::{BumpSubSubject, DatabaseHandle, PulledWorkflowData},
	error::{WorkflowError, WorkflowResult},
	executable::{AsyncResult, Executable},
	history::{
		History,
		cursor::{CheckVersionHistoryResult, Cursor, HistoryResult, RemovedHistoryResult},
		event::SleepState,
		location::Location,
		removed::Removed,
	},
	listen::Listen,
	message::Message,
	metrics,
	registry::RegistryHandle,
	signal::Signal,
	utils::time::{DurationToMillis, TsToMillis},
	workflow::{Workflow, WorkflowInput},
};

/// Retry interval for failed db actions
const DB_ACTION_RETRY: Duration = Duration::from_millis(150);
/// Most db action retries
const MAX_DB_ACTION_RETRIES: usize = 5;

// NOTE: Cloneable because of inner arcs
#[derive(Clone)]
pub struct WorkflowCtx {
	workflow_id: Id,
	/// Name of the workflow to run in the registry.
	name: String,
	create_ts: i64,
	ray_id: Id,
	version: usize,
	// Used for activity retry backoff
	wake_deadline_ts: Option<i64>,

	registry: RegistryHandle,
	db: DatabaseHandle,

	config: rivet_config::Config,
	pools: rivet_pools::Pools,
	cache: rivet_cache::Cache,

	/// Input data passed to this workflow.
	input: Arc<serde_json::value::RawValue>,
	/// Data that can be manipulated via activities over the course of the workflows entire lifetime.
	state: Arc<Mutex<Box<serde_json::value::RawValue>>>,
	/// All events that have ever been recorded on this workflow.
	event_history: History,
	cursor: Cursor,

	/// If this context is currently in a loop, this is the location of the where the loop started.
	loop_location: Option<Location>,

	msg_ctx: MessageCtx,
	/// Used to stop workflow execution by the worker.
	stop: watch::Receiver<()>,

	/// Whether or not this ctx is used as part of a .join
	parallelized: bool,
}

impl WorkflowCtx {
	#[tracing::instrument(skip_all, fields(workflow_id=%data.workflow_id, workflow_name=%data.workflow_name, ray_id=%data.ray_id))]
	pub fn new(
		registry: RegistryHandle,
		db: DatabaseHandle,
		config: rivet_config::Config,
		pools: rivet_pools::Pools,
		cache: rivet_cache::Cache,
		data: PulledWorkflowData,
		stop: watch::Receiver<()>,
	) -> Result<Self> {
		let msg_ctx = MessageCtx::new(&config, &pools, &cache, data.ray_id)?;
		let event_history = Arc::new(data.events);

		Ok(WorkflowCtx {
			workflow_id: data.workflow_id,
			name: data.workflow_name,
			create_ts: data.create_ts,
			ray_id: data.ray_id,
			version: 1,
			wake_deadline_ts: data.wake_deadline_ts,

			registry,
			db,

			config,
			pools,
			cache,

			input: Arc::from(data.input),
			state: Arc::new(Mutex::new(data.state)),

			event_history: event_history.clone(),
			cursor: Cursor::new(event_history, Location::empty()),
			loop_location: None,

			msg_ctx,
			stop,

			parallelized: false,
		})
	}

	/// Creates a workflow ctx reference with a given version.
	pub fn v(&mut self, version: usize) -> VersionedWorkflowCtx<'_> {
		VersionedWorkflowCtx::new(self, version)
	}

	/// Errors if the given version is less than the current version.
	pub(crate) fn compare_version(
		&self,
		step: impl std::fmt::Display,
		version: usize,
	) -> WorkflowResult<()> {
		if version < self.version {
			Err(WorkflowError::HistoryDiverged(format!(
				"version of {step} at {} is less than that of the current context (v{} < v{})",
				version,
				self.cursor.current_location(),
				self.version,
			)))
		} else {
			Ok(())
		}
	}

	#[tracing::instrument(name="workflow", skip_all, fields(workflow_id=%self.workflow_id, workflow_name=%self.name, ray_id=%self.ray_id))]
	pub(crate) async fn run(mut self, parent_span_ctx: SpanContext) -> WorkflowResult<()> {
		tracing::Span::current().add_link(parent_span_ctx);

		tracing::debug!("running workflow");

		// Check for stop before running
		self.check_stop()?;

		// Lookup workflow
		let workflow = self.registry.get_workflow(&self.name)?;
		let prune_variant = workflow.prune_variant;

		// Run workflow
		let mut res = (workflow.run)(&mut self).await;

		// Validate no leftover events
		if res.is_ok() {
			if let Err(err) = self.cursor().check_clear() {
				res = Err(err);
			}
		}

		match res {
			Ok(output) => {
				tracing::debug!("workflow completed");

				let mut retries = 0;
				let mut interval = tokio::time::interval(DB_ACTION_RETRY);
				interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

				// Retry loop
				loop {
					interval.tick().await;

					// Write output
					if let Err(err) = self
						.db
						.complete_workflow(self.workflow_id, &self.name, &output, prune_variant)
						.await
					{
						if retries > MAX_DB_ACTION_RETRIES {
							return Err(err);
						}
						retries += 1;
					} else {
						break;
					}
				}
			}
			Err(err) => {
				let wake_immediate = err.wake_immediate();

				// Retry the workflow if its recoverable
				let wake_deadline_ts = if let Some(deadline_ts) = err.deadline_ts() {
					Some(deadline_ts)
				} else {
					None
				};

				// These signals come from a `listen` call that did not receive any signals. The workflow will
				// be retried when a signal is published
				let wake_signals = err.signals();

				// This sub workflow comes from a `wait_for_workflow` call on a workflow that did not
				// finish. This workflow will be retried when the sub workflow completes
				let wake_sub_workflow = err.sub_workflow();

				let err_str = err.to_string();

				if err.is_recoverable() && !err.is_retryable() {
					tracing::debug!(?err, "workflow sleeping");
				} else {
					tracing::error!(?err, "workflow error");

					metrics::WORKFLOW_ERRORS
						.with_label_values(&[self.name.as_str(), err_str.as_str()])
						.inc();
				}

				let mut retries = 0;
				let mut interval = tokio::time::interval(DB_ACTION_RETRY);
				interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

				// Retry loop
				loop {
					interval.tick().await;

					// Write output
					let res = self
						.db
						.commit_workflow(
							self.workflow_id,
							&self.name,
							wake_immediate,
							wake_deadline_ts,
							wake_signals,
							wake_sub_workflow,
							&err_str,
						)
						.await;

					if let Err(err) = res {
						if retries > MAX_DB_ACTION_RETRIES {
							return Err(err);
						}
						retries += 1;
					} else {
						break;
					}
				}
			}
		}

		Ok(())
	}

	/// Run then handle the result of an activity.
	#[tracing::instrument(skip_all, fields(activity_name=%A::NAME, %location))]
	async fn run_activity<A: Activity>(
		&mut self,
		input: &A::Input,
		location: &Location,
		create_ts: i64,
	) -> WorkflowResult<A::Output> {
		tracing::debug!("running activity");

		let ctx = ActivityCtx::new(
			self.workflow_id,
			(*self
				.state
				.try_lock()
				.map_err(|_| WorkflowError::WorkflowStateInaccessible("should not be locked"))?)
			.to_owned(),
			self.db.clone(),
			&self.config,
			&self.pools,
			&self.cache,
			create_ts,
			self.ray_id,
			A::NAME,
			self.parallelized,
		)?;

		let start_instant = Instant::now();

		let res = tokio::time::timeout(A::TIMEOUT, A::run(&ctx, input).in_current_span())
			.await
			.map_err(|_| WorkflowError::ActivityTimeout(A::NAME, 0));

		let dt = start_instant.elapsed().as_secs_f64();

		match res {
			Ok(Ok(output)) => {
				tracing::debug!("activity success");

				// Write output
				let input_val = rivet_util::serde::json_to_raw_value!(input)
					.map_err(WorkflowError::SerializeActivityInput)?;
				let output_val = rivet_util::serde::json_to_raw_value!(&output)
					.map_err(WorkflowError::SerializeActivityOutput)?;

				tokio::try_join!(
					self.db.commit_workflow_activity_event(
						self.workflow_id,
						location,
						self.version,
						A::NAME,
						create_ts,
						&input_val,
						Ok(&output_val),
						self.loop_location(),
					),
					async {
						// Commit state if it was changed
						if let Some(new_workflow_state) = ctx.into_new_workflow_state() {
							let mut guard = self.state.try_lock().map_err(|_| {
								WorkflowError::WorkflowStateInaccessible("should not be locked")
							})?;

							self.db
								.update_workflow_state(self.workflow_id, &new_workflow_state)
								.await?;

							*guard = new_workflow_state;
						}

						Ok(())
					},
				)?;

				metrics::ACTIVITY_DURATION
					.with_label_values(&[self.name.as_str(), A::NAME, ""])
					.observe(dt);

				Ok(output)
			}
			Ok(Err(err)) => {
				tracing::error!(?err, "activity error");

				let err_str = err.to_string();
				let input_val = rivet_util::serde::json_to_raw_value!(input)
					.map_err(WorkflowError::SerializeActivityInput)?;

				// Write error (failed state)
				self.db
					.commit_workflow_activity_event(
						self.workflow_id,
						location,
						self.version,
						A::NAME,
						create_ts,
						&input_val,
						Err(&err_str),
						self.loop_location(),
					)
					.await?;

				// TODO: Temporarily don't record err to reduce metrics cardinality
				// if !is_recoverable {
				// 	metrics::ACTIVITY_ERRORS
				// 		.with_label_values(&[self.name.as_str(), A::NAME, err_str.as_str()])
				// 		.inc();
				// }
				let err_str = String::new();
				metrics::ACTIVITY_DURATION
					.with_label_values(&[self.name.as_str(), A::NAME, err_str.as_str()])
					.observe(dt);

				Err(WorkflowError::ActivityFailure(A::NAME, err, 0))
			}
			Err(err) => {
				tracing::debug!("activity timeout");

				let err_str = err.to_string();
				let input_val = rivet_util::serde::json_to_raw_value!(input)
					.map_err(WorkflowError::SerializeActivityInput)?;

				self.db
					.commit_workflow_activity_event(
						self.workflow_id,
						location,
						self.version,
						A::NAME,
						create_ts,
						&input_val,
						Err(&err_str),
						self.loop_location(),
					)
					.await?;

				// TODO: Temporarily don't record err to reduce metrics cardinality
				// metrics::ACTIVITY_ERRORS
				// 	.with_label_values(&[self.name.as_str(), A::NAME, err_str.as_str()])
				// 	.inc();
				let err_str = String::new();
				metrics::ACTIVITY_DURATION
					.with_label_values(&[self.name.as_str(), A::NAME, err_str.as_str()])
					.observe(dt);

				Err(err)
			}
		}
	}

	#[tracing::instrument(skip_all)]
	pub(crate) fn set_parallelized(&mut self) {
		self.parallelized = true;
	}

	/// Creates a new workflow run with one more depth in the location.
	/// - **Not to be used directly by workflow users. For implementation uses only.**
	/// - **Remember to validate latent history after this branch is used.**
	#[tracing::instrument(skip_all)]
	pub async fn branch(&mut self) -> WorkflowResult<Self> {
		self.custom_branch(self.input.clone(), self.version).await
	}

	#[tracing::instrument(skip_all, fields(version))]
	pub(crate) async fn custom_branch(
		&mut self,
		input: Arc<serde_json::value::RawValue>,
		version: usize,
	) -> WorkflowResult<Self> {
		let history_res = self.cursor.compare_branch(version)?;
		let location = self.cursor.current_location_for(&history_res);

		// Validate history is consistent
		if !matches!(history_res, HistoryResult::Event(_)) {
			self.db
				.commit_workflow_branch_event(
					self.workflow_id,
					&location,
					version,
					self.loop_location.as_ref(),
				)
				.await?;
		}

		Ok(self.branch_inner(input, version, location))
	}

	/// `custom_branch` with no history validation.
	pub(crate) fn branch_inner(
		&mut self,
		input: Arc<serde_json::value::RawValue>,
		version: usize,
		location: Location,
	) -> WorkflowCtx {
		WorkflowCtx {
			workflow_id: self.workflow_id,
			name: self.name.clone(),
			create_ts: self.create_ts,
			ray_id: self.ray_id,
			version,
			wake_deadline_ts: self.wake_deadline_ts,

			registry: self.registry.clone(),
			db: self.db.clone(),

			config: self.config.clone(),
			pools: self.pools.clone(),
			cache: self.cache.clone(),

			input,
			state: self.state.clone(),

			event_history: self.event_history.clone(),
			cursor: Cursor::new(self.event_history.clone(), location),
			loop_location: self.loop_location.clone(),

			msg_ctx: self.msg_ctx.clone(),
			stop: self.stop.clone(),

			parallelized: self.parallelized,
		}
	}

	/// Like `branch` but it does not add another layer of depth.
	pub fn step(&mut self) -> Self {
		let branch = self.clone();

		self.cursor.inc();

		branch
	}

	pub(crate) fn check_stop(&self) -> WorkflowResult<()> {
		if self.stop.has_changed().unwrap_or(true) {
			Err(WorkflowError::WorkflowEvicted)
		} else {
			Ok(())
		}
	}

	pub(crate) async fn wait_stop(&self) -> WorkflowResult<()> {
		// We have to clone here because this function can't have a mutable reference to self. The state of
		// the stop channel doesn't matter because it only ever receives one message
		let _ = self.stop.clone().changed().await;
		Err(WorkflowError::WorkflowEvicted)
	}
}

impl WorkflowCtx {
	/// Creates a sub workflow builder.
	pub fn workflow<I>(
		&mut self,
		input: impl WorkflowRepr<I>,
	) -> builder::sub_workflow::SubWorkflowBuilder<'_, impl WorkflowRepr<I>, I>
	where
		I: WorkflowInput,
		<I as WorkflowInput>::Workflow: Workflow<Input = I>,
	{
		builder::sub_workflow::SubWorkflowBuilder::new(self, self.version, input)
	}

	/// Run activity. Will replay on failure.
	#[tracing::instrument(skip_all, fields(activity_name=%I::Activity::NAME))]
	pub async fn activity<I>(
		&mut self,
		input: I,
	) -> Result<<<I as ActivityInput>::Activity as Activity>::Output>
	where
		I: ActivityInput,
		<I as ActivityInput>::Activity: Activity<Input = I>,
	{
		self.check_stop()?;

		let history_res = self
			.cursor
			.compare_activity(self.version, I::Activity::NAME)?;
		let location = self.cursor.current_location_for(&history_res);

		// Activity was ran before
		let output = if let HistoryResult::Event(activity) = history_res {
			tracing::debug!("replaying activity");

			// Activity succeeded
			if let Some(output) = activity.parse_output()? {
				output
			}
			// Activity failed, retry
			else {
				let error_count = activity.error_count;

				// Backoff
				if let Some(wake_deadline_ts) = self.wake_deadline_ts {
					tracing::debug!("sleeping for activity backoff");

					let duration = (u64::try_from(wake_deadline_ts)?)
						.saturating_sub(u64::try_from(rivet_util::timestamp::now())?);
					tokio::time::sleep(Duration::from_millis(duration))
						.instrument(tracing::info_span!("backoff_sleep"))
						.await;
				}

				match self
					.run_activity::<I::Activity>(&input, &location, activity.create_ts)
					.await
				{
					Err(err) => {
						// Convert error in the case of max retries exceeded. This will only act on retryable
						// errors
						let err = match err {
							WorkflowError::ActivityFailure(name, err, _) => {
								if error_count.saturating_add(1) >= I::Activity::MAX_RETRIES {
									WorkflowError::ActivityMaxFailuresReached(name, err)
								} else {
									// Add error count to the error for backoff calculation
									WorkflowError::ActivityFailure(name, err, error_count)
								}
							}
							WorkflowError::ActivityTimeout(name, _) => {
								if error_count.saturating_add(1) >= I::Activity::MAX_RETRIES {
									WorkflowError::ActivityMaxFailuresReached(name, err.into())
								} else {
									// Add error count to the error for backoff calculation
									WorkflowError::ActivityTimeout(name, error_count)
								}
							}
							WorkflowError::OperationTimeout(op_name, _) => {
								if error_count.saturating_add(1) >= I::Activity::MAX_RETRIES {
									WorkflowError::ActivityMaxFailuresReached(
										I::Activity::NAME,
										err.into(),
									)
								} else {
									// Add error count to the error for backoff calculation
									WorkflowError::OperationTimeout(op_name, error_count)
								}
							}
							_ => err,
						};

						return Err(err.into());
					}
					x => x?,
				}
			}
		}
		// This is a new activity
		else {
			self.run_activity::<I::Activity>(&input, &location, rivet_util::timestamp::now())
				.await?
		};

		// Move to next event
		self.cursor.update(&location);

		Ok(output)
	}

	/// Joins multiple executable actions (activities, closures) and awaits them simultaneously. This does not
	/// short circuit in the event of an error to make sure activity side effects are recorded.
	#[tracing::instrument(skip_all)]
	pub async fn join<T: Executable>(&mut self, exec: T) -> Result<T::Output> {
		self.check_stop()?;

		exec.execute(self).await
	}

	// TODO: Replace with some method on WorkflowError
	// /// Tests if the given error is unrecoverable. If it is, allows the user to run recovery code safely.
	// /// Should always be used when trying to handle activity errors manually.
	// #[tracing::instrument(skip_all)]
	// pub fn catch_unrecoverable<T>(&mut self, res: Result<T>) -> Result<Result<T>> {
	// 	match res {
	// 		Err(err) => {
	// 			// TODO: This should check .chain() for the error
	// 			match err.downcast::<WorkflowError>() {
	// 				Ok(inner_err) => {
	// 					// Despite "history diverged" errors being unrecoverable, they should not have be returned
	// 					// by this function because the state of the history is already messed up and no new
	// 					// workflow items should be run.
	// 					if !inner_err.is_recoverable()
	// 						&& !matches!(inner_err, WorkflowError::HistoryDiverged(_))
	// 					{
	// 						self.cursor.inc();

	// 						Ok(Err(inner_err.into()))
	// 					} else {
	// 						Err(inner_err.into())
	// 					}
	// 				}
	// 				Err(err) => Err(err),
	// 			}
	// 		}
	// 		Ok(x) => Ok(Ok(x)),
	// 	}
	// }

	/// Creates a signal builder.
	pub fn signal<T: Signal + Serialize>(
		&mut self,
		body: T,
	) -> builder::signal::SignalBuilder<'_, T> {
		builder::signal::SignalBuilder::new(self, self.version, body)
	}

	/// Listens for a signal for a short time before setting the workflow to sleep. Once the signal is
	/// received, the workflow will be woken up and continue.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>()))]
	pub async fn listen<T: Listen>(&mut self) -> Result<T> {
		let signals = self.listen_n::<T>(1).in_current_span().await?;

		signals
			.into_iter()
			.next()
			.context("must return at least 1 signal")
	}

	/// Listens for a N signals for a short time before setting the workflow to sleep. Once signals are
	/// received, the workflow will be woken up and continue. Never returns an empty vec.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>()))]
	pub async fn listen_n<T: Listen>(&mut self, limit: usize) -> Result<Vec<T>> {
		self.check_stop()?;

		let history_res = self.cursor.compare_signals(self.version)?;
		let location = self.cursor.current_location_for(&history_res);

		// Signals received before
		let signals = if let HistoryResult::Event(signals) = history_res {
			tracing::debug!(
				count=%signals.names.len(),
				"replaying signals"
			);

			signals
				.names
				.iter()
				.zip(&signals.bodies)
				.map(|(name, body)| T::parse(name, &body))
				.collect::<std::result::Result<Vec<_>, _>>()?
		}
		// Listen for new signals
		else {
			tracing::debug!("listening for signals");

			let mut bump_sub = self
				.db
				.bump_sub(BumpSubSubject::SignalPublish {
					to_workflow_id: self.workflow_id,
				})
				.await?;
			let mut retries = self.db.max_signal_poll_retries();
			let mut interval = tokio::time::interval(self.db.signal_poll_interval());
			interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

			// Skip first tick, we wait after the db call instead of before
			interval.tick().await;

			let mut ctx = ListenCtx::new(self, &location);
			let mut next_wakeup_at = None;

			loop {
				let wakeup_to_pull = next_wakeup_at.take().map(|wakeup_at: Instant| {
					let duration = wakeup_at.elapsed();
					metrics::SIGNAL_WAKEUP_TO_PULL_SECONDS
						.with_label_values(&[self.name()])
						.observe(duration.as_secs_f64());
					duration
				});
				ctx.reset(retries == 0, wakeup_to_pull);

				match T::listen(&mut ctx, limit).in_current_span().await {
					Ok(res) => break res,
					Err(err) if matches!(err, WorkflowError::NoSignalFound(_)) => {
						if retries == 0 {
							return Err(err.into());
						}
						retries -= 1;
					}
					Err(err) => return Err(err.into()),
				}

				// Poll and wait for a wake at the same time
				next_wakeup_at = tokio::select! {
					_ = bump_sub.next() => Some(Instant::now()),
					_ = interval.tick() => Some(Instant::now()),
					res = self.wait_stop() => {
						res?;
						None
					},
				};
			}
		};

		// Move to next event
		self.cursor.update(&location);

		Ok(signals)
	}

	/// Creates a message builder.
	pub fn msg<M: Message>(&mut self, body: M) -> builder::message::MessageBuilder<'_, M> {
		builder::message::MessageBuilder::new(self, self.version, body)
	}

	/// Runs workflow steps in a loop. If you need side causes, use `WorkflowCtx::loope`.
	#[tracing::instrument(skip_all)]
	pub async fn repeat<F, T>(&mut self, mut cb: F) -> Result<T>
	where
		F: for<'a> FnMut(&'a mut WorkflowCtx) -> AsyncResult<'a, Loop<T>>,
		T: Serialize + DeserializeOwned,
	{
		builder::lupe::LoopBuilder::run(builder::lupe::LoopBuilder::new(self, ()), |ctx, _| cb(ctx))
			.await
	}

	/// Runs workflow steps in a loop with state.
	#[tracing::instrument(skip_all)]
	pub async fn loope<S, F, T>(&mut self, state: S, cb: F) -> Result<T>
	where
		S: Serialize + DeserializeOwned,
		F: for<'a> FnMut(&'a mut WorkflowCtx, &'a mut S) -> AsyncResult<'a, Loop<T>>,
		T: Serialize + DeserializeOwned,
	{
		builder::lupe::LoopBuilder::new(self, state).run(cb).await
	}

	pub fn lupe(&mut self) -> builder::lupe::LoopBuilder<'_, ()> {
		builder::lupe::LoopBuilder::new(self, ())
	}

	#[tracing::instrument(skip_all)]
	pub async fn sleep(&mut self, duration: impl DurationToMillis) -> Result<()> {
		let ts = rivet_util::timestamp::now() as u64 + duration.to_millis()?;

		self.sleep_until(ts as i64).await
	}

	#[tracing::instrument(skip_all, fields(duration))]
	pub async fn sleep_until(&mut self, time: impl TsToMillis) -> Result<()> {
		self.check_stop()?;

		let history_res = self.cursor.compare_sleep(self.version)?;
		let location = self.cursor.current_location_for(&history_res);

		// Slept before
		let (deadline_ts, replay) = if let HistoryResult::Event(sleep) = history_res {
			tracing::debug!("replaying sleep");

			(sleep.deadline_ts, true)
		}
		// Sleep
		else {
			let deadline_ts = time.to_millis()?;

			self.db
				.commit_workflow_sleep_event(
					self.workflow_id,
					&location,
					self.version,
					deadline_ts,
					self.loop_location(),
				)
				.await?;

			(deadline_ts, false)
		};

		let duration = deadline_ts.saturating_sub(rivet_util::timestamp::now());
		tracing::Span::current().record("duration", &duration);

		// No-op
		if duration <= 0 {
			if !replay && duration < -25 {
				tracing::warn!(%duration, "tried to sleep for a negative duration");
			}
		}
		// Sleep in memory if duration is shorter than the worker tick
		else if duration < self.db.worker_poll_interval().as_millis() as i64 + 1 {
			tracing::debug!(%deadline_ts, "sleeping in memory");

			tokio::select! {
				_ = tokio::time::sleep(Duration::from_millis(duration.try_into()?)) => {},
				res = self.wait_stop() => res?,
			}
		}
		// Workflow sleep
		else {
			tracing::debug!(%deadline_ts, "sleeping");

			return Err(WorkflowError::Sleep(deadline_ts).into());
		}

		// Move to next event
		self.cursor.update(&location);

		Ok(())
	}

	/// Listens for a signal with a timeout. Returns `None` if the timeout is reached.
	///
	/// Internally this is a sleep event and a signal event.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>()))]
	pub async fn listen_with_timeout<T: Listen>(
		&mut self,
		duration: impl DurationToMillis,
	) -> Result<Option<T>> {
		let signals = self.listen_n_with_timeout(duration, 1).await?;

		Ok(signals.into_iter().next())
	}

	/// Listens for signals with a timeout. Returns an empty vec if the timeout is reached.
	///
	/// Internally this is a sleep event and a signals event.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>()))]
	pub async fn listen_n_with_timeout<T: Listen>(
		&mut self,
		duration: impl DurationToMillis,
		limit: usize,
	) -> Result<Vec<T>> {
		let time = (rivet_util::timestamp::now() as u64 + duration.to_millis()?) as i64;

		self.listen_n_until(time, limit).await
	}

	/// Listens for a signal until the given timestamp. Returns `None` if the timestamp is reached.
	///
	/// Internally this is a sleep event and a signals event.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>(), duration))]
	pub async fn listen_until<T: Listen>(&mut self, time: impl TsToMillis) -> Result<Option<T>> {
		let signals = self.listen_n_until(time, 1).await?;

		Ok(signals.into_iter().next())
	}

	/// Listens for signals until the given timestamp. Returns an empty vec if the timestamp is reached.
	///
	/// Internally this is a sleep event and a signal event.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>(), duration))]
	pub async fn listen_n_until<T: Listen>(
		&mut self,
		time: impl TsToMillis,
		limit: usize,
	) -> Result<Vec<T>> {
		self.check_stop()?;

		let history_res = self.cursor.compare_sleep(self.version)?;
		let history_res2 = history_res.equivalent();
		let sleep_location = self.cursor.current_location_for(&history_res);

		// Slept before
		let (deadline_ts, state) = if let HistoryResult::Event(sleep) = history_res {
			tracing::debug!("replaying sleep");

			(sleep.deadline_ts, sleep.state)
		}
		// Sleep
		else {
			let deadline_ts = TsToMillis::to_millis(time)?;

			self.db
				.commit_workflow_sleep_event(
					self.workflow_id,
					&sleep_location,
					self.version,
					deadline_ts,
					self.loop_location(),
				)
				.await?;

			(deadline_ts, SleepState::Normal)
		};

		// Move to next event
		self.cursor.update(&sleep_location);

		// Signals received before
		if matches!(state, SleepState::Interrupted) {
			let history_res = self.cursor.compare_signals(self.version)?;
			let signals_location = self.cursor.current_location_for(&history_res);

			if let HistoryResult::Event(signals) = history_res {
				tracing::debug!(
					count=?signals.names.len(),
					"replaying signals",
				);

				let signals = signals
					.names
					.iter()
					.zip(&signals.bodies)
					.map(|(name, body)| T::parse(name, &body))
					.collect::<std::result::Result<Vec<_>, _>>()?;

				// Move to next event
				self.cursor.update(&signals_location);

				// Short circuit
				return Ok(signals);
			} else {
				return Err(WorkflowError::HistoryDiverged(format!(
					"expected signals at {}, found nothing",
					signals_location,
				))
				.into());
			}
		}

		// Location of the signals event (comes after the sleep event)
		let signals_location = self.cursor.current_location_for(&history_res2);
		let duration = deadline_ts.saturating_sub(rivet_util::timestamp::now());
		tracing::Span::current().record("duration", &duration);

		// Duration is now 0, timeout is over
		let signals = if duration <= 0 {
			// After timeout is over, check once for signals
			if matches!(state, SleepState::Normal) {
				let mut ctx = ListenCtx::new_with_sleep(self, &signals_location, &sleep_location);

				match T::listen(&mut ctx, limit).in_current_span().await {
					Ok(x) => x,
					Err(WorkflowError::NoSignalFound(_)) => Vec::new(),
					Err(err) => return Err(err.into()),
				}
			} else {
				Vec::new()
			}
		}
		// Sleep in memory if duration is shorter than the worker tick
		else if duration < self.db.worker_poll_interval().as_millis() as i64 + 1 {
			tracing::debug!(%deadline_ts, "sleeping in memory");

			let res = tokio::time::timeout(
				Duration::from_millis(duration.try_into()?),
				(async {
					tracing::debug!("listening for signals with timeout");

					let mut bump_sub = self
						.db
						.bump_sub(BumpSubSubject::SignalPublish {
							to_workflow_id: self.workflow_id,
						})
						.await?;
					let mut interval = tokio::time::interval(self.db.signal_poll_interval());
					interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

					// Skip first tick, we wait after the db call instead of before
					interval.tick().await;

					let mut ctx =
						ListenCtx::new_with_sleep(self, &signals_location, &sleep_location);
					let mut next_wakeup_at = None;

					loop {
						let wakeup_to_pull = next_wakeup_at.take().map(|wakeup_at: Instant| {
							let duration = wakeup_at.elapsed();
							metrics::SIGNAL_WAKEUP_TO_PULL_SECONDS
								.with_label_values(&[self.name()])
								.observe(duration.as_secs_f64());
							duration
						});
						ctx.reset(false, wakeup_to_pull);

						match T::listen(&mut ctx, limit).in_current_span().await {
							// Retry
							Err(WorkflowError::NoSignalFound(_)) => {}
							x => return x,
						}

						// Poll and wait for a wake at the same time
						next_wakeup_at = tokio::select! {
							_ = bump_sub.next() => Some(Instant::now()),
							_ = interval.tick() => Some(Instant::now()),
							res = self.wait_stop() => {
								res?;
								None
							},
						};
					}
				})
				.in_current_span(),
			)
			.await;

			match res {
				Ok(res) => res?,
				Err(_) => {
					tracing::debug!("timed out listening for signals");

					Vec::new()
				}
			}
		}
		// Workflow sleep for long durations
		else {
			tracing::debug!("listening for signals with timeout");

			let mut bump_sub = self
				.db
				.bump_sub(BumpSubSubject::SignalPublish {
					to_workflow_id: self.workflow_id,
				})
				.await?;
			let mut retries = self.db.max_signal_poll_retries();
			let mut interval = tokio::time::interval(self.db.signal_poll_interval());
			interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

			// Skip first tick, we wait after the db call instead of before
			interval.tick().await;

			let mut ctx = ListenCtx::new_with_sleep(self, &signals_location, &sleep_location);
			let mut next_wakeup_at = None;

			loop {
				let wakeup_to_pull = next_wakeup_at.take().map(|wakeup_at: Instant| {
					let duration = wakeup_at.elapsed();
					metrics::SIGNAL_WAKEUP_TO_PULL_SECONDS
						.with_label_values(&[self.name()])
						.observe(duration.as_secs_f64());
					duration
				});
				ctx.reset(retries == 0, wakeup_to_pull);

				match T::listen(&mut ctx, limit).in_current_span().await {
					Ok(res) => break res,
					Err(WorkflowError::NoSignalFound(signals)) => {
						if retries == 0 {
							return Err(
								WorkflowError::NoSignalFoundAndSleep(signals, deadline_ts).into()
							);
						}
						retries -= 1;
					}
					Err(err) => return Err(err.into()),
				}

				// Poll and wait for a wake at the same time
				next_wakeup_at = tokio::select! {
					_ = bump_sub.next() => Some(Instant::now()),
					_ = interval.tick() => Some(Instant::now()),
					res = self.wait_stop() => {
						res?;
						None
					},
				};
			}
		};

		// Update sleep state
		if !signals.is_empty() {
			// Move to next event, sleep state already updated by pull signals tx
			self.cursor.update(&signals_location);
		} else if matches!(state, SleepState::Normal) {
			self.db
				.update_workflow_sleep_event_state(
					self.workflow_id,
					&sleep_location,
					SleepState::Uninterrupted,
				)
				.await?;
		}

		Ok(signals)
	}

	/// Represents a removed workflow step.
	#[tracing::instrument(skip_all, fields(t=std::any::type_name::<T>()))]
	pub async fn removed<T: Removed>(&mut self) -> Result<()> {
		self.check_stop()?;

		match self.cursor.compare_removed::<T>() {
			RemovedHistoryResult::New => {
				tracing::debug!("inserting removed step");

				self.db
					.commit_workflow_removed_event(
						self.workflow_id,
						&self.cursor.current_location(),
						T::event_type(),
						T::name(),
						self.loop_location(),
					)
					.await?;

				// Move to next event
				self.cursor.inc();
			}
			RemovedHistoryResult::Skip => {
				tracing::debug!("skipping removed step");

				// Move to next event
				self.cursor.inc();
			}
			RemovedHistoryResult::Ignore(msg) => {
				tracing::debug!(
					"removed event filter doesn't match existing event, ignoring: {msg}"
				);
			}
		}

		Ok(())
	}

	/// Returns the version of the current event in history. If no event exists, returns `current_version` and
	/// inserts a version check event.
	#[tracing::instrument(skip_all, fields(latest_version))]
	pub async fn check_version(&mut self, latest_version: usize) -> Result<usize> {
		self.check_stop()?;

		if latest_version == 0 {
			return Err(WorkflowError::InvalidVersion(
				"version for `check_version` must be greater than 0".into(),
			)
			.into());
		}

		let history_res = self.cursor.compare_version_check()?;
		let event_location = self.cursor.current_location_for(&history_res.equivalent());

		let (event_version, checked_version) = match history_res {
			CheckVersionHistoryResult::New => (Some(self.version), latest_version),
			CheckVersionHistoryResult::Event(checked_version) => (None, checked_version),
			CheckVersionHistoryResult::Insertion(next_event_version) => {
				(Some(next_event_version + 1), next_event_version)
			}
		};

		if let Some(event_version) = event_version {
			tracing::debug!("inserting version check");

			self.db
				.commit_workflow_version_check_event(
					self.workflow_id,
					&event_location,
					event_version + self.version - 1,
					checked_version,
					self.loop_location(),
				)
				.await?;
		}

		// Move to next event
		self.cursor.update(&event_location);

		Ok(checked_version + 1 - self.version)
	}
}

impl WorkflowCtx {
	pub(crate) fn input(&self) -> &Arc<serde_json::value::RawValue> {
		&self.input
	}

	pub(crate) fn loop_location(&self) -> Option<&Location> {
		self.loop_location.as_ref()
	}

	pub(crate) fn set_loop_location(&mut self, loop_location: Location) {
		self.loop_location = Some(loop_location);
	}

	pub(crate) fn db(&self) -> &DatabaseHandle {
		&self.db
	}

	pub(crate) fn msg_ctx(&self) -> &MessageCtx {
		&self.msg_ctx
	}

	pub(crate) fn cursor(&self) -> &Cursor {
		&self.cursor
	}

	pub(crate) fn cursor_mut(&mut self) -> &mut Cursor {
		&mut self.cursor
	}

	pub fn name(&self) -> &str {
		&self.name
	}

	pub fn workflow_id(&self) -> Id {
		self.workflow_id
	}

	pub fn ray_id(&self) -> Id {
		self.ray_id
	}

	// Not public because this only denotes the version of the context, use `check_version` instead.
	pub(crate) fn version(&self) -> usize {
		self.version
	}

	pub(crate) fn set_version(&mut self, version: usize) {
		self.version = version;
	}

	/// Timestamp at which the workflow was created.
	pub fn create_ts(&self) -> i64 {
		self.create_ts
	}

	pub fn pools(&self) -> &rivet_pools::Pools {
		&self.pools
	}

	pub fn cache(&self) -> &rivet_cache::Cache {
		&self.cache
	}

	pub fn config(&self) -> &rivet_config::Config {
		&self.config
	}
}

impl Deref for WorkflowCtx {
	type Target = rivet_pools::Pools;

	fn deref(&self) -> &Self::Target {
		&self.pools
	}
}

pub enum Loop<T> {
	Continue,
	Break(T),
}
