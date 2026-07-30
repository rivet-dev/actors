use anyhow::Result;
use vbare::OwnedVersionedData;

use super::ToEnvoy;
use crate::generated::{v6, v8};

const REQUEST_ABORT_GOLDEN: &[u8] = &[
	4, 1, 1, 1, 1, 7, 7, 7, 7, 1, 0, 2, 1, 1, 24, 99, 108, 105, 101, 110, 116, 32, 99, 108, 111,
	115, 101, 100, 32, 99, 111, 110, 110, 101, 99, 116, 105, 111, 110,
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
	let v8::ToEnvoy::ToEnvoyTunnelMessage(msg) = decoded else {
		panic!("expected tunnel message");
	};
	let v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(abort) = msg.message_kind else {
		panic!("expected request abort");
	};

	assert_eq!(abort.reason.kind, v8::HttpStreamAbortReasonKind::Unknown);
	assert!(abort.reason.detail.is_none());
	Ok(())
}

#[test]
fn v8_request_abort_serializes_to_v6_void_abort() -> Result<()> {
	let encoded = ToEnvoy::wrap_latest(v8::ToEnvoy::ToEnvoyTunnelMessage(
		v8::ToEnvoyTunnelMessage {
			message_id: v8::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				v8::ToEnvoyRequestAbort {
					reason: v8::HttpStreamAbortReason {
						kind: v8::HttpStreamAbortReasonKind::ClientDisconnect,
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
fn request_abort_matches_cross_language_golden_bytes() -> Result<()> {
	let encoded = serde_bare::to_vec(&v8::ToEnvoy::ToEnvoyTunnelMessage(
		v8::ToEnvoyTunnelMessage {
			message_id: v8::MessageId {
				gateway_id: [1; 4],
				request_id: [7; 4],
				message_index: 1,
			},
			message_kind: v8::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				v8::ToEnvoyRequestAbort {
					reason: v8::HttpStreamAbortReason {
						kind: v8::HttpStreamAbortReasonKind::ClientDisconnect,
						detail: Some("client closed connection".into()),
					},
				},
			),
		},
	))?;

	assert_eq!(encoded, REQUEST_ABORT_GOLDEN);
	Ok(())
}
