use std::time::Duration;

use gas::prelude::*;
use rivet_types::actor::RunnerPoolError;
use webhook::types::{WebhookEvent, WebhookEventType};

const SIGNAL_DEBOUNCE: Duration = Duration::from_millis(250);
const SIGNAL_BATCH_SIZE: usize = 1024;

// Cap on fields that carry bytes straight from the user's serverless endpoint. These are
// unvetted upstream output, and a webhook forwards them to a third party and stores them in the
// delivery record, which is a single UDB value bound by FoundationDB's 100KB limit. Truncating
// keeps a large error page from breaking delivery recording and bounds what gets relayed.
const MAX_RAW_FIELD_BYTES: usize = 4096;

fn truncate_raw(value: &str) -> String {
	if value.len() <= MAX_RAW_FIELD_BYTES {
		return value.to_string();
	}

	// Step back to a char boundary so the truncated string stays valid UTF-8.
	let mut end = MAX_RAW_FIELD_BYTES;
	while end > 0 && !value.is_char_boundary(end) {
		end -= 1;
	}

	format!("{}... (truncated)", &value[..end])
}

// Bounds the passthrough fields before an error is sent outside the engine. Engine-authored
// strings such as `message` and `reason` are left alone; only fields echoing the user's endpoint
// verbatim are truncated.
fn truncate_error_for_webhook(error: &RunnerPoolError) -> RunnerPoolError {
	match error {
		RunnerPoolError::ServerlessHttpError { status_code, body } => {
			RunnerPoolError::ServerlessHttpError {
				status_code: *status_code,
				body: truncate_raw(body),
			}
		}
		RunnerPoolError::ServerlessInvalidSsePayload {
			message,
			raw_payload,
		} => RunnerPoolError::ServerlessInvalidSsePayload {
			message: message.clone(),
			raw_payload: raw_payload.as_deref().map(truncate_raw),
		},
		RunnerPoolError::ServerlessStreamEndedEarly => RunnerPoolError::ServerlessStreamEndedEarly,
		RunnerPoolError::ServerlessConnectionError { message } => {
			RunnerPoolError::ServerlessConnectionError {
				message: message.clone(),
			}
		}
		RunnerPoolError::ServerlessDestinationBlocked { reason } => {
			RunnerPoolError::ServerlessDestinationBlocked {
				reason: reason.clone(),
			}
		}
		RunnerPoolError::Downgrade => RunnerPoolError::Downgrade,
		RunnerPoolError::InternalError => RunnerPoolError::InternalError,
	}
}

