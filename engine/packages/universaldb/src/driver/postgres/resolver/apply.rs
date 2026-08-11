use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};

use crate::{
	atomic::apply_atomic_op, options::MutationType, tuple::Versionstamp, tx_ops::Operation,
	versionstamp::substitute_raw_versionstamp,
};

/// A winning commit's Postgres-resolved version and its decoded operations. Winners are folded in id
/// order.
pub struct Winner {
	pub commit_version: u64,
	pub operations: Vec<Operation>,
}

/// The materialized result of folding a batch of winners over the current `kv` state. Each distinct
/// key appears at most once across `upserts` and `point_deletes`, so the batch's point writes
/// collapse to a fixed number of statements regardless of batch size.
pub struct WriteSet {
	pub upserts: Vec<(Vec<u8>, Vec<u8>)>,
	pub point_deletes: Vec<Vec<u8>>,
	pub range_deletes: Vec<(Vec<u8>, Vec<u8>)>,
}

/// In-memory working state layered over the pre-batch `kv` snapshot. Folding the batch's winners
/// through this overlay in id order reproduces the exact serial semantics of applying each commit
/// one at a time: a later commit's atomic read observes an earlier commit's write because the
/// overlay is the live working state.
struct Overlay<'a> {
	/// Pre-batch values for every key read by an atomic op in the batch. The only base read.
	base: &'a HashMap<Vec<u8>, Vec<u8>>,
	/// Point writes layered over the base. `Some` is a set, `None` is a point tombstone.
	points: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
	/// Range tombstones in fold order. A key with no overlaying point write that falls in any range
	/// reads as absent.
	ranges: Vec<(Vec<u8>, Vec<u8>)>,
}

impl<'a> Overlay<'a> {
	fn new(base: &'a HashMap<Vec<u8>, Vec<u8>>) -> Self {
		Overlay {
			base,
			points: BTreeMap::new(),
			ranges: Vec::new(),
		}
	}

	/// Read-through lookup: an overlaying point write wins, then a range tombstone, then the base.
	fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
		if let Some(value) = self.points.get(key) {
			return value.clone();
		}
		if self
			.ranges
			.iter()
			.any(|(begin, end)| key >= begin.as_slice() && key < end.as_slice())
		{
			return None;
		}
		self.base.get(key).cloned()
	}

	fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
		self.points.insert(key, Some(value));
	}

	fn clear(&mut self, key: Vec<u8>) {
		self.points.insert(key, None);
	}

	fn clear_range(&mut self, begin: Vec<u8>, end: Vec<u8>) {
		// Drop any point writes inside the range; the range delete subsumes them, and a later set of
		// a key in the range re-adds a point write that wins on read-through again.
		let covered: Vec<Vec<u8>> = self
			.points
			.range(begin.clone()..end.clone())
			.map(|(key, _)| key.clone())
			.collect();
		for key in covered {
			self.points.remove(&key);
		}
		self.ranges.push((begin, end));
	}

	fn into_write_set(self) -> WriteSet {
		let mut upserts = Vec::new();
		let mut point_deletes = Vec::new();
		for (key, value) in self.points {
			match value {
				Some(value) => upserts.push((key, value)),
				None => point_deletes.push(key),
			}
		}
		WriteSet {
			upserts,
			point_deletes,
			range_deletes: self.ranges,
		}
	}
}

/// Fold every winner's operations, in id order, into a single materialized write-set over the
/// pre-batch `base` snapshot. `base` must contain the current value of every key returned by
/// [`atomic_read_keys`].
pub fn fold_winners(winners: Vec<Winner>, base: &HashMap<Vec<u8>, Vec<u8>>) -> Result<WriteSet> {
	let mut overlay = Overlay::new(base);

	for winner in winners {
		// Distinguishes multiple versionstamped operations within a single commit so their 10-byte
		// stamps stay unique (8-byte version shared, 2-byte counter incremented). Resets per winner.
		let mut versionstamp_counter: u16 = 0;

		for op in winner.operations {
			match op {
				Operation::SetValue { key, value } => overlay.set(key, value),
				Operation::Clear { key } => overlay.clear(key),
				Operation::ClearRange { begin, end } => overlay.clear_range(begin, end),
				Operation::AtomicOp {
					key,
					param,
					op_type,
				} => fold_atomic(
					&mut overlay,
					key,
					param,
					op_type,
					winner.commit_version,
					&mut versionstamp_counter,
				)?,
			}
		}
	}

	Ok(overlay.into_write_set())
}

