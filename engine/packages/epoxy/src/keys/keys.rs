use anyhow::Result;
use epoxy_protocol::{PROTOCOL_VERSION, protocol, versioned};
use serde::{Deserialize, Serialize};
use universaldb::prelude::*;
use universaldb::tuple::Versionstamp;
use vbare::OwnedVersionedData;

/// In-flight accepted proposal state stored under `kv/{key}/accepted`.
///
/// This uses raw `serde_bare` serialization rather than the versioned protocol path because
/// accepted state is transient. It is cleared on every commit and never survives a full
/// consensus round, so forward-compatible deserialization is not needed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KvAcceptedValue {
	pub value: Vec<u8>,
	pub ballot: protocol::Ballot,
	#[serde(default)]
	pub version: u64,
	#[serde(default)]
	pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct KvValueKey {
	key: Vec<u8>,
}

impl KvValueKey {
	pub fn new(key: Vec<u8>) -> Self {
		Self { key }
	}

	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl FormalKey for KvValueKey {
	type Value = protocol::CommittedValue;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		versioned::CommittedValue::deserialize_with_embedded_version(raw)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		versioned::CommittedValue::wrap_latest(value)
			.serialize_with_embedded_version(PROTOCOL_VERSION)
	}
}

impl TuplePack for KvValueKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (KV, &self.key, VALUE);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for KvValueKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, key, leaf)) = <(usize, Vec<u8>, usize)>::unpack(input, tuple_depth)?;
		if root != KV {
			return Err(PackError::Message("expected KV root".into()));
		}
		if leaf != VALUE {
			return Err(PackError::Message("expected VALUE leaf".into()));
		}

		let v = KvValueKey { key };

		Ok((input, v))
	}
}

#[derive(Debug, Clone)]
pub struct LegacyCommittedValueKey {
	key: Vec<u8>,
}

impl LegacyCommittedValueKey {
	pub fn new(key: Vec<u8>) -> Self {
		Self { key }
	}

	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl FormalKey for LegacyCommittedValueKey {
	type Value = Vec<u8>;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		Ok(raw.to_vec())
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		Ok(value)
	}
}

impl TuplePack for LegacyCommittedValueKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (KV, &self.key, COMMITTED_VALUE);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for LegacyCommittedValueKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, key, leaf)) = <(usize, Vec<u8>, usize)>::unpack(input, tuple_depth)?;
		if root != KV {
			return Err(PackError::Message("expected KV root".into()));
		}
		if leaf != COMMITTED_VALUE {
			return Err(PackError::Message("expected COMMITTED_VALUE leaf".into()));
		}

		let v = LegacyCommittedValueKey { key };

		Ok((input, v))
	}
}

#[derive(Debug, Clone)]
pub struct KvBallotKey {
	key: Vec<u8>,
}

impl KvBallotKey {
	pub fn new(key: Vec<u8>) -> Self {
		Self { key }
	}

	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl FormalKey for KvBallotKey {
	type Value = protocol::Ballot;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		rivet_util::serde::bare_from_slice!(raw).map_err(Into::into)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		rivet_util::serde::bare_to_vec!(&value).map_err(Into::into)
	}
}

impl TuplePack for KvBallotKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (KV, &self.key, BALLOT);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for KvBallotKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, key, leaf)) = <(usize, Vec<u8>, usize)>::unpack(input, tuple_depth)?;
		if root != KV {
			return Err(PackError::Message("expected KV root".into()));
		}
		if leaf != BALLOT {
			return Err(PackError::Message("expected BALLOT leaf".into()));
		}

		let v = KvBallotKey { key };

		Ok((input, v))
	}
}

/// Deprecated. Use KvAccepted2Key.
#[derive(Debug, Clone)]
pub struct KvAcceptedKey {
	key: Vec<u8>,
}

impl KvAcceptedKey {
	pub fn new(key: Vec<u8>) -> Self {
		Self { key }
	}

	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl FormalKey for KvAcceptedKey {
	type Value = KvAcceptedValue;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		rivet_util::serde::bare_from_slice!(raw).map_err(Into::into)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		rivet_util::serde::bare_to_vec!(&value).map_err(Into::into)
	}
}

