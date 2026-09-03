use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Features {
	/// Controls routing to the streaming Guard Gateway V3 implementation.
	#[serde(default)]
	pub guard_gateway_v3: Option<GuardGatewayV3>,
}

impl Features {
	pub fn guard_gateway_v3(&self) -> GuardGatewayV3 {
		self.guard_gateway_v3.clone().unwrap_or_default()
	}

	pub(super) fn validate(&self) -> Result<()> {
		if let Some(guard_gateway_v3) = &self.guard_gateway_v3 {
			guard_gateway_v3.validate()?;
		}

		Ok(())
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardGatewayV3 {
	#[serde(default)]
	pub mode: GuardGatewayV3Mode,
	#[serde(default)]
	#[schemars(range(max = 100))]
	pub percentage: u8,
}

impl GuardGatewayV3 {
	fn validate(&self) -> Result<()> {
		if self.percentage > 100 {
			bail!("features.guard_gateway_v3.percentage must be in 0..=100");
		}

		Ok(())
	}
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuardGatewayV3Mode {
	#[default]
	Off,
	Opportunistic,
	On,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn guard_gateway_v3_defaults_to_off() {
		let config = crate::config::Root::default()
			.features()
			.guard_gateway_v3();
		assert_eq!(config.mode, GuardGatewayV3Mode::Off);
		assert_eq!(config.percentage, 0);
	}

	#[test]
	fn guard_gateway_v3_deserializes_from_features_section() {
		let root: crate::config::Root = serde_json::from_value(serde_json::json!({
			"features": {
				"guard_gateway_v3": {
					"mode": "on",
					"percentage": 25
				}
			}
		}))
		.unwrap();
		let config = root.features().guard_gateway_v3();
		assert_eq!(config.mode, GuardGatewayV3Mode::On);
		assert_eq!(config.percentage, 25);
	}

	#[test]
	fn guard_gateway_v3_rejects_invalid_percentage() {
		let config = GuardGatewayV3 {
			mode: GuardGatewayV3Mode::On,
			percentage: 101,
		};
		assert!(config.validate().is_err());
	}
}
