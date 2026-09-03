mod cors;
mod gateway3_selector;
mod resolve_actor_query;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use gas::{ctx::message::SubscriptionHandle, prelude::*};
use hyper::header::HeaderName;
use rivet_guard_core::{RouteConfig, RouteTarget, RoutingOutput, request_context::RequestContext};

use super::{
	SEC_WEBSOCKET_PROTOCOL, WS_PROTOCOL_ACTOR, WS_PROTOCOL_SKIP_READY_WAIT, WS_PROTOCOL_TOKEN,
	X_RIVET_SKIP_READY_WAIT, X_RIVET_TOKEN, actor_path::ParsedActorPath,
};
use crate::{
	errors, metrics,
	routing::{
		Phase,
		actor_path::{is_actor_gateway_path, parse_actor_path},
		pegboard_gateway::resolve_actor_query::ResolveQueryActorResult,
		phase_timeout,
	},
	shared_state::SharedState,
};
use cors::{CorsPreflight, set_non_preflight_cors};
use gateway3_selector::select_gateway;
use resolve_actor_query::resolve_query;

/// Time to wait before starting pool error checks
const RUNNER_POOL_ERROR_CHECK_DELAY: Duration = Duration::from_secs(1);
/// Interval between pool error checks
const RUNNER_POOL_ERROR_CHECK_INTERVAL: Duration = Duration::from_secs(2);

pub const X_RIVET_ACTOR: HeaderName = HeaderName::from_static("x-rivet-actor");

/// Route requests to actor services using path-based routing
#[tracing::instrument(skip_all)]
pub async fn route_request_path_based(
	ctx: &StandaloneCtx,
	shared_state: &SharedState,
	req_ctx: &mut RequestContext,
) -> Result<Option<RoutingOutput>> {
	let res = route_request_path_based_inner(ctx, shared_state, req_ctx).await;

	match &res {
		Ok(Some(_)) | Err(_) => {
			// Attach CORS headers to the actual (non-OPTIONS) response so both the
			// actor response and any early error are readable by the browser.
			set_non_preflight_cors(req_ctx);
		}
		_ => {}
	}

	res
}

pub async fn route_request_path_based_inner(
	ctx: &StandaloneCtx,
	shared_state: &SharedState,
	req_ctx: &mut RequestContext,
) -> Result<Option<RoutingOutput>> {
	if req_ctx.method() == hyper::Method::OPTIONS {
		if is_actor_gateway_path(req_ctx.path()) {
			return Ok(Some(RoutingOutput::CustomServe(Arc::new(CorsPreflight))));
		}

		return Ok(None);
	}

	let Some(actor_path) = parse_actor_path(req_ctx.path())? else {
		return Ok(None);
	};

	tracing::debug!(?actor_path, "routing using path-based actor routing");

	let (actor_id, token, stripped_path, skip_ready_wait) = match actor_path {
		ParsedActorPath::Direct(path) => (
			Id::parse(&path.actor_id).context("invalid actor id in path")?,
			read_gateway_token_for_path_based(req_ctx, path.token.as_deref())?
				.map(ToOwned::to_owned),
			path.stripped_path.clone(),
			read_skip_ready_wait_for_path_based(req_ctx)?,
		),
		ParsedActorPath::Query(path) => {
			let token = read_gateway_token_for_path_based(req_ctx, path.token.as_deref())?
				.map(ToOwned::to_owned);

			match phase_timeout(
				Phase::new(
					"route_pegboard_resolve_query",
					&metrics::ROUTE_PEGBOARD_RESOLVE_QUERY_DURATION,
				),
				ctx.config().guard().route_pegboard_resolve_query_timeout(),
				resolve_query(ctx, &path.query),
				|elapsed, timeout| {
					pegboard::errors::RouteResolveQueryTimeout {
						elapsed_ms: elapsed.as_millis() as u64,
						timeout_ms: timeout.as_millis() as u64,
					}
					.build()
				},
			)
			.await?
			{
				ResolveQueryActorResult::Found { actor_id } => (
					actor_id,
					token,
					path.stripped_path.clone(),
					path.query.skip_ready_wait(),
				),
				ResolveQueryActorResult::Forward { dc_label } => {
					let peer_dc = ctx
						.config()
						.dc_for_label(dc_label)
						.ok_or_else(|| rivet_api_util::errors::Datacenter::NotFound.build())?;

					return Ok(Some(RoutingOutput::Route(RouteConfig {
						targets: vec![RouteTarget {
							host: peer_dc
								.proxy_url_host()
								.context("bad peer dc proxy url host")?
								.to_string(),
							port: peer_dc
								.proxy_url_port()
								.context("bad peer dc proxy url port")?,
							path: req_ctx.path().to_owned(),
						}],
					})));
				}
			}
		}
	};

	route_request_inner(
		ctx,
		shared_state,
		req_ctx,
		actor_id,
		&stripped_path,
		token.as_deref(),
		skip_ready_wait,
	)
	.await
	.map(Some)
}

