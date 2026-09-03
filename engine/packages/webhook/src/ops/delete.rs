use epoxy::ops::propose::{
	CheckAndSetCommand, Command, CommandKind, ConsensusFailedReason, Proposal, ProposalResult,
};
use gas::prelude::*;

use crate::{errors, keys, workflows};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
}

// Proposes the epoxy clear (setting the value to `None` is how a key is deleted through epoxy),
// clears the local UDB mirror, then signals the webhook workflow to exit. `graceful_not_found`
// tolerates the workflow already being gone (e.g. a repeat delete), which also makes a repeat
// delete a no-op rather than an error.
//
// `expect_one_of` is always `vec![None]` because epoxy v2 does not implement value-conditional
// compare-and-swap; it accepts only that value. Concurrency is still detected, just at a
// different granularity: consensus decides one value per round, and a proposal that loses the
// round comes back as `ExpectedValueDoesNotMatch`, surfaced here as `Conflict`.
#[operation]
pub async fn webhook_config_delete(ctx: &OperationCtx, input: &Input) -> Result<()> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();

	let global_key = keys::GlobalDataKey::new(namespace_id, name.clone());

	let propose_res = ctx
		.op(epoxy::ops::propose::Input {
			proposal: Proposal {
				commands: vec![Command {
					kind: CommandKind::CheckAndSetCommand(CheckAndSetCommand {
						key: namespace::keys::subspace().pack(&global_key),
						expect_one_of: vec![None],
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
				// Another proposer's value won this round, so the delete did not take effect.
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
