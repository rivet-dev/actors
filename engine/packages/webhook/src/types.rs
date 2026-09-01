use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// Every event type the engine knows how to emit. Not all of them can be subscribed to by a
// webhook: high-throughput types are recorded for analytics but would turn a webhook into a
// per-request firehose, so `is_webhook_safe` gates which ones `validate` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WebhookEventType {
	#[serde(rename = "runner_pool.error")]
	RunnerPoolError,
	#[serde(rename = "runner_pool.healthy")]
	RunnerPoolHealthy,
	#[serde(rename = "actor.http_request")]
	ActorHttpRequest,
}

impl WebhookEventType {
	// Whether a webhook is allowed to subscribe to this event type. High-throughput event types
	// stay available to analytics ingestion but are rejected at webhook config upsert.
	pub fn is_webhook_safe(&self) -> bool {
		match self {
			WebhookEventType::RunnerPoolError => true,
			WebhookEventType::RunnerPoolHealthy => true,
			// One event per actor HTTP request would hammer both the delivery pipeline and the
			// receiving endpoint.
			WebhookEventType::ActorHttpRequest => false,
		}
	}

	// The CloudEvents `type` attribute sent to the destination.
	pub fn as_cloudevents_type(&self) -> &'static str {
		match self {
			WebhookEventType::RunnerPoolError => "dev.rivet.runner_pool.error",
			WebhookEventType::RunnerPoolHealthy => "dev.rivet.runner_pool.healthy",
			WebhookEventType::ActorHttpRequest => "dev.rivet.actor.http_request",
		}
	}

	// Stable name used in API payloads and error messages. Matches the serde renames above.
	pub fn as_str(&self) -> &'static str {
		match self {
			WebhookEventType::RunnerPoolError => "runner_pool.error",
			WebhookEventType::RunnerPoolHealthy => "runner_pool.healthy",
			WebhookEventType::ActorHttpRequest => "actor.http_request",
		}
	}
}

impl From<rivet_data::generated::webhook_config_v1::WebhookEventType> for WebhookEventType {
	fn from(value: rivet_data::generated::webhook_config_v1::WebhookEventType) -> Self {
		match value {
			rivet_data::generated::webhook_config_v1::WebhookEventType::RunnerPoolError => {
				WebhookEventType::RunnerPoolError
			}
			rivet_data::generated::webhook_config_v1::WebhookEventType::RunnerPoolHealthy => {
				WebhookEventType::RunnerPoolHealthy
			}
			rivet_data::generated::webhook_config_v1::WebhookEventType::ActorHttpRequest => {
				WebhookEventType::ActorHttpRequest
			}
		}
	}
}

impl From<WebhookEventType> for rivet_data::generated::webhook_config_v1::WebhookEventType {
	fn from(value: WebhookEventType) -> Self {
		match value {
			WebhookEventType::RunnerPoolError => {
				rivet_data::generated::webhook_config_v1::WebhookEventType::RunnerPoolError
			}
			WebhookEventType::RunnerPoolHealthy => {
				rivet_data::generated::webhook_config_v1::WebhookEventType::RunnerPoolHealthy
			}
			WebhookEventType::ActorHttpRequest => {
				rivet_data::generated::webhook_config_v1::WebhookEventType::ActorHttpRequest
			}
		}
	}
}

impl From<rivet_data::generated::webhook_delivery_v1::WebhookEventType> for WebhookEventType {
	fn from(value: rivet_data::generated::webhook_delivery_v1::WebhookEventType) -> Self {
		match value {
			rivet_data::generated::webhook_delivery_v1::WebhookEventType::RunnerPoolError => {
				WebhookEventType::RunnerPoolError
			}
			rivet_data::generated::webhook_delivery_v1::WebhookEventType::RunnerPoolHealthy => {
				WebhookEventType::RunnerPoolHealthy
			}
			rivet_data::generated::webhook_delivery_v1::WebhookEventType::ActorHttpRequest => {
				WebhookEventType::ActorHttpRequest
			}
		}
	}
}

impl From<WebhookEventType> for rivet_data::generated::webhook_delivery_v1::WebhookEventType {
	fn from(value: WebhookEventType) -> Self {
		match value {
			WebhookEventType::RunnerPoolError => {
				rivet_data::generated::webhook_delivery_v1::WebhookEventType::RunnerPoolError
			}
			WebhookEventType::RunnerPoolHealthy => {
				rivet_data::generated::webhook_delivery_v1::WebhookEventType::RunnerPoolHealthy
			}
			WebhookEventType::ActorHttpRequest => {
				rivet_data::generated::webhook_delivery_v1::WebhookEventType::ActorHttpRequest
			}
		}
	}
}

// Config for a single webhook, keyed by an arbitrary name within a namespace. `subscriptions` is
// which event types this webhook wants delivered; `validate` rejects any that are not
// webhook-safe (see `WebhookEventType::is_webhook_safe`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookConfig {
	pub url: String,
	pub headers: HashMap<String, String>,
	pub subscriptions: Vec<WebhookEventType>,
}

impl From<rivet_data::generated::webhook_config_v1::Data> for WebhookConfig {
	fn from(value: rivet_data::generated::webhook_config_v1::Data) -> Self {
		WebhookConfig {
			url: value.url,
			headers: value.headers,
			subscriptions: value.subscriptions.into_iter().map(Into::into).collect(),
		}
	}
}

impl From<WebhookConfig> for rivet_data::generated::webhook_config_v1::Data {
	fn from(value: WebhookConfig) -> Self {
		rivet_data::generated::webhook_config_v1::Data {
			url: value.url,
			headers: value.headers,
			subscriptions: value.subscriptions.into_iter().map(Into::into).collect(),
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
// report its outcome. `created_at` is when the delivery was first triggered; a `Retry` reuses it
// rather than resetting it, so event history sorts by when the event actually happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRecord {
	pub payload: String,
	pub status: DeliveryStatus,
	pub attempt_count: u32,
	pub last_error: Option<String>,
	pub created_at: i64,
	pub event_type: WebhookEventType,
}

impl From<rivet_data::generated::webhook_delivery_v1::Data> for DeliveryRecord {
	fn from(value: rivet_data::generated::webhook_delivery_v1::Data) -> Self {
		DeliveryRecord {
			payload: value.payload,
			status: value.status.into(),
			attempt_count: value.attempt_count,
			last_error: value.last_error,
			created_at: value.created_at,
			event_type: value.event_type.into(),
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
			created_at: value.created_at,
			event_type: value.event_type.into(),
		}
	}
}
