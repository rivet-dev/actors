use std::time::Duration;

use epoxy::ops::propose::ProposalResult;
use futures_util::FutureExt;
use gas::prelude::*;
use serde::{Deserialize, Serialize};
use universaldb::prelude::FormalKey;
use universaldb::utils::IsolationLevel::*;
use uuid::Uuid;

use crate::{
	errors, keys,
	types::{DeliveryRecord, DeliveryStatus, WebhookConfig, WebhookEventType},
};

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
	pub delivery_id: String,
	pub namespace_id: Id,
	pub name: String,
	pub config: WebhookConfig,
	pub payload: String,
	pub event_type: WebhookEventType,
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
	ctx: &ActivityCtx,
	input: &DeliverInput,
) -> Result<std::result::Result<(), errors::Webhook>> {
	let parsed_url =
		url::Url::parse(&input.config.url).context("stored webhook url is not parseable")?;

	let policy = rivet_pools::reqwest::outbound_policy(ctx.config()).await?;
	if let Err(reason) = policy.check_url(&parsed_url) {
		return Ok(Err(errors::Webhook::DestinationBlocked {
			reason: reason.to_string(),
		}));
	}

	let event = CloudEvent {
		id: input.delivery_id.clone(),
		source: &format!("rivet:webhook:{}:{}", input.namespace_id, input.name),
		specversion: "1.0",
		kind: input.event_type.as_cloudevents_type(),
		time: chrono::Utc::now().to_rfc3339(),
		data: &serde_json::from_str(&input.payload)?,
	};

	let client = rivet_pools::reqwest::guarded_client(ctx.config()).await?;
	let mut req = client
		.post(parsed_url)
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
		Err(err) => {
			let err = anyhow::Error::from(err);

			// A hostname that only resolves to a disallowed address is rejected by the
			// resolver at connect time, which the pre-flight check above cannot see.
			if let Some(reason) = rivet_outbound_guard::block_reason(&err) {
				return Ok(Err(errors::Webhook::DestinationBlocked {
					reason: reason.to_string(),
				}));
			}

			bail!("webhook delivery request failed: {err}");
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordDeliveryInput {
	pub namespace_id: Id,
	pub name: String,
	pub delivery_id: String,
	pub payload: String,
	pub status: DeliveryStatus,
	pub attempt_count: u32,
	pub last_error: Option<String>,
	pub event_type: WebhookEventType,
}

// Writes the current state of a delivery to the local UDB mirror so it can be looked up later by
// `Retry` or listed for event history. Local only, not proposed through epoxy: a delivery only
// ever matters to the datacenter that ran it (see `keys::DeliveryKey`).
//
// Preserves `created_at` from any existing record for this delivery id instead of taking it from
// the caller, so a `Retry` (which re-enters this same activity) doesn't reset when the delivery
// was first triggered. Stamps a fresh `created_at` only the first time a delivery id is recorded.
#[activity(RecordDelivery)]
pub async fn record_delivery(ctx: &ActivityCtx, input: &RecordDeliveryInput) -> Result<()> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();
	let delivery_id = input.delivery_id.clone();
	let payload = input.payload.clone();
	let status = input.status;
	let attempt_count = input.attempt_count;
	let last_error = input.last_error.clone();
	let event_type = input.event_type;
	let now = ctx.ts();

	ctx.udb()?
		.txn("webhook_record_delivery", move |tx| {
			let name = name.clone();
			let delivery_id = delivery_id.clone();
			let payload = payload.clone();
			let last_error = last_error.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				let key = keys::DeliveryKey::new(namespace_id, name, delivery_id);

				let created_at = match tx.read_opt(&key, Serializable).await? {
					Some(existing) => existing.created_at,
					None => now,
				};

				tx.write(
					&key,
					DeliveryRecord {
						payload,
						status,
						attempt_count,
						last_error,
						created_at,
						event_type,
					},
				)?;
				Ok(())
			}
		})
		.await?;
	Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDeliveryInput {
	pub namespace_id: Id,
	pub name: String,
	pub delivery_id: String,
}

#[activity(GetDelivery)]
pub async fn get_delivery(
	ctx: &ActivityCtx,
	input: &GetDeliveryInput,
) -> Result<Option<DeliveryRecord>> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();
	let delivery_id = input.delivery_id.clone();

	ctx.udb()?
		.txn("webhook_get_delivery", move |tx| {
			let name = name.clone();
			let delivery_id = delivery_id.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				tx.read_opt(
					&keys::DeliveryKey::new(namespace_id, name, delivery_id),
					Serializable,
				)
				.await
			}
		})
		.await
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
					// The workflow owns the config, so it is the authority on what this webhook is
					// subscribed to. Producers filter by subscription before signaling, but the
					// config can change between that read and this signal arriving.
					if !config.subscriptions.contains(&sig.event_type) {
						tracing::debug!(
							event_type = sig.event_type.as_str(),
							"dropping trigger for unsubscribed event type"
						);
						return Ok(Loop::Continue);
					}

					let delivery_id = Uuid::new_v4().to_string();

					let outcome = deliver_with_retries(
						ctx,
						namespace_id,
						name.clone(),
						delivery_id,
						config.clone(),
						sig.payload,
						sig.event_type,
					)
					.await?;

					if let DeliveryOutcome::Destroyed = outcome {
						return Ok(Loop::Break(()));
					}
				}
				Main::Retry(sig) => {
					let record = ctx
						.activity(GetDeliveryInput {
							namespace_id,
							name: name.clone(),
							delivery_id: sig.delivery_id.clone(),
						})
						.await?;

					let Some(record) = record else {
						tracing::warn!(
							delivery_id = %sig.delivery_id,
							"retry requested for unknown delivery"
						);
						return Ok(Loop::Continue);
					};

					if !matches!(record.status, DeliveryStatus::Failed) {
						tracing::warn!(
							delivery_id = %sig.delivery_id,
							status = ?record.status,
							"retry requested for delivery not in a failed state"
						);
						return Ok(Loop::Continue);
					}

					let outcome = deliver_with_retries(
						ctx,
						namespace_id,
						name.clone(),
						sig.delivery_id,
						config.clone(),
						record.payload,
						record.event_type,
					)
					.await?;

					if let DeliveryOutcome::Destroyed = outcome {
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

enum DeliveryOutcome {
	// The delivery reached a terminal state, either delivered or permanently failed after
	// exhausting retries. Either way there is nothing left to wait on.
	Done,
	// A `Destroy` signal interrupted an in-progress backoff wait.
	Destroyed,
}

// Runs (or re-runs, for a manual `Retry`) the full attempt loop for one delivery: records it as
// `Pending`, attempts delivery with exponential backoff up to `MAX_DELIVERY_ATTEMPTS`, and records
// the terminal `Succeeded`/`Failed` outcome. Shared by `Trigger` (a brand new delivery) and
// `Retry` (re-attempting a stored, already-failed delivery), since both just need to run this
// same loop against a `delivery_id` and `payload`.
async fn deliver_with_retries(
	ctx: &mut WorkflowCtx,
	namespace_id: Id,
	name: String,
	delivery_id: String,
	config: WebhookConfig,
	payload: String,
	event_type: WebhookEventType,
) -> Result<DeliveryOutcome> {
	ctx.activity(RecordDeliveryInput {
		namespace_id,
		name: name.clone(),
		delivery_id: delivery_id.clone(),
		payload: payload.clone(),
		status: DeliveryStatus::Pending,
		attempt_count: 0,
		last_error: None,
		event_type,
	})
	.await?;

	let mut attempt = 0;

	loop {
		let deliver_res = ctx
			.activity(DeliverInput {
				delivery_id: delivery_id.clone(),
				namespace_id,
				name: name.clone(),
				config: config.clone(),
				payload: payload.clone(),
				event_type,
			})
			.await?;

		match deliver_res {
			Ok(()) => {
				ctx.activity(RecordDeliveryInput {
					namespace_id,
					name: name.clone(),
					delivery_id: delivery_id.clone(),
					payload,
					status: DeliveryStatus::Succeeded,
					attempt_count: attempt + 1,
					last_error: None,
					event_type,
				})
				.await?;

				return Ok(DeliveryOutcome::Done);
			}
			Err(errors::Webhook::DeliveryFailed { status })
				if is_retryable_status(status) && attempt + 1 < MAX_DELIVERY_ATTEMPTS =>
			{
				attempt += 1;

				// Race the backoff wait against `Destroy` so a delete mid-retry stops delivery
				// immediately instead of waiting out the full retry sequence before the workflow
				// notices.
				let destroy_sig = ctx
					.listen_with_timeout::<Destroy>(delivery_backoff(attempt))
					.await?;

				if destroy_sig.is_some() {
					return Ok(DeliveryOutcome::Destroyed);
				}
			}
			Err(error) => {
				tracing::warn!(?error, attempt, "webhook delivery failed permanently");

				ctx.activity(RecordDeliveryInput {
					namespace_id,
					name: name.clone(),
					delivery_id: delivery_id.clone(),
					payload,
					status: DeliveryStatus::Failed,
					attempt_count: attempt + 1,
					last_error: Some(error.build().to_string()),
					event_type,
				})
				.await?;

				return Ok(DeliveryOutcome::Done);
			}
		}
	}
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
	ctx: &ActivityCtx,
	input: &ValidateInput,
) -> Result<std::result::Result<(), errors::Webhook>> {
	let parsed_url = match url::Url::parse(&input.config.url) {
		Ok(parsed_url) => parsed_url,
		Err(err) => {
			return Ok(Err(errors::Webhook::Invalid {
				reason: format!("invalid url: {err}"),
			}));
		}
	};

	// Reject destinations the engine is not allowed to reach before the config is stored.
	// Delivery re-checks this at request time, which also catches configs written before this
	// gate existed and hosts whose DNS answer changes afterwards.
	let policy = rivet_pools::reqwest::outbound_policy(ctx.config()).await?;
	if let Err(reason) = policy.check_url(&parsed_url) {
		return Ok(Err(errors::Webhook::Invalid {
			reason: format!("invalid url: {reason}"),
		}));
	}

	// Enforce the webhook-safe event type allowlist. High-throughput types are still ingested for
	// analytics; they just cannot be a webhook trigger (see the webhook spec).
	for event_type in &input.config.subscriptions {
		if !event_type.is_webhook_safe() {
			return Ok(Err(errors::Webhook::EventTypeNotAllowed {
				event_type: event_type.as_str().to_string(),
			}));
		}
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
		ProposalResult::Committed => {}
		ProposalResult::ConsensusFailed { reason } => match reason {
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
	pub event_type: WebhookEventType,
	// TODO: proper CloudEvents-shaped payload; deferred pending the design task in the spec.
	pub payload: String,
}

#[signal("webhook_update")]
pub struct Update {
	pub config: WebhookConfig,
}

#[signal("webhook_destroy")]
pub struct Destroy {}

#[signal("webhook_retry")]
pub struct Retry {
	pub delivery_id: String,
}

join_signal!(Main {
	Trigger,
	Update,
	Destroy,
	Retry,
});
