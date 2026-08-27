use futures_util::FutureExt;
use gas::prelude::*;
use serde::{Deserialize, Serialize};
use universaldb::prelude::FormalKey;
use universaldb::utils::IsolationLevel::*;

use crate::{errors, keys, types::WebhookConfig};

/// Topic used to correlate `webhook::ops::upsert` (which waits for the outcome) with the
/// `UpsertComplete`/`Failed` messages this workflow sends.
pub fn topic(namespace_id: Id, name: &str) -> (&'static str, String) {
	("webhook", format!("{namespace_id}:{name}"))
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

	ctx.repeat(move |ctx| {
		let name = name.clone();
		async move {
			match ctx.listen::<Main>().await? {
				Main::Update(sig) => {
					upsert(ctx, namespace_id, name, sig.config).await?;
				}
				Main::Trigger(sig) => {
					// TODO: actually deliver the webhook over HTTP, with retries (see webhook
					// spec's note to research common webhook retry behavior). Not built yet.
					tracing::debug!(
						payload = %sig.payload,
						"received webhook trigger, delivery not yet implemented"
					);
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
