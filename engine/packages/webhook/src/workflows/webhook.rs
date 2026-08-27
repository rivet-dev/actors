use std::time::Duration;

use futures_util::FutureExt;
use gas::prelude::*;
use serde::{Deserialize, Serialize};
use universaldb::prelude::FormalKey;
use universaldb::utils::IsolationLevel::*;
use uuid::Uuid;

use crate::{errors, keys, types::WebhookConfig};

/// Topic used to correlate `webhook::ops::upsert` (which waits for the outcome) with the
/// `UpsertComplete`/`Failed` messages this workflow sends.
pub fn topic(namespace_id: Id, name: &str) -> (&'static str, String) {
	("webhook", format!("{namespace_id}:{name}"))
}

#[derive(Debug, Serialize)]
struct CloudEvent<'a> {
	id: String,
	source: &'a str,
	specversion: &'static str,
	#[serde(rename = "type")]
	kind: &'static str,
	time: String,
	data: &'a serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverInput {
	pub event_id: String,
	pub namespace_id: Id,
	pub name: String,
	pub config: WebhookConfig,
	pub payload: String,
}

// The maximum number of delivery attempts for a single triggered event before giving up.
const MAX_DELIVERY_ATTEMPTS: u32 = 5;

// Retries are for transient failures: 429 (rate limited) and 5xx (receiver-side error). Any
// other 4xx means the request itself is wrong and retrying with the same payload won't help.
fn is_retryable_status(status: u16) -> bool {
	match status {
		429 => true,
		500..=599 => true,
		_ => false,
	}
}

// Exponential backoff starting at 5s, doubling each attempt, capped at 5m.
fn delivery_backoff(attempt: u32) -> Duration {
	Duration::from_secs(5u64.saturating_mul(1u64 << attempt.min(6)).min(300))
}

#[activity(Deliver)]
pub async fn deliver(
	_ctx: &ActivityCtx,
	input: &DeliverInput,
) -> Result<std::result::Result<(), errors::Webhook>> {
	let event = CloudEvent {
		id: input.event_id.clone(),
		source: &format!("rivet:webhook:{}:{}", input.namespace_id, input.name),
		specversion: "1.0",
		kind: "dev.rivet.webhook.trigger",
		time: chrono::Utc::now().to_rfc3339(),
		data: &serde_json::from_str(&input.payload)?,
	};
	let mut req = reqwest::Client::new()
		.post(&input.config.url)
		.header("Content-Type", "application/cloudevents+json")
		.json(&event);

	for (k, v) in &input.config.headers {
		req = req.header(k, v);
	}

	match req.send().await {
		Ok(res) if res.status().is_success() => Ok(Ok(())),
		Ok(res) => Ok(Err(errors::Webhook::DeliveryFailed {
			status: res.status().as_u16(),
		})),
		Err(err) => bail!("webhook delivery request failed: {err}"),
	}
}

// One workflow instance per (namespace_id, name, dc) - the dc is implicit since a workflow
// always runs on the datacenter it was dispatched from (see webhook spec).
#[derive(Debug, Deserialize, Serialize)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
	pub config: WebhookConfig,
}

