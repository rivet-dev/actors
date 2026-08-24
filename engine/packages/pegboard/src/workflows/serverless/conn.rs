use std::time::Instant;

use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use futures_util::{FutureExt, StreamExt};
use gas::prelude::*;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest_eventsource as sse;
use rivet_runner_protocol as protocol;
use rivet_runtime::TermSignal;
use rivet_types::actor::RunnerPoolError;
use rivet_types::runner_configs::RunnerConfigKind;
use rivet_util::safe_slice;
use tokio::time::Duration;
use universalpubsub::PublishOpts;
use vbare::OwnedVersionedData;

use crate::metrics;
use crate::pubsub_subjects::RunnerReceiverSubject;
use crate::workflows::{runner_pool, runner_pool_error_tracker, serverless::receiver};

const X_RIVET_ENDPOINT: HeaderName = HeaderName::from_static("x-rivet-endpoint");
const X_RIVET_TOKEN: HeaderName = HeaderName::from_static("x-rivet-token");
const X_RIVET_TOTAL_SLOTS: HeaderName = HeaderName::from_static("x-rivet-total-slots");
const X_RIVET_RUNNER_NAME: HeaderName = HeaderName::from_static("x-rivet-runner-name");
const X_RIVET_NAMESPACE_NAME: HeaderName = HeaderName::from_static("x-rivet-namespace-name");

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Input {
	pub pool_wf_id: Id,
	pub receiver_wf_id: Id,
	pub namespace_id: Id,
	pub runner_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct RescheduleState {
	last_retry_ts: i64,
	retry_count: usize,
}

#[workflow]
pub async fn pegboard_serverless_conn2(ctx: &mut WorkflowCtx, input: &Input) -> Result<()> {
	// Run the connection activity, which will handle the full lifecycle
	let drain_sent = ctx
		.loope(RescheduleState::default(), |ctx, state| {
			let input = input.clone();

			async move {
				let res = ctx
					.activity(OutboundReqInput {
						pool_wf_id: input.pool_wf_id,
						receiver_wf_id: input.receiver_wf_id,
						namespace_id: input.namespace_id,
						runner_name: input.runner_name.clone(),
					})
					.await?;

				if let OutboundReqOutput::Draining { drain_sent } = res {
					return Ok(Loop::Break(drain_sent));
				}

				let mut backoff = reconnect_backoff(
					state.retry_count,
					ctx.config().pegboard().serverless_base_retry_timeout(),
					ctx.config().pegboard().serverless_backoff_max_exponent(),
				);

				let retry_res = ctx
					.activity(CompareRetryInput {
						retry_count: state.retry_count,
						last_retry_ts: state.last_retry_ts,
					})
					.await?;

				state.retry_count = if retry_res.should_reset {
					0
				} else {
					state.retry_count + 1
				};
				state.last_retry_ts = retry_res.now;

				let next = backoff.step().expect("should not have max retry");
				if let Some(_sig) = ctx
					.listen_with_timeout::<Drain>(Instant::from(next) - Instant::now())
					.await?
				{
					tracing::debug!("drain received during serverless connection backoff");

					// Notify pool that drain started
					return Ok(Loop::Break(false));
				}

				Ok(Loop::Continue)
			}
			.boxed()
		})
		.await?;

	// If we failed to send inline during the activity, durably ensure the
	// signal is dispatched here
	if !drain_sent {
		ctx.signal(runner_pool::OutboundConnDrainStarted {
			receiver_wf_id: input.receiver_wf_id,
		})
		.to_workflow_id(input.pool_wf_id)
		.send()
		.await?;
	}

	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct CompareRetryInput {
	retry_count: usize,
	last_retry_ts: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompareRetryOutput {
	should_reset: bool,
	now: i64,
}

#[activity(CompareRetry)]
async fn compare_retry(ctx: &ActivityCtx, input: &CompareRetryInput) -> Result<CompareRetryOutput> {
	let now = util::timestamp::now();

	// If the last retry ts is more than RETRY_RESET_DURATION_MS ago, reset retry count
	let should_reset =
		input.last_retry_ts < now - ctx.config().pegboard().serverless_retry_reset_duration();

	Ok(CompareRetryOutput { should_reset, now })
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct OutboundReqInput {
	pool_wf_id: Id,
	receiver_wf_id: Id,
	namespace_id: Id,
	runner_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum OutboundReqOutput {
	Continue,
	Draining {
		/// Whether or not to retry sending the drain signal because it failed or was never sent.
		drain_sent: bool,
	},
	Retry,
}

#[activity(OutboundReq)]
#[timeout = u64::MAX]
async fn outbound_req(ctx: &ActivityCtx, input: &OutboundReqInput) -> Result<OutboundReqOutput> {
	let mut term_signal = TermSignal::get();

	loop {
		metrics::SERVERLESS_OUTBOUND_REQ_ACTIVE
			.with_label_values(&[&input.namespace_id.to_string(), &input.runner_name])
			.inc();

		let res = outbound_req_inner(ctx, input, &mut term_signal).await;

		metrics::SERVERLESS_OUTBOUND_REQ_ACTIVE
			.with_label_values(&[&input.namespace_id.to_string(), &input.runner_name])
			.dec();

		match res {
			// If the outbound req exited successfully, continue with no backoff
			Ok(OutboundReqOutput::Continue) => {
				if let Err(err) = ctx
					.signal(runner_pool::Bump::default())
					// This is ok because bumps are not stateful
					.bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING()
					.to_workflow_id(input.pool_wf_id)
					.send()
					.await
				{
					tracing::debug!(?err, "failed to send bump signal");

					return Ok(OutboundReqOutput::Draining { drain_sent: false });
				}
			}
			Ok(OutboundReqOutput::Draining { drain_sent }) => {
				return Ok(OutboundReqOutput::Draining { drain_sent });
			}
			Ok(OutboundReqOutput::Retry) => return Ok(OutboundReqOutput::Retry),
			Err(error) => {
				tracing::warn!(?error, "outbound_req_inner failed, retrying after backoff");
				return Ok(OutboundReqOutput::Retry);
			}
		}
	}
}

async fn outbound_req_inner(
	ctx: &ActivityCtx,
	input: &OutboundReqInput,
	term_signal: &mut TermSignal,
) -> Result<OutboundReqOutput> {
	if is_runner_draining(ctx, input.receiver_wf_id).await? {
		return Ok(OutboundReqOutput::Draining { drain_sent: false });
	}

	let (runner_config_res, namespace_res) = tokio::try_join!(
		ctx.op(crate::ops::runner_config::get::Input {
			runners: vec![(input.namespace_id, input.runner_name.clone())],
			bypass_cache: false,
		}),
		ctx.op(namespace::ops::get_global::Input {
			namespace_ids: vec![input.namespace_id],
		})
	)?;
	let Some(runner_config) = runner_config_res.into_iter().next() else {
		tracing::warn!(
			namespace_id = %input.namespace_id,
			runner_name = %input.runner_name,
			"runner config does not exist, ending outbound req"
		);
		return Ok(OutboundReqOutput::Draining { drain_sent: false });
	};

	let RunnerConfigKind::Serverless {
		url,
		headers,
		slots_per_runner,
		request_lifespan,
		..
	} = runner_config.config.kind
	else {
		tracing::warn!(
			namespace_id = %input.namespace_id,
			runner_name = %input.runner_name,
			"runner config is not serverless, ending outbound req"
		);
		return Ok(OutboundReqOutput::Draining { drain_sent: false });
	};

	let Some(namespace) = namespace_res.into_iter().next() else {
		tracing::error!("namespace not found, ending outbound req");
		report_error(
			ctx,
			input.namespace_id,
			&input.runner_name,
			RunnerPoolError::InternalError,
		)
		.await;
		return Ok(OutboundReqOutput::Draining { drain_sent: false });
	};

	let current_dc = ctx.config().topology().current_dc()?;

	let token = if let Some(auth) = &ctx.config().auth {
		Some((
			X_RIVET_TOKEN,
			HeaderValue::try_from(auth.admin_token.read())?,
		))
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
			(
				X_RIVET_TOTAL_SLOTS,
				HeaderValue::try_from(slots_per_runner)?,
			),
			(
				X_RIVET_RUNNER_NAME,
				HeaderValue::try_from(input.runner_name.clone())?,
			),
			(
				X_RIVET_NAMESPACE_NAME,
				HeaderValue::try_from(namespace.name.clone())?,
			),
			// Deprecated
			(
				HeaderName::from_static("x-rivet-namespace-id"),
				HeaderValue::try_from(namespace.name)?,
			),
		])
		.chain(token)
		.collect();

	let endpoint_url = format!("{}/start", url.trim_end_matches('/'));

	tracing::debug!(%endpoint_url, "sending outbound req");

	let client = rivet_pools::reqwest::client_no_timeout().await?;
	let req = client.get(endpoint_url).headers(headers);

	let conn_started = Instant::now();
	let mut source = sse::EventSource::new(req).context("failed creating event source")?;
	let mut runner_id = None;
	let mut runner_protocol_version = None;

	let runner_protocol_version2 = &mut runner_protocol_version;
	let stream_handler = async {
		while let Some(event) = source.next().await {
			match event {
				Ok(event) => {
					// Parse payload
					let payload = match parse_to_serverless_server_event(&event) {
						Ok(Some(x)) => x,
						Ok(None) => continue,
						Err(err) => {
							report_error(
								ctx,
								input.namespace_id,
								&input.runner_name,
								RunnerPoolError::ServerlessInvalidSsePayload {
									message: err.to_string(),
									raw_payload: get_raw_event_data(&event)
										.map(|x| safe_slice(x, 0, 512).to_string()),
								},
							)
							.await;

							return Err(err);
						}
					};

					// Handle message
					match payload {
						protocol::mk2::ToServerlessServer::ToServerlessServerInit(init) => {
							if runner_id.is_none() {
								runner_id =
									Some(Id::parse(&init.runner_id).context("invalid runner id")?);
								*runner_protocol_version2 = Some(init.runner_protocol_version);

								// Report success to error tracker - runner initialized successfully
								report_success(ctx, input.namespace_id, &input.runner_name).await;
							}
						}
					}
				}
				Err(sse::Error::StreamEnded) => {
					let init_received = runner_id.is_some();
					let elapsed_ms = conn_started.elapsed().as_millis() as u64;

					tracing::warn!(
						namespace_id = %input.namespace_id,
						runner_name = %input.runner_name,
						init_received,
						elapsed_ms,
						"serverless SSE stream ended"
					);

					report_error(
						ctx,
						input.namespace_id,
						&input.runner_name,
						RunnerPoolError::ServerlessStreamEndedEarly,
					)
					.await;

					return Ok(());
				}
				Err(sse::Error::InvalidStatusCode(code, res)) => {
					let body = res
						.text()
						.await
						.unwrap_or_else(|_| "<could not read body>".to_string());
					let body_slice = util::safe_slice(&body, 0, 512).to_string();

					report_error(
						ctx,
						input.namespace_id,
						&input.runner_name,
						RunnerPoolError::ServerlessHttpError {
							status_code: code.as_u16(),
							body: body_slice.clone(),
						},
					)
					.await;

					bail!("invalid status code ({code}):\n{}", body_slice);
				}
				Err(err) => {
					match &err {
						reqwest_eventsource::Error::InvalidContentType(value, _) => {
							report_error(
								ctx,
								input.namespace_id,
								&input.runner_name,
								RunnerPoolError::ServerlessConnectionError {
									message: format!(
										"expected Content-Type header to be text/event-stream, received {value:?}"
									),
								},
							)
							.await;

							return Err(anyhow::Error::from(err));
						}
						_ => {
							let wrapped_err = anyhow::Error::from(err);

							report_error(
								ctx,
								input.namespace_id,
								&input.runner_name,
								RunnerPoolError::ServerlessConnectionError {
									// Print entire error chain
									message: wrapped_err
										.chain()
										.map(|err| err.to_string())
										.collect::<Vec<_>>()
										.join("\n"),
								},
							)
							.await;

							return Err(wrapped_err);
						}
					}
				}
			}
		}

		anyhow::Ok(())
	};

	metrics::SERVERLESS_OUTBOUND_REQ_TOTAL
		.with_label_values(&[&input.namespace_id.to_string(), &input.runner_name])
		.inc();

	let sleep_until_drain = Duration::from_secs(request_lifespan as u64).saturating_sub(
		Duration::from_millis(ctx.config().pegboard().serverless_drain_grace_period()),
	);
	tokio::select! {
		res = stream_handler => {
			match res {
				// If the outbound req was stopped from the client side, we can just continue the loop
				Ok(_) => return Ok(OutboundReqOutput::Continue),
				Err(e) => return Err(e.into()),
			}
		},
		_ = tokio::time::sleep(sleep_until_drain) => {}
		_ = term_signal.recv() => {}
	};

	tracing::debug!(?runner_id, "connection reached lifespan, starting drain");

	if let Err(err) = ctx
		.signal(runner_pool::OutboundConnDrainStarted {
			receiver_wf_id: input.receiver_wf_id,
		})
		// This is ok, because we only send DrainStarted once
		.bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING()
		.to_workflow_id(input.pool_wf_id)
		.send()
		.await
	{
		tracing::debug!(?err, "failed to send drain signal");

		return Ok(OutboundReqOutput::Draining { drain_sent: false });
	}

	// After we tell the pool we're draining, any remaining failures
	// don't matter as the pool already stopped caring about us.
	if let Err(err) =
		finish_non_critical_draining(ctx, term_signal, source, runner_id, runner_protocol_version)
			.await
	{
		tracing::debug!(?err, "failed non critical draining phase");
	}

	Ok(OutboundReqOutput::Draining { drain_sent: true })
}

/// Reads from the adjacent serverless runner wf which is keeping track of signals while this workflow runs
/// outbound requests.
#[tracing::instrument(skip_all)]
async fn is_runner_draining(ctx: &ActivityCtx, receiver_wf_id: Id) -> Result<bool> {
	let receiver_wf = ctx
		.get_workflows(vec![receiver_wf_id])
		.await?
		.into_iter()
		.next()
		.context("cannot find own runner wf")?;
	let state = receiver_wf.parse_state::<receiver::State>()?;

	Ok(state.is_draining)
}

#[tracing::instrument(skip_all)]
async fn finish_non_critical_draining(
	ctx: &ActivityCtx,
	term_signal: &mut TermSignal,
	mut source: sse::EventSource,
	mut runner_id: Option<Id>,
	mut runner_protocol_version: Option<u16>,
) -> Result<()> {
	if let Some(runner_id) = runner_id {
		drain_runner(ctx, runner_id).await?;
	}

	// Continue waiting on req while draining
	let runner_protocol_version2 = &mut runner_protocol_version;
	let wait_for_shutdown_fut = async move {
		while let Some(event) = source.next().await {
			match event {
				Ok(event) => {
					let Some(payload) = parse_to_serverless_server_event(&event)? else {
						continue;
					};

					match payload {
						protocol::mk2::ToServerlessServer::ToServerlessServerInit(init) => {
							// If runner_id is none at this point it means we did not send the stopping signal yet, so
							// send it now
							if runner_id.is_none() {
								let runner_id_local =
									Id::parse(&init.runner_id).context("invalid runner id")?;
								runner_id = Some(runner_id_local);
								*runner_protocol_version2 = Some(init.runner_protocol_version);
								drain_runner(ctx, runner_id_local).await?;
							}
						}
					}
				}
				Err(sse::Error::StreamEnded) => break,
				Err(err) => return Err(err.into()),
			}
		}

		Result::<()>::Ok(())
	};

	// Wait for runner to shut down
	tokio::select! {
		res = wait_for_shutdown_fut => return res.map_err(Into::into),
		_ = tokio::time::sleep(Duration::from_millis(ctx.config().pegboard().serverless_drain_grace_period())) => {}
		_ = term_signal.recv() => {}
	}

	// Close connection
	//
	// This will force the runner to stop the request in order to avoid hitting the serverless
	// timeout threshold
	if let (Some(runner_id), Some(runner_protocol_version)) = (runner_id, runner_protocol_version) {
		publish_to_client_stop(ctx, runner_id, runner_protocol_version).await?;
	}

	tracing::debug!(?runner_id, "outbound req stopped");

	Ok(())
}

#[tracing::instrument(skip_all)]
async fn drain_runner(ctx: &ActivityCtx, runner_id: Id) -> Result<()> {
	let res = ctx
		.signal(crate::workflows::runner2::Stop {
			reset_actor_rescheduling: true,
		})
		// This is ok, because runner_id changes every retry of outbound_req
		.bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING()
		.to_workflow::<crate::workflows::runner2::Workflow>()
		.tag("runner_id", runner_id)
		.graceful_not_found()
		.send()
		.await?;

	if res.is_none() {
		// Retry with old runner wf
		let res = ctx
			.signal(crate::workflows::runner::Stop {
				reset_actor_rescheduling: true,
			})
			.bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING()
			.to_workflow::<crate::workflows::runner::Workflow>()
			.tag("runner_id", runner_id)
			.graceful_not_found()
			.send()
			.await?;

		if res.is_none() {
			tracing::warn!(
				?runner_id,
				"runner workflow not found, likely already stopped"
			);
		}
	}

	Ok(())
}

/// Send a stop message to the client.
///
/// This will close the runner's WebSocket.
#[tracing::instrument(skip_all)]
async fn publish_to_client_stop(
	ctx: &ActivityCtx,
	runner_id: Id,
	runner_protocol_version: u16,
) -> Result<()> {
	let receiver_subject = RunnerReceiverSubject::new(runner_id).to_string();

	let message_serialized = if protocol::is_mk2(runner_protocol_version) {
		protocol::versioned::ToRunnerMk2::wrap_latest(protocol::mk2::ToRunner::ToRunnerClose)
			.serialize_with_embedded_version(protocol::PROTOCOL_MK2_VERSION)?
	} else {
		protocol::versioned::ToRunner::wrap_latest(protocol::ToRunner::ToClientClose)
			.serialize_with_embedded_version(protocol::PROTOCOL_MK1_VERSION)?
	};

	ctx.ups()?
		.publish(&receiver_subject, &message_serialized, PublishOpts::one())
		.await?;

	Ok(())
}

#[message("pegboard_serverless_conn_drain")]
#[signal("pegboard_serverless_conn_drain")]
pub struct Drain {}

fn reconnect_backoff(
	retry_count: usize,
	base_retry_timeout: usize,
	max_exponent: usize,
) -> util::throttle::Backoff {
	util::throttle::Backoff::new_at(max_exponent, None, base_retry_timeout, 500, retry_count)
}

/// Report an error to the error tracker workflow.
async fn report_error(
	ctx: &ActivityCtx,
	namespace_id: Id,
	runner_name: &str,
	error: RunnerPoolError,
) {
	if let Err(err) = ctx
		.signal(runner_pool_error_tracker::ReportError { error })
		.bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING()
		.to_workflow::<runner_pool_error_tracker::Workflow>()
		.tags(serde_json::json!({
			"namespace_id": namespace_id,
			"runner_name": runner_name,
		}))
		.graceful_not_found()
		.send()
		.await
	{
		tracing::warn!(?err, "failed to report serverless error");
	}
}

/// Report success to the error tracker workflow.
async fn report_success(ctx: &ActivityCtx, namespace_id: Id, runner_name: &str) {
	if let Err(err) = ctx
		.signal(runner_pool_error_tracker::ReportSuccess {})
		.bypass_signal_from_workflow_I_KNOW_WHAT_IM_DOING()
		.to_workflow::<runner_pool_error_tracker::Workflow>()
		.tags(serde_json::json!({
			"namespace_id": namespace_id,
			"runner_name": runner_name,
		}))
		.graceful_not_found()
		.send()
		.await
	{
		tracing::warn!(?err, "failed to report serverless success");
	}
}

/// Parse SSE event into ToServerlessServer and handle all event types.
fn parse_to_serverless_server_event(
	event: &reqwest_eventsource::Event,
) -> Result<Option<protocol::mk2::ToServerlessServer>> {
	match event {
		sse::Event::Open => Ok(None),
		sse::Event::Message(msg) => match msg.event.as_str() {
			"ping" => Ok(None),
			"message" => {
				let data = BASE64.decode(&msg.data).context("invalid base64 message")?;
				let payload =
					protocol::versioned::ToServerlessServer::deserialize_with_embedded_version(
						&data,
					)
					.context("invalid payload")?;

				Ok(Some(payload))
			}
			event => {
				tracing::warn!(event, "received unknown serverless sse message event kind");
				Ok(None)
			}
		},
	}
}

/// Get the data from the event, if exists.
fn get_raw_event_data(event: &reqwest_eventsource::Event) -> Option<&str> {
	match event {
		reqwest_eventsource::Event::Open => None,
		reqwest_eventsource::Event::Message(ev) => Some(&ev.data),
	}
}
