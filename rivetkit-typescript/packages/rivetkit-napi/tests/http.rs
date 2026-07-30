use super::*;

mod moved_tests {
	use rivetkit_core::HttpRequestBodyStream as CoreHttpRequestBodyStream;
	use tokio::sync::{mpsc, watch};

	use super::HttpRequestBodyStream;

	#[tokio::test]
	async fn cancelling_http_request_body_drops_core_receiver() {
		let (body_tx, body_rx) = mpsc::channel(1);
		let (_abort_tx, abort_rx) = watch::channel(None);
		let stream = HttpRequestBodyStream::new(
			Vec::new(),
			CoreHttpRequestBodyStream::new(body_rx, abort_rx),
		);

		stream.cancel().await.expect("cancel request body stream");

		assert!(body_tx.is_closed());
		assert!(
			stream
				.read()
				.await
				.expect("read cancelled request body")
				.is_none()
		);
	}
}
