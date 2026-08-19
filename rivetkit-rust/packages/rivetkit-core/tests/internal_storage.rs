use std::collections::HashMap;

use super::{KV_TX_MAX_PAYLOAD_BYTES, KV_TX_MAX_ROWS, split_kv_tx_chunks};
use crate::actor::connection::{PersistedConnection, encode_persisted_connection};
use crate::actor::context::ActorContext;
use crate::kv::Kv;
use crate::{StateDelta, WorkflowKvWrite};

fn workflow_storage_context() -> ActorContext {
	ActorContext::new_with_kv(
		"actor-workflow-storage",
		"workflow-storage-test",
		Vec::new(),
		"local",
		Kv::new_in_memory(),
	)
}

#[test]
fn kv_transaction_chunks_enforce_exact_row_boundaries() {
	for (row_count, expected_chunks) in [(127, 1), (128, 1), (129, 2)] {
		let entries = (0usize..row_count)
			.map(|index| (index.to_be_bytes().to_vec(), vec![0]))
			.collect::<Vec<_>>();
		let chunks = split_kv_tx_chunks(&entries);
		assert_eq!(chunks.len(), expected_chunks);
		assert!(chunks.iter().all(|chunk| chunk.len() <= KV_TX_MAX_ROWS));
		assert_eq!(
			chunks.iter().map(|chunk| chunk.len()).sum::<usize>(),
			row_count
		);
	}
}

#[test]
fn kv_transaction_chunks_enforce_exact_payload_boundaries() {
	let half = KV_TX_MAX_PAYLOAD_BYTES / 2;
	let exact = vec![
		(b"a".to_vec(), vec![0; half - 1]),
		(b"b".to_vec(), vec![0; half - 1]),
	];
	assert_eq!(
		exact
			.iter()
			.map(|(key, value)| key.len() + value.len())
			.sum::<usize>(),
		KV_TX_MAX_PAYLOAD_BYTES
	);
	assert_eq!(split_kv_tx_chunks(&exact).len(), 1);

	let over = vec![
		(b"a".to_vec(), vec![0; half]),
		(b"b".to_vec(), vec![0; half - 1]),
	];
	assert_eq!(
		over.iter()
			.map(|(key, value)| key.len() + value.len())
			.sum::<usize>(),
		KV_TX_MAX_PAYLOAD_BYTES + 1
	);
	assert_eq!(split_kv_tx_chunks(&over).len(), 2);
}

#[tokio::test]
async fn workflow_storage_crud_preserves_namespace_and_byte_order() {
	let ctx = workflow_storage_context();
	super::schema::ensure_internal_schema(ctx.sql())
		.await
		.expect("initialize workflow storage");
	let storage = ctx.workflow_storage();
	storage.set(b"a\0", b"one").await.expect("set first");
	storage.set(b"a\xff", b"two").await.expect("set second");
	storage.set(b"b", b"three").await.expect("set third");

	assert_eq!(
		storage.get(b"a\0").await.expect("get"),
		Some(b"one".to_vec())
	);
	assert_eq!(
		storage
			.list(b"a")
			.await
			.expect("list prefix")
			.into_iter()
			.map(|(key, _)| key)
			.collect::<Vec<_>>(),
		vec![b"a\0".to_vec(), b"a\xff".to_vec()],
	);

	storage
		.delete_range(b"a\0", b"a\xff")
		.await
		.expect("delete half-open range");
	assert_eq!(storage.get(b"a\0").await.expect("get deleted"), None);
	assert!(storage.get(b"a\xff").await.expect("get retained").is_some());
	storage
		.delete_prefix(b"a\xff")
		.await
		.expect("delete all-ff suffix prefix");
	assert_eq!(
		storage.list(b"").await.expect("list namespace"),
		vec![(b"b".to_vec(), b"three".to_vec())],
	);
}

#[tokio::test]
async fn workflow_storage_atomic_batch_rejects_over_budget_without_partial_writes() {
	let ctx = workflow_storage_context();
	super::schema::ensure_internal_schema(ctx.sql())
		.await
		.expect("initialize workflow storage");
	let storage = ctx.workflow_storage();
	let entries = (0..=KV_TX_MAX_ROWS)
		.map(|index| {
			(
				index.to_be_bytes().to_vec(),
				vec![u8::try_from(index % 255).unwrap()],
			)
		})
		.collect::<Vec<_>>();
	let refs = entries
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();

	storage
		.batch(&refs)
		.await
		.expect_err("over-budget workflow batch must be rejected");
	assert!(
		storage
			.list(b"")
			.await
			.expect("list after failure")
			.is_empty()
	);
}

