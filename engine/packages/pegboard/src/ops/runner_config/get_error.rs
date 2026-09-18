use std::collections::{HashMap, HashSet};

use gas::prelude::*;
use rivet_types::actor::RunnerPoolError;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Input {
	pub runners: Vec<(Id, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunnerPoolErrorCacheEntry {
	namespace_id: Id,
	runner_name: String,
	error: Option<RunnerPoolError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerPoolErrorEntry {
	pub namespace_id: Id,
	pub runner_name: String,
	pub error: RunnerPoolError,
}

#[operation]
pub async fn pegboard_runner_config_get_error(
	ctx: &OperationCtx,
	input: &Input,
) -> Result<Vec<RunnerPoolErrorEntry>> {
	if input.runners.is_empty() {
		return Ok(Vec::new());
	}

	let entries = ctx
		.cache()
		.clone()
		.request()
		// Short TTL since errors can change quickly
		.ttl(500)
		.fetch_all_json(
			"pegboard.runner_config.get_error",
			input.runners.clone(),
			|mut cache, runners| async move {
				let entries = runner_config_get_error_inner(ctx, runners).await?;

				for entry in entries {
					cache.resolve(&(entry.namespace_id, entry.runner_name.clone()), entry);
				}

				Ok(cache)
			},
		)
		.await?
		.into_iter()
		.filter_map(|entry| {
			entry.error.map(|error| RunnerPoolErrorEntry {
				namespace_id: entry.namespace_id,
				runner_name: entry.runner_name,
				error,
			})
		})
		.collect();

	Ok(entries)
}

async fn runner_config_get_error_inner(
	ctx: &OperationCtx,
	runners: Vec<(Id, String)>,
) -> Result<Vec<RunnerPoolErrorCacheEntry>> {
	let runner_configs = ctx
		.op(crate::ops::runner_config::get::Input {
			runners: runners.clone(),
			bypass_cache: false,
		})
		.await?;
	let envoy_runners = runner_configs
		.into_iter()
		.filter(|runner_config| runner_config.protocol_version.is_some())
		.map(|runner_config| (runner_config.namespace_id, runner_config.name))
		.collect::<HashSet<_>>();
	let runners = runners
		.into_iter()
		.filter(|runner| !envoy_runners.contains(runner))
		.collect::<Vec<_>>();
	if runners.is_empty() {
		return Ok(Vec::new());
	}

	// TODO: Query runner pool workflows as well to check if the workflow is dead
	let queries: Vec<(&str, serde_json::Value)> = runners
		.iter()
		.map(|(namespace_id, runner_name)| {
			(
				crate::workflows::runner_pool_error_tracker::Workflow::NAME,
				serde_json::json!({
					"namespace_id": namespace_id,
					"runner_name": runner_name,
				}),
			)
		})
		.collect();
	let workflow_ids = ctx.find_workflows(&queries).await?;

	// Map workflow_id to runners
	let mut workflow_to_runner: HashMap<Id, (Id, String)> = HashMap::new();
	let mut workflow_ids_to_fetch = Vec::new();

	for (workflow_id, (namespace_id, runner_name)) in workflow_ids.into_iter().zip(runners.iter()) {
		if let Some(workflow_id) = workflow_id {
			workflow_to_runner.insert(workflow_id, (*namespace_id, runner_name.clone()));
			workflow_ids_to_fetch.push(workflow_id);
		}
	}

	if workflow_ids_to_fetch.is_empty() {
		return Ok(Vec::new());
	}

	let workflows = ctx.get_workflows(workflow_ids_to_fetch).await?;

	let mut result = Vec::new();

	for wf in workflows {
		let Some((namespace_id, runner_name)) = workflow_to_runner.get(&wf.workflow_id) else {
			continue;
		};

		if wf.is_dead() {
			result.push(RunnerPoolErrorCacheEntry {
				namespace_id: *namespace_id,
				runner_name: runner_name.clone(),
				error: Some(RunnerPoolError::InternalError),
			});
			continue;
		}

		let state =
			match wf.parse_state::<Option<crate::workflows::runner_pool_error_tracker::State>>() {
				Ok(Some(s)) => s,
				Ok(None) => {
					tracing::warn!(%namespace_id, %runner_name, "pool error tracker has no state");
					continue;
				}
				Err(err) => {
					tracing::error!(
						%namespace_id,
						%runner_name,
						?err,
						"failed to parse error tracker state"
					);
					continue;
				}
			};

		result.push(RunnerPoolErrorCacheEntry {
			namespace_id: *namespace_id,
			runner_name: runner_name.clone(),
			error: state.active_error.as_ref().map(|err| err.error.clone()),
		});
	}

	Ok(result)
}
