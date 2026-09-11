use anyhow::{Context, Result};
use rivet_universaldb_commit::{self as proto, versioned};
use vbare::OwnedVersionedData;

use crate::{
	options::{ConflictRangeType, MutationType},
	tx_ops::Operation,
};

use super::{chunks::CommitChunk, transport::CommitOutcome};

/// Protocol version that introduced [`proto::CommitRequestChunk`]. A fleet negotiated below it has
/// leaders that cannot reassemble a chunked request.
pub const CHUNKED_COMMIT_PROTOCOL_VERSION: u16 = 2;

/// Decoded form of a commit request payload sent from a follower to the leader over NATS.
pub struct DecodedCommit {
	pub read_version: u64,
	pub conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
	pub operations: Vec<Operation>,
	/// Submitting follower's node id (part of the failover dedup key).
	pub client_node_id: Vec<u8>,
	/// Per-process monotonic counter (part of the failover dedup key).
	pub client_seq: u64,
}

/// Encode a follower's commit request at `protocol_version`, the version negotiated across the
/// fleet, with an embedded version header. Encoding at the compiled version instead would produce
/// bytes a leader running older code cannot decode during a rolling deploy.
pub fn encode_commit_request(
	read_version: u64,
	conflict_ranges: &[(Vec<u8>, Vec<u8>, ConflictRangeType)],
	operations: &[Operation],
	client_node_id: &[u8],
	client_seq: u64,
	protocol_version: u16,
) -> Result<Vec<u8>> {
	let request = proto::CommitRequest {
		read_version,
		conflict_ranges: conflict_ranges
			.iter()
			.map(|(begin, end, kind)| proto::ConflictRange {
				begin: begin.clone(),
				end: end.clone(),
				kind: conflict_range_type_to_proto(*kind),
			})
			.collect(),
		operations: operations.iter().map(operation_to_proto).collect(),
		client_node_id: client_node_id.to_vec(),
		client_seq,
	};

	versioned::CommitRequest::wrap_latest(request).serialize_with_embedded_version(protocol_version)
}

/// Decode a commit request payload produced by [`encode_commit_request`].
pub fn decode_commit_request(payload: &[u8]) -> Result<DecodedCommit> {
	let request = versioned::CommitRequest::deserialize_with_embedded_version(payload)?;

	let conflict_ranges = request
		.conflict_ranges
		.into_iter()
		.map(|range| {
			(
				range.begin,
				range.end,
				conflict_range_type_from_proto(range.kind),
			)
		})
		.collect();

	let operations = request
		.operations
		.into_iter()
		.map(operation_from_proto)
		.collect();

	Ok(DecodedCommit {
		read_version: request.read_version,
		conflict_ranges,
		operations,
		client_node_id: request.client_node_id,
		client_seq: request.client_seq,
	})
}

/// Split an encoded commit request into chunk messages that each fit within `max_payload` bytes,
/// encoded at `protocol_version`, which must be at least [`CHUNKED_COMMIT_PROTOCOL_VERSION`].
pub fn encode_commit_request_chunks(
	request: &[u8],
	client_node_id: &[u8],
	client_seq: u64,
	attempt: u32,
	max_payload: usize,
	protocol_version: u16,
) -> Result<Vec<Vec<u8>>> {
	let encode = |index: u32, count: u32, data: Vec<u8>| {
		versioned::CommitRequestChunk::wrap_latest(proto::CommitRequestChunk {
			client_node_id: client_node_id.to_vec(),
			client_seq,
			attempt,
			index,
			count,
			data,
		})
		.serialize_with_embedded_version(protocol_version)
	};

	// Every field except `data` has the same encoded width in every chunk, so an empty chunk measures
	// the envelope. The length prefix of `data` grows from one byte to at most five as a piece grows.
	let overhead = encode(0, 0, Vec::new())?.len() + 4;
	let piece_len = max_payload
		.checked_sub(overhead)
		.filter(|len| *len > 0)
		.with_context(|| {
			format!("nats max_payload of {max_payload} bytes cannot fit a commit request chunk")
		})?;
	let count = u32::try_from(request.len().div_ceil(piece_len))
		.context("commit request needs too many chunks")?;

	request
		.chunks(piece_len)
		.zip(0..)
		.map(|(piece, index)| encode(index, count, piece.to_vec()))
		.collect()
}

/// Decode one chunk produced by [`encode_commit_request_chunks`].
pub fn decode_commit_request_chunk(payload: &[u8]) -> Result<CommitChunk> {
	let chunk = versioned::CommitRequestChunk::deserialize_with_embedded_version(payload)?;
	Ok(CommitChunk {
		client_node_id: chunk.client_node_id,
		client_seq: chunk.client_seq,
		attempt: chunk.attempt,
		index: chunk.index,
		count: chunk.count,
		data: chunk.data,
	})
}

/// Encode a leader's commit reply at `protocol_version`, the version negotiated across the fleet,
/// with an embedded version header.
pub fn encode_commit_reply(outcome: CommitOutcome, protocol_version: u16) -> Result<Vec<u8>> {
	let reply = match outcome {
		CommitOutcome::Committed { commit_version } => {
			proto::CommitReply::CommitCommitted(proto::CommitCommitted { commit_version })
		}
		CommitOutcome::Conflict => proto::CommitReply::CommitConflict,
	};

	versioned::CommitReply::wrap_latest(reply).serialize_with_embedded_version(protocol_version)
}

