use gas::prelude::*;
use rivet_data::converted::ActorByKeyKeyData;
use rivet_runner_protocol::PROTOCOL_MK1_VERSION;
use universaldb::options::MutationType;
use universaldb::utils::IsolationLevel::*;

use super::{DestroyComplete, DestroyStarted, State};

use crate::keys;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Input {
	pub namespace_id: Id,
	pub actor_id: Id,
	pub name: String,
	pub key: Option<String>,
	pub generation: u32,
}

#[workflow]
pub(crate) async fn pegboard_actor_destroy(ctx: &mut WorkflowCtx, input: &Input) -> Result<()> {
	ctx.msg(DestroyStarted {})
		.topic(("actor_id", input.actor_id))
		.send()
		.await?;

	let res = ctx
		.activity(UpdateStateAndDbInput {
			actor_id: input.actor_id,
		})
		.await?;

	// If a slot was allocated at the time of actor destruction then bump the runner pool so it can scale down
	// if needed
	if res.allocated_serverless_slot {
		ctx.removed::<Message<super::BumpServerlessAutoscalerStub>>()
			.await?;

		let bump_res = ctx
			.v(2)
			.signal(crate::workflows::runner_pool::Bump::default())
			.to_workflow::<crate::workflows::runner_pool::Workflow>()
			.tag("namespace_id", input.namespace_id)
			.tag("runner_name", res.runner_name_selector.clone())
			.send()
			.await;

		if let Some(WorkflowError::WorkflowNotFound) = bump_res
			.as_ref()
			.err()
			.and_then(|x| x.chain().find_map(|x| x.downcast_ref::<WorkflowError>()))
		{
			tracing::warn!(
				namespace_id=%input.namespace_id,
				runner_name=%res.runner_name_selector,
				"serverless pool workflow not found, runner config likely deleted"
			);
		} else {
			bump_res?;
		}
	}

	// Clear KV
	ctx.activity(ClearKvInput {
		actor_id: input.actor_id,
	})
	.await?;

	ctx.msg(DestroyComplete {})
		.topic(("actor_id", input.actor_id))
		.send()
		.await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct UpdateStateAndDbInput {
	actor_id: Id,
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct UpdateStateAndDbOutput {
	allocated_serverless_slot: bool,
	runner_name_selector: String,
}

#[activity(UpdateStateAndDb)]
async fn update_state_and_db(
	ctx: &ActivityCtx,
	input: &UpdateStateAndDbInput,
) -> Result<UpdateStateAndDbOutput> {
	let mut state = ctx.state::<State>()?;
	let destroy_ts = util::timestamp::now();

	let runner_id = state.runner_id;
	let namespace_id = state.namespace_id;
	let runner_name_selector = &state.runner_name_selector;
	let allocated_serverless_slot = state.allocated_serverless_slot;
	let name = &state.name;
	let create_ts = state.create_ts;
	let key = &state.key;
	ctx.udb()?
		.txn("pegboard_actor_destroy", |tx| {
			async move {
				let tx = tx.with_subspace(keys::subspace());

				tx.write(&keys::actor::DestroyTsKey::new(input.actor_id), destroy_ts)?;

				clear_slot(
					input.actor_id,
					namespace_id,
					runner_name_selector,
					runner_id,
					allocated_serverless_slot,
					&tx,
				)
				.await?;

				// Update namespace indexes
				tx.delete(&keys::ns::ActiveActorKey::new(
					namespace_id,
					name.clone(),
					create_ts,
					input.actor_id,
				));

				if let Some(key) = &key {
					tx.write(
						&keys::ns::ActorByKeyKey::new(
							namespace_id,
							name.clone(),
							key.clone(),
							create_ts,
							input.actor_id,
						),
						ActorByKeyKeyData {
							workflow_id: ctx.workflow_id(),
							is_destroyed: true,
						},
					)?;
				}

				// Update metrics
				namespace::keys::metric::inc(
					&tx.with_subspace(namespace::keys::subspace()),
					namespace_id,
					namespace::keys::metric::Metric::TotalActors(name.clone()),
					-1,
				);

				Ok(())
			}
		})
		.custom_instrument(tracing::info_span!("actor_destroy_tx"))
		.await?;

	state.destroy_ts = Some(destroy_ts);
	state.runner_id = None;

	let old_allocated_serverless_slot = state.allocated_serverless_slot;
	state.allocated_serverless_slot = false;

	Ok(UpdateStateAndDbOutput {
		allocated_serverless_slot: old_allocated_serverless_slot,
		runner_name_selector: state.runner_name_selector.clone(),
	})
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct ClearKvInput {
	actor_id: Id,
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct ClearKvOutput {
	// Simply an estimate, not accurate under 3MiB
	final_size: i64,
}

#[activity(ClearKv)]
async fn clear_kv(ctx: &ActivityCtx, input: &ClearKvInput) -> Result<ClearKvOutput> {
	let state = ctx.state::<State>()?;
	let namespace_id = state.namespace_id;
	let udb = ctx.udb()?;
	let final_size = udb
		.txn("pegboard_actor_clear_kv", |tx| async move {
			let subspace = keys::actor_kv::subspace(input.actor_id);

			let (start, end) = subspace.range();
			let final_size = tx.get_estimated_range_size_bytes(&start, &end).await?;

			// Matches `delete_all` from actor kv
			tx.clear_subspace_range(&subspace);
			crate::actor_sqlite::clear_v2_storage_for_destroy(&tx, input.actor_id);

			Ok(final_size)
		})
		.custom_instrument(tracing::info_span!("actor_clear_kv_tx"))
		.await?;

	// Keep outside txn - branch delete has its own txn.
	crate::actor_sqlite::clear_branch_storage_for_destroy(&udb, namespace_id, input.actor_id)
		.await?;

	Ok(ClearKvOutput { final_size })
}

pub(crate) async fn clear_slot(
	actor_id: Id,
	namespace_id: Id,
	runner_name_selector: &str,
	runner_id: Option<Id>,
	allocated_serverless_slot: bool,
	tx: &universaldb::Transaction,
) -> Result<()> {
	let tx = tx.with_subspace(keys::subspace());

	// Only clear slot if we have a runner id
	if let Some(runner_id) = runner_id {
		tx.delete(&keys::actor::RunnerIdKey::new(actor_id));

		// This is cleared when the state changes as well as when the actor is destroyed to ensure
		// consistency during rescheduling and forced deletion.
		tx.delete(&keys::runner::ActorKey::new(runner_id, actor_id));

		let runner_workflow_id_key = keys::runner::WorkflowIdKey::new(runner_id);
		let runner_version_key = keys::runner::VersionKey::new(runner_id);
		let runner_remaining_slots_key = keys::runner::RemainingSlotsKey::new(runner_id);
		let runner_total_slots_key = keys::runner::TotalSlotsKey::new(runner_id);
		let runner_last_ping_ts_key = keys::runner::LastPingTsKey::new(runner_id);
		let runner_protocol_version_key = keys::runner::ProtocolVersionKey::new(runner_id);

		let (
			runner_workflow_id,
			runner_version,
			runner_remaining_slots,
			runner_total_slots,
			runner_last_ping_ts,
			runner_protocol_version,
		) = tokio::try_join!(
			tx.read(&runner_workflow_id_key, Serializable),
			tx.read(&runner_version_key, Serializable),
			tx.read(&runner_remaining_slots_key, Serializable),
			tx.read(&runner_total_slots_key, Serializable),
			tx.read(&runner_last_ping_ts_key, Serializable),
			tx.read_opt(&runner_protocol_version_key, Serializable),
		)?;

		let old_runner_remaining_millislots = (runner_remaining_slots * 1000) / runner_total_slots;
		let new_runner_remaining_slots = runner_remaining_slots + 1;

		// Write new remaining slots
		tx.write(&runner_remaining_slots_key, new_runner_remaining_slots)?;

		let old_runner_alloc_key = keys::ns::RunnerAllocIdxKey::new(
			namespace_id,
			runner_name_selector.to_string(),
			runner_version,
			old_runner_remaining_millislots,
			runner_last_ping_ts,
			runner_id,
		);

		// Only update allocation idx if it existed before
		if tx.exists(&old_runner_alloc_key, Serializable).await? {
			// Clear old key
			tx.delete(&old_runner_alloc_key);

			let new_remaining_millislots = (new_runner_remaining_slots * 1000) / runner_total_slots;
			let new_runner_alloc_key = keys::ns::RunnerAllocIdxKey::new(
				namespace_id,
				runner_name_selector.to_string(),
				runner_version,
				new_remaining_millislots,
				runner_last_ping_ts,
				runner_id,
			);

			tx.write(
				&new_runner_alloc_key,
				rivet_data::converted::RunnerAllocIdxKeyData {
					workflow_id: runner_workflow_id,
					remaining_slots: new_runner_remaining_slots,
					total_slots: runner_total_slots,
					// We default here because its not important for mk1 protocol runners
					protocol_version: runner_protocol_version.unwrap_or(PROTOCOL_MK1_VERSION),
				},
			)?;
		}
	}

	if allocated_serverless_slot {
		// Clear the serverless slot even if we do not have a runner id. This happens when the
		// actor is destroyed while pending allocation
		tx.atomic_op(
			&rivet_types::keys::pegboard::ns::ServerlessDesiredSlotsKey::new(
				namespace_id,
				runner_name_selector.to_string(),
			),
			&(-1i64).to_le_bytes(),
			MutationType::Add,
		);
	}

	Ok(())
}
