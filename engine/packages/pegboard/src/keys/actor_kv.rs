use anyhow::Result;
use gas::prelude::*;
use rivet_envoy_protocol as ep;
use universaldb::prelude::*;

pub fn subspace(actor_id: Id) -> universaldb::utils::Subspace {
	universaldb::utils::Subspace::new(&(RIVET, PEGBOARD, ACTOR_KV, actor_id))
}

/// Wraps a key with a trailing NIL byte for exact key matching.
///
/// Encodes as: `[NESTED, ...bytes..., NIL]`
///
/// Use this for:
/// - Storing keys
/// - Getting/deleting specific keys
/// - Range query end points (to create closed boundaries)
#[derive(Debug, Clone, PartialEq)]
pub struct KeyWrapper(pub ep::KvKey);

impl KeyWrapper {
	pub fn tuple_len(key: &ep::KvKey) -> usize {
		key.len() + 2
	}
}

impl TuplePack for KeyWrapper {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let mut offset = VersionstampOffset::None { size: 0 };

		w.write_all(&[universaldb::utils::codes::NESTED])?;
		offset += 1;

		offset += self.0.pack(w, tuple_depth.increment())?;

		w.write_all(&[universaldb::utils::codes::NIL])?;
		offset += 1;

		Ok(offset)
	}
}

impl<'de> TupleUnpack<'de> for KeyWrapper {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let input = universaldb::utils::parse_code(input, universaldb::utils::codes::NESTED)?;

		let (input, inner) = foundationdb_tuple::Bytes::unpack(input, tuple_depth.increment())?;

		let input = universaldb::utils::parse_code(input, universaldb::utils::codes::NIL)?;

		Ok((input, KeyWrapper(inner.into_owned())))
	}
}

/// Wraps a key without a trailing NIL byte for prefix/range matching.
///
/// Encodes as: `[NESTED, ...bytes...]` (no trailing NIL)
///
/// Use this for:
/// - Range query start points (to create open boundaries)
/// - Prefix queries (to match all keys starting with these bytes)
pub struct ListKeyWrapper(pub ep::KvKey);

impl TuplePack for ListKeyWrapper {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let mut offset = VersionstampOffset::None { size: 0 };

		w.write_all(&[universaldb::utils::codes::NESTED])?;
		offset += 1;

		offset += self.0.pack(w, tuple_depth.increment())?;

		// No ending NIL byte compared to `KeyWrapper::pack`

		Ok(offset)
	}
}

// Parses key in first position, ignores the rest
pub struct EntryBaseKey {
	pub key: KeyWrapper,
}

impl<'de> TupleUnpack<'de> for EntryBaseKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, key) = <KeyWrapper>::unpack(input, tuple_depth)?;
		let v = EntryBaseKey { key };

		Ok((&input[0..0], v))
	}
}

pub struct EntryValueChunkKey {
	key: KeyWrapper,
	pub chunk: usize,
}

impl EntryValueChunkKey {
	pub fn new(key: KeyWrapper, chunk: usize) -> Self {
		EntryValueChunkKey { key, chunk }
	}
}

impl TuplePack for EntryValueChunkKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (&self.key, DATA, self.chunk);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for EntryValueChunkKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (key, data, chunk)) = <(KeyWrapper, usize, usize)>::unpack(input, tuple_depth)?;
		if data != DATA {
			return Err(PackError::Message("expected DATA data".into()));
		}

		let v = EntryValueChunkKey { key, chunk };

		Ok((input, v))
	}
}

#[derive(Debug)]
pub struct EntryMetadataKey {
	pub key: KeyWrapper,
}

impl EntryMetadataKey {
	pub fn new(key: KeyWrapper) -> Self {
		EntryMetadataKey { key }
	}
}

impl FormalKey for EntryMetadataKey {
	type Value = ep::KvMetadata;

	// TODO: this is mistakenly not versioned. Transition to vbare so future
	// changes to KvMetadata don't require hand-rolled LegacyXxx fallbacks.
	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		rivet_util::serde::bare_from_slice!(raw).map_err(Into::into)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		rivet_util::serde::bare_to_vec!(&value).map_err(Into::into)
	}
}

impl TuplePack for EntryMetadataKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (&self.key, METADATA);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for EntryMetadataKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (key, data)) = <(KeyWrapper, usize)>::unpack(input, tuple_depth)?;
		if data != METADATA {
			return Err(PackError::Message("expected METADATA data".into()));
		}

		let v = EntryMetadataKey { key };

		Ok((input, v))
	}
}
