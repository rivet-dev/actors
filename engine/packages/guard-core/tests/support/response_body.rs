use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use tokio::sync::mpsc;

use super::{ResponseBody, ResponseBodyError};

fn completion_counter() -> (Arc<AtomicUsize>, impl FnOnce() + Send + 'static) {
	let count = Arc::new(AtomicUsize::new(0));
	let callback_count = count.clone();
	(count, move || {
		callback_count.fetch_add(1, Ordering::SeqCst);
	})
}

#[tokio::test]
async fn completion_runs_once_at_eof() {
	let (count, callback) = completion_counter();
	let body = ResponseBody::Full(Full::new(Bytes::from_static(b"done"))).with_completion(callback);

	assert_eq!(
		body.collect().await.unwrap().to_bytes(),
		Bytes::from_static(b"done")
	);
	assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn completion_runs_once_on_error() {
	let (tx, rx) = mpsc::channel(1);
	tx.send(Err::<Bytes, ResponseBodyError>("stream failed".into()))
		.await
		.unwrap();
	drop(tx);

	let (count, callback) = completion_counter();
	let mut body = ResponseBody::Channel(rx).with_completion(callback);

	assert!(body.frame().await.unwrap().is_err());
	assert_eq!(count.load(Ordering::SeqCst), 1);
	drop(body);
	assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn completion_runs_once_on_drop() {
	let (_tx, rx) = mpsc::channel(1);
	let (count, callback) = completion_counter();
	let body = ResponseBody::Channel(rx).with_completion(callback);

	drop(body);
	assert_eq!(count.load(Ordering::SeqCst), 1);
}
