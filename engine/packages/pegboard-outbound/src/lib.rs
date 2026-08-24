use anyhow::Result;
use futures_util::{StreamExt, stream::FuturesUnordered};
use gas::prelude::*;
use pegboard::pubsub_subjects::ServerlessOutboundSubject;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest_eventsource as sse;
use rivet_envoy_protocol::{self as protocol, PROTOCOL_VERSION, versioned};
use rivet_runtime::TermSignal;
use rivet_types::actor::RunnerPoolError;
use rivet_types::runner_configs::RunnerConfigKind;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use universalpubsub::NextOutput;
use vbare::OwnedVersionedData;

pub mod metrics;

const X_RIVET_ENDPOINT: HeaderName = HeaderName::from_static("x-rivet-endpoint");
const X_RIVET_POOL_NAME: HeaderName = HeaderName::from_static("x-rivet-pool-name");
const X_RIVET_TOKEN: HeaderName = HeaderName::from_static("x-rivet-token");
const X_RIVET_NAMESPACE_NAME: HeaderName = HeaderName::from_static("x-rivet-namespace-name");
const SHUTDOWN_PROGRESS_INTERVAL: Duration = Duration::from_secs(7);
const SSE_OPEN_WARN_THRESHOLD: Duration = Duration::from_secs(5);

#[tracing::instrument(skip_all)]
pub async fn start(config: rivet_config::Config, pools: rivet_pools::Pools) -> Result<()> {
	let cache = rivet_cache::CacheInner::from_env(&config, pools.clone())?;
	let ctx = StandaloneCtx::new(
		db::DatabaseKv::new(config.clone(), pools.clone()).await?,
		config.clone(),
		pools,
		cache,
		"pegboard_outbound",
		Id::new_v1(config.dc_label()),
		Id::new_v1(config.dc_label()),
	)?;

	let mut conns = Vec::new();

	let res = inner(&ctx, &mut conns).await;

	if res.is_err() {
		// Abort all futures in the event of a fatal error. This is not ideal
		for conn in &conns {
			conn.handle.abort();
		}
	}

	let mut term_signal = TermSignal::get();
	let shutdown_duration = config.runtime.guard_shutdown_duration();
	tracing::info!(remaining_conns=%conns.len(), duration=?shutdown_duration, "starting outbound shutdown");

	let mut conn_futs = conns
		.iter_mut()
		.map(|conn| &mut conn.handle)
		.collect::<FuturesUnordered<_>>();

	let mut progress_interval = tokio::time::interval(SHUTDOWN_PROGRESS_INTERVAL);
	progress_interval.tick().await;

	let shutdown_start = Instant::now();
	loop {
		// Future will resolve once all tasks complete
		let complete_fut = async { while let Some(_) = conn_futs.next().await {} };

		tokio::select! {
			// Wait for remaining conns to stop
			_ = complete_fut => {
				break;
			}
			_ = progress_interval.tick() => {
				tracing::info!(remaining_conns=%conn_futs.len(), "outbound still shutting down");
			}
			abort = term_signal.recv() => {
				if abort {
					tracing::warn!("aborting outbound shutdown");
					break;
				}
			}
			_ = tokio::time::sleep(shutdown_duration.saturating_sub(shutdown_start.elapsed())) => {
				tracing::warn!("outbound shutdown timed out");
				break;
			}
		}
	}

	let remaining_conns = conn_futs.into_iter().count();
	if remaining_conns == 0 {
		tracing::info!("all outbound connections complete");
	} else {
		tracing::warn!(%remaining_conns, "not all outbound connections completed");
	}

	tracing::info!("outbound shutdown complete");

	res
}

#[tracing::instrument(skip_all)]
async fn inner(ctx: &StandaloneCtx, conns: &mut Vec<OutboundHandler>) -> Result<()> {
	let mut sub = ctx
		.ups()?
		.queue_subscribe(ServerlessOutboundSubject, "service")
		.await?;
	let mut term_signal = TermSignal::get();

	loop {
		tokio::select! {
			msg = sub.next() => {
				match msg? {
					NextOutput::Message(msg) => {
						match protocol::versioned::ToOutbound::deserialize_with_embedded_version(&msg.payload) {
							Ok(packet) => {
								// TODO: Use interval for gc instead of for every packet
								// Clean up finished conns
								conns.retain(|c| !c.handle.is_finished());

								conns.push(OutboundHandler::new(ctx, packet))
							}
							Err(err) => {
								tracing::error!(?err, "received invalid outbound message");
							}
						}
					},
					NextOutput::Unsubscribed => bail!("outbound sub unsubscribed"),
					NextOutput::NoResponders => bail!("outbound sub no responders"),
				}
			}
			_ = term_signal.recv() => return Ok(()),
		}
	}
}