fn fold_atomic(
	overlay: &mut Overlay<'_>,
	key: Vec<u8>,
	param: Vec<u8>,
	op_type: MutationType,
	commit_version: u64,
	versionstamp_counter: &mut u16,
) -> Result<()> {
	match op_type {
		MutationType::SetVersionstampedKey => {
			let versionstamp = build_versionstamp(commit_version, versionstamp_counter);
			let key = substitute_raw_versionstamp(key, &versionstamp)
				.map_err(anyhow::Error::msg)
				.context("failed substituting versionstamped key")?;
			overlay.set(key, param);
		}
		MutationType::SetVersionstampedValue => {
			let versionstamp = build_versionstamp(commit_version, versionstamp_counter);
			let value = substitute_raw_versionstamp(param, &versionstamp)
				.map_err(anyhow::Error::msg)
				.context("failed substituting versionstamped value")?;
			overlay.set(key, value);
		}
		// Read-modify-write atomics: the leader is the single writer, so reading the overlay's
		// working value and writing the result is serializable with no lost update.
		MutationType::Add
		| MutationType::And
		| MutationType::BitAnd
		| MutationType::Or
		| MutationType::BitOr
		| MutationType::Xor
		| MutationType::BitXor
		| MutationType::AppendIfFits
		| MutationType::Max
		| MutationType::Min
		| MutationType::ByteMin
		| MutationType::ByteMax
		| MutationType::CompareAndClear => {
			let current = overlay.get(&key);
			match apply_atomic_op(current.as_deref(), &param, op_type) {
				Some(new_value) => overlay.set(key, new_value),
				None => overlay.clear(key),
			}
		}
	}

	Ok(())
}

/// Every key a winner's atomic op reads, so the leader can fetch them all in one bulk query before
/// folding. Versionstamped ops do not read, so their keys are skipped.
pub fn atomic_read_keys(winners: &[Winner]) -> Vec<Vec<u8>> {
	let mut keys = Vec::new();
	for winner in winners {
		for op in &winner.operations {
			if let Operation::AtomicOp { key, op_type, .. } = op {
				if reads_current_value(*op_type) {
					keys.push(key.clone());
				}
			}
		}
	}
	keys
}

fn reads_current_value(op_type: MutationType) -> bool {
	match op_type {
		MutationType::SetVersionstampedKey | MutationType::SetVersionstampedValue => false,
		MutationType::Add
		| MutationType::And
		| MutationType::BitAnd
		| MutationType::Or
		| MutationType::BitOr
		| MutationType::Xor
		| MutationType::BitXor
		| MutationType::AppendIfFits
		| MutationType::Max
		| MutationType::Min
		| MutationType::ByteMin
		| MutationType::ByteMax
		| MutationType::CompareAndClear => true,
	}
}

/// Build a 10-byte versionstamp (plus the 2 user-version bytes the substitution helper ignores)
/// from the Postgres-resolved commit version and a per-commit counter.
fn build_versionstamp(commit_version: u64, counter: &mut u16) -> Versionstamp {
	let mut bytes = [0u8; 12];
	bytes[0..8].copy_from_slice(&commit_version.to_be_bytes());
	bytes[8..10].copy_from_slice(&counter.to_be_bytes());
	*counter = counter.wrapping_add(1);
	Versionstamp::from(bytes)
}

// The fold operates on private types (`Overlay`, `Winner`, `WriteSet`) that integration tests under
// `tests/` cannot reach, so its unit tests live in a source-owned sibling file via this shim.
#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
