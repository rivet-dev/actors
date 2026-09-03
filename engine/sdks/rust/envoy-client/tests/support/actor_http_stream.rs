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
	envoy::{EnvoyContext, HttpRequestRoute, ToEnvoyMessage, WebSocketRoute},
	handle::EnvoyHandle,
	http::{HTTP_BODY_MAX_CHUNK_SIZE, ResponseChunk},
	utils::BufferMap,
};

async fn tunnel_context() -> (
	EnvoyContext,
	u64,
	mpsc::UnboundedReceiver<crate::context::WsTxMessage>,
) {
	let (shared, _envoy_rx) = build_shared_context(Arc::new(TestCallbacks::idle()));
	let (ws_tx, ws_rx) = mpsc::unbounded_channel();
	let session = crate::connection::install_connection(&shared, ws_tx).await;
	let ctx = empty_envoy_context(shared);
	(ctx, session, ws_rx)
}

fn empty_envoy_context(shared: Arc<crate::context::SharedContext>) -> EnvoyContext {
	EnvoyContext {
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
		http_request_cancellations: HashMap::new(),
		buffered_messages: Vec::new(),
		processed_command_idx: HashMap::new(),
	}
}

fn insert_tunnel_actor(
	ctx: &mut EnvoyContext,
	actor_id: &str,
	generation: u32,
) -> mpsc::UnboundedReceiver<ToActor> {
	let (actor_tx, actor_rx) = mpsc::unbounded_channel();
	ctx.insert_actor(
		actor_id.to_owned(),
		generation,
		actor_tx,
		Arc::new(AsyncCounter::new()),
		format!("{actor_id}-{generation}"),
		0,
	);
	actor_rx
}

async fn recv_actor_message(actor_rx: &mut mpsc::UnboundedReceiver<ToActor>) -> ToActor {
	tokio::time::timeout(Duration::from_secs(2), actor_rx.recv())
		.await
		.expect("timed out waiting for actor message")
		.expect("actor message channel closed")
}

#[tokio::test]
async fn request_chunk_without_start_is_rejected_instead_of_buffered() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
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
async fn request_start_uses_and_pins_the_selected_generation() {
	let (mut ctx, session, _ws_rx) = tunnel_context().await;
	let mut generation_one = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	let mut generation_two = insert_tunnel_actor(&mut ctx, "test-actor", 2);

	let mut start = request_start();
	start.actor_generation = Some(1);
	start.stream = true;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(start),
		},
	)
	.await;

	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ReqStart { .. }
	));
	assert!(generation_two.try_recv().is_err());
	assert_eq!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.and_then(|route| route.actor_generation),
		Some(1),
	);
	let mut first_chunk_id = message_id();
	first_chunk_id.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: first_chunk_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				protocol::ToEnvoyRequestChunk {
					body: vec![1, 2, 3],
					finish: false,
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ReqChunk { .. }
	));
	assert!(generation_two.try_recv().is_err());

	ctx.remove_actor("test-actor", 1);
	let mut chunk_id = message_id();
	chunk_id.message_index = 2;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: chunk_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				protocol::ToEnvoyRequestChunk {
					body: vec![1, 2, 3],
					finish: false,
				},
			),
		},
	)
	.await;

	assert!(
		generation_two.try_recv().is_err(),
		"a body chunk crossed into the replacement generation"
	);
}

#[tokio::test]
async fn stale_request_generation_is_rejected_without_actor_dispatch() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut current_actor = insert_tunnel_actor(&mut ctx, "test-actor", 2);
	let mut start = request_start();
	start.actor_generation = Some(1);

	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(start),
		},
	)
	.await;

	let response = recv_ws_tunnel_msg(&mut ws_rx).await;
	let protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(response) = response.message_kind
	else {
		panic!("expected generation mismatch response");
	};
	assert_eq!(response.status, 503);
	assert_eq!(
		response.headers.get("content-type").map(String::as_str),
		Some("application/json"),
	);
	assert_eq!(
		response.headers.get("x-rivet-error").map(String::as_str),
		Some("envoy.actor_generation_mismatch"),
	);
	assert_eq!(
		serde_json::from_slice::<serde_json::Value>(
			response.body.as_deref().expect("error response body"),
		)
		.expect("decode canonical error response"),
		serde_json::json!({
			"group": "envoy",
			"code": "actor_generation_mismatch",
			"message": "Actor generation does not match",
			"actor": {
				"actorId": "test-actor",
				"generation": 1,
			},
		}),
	);
	assert!(current_actor.try_recv().is_err());
	assert!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
}

