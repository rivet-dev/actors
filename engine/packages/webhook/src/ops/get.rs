use gas::prelude::*;
use universaldb::utils::IsolationLevel::*;

use crate::{keys, types::WebhookConfig};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
}

// Point read of a single webhook config from the local UDB mirror written by `upsert`, for
// callers that only need to know whether one webhook exists. Use `list` when you need all of a
// namespace's webhooks.
#[operation]
pub async fn webhook_config_get(
	ctx: &OperationCtx,
	input: &Input,
) -> Result<Option<WebhookConfig>> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();

	ctx.udb()?
		.txn("webhook_config_get", move |tx| {
			let name = name.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());
				tx.read_opt(&keys::DataKey::new(namespace_id, name), Serializable)
					.await
			}
		})
		.await
}