/// Decode a commit reply payload produced by [`encode_commit_reply`].
pub fn decode_commit_reply(payload: &[u8]) -> Result<CommitOutcome> {
	let reply = versioned::CommitReply::deserialize_with_embedded_version(payload)?;
	Ok(match reply {
		proto::CommitReply::CommitCommitted(proto::CommitCommitted { commit_version }) => {
			CommitOutcome::Committed { commit_version }
		}
		proto::CommitReply::CommitConflict => CommitOutcome::Conflict,
	})
}

/// Encode a durable-version watermark broadcast at `protocol_version`, the version negotiated
/// across the fleet, with an embedded version header.
pub fn encode_watermark(durable_version: i64, protocol_version: u16) -> Result<Vec<u8>> {
	versioned::Watermark::wrap_latest(proto::Watermark { durable_version })
		.serialize_with_embedded_version(protocol_version)
}

/// Decode a watermark payload produced by [`encode_watermark`], returning the durable version.
pub fn decode_watermark(payload: &[u8]) -> Result<i64> {
	let watermark = versioned::Watermark::deserialize_with_embedded_version(payload)?;
	Ok(watermark.durable_version)
}

fn conflict_range_type_to_proto(kind: ConflictRangeType) -> proto::ConflictRangeType {
	match kind {
		ConflictRangeType::Read => proto::ConflictRangeType::Read,
		ConflictRangeType::Write => proto::ConflictRangeType::Write,
	}
}

fn conflict_range_type_from_proto(kind: proto::ConflictRangeType) -> ConflictRangeType {
	match kind {
		proto::ConflictRangeType::Read => ConflictRangeType::Read,
		proto::ConflictRangeType::Write => ConflictRangeType::Write,
	}
}

fn operation_to_proto(op: &Operation) -> proto::Operation {
	match op {
		Operation::SetValue { key, value } => proto::Operation::SetValue(proto::SetValue {
			key: key.clone(),
			value: value.clone(),
		}),
		Operation::Clear { key } => proto::Operation::Clear(proto::Clear { key: key.clone() }),
		Operation::ClearRange { begin, end } => proto::Operation::ClearRange(proto::ClearRange {
			begin: begin.clone(),
			end: end.clone(),
		}),
		Operation::AtomicOp {
			key,
			param,
			op_type,
		} => proto::Operation::AtomicOp(proto::AtomicOp {
			key: key.clone(),
			param: param.clone(),
			op_type: mutation_type_to_proto(*op_type),
		}),
	}
}

fn operation_from_proto(op: proto::Operation) -> Operation {
	match op {
		proto::Operation::SetValue(proto::SetValue { key, value }) => {
			Operation::SetValue { key, value }
		}
		proto::Operation::Clear(proto::Clear { key }) => Operation::Clear { key },
		proto::Operation::ClearRange(proto::ClearRange { begin, end }) => {
			Operation::ClearRange { begin, end }
		}
		proto::Operation::AtomicOp(proto::AtomicOp {
			key,
			param,
			op_type,
		}) => Operation::AtomicOp {
			key,
			param,
			op_type: mutation_type_from_proto(op_type),
		},
	}
}

fn mutation_type_to_proto(op_type: MutationType) -> proto::MutationType {
	match op_type {
		MutationType::Add => proto::MutationType::Add,
		MutationType::And => proto::MutationType::And,
		MutationType::BitAnd => proto::MutationType::BitAnd,
		MutationType::Or => proto::MutationType::Or,
		MutationType::BitOr => proto::MutationType::BitOr,
		MutationType::Xor => proto::MutationType::Xor,
		MutationType::BitXor => proto::MutationType::BitXor,
		MutationType::AppendIfFits => proto::MutationType::AppendIfFits,
		MutationType::Max => proto::MutationType::Max,
		MutationType::Min => proto::MutationType::Min,
		MutationType::SetVersionstampedKey => proto::MutationType::SetVersionstampedKey,
		MutationType::SetVersionstampedValue => proto::MutationType::SetVersionstampedValue,
		MutationType::ByteMin => proto::MutationType::ByteMin,
		MutationType::ByteMax => proto::MutationType::ByteMax,
		MutationType::CompareAndClear => proto::MutationType::CompareAndClear,
	}
}

fn mutation_type_from_proto(op_type: proto::MutationType) -> MutationType {
	match op_type {
		proto::MutationType::Add => MutationType::Add,
		proto::MutationType::And => MutationType::And,
		proto::MutationType::BitAnd => MutationType::BitAnd,
		proto::MutationType::Or => MutationType::Or,
		proto::MutationType::BitOr => MutationType::BitOr,
		proto::MutationType::Xor => MutationType::Xor,
		proto::MutationType::BitXor => MutationType::BitXor,
		proto::MutationType::AppendIfFits => MutationType::AppendIfFits,
		proto::MutationType::Max => MutationType::Max,
		proto::MutationType::Min => MutationType::Min,
		proto::MutationType::SetVersionstampedKey => MutationType::SetVersionstampedKey,
		proto::MutationType::SetVersionstampedValue => MutationType::SetVersionstampedValue,
		proto::MutationType::ByteMin => MutationType::ByteMin,
		proto::MutationType::ByteMax => MutationType::ByteMax,
		proto::MutationType::CompareAndClear => MutationType::CompareAndClear,
	}
}
