use futures_util::{StreamExt, TryStreamExt};
use gas::prelude::*;
use universaldb::options::StreamingMode;
use universaldb::utils::IsolationLevel::*;

use crate::{keys, types::DeliveryRecord};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
}

#[derive(Debug)]
pub struct Delivery {
	pub delivery_id: String,
	pub record: DeliveryRecord,
}

// Reads every delivery recorded for one webhook from the local UDB mirror written by
// `record_delivery` (see `workflows::webhook`). Unordered; callers sort by `created_at` for
// chronological event history. A full scan of the webhook's delivery subspace, which is fine
// given deliveries are meant to stay low-throughput (see the event-type allowlist in the webhook
// spec) rather than a place to paginate over via range bounds.
#[operation]
pub async fn webhook_delivery_list(ctx: &OperationCtx, input: &Input) -> Result<Vec<Delivery>> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();

	let deliveries = ctx
		.udb()?
		.txn("webhook_delivery_list", move |tx| {
			let name = name.clone();
			async move {
				let tx = tx.with_subspace(namespace::keys::subspace());

				let (start, end) = namespace::keys::subspace()
					.subspace(&keys::DeliveryKey::subspace(namespace_id, name))
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
						let (key, record) = tx.read_entry::<keys::DeliveryKey>(&entry)?;
						Ok(Delivery {
							delivery_id: key.delivery_id,
							record,
						})
					}
				})
				.buffer_unordered(16)
				.try_collect::<Vec<_>>()
				.await
			}
		})
		.await?;

	Ok(deliveries)
}
