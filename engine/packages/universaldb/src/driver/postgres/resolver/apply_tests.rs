use std::collections::HashMap;

use super::*;

fn k(s: &str) -> Vec<u8> {
	s.as_bytes().to_vec()
}

fn add_param(n: i64) -> Vec<u8> {
	n.to_le_bytes().to_vec()
}

fn read_int(value: &[u8]) -> i64 {
	let mut buf = [0u8; 8];
	let len = value.len().min(8);
	buf[..len].copy_from_slice(&value[..len]);
	i64::from_le_bytes(buf)
}

fn set(key: &str, value: &str) -> Operation {
	Operation::SetValue {
		key: k(key),
		value: k(value),
	}
}

fn clear(key: &str) -> Operation {
	Operation::Clear { key: k(key) }
}

fn clear_range(begin: &str, end: &str) -> Operation {
	Operation::ClearRange {
		begin: k(begin),
		end: k(end),
	}
}

fn add(key: &str, n: i64) -> Operation {
	Operation::AtomicOp {
		key: k(key),
		param: add_param(n),
		op_type: MutationType::Add,
	}
}

fn winner(commit_version: u64, operations: Vec<Operation>) -> Winner {
	Winner {
		commit_version,
		operations,
	}
}

/// Materialize a write-set over a base map to inspect the resulting `kv` state.
fn materialize(base: &HashMap<Vec<u8>, Vec<u8>>, write_set: WriteSet) -> HashMap<Vec<u8>, Vec<u8>> {
	let mut state = base.clone();
	for (begin, end) in &write_set.range_deletes {
		state.retain(|key, _| {
			!(key.as_slice() >= begin.as_slice() && key.as_slice() < end.as_slice())
		});
	}
	for key in &write_set.point_deletes {
		state.remove(key);
	}
	for (key, value) in write_set.upserts {
		state.insert(key, value);
	}
	state
}

/// Reference oracle: apply each winner's operations one at a time directly to a working state, the
/// exact serial semantics the fold must reproduce.
fn reference(base: &HashMap<Vec<u8>, Vec<u8>>, winners: &[Winner]) -> HashMap<Vec<u8>, Vec<u8>> {
	let mut state = base.clone();
	for w in winners {
		let mut counter: u16 = 0;
		for op in &w.operations {
			match op {
				Operation::SetValue { key, value } => {
					state.insert(key.clone(), value.clone());
				}
				Operation::Clear { key } => {
					state.remove(key);
				}
				Operation::ClearRange { begin, end } => {
					state.retain(|key, _| {
						!(key.as_slice() >= begin.as_slice() && key.as_slice() < end.as_slice())
					});
				}
				Operation::AtomicOp {
					key,
					param,
					op_type,
				} => match op_type {
					MutationType::SetVersionstampedKey => {
						let vs = build_versionstamp(w.commit_version, &mut counter);
						let new_key = substitute_raw_versionstamp(key.clone(), &vs).unwrap();
						state.insert(new_key, param.clone());
					}
					MutationType::SetVersionstampedValue => {
						let vs = build_versionstamp(w.commit_version, &mut counter);
						let new_value = substitute_raw_versionstamp(param.clone(), &vs).unwrap();
						state.insert(key.clone(), new_value);
					}
					_ => {
						let current = state.get(key).map(|v| v.as_slice());
						match apply_atomic_op(current, param, *op_type) {
							Some(v) => {
								state.insert(key.clone(), v);
							}
							None => {
								state.remove(key);
							}
						}
					}
				},
			}
		}
	}
	state
}

/// Build a 4-byte-offset-trailed buffer that `substitute_raw_versionstamp` can stamp at `offset`.
fn stampable(prefix: &[u8], offset: u32) -> Vec<u8> {
	let mut buf = Vec::new();
	buf.extend_from_slice(prefix);
	buf.extend_from_slice(&[0u8; 10]);
	buf.extend_from_slice(&offset.to_le_bytes());
	buf
}

/// `fold_winners` consumes its input, so clone for tests that also run the oracle on the same data.
fn fold_winners_clone(winners: &[Winner], base: &HashMap<Vec<u8>, Vec<u8>>) -> WriteSet {
	let cloned: Vec<Winner> = winners
		.iter()
		.map(|w| Winner {
			commit_version: w.commit_version,
			operations: w.operations.clone(),
		})
		.collect();
	fold_winners(cloned, base).unwrap()
}

#[test]
fn same_key_set_later_id_wins() {
	let winners = vec![
		winner(1, vec![set("a", "first")]),
		winner(2, vec![set("a", "second")]),
	];
	let base = HashMap::new();
	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	assert_eq!(
		state.get(&k("a")).map(|v| v.as_slice()),
		Some(b"second".as_slice())
	);
	assert_eq!(state, reference(&base, &winners));
}

