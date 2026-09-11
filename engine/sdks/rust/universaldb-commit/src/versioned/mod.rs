use anyhow::{Ok, Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::{v1, v2};

mod v1_to_v2;
mod v2_to_v1;

pub enum CommitRequest {
	V1(v1::CommitRequest),
	V2(v2::CommitRequest),
}

impl OwnedVersionedData for CommitRequest {
	type Latest = v2::CommitRequest;

	fn wrap_latest(latest: v2::CommitRequest) -> Self {
		CommitRequest::V2(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			CommitRequest::V2(data) => Ok(data),
			CommitRequest::V1(_) => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(CommitRequest::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(CommitRequest::V2(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			CommitRequest::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
			CommitRequest::V2(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v2_to_v1]
	}
}

impl CommitRequest {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			CommitRequest::V1(data) => Ok(CommitRequest::V2(
				v1_to_v2::convert_commit_request_v1_to_v2(data)?,
			)),
			CommitRequest::V2(_) => bail!("unexpected version"),
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		match self {
			CommitRequest::V2(data) => Ok(CommitRequest::V1(
				v2_to_v1::convert_commit_request_v2_to_v1(data)?,
			)),
			CommitRequest::V1(_) => bail!("unexpected version"),
		}
	}
}

/// Chunked commit requests were introduced in v2, so there is no earlier version to convert from.
/// The identity converters make vbare count v2 as this type's latest version.
pub enum CommitRequestChunk {
	V2(v2::CommitRequestChunk),
}

impl OwnedVersionedData for CommitRequestChunk {
	type Latest = v2::CommitRequestChunk;

	fn wrap_latest(latest: v2::CommitRequestChunk) -> Self {
		CommitRequestChunk::V2(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			CommitRequestChunk::V2(data) => Ok(data),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			2 => Ok(CommitRequestChunk::V2(serde_bare::from_slice(payload)?)),
			_ => bail!("commit request chunks do not exist at version {version}"),
		}
	}

	fn serialize_version(self, version: u16) -> Result<Vec<u8>> {
		match (self, version) {
			(CommitRequestChunk::V2(data), 2) => serde_bare::to_vec(&data).map_err(Into::into),
			(CommitRequestChunk::V2(_), _) => {
				bail!("commit request chunks do not exist at version {version}")
			}
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok]
	}
}

pub enum CommitReply {
	V1(v1::CommitReply),
	V2(v2::CommitReply),
}

impl OwnedVersionedData for CommitReply {
	type Latest = v2::CommitReply;

	fn wrap_latest(latest: v2::CommitReply) -> Self {
		CommitReply::V2(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			CommitReply::V2(data) => Ok(data),
			CommitReply::V1(_) => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(CommitReply::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(CommitReply::V2(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			CommitReply::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
			CommitReply::V2(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v2_to_v1]
	}
}

impl CommitReply {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			CommitReply::V1(data) => Ok(CommitReply::V2(v1_to_v2::convert_commit_reply_v1_to_v2(
				data,
			)?)),
			CommitReply::V2(_) => bail!("unexpected version"),
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		match self {
			CommitReply::V2(data) => Ok(CommitReply::V1(v2_to_v1::convert_commit_reply_v2_to_v1(
				data,
			)?)),
			CommitReply::V1(_) => bail!("unexpected version"),
		}
	}
}

pub enum Watermark {
	V1(v1::Watermark),
	V2(v2::Watermark),
}

impl OwnedVersionedData for Watermark {
	type Latest = v2::Watermark;

	fn wrap_latest(latest: v2::Watermark) -> Self {
		Watermark::V2(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Watermark::V2(data) => Ok(data),
			Watermark::V1(_) => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Watermark::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Watermark::V2(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Watermark::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
			Watermark::V2(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v2_to_v1]
	}
}

impl Watermark {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Watermark::V1(data) => Ok(Watermark::V2(v1_to_v2::convert_watermark_v1_to_v2(data)?)),
			Watermark::V2(_) => bail!("unexpected version"),
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Watermark::V2(data) => Ok(Watermark::V1(v2_to_v1::convert_watermark_v2_to_v1(data)?)),
			Watermark::V1(_) => bail!("unexpected version"),
		}
	}
}
