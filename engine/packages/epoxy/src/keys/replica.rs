use anyhow::*;
use epoxy_protocol::protocol;
use universaldb::prelude::*;

#[derive(Debug)]
pub struct ConfigKey;

impl FormalKey for ConfigKey {
	type Value = protocol::ClusterConfig;

	// TODO: this is mistakenly not versioned. Transition to vbare so future
	// changes to ClusterConfig don't require hand-rolled LegacyXxx fallbacks.
	fn deserialize(&self, raw: &[u8]) -> Result<Self::Value> {
		rivet_util::serde::bare_from_slice!(raw).map_err(Into::into)
	}

	fn serialize(&self, value: Self::Value) -> Result<Vec<u8>> {
		rivet_util::serde::bare_to_vec!(&value).map_err(Into::into)
	}
}

impl TuplePack for ConfigKey {
	fn pack<W: std::io::Write>(
		&self,
		w: &mut W,
		tuple_depth: TupleDepth,
	) -> std::io::Result<VersionstampOffset> {
		(CONFIG,).pack(w, tuple_depth)
	}
}
