use rivet_envoy_protocol as protocol;

use super::tunnel_message_inner_data_len;

#[test]
fn response_abort_counts_detail_utf8_bytes() {
	let detail = "actor failed: café".to_owned();
	let message =
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(protocol::ToRivetResponseAbort {
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::HandlerError,
				detail: Some(detail.clone()),
			},
		});

	assert_eq!(tunnel_message_inner_data_len(&message), detail.len());
}

#[test]
fn response_abort_without_detail_has_no_inner_payload() {
	let message =
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(protocol::ToRivetResponseAbort {
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::HandlerError,
				detail: None,
			},
		});

	assert_eq!(tunnel_message_inner_data_len(&message), 0);
}
