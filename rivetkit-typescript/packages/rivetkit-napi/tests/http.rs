use super::*;

mod moved_tests {
	use rivetkit_core::HttpRequestBodyStream as CoreHttpRequestBodyStream;
	use tokio::sync::{mpsc, watch};

	use super::{HttpRequestBodyStream, HttpResponseBodyStream, ResponseChunk};

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

	#[tokio::test]
	async fn cancelling_http_request_body_unblocks_a_pending_read() {
		let (_body_tx, body_rx) = mpsc::channel(1);
		let (_abort_tx, abort_rx) = watch::channel(None);
		let stream = std::sync::Arc::new(HttpRequestBodyStream::new(
			Vec::new(),
			CoreHttpRequestBodyStream::new(body_rx, abort_rx),
		));
		let read = {
			let stream = stream.clone();
			tokio::spawn(async move { stream.read().await })
		};
		tokio::task::yield_now().await;

		stream.cancel().await.expect("cancel pending read");
		let result = tokio::time::timeout(std::time::Duration::from_secs(1), read)
			.await
			.expect("pending read should unblock")
			.expect("join pending read")
			.expect("pending read result");
		assert!(result.is_none());
	}

	#[tokio::test]
	async fn response_end_cannot_overtake_a_blocked_write() {
		let (tx, mut rx) = mpsc::channel(1);
		let stream = HttpResponseBodyStream::new(tx);
		stream
			.write(vec![1].into())
			.await
			.expect("write first response chunk");

		let blocked_write = {
			let stream = stream.clone();
			tokio::spawn(async move { stream.write(vec![2].into()).await })
		};
		tokio::task::yield_now().await;
		let end = {
			let stream = stream.clone();
			tokio::spawn(async move { stream.end().await })
		};

		assert!(matches!(
			rx.recv().await,
			Some(ResponseChunk::Data { data, finish: false }) if data == vec![1]
		));
		assert!(matches!(
			rx.recv().await,
			Some(ResponseChunk::Data { data, finish: false }) if data == vec![2]
		));
		assert!(matches!(
			rx.recv().await,
			Some(ResponseChunk::Data { data, finish: true }) if data.is_empty()
		));

		blocked_write.await.expect("join blocked write").expect("blocked write");
		end.await.expect("join end").expect("end response");
		assert!(stream.write(vec![3].into()).await.is_err());
	}

	#[tokio::test]
	async fn response_error_cannot_overtake_a_blocked_write() {
		let (tx, mut rx) = mpsc::channel(1);
		let stream = HttpResponseBodyStream::new(tx);
		stream.write(vec![1].into()).await.expect("first write");
		let blocked_write = {
			let stream = stream.clone();
			tokio::spawn(async move { stream.write(vec![2].into()).await })
		};
		tokio::task::yield_now().await;
		let error = {
			let stream = stream.clone();
			tokio::spawn(async move { stream.error("boom".to_owned()).await })
		};

		assert!(matches!(
			rx.recv().await,
			Some(ResponseChunk::Data { data, finish: false }) if data == vec![1]
		));
		assert!(matches!(
			rx.recv().await,
			Some(ResponseChunk::Data { data, finish: false }) if data == vec![2]
		));
		assert!(matches!(rx.recv().await, Some(ResponseChunk::Error(message)) if message == "boom"));

		blocked_write.await.unwrap().unwrap();
		error.await.unwrap().unwrap();
		assert!(stream.end().await.is_err());
	}

	#[tokio::test]
	async fn response_writer_splits_large_chunks_before_terminal() {
		let (tx, mut rx) = mpsc::channel(4);
		let stream = HttpResponseBodyStream::new(tx);
		let size = rivetkit_core::HTTP_BODY_MAX_CHUNK_SIZE * 2 + 1;
		stream.write(vec![7; size].into()).await.expect("large write");
		stream.end().await.expect("end response");

		for expected in [
			rivetkit_core::HTTP_BODY_MAX_CHUNK_SIZE,
			rivetkit_core::HTTP_BODY_MAX_CHUNK_SIZE,
			1,
		] {
			assert!(matches!(
				rx.recv().await,
				Some(ResponseChunk::Data { data, finish: false }) if data.len() == expected
			));
		}
		assert!(matches!(
			rx.recv().await,
			Some(ResponseChunk::Data { data, finish: true }) if data.is_empty()
		));
	}
}
