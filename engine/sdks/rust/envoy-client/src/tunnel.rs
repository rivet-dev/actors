use rivet_envoy_protocol as protocol;
use std::collections::HashMap;

use crate::connection::ws_send;
use crate::envoy::{BufferedActorMessage, EnvoyContext, HttpRequestRoute};

fn make_ws_key(gateway_id: &protocol::GatewayId, request_id: &protocol::RequestId) -> [u8; 8] {
	let mut key = [0u8; 8];
	key[..4].copy_from_slice(gateway_id);
	key[4..].copy_from_slice(request_id);
	key
}

fn advance_http_message_index(
	ctx: &mut EnvoyContext,
	message_id: &protocol::MessageId,
) -> bool {
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	let Some(expected) = ctx.http_message_indices.get_mut(&key) else {
		tracing::warn!(
			message_index = message_id.message_index,
			"received HTTP tunnel message without request start"
		);
		return false;
	};
	if message_id.message_index != *expected {
		tracing::warn!(
			expected_message_index = *expected,
			actual_message_index = message_id.message_index,
			"received reordered HTTP tunnel message"
		);
		return false;
	}
	*expected = expected.wrapping_add(1);
	true
}

pub struct HibernatingWebSocketMetadata {
	pub gateway_id: protocol::GatewayId,
	pub request_id: protocol::RequestId,
	pub envoy_message_index: u16,
	pub rivet_message_index: u16,
	pub path: String,
	pub headers: std::collections::HashMap<String, String>,
}