// CloudEvents `data` for a runner pool health transition.
#[derive(Debug, Serialize)]
struct RunnerPoolEventPayload<'a> {
	namespace_id: Id,
	runner_name: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	error: Option<RunnerPoolError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Input {
	pub namespace_id: Id,
	pub runner_name: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct State {
	/// Persistent error state - set on error, cleared after consecutive successes.
	/// Used to track errors during backoff periods when no new requests are made.
	pub active_error: Option<ActiveError>,

	/// Count of consecutive successes since last error.
	/// Error is only cleared after reaching the configured threshold.
	pub consecutive_successes: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActiveError {
	pub timestamp: i64,
	pub error: RunnerPoolError,
}

#[workflow]
pub async fn pegboard_runner_pool_error_tracker(
	ctx: &mut WorkflowCtx,
	input: &Input,
) -> Result<()> {
	tracing::debug!(
		namespace_id = %input.namespace_id,
		runner_name = %input.runner_name,
		"starting error tracker"
	);

	ctx.activity(InitStateInput {}).await?;

	let namespace_id = input.namespace_id;
	let runner_name = input.runner_name.clone();

	// Batch receive signals with debounce. This allows us to (a) not require polling if the pool
	// is idle and has no signals and (b) avoid a hot loop by debouncing signal processing.
	ctx.lupe()
		// Txn sizes can quickly get large in this workflow, need to commit loop more often
		.commit_interval(1)
		.run(move |ctx, _| {
			let runner_name = runner_name.clone();
			Box::pin(async move {
				// Sleep until we receive a signal
				let signals_a = ctx.v(2).listen_n::<Main>(SIGNAL_BATCH_SIZE).await?;

				// Debounce rest of signals if we haven't already reached the batch size
				let remaining_signals = SIGNAL_BATCH_SIZE.saturating_sub(signals_a.len());
				let signals_b = if remaining_signals > 0 {
					ctx.listen_n_with_timeout::<Main>(SIGNAL_DEBOUNCE, remaining_signals)
						.await?
				} else {
					Vec::new()
				};

				let signals_inner = signals_a
					.into_iter()
					.chain(signals_b.into_iter())
					.map(|s| match s {
						Main::ReportSuccess(x) => MainInner::ReportSuccess(x),
						Main::ReportError(x) => MainInner::ReportError(x),
						Main::Shutdown(x) => MainInner::Shutdown(x),
					})
					.collect();

				// Process signals
				let ProcessSignalsOutput {
					shutdown,
					transitions,
				} = ctx.activity(ProcessSignalsInput {
					signals: signals_inner,
				})
				.await?;

				for transition in transitions {
					let (event_type, error) = match transition {
						HealthTransition::Errored(error) => (
							WebhookEventType::RunnerPoolError,
							Some(truncate_error_for_webhook(&error)),
						),
						HealthTransition::Recovered => (WebhookEventType::RunnerPoolHealthy, None),
					};

					let webhook_names = ctx
						.activity(ListSubscribedWebhooksInput {
							namespace_id,
							event_type,
						})
						.await?;

					if webhook_names.is_empty() {
						continue;
					}

					let data = serde_json::to_value(RunnerPoolEventPayload {
						namespace_id,
						runner_name: &runner_name,
						error,
					})?;

					for webhook_name in webhook_names {
						// `graceful_not_found` because a webhook can be deleted between the
						// listing above and this signal.
						ctx.signal(webhook::workflows::webhook::Trigger {
							event: WebhookEvent {
								event_type,
								// CloudEvents `subject`: which runner pool within the namespace
								// this event is about.
								subject: Some(runner_name.clone()),
								data: data.clone(),
							},
						})
						.to_workflow::<webhook::workflows::webhook::Workflow>()
						.tag("namespace_id", namespace_id)
						.tag("name", webhook_name)
						.graceful_not_found()
						.send()
						.await?;
					}
				}

				if shutdown {
					Ok(Loop::Break(()))
				} else {
					Ok(Loop::Continue)
				}
			})
		})
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct InitStateInput {}

#[activity(InitState)]
pub async fn init_state(ctx: &ActivityCtx, _input: &InitStateInput) -> Result<()> {
	let mut state = ctx.state::<Option<State>>()?;
	*state = Some(State::default());
	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct ProcessSignalsInput {
	pub signals: Vec<MainInner>,
}

/// A change in the pool's active error state. Edge-triggered: emitted only when the state
/// actually flips, not on every reported error or success.
#[derive(Debug, Serialize, Deserialize)]
pub enum HealthTransition {
	/// The pool went from clean to having an active error.
	Errored(RunnerPoolError),
	/// The pool's active error cleared after enough consecutive successes.
	Recovered,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessSignalsOutput {
	/// `true` if a shutdown signal was received.
	pub shutdown: bool,
	/// Health transitions observed while processing this batch. Returned rather than acted on
	/// here because this activity's result is replayed from history on workflow replay, so the
	/// workflow body is the only place that can durably send one webhook signal per transition.
	pub transitions: Vec<HealthTransition>,
}

#[activity(ProcessSignals)]
pub async fn process_signals(
	ctx: &ActivityCtx,
	input: &ProcessSignalsInput,
) -> Result<ProcessSignalsOutput> {
	let mut state = ctx.state::<State>()?;
	let now = util::timestamp::now();
	let mut transitions = Vec::new();

	for signal in &input.signals {
		match signal {
			MainInner::ReportError(report) => {
				let was_clean = state.active_error.is_none();
				tracing::debug!(
					workflow_id = %ctx.workflow_id(),
					error = ?report.error,
					was_clean,
					"runner pool error tracker received error"
				);
				if was_clean {
					transitions.push(HealthTransition::Errored(report.error.clone()));
				}
				state.active_error = Some(ActiveError {
					timestamp: now,
					error: report.error.clone(),
				});
				state.consecutive_successes = 0;
			}
			MainInner::ReportSuccess(_) => {
				state.consecutive_successes += 1;

				// Only clear error after threshold reached
				let threshold = ctx
					.config()
					.pegboard()
					.runner_pool_error_consecutive_successes_to_clear();
				if state.consecutive_successes >= threshold {
					if state.active_error.is_some() {
						tracing::debug!(
							workflow_id = %ctx.workflow_id(),
							consecutive_successes = state.consecutive_successes,
							"runner pool error tracker cleared active error"
						);
						transitions.push(HealthTransition::Recovered);
					}
					state.active_error = None;
				}
			}
			MainInner::Shutdown(_) => {
				return Ok(ProcessSignalsOutput {
					shutdown: true,
					transitions,
				});
			}
		}
	}

	Ok(ProcessSignalsOutput {
		shutdown: false,
		transitions,
	})
}

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct ListSubscribedWebhooksInput {
	pub namespace_id: Id,
	pub event_type: WebhookEventType,
}

/// Names of the namespace's webhooks that are subscribed to `event_type`. The webhook workflow
/// re-checks its own subscription when the trigger arrives, so this filter is an optimization
/// that avoids signaling webhooks that would just drop the event.
#[activity(ListSubscribedWebhooks)]
pub async fn list_subscribed_webhooks(
	ctx: &ActivityCtx,
	input: &ListSubscribedWebhooksInput,
) -> Result<Vec<String>> {
	let webhooks = ctx
		.op(webhook::ops::list::Input {
			namespace_id: input.namespace_id,
		})
		.await?;

	Ok(webhooks
		.into_iter()
		.filter(|webhook| webhook.config.subscriptions.contains(&input.event_type))
		.map(|webhook| webhook.name)
		.collect())
}

#[derive(Debug, Clone, Hash)]
#[signal("pegboard_runner_pool_error_tracker_report_error")]
pub struct ReportError {
	pub error: RunnerPoolError,
}

#[derive(Debug, Clone, Hash)]
#[signal("pegboard_runner_pool_error_tracker_report_success")]
pub struct ReportSuccess {}

#[derive(Debug, Clone, Hash)]
#[signal("pegboard_runner_pool_error_tracker_shutdown")]
pub struct Shutdown {}

join_signal!(Main {
	ReportError,
	ReportSuccess,
	Shutdown,
});

// HACK: Cannot implement `Hash` on `Main`
#[derive(Debug, Serialize, Deserialize, Hash)]
pub enum MainInner {
	ReportError(ReportError),
	ReportSuccess(ReportSuccess),
	Shutdown(Shutdown),
}
