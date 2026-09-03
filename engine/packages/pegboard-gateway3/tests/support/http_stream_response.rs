use super::*;

#[test]
fn response_stream_message_index_advances_and_wraps() {
	assert_eq!(advance_http_stream_message_index(7, 7), Ok(8));
	assert_eq!(
		advance_http_stream_message_index(protocol::MessageIndex::MAX, protocol::MessageIndex::MAX),
		Ok(0)
	);
}

#[test]
fn response_stream_message_index_rejects_gaps() {
	assert_eq!(advance_http_stream_message_index(7, 8), Err(()));
	assert_eq!(advance_http_stream_message_index(7, 6), Err(()));
}

#[test]
fn blocked_response_delivery_uses_a_terminal_abort_for_every_failure() {
	assert_eq!(
		delivery_failure_abort_kind(RequestStopResult::EnvoyError),
		protocol::HttpStreamAbortReasonKind::InternalError,
	);
	assert_eq!(
		delivery_failure_abort_kind(RequestStopResult::ActorReadyTimeout),
		protocol::HttpStreamAbortReasonKind::InternalError,
	);
	assert_eq!(
		delivery_failure_abort_kind(RequestStopResult::RequestTimeout),
		protocol::HttpStreamAbortReasonKind::InternalError,
	);
	assert_eq!(
		delivery_failure_abort_kind(RequestStopResult::ClientDisconnect),
		protocol::HttpStreamAbortReasonKind::Cancelled,
	);
}

#[tokio::test]
async fn envoy_abort_interrupts_response_delivery_blocked_by_client_backpressure() {
	let (body_tx, mut body_rx) = mpsc::channel(1);
	body_tx
		.send(Ok(Bytes::from_static(b"already buffered")))
		.await
		.expect("fill downstream response channel");
	let (abort_tx, mut abort_rx) = watch::channel(None);
	let blocked = deliver_response_chunk(
		&body_tx,
		&mut abort_rx,
		Bytes::from_static(b"blocked chunk"),
	);
	tokio::pin!(blocked);

	assert!(
		tokio::time::timeout(Duration::from_millis(20), &mut blocked)
			.await
			.is_err(),
		"response delivery did not apply client backpressure",
	);
	abort_tx.send_replace(Some(protocol::HttpStreamAbortReason {
		kind: protocol::HttpStreamAbortReasonKind::InternalError,
		detail: Some("Envoy session closed".to_owned()),
	}));

	let result = tokio::time::timeout(Duration::from_secs(1), &mut blocked)
		.await
		.expect("terminal abort did not interrupt blocked response delivery");
	assert!(matches!(
		result,
		ResponseChunkDelivery::EnvoyAborted(Some(protocol::HttpStreamAbortReason {
			kind: protocol::HttpStreamAbortReasonKind::InternalError,
			..
		}))
	));
	assert_eq!(
		body_rx
			.recv()
			.await
			.expect("read original buffered chunk")
			.expect("buffered chunk should not be an error"),
		Bytes::from_static(b"already buffered"),
	);
	assert!(body_rx.try_recv().is_err());
}