/// Route requests to actor services based on headers
#[tracing::instrument(skip_all)]
pub async fn route_request(
	ctx: &StandaloneCtx,
	shared_state: &SharedState,
	req_ctx: &mut RequestContext,
	target: &str,
) -> Result<Option<RoutingOutput>> {
	// Check target
	if target != "actor" {
		return Ok(None);
	}

	if req_ctx.method() == hyper::Method::OPTIONS {
		return Ok(Some(RoutingOutput::CustomServe(Arc::new(CorsPreflight))));
	}

	if !req_ctx.is_websocket() && !is_actor_http_request_path(req_ctx.path()) {
		return Ok(None);
	}

	// Attach CORS headers to the actual (non-OPTIONS) response so both the
	// actor response and any early error are readable by the browser.
	set_non_preflight_cors(req_ctx);

	// Extract actor ID and token from WebSocket protocol or HTTP headers
	let (actor_id_str, token, skip_ready_wait) = if req_ctx.is_websocket() {
		// For WebSocket, parse the sec-websocket-protocol header
		let protocols_header = req_ctx
			.headers()
			.get(SEC_WEBSOCKET_PROTOCOL)
			.and_then(|protocols| protocols.to_str().ok())
			.ok_or_else(|| {
				crate::errors::MissingHeader {
					header: "sec-websocket-protocol".to_string(),
				}
				.build()
			})?;

		let protocols: Vec<&str> = protocols_header.split(',').map(|p| p.trim()).collect();

		let actor_id_raw = protocols
			.iter()
			.find_map(|p| p.strip_prefix(WS_PROTOCOL_ACTOR))
			.ok_or_else(|| {
				crate::errors::MissingHeader {
					header: "`rivet_actor.*` protocol in sec-websocket-protocol".to_string(),
				}
				.build()
			})?;

		let actor_id = urlencoding::decode(actor_id_raw)
			.context("invalid url encoding in actor id")?
			.to_string();

		let token = protocols
			.iter()
			.find_map(|p| p.strip_prefix(WS_PROTOCOL_TOKEN))
			.map(ToOwned::to_owned);

		let skip_ready_wait = protocols.iter().any(|p| p == &WS_PROTOCOL_SKIP_READY_WAIT);

		(actor_id, token, skip_ready_wait)
	} else {
		// For HTTP, use headers
		let actor_id = req_ctx
			.headers()
			.get(X_RIVET_ACTOR)
			.map(|x| x.to_str())
			.transpose()
			.context("invalid x-rivet-actor header")?
			.ok_or_else(|| {
				crate::errors::MissingHeader {
					header: X_RIVET_ACTOR.to_string(),
				}
				.build()
			})?;

		let token = req_ctx
			.headers()
			.get(X_RIVET_TOKEN)
			.map(|x| x.to_str())
			.transpose()
			.context("invalid x-rivet-token header")?
			.map(ToOwned::to_owned);

		let skip_ready_wait = read_skip_ready_wait_header(req_ctx)?;

		(actor_id.to_string(), token, skip_ready_wait)
	};

	// Find actor to route to
	let actor_id = Id::parse(&actor_id_str).context("invalid x-rivet-actor header")?;
	let stripped_path = req_ctx.path().to_owned();

	route_request_inner(
		ctx,
		shared_state,
		req_ctx,
		actor_id,
		&stripped_path,
		token.as_deref(),
		skip_ready_wait,
	)
	.await
	.map(Some)
}

