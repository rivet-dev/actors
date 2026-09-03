use anyhow::{Ok, Result, bail};
use gas::prelude::Id;
use vbare::OwnedVersionedData;

use crate::converted;
use crate::generated::*;

mod namespace_runner_config;

pub use namespace_runner_config::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerAllocIdxKeyDataV1 {
	pub workflow_id: Id,
	pub remaining_slots: u32,
	pub total_slots: u32,
}

impl TryFrom<pegboard_namespace_runner_alloc_idx_v1::Data> for RunnerAllocIdxKeyDataV1 {
	type Error = anyhow::Error;

	fn try_from(value: pegboard_namespace_runner_alloc_idx_v1::Data) -> Result<Self> {
		Ok(RunnerAllocIdxKeyDataV1 {
			workflow_id: Id::from_slice(&value.workflow_id)?,
			remaining_slots: value.remaining_slots,
			total_slots: value.total_slots,
		})
	}
}

impl From<RunnerAllocIdxKeyDataV1> for pegboard_namespace_runner_alloc_idx_v1::Data {
	fn from(value: RunnerAllocIdxKeyDataV1) -> Self {
		pegboard_namespace_runner_alloc_idx_v1::Data {
			workflow_id: value.workflow_id.as_bytes(),
			remaining_slots: value.remaining_slots,
			total_slots: value.total_slots,
		}
	}
}

pub enum RunnerAllocIdxKeyData {
	V1(RunnerAllocIdxKeyDataV1),
	V2(converted::RunnerAllocIdxKeyData),
}

impl OwnedVersionedData for RunnerAllocIdxKeyData {
	type Latest = converted::RunnerAllocIdxKeyData;