#[workflow]
pub async fn webhook(ctx: &mut WorkflowCtx, input: &Input) -> Result<()> {
	tracing::debug!(
		namespace_id = %input.namespace_id,
		name = %input.name,
		"starting webhook workflow"
	);

	if !upsert(
		ctx,
		input.namespace_id,
		input.name.clone(),
		input.config.clone(),
	)
	.await?
	{
		return Ok(());
	}

	let namespace_id = input.namespace_id;
	let name = input.name.clone();

	// Carries the current config as durable loop state so `Trigger` has something to deliver
	// to; `Update` refreshes it only after `upsert` confirms the new config actually persisted.
	ctx.loope(input.config.clone(), move |ctx, config| {
		let name = name.clone();
		async move {
			match ctx.listen::<Main>().await? {
				Main::Update(sig) => {
					if upsert(ctx, namespace_id, name.clone(), sig.config.clone()).await? {
						*config = sig.config;
					}
				}
				Main::Trigger(sig) => {
					let event_id = Uuid::new_v4().to_string();
					let mut attempt = 0;
					let mut destroyed = false;

					loop {
						let deliver_res = ctx
							.activity(DeliverInput {
								event_id: event_id.clone(),
								namespace_id,
								name: name.clone(),
								config: config.clone(),
								payload: sig.payload.clone(),
							})
							.await?;

						match deliver_res {
							Ok(()) => break,
							Err(errors::Webhook::DeliveryFailed { status })
								if is_retryable_status(status)
									&& attempt + 1 < MAX_DELIVERY_ATTEMPTS =>
							{
								attempt += 1;

								// Race the backoff wait against `Destroy` so a delete mid-retry
								// stops delivery immediately instead of waiting out the full
								// retry sequence before the workflow notices.
								let destroy_sig = ctx
									.listen_with_timeout::<Destroy>(delivery_backoff(attempt))
									.await?;

								if destroy_sig.is_some() {
									destroyed = true;
									break;
								}
							}
							Err(error) => {
								tracing::warn!(
									?error,
									attempt,
									"webhook delivery failed permanently"
								);
								break;
							}
						}
					}

					if destroyed {
						return Ok(Loop::Break(()));
					}
				}
				Main::Destroy(_) => {
					return Ok(Loop::Break(()));
				}
			}

			Ok(Loop::<()>::Continue)
		}
		.boxed()
	})
	.await?;

	Ok(())
}

async fn upsert(
	ctx: &mut WorkflowCtx,
	namespace_id: Id,
	name: String,
	config: WebhookConfig,
) -> Result<bool> {
	let validate_res = ctx
		.activity(ValidateInput {
			config: config.clone(),
		})
		.await?;

	if let Err(error) = validate_res {
		ctx.msg(Failed { error })
			.topic(topic(namespace_id, &name))
			.send()
			.await?;

		// TODO(RVT-3928): return Ok(Err) (is what is written in the equiv namespace file)
		return Ok(false);
	}

	let upsert_res = ctx
		.activity(UpsertConfigInput {
			namespace_id,
			name: name.clone(),
			config,
		})
		.await?;

	if let Err(error) = upsert_res {
		ctx.msg(Failed { error })
			.topic(topic(namespace_id, &name))
			.send()
			.await?;

		// TODO(RVT-3928): return Ok(Err) (is what is written in the equiv namespace file)
		return Ok(false);
	}

	ctx.msg(UpsertComplete {})
		.topic(topic(namespace_id, &name))
		.send()
		.await?;

	Ok(true)
}

#[message("webhook_upsert_complete")]
pub struct UpsertComplete {}