#[tokio::test]
async fn workflow_storage_atomic_batch_enforces_exact_row_and_byte_boundaries() {
	let row_ctx = workflow_storage_context();
	super::schema::ensure_internal_schema(row_ctx.sql())
		.await
		.expect("initialize row-budget storage");
	let row_storage = row_ctx.workflow_storage();
	let exact_rows = (0..KV_TX_MAX_ROWS)
		.map(|index| (index.to_be_bytes().to_vec(), vec![0]))
		.collect::<Vec<_>>();
	let exact_row_refs = exact_rows
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	row_storage
		.batch(&exact_row_refs)
		.await
		.expect("exact row budget should succeed");
	assert_eq!(
		row_storage.list(b"").await.expect("list exact rows").len(),
		KV_TX_MAX_ROWS,
	);

	let byte_ctx = workflow_storage_context();
	super::schema::ensure_internal_schema(byte_ctx.sql())
		.await
		.expect("initialize byte-budget storage");
	let byte_storage = byte_ctx.workflow_storage();
	let half = KV_TX_MAX_PAYLOAD_BYTES / 2;
	// Each public one-byte key gains the hidden two-byte namespace prefix.
	let exact_bytes = vec![
		(b"a".to_vec(), vec![0; half - 3]),
		(b"b".to_vec(), vec![0; half - 3]),
	];
	let exact_byte_refs = exact_bytes
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	byte_storage
		.batch(&exact_byte_refs)
		.await
		.expect("exact byte budget should succeed");

	let over_bytes = vec![
		(b"c".to_vec(), vec![0; half - 3]),
		(b"d".to_vec(), vec![0; half - 2]),
	];
	let over_byte_refs = over_bytes
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	byte_storage
		.batch(&over_byte_refs)
		.await
		.expect_err("one byte over the budget must be rejected");
	assert_eq!(
		byte_storage
			.list(b"")
			.await
			.expect("list after byte-budget failure")
			.into_iter()
			.map(|(key, _)| key)
			.collect::<Vec<_>>(),
		vec![b"a".to_vec(), b"b".to_vec()],
	);
}

#[tokio::test]
async fn workflow_flush_budget_includes_actor_and_connection_state_rows() {
	let actor_ctx = workflow_storage_context();
	super::schema::ensure_internal_schema(actor_ctx.sql())
		.await
		.expect("initialize actor-state flush storage");
	let workflow_writes = (0..(KV_TX_MAX_ROWS - 1))
		.map(|index| WorkflowKvWrite {
			key: index.to_be_bytes().to_vec(),
			value: vec![0],
		})
		.collect::<Vec<_>>();
	actor_ctx
		.commit_serialized_state_and_workflow_batch(
			vec![StateDelta::ActorState(b"new-state".to_vec())],
			workflow_writes.clone(),
		)
		.await
		.expect_err("two actor rows plus 127 workflow rows exceed the row budget");
	assert_eq!(actor_ctx.state(), Vec::<u8>::new());
	assert!(
		actor_ctx
			.workflow_storage()
			.list(b"")
			.await
			.expect("list after actor-state failure")
			.is_empty(),
	);

	let connection_ctx = workflow_storage_context();
	super::schema::ensure_internal_schema(connection_ctx.sql())
		.await
		.expect("initialize connection-state flush storage");
	let connection = PersistedConnection {
		id: "conn-1".to_owned(),
		parameters: Vec::new(),
		state: b"connection-state".to_vec(),
		subscriptions: Vec::new(),
		gateway_id: [1, 2, 3, 4],
		request_id: [5, 6, 7, 8],
		server_message_index: 0,
		client_message_index: 0,
		request_path: "/".to_owned(),
		request_headers: HashMap::new(),
	};
	connection_ctx
		.commit_serialized_state_and_workflow_batch(
			vec![StateDelta::ConnHibernation {
				conn: connection.id.clone(),
				bytes: encode_persisted_connection(&connection)
					.expect("encode connection state delta"),
			}],
			workflow_writes,
		)
		.await
		.expect_err("two connection rows plus 127 workflow rows exceed the row budget");
	assert!(
		connection_ctx
			.workflow_storage()
			.list(b"")
			.await
			.expect("list after connection-state failure")
			.is_empty(),
	);
}
