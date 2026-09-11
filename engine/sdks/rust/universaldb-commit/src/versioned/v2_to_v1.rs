#![allow(dead_code, unused_variables)]

use anyhow::Result;

use crate::generated::{v1, v2};

pub fn convert_conflict_range_type_v2_to_v1(
	x: v2::ConflictRangeType,
) -> Result<v1::ConflictRangeType> {
	Ok(match x {
		v2::ConflictRangeType::Read => v1::ConflictRangeType::Read,
		v2::ConflictRangeType::Write => v1::ConflictRangeType::Write,
	})
}

pub fn convert_conflict_range_v2_to_v1(x: v2::ConflictRange) -> Result<v1::ConflictRange> {
	Ok(v1::ConflictRange {
		begin: x.begin,
		end: x.end,
		kind: convert_conflict_range_type_v2_to_v1(x.kind)?,
	})
}

pub fn convert_mutation_type_v2_to_v1(x: v2::MutationType) -> Result<v1::MutationType> {
	Ok(match x {
		v2::MutationType::Add => v1::MutationType::Add,
		v2::MutationType::And => v1::MutationType::And,
		v2::MutationType::BitAnd => v1::MutationType::BitAnd,
		v2::MutationType::Or => v1::MutationType::Or,
		v2::MutationType::BitOr => v1::MutationType::BitOr,
		v2::MutationType::Xor => v1::MutationType::Xor,
		v2::MutationType::BitXor => v1::MutationType::BitXor,
		v2::MutationType::AppendIfFits => v1::MutationType::AppendIfFits,
		v2::MutationType::Max => v1::MutationType::Max,
		v2::MutationType::Min => v1::MutationType::Min,
		v2::MutationType::SetVersionstampedKey => v1::MutationType::SetVersionstampedKey,
		v2::MutationType::SetVersionstampedValue => v1::MutationType::SetVersionstampedValue,
		v2::MutationType::ByteMin => v1::MutationType::ByteMin,
		v2::MutationType::ByteMax => v1::MutationType::ByteMax,
		v2::MutationType::CompareAndClear => v1::MutationType::CompareAndClear,
	})
}

pub fn convert_set_value_v2_to_v1(x: v2::SetValue) -> Result<v1::SetValue> {
	Ok(v1::SetValue {
		key: x.key,
		value: x.value,
	})
}

pub fn convert_clear_v2_to_v1(x: v2::Clear) -> Result<v1::Clear> {
	Ok(v1::Clear { key: x.key })
}

pub fn convert_clear_range_v2_to_v1(x: v2::ClearRange) -> Result<v1::ClearRange> {
	Ok(v1::ClearRange {
		begin: x.begin,
		end: x.end,
	})
}

pub fn convert_atomic_op_v2_to_v1(x: v2::AtomicOp) -> Result<v1::AtomicOp> {
	Ok(v1::AtomicOp {
		key: x.key,
		param: x.param,
		op_type: convert_mutation_type_v2_to_v1(x.op_type)?,
	})
}

pub fn convert_operation_v2_to_v1(x: v2::Operation) -> Result<v1::Operation> {
	Ok(match x {
		v2::Operation::SetValue(v) => v1::Operation::SetValue(convert_set_value_v2_to_v1(v)?),
		v2::Operation::Clear(v) => v1::Operation::Clear(convert_clear_v2_to_v1(v)?),
		v2::Operation::ClearRange(v) => v1::Operation::ClearRange(convert_clear_range_v2_to_v1(v)?),
		v2::Operation::AtomicOp(v) => v1::Operation::AtomicOp(convert_atomic_op_v2_to_v1(v)?),
	})
}

pub fn convert_commit_request_v2_to_v1(x: v2::CommitRequest) -> Result<v1::CommitRequest> {
	Ok(v1::CommitRequest {
		read_version: x.read_version,
		conflict_ranges: x
			.conflict_ranges
			.into_iter()
			.map(|v| convert_conflict_range_v2_to_v1(v))
			.collect::<Result<Vec<_>>>()?,
		operations: x
			.operations
			.into_iter()
			.map(|v| convert_operation_v2_to_v1(v))
			.collect::<Result<Vec<_>>>()?,
		client_node_id: x.client_node_id,
		client_seq: x.client_seq,
	})
}

pub fn convert_commit_committed_v2_to_v1(x: v2::CommitCommitted) -> Result<v1::CommitCommitted> {
	Ok(v1::CommitCommitted {
		commit_version: x.commit_version,
	})
}

pub fn convert_commit_reply_v2_to_v1(x: v2::CommitReply) -> Result<v1::CommitReply> {
	Ok(match x {
		v2::CommitReply::CommitCommitted(v) => {
			v1::CommitReply::CommitCommitted(convert_commit_committed_v2_to_v1(v)?)
		}
		v2::CommitReply::CommitConflict => v1::CommitReply::CommitConflict,
	})
}

pub fn convert_watermark_v2_to_v1(x: v2::Watermark) -> Result<v1::Watermark> {
	Ok(v1::Watermark {
		durable_version: x.durable_version,
	})
}
