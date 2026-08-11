use std::{
	collections::BTreeMap,
	sync::{Mutex, MutexGuard},
};

use anyhow::Result;

use crate::{
	atomic::apply_atomic_op,
	key_selector::KeySelector,
	options::{ConflictRangeType, MutationType},
	range_option::RangeOption,
	utils::{IsolationLevel, end_of_key_range},
	value::{KeyValue, Slice, Values},
};

#[derive(Debug, Clone)]
pub enum Operation {
	SetValue {
		key: Vec<u8>,
		value: Vec<u8>,
	},
	Clear {
		key: Vec<u8>,
	},
	ClearRange {
		begin: Vec<u8>,
		end: Vec<u8>,
	},
	AtomicOp {
		key: Vec<u8>,
		param: Vec<u8>,
		op_type: MutationType,
	},
}

/// Whether a transaction has nothing to commit: no mutations and no explicitly added write conflict
/// ranges. FDB never commits a read-only transaction, so it can never conflict and never causes
/// another transaction to conflict. Drivers use this to skip the commit path entirely.
pub fn is_read_only(
	operations: &[Operation],
	conflict_ranges: &[(Vec<u8>, Vec<u8>, ConflictRangeType)],
) -> bool {
	operations.is_empty()
		&& !conflict_ranges
			.iter()
			.any(|(_, _, conflict_type)| match conflict_type {
				ConflictRangeType::Write => true,
				ConflictRangeType::Read => false,
			})
}

#[derive(Debug, Clone)]
pub enum GetOutput {
	Value(Vec<u8>),
	Cleared,
	None,
	/// Indicates that atomic operations were found and need database value to resolve
	ApplyAtomicOps(Vec<(Vec<u8>, MutationType)>), // (param, op_type) pairs
}

#[derive(Default)]
pub struct TransactionOperations {
	operations: Mutex<Vec<Operation>>,
	conflict_ranges: Mutex<Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>>,
}

impl TransactionOperations {
	pub fn consume(&self) -> (Vec<Operation>, Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>) {
		(
			std::mem::take(&mut self.operations.lock().unwrap()),
			std::mem::take(&mut self.conflict_ranges.lock().unwrap()),
		)
	}

	pub fn add_operation(&self, operation: Operation) {
		self.operations.lock().unwrap().push(operation);
	}