#[test]
fn two_adds_same_key_fold_sequentially() {
	// 5 -> 6 -> 7 across two commits in one batch.
	let mut base = HashMap::new();
	base.insert(k("n"), add_param(5));
	let winners = vec![winner(1, vec![add("n", 1)]), winner(2, vec![add("n", 1)])];
	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	assert_eq!(read_int(state.get(&k("n")).unwrap()), 7);
	assert_eq!(state, reference(&base, &winners));
}

#[test]
fn set_then_add_atomic_sees_the_set() {
	let base = HashMap::new();
	let winners = vec![
		winner(1, vec![set("n", "\x0a\0\0\0\0\0\0\0")]),
		winner(2, vec![add("n", 1)]),
	];
	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	assert_eq!(read_int(state.get(&k("n")).unwrap()), 11);
	assert_eq!(state, reference(&base, &winners));
}

#[test]
fn clear_range_then_add_sees_none() {
	let mut base = HashMap::new();
	base.insert(k("r/x"), add_param(100));
	let winners = vec![
		winner(1, vec![clear_range("r/", "r0")]),
		winner(2, vec![add("r/x", 1)]),
	];
	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	// The range clear wiped the base 100, so the add starts from absent/0 -> 1.
	assert_eq!(read_int(state.get(&k("r/x")).unwrap()), 1);
	assert_eq!(state, reference(&base, &winners));
}

#[test]
fn set_inside_cleared_range_is_reinserted() {
	let mut base = HashMap::new();
	base.insert(k("r/x"), k("old"));
	let winners = vec![winner(1, vec![clear_range("r/", "r0"), set("r/x", "new")])];
	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	assert_eq!(
		state.get(&k("r/x")).map(|v| v.as_slice()),
		Some(b"new".as_slice())
	);
	assert_eq!(state, reference(&base, &winners));
}

#[test]
fn versionstamped_key_and_value_distinct_stamps() {
	let base = HashMap::new();
	let winners = vec![winner(
		42,
		vec![
			Operation::AtomicOp {
				key: stampable(b"vk/", 3),
				param: k("v1"),
				op_type: MutationType::SetVersionstampedKey,
			},
			Operation::AtomicOp {
				key: k("vv/"),
				param: stampable(b"", 0),
				op_type: MutationType::SetVersionstampedValue,
			},
		],
	)];
	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	assert_eq!(state, reference(&base, &winners));

	// The versionstamped key embeds commit_version 42 with per-commit counter 0.
	let stamped_key: Vec<u8> = {
		let mut key = b"vk/".to_vec();
		key.extend_from_slice(&42u64.to_be_bytes());
		key.extend_from_slice(&0u16.to_be_bytes());
		key
	};
	assert_eq!(
		state.get(&stamped_key).map(|v| v.as_slice()),
		Some(b"v1".as_slice())
	);

	// The versionstamped value uses the same commit_version but the next counter (1).
	let stamped_value = state.get(&k("vv/")).unwrap();
	assert_eq!(&stamped_value[0..8], &42u64.to_be_bytes());
	assert_eq!(&stamped_value[8..10], &1u16.to_be_bytes());
}

#[test]
fn point_delete_and_upsert_disjoint() {
	let mut base = HashMap::new();
	base.insert(k("keep"), k("base"));
	base.insert(k("drop"), k("base"));
	let winners = vec![winner(1, vec![clear("drop"), set("keep", "new")])];
	let ws = fold_winners_clone(&winners, &base);
	assert_eq!(ws.point_deletes, vec![k("drop")]);
	assert_eq!(ws.upserts, vec![(k("keep"), k("new"))]);
	let state = materialize(&base, ws);
	assert_eq!(state, reference(&base, &winners));
}

/// A multi-op, multi-winner batch mixing every operation kind must equal the serial oracle.
#[test]
fn mixed_batch_matches_oracle() {
	let mut base = HashMap::new();
	base.insert(k("c1"), add_param(10));
	base.insert(k("c2"), add_param(20));
	base.insert(k("old"), k("x"));
	base.insert(k("range/a"), k("ra"));
	base.insert(k("range/b"), k("rb"));

	let winners = vec![
		winner(1, vec![set("c1", "\x01\0\0\0\0\0\0\0"), add("c1", 4)]),
		winner(2, vec![clear("old"), add("c2", 5)]),
		winner(
			3,
			vec![clear_range("range/", "range0"), set("range/a", "back")],
		),
		winner(4, vec![add("c2", 100), add("c1", 1)]),
	];

	let ws = fold_winners_clone(&winners, &base);
	let state = materialize(&base, ws);
	assert_eq!(state, reference(&base, &winners));

	// Spot checks of the folded result.
	assert_eq!(read_int(state.get(&k("c1")).unwrap()), 6); // 1 +4 +1
	assert_eq!(read_int(state.get(&k("c2")).unwrap()), 125); // 20 +5 +100
	assert!(state.get(&k("old")).is_none());
	assert_eq!(
		state.get(&k("range/a")).map(|v| v.as_slice()),
		Some(b"back".as_slice())
	);
	assert!(state.get(&k("range/b")).is_none());
}
