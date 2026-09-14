//! Chunked commit request encoding and leader-side reassembly.

use std::time::Instant;

use super::{
	super::codec::{self, CHUNKED_COMMIT_PROTOCOL_VERSION},
	ChunkAssembler, CommitChunk, PENDING_CHUNK_MAX_IDLE,
};

const NODE_ID: &[u8] = b"follower-node";
const CLIENT_SEQ: u64 = 7;

fn chunk(attempt: u32, index: u32, count: u32, data: &[u8]) -> CommitChunk {
	CommitChunk {
		client_node_id: NODE_ID.to_vec(),
		client_seq: CLIENT_SEQ,
		attempt,
		index,
		count,
		data: data.to_vec(),
	}
}

#[test]
fn encoded_chunks_fit_max_payload_and_reassemble() {
	let request: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();

	for max_payload in [256, 4096, 65_536, 1024 * 1024] {
		let encoded = codec::encode_commit_request_chunks(
			&request,
			NODE_ID,
			CLIENT_SEQ,
			3,
			max_payload,
			CHUNKED_COMMIT_PROTOCOL_VERSION,
		)
		.unwrap();
		assert!(
			encoded.iter().all(|bytes| bytes.len() <= max_payload),
			"a chunk exceeded max_payload {max_payload}"
		);

		let mut assembler = ChunkAssembler::default();
		let now = Instant::now();
		let last = encoded.len() - 1;
		for (i, bytes) in encoded.iter().enumerate() {
			let decoded = codec::decode_commit_request_chunk(bytes).unwrap();
			let assembled = assembler.push(decoded, now);
			if i < last {
				assert!(
					assembled.is_none(),
					"request completed before its last chunk"
				);
			} else {
				assert_eq!(assembled.as_deref(), Some(request.as_slice()));
			}
		}
	}
}

#[test]
fn chunks_require_the_chunked_protocol_version() {
	let result = codec::encode_commit_request_chunks(
		&[0; 1024],
		NODE_ID,
		CLIENT_SEQ,
		0,
		256,
		CHUNKED_COMMIT_PROTOCOL_VERSION - 1,
	);
	assert!(result.is_err());
}

#[test]
fn max_payload_smaller_than_the_envelope_is_rejected() {
	let result = codec::encode_commit_request_chunks(
		&[0; 1024],
		NODE_ID,
		CLIENT_SEQ,
		0,
		16,
		CHUNKED_COMMIT_PROTOCOL_VERSION,
	);
	assert!(result.is_err());
}

#[test]
fn gap_abandons_the_attempt_until_it_is_resent() {
	let mut assembler = ChunkAssembler::default();
	let now = Instant::now();

	assert!(assembler.push(chunk(0, 0, 3, b"a"), now).is_none());
	assert!(assembler.push(chunk(0, 2, 3, b"c"), now).is_none());
	// The gap dropped the attempt, so the late middle piece cannot complete it.
	assert!(assembler.push(chunk(0, 1, 3, b"b"), now).is_none());

	assert!(assembler.push(chunk(1, 0, 3, b"a"), now).is_none());
	assert!(assembler.push(chunk(1, 1, 3, b"b"), now).is_none());
	assert_eq!(
		assembler.push(chunk(1, 2, 3, b"c"), now).as_deref(),
		Some(&b"abc"[..])
	);
}

#[test]
fn pieces_of_an_earlier_attempt_are_ignored() {
	let mut assembler = ChunkAssembler::default();
	let now = Instant::now();

	assert!(assembler.push(chunk(1, 0, 2, b"new-"), now).is_none());
	assert!(assembler.push(chunk(0, 1, 2, b"old"), now).is_none());
	assert!(assembler.push(chunk(0, 0, 2, b"old-"), now).is_none());
	assert_eq!(
		assembler.push(chunk(1, 1, 2, b"piece"), now).as_deref(),
		Some(&b"new-piece"[..])
	);
}

#[test]
fn idle_requests_are_evicted() {
	let mut assembler = ChunkAssembler::default();
	let start = Instant::now();

	assert!(assembler.push(chunk(0, 0, 2, b"a"), start).is_none());
	assembler.evict_idle(start + PENDING_CHUNK_MAX_IDLE);
	assert!(
		assembler
			.push(chunk(0, 1, 2, b"b"), start + PENDING_CHUNK_MAX_IDLE)
			.is_none()
	);
}