fn is_actor_http_request_path(path: &str) -> bool {
	let Some(stripped) = path.strip_prefix("/request") else {
		return false;
	};

	stripped.is_empty() || matches!(stripped.as_bytes().first(), Some(b'/') | Some(b'?'))
}

#[tracing::instrument(
	skip_all,
	fields(
		gateway = tracing::field::Empty,
		envoy_protocol = tracing::field::Empty,
		request_kind = tracing::field::Empty,
		mode = tracing::field::Empty,
		decision = tracing::field::Empty,
	)
)]
async fn route_request_inner(
	ctx: &StandaloneCtx,
	shared_state: &SharedState,
	req_ctx: &mut RequestContext,
	actor_id: Id,
	stripped_path: &str,
	_token: Option<&str>,
	skip_ready_wait: bool,
) -> Result<RoutingOutput> {
	// NOTE: Token validation implemented in EE

	// Route to peer dc where the actor lives
	if actor_id.label() != ctx.config().dc_label() {
		tracing::debug!(peer_dc_label=?actor_id.label(), "re-routing actor to peer dc");

		let peer_dc = ctx
			.config()
			.dc_for_label(actor_id.label())
			.ok_or_else(|| rivet_api_util::errors::Datacenter::NotFound.build())?;

		return Ok(RoutingOutput::Route(RouteConfig {
			targets: vec![RouteTarget {
				host: peer_dc
					.proxy_url_host()
					.context("bad peer dc proxy url host")?
					.to_string(),
				port: peer_dc
					.proxy_url_port()
					.context("bad peer dc proxy url port")?,
				path: req_ctx.path().to_owned(),
			}],
		}));
	}

	// Create subs before checking if actor exists/is not destroyed
	let (
		ready_sub,
		stopped_sub,
		fail_sub,
		destroy_sub,
		migrate_sub,
		ready_sub2,
		stopped_sub2,
		fail_sub2,
		destroy_sub2,
	) = phase_timeout(
		Phase::new(
			"route_pegboard_subscribe",
			&metrics::ROUTE_PEGBOARD_SUBSCRIBE_DURATION,
		)
		.with_actor_id(actor_id),
		ctx.config().guard().route_pegboard_subscribe_timeout(),
		async {
			tokio::try_join!(
				ctx.subscribe::<pegboard::workflows::actor::Ready>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor::Stopped>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor::Failed>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor::DestroyStarted>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor::MigratedToV2>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor2::Ready>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor2::Stopped>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor2::Failed>(("actor_id", actor_id)),
				ctx.subscribe::<pegboard::workflows::actor2::DestroyStarted>((
					"actor_id", actor_id
				)),
			)
		},
		|elapsed, timeout| {
			pegboard::errors::RouteSubscribeTimeout {
				elapsed_ms: elapsed.as_millis() as u64,
				timeout_ms: timeout.as_millis() as u64,
			}
			.build()
		},
	)
	.await?;

	// Fetch actor info
	let Some(actor) = phase_timeout(
		Phase::new(
			"route_pegboard_fetch_actor",
			&metrics::ROUTE_PEGBOARD_FETCH_ACTOR_DURATION,
		)
		.with_actor_id(actor_id),
		ctx.config().guard().route_pegboard_fetch_actor_timeout(),
		ctx.op(pegboard::ops::actor::get_for_gateway::Input { actor_id }),
		|elapsed, timeout| {
			pegboard::errors::RouteFetchActorTimeout {
				actor_id: actor_id.to_string(),
				elapsed_ms: elapsed.as_millis() as u64,
				timeout_ms: timeout.as_millis() as u64,
			}
			.build()
		},
	)
	.await?
	else {
		return Err(pegboard::errors::Actor::NotFound.build());
	};

	if actor.destroyed {
		return Err(pegboard::errors::Actor::NotFound.build());
	}

	match actor.version {
		2 => {
			drop(ready_sub);
			drop(stopped_sub);
			drop(fail_sub);
			drop(destroy_sub);
			drop(migrate_sub);

			handle_actor_v2(
				ctx,
				shared_state,
				req_ctx,
				actor_id,
				actor,
				stripped_path,
				skip_ready_wait,
				ready_sub2,
				stopped_sub2,
				fail_sub2,
				destroy_sub2,
			)
			.await
		}
		1 => {
			handle_actor_v1(
				ctx,
				shared_state,
				req_ctx,
				actor_id,
				actor,
				stripped_path,
				skip_ready_wait,
				ready_sub,
				stopped_sub,
				fail_sub,
				destroy_sub,
				migrate_sub,
				ready_sub2,
				stopped_sub2,
				fail_sub2,
				destroy_sub2,
			)
			.await
		}
		_ => bail!("unknown actor version"),
	}
}

