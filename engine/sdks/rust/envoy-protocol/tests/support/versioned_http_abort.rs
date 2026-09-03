use anyhow::Result;
use vbare::OwnedVersionedData;

use super::{ToEnvoy, ToRivet};
use crate::generated::{v6, v7};

const REQUEST_ABORT_GOLDEN: &[u8] = &[
	4, 1, 1, 1, 1, 7, 7, 7, 7, 1, 0, 2, 1, 7, 97, 99, 116, 111, 114, 45, 49, 1, 7, 0, 0, 0, 1, 1,
	24, 99, 108, 105, 101, 110, 116, 32, 99, 108, 111, 115, 101, 100, 32, 99, 111, 110, 110, 101,
	99, 116, 105, 111, 110,
];

#[test]
fn v6_request_abort_deserializes_with_unknown_reason() -> Result<()> {
	let payload = serde_bare::to_vec(&v6::ToEnvoy::ToEnvoyTunnelMessage(
		v6::ToEnvoyTunnelMessage {
			message_id: v6::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v6::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort,
		},
	))?;

	let decoded = ToEnvoy::deserialize(&payload, 6)?;
	let v7::ToEnvoy::ToEnvoyTunnelMessage(msg) = decoded else {
		panic!("expected tunnel message");
	};
	let v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(abort) = msg.message_kind else {
		panic!("expected request abort");
	};

	assert_eq!(abort.reason.kind, v7::HttpStreamAbortReasonKind::Unknown);
	assert!(abort.reason.detail.is_none());
	Ok(())
}

#[test]
fn v7_request_abort_serializes_to_v6_void_abort() -> Result<()> {
	let encoded = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				v7::ToEnvoyRequestAbort {
					actor_id: None,
					actor_generation: None,
					reason: v7::HttpStreamAbortReason {
						kind: v7::HttpStreamAbortReasonKind::Cancelled,
						detail: Some("client closed connection".into()),
					},
				},
			),
		},
	))
	.serialize(6)?;

	let decoded: v6::ToEnvoy = serde_bare::from_slice(&encoded)?;
	let v6::ToEnvoy::ToEnvoyTunnelMessage(msg) = decoded else {
		panic!("expected tunnel message");
	};
	assert!(matches!(
		msg.message_kind,
		v6::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort
	));
	Ok(())
}

#[test]
fn generation_routed_v7_request_abort_cannot_downgrade_to_v6() {
	let result = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				v7::ToEnvoyRequestAbort {
					actor_id: Some("actor-1".to_owned()),
					actor_generation: Some(7),
					reason: v7::HttpStreamAbortReason {
						kind: v7::HttpStreamAbortReasonKind::Cancelled,
						detail: None,
					},
				},
			),
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn request_abort_matches_cross_language_golden_bytes() -> Result<()> {
	let encoded = serde_bare::to_vec(&v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				v7::ToEnvoyRequestAbort {
					actor_id: Some("actor-1".to_owned()),
					actor_generation: Some(7),
					reason: v7::HttpStreamAbortReason {
						kind: v7::HttpStreamAbortReasonKind::Cancelled,
						detail: Some("client closed connection".into()),
					},
				},
			),
		},
	))?;

	assert_eq!(encoded, REQUEST_ABORT_GOLDEN);
	Ok(())
}

#[test]
fn v6_request_start_upgrades_without_response_streaming() -> Result<()> {
	let payload = serde_bare::to_vec(&v6::ToEnvoy::ToEnvoyTunnelMessage(
		v6::ToEnvoyTunnelMessage {
			message_id: v6::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 0,
			},
			message_kind: v6::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				v6::ToEnvoyRequestStart {
					actor_id: "actor".into(),
					method: "POST".into(),
					path: "/upload".into(),
					headers: Default::default(),
					body: None,
					stream: true,
				},
			),
		},
	))?;

	let decoded = ToEnvoy::deserialize(&payload, 6)?;
	let v7::ToEnvoy::ToEnvoyTunnelMessage(msg) = decoded else {
		panic!("expected tunnel message");
	};
	let v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(start) = msg.message_kind else {
		panic!("expected request start");
	};
	assert_eq!(start.actor_generation, None);
	assert!(!start.response_stream);
	Ok(())
}

#[test]
fn v7_response_stream_request_cannot_downgrade_to_v6() {
	let result = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 0,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				v7::ToEnvoyRequestStart {
					actor_id: "actor".into(),
					actor_generation: None,
					method: "GET".into(),
					path: "/events".into(),
					headers: Default::default(),
					body: None,
					stream: false,
					response_stream: true,
				},
			),
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn v7_generation_routed_http_request_cannot_downgrade_to_v6() {
	let result = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 0,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				v7::ToEnvoyRequestStart {
					actor_id: "actor".into(),
					actor_generation: Some(3),
					method: "POST".into(),
					path: "/upload".into(),
					headers: Default::default(),
					body: None,
					stream: true,
					response_stream: false,
				},
			),
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn v7_generation_routed_websocket_cannot_downgrade_to_v6() {
	let result = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 0,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				v7::ToEnvoyWebSocketOpen {
					actor_id: "actor".into(),
					actor_generation: Some(3),
					path: "/socket".into(),
					headers: Default::default(),
				},
			),
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn v7_request_window_update_cannot_downgrade_to_v6() {
	let result = ToRivet::wrap_latest(v7::ToRivet::ToRivetTunnelMessage(
		v7::ToRivetTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToRivetTunnelMessageKind::ToRivetRequestBodyWindowUpdate(
				v7::ToRivetRequestBodyWindowUpdate { consumed_bytes: 1 },
			),
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn v7_response_window_update_cannot_downgrade_to_v6() {
	let result = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyResponseBodyWindowUpdate(
				v7::ToEnvoyResponseBodyWindowUpdate { consumed_bytes: 1 },
			),
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn v7_gateway_upload_cancel_cannot_downgrade_to_v6() {
	let result = ToEnvoy::wrap_latest(v7::ToEnvoy::ToEnvoyTunnelMessage(
		v7::ToEnvoyTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToEnvoyTunnelMessageKind::ToEnvoyRequestBodyCancel,
		},
	))
	.serialize(6);

	assert!(result.is_err());
}

#[test]
fn v7_actor_upload_cancel_cannot_downgrade_to_v6() {
	let result = ToRivet::wrap_latest(v7::ToRivet::ToRivetTunnelMessage(
		v7::ToRivetTunnelMessage {
			message_id: v7::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v7::ToRivetTunnelMessageKind::ToRivetRequestBodyCancel,
		},
	))
	.serialize(6);

	assert!(result.is_err());
}
