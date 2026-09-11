#![allow(dead_code, unused_variables)]

use anyhow::Result;

use crate::generated::{v1, v2};

pub fn convert_conflict_range_type_v1_to_v2(
	x: v1::ConflictRangeType,
) -> Result<v2::ConflictRangeType> {
	Ok(match x {
		v1::ConflictRangeType::Read => v2::ConflictRangeType::Read,
		v1::ConflictRangeType::Write => v2::ConflictRangeType::Write,
	})
}

pub fn convert_conflict_range_v1_to_v2(x: v1::ConflictRange) -> Result<v2::ConflictRange> {
	Ok(v2::ConflictRange {
		begin: x.begin,
		end: x.end,
		kind: convert_conflict_range_type_v1_to_v2(x.kind)?,
	})
}

pub fn convert_mutation_type_v1_to_v2(x: v1::MutationType) -> Result<v2::MutationType> {
	Ok(match x {
		v1::MutationType::Add => v2::MutationType::Add,
		v1::MutationType::And => v2::MutationType::And,
		v1::MutationType::BitAnd => v2::MutationType::BitAnd,
		v1::MutationType::Or => v2::MutationType::Or,
		v1::MutationType::BitOr => v2::MutationType::BitOr,
		v1::MutationType::Xor => v2::MutationType::Xor,
		v1::MutationType::BitXor => v2::MutationType::BitXor,
		v1::MutationType::AppendIfFits => v2::MutationType::AppendIfFits,
		v1::MutationType::Max => v2::MutationType::Max,
		v1::MutationType::Min => v2::MutationType::Min,
		v1::MutationType::SetVersionstampedKey => v2::MutationType::SetVersionstampedKey,
		v1::MutationType::SetVersionstampedValue => v2::MutationType::SetVersionstampedValue,
		v1::MutationType::ByteMin => v2::MutationType::ByteMin,
		v1::MutationType::ByteMax => v2::MutationType::ByteMax,
		v1::MutationType::CompareAndClear => v2::MutationType::CompareAndClear,
	})
}

pub fn convert_set_value_v1_to_v2(x: v1::SetValue) -> Result<v2::SetValue> {
	Ok(v2::SetValue {
		key: x.key,
		value: x.value,
	})
}

pub fn convert_clear_v1_to_v2(x: v1::Clear) -> Result<v2::Clear> {
	Ok(v2::Clear { key: x.key })
}

pub fn convert_clear_range_v1_to_v2(x: v1::ClearRange) -> Result<v2::ClearRange> {
	Ok(v2::ClearRange {
		begin: x.begin,
		end: x.end,
	})
}

pub fn convert_atomic_op_v1_to_v2(x: v1::AtomicOp) -> Result<v2::AtomicOp> {
	Ok(v2::AtomicOp {
		key: x.key,
		param: x.param,
		op_type: convert_mutation_type_v1_to_v2(x.op_type)?,
	})
}

pub fn convert_operation_v1_to_v2(x: v1::Operation) -> Result<v2::Operation> {
	Ok(match x {
		v1::Operation::SetValue(v) => v2::Operation::SetValue(convert_set_value_v1_to_v2(v)?),
		v1::Operation::Clear(v) => v2::Operation::Clear(convert_clear_v1_to_v2(v)?),
		v1::Operation::ClearRange(v) => v2::Operation::ClearRange(convert_clear_range_v1_to_v2(v)?),
		v1::Operation::AtomicOp(v) => v2::Operation::AtomicOp(convert_atomic_op_v1_to_v2(v)?),
	})
}

pub fn convert_commit_request_v1_to_v2(x: v1::CommitRequest) -> Result<v2::CommitRequest> {
	Ok(v2::CommitRequest {
		read_version: x.read_version,
		conflict_ranges: x
			.conflict_ranges
			.into_iter()
			.map(|v| convert_conflict_range_v1_to_v2(v))
			.collect::<Result<Vec<_>>>()?,
		operations: x
			.operations
			.into_iter()
			.map(|v| convert_operation_v1_to_v2(v))
			.collect::<Result<Vec<_>>>()?,
		client_node_id: x.client_node_id,
		client_seq: x.client_seq,
	})
}

pub fn convert_commit_committed_v1_to_v2(x: v1::CommitCommitted) -> Result<v2::CommitCommitted> {
	Ok(v2::CommitCommitted {
		commit_version: x.commit_version,
	})
}

pub fn convert_commit_reply_v1_to_v2(x: v1::CommitReply) -> Result<v2::CommitReply> {
	Ok(match x {
		v1::CommitReply::CommitCommitted(v) => {
			v2::CommitReply::CommitCommitted(convert_commit_committed_v1_to_v2(v)?)
		}
		v1::CommitReply::CommitConflict => v2::CommitReply::CommitConflict,
	})
}

pub fn convert_watermark_v1_to_v2(x: v1::Watermark) -> Result<v2::Watermark> {
	Ok(v2::Watermark {
		durable_version: x.durable_version,
	})
}
