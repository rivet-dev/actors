use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};
use tracing::Instrument;

use crate::{
	ctx::WorkflowCtx,
	ctx::workflow::Loop,
	error::WorkflowError,
	executable::AsyncResult,
	history::{cursor::HistoryResult, location::Coordinate},
	metrics,
};

/// How often to commit loop event data to db and mark previous loop history to forgotten
const DEFAULT_LOOP_COMMIT_INTERVAL: usize = 20;

pub struct LoopBuilder<'a, S> {
	ctx: &'a mut WorkflowCtx,
	state: S,
	commit_interval: Option<usize>,
}

impl<'a, S: Serialize + DeserializeOwned> LoopBuilder<'a, S> {
	pub(crate) fn new(ctx: &'a mut WorkflowCtx, state: S) -> Self {
		LoopBuilder {
			ctx,
			state,
			commit_interval: None,
		}
	}

	pub fn with_state<S2: Serialize + DeserializeOwned>(self, state: S2) -> LoopBuilder<'a, S2> {
		LoopBuilder {
			ctx: self.ctx,
			state,
			commit_interval: self.commit_interval,
		}
	}

	pub fn commit_interval(self, commit_interval: usize) -> Self {
		LoopBuilder {
			ctx: self.ctx,
			state: self.state,
			commit_interval: Some(commit_interval),
		}
	}

	#[tracing::instrument(skip_all)]
	pub async fn run<F, T>(self, mut cb: F) -> Result<T>
	where
		F: for<'b> FnMut(&'b mut WorkflowCtx, &'b mut S) -> AsyncResult<'b, Loop<T>>,
		T: Serialize + DeserializeOwned,
	{
		let LoopBuilder {
			ctx,
			state,
			commit_interval,
		} = self;

		ctx.check_stop()?;

		let history_res = ctx.cursor().compare_loop(ctx.version())?;
		let loop_location = ctx.cursor().current_location_for(&history_res);

		// Loop existed before
		let (mut iteration, mut state, output, mut loop_event_init_fut) =
			if let HistoryResult::Event(loop_event) = history_res {
				let state = loop_event.parse_state()?;
				let output = loop_event.parse_output()?;

				(loop_event.iteration, state, output, None)
			} else {
				let state_val = rivet_util::serde::json_to_raw_value!(&state)
					.map_err(WorkflowError::SerializeLoopOutput)?;

				// Clone data to move into future
				let loop_location = loop_location.clone();
				let db2 = ctx.db().clone();
				let workflow_id = ctx.workflow_id();
				let name = ctx.name().to_string();
				let version = ctx.version();
				let nested_loop_location = ctx.loop_location().cloned();

				// This future is deferred until later for parallelization
				let loop_event_init_fut = async move {
					db2.upsert_workflow_loop_event(
						workflow_id,
						&name,
						&loop_location,
						version,
						0,
						&state_val,
						None,
						nested_loop_location.as_ref(),
					)
					.await
				};

				(0, state, None, Some(loop_event_init_fut))
			};

		// Create a branch for the loop event
		let mut loop_branch =
			ctx.branch_inner(ctx.input().clone(), ctx.version(), loop_location.clone());

		// Loop complete
		let output = if let Some(output) = output {
			tracing::debug!("replaying loop output");

			output
		}
		// Run loop
		else {
			tracing::debug!("running loop");

			// Used to defer loop upsertion for parallelization
			let mut loop_event_upsert_fut = None;
			let mut iteration_dt = Duration::ZERO;

			loop {
				ctx.check_stop()?;

				// Create a new branch for each iteration of the loop at location {...loop location, iteration idx}
				let mut iteration_branch = loop_branch.branch_inner(
					ctx.input().clone(),
					ctx.version(),
					loop_branch
						.cursor()
						.root()
						.join(Coordinate::simple(iteration + 1)),
				);
				let iteration_branch_root = iteration_branch.cursor().root().clone();

				// Set branch loop location to the current loop
				iteration_branch.set_loop_location(loop_location.clone());

				let previous_iteration = iteration;

				// Async block for instrumentation purposes
				let res = async {
					let db2 = ctx.db().clone();

					// NOTE: Great care has been taken to optimize this function. This join allows multiple
					// txns to run simultaneously instead of in series but is hard to read.
					//
					// 1. First (but not necessarily chronologically first because its parallelized), we
					//    commit the loop event. This only happens on the first iteration of the loop
					// 2. Second, we commit the branch event for the current iteration
					// 3. Third, we run the user's loop code
					// 4. Last, if we have to upsert the loop event, we save the future and poll it in the
					//    next iteration of the loop as part of this join
					let (
						loop_event_commit_res,
						loop_event_upsert_res,
						branch_commit_res,
						(loop_res, cb_dt),
					) = tokio::join!(
						async {
							if let Some(loop_event_init_fut) = loop_event_init_fut.take() {
								loop_event_init_fut.await
							} else {
								Ok(())
							}
						},
						async {
							if let Some(loop_event_upsert_fut) = loop_event_upsert_fut.take() {
								let start_instant = Instant::now();
								loop_event_upsert_fut.await?;
								metrics::LOOP_COMMIT_DURATION
									.with_label_values(&[&ctx.name().to_string()])
									.observe(start_instant.elapsed().as_secs_f64());
							}

							anyhow::Ok(())
						},
						async {
							// Insert event if iteration is not a replay
							if !loop_branch.cursor().compare_loop_branch(iteration)? {
								db2.commit_workflow_branch_event(
									ctx.workflow_id(),
									&iteration_branch_root,
									ctx.version(),
									Some(&loop_location),
								)
								.await?;

								// Only record iteration duration if its not a replay
								metrics::LOOP_ITERATION_DURATION
									.with_label_values(&[&ctx.name().to_string()])
									.observe(iteration_dt.as_secs_f64());
							}

							anyhow::Ok(())
						},
						async {
							let iteration_start_instant = Instant::now();

							(
								cb(&mut iteration_branch, &mut state).await,
								iteration_start_instant.elapsed(),
							)
						}
					);

					loop_event_commit_res?;
					loop_event_upsert_res?;
					branch_commit_res?;

					iteration_dt = cb_dt;

					// Run loop
					match loop_res? {
						Loop::Continue => {
							iteration += 1;

							// Commit workflow state to db
							if iteration % commit_interval.unwrap_or(DEFAULT_LOOP_COMMIT_INTERVAL)
								== 0
							{
								let state_val = rivet_util::serde::json_to_raw_value!(&state)
									.map_err(WorkflowError::SerializeLoopOutput)?;

								// Clone data to move into future
								let loop_location = loop_location.clone();
								let db2 = ctx.db().clone();
								let workflow_id = ctx.workflow_id();
								let name = ctx.name();
								let version = ctx.version();
								let nested_loop_location = ctx.loop_location().cloned();

								// Defer upsertion to next iteration so it runs in parallel
								loop_event_upsert_fut = Some(async move {
									db2.upsert_workflow_loop_event(
										workflow_id,
										&name,
										&loop_location,
										version,
										iteration,
										&state_val,
										None,
										nested_loop_location.as_ref(),
									)
									.await
								});
							}

							anyhow::Ok(None)
						}
						Loop::Break(res) => {
							iteration += 1;

							let state_val = rivet_util::serde::json_to_raw_value!(&state)
								.map_err(WorkflowError::SerializeLoopOutput)?;
							let output_val = rivet_util::serde::json_to_raw_value!(&res)
								.map_err(WorkflowError::SerializeLoopOutput)?;

							// Commit loop output and final state to db. Note that we don't defer this because
							// there will be no more loop iterations afterwards.
							ctx.db()
								.upsert_workflow_loop_event(
									ctx.workflow_id(),
									&ctx.name(),
									&loop_location,
									ctx.version(),
									iteration,
									&state_val,
									Some(&output_val),
									ctx.loop_location(),
								)
								.await?;

							Ok(Some(res))
						}
					}
				}
				.instrument(tracing::info_span!("iteration", iteration=%previous_iteration))
				.await?;

				// Validate no leftover events
				iteration_branch.cursor().check_clear()?;

				if let Some(res) = res {
					break res;
				}
			}
		};

		// Move to next event
		ctx.cursor_mut().update(&loop_location);

		Ok(output)
	}
}