// Handles serverless (/start request)
struct OutboundHandler {
	handle: JoinHandle<()>,
}

impl OutboundHandler {
	fn new(ctx: &StandaloneCtx, packet: protocol::ToOutbound) -> Self {
		let ctx = ctx.clone();
		let handle = tokio::spawn(
			async move {
				if let Err(err) = handle(&ctx, packet).await {
					tracing::warn!(?err, "outbound handler failed");
				}
			}
			.instrument(tracing::info_span!("outbound_handle_task")),
		);

		OutboundHandler { handle }
	}
}

#[tracing::instrument(skip_all, fields(namespace_id = tracing::field::Empty, pool_name = tracing::field::Empty, actor_id = tracing::field::Empty, url = tracing::field::Empty))]
async fn handle(ctx: &StandaloneCtx, packet: protocol::ToOutbound) -> Result<()> {
	let (namespace_id, pool_name, checkpoint, actor_config) = match packet {
		protocol::ToOutbound::ToOutboundActorStart(protocol::ToOutboundActorStart {
			namespace_id,
			pool_name,
			checkpoint,
			actor_config,
		}) => (namespace_id, pool_name, checkpoint, actor_config),
	};

	let current_span = tracing::Span::current();
	current_span.record("namespace_id", &namespace_id);
	current_span.record("pool_name", &pool_name);
	current_span.record("actor_id", &checkpoint.actor_id);

	let namespace_id = Id::parse(&namespace_id)?;
	let actor_id = Id::parse(&checkpoint.actor_id)?;
	let generation = checkpoint.generation;
	let namespace_id_str = namespace_id.to_string();
	metrics::REQ_BY_GENERATION_TOTAL
		.with_label_values(&[
			namespace_id_str.as_str(),
			pool_name.as_str(),
			generation_bucket(generation),
		])
		.inc();

	tracing::debug!(?namespace_id, %pool_name, ?actor_id, ?generation, "received outbound request");

	let db = ctx.udb()?;
	let (namespace_res, pool_res, hibernating_requests, preloaded_kv) = tokio::try_join!(
		ctx.op(namespace::ops::get_global::Input {
			namespace_ids: vec![namespace_id],
		}),
		ctx.op(pegboard::ops::runner_config::get::Input {
			runners: vec![(namespace_id, pool_name.clone())],
			bypass_cache: false,
		}),
		ctx.op(pegboard::ops::actor::hibernating_request::list::Input { actor_id }),
		pegboard::actor_kv::preload::fetch_preloaded_kv(
			&db,
			ctx.config().pegboard(),
			actor_id,
			namespace_id,
			&actor_config.name,
		),
	)?;
	let Some(namespace) = namespace_res.into_iter().next() else {
		tracing::error!("namespace not found, ending outbound handler");
		report_error(
			ctx,
			namespace_id,
			&pool_name,
			RunnerPoolError::InternalError,
		)
		.await;
		return Ok(());
	};
	let Some(pool) = pool_res.into_iter().next() else {
		tracing::debug!("pool does not exist, ending outbound handler");
		return Ok(());
	};

	let RunnerConfigKind::Serverless {
		url,
		headers,
		request_lifespan,
		drain_grace_period,
		..
	} = pool.config.kind
	else {
		tracing::warn!(
			?actor_id,
			"config no longer serverless, ignoring outbound allocation"
		);
		return Ok(());
	};

	tracing::Span::current().record("url", &url);

	let protocol_version = pool.protocol_version.unwrap_or(PROTOCOL_VERSION);
	let res = async {
		let payload_body =
			versioned::ToEnvoy::wrap_latest(protocol::ToEnvoy::ToEnvoyCommands(vec![
				protocol::CommandWrapper {
					checkpoint,
					inner: protocol::Command::CommandStartActor(protocol::CommandStartActor {
						config: actor_config,
						hibernating_requests: hibernating_requests
							.into_iter()
							.map(|x| protocol::HibernatingRequest {
								gateway_id: x.gateway_id,
								request_id: x.request_id,
							})
							.collect(),
						preloaded_kv,
					}),
				},
			]))
			.serialize(protocol_version)?;
		let mut payload = Vec::with_capacity(2 + payload_body.len());
		payload.extend_from_slice(&protocol_version.to_le_bytes());
		payload.extend_from_slice(&payload_body);

		// Send ack to actor wf before starting an outbound req
		ctx.signal(pegboard::workflows::actor2::Allocated { generation })
			.to_workflow::<pegboard::workflows::actor2::Workflow>()
			.tag("actor_id", &actor_id)
			.send()
			.await?;

		let token = if let Some(auth) = &ctx.config().auth {
			Some(auth.admin_token.read().as_str())
		} else {
			None
		};

		metrics::REQ_ACTIVE
			.with_label_values(&[&namespace_id.to_string(), &pool_name])
			.inc();

		let res = serverless_outbound_req(
			ctx,
			namespace_id,
			&pool_name,
			&namespace.name,
			actor_id,
			generation,
			payload,
			&url,
			headers,
			request_lifespan,
			drain_grace_period,
			token,
		)
		.await;

		metrics::REQ_ACTIVE
			.with_label_values(&[&namespace_id.to_string(), &pool_name])
			.dec();

		res
	}
	.await;

	res
}

