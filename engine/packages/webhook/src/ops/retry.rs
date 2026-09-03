use gas::prelude::*;
use universaldb::utils::IsolationLevel::*;

use crate::{errors, keys, types::DeliveryStatus, workflows};

#[derive(Debug)]
pub struct Input {
	pub namespace_id: Id,
	pub name: String,
	pub delivery_id: String,
}

// Validates the delivery exists and is in a failed state, then signals the webhook workflow to
// retry it. The workflow re-checks the same state on its side before actually redelivering, since
// this read and the signal are not part of the same transaction.
#[operation]
pub async fn webhook_delivery_retry(ctx: &OperationCtx, input: &Input) -> Result<()> {
	let namespace_id = input.namespace_id;
	let name = input.name.clone();
	let delivery_id = input.delivery_id.clone();

	let record = ctx
		.udb()?
		.txn("webhook_delivery_retry_read", {
			let name = name.clone();
			let delivery_id = delivery_id.clone();
			move |tx| {
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
			}
		})
		.await?;

	let record = record.ok_or_else(|| errors::Webhook::DeliveryNotFound.build())?;

	if !matches!(record.status, DeliveryStatus::Failed) {
		return Err(errors::Webhook::DeliveryNotRetryable.build());
	}

	let signal_res = ctx
		.signal(workflows::webhook::Retry { delivery_id })
		.to_workflow::<workflows::webhook::Workflow>()
		.tag("namespace_id", namespace_id)
		.tag("name", name)
		.graceful_not_found()
		.send()
		.await?;

	if signal_res.is_none() {
		return Err(errors::Webhook::NotFound.build());
	}

	Ok(())
}
