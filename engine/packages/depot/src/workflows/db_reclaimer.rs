use crate::{
	compaction::{
		companion::{CompanionKind, run_companion_loop},
		shared::*,
		*,
	},
	workflows::db_manager::branch_record_is_live_at_generation,
};
use universaldb::prelude::Priority;

#[cfg(feature = "test-faults")]
use crate::compaction::test_hooks;
#[cfg(feature = "test-faults")]
use crate::fault::ReclaimFaultPoint;

#[workflow(DbReclaimerWorkflow)]
pub async fn depot_db_reclaimer(ctx: &mut WorkflowCtx, input: &DbReclaimerInput) -> Result<()> {
	run_companion_loop(ctx, input.database_branch_id, CompanionKind::Reclaim).await
}

#[activity(ReclaimFdbJob)]
pub async fn reclaim_fdb_job(
	ctx: &ActivityCtx,
	input: &ReclaimFdbJobInput,
) -> Result<ReclaimFdbJobOutput> {
	let input = input.clone();
	let input_for_tx = input.clone();
	let now_ms = ctx.ts();

	let output = ctx
		.udb()?
		.txn("depot_reclaim_fdb", move |tx| {
			let input = input_for_tx.clone();
			async move {
				tx.priority(Priority::Low)?;
				reclaim_fdb_job_tx(&tx, &input, now_ms).await
			}
		})
		.await?;
	Ok(output)
}

