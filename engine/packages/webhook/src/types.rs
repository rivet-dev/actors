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