async fn handle_actor_v2(
	ctx: &StandaloneCtx,
	shared_state: &SharedState,
	req_ctx: &RequestContext,
	actor_id: Id,
	mut actor: pegboard::ops::actor::get_for_gateway::Output,
	stripped_path: &str,
	skip_ready_wait: bool,
	mut ready_sub: SubscriptionHandle<pegboard::workflows::actor2::Ready>,
	mut stopped_sub: SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	mut fail_sub: SubscriptionHandle<pegboard::workflows::actor2::Failed>,
	mut destroy_sub: SubscriptionHandle<pegboard::workflows::actor2::DestroyStarted>,
) -> Result<RoutingOutput> {
	// Wake actor if sleeping
	if actor.sleeping {
		tracing::debug!(
			?actor_id,
			actor_key = ?actor.key,
			pool_name = ?actor.runner_name_selector,
			"actor sleeping, waking"
		);

		phase_timeout(
			Phase::new(
				"route_pegboard_wake_signal",
				&metrics::ROUTE_PEGBOARD_WAKE_SIGNAL_DURATION,
			)
			.with_namespace_id(actor.namespace_id)
			.with_actor_id(actor_id),
			ctx.config().guard().route_pegboard_wake_signal_timeout(),
			ctx.signal(pegboard::workflows::actor2::Wake {})
				.to_workflow_id(actor.workflow_id)
				.send(),
			|elapsed, timeout| {
				pegboard::errors::RouteWakeSignalTimeout {
					actor_id: actor_id.to_string(),
					elapsed_ms: elapsed.as_millis() as u64,
					timeout_ms: timeout.as_millis() as u64,
				}
				.build()
			},
		)
		.await?;
	}

	let actor_envoy_key = actor.envoy_key.clone();
	let was_sleeping = actor.sleeping;
	let mut waited_for_ready = false;
	let envoy_key = if let (Some(envoy_key), true) = (
		actor_envoy_key.clone(),
		actor.connectable || skip_ready_wait,
	) {
		envoy_key
	} else {
		waited_for_ready = true;
		tracing::debug!(
			?actor_id,
			actor_key = ?actor.key,
			pool_name = ?actor.runner_name_selector,
			"waiting for actor to become ready"
		);

		let mut wake_retries = 0;
		let ready_wait_started_at = std::time::Instant::now();
		let ready_wait_namespace_id = actor.namespace_id.to_string();
		let ready_wait_pool_name = actor
			.runner_name_selector
			.as_deref()
			.unwrap_or("unknown")
			.to_string();
		let was_sleeping_label = if was_sleeping { "true" } else { "false" };
		let record_ready_wait = |outcome: &'static str, wake_retries: u32| {
			let wake_retries_bucket = match wake_retries {
				0 => "0",
				1 => "1",
				2 => "2",
				_ => "3+",
			};
			metrics::ROUTE_PEGBOARD_READY_WAIT_DURATION
				.with_label_values(&[
					ready_wait_namespace_id.as_str(),
					ready_wait_pool_name.as_str(),
					was_sleeping_label,
					wake_retries_bucket,
					outcome,
				])
				.observe(ready_wait_started_at.elapsed().as_secs_f64());
		};

		// Create pool error check future
		let pool_error_check_fut = check_runner_pool_error_loop(
			ctx,
			actor.namespace_id,
			actor.runner_name_selector.as_deref(),
		);
		tokio::pin!(pool_error_check_fut);

		// Wait for ready, fail, or destroy
		loop {
			tokio::select! {
				res = ready_sub.next() => {
					let envoy_key = res?.into_body().envoy_key;
					record_ready_wait("ready", wake_retries);
					break envoy_key;
				}
				res = stopped_sub.next() => {
					res?;

					if wake_retries < 8 {
						tracing::debug!(
							?actor_id,
							actor_key = ?actor.key,
							pool_name = ?actor.runner_name_selector,
							?wake_retries,
							"actor stopped while we were waiting for it to become ready, attempting rewake"
						);
						wake_retries += 1;

						let res = phase_timeout(
							Phase::new(
								"route_pegboard_wake_signal",
								&metrics::ROUTE_PEGBOARD_WAKE_SIGNAL_DURATION,
							)
							.with_namespace_id(actor.namespace_id)
							.with_actor_id(actor_id),
							ctx.config().guard().route_pegboard_wake_signal_timeout(),
							ctx.signal(pegboard::workflows::actor2::Wake {})
								.to_workflow_id(actor.workflow_id)
								.graceful_not_found()
								.send(),
							|elapsed, timeout| {
								pegboard::errors::RouteWakeSignalTimeout {
									actor_id: actor_id.to_string(),
									elapsed_ms: elapsed.as_millis() as u64,
									timeout_ms: timeout.as_millis() as u64,
								}
								.build()
							},
						)
						.await?;

						if res.is_none() {
							tracing::warn!(
								?actor_id,
								actor_key = ?actor.key,
								pool_name = ?actor.runner_name_selector,
								"actor workflow not found for rewake"
							);
							record_ready_wait("rewake_not_found", wake_retries);
							return Err(pegboard::errors::Actor::NotFound.build());
						}
					} else {
						tracing::warn!(
							?actor_id,
							actor_key = ?actor.key,
							pool_name = ?actor.runner_name_selector,
							"actor retried waking 8 times, has not yet started"
						);
						record_ready_wait("stopped", wake_retries);
						return Err(rivet_guard_core::errors::ActorWakeRetriesExceeded {
							actor_id: actor_id.to_string(),
							wake_retries,
							reason: "actor_stopped_before_ready".to_owned(),
						}
						.build());
					}
				}
				res = fail_sub.next() => {
					let msg = res?;
					record_ready_wait("failed", wake_retries);
					return Err(msg.error.clone().build());
				}
				res = destroy_sub.next() => {
					res?;
					record_ready_wait("destroyed", wake_retries);
					return Err(pegboard::errors::Actor::DestroyedWhileWaitingForReady.build());
				}
				res = &mut pool_error_check_fut => {
					if res? {
						record_ready_wait("pool_error", wake_retries);
						return Err(errors::ActorRunnerFailed { actor_id }.build());
					}
				}
				// Ready timeout
				_ = tokio::time::sleep(ctx.config().guard().actor_ready_timeout()) => {
					tracing::warn!(
						?actor_id,
						actor_key = ?actor.key,
						pool_name = ?actor.runner_name_selector,
						"timed out waiting for actor to become ready"
					);
					record_ready_wait("timeout", wake_retries);
					return Err(errors::ActorReadyTimeout { actor_id }.build());
				}
			}
		}
	};

	if waited_for_ready {
		let Some(refreshed_actor) = ctx
			.op(pegboard::ops::actor::get_for_gateway::Input { actor_id })
			.await?
		else {
			return Err(pegboard::errors::Actor::NotFound.build());
		};

		if refreshed_actor.destroyed || refreshed_actor.envoy_key.as_deref() != Some(&envoy_key) {
			return Err(rivet_guard_core::errors::ActorStoppedWhileWaiting {
				actor_id: actor_id.to_string(),
				phase: "route_after_ready".to_owned(),
			}
			.build());
		}

		actor = refreshed_actor;
	}

	let pool_name = actor
		.runner_name_selector
		.clone()
		.unwrap_or_else(|| "unknown".to_string());

	tracing::debug!(
		?actor_id,
		actor_key = ?actor.key,
		%pool_name,
		%envoy_key,
		"actor ready"
	);

	let selection = select_gateway(
		&ctx.config().features().guard_gateway_v3(),
		req_ctx,
		actor.namespace_id,
		actor_id,
		actor.envoy_protocol_version,
	);
	selection.record();
	let request_kind = if req_ctx.is_websocket() {
		rivet_guard_core::metrics::PegboardGatewayRequestKind::WebSocket
	} else {
		rivet_guard_core::metrics::PegboardGatewayRequestKind::Http
	};

	if selection.use_gateway3() {
		let actor_generation = actor
			.generation
			.context("gateway3 actor is missing its generation")?;
		let lifecycle = rivet_guard_core::metrics::PegboardGatewayLifecycle::new(
			rivet_guard_core::metrics::PegboardGatewayVersion::V3,
			actor.envoy_protocol_version,
			request_kind,
		);
		let gateway = pegboard_gateway3::PegboardGateway3::new(
			ctx.clone(),
			shared_state.pegboard_gateway3.clone(),
			lifecycle,
			actor.namespace_id,
			pool_name,
			envoy_key,
			actor.envoy_protocol_version,
			actor_id,
			actor.key,
			actor_generation,
			stripped_path.to_string(),
		);
		Ok(RoutingOutput::CustomServe(std::sync::Arc::new(gateway)))
	} else {
		let lifecycle = rivet_guard_core::metrics::PegboardGatewayLifecycle::new(
			rivet_guard_core::metrics::PegboardGatewayVersion::V2,
			actor.envoy_protocol_version,
			request_kind,
		);
		let gateway = pegboard_gateway2::PegboardGateway2::new(
			ctx.clone(),
			shared_state.pegboard_gateway2.clone(),
			lifecycle,
			actor.namespace_id,
			pool_name,
			envoy_key,
			actor.envoy_protocol_version,
			actor_id,
			actor.key,
			None,
			stripped_path.to_string(),
		);
		Ok(RoutingOutput::CustomServe(std::sync::Arc::new(gateway)))
	}
}

