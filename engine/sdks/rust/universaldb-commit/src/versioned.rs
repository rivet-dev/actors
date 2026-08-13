use anyhow::{Ok, Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::v1;

// Only v1 exists today. When adding v2+, generate converters with
// `scripts/vbare-gen-converters` (see the envoy-protocol package for the
// resulting `versioned/` module layout) and wire them in here.
pub enum CommitRequest {
	V1(v1::CommitRequest),
}

impl OwnedVersionedData for CommitRequest {
	type Latest = v1::CommitRequest;

	fn wrap_latest(latest: v1::CommitRequest) -> Self {
		CommitRequest::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			CommitRequest::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(CommitRequest::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			CommitRequest::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

pub enum CommitReply {
	V1(v1::CommitReply),
}

impl OwnedVersionedData for CommitReply {
	type Latest = v1::CommitReply;

	fn wrap_latest(latest: v1::CommitReply) -> Self {
		CommitReply::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			CommitReply::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(CommitReply::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			CommitReply::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

pub enum Watermark {
	V1(v1::Watermark),
}

impl OwnedVersionedData for Watermark {
	type Latest = v1::Watermark;

	fn wrap_latest(latest: v1::Watermark) -> Self {
		Watermark::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Watermark::V1(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Watermark::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Watermark::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}
