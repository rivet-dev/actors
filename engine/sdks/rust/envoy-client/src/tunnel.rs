use rivet_envoy_protocol as protocol;
use std::collections::HashMap;

use crate::connection::ws_send;
use crate::envoy::{BufferedActorMessage, EnvoyContext};

fn make_ws_key(gateway_id: &protocol::GatewayId, request_id: &protocol::RequestId) -> [u8; 8] {
	let mut key = [0u8; 8];
	key[..4].copy_from_slice(gateway_id);
	key[4..].copy_from_slice(request_id);
	key
}

pub struct HibernatingWebSocketMetadata {
	pub gateway_id: protocol::GatewayId,
	pub request_id: protocol::RequestId,
	pub envoy_message_index: u16,
	pub rivet_message_index: u16,
	pub path: String,
	pub headers: std::collections::HashMap<String, String>,
}

pub async fn handle_tunnel_message(ctx: &mut EnvoyContext, msg: protocol::ToEnvoyTunnelMessage) {
	let message_id = msg.message_id;
	match msg.message_kind {
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(req) => {
			handle_request_start(ctx, message_id, req).await;
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(chunk) => {
			handle_request_chunk(ctx, message_id, chunk).await;
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(abort) => {
			handle_request_abort(ctx, message_id, abort.reason);
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

async fn handle_request_start(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	req: protocol::ToEnvoyRequestStart,
) {
	let actor_id = req.actor_id.clone();
	let has_actor = ctx.get_actor(&actor_id, None).is_some();

	if !has_actor {
		tracing::warn!(actor_id = %actor_id, "received request for unknown actor");
		send_error_response(
			ctx,
			message_id.gateway_id,
			message_id.request_id,
			"envoy.actor_not_found",
			"Actor not found",
		)
		.await;
		return;
	}

	ctx.request_to_actor.insert(
		&[&message_id.gateway_id, &message_id.request_id],
		actor_id.clone(),
	);

	let actor = ctx.get_actor(&actor_id, None).unwrap();
	let actor_handle = actor.handle.clone();
	let _ = actor_handle.send(crate::actor::ToActor::ReqStart {
		message_id: message_id.clone(),
		req,
	});
}

async fn handle_request_chunk(
	ctx: &mut EnvoyContext,
	message_id: protocol::MessageId,
	chunk: protocol::ToEnvoyRequestChunk,
) {
	let actor_id = ctx
		.request_to_actor
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.cloned();

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
		.request_to_actor
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.cloned();
	if let Some(actor_id) = &actor_id {
		if let Some(actor) = ctx.get_actor(actor_id, None) {
			let _ = actor.handle.send(crate::actor::ToActor::ReqAbort {
				message_id: message_id.clone(),
				reason,
			});
		}
	}

	ctx.request_to_actor
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
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	error_code: &str,
	message: &str,
) {
	let body = message.as_bytes().to_vec();
	let mut headers = HashMap::new();
	headers.insert("x-rivet-error".to_string(), error_code.to_owned());
	headers.insert("content-length".to_string(), body.len().to_string());

	ws_send(
		&ctx.shared,
		protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
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
		}),
	)
	.await;
}
