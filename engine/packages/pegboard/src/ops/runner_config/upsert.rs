use epoxy::ops::propose::{Command, CommandKind, Proposal, SetCommand};
use gas::prelude::*;
use rivet_types::runner_configs::{RunnerConfig, RunnerConfigKind};
use universaldb::prelude::*;

use crate::{errors, keys, utils::runner_config_variant};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
	pub config: RunnerConfig,
}

#[operation]
pub async fn pegboard_runner_config_upsert(ctx: &OperationCtx, input: &Input) -> Result<bool> {
	let config = input.config.clone();
	let config_affects_pool = config.affects_pool();
	let serverless_config =
		if let RunnerConfigKind::Serverless { url, headers, .. } = &input.config.kind {
			Some((url.clone(), headers.clone()))
		} else {
			None
		};

	// Validation
	match &config.kind {
		RunnerConfigKind::Normal { .. } => {}
		RunnerConfigKind::Serverless {
			url,
			headers,
			request_lifespan,
			drain_grace_period,
			slots_per_runner,
			..
		} => {
			let parsed_url = match url::Url::parse(url) {
				Ok(parsed_url) => parsed_url,
				Err(err) => {
					return Err(errors::RunnerConfig::Invalid {
						reason: format!("invalid serverless url: {err}"),
					}
					.build());
				}
			};

			// Reject destinations the engine is not allowed to reach before the config is stored.
			// Requests are checked again when they connect, which catches configs written before
			// this gate existed and hosts whose DNS answer changes afterwards.
			let policy = rivet_pools::reqwest::outbound_policy(ctx.config()).await?;
			if let Err(reason) = policy.check_url(&parsed_url) {
				return Err(errors::RunnerConfig::Invalid {
					reason: format!("invalid serverless url: {reason}"),
				}
				.build());
			}

			if headers.len() > 16 {
				return Err(errors::RunnerConfig::Invalid {
					reason: "too many headers (max 16)".to_string(),
				}
				.build());
			}

			for (n, v) in headers {
				if n.len() > 128 {
					return Err(errors::RunnerConfig::Invalid {
						reason: format!("invalid header name: too long (max 128)"),
					}
					.build());
				}
				if let Err(err) = n.parse::<reqwest::header::HeaderName>() {
					return Err(errors::RunnerConfig::Invalid {
						reason: format!("invalid header name: {err}"),
					}
					.build());
				}
				if v.len() > 4096 {
					return Err(errors::RunnerConfig::Invalid {
						reason: format!("invalid header value: too long (max 4096)"),
					}
					.build());
				}
				if let Err(err) = v.parse::<reqwest::header::HeaderValue>() {
					return Err(errors::RunnerConfig::Invalid {
						reason: format!("invalid header value: {err}"),
					}
					.build());
				}
			}

			if *slots_per_runner == 0 {
				return Err(errors::RunnerConfig::Invalid {
					reason: "`slots_per_runner` cannot be 0".to_string(),
				}
				.build());
			}

			if *drain_grace_period >= *request_lifespan {
				return Err(errors::RunnerConfig::Invalid {
					reason: format!(
						"`drain_grace_period` must be less than `request_lifespan` ({drain_grace_period}s >= {request_lifespan}s)"
					),
				}
				.build());
			}

			let actor_stop_threshold_ms = ctx.config().pegboard().actor_stop_threshold();
			let drain_grace_period_ms = i64::from(*drain_grace_period) * 1000;
			if drain_grace_period_ms > actor_stop_threshold_ms {
				return Err(errors::RunnerConfig::Invalid {
					reason: format!(
						"`drain_grace_period` cannot be greater than `actor_stop_threshold` ({drain_grace_period_ms}ms > {actor_stop_threshold_ms}ms). If `drain_grace_period` was omitted, the default is 1800s."
					),
				}
				.build());
			}
		}
	}

	// TODO: Race
	let existing_config = ctx
		.op(crate::ops::runner_config::get::Input {
			runners: vec![(input.namespace_id, input.name.clone())],
			bypass_cache: true,
		})
		.await?
		.into_iter()
		.next()
		.map(|c| c.config);

	// Check if config changed (for serverless, compare URL and headers)
	let (endpoint_config_changed, pool_created) = if let Some(existing_config) = &existing_config {
		// Check if serverless endpoint config changed
		match (&existing_config.kind, &config.kind) {
			(
				RunnerConfigKind::Serverless {
					url: old_url,
					headers: old_headers,
					..
				},
				RunnerConfigKind::Serverless {
					url: new_url,
					headers: new_headers,
					..
				},
			) => (old_url != new_url || old_headers != new_headers, false),
			(RunnerConfigKind::Normal { .. }, RunnerConfigKind::Serverless { .. }) => {
				// Config type changed to serverless
				(true, true)
			}
			_ => {
				// Not serverless
				(true, false)
			}
		}
	} else {
		// New config
		(true, serverless_config.is_some())
	};

	// Save to epoxy
	let global_runner_config_key = keys::runner_config::GlobalDataKey::new(
		ctx.config().dc_label(),
		input.namespace_id,
		input.name.clone(),
	);
	ctx.op(epoxy::ops::propose::Input {
		proposal: Proposal {
			commands: vec![Command {
				kind: CommandKind::SetCommand(SetCommand {
					key: namespace::keys::subspace().pack(&global_runner_config_key),
					value: Some(global_runner_config_key.serialize(config.clone())?),
				}),
			}],
		},
		purge_cache: true,
		mutable: true,
		target_replicas: None,
	})
	.await?;

	// We still have to write locally for listing
	// TODO: non-transactional. Epoxy propose and the local UDB write can diverge if we crash or
	// error between them.
	ctx.udb()?
		.txn("pegboard_runner_config_upsert", |tx| {
			let config = &config;
			let existing_config = &existing_config;
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());

				// Delete previous index
				if let Some(existing_config) = &existing_config {
					tx.delete(&keys::runner_config::ByVariantKey::new(
						input.namespace_id,
						runner_config_variant(&existing_config),
						input.name.clone(),
					));
				}

				// Write new config
				let runner_config_key =
					keys::runner_config::DataKey::new(input.namespace_id, input.name.clone());
				tx.write(&runner_config_key, config.clone())?;
				tx.write(
					&keys::runner_config::ByVariantKey::new(
						input.namespace_id,
						runner_config_variant(config),
						input.name.clone(),
					),
					config.clone(),
				)?;

				Ok(())
			}
		})
		.custom_instrument(tracing::info_span!("runner_config_upsert_tx"))
		.await?;

	if endpoint_config_changed {
		crate::utils::purge_runner_config_caches(ctx.cache(), input.namespace_id, &input.name)
			.await?;

		// Update runner metadata before notifying the pool workflow so newer
		// RivetKit serverless handlers are treated as envoy-backed immediately.
		if let Some((url, headers)) = serverless_config {
			tracing::debug!("endpoint config changed, refreshing metadata");
			if let Err(err) = ctx
				.op(crate::ops::runner_config::refresh_metadata::Input {
					namespace_id: input.namespace_id,
					runner_name: input.name.clone(),
					url,
					headers,
				})
				.await
			{
				tracing::warn!(?err, runner_name=?input.name, "failed to refresh runner config metadata");
			}
		}
	}

	if pool_created {
		ctx.workflow(crate::workflows::runner_pool::Input {
			namespace_id: input.namespace_id,
			runner_name: input.name.clone(),
		})
		.tag("namespace_id", input.namespace_id)
		.tag("runner_name", input.name.clone())
		.unique()
		.dispatch()
		.await?;
	} else if config_affects_pool {
		let signal_res = ctx
			.signal(crate::workflows::runner_pool::Bump {
				endpoint_config_changed,
			})
			.to_workflow::<crate::workflows::runner_pool::Workflow>()
			.tag("namespace_id", input.namespace_id)
			.tag("runner_name", input.name.clone())
			.graceful_not_found()
			.send()
			.await?;

		// Backfill
		if signal_res.is_none() {
			ctx.workflow(crate::workflows::runner_pool::Input {
				namespace_id: input.namespace_id,
				runner_name: input.name.clone(),
			})
			.tag("namespace_id", input.namespace_id)
			.tag("runner_name", input.name.clone())
			.unique()
			.dispatch()
			.await?;
		}
	}

	Ok(endpoint_config_changed)
}
