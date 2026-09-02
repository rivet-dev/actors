use std::{error::Error, fmt};

use anyhow::{Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::{v1, v2, v3, v4, v5, v6, v7};

mod v1_to_v2;
mod v2_to_v1;
mod v2_to_v3;
mod v3_to_v2;
mod v3_to_v4;
mod v4_to_v3;
mod v4_to_v5;
mod v5_to_v4;
mod v5_to_v6;
mod v6_to_v5;
mod v6_to_v7;
mod v7_to_v6;

// MARK: Protocol compatibility errors

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolCompatibilityFeature {
	SqliteStartupData,
	SqlitePageIo,
	SqlitePageRange,
	RemoteSqliteExecution,
	RemoteSqliteBatchExecution,
}

impl ProtocolCompatibilityFeature {
	fn description(self, direction: ProtocolCompatibilityDirection) -> &'static str {
		match self {
			ProtocolCompatibilityFeature::SqliteStartupData => "sqlite startup data",
			ProtocolCompatibilityFeature::SqlitePageIo => match direction {
				ProtocolCompatibilityDirection::ToEnvoy => "sqlite responses",
				ProtocolCompatibilityDirection::ToRivet => "sqlite requests",
			},
			ProtocolCompatibilityFeature::SqlitePageRange => match direction {
				ProtocolCompatibilityDirection::ToEnvoy => "sqlite range responses",
				ProtocolCompatibilityDirection::ToRivet => "sqlite range requests",
			},
			ProtocolCompatibilityFeature::RemoteSqliteExecution => match direction {
				ProtocolCompatibilityDirection::ToEnvoy => "remote sqlite responses",
				ProtocolCompatibilityDirection::ToRivet => "remote sqlite requests",
			},
			ProtocolCompatibilityFeature::RemoteSqliteBatchExecution => match direction {
				ProtocolCompatibilityDirection::ToEnvoy => "remote sqlite batch responses",
				ProtocolCompatibilityDirection::ToRivet => "remote sqlite batch requests",
			},
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolCompatibilityDirection {
	ToEnvoy,
	ToRivet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolCompatibilityError {
	pub feature: ProtocolCompatibilityFeature,
	pub direction: ProtocolCompatibilityDirection,
	pub required_version: u16,
	pub target_version: u16,
}

impl fmt::Display for ProtocolCompatibilityError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let verb = match self.feature {
			ProtocolCompatibilityFeature::SqliteStartupData => "requires",
			ProtocolCompatibilityFeature::SqlitePageIo
			| ProtocolCompatibilityFeature::SqlitePageRange
			| ProtocolCompatibilityFeature::RemoteSqliteExecution => "require",
			ProtocolCompatibilityFeature::RemoteSqliteBatchExecution => "require",
		};
		write!(
			f,
			"{} {} envoy-protocol v{} but target version is v{}",
			self.feature.description(self.direction),
			verb,
			self.required_version,
			self.target_version,
		)
	}
}

impl Error for ProtocolCompatibilityError {}

pub(crate) fn incompatible(
	feature: ProtocolCompatibilityFeature,
	direction: ProtocolCompatibilityDirection,
	required_version: u16,
	target_version: u16,
) -> anyhow::Error {
	ProtocolCompatibilityError {
		feature,
		direction,
		required_version,
		target_version,
	}
	.into()
}

// MARK: ToEnvoy

pub enum ToEnvoy {
	V1(v1::ToEnvoy),
	V2(v2::ToEnvoy),
	V3(v3::ToEnvoy),
	V4(v4::ToEnvoy),
	V5(v5::ToEnvoy),
	V6(v6::ToEnvoy),
	V7(v7::ToEnvoy),
}

impl OwnedVersionedData for ToEnvoy {
	type Latest = v7::ToEnvoy;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V7(x) => Ok(x),
			_ => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Self::V2(serde_bare::from_slice(payload)?)),
			3 => Ok(Self::V3(serde_bare::from_slice(payload)?)),
			4 => Ok(Self::V4(serde_bare::from_slice(payload)?)),
			5 => Ok(Self::V5(serde_bare::from_slice(payload)?)),
			6 => Ok(Self::V6(serde_bare::from_slice(payload)?)),
			7 => Ok(Self::V7(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V2(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V3(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V4(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V5(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V6(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V7(x) => serde_bare::to_vec(&x).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
			Self::v6_to_v7,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v7_to_v6,
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl ToEnvoy {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Self::V1(x) => Ok(Self::V2(v1_to_v2::convert_to_envoy_v1_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V1(v2_to_v1::convert_to_envoy_v2_to_v1(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v3(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V3(v2_to_v3::convert_to_envoy_v2_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v2(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V2(v3_to_v2::convert_to_envoy_v3_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v4(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V4(v3_to_v4::convert_to_envoy_v3_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v3(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V3(v4_to_v3::convert_to_envoy_v4_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v5(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V5(v4_to_v5::convert_to_envoy_v4_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v4(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V4(v5_to_v4::convert_to_envoy_v5_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v6(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V6(v5_to_v6::convert_to_envoy_v5_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v5(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V5(v6_to_v5::convert_to_envoy_v6_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v7(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V7(v6_to_v7::convert_to_envoy_v6_to_v7(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v7_to_v6(self) -> Result<Self> {
		match self {
			Self::V7(x) => Ok(Self::V6(v7_to_v6::convert_to_envoy_v7_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
}

// MARK: ToRivet

pub enum ToRivet {
	V1(v1::ToRivet),
	V2(v2::ToRivet),
	V3(v3::ToRivet),
	V4(v4::ToRivet),
	V5(v5::ToRivet),
	V6(v6::ToRivet),
	V7(v7::ToRivet),
}

impl OwnedVersionedData for ToRivet {
	type Latest = v7::ToRivet;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V7(x) => Ok(x),
			_ => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Self::V2(serde_bare::from_slice(payload)?)),
			3 => Ok(Self::V3(serde_bare::from_slice(payload)?)),
			4 => Ok(Self::V4(serde_bare::from_slice(payload)?)),
			5 => Ok(Self::V5(serde_bare::from_slice(payload)?)),
			6 => Ok(Self::V6(serde_bare::from_slice(payload)?)),
			7 => Ok(Self::V7(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V2(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V3(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V4(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V5(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V6(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V7(x) => serde_bare::to_vec(&x).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
			Self::v6_to_v7,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v7_to_v6,
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl ToRivet {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Self::V1(x) => Ok(Self::V2(v1_to_v2::convert_to_rivet_v1_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V1(v2_to_v1::convert_to_rivet_v2_to_v1(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v3(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V3(v2_to_v3::convert_to_rivet_v2_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v2(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V2(v3_to_v2::convert_to_rivet_v3_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v4(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V4(v3_to_v4::convert_to_rivet_v3_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v3(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V3(v4_to_v3::convert_to_rivet_v4_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v5(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V5(v4_to_v5::convert_to_rivet_v4_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v4(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V4(v5_to_v4::convert_to_rivet_v5_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v6(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V6(v5_to_v6::convert_to_rivet_v5_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v5(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V5(v6_to_v5::convert_to_rivet_v6_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v7(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V7(v6_to_v7::convert_to_rivet_v6_to_v7(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v7_to_v6(self) -> Result<Self> {
		match self {
			Self::V7(x) => Ok(Self::V6(v7_to_v6::convert_to_rivet_v7_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
}

// MARK: ToEnvoyConn

pub enum ToEnvoyConn {
	V1(v1::ToEnvoyConn),
	V2(v2::ToEnvoyConn),
	V3(v3::ToEnvoyConn),
	V4(v4::ToEnvoyConn),
	V5(v5::ToEnvoyConn),
	V6(v6::ToEnvoyConn),
	V7(v7::ToEnvoyConn),
}

impl OwnedVersionedData for ToEnvoyConn {
	type Latest = v7::ToEnvoyConn;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V7(x) => Ok(x),
			_ => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Self::V2(serde_bare::from_slice(payload)?)),
			3 => Ok(Self::V3(serde_bare::from_slice(payload)?)),
			4 => Ok(Self::V4(serde_bare::from_slice(payload)?)),
			5 => Ok(Self::V5(serde_bare::from_slice(payload)?)),
			6 => Ok(Self::V6(serde_bare::from_slice(payload)?)),
			7 => Ok(Self::V7(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V2(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V3(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V4(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V5(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V6(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V7(x) => serde_bare::to_vec(&x).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
			Self::v6_to_v7,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v7_to_v6,
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl ToEnvoyConn {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Self::V1(x) => Ok(Self::V2(v1_to_v2::convert_to_envoy_conn_v1_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V1(v2_to_v1::convert_to_envoy_conn_v2_to_v1(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v3(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V3(v2_to_v3::convert_to_envoy_conn_v2_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v2(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V2(v3_to_v2::convert_to_envoy_conn_v3_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v4(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V4(v3_to_v4::convert_to_envoy_conn_v3_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v3(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V3(v4_to_v3::convert_to_envoy_conn_v4_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v5(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V5(v4_to_v5::convert_to_envoy_conn_v4_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v4(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V4(v5_to_v4::convert_to_envoy_conn_v5_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v6(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V6(v5_to_v6::convert_to_envoy_conn_v5_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v5(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V5(v6_to_v5::convert_to_envoy_conn_v6_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v7(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V7(v6_to_v7::convert_to_envoy_conn_v6_to_v7(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v7_to_v6(self) -> Result<Self> {
		match self {
			Self::V7(x) => Ok(Self::V6(v7_to_v6::convert_to_envoy_conn_v7_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
}

// MARK: ToGateway

pub enum ToGateway {
	V1(v1::ToGateway),
	V2(v2::ToGateway),
	V3(v3::ToGateway),
	V4(v4::ToGateway),
	V5(v5::ToGateway),
	V6(v6::ToGateway),
	V7(v7::ToGateway),
}

impl OwnedVersionedData for ToGateway {
	type Latest = v7::ToGateway;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V7(x) => Ok(x),
			_ => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Self::V2(serde_bare::from_slice(payload)?)),
			3 => Ok(Self::V3(serde_bare::from_slice(payload)?)),
			4 => Ok(Self::V4(serde_bare::from_slice(payload)?)),
			5 => Ok(Self::V5(serde_bare::from_slice(payload)?)),
			6 => Ok(Self::V6(serde_bare::from_slice(payload)?)),
			7 => Ok(Self::V7(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V2(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V3(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V4(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V5(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V6(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V7(x) => serde_bare::to_vec(&x).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
			Self::v6_to_v7,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v7_to_v6,
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl ToGateway {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Self::V1(x) => Ok(Self::V2(v1_to_v2::convert_to_gateway_v1_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V1(v2_to_v1::convert_to_gateway_v2_to_v1(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v3(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V3(v2_to_v3::convert_to_gateway_v2_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v2(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V2(v3_to_v2::convert_to_gateway_v3_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v4(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V4(v3_to_v4::convert_to_gateway_v3_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v3(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V3(v4_to_v3::convert_to_gateway_v4_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v5(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V5(v4_to_v5::convert_to_gateway_v4_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v4(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V4(v5_to_v4::convert_to_gateway_v5_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v6(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V6(v5_to_v6::convert_to_gateway_v5_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v5(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V5(v6_to_v5::convert_to_gateway_v6_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v7(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V7(v6_to_v7::convert_to_gateway_v6_to_v7(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v7_to_v6(self) -> Result<Self> {
		match self {
			Self::V7(x) => Ok(Self::V6(v7_to_v6::convert_to_gateway_v7_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
}

// MARK: ToOutbound

pub enum ToOutbound {
	V1(v1::ToOutbound),
	V2(v2::ToOutbound),
	V3(v3::ToOutbound),
	V4(v4::ToOutbound),
	V5(v5::ToOutbound),
	V6(v6::ToOutbound),
	V7(v7::ToOutbound),
}

impl OwnedVersionedData for ToOutbound {
	type Latest = v7::ToOutbound;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V7(x) => Ok(x),
			_ => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Self::V2(serde_bare::from_slice(payload)?)),
			3 => Ok(Self::V3(serde_bare::from_slice(payload)?)),
			4 => Ok(Self::V4(serde_bare::from_slice(payload)?)),
			5 => Ok(Self::V5(serde_bare::from_slice(payload)?)),
			6 => Ok(Self::V6(serde_bare::from_slice(payload)?)),
			7 => Ok(Self::V7(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V2(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V3(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V4(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V5(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V6(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V7(x) => serde_bare::to_vec(&x).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
			Self::v6_to_v7,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v7_to_v6,
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl ToOutbound {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Self::V1(x) => Ok(Self::V2(v1_to_v2::convert_to_outbound_v1_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V1(v2_to_v1::convert_to_outbound_v2_to_v1(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v3(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V3(v2_to_v3::convert_to_outbound_v2_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v2(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V2(v3_to_v2::convert_to_outbound_v3_to_v2(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v4(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V4(v3_to_v4::convert_to_outbound_v3_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v3(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V3(v4_to_v3::convert_to_outbound_v4_to_v3(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v5(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V5(v4_to_v5::convert_to_outbound_v4_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v4(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V4(v5_to_v4::convert_to_outbound_v5_to_v4(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v6(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V6(v5_to_v6::convert_to_outbound_v5_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v5(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V5(v6_to_v5::convert_to_outbound_v6_to_v5(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v7(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V7(v6_to_v7::convert_to_outbound_v6_to_v7(x)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v7_to_v6(self) -> Result<Self> {
		match self {
			Self::V7(x) => Ok(Self::V6(v7_to_v6::convert_to_outbound_v7_to_v6(x)?)),
			_ => bail!("unexpected version"),
		}
	}
}

// MARK: ActorCommandKeyData

pub enum ActorCommandKeyData {
	V1(v1::ActorCommandKeyData),
	V2(v2::ActorCommandKeyData),
	V3(v3::ActorCommandKeyData),
	V4(v4::ActorCommandKeyData),
	V5(v5::ActorCommandKeyData),
	V6(v6::ActorCommandKeyData),
	V7(v7::ActorCommandKeyData),
}

impl OwnedVersionedData for ActorCommandKeyData {
	type Latest = v7::ActorCommandKeyData;

	fn wrap_latest(latest: Self::Latest) -> Self {
		Self::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		match self {
			Self::V7(x) => Ok(x),
			_ => bail!("version not latest"),
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(Self::V1(serde_bare::from_slice(payload)?)),
			2 => Ok(Self::V2(serde_bare::from_slice(payload)?)),
			3 => Ok(Self::V3(serde_bare::from_slice(payload)?)),
			4 => Ok(Self::V4(serde_bare::from_slice(payload)?)),
			5 => Ok(Self::V5(serde_bare::from_slice(payload)?)),
			6 => Ok(Self::V6(serde_bare::from_slice(payload)?)),
			7 => Ok(Self::V7(serde_bare::from_slice(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			Self::V1(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V2(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V3(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V4(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V5(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V6(x) => serde_bare::to_vec(&x).map_err(Into::into),
			Self::V7(x) => serde_bare::to_vec(&x).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
			Self::v6_to_v7,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v7_to_v6,
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl ActorCommandKeyData {
	fn v1_to_v2(self) -> Result<Self> {
		match self {
			Self::V1(x) => Ok(Self::V2(v1_to_v2::convert_actor_command_key_data_v1_to_v2(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v1(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V1(v2_to_v1::convert_actor_command_key_data_v2_to_v1(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v2_to_v3(self) -> Result<Self> {
		match self {
			Self::V2(x) => Ok(Self::V3(v2_to_v3::convert_actor_command_key_data_v2_to_v3(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v2(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V2(v3_to_v2::convert_actor_command_key_data_v3_to_v2(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v3_to_v4(self) -> Result<Self> {
		match self {
			Self::V3(x) => Ok(Self::V4(v3_to_v4::convert_actor_command_key_data_v3_to_v4(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v3(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V3(v4_to_v3::convert_actor_command_key_data_v4_to_v3(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v4_to_v5(self) -> Result<Self> {
		match self {
			Self::V4(x) => Ok(Self::V5(v4_to_v5::convert_actor_command_key_data_v4_to_v5(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v4(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V4(v5_to_v4::convert_actor_command_key_data_v5_to_v4(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v5_to_v6(self) -> Result<Self> {
		match self {
			Self::V5(x) => Ok(Self::V6(v5_to_v6::convert_actor_command_key_data_v5_to_v6(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v5(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V5(v6_to_v5::convert_actor_command_key_data_v6_to_v5(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v6_to_v7(self) -> Result<Self> {
		match self {
			Self::V6(x) => Ok(Self::V7(v6_to_v7::convert_actor_command_key_data_v6_to_v7(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
	fn v7_to_v6(self) -> Result<Self> {
		match self {
			Self::V7(x) => Ok(Self::V6(v7_to_v6::convert_actor_command_key_data_v7_to_v6(
				x,
			)?)),
			_ => bail!("unexpected version"),
		}
	}
}

// MARK: Tests

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use vbare::OwnedVersionedData;

	use super::{ActorCommandKeyData, ToEnvoy};
	use crate::{
		PROTOCOL_VERSION,
		generated::{v1, v2, v7},
	};

	#[test]
	fn protocol_version_constant_matches_schema_version() {
		assert_eq!(PROTOCOL_VERSION, 7);
	}

	#[test]
	fn v1_start_command_deserializes_into_latest_without_sqlite_startup_data() -> Result<()> {
		let payload =
			serde_bare::to_vec(&v1::ToEnvoy::ToEnvoyCommands(vec![v1::CommandWrapper {
				checkpoint: v1::ActorCheckpoint {
					actor_id: "actor".into(),
					generation: 7,
					index: 3,
				},
				inner: v1::Command::CommandStartActor(v1::CommandStartActor {
					config: v1::ActorConfig {
						name: "demo".into(),
						key: Some("key".into()),
						create_ts: 42,
						input: None,
					},
					hibernating_requests: Vec::new(),
					preloaded_kv: None,
				}),
			}]))?;

		let decoded = ToEnvoy::deserialize(&payload, 1)?;
		let v7::ToEnvoy::ToEnvoyCommands(commands) = decoded else {
			panic!("expected commands");
		};
		let v7::Command::CommandStartActor(start) = &commands[0].inner else {
			panic!("expected start actor");
		};

		assert!(start.preloaded_kv.is_none());
		assert_eq!(commands[0].checkpoint.generation, 7);

		Ok(())
	}

	#[test]
	fn v2_sqlite_response_does_not_deserialize_to_stateless_protocol() -> Result<()> {
		let payload = serde_bare::to_vec(&v2::ToEnvoy::ToEnvoySqliteCommitResponse(
			v2::ToEnvoySqliteCommitResponse {
				request_id: 1,
				data: v2::SqliteCommitResponse::SqliteErrorResponse(v2::SqliteErrorResponse {
					message: "old sqlite".into(),
				}),
			},
		))?;

		assert!(ToEnvoy::deserialize(&payload, 2).is_err());
		Ok(())
	}

	#[test]
	fn actor_command_key_data_round_trips_to_v1() -> Result<()> {
		let encoded = ActorCommandKeyData::wrap_latest(v7::ActorCommandKeyData::CommandStartActor(
			v7::CommandStartActor {
				config: v7::ActorConfig {
					name: "demo".into(),
					key: None,
					create_ts: 7,
					input: None,
				},
				hibernating_requests: Vec::new(),
				preloaded_kv: None,
			},
		))
		.serialize(1)?;

		let decoded = ActorCommandKeyData::deserialize(&encoded, 1)?;
		let v7::ActorCommandKeyData::CommandStartActor(start) = decoded else {
			panic!("expected start actor");
		};
		assert_eq!(start.config.name, "demo");

		Ok(())
	}
}

#[cfg(test)]
#[path = "../../tests/support/versioned_http_abort.rs"]
mod http_abort_tests;