pub async fn handle_tunnel_message(
	ctx: &mut EnvoyContext,
	connection_session: u64,
	msg: protocol::ToEnvoyTunnelMessage,
) {
	let message_id = msg.message_id;
	let is_http_continuation = matches!(
		&msg.message_kind,
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(_)
			| protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(_)
			| protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestBodyCancel
			| protocol::ToEnvoyTunnelMessageKind::ToEnvoyResponseBodyWindowUpdate(_)
	);
	if is_http_continuation
		&& let Some(route) = ctx
			.http_request_routes
			.get(&[&message_id.gateway_id, &message_id.request_id])
		&& route.session != connection_session
	{
		handle_http_protocol_violation(
			ctx,
			connection_session,
			message_id,
			"HTTP request identifier belongs to another connection",
		)
		.await;
		return;
	}
	match msg.message_kind {
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(req) => {
			handle_request_start(ctx, connection_session, message_id, req).await;
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(chunk) => {
			if advance_http_message_index(ctx, &message_id) {
				handle_request_chunk(ctx, message_id, chunk).await;
			} else {
				handle_http_protocol_violation(
					ctx,
					connection_session,
					message_id,
					"invalid HTTP tunnel message sequence",
				)
				.await;
			}
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(abort) => {
			if advance_http_message_index(ctx, &message_id) {
				handle_request_abort(ctx, message_id, abort.reason);
			} else {
				handle_http_protocol_violation(
					ctx,
					connection_session,
					message_id,
					"invalid HTTP request-abort sequence",
				)
				.await;
			}
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestBodyCancel => {
			if advance_http_message_index(ctx, &message_id) {
				handle_request_body_cancel(ctx, message_id);
			} else {
				handle_http_protocol_violation(
					ctx,
					connection_session,
					message_id,
					"invalid HTTP request-body-cancel sequence",
				)
				.await;
			}
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyResponseBodyWindowUpdate(update) => {
			if advance_http_message_index(ctx, &message_id) {
				handle_response_body_window_update(ctx, message_id, update.consumed_bytes);
			} else {
				handle_http_protocol_violation(
					ctx,
					connection_session,
					message_id,
					"invalid HTTP response-window sequence",
				)
				.await;
			}
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(open) => {
			handle_ws_open(ctx, message_id, open).await;
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(msg) => {
			handle_ws_message(ctx, message_id, msg);
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(close) => {
			handle_ws_close(ctx, message_id, close);
		}
	}
}

async fn handle_http_protocol_violation(
	ctx: &mut EnvoyContext,
	connection_session: u64,
	message_id: protocol::MessageId,
	detail: &'static str,
) {
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	if let Some(route) = ctx.http_request_routes.get(&key) {
		if route.session != connection_session {
			send_response_abort_for_session(ctx, connection_session, message_id, detail).await;
			return;
		}
		let actor_id = route.actor_id.clone();
		if let Some(actor) = ctx.get_actor(&actor_id, None) {
			let _ = actor.handle.send(crate::actor::ToActor::ReqProtocolViolation {
				message_id: message_id.clone(),
				detail: detail.to_owned(),
			});
		}
		ctx.http_request_routes.remove(&key);
		ctx.http_message_indices.remove(&key);
		return;
	}

	send_response_abort_for_session(ctx, connection_session, message_id, detail).await;
}

async fn send_response_abort_for_session(
	ctx: &EnvoyContext,
	connection_session: u64,
	mut message_id: protocol::MessageId,
	detail: &'static str,
) {
	message_id.message_index = 0;
	let _ = crate::connection::ws_send_http_for_session(
		&ctx.shared,
		protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
			message_id,
			message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
				protocol::ToRivetResponseAbort {
					reason: protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::InternalError,
						detail: Some(detail.to_owned()),
					},
				},
			),
		}),
		connection_session,
	)
	.await;
}

fn handle_request_body_cancel(ctx: &mut EnvoyContext, message_id: protocol::MessageId) {
	let actor_id = ctx
		.http_request_routes
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.map(|route| route.actor_id.clone());
	if let Some(actor_id) = &actor_id
		&& let Some(actor) = ctx.get_actor(actor_id, None)
	{
		let _ = actor
			.handle
			.send(crate::actor::ToActor::ReqBodyCancel { message_id });
	}
}

fn handle_response_body_window_update(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	consumed_bytes: u64,
) {
	let actor_id = ctx
		.http_request_routes
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.map(|route| route.actor_id.clone());
	if let Some(actor_id) = &actor_id
		&& let Some(actor) = ctx.get_actor(actor_id, None)
	{
		let _ = actor
			.handle
			.send(crate::actor::ToActor::ResponseBodyWindowUpdate {
				message_id,
				consumed_bytes,
			});
	}
}

async fn handle_request_start(
	ctx: &mut EnvoyContext,
	connection_session: u64,
	message_id: protocol::MessageId,
	req: protocol::ToEnvoyRequestStart,
) {
	let actor_id = req.actor_id.clone();
	let has_actor = ctx.get_actor(&actor_id, None).is_some();

	if !has_actor {
		tracing::warn!(actor_id = %actor_id, "received request for unknown actor");
		send_error_response(
			ctx,
			req.response_stream.then_some(connection_session),
			message_id.gateway_id,
			message_id.request_id,
			"envoy.actor_not_found",
			"Actor not found",
		)
		.await;
		return;
	}
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	if message_id.message_index != 0 {
		tracing::warn!(
			message_index = message_id.message_index,
			"received invalid HTTP request start sequence"
		);
		send_response_abort_for_session(
			ctx,
			connection_session,
			message_id,
			"HTTP request start must use message index zero",
		)
		.await;
		return;
	}
	if let Some(existing) = ctx.http_request_routes.get(&key) {
		let same_session = existing.session == connection_session;
		handle_http_protocol_violation(
			ctx,
			connection_session,
			message_id,
			if same_session {
				"duplicate HTTP request start"
			} else {
				"HTTP request identifier belongs to another connection"
			},
		)
		.await;
		return;
	}
	ctx.http_message_indices
		.insert(&[&message_id.gateway_id, &message_id.request_id], 1);

	ctx.http_request_routes.insert(
		&[&message_id.gateway_id, &message_id.request_id],
		HttpRequestRoute {
			actor_id: actor_id.clone(),
			session: connection_session,
			gateway_id: message_id.gateway_id,
			request_id: message_id.request_id,
		},
	);

	let actor = ctx.get_actor(&actor_id, None).unwrap();
	let actor_handle = actor.handle.clone();
	let _ = actor_handle.send(crate::actor::ToActor::ReqStart {
		message_id: message_id.clone(),
		req,
		connection_session,
	});
}

async fn handle_request_chunk(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	chunk: protocol::ToEnvoyRequestChunk,
) {
	let actor_id = ctx
		.http_request_routes
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.map(|route| route.actor_id.clone());

	if let Some(actor_id) = &actor_id {
		if let Some(actor) = ctx.get_actor(actor_id, None) {
			let _ = actor.handle.send(crate::actor::ToActor::ReqChunk {
				message_id: message_id.clone(),
				chunk,
			});
		} else {
			tracing::warn!(actor_id = %actor_id, "received request chunk for unknown actor");
		}
	} else {
		tracing::warn!(
			gateway_id = ?message_id.gateway_id,
			request_id = ?message_id.request_id,
			message_index = message_id.message_index,
			"received request chunk without request start"
		);
		send_error_response(
			ctx,
			None,
			message_id.gateway_id,
			message_id.request_id,
			"envoy.request_not_found",
			"Request start was not delivered",
		)
		.await;
	}
}

fn handle_request_abort(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	reason: protocol::HttpStreamAbortReason,
) {
	let actor_id = ctx
		.http_request_routes
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.map(|route| route.actor_id.clone());
	if let Some(actor_id) = &actor_id {
		if let Some(actor) = ctx.get_actor(actor_id, None) {
			let _ = actor.handle.send(crate::actor::ToActor::ReqAbort {
				message_id: message_id.clone(),
				reason,
			});
		}
	}

	ctx.http_request_routes
		.remove(&[&message_id.gateway_id, &message_id.request_id]);
	ctx.http_message_indices
		.remove(&[&message_id.gateway_id, &message_id.request_id]);
}

async fn handle_ws_open(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	open: protocol::ToEnvoyWebSocketOpen,
) {
	let actor_id = open.actor_id.clone();
	let has_actor = ctx.get_actor(&actor_id, None).is_some();

	if !has_actor {
		tracing::warn!(actor_id = %actor_id, "received ws open for unknown actor");

		ws_send(
			&ctx.shared,
			protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
				message_id,
				message_kind: protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(
					protocol::ToRivetWebSocketClose {
						code: Some(1011),
						reason: Some("Actor not found".to_string()),
						hibernate: false,
					},
				),
			}),
		)
		.await;
		return;
	}

	ctx.request_to_actor.insert(
		&[&message_id.gateway_id, &message_id.request_id],
		actor_id.clone(),
	);
	ctx.shared
		.live_tunnel_requests
		.lock()
		.expect("shared live tunnel request registry poisoned")
		.insert(
			make_ws_key(&message_id.gateway_id, &message_id.request_id),
			actor_id.clone(),
		);

	// Convert HashMap headers to BTreeMap for the actor message
	let headers = open
		.headers
		.iter()
		.map(|(k, v)| (k.clone(), v.clone()))
		.collect();

	let actor = ctx.get_actor(&actor_id, None).unwrap();
	let _ = actor.handle.send(crate::actor::ToActor::WsOpen {
		message_id,
		path: open.path,
		headers,
	});
}

fn handle_ws_message(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	msg: protocol::ToEnvoyWebSocketMessage,
) {
	let actor_id = ctx
		.request_to_actor
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.cloned();
	if let Some(actor_id) = &actor_id {
		if let Some(actor) = ctx.get_actor(actor_id, None) {
			let _ = actor
				.handle
				.send(crate::actor::ToActor::WsMsg { message_id, msg });
		} else {
			ctx.buffered_actor_messages
				.entry(actor_id.clone())
				.or_default()
				.push(BufferedActorMessage::WsMsg { message_id, msg });
		}
	}
}

fn handle_ws_close(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	close: protocol::ToEnvoyWebSocketClose,
) {
	let actor_id = ctx
		.request_to_actor
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.cloned();
	if let Some(actor_id) = &actor_id {
		if let Some(actor) = ctx.get_actor(actor_id, None) {
			let _ = actor.handle.send(crate::actor::ToActor::WsClose {
				message_id: message_id.clone(),
				close,
			});
		} else {
			ctx.buffered_actor_messages
				.entry(actor_id.clone())
				.or_default()
				.push(BufferedActorMessage::WsClose {
					message_id: message_id.clone(),
					close,
				});
		}
	}

	ctx.request_to_actor
		.remove(&[&message_id.gateway_id, &message_id.request_id]);
	ctx.shared
		.live_tunnel_requests
		.lock()
		.expect("shared live tunnel request registry poisoned")
		.remove(&make_ws_key(&message_id.gateway_id, &message_id.request_id));
}

pub fn send_hibernatable_ws_message_ack(
	ctx: &mut EnvoyContext,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	envoy_message_index: u16,
) {
	let actor_id = ctx
		.request_to_actor
		.get(&[&gateway_id, &request_id])
		.cloned();
	if let Some(actor_id) = &actor_id {
		if let Some(actor) = ctx.get_actor(actor_id, None) {
			let _ = actor.handle.send(crate::actor::ToActor::HwsAck {
				gateway_id,
				request_id,
				envoy_message_index,
			});
		}
	}
}

pub async fn resend_buffered_tunnel_messages(ctx: &mut EnvoyContext) {
	if ctx.buffered_messages.is_empty() {
		return;
	}

	tracing::info!(
		count = ctx.buffered_messages.len(),
		"resending buffered tunnel messages"
	);

	let messages = std::mem::take(&mut ctx.buffered_messages);
	for msg in messages {
		ws_send(&ctx.shared, protocol::ToRivet::ToRivetTunnelMessage(msg)).await;
	}
}

async fn send_error_response(
	ctx: &EnvoyContext,
	connection_session: Option<u64>,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	error_code: &str,
	message: &str,
) {
	let body = message.as_bytes().to_vec();
	let mut headers = HashMap::new();
	headers.insert("x-rivet-error".to_string(), error_code.to_owned());
	headers.insert("content-length".to_string(), body.len().to_string());

	let response = protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
			message_id: protocol::MessageId {
				gateway_id,
				request_id,
				message_index: 0,
			},
			message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
				protocol::ToRivetResponseStart {
					status: 503,
					headers,
					body: Some(body),
					stream: false,
				},
			),
		});
	match connection_session {
		Some(session) => {
			let _ = crate::connection::ws_send_http_for_session(&ctx.shared, response, session).await;
		}
		None => {
			ws_send(&ctx.shared, response).await;
		}
	}
}