#[tokio::test]
async fn missing_request_actor_returns_a_canonical_error_without_dispatch() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;

	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				request_start(),
			),
		},
	)
	.await;

	let response = recv_ws_tunnel_msg(&mut ws_rx).await;
	let protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(response) = response.message_kind
	else {
		panic!("expected actor-not-found response");
	};
	assert_eq!(response.status, 503);
	assert_eq!(
		response.headers.get("content-type").map(String::as_str),
		Some("application/json"),
	);
	assert_eq!(
		response.headers.get("x-rivet-error").map(String::as_str),
		Some("envoy.actor_not_found"),
	);
	assert_eq!(
		serde_json::from_slice::<serde_json::Value>(
			response.body.as_deref().expect("error response body"),
		)
		.expect("decode canonical error response"),
		serde_json::json!({
			"group": "envoy",
			"code": "actor_not_found",
			"message": "Actor not found",
			"actor": {
				"actorId": "test-actor",
				"generation": 1,
			},
		}),
	);
	assert!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
	assert!(
		ctx.http_message_indices
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
}

#[tokio::test]
async fn cancellation_before_delayed_request_start_prevents_actor_dispatch() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut actor_rx = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	let mut abort_id = message_id();
	abort_id.message_index = 1;
	let abort = protocol::ToEnvoyTunnelMessage {
		message_id: abort_id,
		message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
			protocol::ToEnvoyRequestAbort {
				actor_id: Some("test-actor".to_owned()),
				actor_generation: Some(1),
				reason: protocol::HttpStreamAbortReason {
					kind: protocol::HttpStreamAbortReasonKind::Cancelled,
					detail: Some("request start delivery was indeterminate".to_owned()),
				},
			},
		),
	};

	crate::tunnel::handle_tunnel_message(&mut ctx, session, abort.clone()).await;
	crate::tunnel::handle_tunnel_message(&mut ctx, session, abort).await;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				request_start(),
			),
		},
	)
	.await;

	assert!(actor_rx.try_recv().is_err());
	assert!(ws_rx.try_recv().is_err());
	assert!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
	assert_eq!(ctx.http_request_cancellations.len(), 1);
}

#[tokio::test]
async fn cancellation_tombstone_suppresses_a_replayed_admitted_request_start() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut actor_rx = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	let start = protocol::ToEnvoyTunnelMessage {
		message_id: message_id(),
		message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(request_start()),
	};

	crate::tunnel::handle_tunnel_message(&mut ctx, session, start.clone()).await;
	assert!(matches!(
		recv_actor_message(&mut actor_rx).await,
		ToActor::ReqStart { .. }
	));

	let mut abort_id = message_id();
	abort_id.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: abort_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				protocol::ToEnvoyRequestAbort {
					actor_id: Some("test-actor".to_owned()),
					actor_generation: Some(1),
					reason: protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::Cancelled,
						detail: None,
					},
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut actor_rx).await,
		ToActor::ReqAbort { .. }
	));

	crate::tunnel::handle_tunnel_message(&mut ctx, session, start).await;
	assert!(actor_rx.try_recv().is_err());
	assert!(ws_rx.try_recv().is_err());
}

#[tokio::test]
async fn rejected_streamed_request_drains_pipelined_body_without_a_second_response() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut current_actor = insert_tunnel_actor(&mut ctx, "test-actor", 2);
	let mut start = request_start();
	start.actor_generation = Some(1);
	start.method = "POST".to_owned();
	start.stream = true;

	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(start),
		},
	)
	.await;
	let response = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		response.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_)
	));
	assert_eq!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.map(|route| route.actor_admitted),
		Some(false),
	);

	let mut chunk_id = message_id();
	chunk_id.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: chunk_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				protocol::ToEnvoyRequestChunk {
					body: vec![1, 2, 3],
					finish: false,
				},
			),
		},
	)
	.await;
	assert!(current_actor.try_recv().is_err());
	assert!(ws_rx.try_recv().is_err());

	let mut cancel_id = message_id();
	cancel_id.message_index = 2;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: cancel_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestBodyCancel,
		},
	)
	.await;
	assert!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
	assert!(
		ctx.http_message_indices
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
	assert!(ws_rx.try_recv().is_err());
}