fn generation_bucket(generation: u32) -> &'static str {
	match generation {
		0 => "0",
		1 => "1",
		2 => "2",
		3..=5 => "3-5",
		6..=10 => "6-10",
		11..=u32::MAX => "11+",
	}
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum DrainReason {
	LifespanReached,
	GoingAway,
	Stopping,
	ConnectionLost,
	TermSignal,
}

fn error_label(error: &RunnerPoolError) -> &'static str {
	match error {
		RunnerPoolError::ServerlessHttpError { .. } => "http_error",
		RunnerPoolError::ServerlessConnectionError { .. } => "connection_error",
		RunnerPoolError::ServerlessStreamEndedEarly => "stream_ended_early",
		RunnerPoolError::ServerlessInvalidSsePayload { .. } => "invalid_payload",
		RunnerPoolError::Downgrade => "downgrade",
		RunnerPoolError::InternalError => "internal",
	}
}

fn status_label(error: &RunnerPoolError) -> &'static str {
	match error {
		RunnerPoolError::ServerlessHttpError { status_code, .. } => match status_code {
			429 => "429",
			503 => "503",
			500..=599 => "5xx",
			400..=499 => "4xx",
			200..=299 => "2xx",
			_ => "other",
		},
		RunnerPoolError::ServerlessConnectionError { .. }
		| RunnerPoolError::ServerlessStreamEndedEarly
		| RunnerPoolError::ServerlessInvalidSsePayload { .. }
		| RunnerPoolError::Downgrade
		| RunnerPoolError::InternalError => "",
	}
}

fn error_result_label(error: &RunnerPoolError) -> &'static str {
	match error {
		RunnerPoolError::ServerlessHttpError { status_code, .. } => match status_code {
			429 => "error_http_429",
			503 => "error_http_503",
			500..=599 => "error_http_5xx",
			400..=499 => "error_http_4xx",
			_ => "error_http_other",
		},
		RunnerPoolError::ServerlessConnectionError { .. } => "error_connection",
		RunnerPoolError::ServerlessStreamEndedEarly => "error_stream_ended",
		RunnerPoolError::ServerlessInvalidSsePayload { .. } => "error_invalid_payload",
		RunnerPoolError::Downgrade => "error_downgrade",
		RunnerPoolError::InternalError => "error_internal",
	}
}

fn drain_reason_label(reason: DrainReason) -> &'static str {
	match reason {
		DrainReason::LifespanReached => "lifespan_reached",
		DrainReason::GoingAway => "going_away",
		DrainReason::Stopping => "stopping",
		DrainReason::ConnectionLost => "connection_lost",
		DrainReason::TermSignal => "term_signal",
	}
}

fn observe_req_duration(
	namespace_id: Id,
	pool_name: &str,
	req_start: Instant,
	result: &str,
	drain_reason: &str,
) {
	let namespace_id = namespace_id.to_string();
	metrics::REQ_DURATION_SECONDS
		.with_label_values(&[namespace_id.as_str(), pool_name, result, drain_reason])
		.observe(req_start.elapsed().as_secs_f64());
}

