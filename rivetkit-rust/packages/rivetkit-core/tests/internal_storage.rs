use super::{KV_TX_MAX_PAYLOAD_BYTES, KV_TX_MAX_ROWS, split_kv_tx_chunks};

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
