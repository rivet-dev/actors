use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, anyhow};
use tokio::sync::oneshot;

use super::ordered_handoff;

#[tokio::test]
async fn gateway_reply_waits_for_websocket_handoff() {
	let replied = Arc::new(AtomicBool::new(false));
	let replied_for_task = replied.clone();
	let (handoff_tx, handoff_rx) = oneshot::channel();

	let task = tokio::spawn(ordered_handoff(
		async move {
			handoff_rx.await.expect("handoff sender dropped");
			Ok(())
		},
		async move {
			replied_for_task.store(true, Ordering::Release);
			Ok(())
		},
	));

	tokio::task::yield_now().await;
	assert!(!replied.load(Ordering::Acquire));

	handoff_tx.send(()).expect("handoff receiver dropped");
	task.await.expect("handoff task panicked").unwrap();
	assert!(replied.load(Ordering::Acquire));
}

#[tokio::test]
async fn failed_websocket_handoff_is_not_acknowledged() {
	let replied = Arc::new(AtomicBool::new(false));
	let replied_for_task = replied.clone();

	let result: Result<()> =
		ordered_handoff(async { Err(anyhow!("websocket closed")) }, async move {
			replied_for_task.store(true, Ordering::Release);
			Ok(())
		})
		.await;

	assert!(result.is_err());
	assert!(!replied.load(Ordering::Acquire));
}