#[tracing::instrument(skip_all)]
async fn serverless_outbound_req(
	ctx: &StandaloneCtx,
	namespace_id: Id,
	pool_name: &str,
	namespace_name: &str,
	actor_id: Id,
	generation: u32,
	payload: Vec<u8>,
	url: &str,
	headers: HashMap<String, String>,
	request_lifespan: u32,
	drain_grace_period: u32,
	token: Option<&str>,
) -> Result<()> {
	let current_dc = ctx.config().topology().current_dc()?;
	let mut term_signal = TermSignal::get();

	let token_header = if let Some(token) = token {
		Some((X_RIVET_TOKEN, HeaderValue::try_from(token)?))
	} else {
		None
	};

	let headers = headers
		.into_iter()
		.flat_map(|(k, v)| {
			// NOTE: This will filter out invalid headers without warning
			Some((
				k.parse::<HeaderName>().ok()?,
				v.parse::<HeaderValue>().ok()?,
			))
		})
		.chain([
			(
				X_RIVET_ENDPOINT,
				HeaderValue::try_from(current_dc.public_url.to_string())?,
			),
			(X_RIVET_POOL_NAME, HeaderValue::try_from(pool_name)?),
			(
				X_RIVET_NAMESPACE_NAME,
				HeaderValue::try_from(namespace_name)?,
			),
		])
		.chain(token_header)
		.collect();

	let endpoint_url = format!("{}/start", url.trim_end_matches('/'));

	let client = rivet_pools::reqwest::client_no_timeout().await?;
	let req = client
		.post(endpoint_url.clone())
		.body(payload)
		.headers(headers);
	let req_start = Instant::now();
	let mut source = sse::EventSource::new(req).context("failed creating event source")?;

	tracing::debug!(%endpoint_url, "sending outbound req");

	let namespace_id_label = namespace_id.to_string();
	let stream_handler = async {
		while let Some(event) = source.next().await {
			match event {
				Ok(event) => match event {
					sse::Event::Open => {
						let open_duration = req_start.elapsed();
						metrics::REQ_SSE_OPEN_DURATION
							.with_label_values(&[namespace_id_label.as_str(), pool_name])
							.observe(open_duration.as_secs_f64());

						if open_duration > SSE_OPEN_WARN_THRESHOLD {
							tracing::warn!(
								?namespace_id,
								%pool_name,
								?actor_id,
								%generation,
								%endpoint_url,
								duration_ms = open_duration.as_millis(),
								threshold_ms = SSE_OPEN_WARN_THRESHOLD.as_millis(),
								"serverless outbound sse open was slow"
							);
						}

						tracing::debug!(%endpoint_url, "sse stream opened");
					}
					sse::Event::Message(msg) => {
						match msg.event.as_str() {
							"ping" => {}
							// Special message that allows the client to close first. This is used when the
							// envoy's actor stops so that it can close early instead of waiting for the entire
							// request lifespan.
							"stopping" => return Ok(Some(DrainReason::Stopping)),
							event => {
								tracing::warn!(
									event,
									"received unknown serverless sse message event kind"
								);
							}
						}
					}
				},
				Err(sse::Error::StreamEnded) => {
					tracing::debug!("outbound req stopped early");

					let error = RunnerPoolError::ServerlessStreamEndedEarly;
					report_error(ctx, namespace_id, &pool_name, error.clone()).await;
					observe_req_duration(
						namespace_id,
						pool_name,
						req_start,
						error_result_label(&error),
						"",
					);

					return Ok(None);
				}
				Err(sse::Error::InvalidStatusCode(code, res)) => {
					let body = res
						.text()
						.await
						.unwrap_or_else(|_| "<could not read body>".to_string());
					let body_slice = util::safe_slice(&body, 0, 512).to_string();

					let error = RunnerPoolError::ServerlessHttpError {
						status_code: code.as_u16(),
						body: body_slice.clone(),
					};
					report_error(ctx, namespace_id, &pool_name, error.clone()).await;
					observe_req_duration(
						namespace_id,
						pool_name,
						req_start,
						error_result_label(&error),
						"",
					);

					bail!("invalid status code ({code}):\n{body_slice}");
				}
				Err(sse::Error::InvalidContentType(value, _)) => {
					let error = RunnerPoolError::ServerlessConnectionError {
						message: format!(
							"expected Content-Type header to be text/event-stream, received {value:?}"
						),
					};
					report_error(ctx, namespace_id, &pool_name, error.clone()).await;
					observe_req_duration(
						namespace_id,
						pool_name,
						req_start,
						error_result_label(&error),
						"",
					);

					bail!("invalid content type: {value:?}");
				}
				Err(err) => {
					let wrapped_err = anyhow::Error::from(err);

					let error = RunnerPoolError::ServerlessConnectionError {
						// Print entire error chain
						message: wrapped_err
							.chain()
							.map(|err| err.to_string())
							.collect::<Vec<_>>()
							.join("\n"),
					};
					report_error(ctx, namespace_id, &pool_name, error.clone()).await;
					observe_req_duration(
						namespace_id,
						pool_name,
						req_start,
						error_result_label(&error),
						"",
					);

					return Err(wrapped_err);
				}
			}
		}

		anyhow::Ok(None)
	}
	.instrument(tracing::info_span!("stream_handler"));

	metrics::REQ_TOTAL
		.with_label_values(&[namespace_id.to_string().as_str(), pool_name])
		.inc();

	let sleep_until_drain =
		Duration::from_secs(request_lifespan.saturating_sub(drain_grace_period) as u64);
	let drain_reason = tokio::select! {
		res = stream_handler => {
			// Mark actor as lost for immediate reallocation if SSE errors
			if res.is_err() {
				let res = ctx.signal(pegboard::workflows::actor2::Lost {
						generation,
						reason: pegboard::workflows::actor2::LostReason::EnvoyConnectionLost,
					})
					.to_workflow::<pegboard::workflows::actor2::Workflow>()
					.tag("actor_id", &actor_id)
					.graceful_not_found()
					.send()
					.await?;

				if res.is_none() {
					tracing::warn!(
						?actor_id,
						"actor workflow not found for lost signal"
					);
				}
			}

			if let Ok(Some(drain_reason)) = &res {
				observe_req_duration(
					namespace_id,
					pool_name,
					req_start,
					"success",
					drain_reason_label(*drain_reason),
				);
			}

			return res.map(|_| ());
		}
		_ = tokio::time::sleep(sleep_until_drain) => DrainReason::LifespanReached,
		_ = term_signal.recv() => DrainReason::TermSignal,
	};

	tracing::debug!("connection reached lifespan, starting drain");

	// Start actor reallocation
	let res = ctx
		.signal(pegboard::workflows::actor2::GoingAway { generation })
		.to_workflow::<pegboard::workflows::actor2::Workflow>()
		.tag("actor_id", &actor_id)
		.graceful_not_found()
		.send()
		.await?;

	if res.is_none() {
		tracing::warn!(?actor_id, "actor workflow not found for going away signal");
	}

	// Check if stream closed
	if !matches!(source.ready_state(), sse::ReadyState::Closed) {
		// Handle sse events as no-op while we drain. This checks for early exits
		let stream_handler = async {
			while let Some(event) = source.next().await {
				match event {
					Ok(event) => match event {
						sse::Event::Open => {}
						sse::Event::Message(msg) => match msg.event.as_str() {
							"ping" => {}
							"stopping" => return,
							_ => {}
						},
					},
					Err(_) => return,
				}
			}
		};

		tokio::select! {
			_ = stream_handler => {}
			// Wait for the grace period
			_ = tokio::time::sleep(Duration::from_secs(drain_grace_period as u64)) => {}
		}
	}

	tracing::debug!("outbound req stopped");

	observe_req_duration(
		namespace_id,
		pool_name,
		req_start,
		"success",
		drain_reason_label(drain_reason),
	);

	Ok(())
}

/// Report an error to the error tracker workflow.
#[tracing::instrument(skip_all)]
async fn report_error(
	ctx: &StandaloneCtx,
	namespace_id: Id,
	pool_name: &str,
	error: RunnerPoolError,
) {
	let namespace_id_str = namespace_id.to_string();
	metrics::REQ_ERROR_TOTAL
		.with_label_values(&[
			namespace_id_str.as_str(),
			pool_name,
			error_label(&error),
			status_label(&error),
		])
		.inc();

	if let Err(err) = ctx
		.signal(pegboard::workflows::runner_pool_error_tracker::ReportError { error })
		.to_workflow::<pegboard::workflows::runner_pool_error_tracker::Workflow>()
		.tag("namespace_id", namespace_id)
		.tag("runner_name", pool_name)
		.graceful_not_found()
		.send()
		.await
	{
		tracing::warn!(?err, "failed to report serverless error");
	}
}