	pub fn operations(&self) -> MutexGuard<'_, Vec<Operation>> {
		self.operations.lock().unwrap()
	}

	pub fn set(&self, key: &[u8], value: &[u8]) {
		self.add_conflict_range(key, &end_of_key_range(key), ConflictRangeType::Write);

		self.add_operation(Operation::SetValue {
			key: key.to_vec(),
			value: value.to_vec(),
		});
	}

	pub fn clear(&self, key: &[u8]) {
		self.add_conflict_range(key, &end_of_key_range(key), ConflictRangeType::Write);

		self.add_operation(Operation::Clear { key: key.to_vec() });
	}

	pub fn clear_range(&self, begin: &[u8], end: &[u8]) {
		self.add_conflict_range(begin, end, ConflictRangeType::Write);

		self.add_operation(Operation::ClearRange {
			begin: begin.to_vec(),
			end: end.to_vec(),
		});
	}

	pub fn atomic_op(&self, key: &[u8], param: &[u8], op_type: MutationType) {
		self.add_conflict_range(key, &end_of_key_range(key), ConflictRangeType::Write);

		self.add_operation(Operation::AtomicOp {
			key: key.to_vec(),
			param: param.to_vec(),
			op_type,
		});
	}

	pub fn get(&self, key: &[u8], isolation_level: IsolationLevel) -> GetOutput {
		if let IsolationLevel::Serializable = isolation_level {
			self.add_conflict_range(key, &end_of_key_range(key), ConflictRangeType::Read);
		}

		let mut atomic_ops: Vec<(Vec<u8>, MutationType)> = Vec::new();

		// Iterate through operations in reverse order to find the most recent operation for this key
		for op in self.operations().iter().rev() {
			match op {
				Operation::SetValue {
					key: set_key,
					value,
				} if set_key.as_slice() == key => {
					let mut result_value = Some(value.clone());

					// If we found atomic ops after this set, apply them to this value
					if !atomic_ops.is_empty() {
						// Apply atomic operations in forward order (reverse of how we collected them)
						for (param, op_type) in atomic_ops.into_iter().rev() {
							result_value =
								apply_atomic_op(result_value.as_deref(), &param, op_type);
						}
					}

					return result_value
						.map(GetOutput::Value)
						.unwrap_or(GetOutput::Cleared);
				}
				Operation::Clear { key: cleared_key } if cleared_key.as_slice() == key => {
					return GetOutput::Cleared;
				}
				Operation::ClearRange { begin, end }
					if key >= begin.as_slice() && key < end.as_slice() =>
				{
					return GetOutput::Cleared;
				}
				Operation::AtomicOp {
					key: atomic_key,
					param,
					op_type,
				} if atomic_key.as_slice() == key => {
					atomic_ops.push((param.clone(), *op_type));
				}
				_ => {}
			}
		}

		// If we found atomic operations but no set/clear, we need the database value
		if !atomic_ops.is_empty() {
			// Reverse to get operations in forward order
			atomic_ops.reverse();
			GetOutput::ApplyAtomicOps(atomic_ops)
		} else {
			GetOutput::None
		}
	}

	pub async fn get_with_callback<F, Fut>(
		&self,
		key: &[u8],
		isolation_level: IsolationLevel,
		get_from_db: F,
	) -> Result<Option<Slice>>
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = Result<Option<Slice>>>,
	{
		// Check local operations first
		match self.get(key, isolation_level) {
			GetOutput::Value(value) => Ok(Some(value.into())),
			GetOutput::Cleared => Ok(None),
			GetOutput::None => {
				// Fall back to database
				get_from_db().await
			}
			GetOutput::ApplyAtomicOps(atomic_ops) => {
				// Get the current value from database and apply atomic operations
				let db_value = get_from_db().await?;
				let mut result_value = db_value;

				// Apply all atomic operations in order
				for (param, op_type) in atomic_ops {
					result_value = apply_atomic_op(
						result_value.as_ref().map(|x| x.as_slice()),
						&param,
						op_type,
					)
					.map(Into::into);
				}

				Ok(result_value)
			}
		}
	}

	pub async fn get_key<F, Fut>(
		&self,
		selector: &KeySelector<'_>,
		isolation_level: IsolationLevel,
		get_from_db: F,
	) -> Result<Slice>
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = Result<Slice>>,
	{
		// Get the database result first
		let db_key = get_from_db().await?;

		// If there are no local operations, just return the database result
		if self.operations().is_empty() {
			// Add conflict range on resolved key
			if let IsolationLevel::Serializable = isolation_level {
				self.add_conflict_range(
					&db_key,
					&end_of_key_range(&db_key),
					ConflictRangeType::Read,
				);
			}

			return Ok(db_key);
		}

		// Check if db_key is cleared locally
		let db_key_cleared =
			!db_key.is_empty() && matches!(self.get(&db_key, isolation_level), GetOutput::Cleared);

		// Build a map of all local keys that currently exist (not cleared)
		let mut local_keys = BTreeMap::new();

		for op in &*self.operations() {
			match op {
				Operation::SetValue { key, .. } => {
					local_keys.insert(key.clone(), ());
				}
				Operation::Clear { key } => {
					local_keys.remove(key);
				}
				Operation::ClearRange { begin, end } => {
					let keys_to_remove: Vec<_> = local_keys
						.range(begin.clone()..end.clone())
						.map(|(k, _)| k.clone())
						.collect();
					for key in keys_to_remove {
						local_keys.remove(&key);
					}
				}
				// TODO: When MutationType::SetVersionstampedKey is implemented, fix
				Operation::AtomicOp { .. } => {}
			}
		}

		let search_key = selector.key().to_vec();
		let is_forward = selector.offset() >= 1;
		let include_equal = !selector.or_equal();

		// Find the best local key based on selector direction
		let best_local = if is_forward {
			// Looking for first key >= or > search_key
			if include_equal {
				local_keys
					.range(search_key.clone()..)
					.next()
					.map(|(k, _)| k.clone())
			} else {
				local_keys
					.range((
						std::ops::Bound::Excluded(search_key.clone()),
						std::ops::Bound::Unbounded,
					))
					.next()
					.map(|(k, _)| k.clone())
			}
		} else {
			// Looking for last key <= or < search_key
			if include_equal {
				local_keys
					.range(..=search_key.clone())
					.next_back()
					.map(|(k, _)| k.clone())
			} else {
				local_keys
					.range(..search_key.clone())
					.next_back()
					.map(|(k, _)| k.clone())
			}
		};

		// Determine which key to return
		let key = match (best_local, db_key_cleared) {
			(Some(local), false) if !db_key.is_empty() => {
				// Both keys exist, pick the appropriate one based on direction
				if is_forward {
					// Return the smaller key
					if db_key.as_slice() < local.as_slice() {
						db_key
					} else {
						local.into()
					}
				} else {
					// Return the larger key
					if db_key.as_slice() > local.as_slice() {
						db_key
					} else {
						local.into()
					}
				}
			}
			(Some(local), _) => local.into(),
			(None, false) => db_key,
			(None, true) => vec![].into(),
		};

		// Add conflict range on resolved key
		if let IsolationLevel::Serializable = isolation_level {
			self.add_conflict_range(&key, &end_of_key_range(&key), ConflictRangeType::Read);
		}

		Ok(key)
	}

	pub async fn get_range<F, Fut>(
		&self,
		opt: &RangeOption<'_>,
		isolation_level: IsolationLevel,
		get_from_db: F,
	) -> Result<Values>
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = Result<Values>>,
	{
		if let IsolationLevel::Serializable = isolation_level {
			self.add_conflict_range(opt.begin.key(), opt.end.key(), ConflictRangeType::Read);
		}

		// Get database results
		let db_values = get_from_db().await?;

		// If there are no local operations, just return the database results
		if self.operations().is_empty() {
			return Ok(db_values);
		}

		let begin = opt.begin.key();
		let end = opt.end.key();

		// Start with database results in a map
		let mut result_map = BTreeMap::new();
		for kv in db_values.into_iter() {
			let key = kv.key().to_vec();
			let value = kv.value().to_vec();
			result_map.insert(key, value);
		}

		// Apply local operations
		for op in &*self.operations() {
			match op {
				Operation::SetValue { key, value } => {
					if key.as_slice() >= begin && key.as_slice() < end {
						result_map.insert(key.clone(), value.clone());
					}
				}
				Operation::Clear { key } => {
					result_map.remove(key);
				}
				Operation::ClearRange {
					begin: clear_begin,
					end: clear_end,
				} => {
					// Remove all keys in the cleared range
					let keys_to_remove: Vec<_> = result_map
						.range(clear_begin.clone()..clear_end.clone())
						.map(|(k, _)| k.clone())
						.collect();
					for key in keys_to_remove {
						result_map.remove(&key);
					}
				}
				Operation::AtomicOp {
					key,
					param,
					op_type,
				} => {
					if key.as_slice() >= begin && key.as_slice() < end {
						// Get current value for this key (from result_map or empty if not exists)
						let current_value = result_map.get(key);
						let current_slice = current_value.map(|v| &**v);

						// Apply the atomic operation
						let new_value = apply_atomic_op(current_slice, param, *op_type);

						if let Some(new_value) = new_value {
							result_map.insert(key.clone(), new_value);
						} else {
							result_map.remove(key);
						}
					}
				}
			}
		}

		// Build result respecting the limit
		let mut keyvalues = Vec::new();
		let limit = opt.limit.unwrap_or(usize::MAX);

		for (key, value) in result_map.into_iter().take(limit) {
			keyvalues.push(KeyValue::new(key, value));
		}

		Ok(Values::new(keyvalues))
	}

	pub fn clear_all(&self) {
		self.operations.lock().unwrap().clear();
		self.conflict_ranges.lock().unwrap().clear();
	}

	pub fn add_conflict_range(&self, begin: &[u8], end: &[u8], conflict_type: ConflictRangeType) {
		self.conflict_ranges
			.lock()
			.unwrap()
			.push((begin.to_vec(), end.to_vec(), conflict_type));
	}
}
