use futures_util::{StreamExt, TryStreamExt};
use gas::prelude::*;
use universaldb::options::StreamingMode;
use universaldb::utils::IsolationLevel::*;

use crate::{keys, types::WebhookConfig};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
}

#[derive(Debug)]
pub struct Webhook {
	pub name: String,
	pub config: WebhookConfig,
}

// Reads from the local UDB mirror written by `upsert`, not epoxy directly (see
// `webhook_config_upsert` for why).
#[operation]
pub async fn webhook_config_list(ctx: &OperationCtx, input: &Input) -> Result<Vec<Webhook>> {
	let webhooks = ctx
		.udb()?
		.txn("webhook_config_list", |tx| async move {
			let tx = tx.with_subspace(namespace::keys::subspace());

			let (start, end) = namespace::keys::subspace()
				.subspace(&keys::DataKey::subspace(input.namespace_id))
				.range();

			tx.get_ranges_keyvalues(
				universaldb::RangeOption {
					mode: StreamingMode::WantAll,
					..(start, end).into()
				},
				Serializable,
			)
			.map(|res| {
				let tx = tx.clone();
				async move {
					let entry = res?;
					let (key, config) = tx.read_entry::<keys::DataKey>(&entry)?;
					Ok(Webhook {
						name: key.name,
						config,
					})
				}
			})
			.buffer_unordered(16)
			.try_collect::<Vec<_>>()
			.await
		})
		.await?;

	Ok(webhooks)
}
