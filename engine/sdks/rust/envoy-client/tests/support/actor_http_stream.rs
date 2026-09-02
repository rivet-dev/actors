use std::{
	collections::HashMap,
	sync::atomic::{AtomicUsize, Ordering},
	sync::{Arc, Mutex},
	time::Duration,
};

use tokio::sync::{mpsc, oneshot};
use tokio::task::yield_now;

use super::http::ActiveHttpRequestGuard;
use super::tests::{
	StreamingCallbacks, StreamingRequestCallbacks, TestCallbacks, actor_config,
	build_shared_context, message_id, recv_ws_tunnel_msg, request_start, wait_for_zero,
};
use super::{ToActor, create_actor, protocol};
use crate::{
	async_counter::AsyncCounter,
	context::SharedActorEntry,
	envoy::EnvoyContext,
	handle::EnvoyHandle,
	http::{HTTP_BODY_MAX_CHUNK_SIZE, ResponseChunk},
	utils::BufferMap,
};

#[tokio::test]
async fn request_chunk_without_start_is_rejected_instead_of_buffered() {
	let (shared, _envoy_rx) = build_shared_context(Arc::new(TestCallbacks::idle()));
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let session = crate::connection::install_connection(&shared, ws_tx).await;
	let mut ctx = EnvoyContext {
		shared,
		shutting_down: false,
		actors: HashMap::new(),
		buffered_actor_messages: HashMap::new(),
		kv_requests: HashMap::new(),
		next_kv_request_id: 0,
		sqlite_requests: HashMap::new(),
		next_sqlite_request_id: 0,
		remote_sqlite_requests: HashMap::new(),
		next_remote_sqlite_request_id: 0,
		request_to_actor: BufferMap::new(),
		http_request_routes: BufferMap::new(),
		http_message_indices: BufferMap::new(),
		buffered_messages: Vec::new(),
		processed_command_idx: HashMap::new(),
	};
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				protocol::ToEnvoyRequestChunk {
					body: vec![1, 2, 3],
					finish: false,
				},
			),
		},
	)
	.await;

	let abort = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		abort.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(protocol::ToRivetResponseAbort {
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::InternalError,
				..
			}
		})
	));
}

#[tokio::test]
async fn streamed_request_remains_cancellable_after_upload_finishes() {
	let (fetch_started_tx, fetch_started_rx) = oneshot::channel();
	let (fetch_dropped_tx, fetch_dropped_rx) = oneshot::channel();
	let callbacks = Arc::new(TestCallbacks::hanging(fetch_started_tx, fetch_dropped_tx));
	let (shared, _envoy_rx) = build_shared_context(callbacks);
	let (actor_tx, _) = create_actor(
		shared,
		"actor-finished-upload".to_string(),
		1,
		actor_config(),
		Vec::new(),
		None,
	);
	let mut request = request_start();
	request.method = "POST".to_owned();
	request.stream = true;

	actor_tx
		.send(ToActor::ReqStart {
			message_id: message_id(),
			req: request,
			connection_session: 1,
		})
		.expect("failed to send request start");
	fetch_started_rx
		.await
		.expect("fetch start sender dropped before request began");
	actor_tx
		.send(ToActor::ReqChunk {
			message_id: message_id(),
			chunk: protocol::ToEnvoyRequestChunk {
				body: Vec::new(),
				finish: true,
			},
		})
		.expect("failed to finish upload");
	actor_tx
		.send(ToActor::ReqAbort {
			message_id: message_id(),
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::Cancelled,
				detail: None,
			},
		})
		.expect("failed to abort completed upload");

	tokio::time::timeout(Duration::from_secs(2), fetch_dropped_rx)
		.await
		.expect("request handler was not cancelled after upload completion")
		.expect("fetch drop sender dropped");
}

