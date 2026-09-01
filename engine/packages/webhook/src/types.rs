use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// Config for a single webhook, keyed by an arbitrary name within a namespace. Fields are
// provisional pending the CloudEvents-shaped trigger payload design (see webhook spec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookConfig {
	pub url: String,
	pub headers: HashMap<String, String>,
}

impl From<rivet_data::generated::webhook_config_v1::Data> for WebhookConfig {
	fn from(value: rivet_data::generated::webhook_config_v1::Data) -> Self {
		WebhookConfig {
			url: value.url,
			headers: value.headers,
		}
	}
}

impl From<WebhookConfig> for rivet_data::generated::webhook_config_v1::Data {
	fn from(value: WebhookConfig) -> Self {
		rivet_data::generated::webhook_config_v1::Data {
			url: value.url,
			headers: value.headers,
		}
	}
}

// Status of a single stored delivery, keyed by delivery id (see `keys::DeliveryKey`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryStatus {
	Pending,
	Succeeded,
	Failed,
}

impl From<rivet_data::generated::webhook_delivery_v1::DeliveryStatus> for DeliveryStatus {
	fn from(value: rivet_data::generated::webhook_delivery_v1::DeliveryStatus) -> Self {
		match value {
			rivet_data::generated::webhook_delivery_v1::DeliveryStatus::Pending => {
				DeliveryStatus::Pending
			}
			rivet_data::generated::webhook_delivery_v1::DeliveryStatus::Succeeded => {
				DeliveryStatus::Succeeded
			}
			rivet_data::generated::webhook_delivery_v1::DeliveryStatus::Failed => {
				DeliveryStatus::Failed
			}
		}
	}
}

impl From<DeliveryStatus> for rivet_data::generated::webhook_delivery_v1::DeliveryStatus {
	fn from(value: DeliveryStatus) -> Self {
		match value {
			DeliveryStatus::Pending => {
				rivet_data::generated::webhook_delivery_v1::DeliveryStatus::Pending
			}
			DeliveryStatus::Succeeded => {
				rivet_data::generated::webhook_delivery_v1::DeliveryStatus::Succeeded
			}
			DeliveryStatus::Failed => {
				rivet_data::generated::webhook_delivery_v1::DeliveryStatus::Failed
			}
		}
	}
}

// Stored record for a single delivery (a triggered event, identified by delivery id, and every
// attempt made to deliver it). Not the CloudEvents payload itself, just enough to retry it and
// report its outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRecord {
	pub payload: String,
	pub status: DeliveryStatus,
	pub attempt_count: u32,
	pub last_error: Option<String>,
}

impl From<rivet_data::generated::webhook_delivery_v1::Data> for DeliveryRecord {
	fn from(value: rivet_data::generated::webhook_delivery_v1::Data) -> Self {
		DeliveryRecord {
			payload: value.payload,
			status: value.status.into(),
			attempt_count: value.attempt_count,
			last_error: value.last_error,
		}
	}
}

impl From<DeliveryRecord> for rivet_data::generated::webhook_delivery_v1::Data {
	fn from(value: DeliveryRecord) -> Self {
		rivet_data::generated::webhook_delivery_v1::Data {
			payload: value.payload,
			status: value.status.into(),
			attempt_count: value.attempt_count,
			last_error: value.last_error,
		}
	}
}