async fn handle_actor_v1(
	ctx: &StandaloneCtx,
	shared_state: &SharedState,
	req_ctx: &RequestContext,
	actor_id: Id,
	actor: pegboard::ops::actor::get_for_gateway::Output,
	stripped_path: &str,
	skip_ready_wait: bool,
	mut ready_sub: SubscriptionHandle<pegboard::workflows::actor::Ready>,
	mut stopped_sub: SubscriptionHandle<pegboard::workflows::actor::Stopped>,
	mut fail_sub: SubscriptionHandle<pegboard::workflows::actor::Failed>,
	mut destroy_sub: SubscriptionHandle<pegboard::workflows::actor::DestroyStarted>,
	mut migrate_sub: SubscriptionHandle<pegboard::workflows::actor::MigratedToV2>,
	ready_sub2: SubscriptionHandle<pegboard::workflows::actor2::Ready>,
	stopped_sub2: SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	fail_sub2: SubscriptionHandle<pegboard::workflows::actor2::Failed>,
	destroy_sub2: SubscriptionHandle<pegboard::workflows::actor2::DestroyStarted>,
) -> Result<RoutingOutput> {
	// Wake actor if sleeping
	if actor.sleeping {
		tracing::debug!(?actor_id, "actor sleeping, waking");

		phase_timeout(
			Phase::new(
				"route_pegboard_wake_signal",
				&metrics::ROUTE_PEGBOARD_WAKE_SIGNAL_DURATION,
			)
			.with_namespace_id(actor.namespace_id)
			.with_actor_id(actor_id),
			ctx.config().guard().route_pegboard_wake_signal_timeout(),
			ctx.signal(pegboard::workflows::actor::Wake {
				allocation_override: pegboard::workflows::actor::AllocationOverride::DontSleep {
					pending_timeout: Some(ctx.config().guard().actor_force_wake_pending_timeout()),
				},
			})
			.to_workflow_id(actor.workflow_id)
			.send(),
			|elapsed, timeout| {
				pegboard::errors::RouteWakeSignalTimeout {
					actor_id: actor_id.to_string(),
					elapsed_ms: elapsed.as_millis() as u64,
					timeout_ms: timeout.as_millis() as u64,
				}
				.build()
			},
		)
		.await?;
	}

	let runner_id = if let (Some(runner_id), true) =
		(actor.runner_id, actor.connectable || skip_ready_wait)
	{
		runner_id
	} else {
		tracing::debug!(?actor_id, "waiting for actor to become ready");

		let mut wake_retries = 0;

		// Create pool error check future
		let runner_name_selector = actor.runner_name_selector.clone();
		let pool_error_check_fut =
			check_runner_pool_error_loop(ctx, actor.namespace_id, runner_name_selector.as_deref());
		tokio::pin!(pool_error_check_fut);

		// Wait for ready, fail, or destroy
		loop {
			tokio::select! {
				res = ready_sub.next() => break res?.runner_id,
				res = stopped_sub.next() => {
					res?;

					if wake_retries < 8 {
						tracing::debug!(?actor_id, ?wake_retries, "actor stopped while we were waiting for it to become ready, attempting rewake");
						wake_retries += 1;

						let res = phase_timeout(
							Phase::new(
								"route_pegboard_wake_signal",
								&metrics::ROUTE_PEGBOARD_WAKE_SIGNAL_DURATION,
							)
							.with_namespace_id(actor.namespace_id)
							.with_actor_id(actor_id),
							ctx.config().guard().route_pegboard_wake_signal_timeout(),
							ctx.signal(pegboard::workflows::actor::Wake {
								allocation_override: pegboard::workflows::actor::AllocationOverride::DontSleep {
									pending_timeout: Some(
										ctx.config().guard().actor_force_wake_pending_timeout(),
									),
								},
							})
							.to_workflow_id(actor.workflow_id)
							.graceful_not_found()
							.send(),
							|elapsed, timeout| {
								pegboard::errors::RouteWakeSignalTimeout {
									actor_id: actor_id.to_string(),
									elapsed_ms: elapsed.as_millis() as u64,
									timeout_ms: timeout.as_millis() as u64,
								}
								.build()
							},
						)
						.await?;

						if res.is_none() {
							tracing::warn!(
								?actor_id,
								"actor workflow not found for rewake"
							);
							return Err(pegboard::errors::Actor::NotFound.build());
						}
					} else {
						tracing::warn!("actor retried waking 8 times, has not yet started");
						return Err(rivet_guard_core::errors::ActorWakeRetriesExceeded {
							actor_id: actor_id.to_string(),
							wake_retries,
							reason: "actor_stopped_before_ready".to_owned(),
						}
						.build());
					}
				}
				res = fail_sub.next() => {
					let msg = res?;
					return Err(msg.error.clone().build());
				}
				res = destroy_sub.next() => {
					res?;
					return Err(pegboard::errors::Actor::DestroyedWhileWaitingForReady.build());
				}
				res = migrate_sub.next() => {
					res?;
					return handle_actor_v2(
						ctx,
						shared_state,
						req_ctx,
						actor_id,
						actor,
						stripped_path,
						skip_ready_wait,
						ready_sub2,
						stopped_sub2,
						fail_sub2,
						destroy_sub2,
					).await;
				}
				res = &mut pool_error_check_fut => {
					if res? {
						return Err(errors::ActorRunnerFailed { actor_id }.build());
					}
				}
				// Ready timeout
				_ = tokio::time::sleep(ctx.config().guard().actor_ready_timeout()) => {
					return Err(errors::ActorReadyTimeout { actor_id }.build());
				}
			}
		}
	};

	tracing::debug!(?actor_id, ?runner_id, "actor ready");

	// Return pegboard-gateway instance with path
	let gateway = pegboard_gateway::PegboardGateway::new(
		ctx.clone(),
		shared_state.pegboard_gateway.clone(),
		runner_id,
		actor_id,
		stripped_path.to_string(),
	);
	Ok(RoutingOutput::CustomServe(std::sync::Arc::new(gateway)))
}