#[tokio::test]
async fn streamed_request_returns_credit_only_after_handler_consumption() {
	let (request_tx, request_rx) = oneshot::channel();
	let callbacks = Arc::new(StreamingRequestCallbacks {
		request_tx: Mutex::new(Some(request_tx)),
	});
	let (shared, _envoy_rx) = build_shared_context(callbacks);
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let session = crate::connection::install_connection(&shared, ws_tx).await;
	let (actor_tx, _active_http_request_count) = create_actor(
		shared,
		"actor-request-window".to_string(),
		1,
		actor_config(),
		Vec::new(),
		None,
	);
	let mut start = request_start();
	start.method = "POST".to_owned();
	start.stream = true;

	actor_tx
		.send(ToActor::ReqStart {
			message_id: message_id(),
			req: start,
			connection_session: session,
		})
		.expect("send streamed request start");
	let mut request = request_rx.await.expect("receive streamed request");
	let mut body = request
		.body_stream
		.take()
		.expect("request should carry a body stream");
	actor_tx
		.send(ToActor::ReqChunk {
			message_id: message_id(),
			chunk: protocol::ToEnvoyRequestChunk {
				body: vec![7; 32],
				finish: false,
			},
		})
		.expect("send request body chunk");

	assert!(
		tokio::time::timeout(Duration::from_millis(20), ws_rx.recv())
			.await
			.is_err(),
		"actor returned request credit before the handler consumed bytes",
	);
	assert_eq!(
		body.recv().await.expect("read request body"),
		Some(vec![7; 32]),
	);
	let credit = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		credit.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyWindowUpdate(
			protocol::ToRivetRequestBodyWindowUpdate { consumed_bytes: 32 }
		)
	));

	actor_tx
		.send(ToActor::ReqAbort {
			message_id: message_id(),
			reason: protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::Cancelled,
				detail: None,
			},
		})
		.expect("abort request after flow-control assertion");
}

#[tokio::test]
async fn active_http_request_count_spans_streaming_response_drain() {
	let (body_tx_tx, body_tx_rx) = oneshot::channel();
	let callbacks = Arc::new(StreamingCallbacks {
		body_tx: Mutex::new(Some(body_tx_tx)),
	});
	let (shared, _envoy_rx) = build_shared_context(callbacks);
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let session = crate::connection::install_connection(&shared, ws_tx).await;
	let (actor_tx, active_http_request_count) = create_actor(
		shared,
		"actor-stream".to_string(),
		1,
		actor_config(),
		Vec::new(),
		None,
	);

	actor_tx
		.send(ToActor::ReqStart {
			message_id: message_id(),
			req: request_start(),
			connection_session: session,
		})
		.expect("failed to send request start");

	let body_tx = tokio::time::timeout(Duration::from_secs(2), body_tx_rx)
		.await
		.expect("timed out waiting for response body sender")
		.expect("response body sender dropped");
	let start_msg = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		start_msg.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(protocol::ToRivetResponseStart {
			stream: true,
			..
		})
	));
	assert_eq!(active_http_request_count.load(), 1);

	body_tx
		.send(ResponseChunk::Data {
			data: vec![7; HTTP_BODY_MAX_CHUNK_SIZE + 3],
			finish: false,
		})
		.await
		.expect("failed to send response data");

	let first = recv_ws_tunnel_msg(&mut ws_rx).await;
	let second = recv_ws_tunnel_msg(&mut ws_rx).await;
	match first.message_kind {
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => {
			assert_eq!(first.message_id.message_index, 1);
			assert_eq!(chunk.body.len(), HTTP_BODY_MAX_CHUNK_SIZE);
			assert!(!chunk.finish);
		}
		other => panic!("expected first response chunk, got {other:?}"),
	}
	match second.message_kind {
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => {
			assert_eq!(second.message_id.message_index, 2);
			assert_eq!(chunk.body.len(), 3);
			assert!(!chunk.finish);
		}
		other => panic!("expected second response chunk, got {other:?}"),
	}
	assert_eq!(active_http_request_count.load(), 1);

	body_tx
		.send(ResponseChunk::Data {
			data: Vec::new(),
			finish: true,
		})
		.await
		.expect("failed to finish response stream");
	let finish = recv_ws_tunnel_msg(&mut ws_rx).await;
	match finish.message_kind {
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => {
			assert_eq!(finish.message_id.message_index, 3);
			assert!(chunk.body.is_empty());
			assert!(chunk.finish);
		}
		other => panic!("expected finish response chunk, got {other:?}"),
	}

	wait_for_zero(&active_http_request_count).await;
}

