use epoxy::ops::propose::{Command, CommandKind, Proposal, SetCommand};
use gas::prelude::*;

use crate::keys;

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
}

// Clears the local UDB mirror and proposes the epoxy clear (setting the value to `None` is how
// a key is deleted through epoxy). Does not yet signal the webhook workflow to exit (see
// webhook spec) - that comes in a follow-up change.
#[operation]
pub async fn webhook_config_delete(ctx: &OperationCtx, input: &Input) -> Result<()> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();
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

	let global_key = keys::GlobalDataKey::new(input.namespace_id, input.name.clone());
	ctx.op(epoxy::ops::propose::Input {
		proposal: Proposal {
			commands: vec![Command {
				kind: CommandKind::SetCommand(SetCommand {
					key: namespace::keys::subspace().pack(&global_key),
					value: None,
				}),
			}],
		},
		purge_cache: true,
		mutable: true,
		target_replicas: None,
	})
	.await?;

	Ok(())
}