async fn reclaim_fdb_job_tx(
	tx: &universaldb::Transaction,
	input: &ReclaimFdbJobInput,
	now_ms: i64,
) -> Result<ReclaimFdbJobOutput> {
	if input.job_kind != CompactionJobKind::Reclaim {
		return Ok(rejected_reclaim_job("reclaimer received a non-reclaim job"));
	}
	#[cfg(feature = "test-faults")]
	if let Some(output) = reclaim_fdb_fault_output(
		input.database_branch_id,
		ReclaimFaultPoint::PlanBeforeSnapshot,
	)
	.await?
	{
		return Ok(output);
	}
	if input.input_range.txid_refs.is_empty() && !input.input_range.staged_hot_shards.is_empty() {
		return cleanup_repair_fdb_outputs_tx(tx, input).await;
	}

	let branch_record = tx_get_value(
		tx,
		&keys::branches_list_key(input.database_branch_id),
		Serializable,
	)
	.await?
	.as_deref()
	.map(decode_database_branch_record)
	.transpose()
	.context("decode sqlite database branch record for FDB reclaim")?;
	if !branch_record_is_live_at_generation(branch_record.as_ref(), input.base_lifecycle_generation)
	{
		return Ok(rejected_reclaim_job("database branch lifecycle changed"));
	}

	let root = tx_get_value(
		tx,
		&keys::branch_compaction_root_key(input.database_branch_id),
		Serializable,
	)
	.await?
	.as_deref()
	.map(decode_compaction_root)
	.transpose()
	.context("decode sqlite compaction root for FDB reclaim")?
	.unwrap_or(CompactionRoot {
		schema_version: 1,
		manifest_generation: 0,
		hot_watermark_txid: 0,
		cold_watermark_txid: 0,
		cold_watermark_versionstamp: [0; 16],
	});
	if root.manifest_generation != input.base_manifest_generation {
		return Ok(rejected_reclaim_job("base manifest generation changed"));
	}

	let mut db_pins =
		history_pin::read_db_history_pins(tx, input.database_branch_id, Serializable).await?;
	if resolve_bucket_fork_pins(tx, input.database_branch_id, &mut db_pins).await? {
		return Ok(rejected_reclaim_job("bucket fork proof is ambiguous"));
	}
	let snapshot = read_reclaim_input_snapshot(
		tx,
		input.database_branch_id,
		&root,
		&db_pins,
		Serializable,
		now_ms,
	)
	.await?;
	if !input.input_range.txid_refs.is_empty() && snapshot.txid_refs != input.input_range.txid_refs
	{
		return Ok(rejected_reclaim_job("reclaim txid set changed"));
	}
	if !input.input_range.txid_refs.is_empty() {
		if !snapshot.pidx_entries.is_empty() {
			return Ok(rejected_reclaim_job("PIDX still references reclaim txids"));
		}
		if !reclaim_coverage_is_complete(&snapshot) {
			return Ok(rejected_reclaim_job(
				"replacement SHARD coverage is missing",
			));
		}
	}

	let input_fingerprint = fingerprint_reclaim_inputs(input.database_branch_id, &root, &snapshot);
	if input_fingerprint != input.input_fingerprint {
		return Ok(rejected_reclaim_job("reclaim input fingerprint changed"));
	}
	#[cfg(feature = "test-faults")]
	if let Some(output) = reclaim_fdb_fault_output(
		input.database_branch_id,
		ReclaimFaultPoint::PlanAfterSnapshot,
	)
	.await?
	{
		return Ok(output);
	}

	let selected_reclaim_txids = input
		.input_range
		.txid_refs
		.iter()
		.map(|txid_ref| txid_ref.txid)
		.collect::<BTreeSet<_>>();
	let mut key_count = 0_u32;
	let mut byte_count = 0_u64;
	#[cfg(feature = "test-faults")]
	if let Some(output) =
		reclaim_fdb_fault_output(input.database_branch_id, ReclaimFaultPoint::BeforeHotDelete)
			.await?
	{
		return Ok(output);
	}
	for (txid, key, value, commit) in &snapshot.commits {
		if !selected_reclaim_txids.contains(txid) {
			continue;
		}
		udb::compare_and_clear(tx, key, value);
		key_count = key_count.saturating_add(1);
		byte_count = byte_count.saturating_add(u64::try_from(value.len()).unwrap_or(u64::MAX));

		let vtx_key = keys::branch_vtx_key(input.database_branch_id, commit.versionstamp);
		if let Some(vtx_value) = tx_get_value(tx, &vtx_key, Serializable).await? {
			if vtx_value == txid.to_be_bytes() {
				udb::compare_and_clear(tx, &vtx_key, &vtx_value);
				key_count = key_count.saturating_add(1);
				byte_count =
					byte_count.saturating_add(u64::try_from(vtx_value.len()).unwrap_or(u64::MAX));
			} else {
				return Ok(rejected_reclaim_job("VTX row changed for reclaim txid"));
			}
		}
	}
	for (key, value) in &snapshot.delta_chunks {
		let txid = keys::decode_branch_delta_chunk_txid(input.database_branch_id, key)?;
		if !selected_reclaim_txids.contains(&txid) {
			continue;
		}
		udb::compare_and_clear(tx, key, value);
		key_count = key_count.saturating_add(1);
		byte_count = byte_count.saturating_add(u64::try_from(value.len()).unwrap_or(u64::MAX));
	}
	for (_, key, value, _) in &snapshot.expired_pitr_interval_rows {
		udb::compare_and_clear(tx, key, value);
		key_count = key_count.saturating_add(1);
		byte_count = byte_count.saturating_add(u64::try_from(value.len()).unwrap_or(u64::MAX));
	}
	#[cfg(feature = "test-faults")]
	if let Some(output) =
		reclaim_fdb_fault_output(input.database_branch_id, ReclaimFaultPoint::AfterHotDelete)
			.await?
	{
		return Ok(output);
	}

	Ok(ReclaimFdbJobOutput {
		status: CompactionJobStatus::Succeeded,
		output_refs: vec![ReclaimOutputRef {
			key_count,
			byte_count,
			min_txid: input.input_range.txids.min_txid,
			max_txid: input.input_range.txids.max_txid,
		}],
	})
}

