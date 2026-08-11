use std::time::Duration;

use futures_util::FutureExt;
use gas::prelude::*;
use rivet_types::runner_configs::RunnerConfigKind;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Input {
	pub namespace_id: Id,
	pub runner_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct State {
	last_retry_ts: i64,
	retry_count: usize,
}

#[workflow]
pub async fn pegboard_runner_pool_metadata_poller(
	ctx: &mut WorkflowCtx,
	input: &Input,
) -> Result<()> {
	ctx.repeat(|ctx| {
		let input = input.clone();
		async move {
			let poll_result = ctx
				.activity(PollMetadataInput {
					namespace_id: input.namespace_id,
					runner_name: input.runner_name.clone(),
				})
				.await?;

			match poll_result {
				PollMetadataOutput::Success {
					poll_interval,
					older_runner_workflow_ids,
				} => {
					// Send stop signals to older runners
					if !older_runner_workflow_ids.is_empty() {
						ctx.loope(older_runner_workflow_ids, |ctx, workflow_ids| {
							async move {
								let Some(workflow_id) = workflow_ids.pop() else {
									return Ok(Loop::Break(()));
								};

								ctx.signal(crate::workflows::runner2::Stop {
									reset_actor_rescheduling: false,
								})
								.to_workflow_id(workflow_id)
								.send()
								.await?;

								Ok(Loop::Continue)
							}
							.boxed()
						})
						.await?;
					}

					// Wait at the end of the loop so the first poll happens immediately
					// and the poll interval is read fresh from the config each iteration.
					let _ = ctx
						.listen_with_timeout::<EndpointConfigChanged>(Duration::from_millis(
							poll_interval,
						))
						.await?;

					Ok(Loop::Continue)
				}
				PollMetadataOutput::FetchError { poll_interval } => {
					// Wait before retrying
					let _ = ctx
						.listen_with_timeout::<EndpointConfigChanged>(Duration::from_millis(
							poll_interval,
						))
						.await?;

					Ok(Loop::Continue)
				}
				PollMetadataOutput::Break => Ok(Loop::Break(())),
			}
		}
		.boxed()
	})
	.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct PollMetadataInput {
	namespace_id: Id,
	runner_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum PollMetadataOutput {
	Success {
		poll_interval: u64,
		older_runner_workflow_ids: Vec<Id>,
	},
	FetchError {
		poll_interval: u64,
	},
	Break,
}

/// Combined activity that fetches metadata, updates actor names, and drains older versions.
#[activity(PollMetadata)]
async fn poll_metadata(ctx: &ActivityCtx, input: &PollMetadataInput) -> Result<PollMetadataOutput> {
	let mut state = ctx.state::<Option<State>>()?;
	if state.is_none() {
		*state = Some(State::default());
	}
	let state = state.as_mut().unwrap();

	tracing::debug!(
		namespace_id = %input.namespace_id,
		runner_name = %input.runner_name,
		"polling metadata"
	);

	// Get runner config
	let runner_config_res = ctx
		.op(crate::ops::runner_config::get::Input {
			runners: vec![(input.namespace_id, input.runner_name.clone())],
			bypass_cache: true,
		})
		.await?;

	let Some(runner_config) = runner_config_res.into_iter().next() else {
		tracing::debug!(
			namespace_id = %input.namespace_id,
			runner_name = %input.runner_name,
			"runner config not found, stopping metadata poller"
		);
		return Ok(PollMetadataOutput::Break);
	};

	let RunnerConfigKind::Serverless {
		url,
		headers,
		metadata_poll_interval,
		..
	} = runner_config.config.kind
	else {
		tracing::debug!(
			namespace_id = %input.namespace_id,
			runner_name = %input.runner_name,
			"runner config is not serverless, stopping metadata poller"
		);
		return Ok(PollMetadataOutput::Break);
	};

	// Reset backoff
	if state.last_retry_ts < util::timestamp::now() - util::duration::minutes(10) {
		state.retry_count = 0;
	}

	// Calculate effective poll interval (in milliseconds)
	let min_poll_interval = ctx.config().pegboard().min_metadata_poll_interval();
	let default_poll_interval = ctx.config().pegboard().default_metadata_poll_interval();
	let base_poll_interval = metadata_poll_interval
		.unwrap_or(default_poll_interval)
		.max(min_poll_interval);
	let backoff = util::throttle::Backoff::new_at(
		8,
		None,
		base_poll_interval as usize,
		500,
		state.retry_count,
	);
	let backoff_poll_interval = backoff.current_duration() as u64;

	let result = ctx
		.op(crate::ops::runner_config::refresh_metadata::Input {
			namespace_id: input.namespace_id,
			runner_name: input.runner_name.clone(),
			url,
			headers,
		})
		.await?;

	let metadata = match result {
		Ok(metadata) => metadata,
		Err(error) => {
			tracing::warn!(
				namespace_id = %input.namespace_id,
				runner_name = %input.runner_name,
				?error,
				"failed to fetch metadata, will retry"
			);

			state.last_retry_ts = util::timestamp::now();
			state.retry_count += 1;

			return Ok(PollMetadataOutput::FetchError {
				poll_interval: backoff_poll_interval,
			});
		}
	};

	// Drain older runners if runner_version is set
	let older_runner_workflow_ids = if let Some(version) = metadata.runner_version {
		ctx.op(crate::ops::runner::drain::Input {
			namespace_id: input.namespace_id,
			name: input.runner_name.clone(),
			version,
			// Signals are sent by the workflow directly
			send_runner_stop_signals: false,
		})
		.await?
		.older_runner_workflow_ids
	} else {
		Vec::new()
	};

	// Drain older envoys if envoy_version is set
	if let Some(version) = metadata.envoy_version {
		ctx.op(crate::ops::envoy::drain::Input {
			namespace_id: input.namespace_id,
			pool_name: input.runner_name.clone(),
			version,
		})
		.await?;
	}

	Ok(PollMetadataOutput::Success {
		poll_interval: base_poll_interval,
		older_runner_workflow_ids,
	})
}

#[signal("pegboard_runner_pool_metadata_poller_endpoint_config_changed")]
#[derive(Debug)]
pub struct EndpointConfigChanged {}
