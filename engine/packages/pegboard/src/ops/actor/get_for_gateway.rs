use gas::prelude::*;
use universaldb::utils::IsolationLevel::*;

use crate::keys;

#[derive(Debug)]
pub struct Input {
	pub actor_id: Id,
}

#[derive(Debug)]
pub struct Output {
	pub namespace_id: Id,
	pub workflow_id: Id,
	pub key: Option<String>,
	// NOTE: None if older actor has not received the new key
	pub runner_name_selector: Option<String>,
	pub sleeping: bool,
	pub destroyed: bool,
	pub connectable: bool,
	pub runner_id: Option<Id>,
	pub envoy_key: Option<String>,
	pub envoy_protocol_version: Option<u16>,
	pub generation: Option<u32>,
	pub version: u32,
}

#[operation]
#[timeout = 5]
pub async fn pegboard_actor_get_for_gateway(
	ctx: &OperationCtx,
	input: &Input,
) -> Result<Option<Output>> {
	ctx.udb()?
		.txn("pegboard_actor_get_for_gateway", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			let namespace_id_key = keys::actor::NamespaceIdKey::new(input.actor_id);
			let workflow_id_key = keys::actor::WorkflowIdKey::new(input.actor_id);
			let key_key = keys::actor::KeyKey::new(input.actor_id);
			let runner_name_selector_key = keys::actor::RunnerNameSelectorKey::new(input.actor_id);
			let sleep_ts_key = keys::actor::SleepTsKey::new(input.actor_id);
			let destroy_ts_key = keys::actor::DestroyTsKey::new(input.actor_id);
			let connectable_key = keys::actor::ConnectableKey::new(input.actor_id);
			let runner_id_key = keys::actor::RunnerIdKey::new(input.actor_id);
			let version_key = keys::actor::VersionKey::new(input.actor_id);
			let envoy_key_key = keys::actor::EnvoyKeyKey::new(input.actor_id);
			let generation_key = keys::actor::GenerationKey::new(input.actor_id);

			let (
				namespace_id_entry,
				workflow_id_entry,
				key,
				runner_name_selector,
				sleeping,
				destroyed,
				connectable,
				runner_id,
				version_entry,
				envoy_key,
				generation,
			) = tokio::try_join!(
				tx.read_opt(&namespace_id_key, Serializable),
				tx.read_opt(&workflow_id_key, Serializable),
				tx.read_opt(&key_key, Serializable),
				tx.read_opt(&runner_name_selector_key, Serializable),
				tx.exists(&sleep_ts_key, Serializable),
				tx.exists(&destroy_ts_key, Serializable),
				tx.exists(&connectable_key, Serializable),
				tx.read_opt(&runner_id_key, Serializable),
				tx.read_opt(&version_key, Serializable),
				tx.read_opt(&envoy_key_key, Serializable),
				tx.read_opt(&generation_key, Serializable),
			)?;

			let (Some(namespace_id), Some(workflow_id)) = (namespace_id_entry, workflow_id_entry)
			else {
				return Ok(None);
			};

			tx.tag(&format!("actor_get_for_gateway:{namespace_id}"))?;

			let envoy_protocol_version = if let Some(envoy_key) = &envoy_key {
				tx.read_opt(
					&keys::envoy::ProtocolVersionKey::new(namespace_id, envoy_key.clone()),
					Serializable,
				)
				.await?
			} else {
				None
			};

			Ok(Some(Output {
				namespace_id,
				workflow_id,
				key,
				runner_name_selector,
				sleeping,
				destroyed,
				connectable,
				runner_id,
				envoy_key,
				envoy_protocol_version,
				generation,
				version: version_entry.unwrap_or(1),
			}))
		})
		.custom_instrument(tracing::info_span!("actor_get_for_gateway_tx"))
		.await
}