fn read_gateway_token_for_path_based<'a>(
	req_ctx: &'a RequestContext,
	token_from_path: Option<&'a str>,
) -> Result<Option<&'a str>> {
	if let Some(token) = token_from_path {
		return Ok(Some(token));
	}

	if req_ctx.is_websocket() {
		let protocols_header = req_ctx
			.headers()
			.get(SEC_WEBSOCKET_PROTOCOL)
			.and_then(|protocols| protocols.to_str().ok())
			.ok_or_else(|| {
				crate::errors::MissingHeader {
					header: "sec-websocket-protocol".to_string(),
				}
				.build()
			})?;

		let protocols = protocols_header
			.split(',')
			.map(|p| p.trim())
			.collect::<Vec<&str>>();

		Ok(protocols
			.iter()
			.find_map(|p| p.strip_prefix(WS_PROTOCOL_TOKEN)))
	} else {
		req_ctx
			.headers()
			.get(X_RIVET_TOKEN)
			.map(|x| x.to_str())
			.transpose()
			.context("invalid x-rivet-token header")
	}
}

fn read_skip_ready_wait_for_path_based(req_ctx: &RequestContext) -> Result<bool> {
	if req_ctx.is_websocket() {
		Ok(req_ctx
			.headers()
			.get(SEC_WEBSOCKET_PROTOCOL)
			.and_then(|protocols| protocols.to_str().ok())
			.is_some_and(|protocols| {
				protocols
					.split(',')
					.map(|p| p.trim())
					.any(|p| p == WS_PROTOCOL_SKIP_READY_WAIT)
			}))
	} else {
		read_skip_ready_wait_header(req_ctx)
	}
}