#[tokio::test]
async fn websocket_open_uses_and_pins_the_selected_generation() {
	let (mut ctx, session, _ws_rx) = tunnel_context().await;
	let mut generation_one = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	let mut generation_two = insert_tunnel_actor(&mut ctx, "test-actor", 2);

	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				protocol::ToEnvoyWebSocketOpen {
					actor_id: "test-actor".to_owned(),
					actor_generation: Some(1),
					path: "/socket".to_owned(),
					headers: Default::default(),
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::WsOpen { .. }
	));
	let mut first_message = message_id();
	first_message.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: first_message,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				protocol::ToEnvoyWebSocketMessage {
					data: b"selected generation".to_vec(),
					binary: false,
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::WsMsg { .. }
	));
	assert!(generation_two.try_recv().is_err());

	ctx.remove_actor("test-actor", 1);
	let mut message = message_id();
	message.message_index = 2;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				protocol::ToEnvoyWebSocketMessage {
					data: b"replacement must not receive this".to_vec(),
					binary: false,
				},
			),
		},
	)
	.await;
	assert!(generation_two.try_recv().is_err());
}

#[tokio::test]
async fn stale_websocket_generation_is_rejected_without_actor_dispatch() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut current_actor = insert_tunnel_actor(&mut ctx, "test-actor", 2);

	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				protocol::ToEnvoyWebSocketOpen {
					actor_id: "test-actor".to_owned(),
					actor_generation: Some(1),
					path: "/socket".to_owned(),
					headers: Default::default(),
				},
			),
		},
	)
	.await;

	let response = recv_ws_tunnel_msg(&mut ws_rx).await;
	let protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(close) = response.message_kind
	else {
		panic!("expected generation mismatch close");
	};
	assert_eq!(close.code, Some(1011));
	assert_eq!(
		close.reason.as_deref(),
		Some("envoy.actor_generation_mismatch")
	);
	assert!(current_actor.try_recv().is_err());
}

#[tokio::test]
async fn exact_generation_rejects_an_actor_after_stop_begins_but_legacy_routing_is_unchanged() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut actor_rx = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	ctx.actors
		.get_mut("test-actor")
		.and_then(|generations| generations.get_mut(&1))
		.expect("actor generation must exist")
		.received_stop = true;

	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(request_start()),
		},
	)
	.await;
	let response = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		response.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_)
	));
	assert!(actor_rx.try_recv().is_err());

	let mut websocket_id = message_id();
	websocket_id.request_id = [4, 3, 2, 1];
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: websocket_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				protocol::ToEnvoyWebSocketOpen {
					actor_id: "test-actor".to_owned(),
					actor_generation: Some(1),
					path: "/socket".to_owned(),
					headers: Default::default(),
				},
			),
		},
	)
	.await;
	let response = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert!(matches!(
		response.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(_)
	));
	assert!(actor_rx.try_recv().is_err());

	let mut legacy_start = request_start();
	legacy_start.actor_generation = None;
	let mut legacy_id = message_id();
	legacy_id.request_id = [9, 8, 7, 6];
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: legacy_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(legacy_start),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut actor_rx).await,
		ToActor::ReqStart { .. }
	));
}