pub(super) async fn cleanup_repair_fdb_outputs_tx(
	tx: &universaldb::Transaction,
	input: &ReclaimFdbJobInput,
) -> Result<ReclaimFdbJobOutput> {
	let input_fingerprint =
		fingerprint_repair_reclaim_range(input.database_branch_id, &input.input_range);
	if input_fingerprint != input.input_fingerprint {
		return Ok(rejected_reclaim_job(
			"repair cleanup input fingerprint changed",
		));
	}

	let branch_record = tx_get_value(
		tx,
		&keys::branches_list_key(input.database_branch_id),
		Serializable,
	)
	.await?
	.as_deref()
	.map(decode_database_branch_record)
	.transpose()
	.context("decode sqlite database branch record for repair cleanup")?;
	if !branch_record_is_live_at_generation(branch_record.as_ref(), input.base_lifecycle_generation)
	{
		return Ok(rejected_reclaim_job("database branch lifecycle changed"));
	}

	let root = tx_get_value(
		tx,
		&keys::branch_compaction_root_key(input.database_branch_id),
		Snapshot,
	)
	.await?
	.as_deref()
	.map(decode_compaction_root)
	.transpose()
	.context("decode sqlite compaction root for repair cleanup")?;
	let manifest_generation = root
		.as_ref()
		.map(|root| root.manifest_generation)
		.unwrap_or(input.base_manifest_generation);

	let mut key_count = 0_u32;
	let mut byte_count = 0_u64;
	for staged in &input.input_range.staged_hot_shards {
		let stage_key = keys::branch_compaction_stage_hot_shard_key(
			input.database_branch_id,
			staged.job_id,
			staged.output_ref.shard_id,
			staged.output_ref.as_of_txid,
			0,
		);
		let Some(stage_value) = tx_get_value(tx, &stage_key, Serializable).await? else {
			continue;
		};
		if staged.output_ref.size_bytes != u64::try_from(stage_value.len()).unwrap_or(u64::MAX)
			|| staged.output_ref.content_hash != content_hash(&stage_value)
		{
			tracing::error!(
				?input.database_branch_id,
				manifest_generation,
				?staged.job_id,
				shard_id = staged.output_ref.shard_id,
				as_of_txid = staged.output_ref.as_of_txid,
				repair_action = "retain_staged_hot_output",
				"staged hot shard cleanup found mismatched bytes"
			);
			return Ok(rejected_reclaim_job(
				"staged hot shard cleanup bytes changed",
			));
		}

		tracing::warn!(
			?input.database_branch_id,
			manifest_generation,
			?staged.job_id,
			shard_id = staged.output_ref.shard_id,
			as_of_txid = staged.output_ref.as_of_txid,
			repair_action = "clear_staged_hot_output",
			"clearing orphan staged hot shard output"
		);
		udb::compare_and_clear(tx, &stage_key, &stage_value);
		key_count = key_count.saturating_add(1);
		byte_count =
			byte_count.saturating_add(u64::try_from(stage_value.len()).unwrap_or(u64::MAX));
	}

	Ok(ReclaimFdbJobOutput {
		status: CompactionJobStatus::Succeeded,
		output_refs: vec![ReclaimOutputRef {
			key_count,
			byte_count,
			min_txid: input.input_range.txids.min_txid,
			max_txid: input.input_range.txids.max_txid,
		}],
	})
}

fn rejected_reclaim_job(reason: impl Into<String>) -> ReclaimFdbJobOutput {
	ReclaimFdbJobOutput {
		status: CompactionJobStatus::Rejected {
			reason: reason.into(),
		},
		output_refs: Vec::new(),
	}
}

#[cfg(feature = "test-faults")]
async fn reclaim_fdb_fault_output(
	database_branch_id: DatabaseBranchId,
	point: ReclaimFaultPoint,
) -> Result<Option<ReclaimFdbJobOutput>> {
	match test_hooks::maybe_fire_reclaim_fault(database_branch_id, point).await {
		Ok(Some(_)) | Ok(None) => Ok(None),
		Err(err) => Ok(Some(ReclaimFdbJobOutput {
			status: CompactionJobStatus::Failed {
				error: err.to_string(),
			},
			output_refs: Vec::new(),
		})),
	}
}
