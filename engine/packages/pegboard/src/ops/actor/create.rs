use anyhow::{Context, Result};
use gas::prelude::*;
use moka::future::Cache;
use rivet_api_util::{Method, request_remote_datacenter};
use rivet_types::actors::{Actor, CrashPolicy};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;

const RATE_LIMITER_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
static RATE_LIMITERS: OnceLock<Cache<Id, Arc<Mutex<rivet_util::throttle::RateLimiter>>>> =
	OnceLock::new();

#[derive(Debug)]
pub struct Input {
	pub actor_id: Id,
	pub namespace_id: Id,
	pub name: String,
	pub key: Option<String>,
	pub runner_name_selector: String,
	pub crash_policy: CrashPolicy,
	pub input: Option<String>,
	/// If true, will handle ForwardToDatacenter errors by forwarding the request to the correct datacenter.
	/// Used by api-public. api-peer should set this to false.
	pub forward_request: bool,
	/// Datacenter to create the actor in
	///
	/// Providing this value will cause an error if attempting to create an actor where the key is
	/// reserved in a different datacenter.
	pub datacenter_name: Option<String>,
}

#[derive(Debug)]
pub struct Output {
	pub actor: Actor,
}

#[operation]
pub async fn pegboard_actor_create(ctx: &OperationCtx, input: &Input) -> Result<Output> {
	let rate_limit = RATE_LIMITERS
		.get_or_init(|| {
			Cache::builder()
				.max_capacity(10_000)
				.time_to_idle(RATE_LIMITER_CACHE_TTL)
				.build()
		})
		.entry(input.namespace_id)
		.or_insert_with(async {
			let pegboard_config = ctx.config().pegboard();
			Arc::new(Mutex::new(rivet_util::throttle::RateLimiter::new(
				rivet_util::throttle::RateLimitMethod::LeakyBucket {
					requests: pegboard_config.actor_create_rate_limit_requests(),
					drip_rate: Duration::from_millis(
						pegboard_config.actor_create_rate_limit_drip_rate_ms(),
					),
				},
			)))
		})
		.await;

	// Limit actor creation per namespace id
	if !rate_limit.value().lock().await.try_acquire() {
		return Err(crate::errors::Actor::CreationRateLimit.build());
	}

	// Set up subscriptions before dispatching workflow
	let (
		mut create_sub,
		mut fail_sub,
		mut destroy_sub,
		mut create_sub2,
		mut fail_sub2,
		mut destroy_sub2,
		pool_res,
	) = tokio::try_join!(
		ctx.subscribe::<crate::workflows::actor::CreateComplete>(("actor_id", input.actor_id)),
		ctx.subscribe::<crate::workflows::actor::Failed>(("actor_id", input.actor_id)),
		ctx.subscribe::<crate::workflows::actor::DestroyStarted>(("actor_id", input.actor_id)),
		ctx.subscribe::<crate::workflows::actor2::CreateComplete>(("actor_id", input.actor_id)),
		ctx.subscribe::<crate::workflows::actor2::Failed>(("actor_id", input.actor_id)),
		ctx.subscribe::<crate::workflows::actor2::DestroyStarted>(("actor_id", input.actor_id)),
		ctx.op(crate::ops::runner_config::get::Input {
			runners: vec![(input.namespace_id, input.runner_name_selector.clone())],
			bypass_cache: false,
		}),
	)?;

	let actor_v2 = pool_res
		.into_iter()
		.next()
		.map(|p| p.protocol_version.is_some())
		.unwrap_or_default();
	if actor_v2 {
		// Dispatch actor workflow
		ctx.workflow(crate::workflows::actor2::Input {
			actor_id: input.actor_id,
			name: input.name.clone(),
			pool_name: input.runner_name_selector.clone(),
			key: input.key.clone(),
			namespace_id: input.namespace_id,
			input: input.input.clone(),
			from_v1: false,
		})
		.tag("actor_id", input.actor_id)
		.dispatch()
		.await?;

		// Wait for actor creation to complete, fail, or be destroyed
		tokio::select! {
			res = create_sub2.next() => { res?; },
			res = fail_sub2.next() => {
				let msg = res?;
				let error = msg.into_body().error;

				// Check if this request needs to be forwarded
				//
				// We cannot forward if `datacenter_name` is specified because this actor is being
				// restricted to the given datacenter.
				if input.forward_request && input.datacenter_name.is_none() {
					if let crate::errors::Actor::KeyReservedInDifferentDatacenter { datacenter_label } = &error {
						// Forward the request to the correct datacenter
						return forward_to_datacenter(
							ctx,
							*datacenter_label,
							input.namespace_id,
							input.name.clone(),
							input.key.clone(),
							input.runner_name_selector.clone(),
							input.input.clone(),
						input.crash_policy
						).await;
					}
				}

				// Otherwise, return the error as-is
				return Err(error.build());
			}
			res = destroy_sub2.next() => {
				res?;
				return Err(crate::errors::Actor::DestroyedDuringCreation.build());
			}
		}
	} else {
		// Dispatch actor workflow
		ctx.workflow(crate::workflows::actor::Input {
			actor_id: input.actor_id,
			name: input.name.clone(),
			runner_name_selector: input.runner_name_selector.clone(),
			key: input.key.clone(),
			namespace_id: input.namespace_id,
			crash_policy: input.crash_policy,
			input: input.input.clone(),
		})
		.tag("actor_id", input.actor_id)
		.dispatch()
		.await?;

		// Wait for actor creation to complete, fail, or be destroyed
		tokio::select! {
			res = create_sub.next() => { res?; },
			res = fail_sub.next() => {
				let msg = res?;
				let error = msg.into_body().error;

				// Check if this request needs to be forwarded
				//
				// We cannot forward if `datacenter_name` is specified because this actor is being
				// restricted to the given datacenter.
				if input.forward_request && input.datacenter_name.is_none() {
					if let crate::errors::Actor::KeyReservedInDifferentDatacenter { datacenter_label } = &error {
						// Forward the request to the correct datacenter
						return forward_to_datacenter(
							ctx,
							*datacenter_label,
							input.namespace_id,
							input.name.clone(),
							input.key.clone(),
							input.runner_name_selector.clone(),
							input.input.clone(),
						input.crash_policy
						).await;
					}
				}

				// Otherwise, return the error as-is
				return Err(error.build());
			}
			res = destroy_sub.next() => {
				res?;
				return Err(crate::errors::Actor::DestroyedDuringCreation.build());
			}
		}
	}

	// Fetch the created actor
	let actors_res = ctx
		.op(crate::ops::actor::get::Input {
			actor_ids: vec![input.actor_id],
			fetch_error: false,
		})
		.await?;

	let actor = actors_res
		.actors
		.into_iter()
		.next()
		.ok_or_else(|| crate::errors::Actor::NotFound.build())?;

	Ok(Output { actor })
}

