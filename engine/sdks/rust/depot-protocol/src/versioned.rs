use anyhow::{Context, Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::v1;

pub enum DBHead {
	V1(v1::DBHead),
}

impl OwnedVersionedData for DBHead {
	type Latest = v1::DBHead;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid depot db head version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

/// Encode the latest `DBHead` with an embedded 2-byte version prefix.
pub fn encode_db_head(head: v1::DBHead) -> Result<Vec<u8>> {
	DBHead::wrap_latest(head)
		.serialize_with_embedded_version(crate::SQLITE_STORAGE_PROTOCOL_VERSION)
		.context("encode sqlite db head")
}

/// Decode a versioned `DBHead` payload into the latest schema variant.
pub fn decode_db_head(payload: &[u8]) -> Result<v1::DBHead> {
	DBHead::deserialize_with_embedded_version(payload).context("decode sqlite db head")
}