#[tokio::test]
async fn streamed_response_stalls_at_window_until_gateway_consumes_bytes() {
	let (body_tx_tx, body_tx_rx) = oneshot::channel();
	let callbacks = Arc::new(StreamingCallbacks {
		body_tx: Mutex::new(Some(body_tx_tx)),
	});
	let (shared, _envoy_rx) = build_shared_context(callbacks);
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let session = crate::connection::install_connection(&shared, ws_tx).await;
	let (actor_tx, active_http_request_count) = create_actor(
		shared,
		"actor-response-window".to_string(),
		1,
		actor_config(),
		Vec::new(),
		None,
	);

	actor_tx
		.send(ToActor::ReqStart {
			message_id: message_id(),
			req: request_start(),
			connection_session: session,
		})
		.expect("send request start");
	let body_tx = body_tx_rx.await.expect("receive response body sender");
	let start = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		start.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_)
	));

	body_tx
		.send(ResponseChunk::Data {
			data: vec![7; protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as usize + 1],
			finish: false,
		})
		.await
		.expect("send response larger than initial window");
	let window_chunks =
		protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES as usize / HTTP_BODY_MAX_CHUNK_SIZE;
	for expected_index in 1..=window_chunks {
		let chunk = recv_ws_tunnel_msg(&mut ws_rx).await;
		assert_eq!(chunk.message_id.message_index as usize, expected_index);
		assert!(matches!(
			chunk.message_kind,
			protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(
				protocol::ToRivetResponseChunk { ref body, finish: false }
			) if body.len() == HTTP_BODY_MAX_CHUNK_SIZE
		));
	}
	assert!(
		tokio::time::timeout(Duration::from_millis(20), ws_rx.recv())
			.await
			.is_err(),
		"actor sent bytes beyond the response window"
	);

	actor_tx
		.send(ToActor::ResponseBodyWindowUpdate {
			message_id: message_id(),
			consumed_bytes: 1,
		})
		.expect("return response credit");
	let final_data = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert_eq!(
		final_data.message_id.message_index as usize,
		window_chunks + 1
	);
	assert!(matches!(
		final_data.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(
			protocol::ToRivetResponseChunk { ref body, finish: false }
		) if body == &[7]
	));

	body_tx
		.send(ResponseChunk::Data {
			data: Vec::new(),
			finish: true,
		})
		.await
		.expect("finish response stream");
	let finish = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		finish.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(
			protocol::ToRivetResponseChunk { ref body, finish: true }
		) if body.is_empty()
	));
	wait_for_zero(&active_http_request_count).await;
}

#[tokio::test]
async fn http_request_guard_counter_is_visible_through_envoy_handle() {
	let (shared, _envoy_rx) = build_shared_context(Arc::new(TestCallbacks::idle()));
	let handle = EnvoyHandle {
		shared: shared.clone(),
		started_rx: tokio::sync::watch::channel(()).1,
	};
	let counter = Arc::new(AsyncCounter::new());
	shared
		.actors
		.lock()
		.expect("shared actor registry poisoned")
		.entry("actor-4".to_string())
		.or_insert_with(HashMap::new)
		.insert(
			4,
			SharedActorEntry {
				handle: mpsc::unbounded_channel().0,
				active_http_request_count: counter.clone(),
			},
		);

	let request_guard = ActiveHttpRequestGuard::new(counter);
	let handle_counter = handle
		.http_request_counter("actor-4", Some(4))
		.expect("counter should be returned");
	assert_eq!(handle_counter.load(), 1);

	drop(request_guard);
	assert_eq!(handle_counter.load(), 0);
	assert!(
		handle_counter
			.wait_zero(crate::time::Instant::now() + Duration::from_secs(2))
			.await
	);
}

#[tokio::test]
async fn active_http_request_counter_waiter_wakes_only_after_final_drop() {
	let counter = Arc::new(AsyncCounter::new());
	let guard_a = ActiveHttpRequestGuard::new(counter.clone());
	let guard_b = ActiveHttpRequestGuard::new(counter.clone());
	let wake_count = Arc::new(AtomicUsize::new(0));

	let waiter = tokio::spawn({
		let counter = counter.clone();
		let wake_count = wake_count.clone();
		async move {
			let woke = counter
				.wait_zero(crate::time::Instant::now() + Duration::from_secs(2))
				.await;
			if woke {
				wake_count.fetch_add(1, Ordering::SeqCst);
			}
			woke
		}
	});

	yield_now().await;
	drop(guard_a);
	yield_now().await;
	assert_eq!(wake_count.load(Ordering::SeqCst), 0);
	assert!(
		!waiter.is_finished(),
		"waiter should stay pending until the final in-flight request completes"
	);

	drop(guard_b);
	assert!(waiter.await.expect("waiter should join"));
	assert_eq!(wake_count.load(Ordering::SeqCst), 1);
}