impl TuplePack for KvAcceptedKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (KV, &self.key, ACCEPTED);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for KvAcceptedKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, key, leaf)) = <(usize, Vec<u8>, usize)>::unpack(input, tuple_depth)?;
		if root != KV {
			return Err(PackError::Message("expected KV root".into()));
		}
		if leaf != ACCEPTED {
			return Err(PackError::Message("expected ACCEPTED leaf".into()));
		}

		let v = KvAcceptedKey { key };

		Ok((input, v))
	}
}

#[derive(Debug, Clone)]
pub struct KvAccepted2Key {
	key: Vec<u8>,
}

impl KvAccepted2Key {
	pub fn new(key: Vec<u8>) -> Self {
		Self { key }
	}

	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl FormalKey for KvAccepted2Key {
	type Value = protocol::AcceptedValue;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		versioned::AcceptedValue::deserialize_with_embedded_version(raw)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		versioned::AcceptedValue::wrap_latest(value)
			.serialize_with_embedded_version(PROTOCOL_VERSION)
	}
}

impl TuplePack for KvAccepted2Key {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (KV, &self.key, ACCEPTED2);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for KvAccepted2Key {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, key, leaf)) = <(usize, Vec<u8>, usize)>::unpack(input, tuple_depth)?;
		if root != KV {
			return Err(PackError::Message("expected KV root".into()));
		}
		if leaf != ACCEPTED2 {
			return Err(PackError::Message("expected ACCEPTED2 leaf".into()));
		}

		let v = KvAccepted2Key { key };

		Ok((input, v))
	}
}

#[derive(Debug, Clone)]
pub struct KvOptimisticCacheKey {
	key: Vec<u8>,
}

impl KvOptimisticCacheKey {
	pub fn new(key: Vec<u8>) -> Self {
		Self { key }
	}

	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl FormalKey for KvOptimisticCacheKey {
	type Value = protocol::CachedValue;

	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		versioned::CachedValue::deserialize_with_embedded_version(raw)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		versioned::CachedValue::wrap_latest(value).serialize_with_embedded_version(PROTOCOL_VERSION)
	}
}

impl TuplePack for KvOptimisticCacheKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (KV, &self.key, CACHE);
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for KvOptimisticCacheKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, key, leaf)) = <(usize, Vec<u8>, usize)>::unpack(input, tuple_depth)?;
		if root != KV {
			return Err(PackError::Message("expected KV root".into()));
		}
		if leaf != CACHE {
			return Err(PackError::Message("expected CACHE leaf".into()));
		}

		let v = KvOptimisticCacheKey { key };

		Ok((input, v))
	}
}

#[derive(Debug, Clone)]
pub struct ChangelogKey {
	versionstamp: Versionstamp,
}

impl ChangelogKey {
	pub fn new(versionstamp: Versionstamp) -> Self {
		Self { versionstamp }
	}

	pub fn versionstamp(&self) -> &Versionstamp {
		&self.versionstamp
	}
}

impl FormalKey for ChangelogKey {
	type Value = protocol::ChangelogEntry;

	// TODO: this is mistakenly not versioned. Transition to vbare so future
	// changes to ChangelogEntry don't require hand-rolled LegacyXxx fallbacks.
	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		rivet_util::serde::bare_from_slice!(raw).map_err(Into::into)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		rivet_util::serde::bare_to_vec!(&value).map_err(Into::into)
	}
}

impl TuplePack for ChangelogKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		let t = (CHANGELOG, self.versionstamp.clone());
		t.pack(w, tuple_depth)
	}
}

impl<'de> TupleUnpack<'de> for ChangelogKey {
	fn unpack(input: &[u8], tuple_depth: TupleDepth) -> PackResult<(&[u8], Self)> {
		let (input, (root, versionstamp)) = <(usize, Versionstamp)>::unpack(input, tuple_depth)?;
		if root != CHANGELOG {
			return Err(PackError::Message("expected CHANGELOG root".into()));
		}

		let v = ChangelogKey { versionstamp };

		Ok((input, v))
	}
}
