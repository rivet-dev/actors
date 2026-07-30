use rivet_envoy_protocol as protocol;

use super::to_envoy_tunnel_message_inner_data_len;

#[test]
fn request_abort_counts_detail_utf8_bytes() {
	let detail = "client failed: café".to_owned();
	let message =
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(protocol::ToEnvoyRequestAbort {
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::ClientDisconnect,
				detail: Some(detail.clone()),
			},
		});

	assert_eq!(
		to_envoy_tunnel_message_inner_data_len(&message),
		detail.len()
	);
}

#[test]
fn request_abort_without_detail_has_no_inner_payload() {
	let message =
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(protocol::ToEnvoyRequestAbort {
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::ClientDisconnect,
				detail: None,
			},
		});

	assert_eq!(to_envoy_tunnel_message_inner_data_len(&message), 0);
}
