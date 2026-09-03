use rivet_error::RivetError;
use rivet_guard_core::errors::{
	ActorStoppedWhileWaitingForWebSocketOpen, ActorWakeRetriesExceeded, RequestDeliveryUnconfirmed,
	TunnelMessageTimeout, WebSocketOpenTimeout, WebSocketTargetChanged,
};

#[test]
fn websocket_open_timeout_includes_timeout_metadata() {
	let err = WebSocketOpenTimeout { timeout_ms: 5000 }.build();
	let rivet_err = RivetError::extract(&err);

	assert_eq!(rivet_err.group(), "guard");
	assert_eq!(rivet_err.code(), "websocket_open_timeout");
	assert_eq!(
		rivet_err.message(),
		"Timed out waiting for WebSocket open after 5000 ms."
	);

	let metadata = rivet_err.metadata().expect("metadata should be present");
	assert_eq!(metadata["timeout_ms"], 5000);
}

#[test]
fn websocket_open_actor_stop_includes_phase_and_actor_metadata() {
	let err = ActorStoppedWhileWaitingForWebSocketOpen {
		actor_id: "actor-123".to_owned(),
		phase: "waiting_for_websocket_open".to_owned(),
	}
	.build();
	let rivet_err = RivetError::extract(&err);

	assert_eq!(rivet_err.group(), "guard");
	assert_eq!(
		rivet_err.code(),
		"actor_stopped_while_waiting_for_websocket_open"
	);

	let metadata = rivet_err.metadata().expect("metadata should be present");
	assert_eq!(metadata["actor_id"], "actor-123");
	assert_eq!(metadata["phase"], "waiting_for_websocket_open");
}

#[test]
fn tunnel_message_timeout_includes_gc_reason_metadata() {
	let err = TunnelMessageTimeout {
		phase: "waiting_for_response_start".to_owned(),
		reason: "Some(HibernationTimeout)".to_owned(),
	}
	.build();
	let rivet_err = RivetError::extract(&err);

	assert_eq!(rivet_err.group(), "guard");
	assert_eq!(rivet_err.code(), "tunnel_message_timeout");

	let metadata = rivet_err.metadata().expect("metadata should be present");
	assert_eq!(metadata["phase"], "waiting_for_response_start");
	assert_eq!(metadata["reason"], "Some(HibernationTimeout)");
}

#[test]
fn request_delivery_unconfirmed_is_a_distinct_error() {
	let err = RequestDeliveryUnconfirmed {
		phase: "request_start".to_owned(),
		reason: "envoy_handoff_ack_timeout".to_owned(),
	}
	.build();
	let rivet_err = RivetError::extract(&err);

	assert_eq!(rivet_err.group(), "guard");
	assert_eq!(rivet_err.code(), "request_delivery_unconfirmed");
	let metadata = rivet_err.metadata().expect("metadata should be present");
	assert_eq!(metadata["phase"], "request_start");
	assert_eq!(metadata["reason"], "envoy_handoff_ack_timeout");
}

#[test]
fn actor_wake_retries_exceeded_includes_retry_metadata() {
	let err = ActorWakeRetriesExceeded {
		actor_id: "actor-456".to_owned(),
		wake_retries: 8,
		reason: "actor_stopped_before_ready".to_owned(),
	}
	.build();
	let rivet_err = RivetError::extract(&err);

	assert_eq!(rivet_err.group(), "guard");
	assert_eq!(rivet_err.code(), "actor_wake_retries_exceeded");

	let metadata = rivet_err.metadata().expect("metadata should be present");
	assert_eq!(metadata["actor_id"], "actor-456");
	assert_eq!(metadata["wake_retries"], 8);
	assert_eq!(metadata["reason"], "actor_stopped_before_ready");
}

#[test]
fn websocket_target_changed_includes_target_metadata() {
	let err = WebSocketTargetChanged {
		phase: "custom_serve_websocket_retry".to_owned(),
		from_target_kind: "custom_serve".to_owned(),
		to_target_kind: "target".to_owned(),
	}
	.build();
	let rivet_err = RivetError::extract(&err);

	assert_eq!(rivet_err.group(), "guard");
	assert_eq!(rivet_err.code(), "target_changed");

	let metadata = rivet_err.metadata().expect("metadata should be present");
	assert_eq!(metadata["phase"], "custom_serve_websocket_retry");
	assert_eq!(metadata["from_target_kind"], "custom_serve");
	assert_eq!(metadata["to_target_kind"], "target");
}