#[tokio::test]
async fn admitted_exact_generation_routes_continue_during_actor_stop() {
	let (mut ctx, session, mut ws_rx) = tunnel_context().await;
	let mut generation_one = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	let mut generation_two = insert_tunnel_actor(&mut ctx, "test-actor", 2);

	let mut start = request_start();
	start.actor_generation = Some(1);
	start.method = "POST".to_owned();
	start.stream = true;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(start),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ReqStart { .. }
	));

	let mut websocket_id = message_id();
	websocket_id.request_id = [4, 3, 2, 1];
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: websocket_id.clone(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				protocol::ToEnvoyWebSocketOpen {
					actor_id: "test-actor".to_owned(),
					actor_generation: Some(1),
					path: "/socket".to_owned(),
					headers: Default::default(),
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::WsOpen { .. }
	));

	ctx.actors
		.get_mut("test-actor")
		.and_then(|generations| generations.get_mut(&1))
		.expect("actor generation must exist")
		.received_stop = true;

	let mut chunk_id = message_id();
	chunk_id.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: chunk_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(
				protocol::ToEnvoyRequestChunk {
					body: vec![1, 2, 3],
					finish: false,
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ReqChunk { .. }
	));

	let mut body_cancel_id = message_id();
	body_cancel_id.message_index = 2;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: body_cancel_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestBodyCancel,
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ReqBodyCancel { .. }
	));

	let mut window_id = message_id();
	window_id.message_index = 3;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: window_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyResponseBodyWindowUpdate(
				protocol::ToEnvoyResponseBodyWindowUpdate { consumed_bytes: 10 },
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ResponseBodyWindowUpdate {
			consumed_bytes: 10,
			..
		}
	));

	let mut abort_id = message_id();
	abort_id.message_index = 4;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: abort_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(
				protocol::ToEnvoyRequestAbort {
					actor_id: Some("test-actor".to_owned()),
					actor_generation: Some(1),
					reason: protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::Cancelled,
						detail: None,
					},
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::ReqAbort { .. }
	));
	assert!(
		ctx.http_request_routes
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);
	assert!(
		ctx.http_message_indices
			.get(&[&message_id().gateway_id, &message_id().request_id])
			.is_none()
	);

	let mut websocket_message_id = websocket_id.clone();
	websocket_message_id.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: websocket_message_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				protocol::ToEnvoyWebSocketMessage {
					data: b"during stop".to_vec(),
					binary: false,
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::WsMsg { .. }
	));
	crate::tunnel::send_hibernatable_ws_message_ack(
		&mut ctx,
		websocket_id.gateway_id,
		websocket_id.request_id,
		7,
	);
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::HwsAck {
			envoy_message_index: 7,
			..
		}
	));

	websocket_id.message_index = 2;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: websocket_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(
				protocol::ToEnvoyWebSocketClose {
					code: Some(1000),
					reason: None,
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::WsClose { .. }
	));
	assert!(generation_two.try_recv().is_err());

	let mut rejected_start = request_start();
	rejected_start.actor_generation = Some(1);
	let mut rejected_id = message_id();
	rejected_id.request_id = [9, 9, 9, 9];
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: rejected_id,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(
				rejected_start,
			),
		},
	)
	.await;
	assert!(matches!(
		recv_ws_tunnel_msg(&mut ws_rx).await.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_)
	));
	assert!(generation_one.try_recv().is_err());
	assert!(generation_two.try_recv().is_err());
}

#[tokio::test]
async fn legacy_routes_track_the_highest_live_generation() {
	let (mut ctx, session, _ws_rx) = tunnel_context().await;
	let mut generation_one = insert_tunnel_actor(&mut ctx, "test-actor", 1);
	let mut generation_two = insert_tunnel_actor(&mut ctx, "test-actor", 2);
	let mut legacy_start = request_start();
	legacy_start.actor_generation = None;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(legacy_start),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_two).await,
		ToActor::ReqStart { .. }
	));
	assert!(generation_one.try_recv().is_err());

	let mut websocket_id = message_id();
	websocket_id.request_id = [9, 8, 7, 6];
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: websocket_id.clone(),
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				protocol::ToEnvoyWebSocketOpen {
					actor_id: "test-actor".to_owned(),
					actor_generation: None,
					path: "/socket".to_owned(),
					headers: Default::default(),
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_two).await,
		ToActor::WsOpen { .. }
	));
	assert!(generation_one.try_recv().is_err());

	ctx.remove_actor("test-actor", 2);
	let mut message = websocket_id;
	message.message_index = 1;
	crate::tunnel::handle_tunnel_message(
		&mut ctx,
		session,
		protocol::ToEnvoyTunnelMessage {
			message_id: message,
			message_kind: protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				protocol::ToEnvoyWebSocketMessage {
					data: b"legacy replacement".to_vec(),
					binary: false,
				},
			),
		},
	)
	.await;
	assert!(matches!(
		recv_actor_message(&mut generation_one).await,
		ToActor::WsMsg { .. }
	));
}

