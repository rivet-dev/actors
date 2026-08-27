use rivet_error::*;
use serde::{Deserialize, Serialize};

#[derive(RivetError, Debug, Deserialize, Serialize)]
#[error("webhook")]
pub enum Webhook {
	#[error(
		"invalid",
		"Invalid webhook config.",
		"Invalid webhook config: {reason}"
	)]
	Invalid { reason: String },
	#[error(
		"conflict",
		"Webhook config changed concurrently.",
		"Webhook config was modified concurrently, please retry."
	)]
	Conflict,
}
