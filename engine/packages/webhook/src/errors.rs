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
	#[error(
		"delivery_failed",
		"Webhook delivery failed.",
		"Webhook delivery failed with status {status}."
	)]
	DeliveryFailed { status: u16 },
	#[error(
		"destination_blocked",
		"Webhook destination is not allowed.",
		"Webhook destination is not allowed: {reason}"
	)]
	DestinationBlocked { reason: String },
	#[error("not_found", "Webhook not found.", "Webhook not found.")]
	NotFound,
	#[error(
		"delivery_not_found",
		"Webhook delivery not found.",
		"Webhook delivery not found."
	)]
	DeliveryNotFound,
	#[error(
		"delivery_not_retryable",
		"Webhook delivery is not in a retryable state.",
		"Webhook delivery is not in a retryable state; only failed deliveries can be retried."
	)]
	DeliveryNotRetryable,
	#[error(
		"event_type_not_allowed",
		"Event type cannot be subscribed to by a webhook.",
		"Event type {event_type} cannot be subscribed to by a webhook because it is too high-throughput."
	)]
	EventTypeNotAllowed { event_type: String },
}