#[tokio::test]
async fn hibernating_websocket_rebinds_only_generation_routed_connections() {
	let (mut ctx, _session, _ws_rx) = tunnel_context().await;
	let message_id = message_id();
	ctx.request_to_actor.insert(
		&[&message_id.gateway_id, &message_id.request_id],
		WebSocketRoute {
			actor_id: "test-actor".to_owned(),
			actor_generation: Some(1),
		},
	);
	assert!(!ctx.rebind_websocket(
		"different-actor",
		2,
		&message_id.gateway_id,
		&message_id.request_id,
	));
	assert_eq!(
		ctx.request_to_actor
			.get(&[&message_id.gateway_id, &message_id.request_id])
			.and_then(|route| route.actor_generation),
		Some(1),
	);

	assert!(ctx.rebind_websocket(
		"test-actor",
		2,
		&message_id.gateway_id,
		&message_id.request_id,
	));
	assert_eq!(
		ctx.request_to_actor
			.get(&[&message_id.gateway_id, &message_id.request_id])
			.and_then(|route| route.actor_generation),
		Some(2),
	);

	let legacy_request_id = [9, 8, 7, 6];
	ctx.request_to_actor.insert(
		&[&message_id.gateway_id, &legacy_request_id],
		WebSocketRoute {
			actor_id: "test-actor".to_owned(),
			actor_generation: None,
		},
	);
	assert!(ctx.rebind_websocket("test-actor", 2, &message_id.gateway_id, &legacy_request_id,));
	assert_eq!(
		ctx.request_to_actor
			.get(&[&message_id.gateway_id, &legacy_request_id])
			.and_then(|route| route.actor_generation),
		None,
	);
}

#[tokio::test]
async fn hibernating_websocket_rebind_creates_a_route_on_a_new_envoy() {
	let (mut ctx, _session, _ws_rx) = tunnel_context().await;
	let message_id = message_id();

	assert!(ctx.rebind_websocket(
		"test-actor",
		2,
		&message_id.gateway_id,
		&message_id.request_id,
	));
	let route = ctx
		.request_to_actor
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.expect("restore on a new Envoy should create the ephemeral route");
	assert_eq!(route.actor_id, "test-actor");
	assert_eq!(route.actor_generation, Some(2));
	assert_eq!(
		ctx.shared
			.live_tunnel_requests
			.lock()
			.expect("live tunnel request registry")
			.get(&crate::tunnel::make_ws_key(
				&message_id.gateway_id,
				&message_id.request_id,
			))
			.map(String::as_str),
		Some("test-actor"),
	);
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
async fn connection_close_replays_one_indexed_terminal_abort_after_reconnect() {
	let (body_tx_tx, body_tx_rx) = oneshot::channel();
	let callbacks = Arc::new(StreamingCallbacks {
		body_tx: Mutex::new(Some(body_tx_tx)),
	});
	let (shared, mut envoy_rx) = build_shared_context(callbacks);
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let session_one = crate::connection::install_connection(&shared, ws_tx).await;
	let (actor_tx, active_http_request_count) = create_actor(
		shared.clone(),
		"actor-session-flap".to_owned(),
		1,
		actor_config(),
		Vec::new(),
		None,
	);

	actor_tx
		.send(ToActor::ReqStart {
			message_id: message_id(),
			req: request_start(),
			connection_session: session_one,
		})
		.expect("send request start");
	let body_tx = body_tx_rx.await.expect("receive response body sender");
	let start = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert_eq!(start.message_id.message_index, 0);
	assert!(matches!(
		start.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_)
	));
	body_tx
		.send(ResponseChunk::Data {
			data: vec![7],
			finish: false,
		})
		.await
		.expect("send response chunk");
	let chunk = recv_ws_tunnel_msg(&mut ws_rx).await;
	assert_eq!(chunk.message_id.message_index, 1);
	assert!(matches!(
		chunk.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(_)
	));

	crate::connection::remove_connection_for_session(&shared, session_one).await;
	actor_tx
		.send(ToActor::ConnectionClosed {
			session: session_one,
		})
		.expect("notify actor of closed gateway session");

	let terminal = tokio::time::timeout(Duration::from_secs(2), async {
		loop {
			match envoy_rx.recv().await {
				Some(ToEnvoyMessage::SendOrBufferTunnelMsg { msg }) => break msg,
				Some(_) => {}
				None => panic!("envoy channel closed before terminal abort"),
			}
		}
	})
	.await
	.expect("timed out waiting for terminal abort");
	assert_eq!(terminal.message_id.message_index, 2);
	assert!(matches!(
		terminal.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
			protocol::ToRivetResponseAbort {
				reason: protocol::HttpStreamAbortReason {
					kind: protocol::HttpStreamAbortReasonKind::InternalError,
					..
				}
			}
		)
	));
	wait_for_zero(&active_http_request_count).await;

	let mut envoy_ctx = empty_envoy_context(shared.clone());
	let key: [&[u8]; 2] = [&terminal.message_id.gateway_id, &terminal.message_id.request_id];
	envoy_ctx.http_request_routes.insert(
		&key,
		HttpRequestRoute {
			actor_id: "actor-session-flap".to_owned(),
			actor_generation: Some(1),
			actor_admitted: true,
			session: session_one,
			gateway_id: terminal.message_id.gateway_id,
			request_id: terminal.message_id.request_id,
		},
	);
	envoy_ctx.http_message_indices.insert(&key, 1);
	crate::envoy::remove_http_routes_for_session(&mut envoy_ctx, session_one);
	assert!(envoy_ctx.http_request_routes.get(&key).is_none());
	assert!(envoy_ctx.http_message_indices.get(&key).is_none());

	crate::tunnel::send_or_buffer_tunnel_message(&mut envoy_ctx, terminal).await;
	assert_eq!(envoy_ctx.buffered_messages.len(), 1);

	let (replacement_ws_tx, mut replacement_ws_rx) = mpsc::unbounded_channel();
	let session_two = crate::connection::install_connection(&shared, replacement_ws_tx).await;
	assert_ne!(session_two, session_one);
	crate::tunnel::resend_buffered_tunnel_messages(&mut envoy_ctx).await;
	let replayed = recv_ws_tunnel_msg(&mut replacement_ws_rx).await;
	assert_eq!(replayed.message_id.message_index, 2);
	assert!(matches!(
		replayed.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_)
	));
	assert!(envoy_ctx.buffered_messages.is_empty());
	assert!(replacement_ws_rx.try_recv().is_err());

	while envoy_rx.try_recv().is_ok() {}
	actor_tx
		.send(ToActor::ConnectionClosed {
			session: session_one,
		})
		.expect("repeat closed-session notification");
	yield_now().await;
	assert!(
		envoy_rx.try_recv().is_err(),
		"closed request emitted a second terminal message"
	);
}