/// Forward the actor creation request to the correct datacenter
async fn forward_to_datacenter(
	ctx: &OperationCtx,
	datacenter_label: u16,
	namespace_id: Id,
	name: String,
	key: Option<String>,
	runner_name_selector: String,
	input: Option<String>,
	crash_policy: CrashPolicy,
) -> Result<Output> {
	// Get the datacenter configuration
	let _target_dc = ctx
		.config()
		.dc_for_label(datacenter_label)
		.with_context(|| format!("datacenter not found for label {}", datacenter_label))?;

	// Get namespace name for the remote call
	let namespace = ctx
		.op(namespace::ops::get_global::Input {
			namespace_ids: vec![namespace_id],
		})
		.await?
		.into_iter()
		.next()
		.ok_or_else(|| namespace::errors::Namespace::NotFound.build())?;

	// Make request to remote datacenter
	let response = request_remote_datacenter::<rivet_api_types::actors::create::CreateResponse>(
		ctx.config(),
		datacenter_label,
		"/actors",
		Method::POST,
		Some(&rivet_api_types::actors::create::CreateQuery {
			namespace: namespace.name.clone(),
		}),
		Some(&rivet_api_types::actors::create::CreateRequest {
			datacenter: None,
			name,
			key,
			input,
			runner_name_selector,
			crash_policy,
		}),
	)
	.await?;

	Ok(Output {
		actor: response.actor,
	})
}
