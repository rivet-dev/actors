use anyhow::{Ok, Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::{v1, v2, v3};

pub enum UpsMessage {
	V1(v1::UpsMessage),
	V2(v2::UpsMessage),
	V3(v3::UpsMessage),
}

impl OwnedVersionedData for UpsMessage {
	type Latest = v3::UpsMessage;

	fn wrap_latest(latest: v3::UpsMessage) -> Self {
		UpsMessage::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let UpsMessage::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(UpsMessage::V1(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			2 => Ok(UpsMessage::V2(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			3 => Ok(UpsMessage::V3(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			UpsMessage::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			UpsMessage::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			UpsMessage::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Self::v2_to_v1]
	}
}

impl UpsMessage {
	fn v1_to_v2(self) -> Result<Self> {
		let UpsMessage::V1(v1::UpsMessage { body }) = self else {
			bail!("expected v1");
		};

		let body = match body {
			v1::MessageBody::MessageStart(v1::MessageStart {
				message_id,
				chunk_count,
				reply_subject,
				payload,
			}) => v2::MessageBody::MessageStart(v2::MessageStart {
				message_id,
				chunk_count,
				reply_subject,
				request_deadline_at: None,
				payload,
			}),
			v1::MessageBody::MessageChunk(v1::MessageChunk {
				message_id,
				chunk_index,
				payload,
			}) => v2::MessageBody::MessageChunk(v2::MessageChunk {
				message_id,
				chunk_index,
				payload,
			}),
		};

		Ok(UpsMessage::V2(v2::UpsMessage { body }))
	}

	fn v2_to_v3(self) -> Result<Self> {
		let UpsMessage::V2(v2::UpsMessage { body }) = self else {
			bail!("expected v2");
		};

		let body = match body {
			v2::MessageBody::MessageStart(v2::MessageStart {
				message_id,
				chunk_count,
				reply_subject,
				request_deadline_at,
				payload,
			}) => v3::MessageBody::MessageStart(v3::MessageStart {
				message_id,
				chunk_count,
				timestamp: 0,
				reply_subject,
				request_deadline_at,
				payload,
			}),
			v2::MessageBody::MessageChunk(v2::MessageChunk {
				message_id,
				chunk_index,
				payload,
			}) => v3::MessageBody::MessageChunk(v3::MessageChunk {
				message_id,
				chunk_index,
				timestamp: 0,
				payload,
			}),
		};

		Ok(UpsMessage::V3(v3::UpsMessage { body }))
	}

	fn v3_to_v2(self) -> Result<Self> {
		let UpsMessage::V3(v3::UpsMessage { body }) = self else {
			bail!("expected v3");
		};

		let body = match body {
			v3::MessageBody::MessageStart(v3::MessageStart {
				message_id,
				chunk_count,
				timestamp: _,
				reply_subject,
				request_deadline_at,
				payload,
			}) => v2::MessageBody::MessageStart(v2::MessageStart {
				message_id,
				chunk_count,
				reply_subject,
				request_deadline_at,
				payload,
			}),
			v3::MessageBody::MessageChunk(v3::MessageChunk {
				message_id,
				chunk_index,
				timestamp: _,
				payload,
			}) => v2::MessageBody::MessageChunk(v2::MessageChunk {
				message_id,
				chunk_index,
				payload,
			}),
		};

		Ok(UpsMessage::V2(v2::UpsMessage { body }))
	}

	fn v2_to_v1(self) -> Result<Self> {
		let UpsMessage::V2(v2::UpsMessage { body }) = self else {
			bail!("expected v2");
		};

		let body = match body {
			v2::MessageBody::MessageStart(v2::MessageStart {
				message_id,
				chunk_count,
				reply_subject,
				request_deadline_at: _,
				payload,
			}) => v1::MessageBody::MessageStart(v1::MessageStart {
				message_id,
				chunk_count,
				reply_subject,
				payload,
			}),
			v2::MessageBody::MessageChunk(v2::MessageChunk {
				message_id,
				chunk_index,
				payload,
			}) => v1::MessageBody::MessageChunk(v1::MessageChunk {
				message_id,
				chunk_index,
				payload,
			}),
		};

		Ok(UpsMessage::V1(v1::UpsMessage { body }))
	}
}