#[tokio::test]
async fn failed_response_write_queues_terminal_before_connclose_is_processed() {
	let (body_tx_tx, body_tx_rx) = oneshot::channel();
	let callbacks = Arc::new(StreamingCallbacks {
		body_tx: Mutex::new(Some(body_tx_tx)),
	});
	let (shared, mut envoy_rx) = build_shared_context(callbacks);
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let session = crate::connection::install_connection(&shared, ws_tx).await;
	let (actor_tx, active_http_request_count) = create_actor(
		shared.clone(),
		"actor-write-failure".to_owned(),
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
	assert!(matches!(
		recv_ws_tunnel_msg(&mut ws_rx).await.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(_)
	));

	crate::connection::remove_connection_for_session(&shared, session).await;
	body_tx
		.send(ResponseChunk::Data {
			data: vec![9],
			finish: false,
		})
		.await
		.expect("send response after transport closes");
	let terminal = tokio::time::timeout(Duration::from_secs(2), async {
		loop {
			match envoy_rx.recv().await {
				Some(ToEnvoyMessage::SendOrBufferTunnelMsg { msg }) => break msg,
				Some(_) => {}
				None => panic!("envoy channel closed before terminal abort"),
			}
		}
	})
	.await
	.expect("timed out waiting for terminal abort");
	assert_eq!(terminal.message_id.message_index, 1);
	assert!(matches!(
		terminal.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_)
	));
	wait_for_zero(&active_http_request_count).await;
}

#[tokio::test]
async fn queued_terminal_uses_an_already_reconnected_session() {
	let (shared, _envoy_rx) = build_shared_context(Arc::new(TestCallbacks::idle()));
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	let _session = crate::connection::install_connection(&shared, ws_tx).await;
	let mut ctx = empty_envoy_context(shared);
	let terminal = protocol::ToRivetTunnelMessage {
		message_id: message_id(),
		message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
			protocol::ToRivetResponseAbort {
				reason: protocol::HttpStreamAbortReason {
					kind: protocol::HttpStreamAbortReasonKind::InternalError,
					detail: None,
				},
			},
		),
	};

	crate::tunnel::send_or_buffer_tunnel_message(&mut ctx, terminal).await;
	assert!(ctx.buffered_messages.is_empty());
	assert!(matches!(
		recv_ws_tunnel_msg(&mut ws_rx).await.message_kind,
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_)
	));
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
