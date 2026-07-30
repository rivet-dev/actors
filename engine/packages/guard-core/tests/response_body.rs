use bytes::Bytes;
use http_body_util::BodyExt;
use rivet_guard_core::ResponseBody;
use tokio::sync::mpsc;

#[tokio::test]
async fn channel_body_yields_sent_chunks() {
	let (tx, rx) = mpsc::channel(2);
	tx.send(Ok(Bytes::from_static(b"hello "))).await.unwrap();
	tx.send(Ok(Bytes::from_static(b"world"))).await.unwrap();
	drop(tx);

	let collected = ResponseBody::Channel(rx).collect().await.unwrap();

	assert_eq!(collected.to_bytes(), Bytes::from_static(b"hello world"));
}

#[tokio::test]
async fn channel_body_surfaces_errors() {
	let (tx, rx) = mpsc::channel(1);
	tx.send(Err(std::io::Error::other("stream failed").into()))
		.await
		.unwrap();
	drop(tx);

	let mut body = ResponseBody::Channel(rx);
	let frame = body.frame().await.expect("expected frame");

	assert!(frame.is_err());
}
