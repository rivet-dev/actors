use epoxy::ops::propose::{Command, CommandKind, Proposal, SetCommand};
use gas::prelude::*;
use universaldb::prelude::*;

use crate::{keys, types::WebhookConfig};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
	pub config: WebhookConfig,
}

// Writes the webhook config to epoxy (the durable, replicated copy) and mirrors it into local
// UDB (what `list` reads, since epoxy is slow and not meant for frequent/scan-style reads).
// Does not yet spawn or signal the per-(namespace, webhook name, dc) webhook workflow (see
// webhook spec) - that comes in a follow-up change.
#[operation]
pub async fn webhook_config_upsert(ctx: &OperationCtx, input: &Input) -> Result<()> {
	if let Err(err) = url::Url::parse(&input.config.url) {
		bail!("invalid webhook url: {err}");
	}

	let global_key = keys::GlobalDataKey::new(input.namespace_id, input.name.clone());

	ctx.op(epoxy::ops::propose::Input {
		proposal: Proposal {
			commands: vec![Command {
				kind: CommandKind::SetCommand(SetCommand {
					key: namespace::keys::subspace().pack(&global_key),
					value: Some(global_key.serialize(input.config.clone())?),
				}),
			}],
		},
		purge_cache: true,
		mutable: true,
		target_replicas: None,
	})
	.await?;

	// We still have to write locally for listing.
	// TODO: non-transactional. Epoxy propose and the local UDB write can diverge if we crash or
	// error between them.
	let namespace_id = input.namespace_id;
	let name = input.name.clone();
	let config = input.config.clone();
	ctx.udb()?
		.txn("webhook_config_upsert", |tx| {
			let name = name.clone();
			let config = config.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				tx.write(&keys::DataKey::new(namespace_id, name), config)?;
				Ok(())
			}
		})
		.await?;

	Ok(())
}
