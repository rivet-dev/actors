use epoxy::ops::propose::{
	CheckAndSetCommand, Command, CommandKind, ConsensusFailedReason, Proposal, ProposalResult,
};
use gas::prelude::*;
use universaldb::prelude::FormalKey;
use universaldb::utils::IsolationLevel::*;

use crate::{errors, keys, workflows};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
}

// Proposes the epoxy clear (setting the value to `None` is how a key is deleted through epoxy),
// clears the local UDB mirror, then signals the webhook workflow to exit. `graceful_not_found`
// tolerates the workflow already being gone (e.g. a repeat delete).
//
// The epoxy clear is a check-and-set against the local mirror's last-known value, same as
// `upsert`'s write, so a concurrent update from another datacenter is detected instead of
// silently deleted out from under it.
#[operation]
pub async fn webhook_config_delete(ctx: &OperationCtx, input: &Input) -> Result<()> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();

	let global_key = keys::GlobalDataKey::new(namespace_id, name.clone());
	let local_key = keys::DataKey::new(namespace_id, name.clone());

	let existing = ctx
		.udb()?
		.txn("webhook_config_delete_read", |tx| {
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
			proposal: Proposal {
				commands: vec![Command {
					kind: CommandKind::CheckAndSetCommand(CheckAndSetCommand {
						key: namespace::keys::subspace().pack(&global_key),
						expect_one_of: vec![expect],
						new_value: None,
					}),
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
			ConsensusFailedReason::ExpectedValueDoesNotMatch { .. } => {
				return Err(errors::Webhook::Conflict.build());
			}
			ConsensusFailedReason::PreparePhaseConsensusFailed => {
				bail!("epoxy propose failed: prepare phase consensus failed");
			}
			ConsensusFailedReason::AcceptPhaseConsensusFailed => {
				bail!("epoxy propose failed: accept phase consensus failed");
			}
			ConsensusFailedReason::StaleBallot => {
				bail!("epoxy propose failed: stale ballot");
			}
		},
	}

	ctx.udb()?
		.txn("webhook_config_delete", |tx| {
			let name = name.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				tx.delete(&keys::DataKey::new(namespace_id, name));
				Ok(())
			}
		})
		.await?;

	ctx.signal(workflows::webhook::Destroy {})
		.to_workflow::<workflows::webhook::Workflow>()
		.tag("namespace_id", input.namespace_id)
		.tag("name", input.name.clone())
		.graceful_not_found()
		.send()
		.await?;

	Ok(())
}