	fn wrap_latest(latest: converted::RunnerAllocIdxKeyData) -> Self {
		RunnerAllocIdxKeyData::V2(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let RunnerAllocIdxKeyData::V2(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(RunnerAllocIdxKeyData::V1(
				serde_bare::from_slice::<pegboard_namespace_runner_alloc_idx_v1::Data>(payload)?
					.try_into()?,
			)),
			2 => Ok(RunnerAllocIdxKeyData::V2(
				serde_bare::from_slice::<pegboard_namespace_runner_alloc_idx_v2::Data>(payload)?
					.try_into()?,
			)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			RunnerAllocIdxKeyData::V1(data) => {
				let data: pegboard_namespace_runner_alloc_idx_v1::Data = data.into();
				serde_bare::to_vec(&data).map_err(Into::into)
			}
			RunnerAllocIdxKeyData::V2(data) => {
				let data: pegboard_namespace_runner_alloc_idx_v2::Data = data.try_into()?;
				serde_bare::to_vec(&data).map_err(Into::into)
			}
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v2_to_v1]
	}
}

impl RunnerAllocIdxKeyData {
	fn v1_to_v2(self) -> Result<Self> {
		if let RunnerAllocIdxKeyData::V1(x) = self {
			Ok(RunnerAllocIdxKeyData::V2(
				converted::RunnerAllocIdxKeyData {
					workflow_id: x.workflow_id,
					remaining_slots: x.remaining_slots,
					total_slots: x.total_slots,
					// Default to mk1
					protocol_version: rivet_runner_protocol::PROTOCOL_MK1_VERSION,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		if let RunnerAllocIdxKeyData::V2(x) = self {
			Ok(RunnerAllocIdxKeyData::V1(RunnerAllocIdxKeyDataV1 {
				workflow_id: x.workflow_id,
				remaining_slots: x.remaining_slots,
				total_slots: x.total_slots,
			}))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum WebhookConfigData {
	V1(webhook_config_v1::Data),
}

impl OwnedVersionedData for WebhookConfigData {
	type Latest = webhook_config_v1::Data;

	fn wrap_latest(latest: webhook_config_v1::Data) -> Self {
		WebhookConfigData::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let WebhookConfigData::V1(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(WebhookConfigData::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			WebhookConfigData::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

pub enum WebhookDeliveryData {
	V1(webhook_delivery_v1::Data),
}

impl OwnedVersionedData for WebhookDeliveryData {
	type Latest = webhook_delivery_v1::Data;

	fn wrap_latest(latest: webhook_delivery_v1::Data) -> Self {
		WebhookDeliveryData::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let WebhookDeliveryData::V1(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(WebhookDeliveryData::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			WebhookDeliveryData::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

pub enum MetadataKeyData {
	V1(pegboard_runner_metadata_v1::Data),
}

impl OwnedVersionedData for MetadataKeyData {
	type Latest = pegboard_runner_metadata_v1::Data;

	fn wrap_latest(latest: pegboard_runner_metadata_v1::Data) -> Self {
		MetadataKeyData::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let MetadataKeyData::V1(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(MetadataKeyData::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			MetadataKeyData::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

pub enum ActorByKeyKeyData {
	V1(converted::ActorByKeyKeyData),
}

impl OwnedVersionedData for ActorByKeyKeyData {
	type Latest = converted::ActorByKeyKeyData;

	fn wrap_latest(latest: converted::ActorByKeyKeyData) -> Self {
		ActorByKeyKeyData::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let ActorByKeyKeyData::V1(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(ActorByKeyKeyData::V1(
				serde_bare::from_slice::<pegboard_namespace_actor_by_key_v1::Data>(payload)?
					.try_into()?,
			)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ActorByKeyKeyData::V1(data) => {
				let data: pegboard_namespace_actor_by_key_v1::Data = data.try_into()?;
				serde_bare::to_vec(&data).map_err(Into::into)
			}
		}
	}
}

pub enum RunnerByKeyKeyData {
	V1(converted::RunnerByKeyKeyData),
}

impl OwnedVersionedData for RunnerByKeyKeyData {
	type Latest = converted::RunnerByKeyKeyData;

	fn wrap_latest(latest: converted::RunnerByKeyKeyData) -> Self {
		RunnerByKeyKeyData::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let RunnerByKeyKeyData::V1(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(RunnerByKeyKeyData::V1(
				serde_bare::from_slice::<pegboard_namespace_runner_by_key_v1::Data>(payload)?
					.try_into()?,
			)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			RunnerByKeyKeyData::V1(data) => {
				let data: pegboard_namespace_runner_by_key_v1::Data = data.try_into()?;
				serde_bare::to_vec(&data).map_err(Into::into)
			}
		}
	}
}

pub enum ActorNameKeyData {
	V1(pegboard_namespace_actor_name_v1::Data),
}

impl OwnedVersionedData for ActorNameKeyData {
	type Latest = pegboard_namespace_actor_name_v1::Data;

	fn wrap_latest(latest: pegboard_namespace_actor_name_v1::Data) -> Self {
		ActorNameKeyData::V1(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let ActorNameKeyData::V1(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(ActorNameKeyData::V1(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ActorNameKeyData::V1(data) => serde_bare::to_vec(&data).map_err(Into::into),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gas::prelude::Uuid;

	fn test_id(value: u128, label: u16) -> Id {
		Id::v1(Uuid::from_u128(value), label)
	}

	#[test]
	fn runner_alloc_idx_ids_round_trip_as_native_id_without_wire_change() {
		let workflow_id = test_id(0x11111111111111111111111111111111, 42);
		let typed = converted::RunnerAllocIdxKeyData {
			workflow_id,
			remaining_slots: 7,
			total_slots: 11,
			protocol_version: 6,
		};

		let expected_latest = serde_bare::to_vec(&pegboard_namespace_runner_alloc_idx_v2::Data {
			workflow_id: workflow_id.as_bytes(),
			remaining_slots: 7,
			total_slots: 11,
			protocol_version: 6,
		})
		.expect("generated latest data should encode");
		let encoded_latest = RunnerAllocIdxKeyData::wrap_latest(typed.clone())
			.serialize(2)
			.expect("typed latest data should encode");

		assert_eq!(encoded_latest, expected_latest);
		assert_eq!(
			RunnerAllocIdxKeyData::deserialize(&encoded_latest, 2)
				.expect("typed latest data should decode"),
			typed
		);

		let expected_v1 = serde_bare::to_vec(&pegboard_namespace_runner_alloc_idx_v1::Data {
			workflow_id: workflow_id.as_bytes(),
			remaining_slots: 7,
			total_slots: 11,
		})
		.expect("generated v1 data should encode");
		let encoded_v1 = RunnerAllocIdxKeyData::wrap_latest(typed)
			.serialize(1)
			.expect("typed v1 data should encode");

		assert_eq!(encoded_v1, expected_v1);
		assert_eq!(
			RunnerAllocIdxKeyData::deserialize(&encoded_v1, 1)
				.expect("typed v1 data should decode"),
			converted::RunnerAllocIdxKeyData {
				workflow_id,
				remaining_slots: 7,
				total_slots: 11,
				protocol_version: rivet_runner_protocol::PROTOCOL_MK1_VERSION,
			}
		);
	}

	#[test]
	fn actor_by_key_ids_round_trip_as_native_id_without_wire_change() {
		let workflow_id = test_id(0x22222222222222222222222222222222, 43);
		let typed = converted::ActorByKeyKeyData {
			workflow_id,
			is_destroyed: true,
		};

		let expected = serde_bare::to_vec(&pegboard_namespace_actor_by_key_v1::Data {
			workflow_id: workflow_id.as_bytes(),
			is_destroyed: true,
		})
		.expect("generated data should encode");
		let encoded = ActorByKeyKeyData::wrap_latest(typed.clone())
			.serialize(1)
			.expect("typed data should encode");

		assert_eq!(encoded, expected);
		assert_eq!(
			ActorByKeyKeyData::deserialize(&encoded, 1).expect("typed data should decode"),
			typed
		);
	}

	#[test]
	fn runner_by_key_ids_round_trip_as_native_id_without_wire_change() {
		let runner_id = test_id(0x33333333333333333333333333333333, 44);
		let workflow_id = test_id(0x44444444444444444444444444444444, 45);
		let typed = converted::RunnerByKeyKeyData {
			runner_id,
			workflow_id,
		};

		let expected = serde_bare::to_vec(&pegboard_namespace_runner_by_key_v1::Data {
			runner_id: runner_id.as_bytes(),
			workflow_id: workflow_id.as_bytes(),
		})
		.expect("generated data should encode");
		let encoded = RunnerByKeyKeyData::wrap_latest(typed.clone())
			.serialize(1)
			.expect("typed data should encode");

		assert_eq!(encoded, expected);
		assert_eq!(
			RunnerByKeyKeyData::deserialize(&encoded, 1).expect("typed data should decode"),
			typed
		);
	}
}