#[message("webhook_failed")]
pub struct Failed {
	pub error: errors::Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateInput {
	pub config: WebhookConfig,
}

#[activity(Validate)]
pub async fn validate(
	_ctx: &ActivityCtx,
	input: &ValidateInput,
) -> Result<std::result::Result<(), errors::Webhook>> {
	if let Err(err) = url::Url::parse(&input.config.url) {
		return Ok(Err(errors::Webhook::Invalid {
			reason: format!("invalid url: {err}"),
		}));
	}

	if input.config.headers.len() > 16 {
		return Ok(Err(errors::Webhook::Invalid {
			reason: "too many headers (max 16)".to_string(),
		}));
	}

	for (name, value) in &input.config.headers {
		if name.len() > 128 {
			return Ok(Err(errors::Webhook::Invalid {
				reason: "invalid header name: too long (max 128)".to_string(),
			}));
		}
		if let Err(err) = name.parse::<reqwest::header::HeaderName>() {
			return Ok(Err(errors::Webhook::Invalid {
				reason: format!("invalid header name: {err}"),
			}));
		}
		if value.len() > 4096 {
			return Ok(Err(errors::Webhook::Invalid {
				reason: "invalid header value: too long (max 4096)".to_string(),
			}));
		}
		if let Err(err) = value.parse::<reqwest::header::HeaderValue>() {
			return Ok(Err(errors::Webhook::Invalid {
				reason: format!("invalid header value: {err}"),
			}));
		}
	}

	Ok(Ok(()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertConfigInput {
	pub namespace_id: Id,
	pub name: String,
	pub config: WebhookConfig,
}

// Writes the webhook config to epoxy (the durable, replicated copy) and mirrors it into local
// UDB (what `list` reads, since epoxy is slow and not meant for frequent/scan-style reads).
// The epoxy write is a check-and-set against the local mirror's last-known value.
#[activity(UpsertConfig)]
pub async fn upsert_config(
	ctx: &ActivityCtx,
	input: &UpsertConfigInput,
) -> Result<std::result::Result<(), errors::Webhook>> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();

	let global_key = keys::GlobalDataKey::new(namespace_id, name.clone());
	let local_key = keys::DataKey::new(namespace_id, name.clone());

	let existing = ctx
		.udb()?
		.txn("webhook_upsert_config_read", |tx| {
			let local_key = keys::DataKey::new(namespace_id, name.clone());
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				tx.read_opt(&local_key, Serializable).await
			}
		})
		.await?;
	let expect = existing
		.map(|config| local_key.serialize(config))
		.transpose()?;

	let propose_res = ctx
		.op(epoxy::ops::propose::Input {
			proposal: epoxy::ops::propose::Proposal {
				commands: vec![epoxy::ops::propose::Command {
					kind: epoxy::ops::propose::CommandKind::CheckAndSetCommand(
						epoxy::ops::propose::CheckAndSetCommand {
							key: namespace::keys::subspace().pack(&global_key),
							expect_one_of: vec![expect],
							new_value: Some(global_key.serialize(input.config.clone())?),
						},
					),
				}],
			},
			purge_cache: true,
			mutable: true,
			target_replicas: None,
		})
		.await?;

	match propose_res {
		epoxy::ops::propose::ProposalResult::Committed => {}
		epoxy::ops::propose::ProposalResult::ConsensusFailed { reason } => match reason {
			epoxy::ops::propose::ConsensusFailedReason::ExpectedValueDoesNotMatch { .. } => {
				return Ok(Err(errors::Webhook::Conflict));
			}
			epoxy::ops::propose::ConsensusFailedReason::PreparePhaseConsensusFailed => {
				bail!("epoxy propose failed: prepare phase consensus failed");
			}
			epoxy::ops::propose::ConsensusFailedReason::AcceptPhaseConsensusFailed => {
				bail!("epoxy propose failed: accept phase consensus failed");
			}
			epoxy::ops::propose::ConsensusFailedReason::StaleBallot => {
				bail!("epoxy propose failed: stale ballot");
			}
		},
	}

	// We still have to write locally for listing.
	// TODO: non-transactional. Epoxy propose and the local UDB write can diverge if we crash or
	// error between them.
	let config = input.config.clone();
	ctx.udb()?
		.txn("webhook_upsert_config", |tx| {
			let name = name.clone();
			let config = config.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				tx.write(&keys::DataKey::new(namespace_id, name), config)?;
				Ok(())
			}
		})
		.await?;

	Ok(Ok(()))
}

#[signal("webhook_trigger")]
pub struct Trigger {
	// TODO: proper CloudEvents-shaped payload; deferred pending the design task in the spec.
	pub payload: String,
}

#[signal("webhook_update")]
pub struct Update {
	pub config: WebhookConfig,
}

#[signal("webhook_destroy")]
pub struct Destroy {}

join_signal!(Main {
	Trigger,
	Update,
	Destroy,
});