fn read_skip_ready_wait_header(req_ctx: &RequestContext) -> Result<bool> {
	let Some(value) = req_ctx.headers().get(X_RIVET_SKIP_READY_WAIT) else {
		return Ok(false);
	};

	let value = value
		.to_str()
		.context("invalid x-rivet-skip-ready-wait header")?;
	parse_skip_ready_wait_bool(value).ok_or_else(|| {
		crate::errors::InvalidHeader {
			header: X_RIVET_SKIP_READY_WAIT.to_string(),
			detail: "expected true, false, 1, or 0".to_string(),
		}
		.build()
	})
}

fn parse_skip_ready_wait_bool(value: &str) -> Option<bool> {
	match value {
		"true" | "1" => Some(true),
		"false" | "0" => Some(false),
		_ => None,
	}
}

/// Waits for initial delay, then periodically checks for runner pool errors.
///
/// Returns `true` if the pool has an active error, `false` otherwise.
///
/// This is used to short circuit waiting for the actor to schedule by checking if the underlying
/// pool is unhealthy. The initial delay is intended to give the actor time to allocate cleanly in
/// case the pool status is flapping.
async fn check_runner_pool_error_loop(
	ctx: &StandaloneCtx,
	namespace_id: Id,
	runner_name: Option<&str>,
) -> Result<bool> {
	// Skip pool error check for actors that have not backfilled yet
	let Some(runner_name) = runner_name else {
		std::future::pending::<()>().await;
		unreachable!()
	};

	tokio::time::sleep(RUNNER_POOL_ERROR_CHECK_DELAY).await;

	loop {
		let errors = ctx
			.op(pegboard::ops::runner_config::get_error::Input {
				runners: vec![(namespace_id, runner_name.to_string())],
			})
			.await?;

		if let Some(entry) = errors.into_iter().next() {
			tracing::warn!(
				%namespace_id,
				%runner_name,
				error = ?entry.error,
				"runner pool has active error, fast-failing request"
			);
			return Ok(true);
		}

		// Wait before next check
		tokio::time::sleep(RUNNER_POOL_ERROR_CHECK_INTERVAL).await;
	}
}
