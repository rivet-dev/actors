//! Real storage regression for the backfill's memory bound and durable workflow cursor.

use super::*;
use crate::db::Database;

fn backfill_read_bytes() -> u64 {
	rivet_metrics::REGISTRY
		.gather()
		.into_iter()
		.filter(|family| family.name() == "rivet_udb_operation_bytes")
		.flat_map(|family| family.get_metric().to_vec())
		.filter(|metric| {
			let has_label = |name, value| {
				metric
					.get_label()
					.iter()
					.any(|label| label.name() == name && label.value() == value)
			};
			has_label("name", "gas_debug_backfill_dead_workflows") && has_label("direction", "read")
		})
		.map(|metric| metric.get_counter().value() as u64)
		.sum()
}

#[tokio::test]
async fn backfill_skips_large_payloads_and_preserves_classification_and_cursors() -> Result<()> {
	let deps = rivet_test_deps::TestDeps::new().await?;
	let db = <DatabaseKv as Database>::new(deps.config().clone(), deps.pools().clone()).await?;
	assert_eq!(db.backfill_dead_workflows(1, None).await?, (0, None));

	let mut ids = (0..10)
		.map(|_| Id::new_v1(deps.config().dc_label()))
		.collect::<Vec<_>>();
	ids.sort();
	deps.pools()
		.udb()?
		.txn("seed_backfill_regression", |tx| {
			let ids = ids.clone();
			let subspace = db.subspace.clone();
			async move {
				let tx = tx.with_subspace(subspace.clone());
				for (index, id) in ids.iter().copied().enumerate() {
					if index != 7 && index != 9 {
						tx.write(
							&keys::workflow::NameKey::new(id),
							"backfill-regression".to_string(),
						)?;
					}
					if index != 8 && index != 9 {
						tx.write(&keys::workflow::ErrorKey::new(id), "failed".to_string())?;
					}
				}
				// One workflow contains many pages of irrelevant data. Classification must not load it.
				let payload = vec![b'x'; 1024];
				for chunk in 0..4096 {
					tx.set(
						&subspace.pack(&keys::workflow::InputKey::new(ids[0]).chunk(chunk)),
						&payload,
					);
				}
				tx.write(&keys::workflow::WorkerIdKey::new(ids[2]), ids[2])?;
				tx.write(&keys::workflow::HasWakeConditionKey::new(ids[3]), ())?;
				tx.write(&keys::workflow::SilenceTsKey::new(ids[4]), 1)?;
				// Any output chunk excludes the workflow, even if chunk zero is absent.
				let large_value = vec![b'x'; 256 * 1024];
				tx.set(
					&subspace.pack(&keys::workflow::OutputKey::new(ids[5]).chunk(3)),
					&large_value,
				);
				// Discovery must read only the key even when the first and only value is large.
				tx.set(
					&subspace.pack(&keys::workflow::InputKey::new(ids[9]).chunk(0)),
					&large_value,
				);
				Ok(())
			}
		})
		.await?;

	let before = backfill_read_bytes();
	let (count, mut cursor) = db.backfill_dead_workflows(1, None).await?;
	assert_eq!(count, 1);
	let bytes = backfill_read_bytes() - before;
	assert!(bytes > 0, "backfill byte instrumentation was not observed");
	assert!(
		bytes < 64 * 1024,
		"classifying one workflow read {bytes} bytes of unrelated payload"
	);
	assert!(cursor.is_some());
	// Recreate the database facade as on an activity retry, retaining only the durable cursor.
	let db = <DatabaseKv as Database>::new(deps.config().clone(), deps.pools().clone()).await?;

	// The pre-fix cursor is an actual first key of an unprocessed workflow, not an encoded struct.
	let legacy_cursor = db.subspace.pack(&keys::workflow::NameKey::new(ids[1]));
	let resumed = db.backfill_dead_workflows(1, Some(&legacy_cursor)).await?;
	assert_eq!(resumed.0, 1);
	let replayed = db.backfill_dead_workflows(1, Some(&legacy_cursor)).await?;
	assert_eq!(resumed, replayed);

	let mut total = count;
	for _ in 0..ids.len() + 1 {
		let Some(previous) = cursor else { break };
		// Round-trip the persisted bytes through JSON exactly like the existing activity input.
		let previous: Vec<u8> = serde_json::from_str(&serde_json::to_string(&previous)?)?;
		let (count, next) = db.backfill_dead_workflows(1, Some(&previous)).await?;
		total += count;
		if let Some(next) = &next {
			assert!(
				next > &previous,
				"cursor did not advance at a workflow boundary"
			);
		}
		cursor = next;
	}
	assert!(cursor.is_none());
	assert_eq!(total, ids.len());
	let total_bytes = backfill_read_bytes() - before;
	assert!(
		total_bytes < 64 * 1024,
		"backfill discovery or output detection loaded payloads: {total_bytes} bytes"
	);

	let indexed = deps
		.pools()
		.udb()?
		.txn("read_backfill_regression", |tx| {
			let subspace = db.subspace.clone();
			async move {
				let range = subspace.subspace(&keys::workflow::DeadIdxKey::subspace(
					"backfill-regression".to_string(),
				));
				tx.get_ranges_keyvalues((&range).into(), Snapshot)
					.map(|entry| {
						Ok(subspace
							.unpack::<keys::workflow::DeadIdxKey>(entry?.key())?
							.workflow_id)
					})
					.try_collect::<Vec<_>>()
					.await
			}
		})
		.await?;
	assert_eq!(indexed, vec![ids[0], ids[1], ids[6]]);
	Ok(())
}
