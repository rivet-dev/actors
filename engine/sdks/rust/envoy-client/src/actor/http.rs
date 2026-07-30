use std::{collections::HashMap, sync::Arc};

use rivet_envoy_protocol as protocol;
use tokio::{
	sync::{mpsc, watch},
	task::{AbortHandle, JoinError, JoinSet},
};
use tracing::Instrument;

use super::{ActorContext, ToActor};
use crate::{
	async_counter::AsyncCounter,
	connection::ws_send,
	handle::EnvoyHandle,
	http::{
		HTTP_BODY_MAX_CHUNK_SIZE, HTTP_BODY_STREAM_CHANNEL_CAPACITY, HttpRequest,
		HttpRequestBodyError, HttpRequestBodyStream, HttpResponse, ResponseChunk,
	},
	utils::{BufferMap, spawn_detached},
};

pub(super) struct HttpRequests {
	pending: BufferMap<PendingHttpRequest>,
}

impl HttpRequests {
	pub(super) fn new() -> Self {
		Self {
			pending: BufferMap::new(),
		}
	}
}

struct PendingHttpRequest {
	body_tx: Option<mpsc::Sender<Vec<u8>>>,
	body_abort_tx: Option<watch::Sender<Option<HttpRequestBodyError>>>,
	task_abort_handle: Option<AbortHandle>,
	body_rejected: bool,
	upload_complete: bool,
	response_complete: bool,
}

enum RequestPhase {
	Upload,
	Response,
}

/// Counts one HTTP request task from dispatch through the full response drain.
///
/// This guard is created before invoking the runtime callback and is held across
/// `send_response`, including streaming response drains. Sleep and shutdown
/// logic relies on this counter staying non-zero until the final response chunk
/// is sent or the task is aborted.
pub(super) struct ActiveHttpRequestGuard {
	active_http_request_count: Arc<AsyncCounter>,
}

impl ActiveHttpRequestGuard {
	pub(super) fn new(active_http_request_count: Arc<AsyncCounter>) -> Self {
		active_http_request_count.increment();
		Self {
			active_http_request_count,
		}
	}
}

impl Drop for ActiveHttpRequestGuard {
	fn drop(&mut self) {
		self.active_http_request_count.decrement();
	}
}

pub(super) fn handle_req_start(
	ctx: &mut ActorContext,
	handle: &EnvoyHandle,
	http_request_tasks: &mut JoinSet<()>,
	message_id: protocol::MessageId,
	req: protocol::ToEnvoyRequestStart,
) {
	let pending = PendingHttpRequest {
		body_tx: None,
		body_abort_tx: None,
		task_abort_handle: None,
		body_rejected: false,
		upload_complete: !req.stream,
		response_complete: false,
	};
	ctx.http_requests
		.pending
		.insert(&[&message_id.gateway_id, &message_id.request_id], pending);

	let headers: HashMap<String, String> = req.headers.into_iter().collect();
	let body_stream = if req.stream {
		let (body_tx, body_rx) = mpsc::channel(HTTP_BODY_STREAM_CHANNEL_CAPACITY);
		let (body_abort_tx, body_abort_rx) = watch::channel(None);
		if let Some(pending) = ctx
			.http_requests
			.pending
			.get_mut(&[&message_id.gateway_id, &message_id.request_id])
		{
			pending.body_tx = Some(body_tx);
			pending.body_abort_tx = Some(body_abort_tx);
		}
		Some(HttpRequestBodyStream::new(body_rx, body_abort_rx))
	} else {
		None
	};

	let request = HttpRequest {
		method: req.method,
		path: req.path,
		headers,
		body: req.body,
		body_stream,
	};

	let shared = ctx.shared.clone();
	let handle = handle.clone();
	let actor_id = ctx.actor_id.clone();
	let gateway_id = message_id.gateway_id;
	let request_id = message_id.request_id;
	let request_guard = ActiveHttpRequestGuard::new(ctx.active_http_request_count.clone());
	let actor_tx = ctx.tx.clone();
	let completion_message_id = message_id.clone();

	let task = async move {
		let _request_guard = request_guard;
		match shared
			.config
			.callbacks
			.fetch(handle, actor_id, gateway_id, request_id, request)
			.await
		{
			Ok(response) => send_response(&shared, gateway_id, request_id, response).await,
			Err(error) => {
				tracing::error!(?error, "fetch failed");
				send_fetch_error_response(&shared, gateway_id, request_id).await;
			}
		}
		let _ = actor_tx.send(ToActor::ReqComplete {
			message_id: completion_message_id,
		});
	}
	.in_current_span();

	#[cfg(target_arch = "wasm32")]
	let task_abort_handle = http_request_tasks.spawn_local(task);
	#[cfg(not(target_arch = "wasm32"))]
	let task_abort_handle = http_request_tasks.spawn(task);
	if let Some(pending) = ctx
		.http_requests
		.pending
		.get_mut(&[&message_id.gateway_id, &message_id.request_id])
	{
		pending.task_abort_handle = Some(task_abort_handle);
	}
}

pub(super) fn handle_task_result(result: Result<(), JoinError>) {
	if let Err(error) = result {
		if error.is_cancelled() {
			return;
		}

		tracing::error!(?error, "http request task failed");
	}
}

pub(super) async fn abort_and_join_tasks(
	ctx: &mut ActorContext,
	http_request_tasks: &mut JoinSet<()>,
) {
	if http_request_tasks.is_empty() {
		return;
	}

	let active_http_request_count = ctx.active_http_request_count.load();
	tracing::debug!(
		active_http_request_count,
		"aborting in-flight http request tasks"
	);

	http_request_tasks.abort_all();
	while let Some(result) = http_request_tasks.join_next().await {
		handle_task_result(result);
	}
}

pub(super) fn handle_req_chunk(
	ctx: &mut ActorContext,
	message_id: protocol::MessageId,
	chunk: protocol::ToEnvoyRequestChunk,
) {
	let finish = chunk.finish;
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	let mut response_aborted = false;
	match ctx.http_requests.pending.get_mut(&key) {
		Some(pending) if pending.body_rejected => {}
		Some(pending) => {
			if !chunk.body.is_empty() {
				if let Some(body_tx) = pending.body_tx.clone() {
					match body_tx.try_send(chunk.body) {
						Ok(()) => {}
						Err(mpsc::error::TrySendError::Closed(_)) => {
							tracing::debug!("streamed request body consumer closed");
							pending.body_tx = None;
						}
						Err(mpsc::error::TrySendError::Full(_)) => {
							let reason = protocol::HttpStreamAbortReason {
								kind: protocol::HttpStreamAbortReasonKind::Overloaded,
								detail: Some(
									"request body consumer did not keep up with the upload"
										.to_owned(),
								),
							};
							tracing::warn!("streamed request body channel overloaded");
							reject_pending_request(pending, &reason);
							response_aborted = true;

							let shared = ctx.shared.clone();
							let gateway_id = message_id.gateway_id;
							let request_id = message_id.request_id;
							spawn_detached(async move {
								send_response_abort(&shared, gateway_id, request_id, 0, reason)
									.await;
							});
						}
					}
				} else {
					tracing::warn!("received chunk for pending request without stream controller");
					if !finish {
						return;
					}
				}
			}
		}
		None => {
			tracing::warn!(
				gateway_id = ?message_id.gateway_id,
				request_id = ?message_id.request_id,
				message_index = message_id.message_index,
				"received request chunk without an active request"
			);
			let shared = ctx.shared.clone();
			let gateway_id = message_id.gateway_id;
			let request_id = message_id.request_id;
			spawn_detached(async move {
				send_response_abort(
					&shared,
					gateway_id,
					request_id,
					0,
					protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::HandlerError,
						detail: Some("request start was not delivered".to_owned()),
					},
				)
				.await;
			});
			return;
		}
	}

	if finish {
		complete_phase(ctx, message_id.clone(), RequestPhase::Upload);
	}
	if response_aborted {
		complete_phase(ctx, message_id, RequestPhase::Response);
	}
}

fn reject_pending_request(
	pending: &mut PendingHttpRequest,
	reason: &protocol::HttpStreamAbortReason,
) {
	if let Some(body_abort_tx) = pending.body_abort_tx.take() {
		body_abort_tx.send_replace(Some(HttpRequestBodyError {
			reason: reason.clone(),
		}));
	}
	if let Some(task_abort_handle) = pending.task_abort_handle.take() {
		task_abort_handle.abort();
	}
	pending.body_tx = None;
	pending.body_rejected = true;
}

pub(super) fn handle_req_complete(ctx: &mut ActorContext, message_id: protocol::MessageId) {
	complete_phase(ctx, message_id, RequestPhase::Response);
}

fn complete_phase(ctx: &mut ActorContext, message_id: protocol::MessageId, phase: RequestPhase) {
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	let Some(pending) = ctx.http_requests.pending.get_mut(&key) else {
		return;
	};
	match phase {
		RequestPhase::Upload => {
			pending.upload_complete = true;
			pending.body_tx = None;
			pending.body_abort_tx = None;
		}
		RequestPhase::Response => pending.response_complete = true,
	}
	if !pending.upload_complete || !pending.response_complete {
		return;
	}

	ctx.http_requests.pending.remove(&key);
	let _ = crate::envoy::send_to_envoy_tx(
		&ctx.shared,
		crate::envoy::ToEnvoyMessage::HttpRequestComplete {
			gateway_id: message_id.gateway_id,
			request_id: message_id.request_id,
		},
	);
}

pub(super) fn handle_req_abort(
	ctx: &mut ActorContext,
	message_id: protocol::MessageId,
	reason: protocol::HttpStreamAbortReason,
) {
	if let Some(mut pending) = ctx
		.http_requests
		.pending
		.remove(&[&message_id.gateway_id, &message_id.request_id])
	{
		if let Some(body_abort_tx) = pending.body_abort_tx.take() {
			body_abort_tx.send_replace(Some(HttpRequestBodyError { reason }));
		}
		if let Some(task_abort_handle) = pending.task_abort_handle.take() {
			task_abort_handle.abort();
		}
	}
}

async fn send_response(
	shared: &crate::context::SharedContext,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	response: HttpResponse,
) {
	let HttpResponse {
		status,
		mut headers,
		body,
		mut body_stream,
	} = response;
	let is_streaming = body_stream.is_some();
	if !is_streaming {
		if let Some(body) = &body {
			if !headers.contains_key("content-length") {
				headers.insert("content-length".to_owned(), body.len().to_string());
			}
		}
	}

	ws_send(
		shared,
		protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
			message_id: protocol::MessageId {
				gateway_id,
				request_id,
				message_index: 0,
			},
			message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
				protocol::ToRivetResponseStart {
					status,
					headers,
					body,
					stream: is_streaming,
				},
			),
		}),
	)
	.await;

	let Some(body_stream) = body_stream.as_mut() else {
		return;
	};
	let mut message_index: u16 = 1;
	let mut saw_finish = false;
	while let Some(chunk) = body_stream.recv().await {
		let finish = match chunk {
			ResponseChunk::Data { data, finish } => {
				send_response_data_chunks(
					shared,
					gateway_id,
					request_id,
					&mut message_index,
					data,
					finish,
				)
				.await;
				finish
			}
			ResponseChunk::Error(detail) => {
				send_response_abort(
					shared,
					gateway_id,
					request_id,
					message_index,
					protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::HandlerError,
						detail: Some(detail),
					},
				)
				.await;
				true
			}
		};
		if finish {
			saw_finish = true;
			break;
		}
	}
	if !saw_finish {
		send_response_abort(
			shared,
			gateway_id,
			request_id,
			message_index,
			protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::HandlerError,
				detail: Some("response body stream closed before finish".to_owned()),
			},
		)
		.await;
	}
}

async fn send_response_data_chunks(
	shared: &crate::context::SharedContext,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	message_index: &mut u16,
	data: Vec<u8>,
	finish: bool,
) {
	if data.is_empty() {
		send_response_chunk(shared, gateway_id, request_id, *message_index, data, finish).await;
		*message_index = (*message_index).wrapping_add(1);
		return;
	}

	let total_len = data.len();
	for (idx, chunk) in data.chunks(HTTP_BODY_MAX_CHUNK_SIZE).enumerate() {
		let chunk_finish = finish && (idx + 1) * HTTP_BODY_MAX_CHUNK_SIZE >= total_len;
		send_response_chunk(
			shared,
			gateway_id,
			request_id,
			*message_index,
			chunk.to_vec(),
			chunk_finish,
		)
		.await;
		*message_index = (*message_index).wrapping_add(1);
	}
}

async fn send_response_chunk(
	shared: &crate::context::SharedContext,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	message_index: u16,
	body: Vec<u8>,
	finish: bool,
) {
	ws_send(
		shared,
		protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
			message_id: protocol::MessageId {
				gateway_id,
				request_id,
				message_index,
			},
			message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(
				protocol::ToRivetResponseChunk { body, finish },
			),
		}),
	)
	.await;
}

async fn send_response_abort(
	shared: &crate::context::SharedContext,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	message_index: u16,
	reason: protocol::HttpStreamAbortReason,
) {
	ws_send(
		shared,
		protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
			message_id: protocol::MessageId {
				gateway_id,
				request_id,
				message_index,
			},
			message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
				protocol::ToRivetResponseAbort { reason },
			),
		}),
	)
	.await;
}

async fn send_fetch_error_response(
	shared: &crate::context::SharedContext,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
) {
	let body =
		br#"{"group":"envoy","code":"fetch_failed","message":"actor fetch failed","metadata":{}}"#
			.to_vec();
	let headers = HashMap::from([
		("content-type".to_owned(), "application/json".to_owned()),
		("content-length".to_owned(), body.len().to_string()),
		("x-rivet-error".to_owned(), "envoy.fetch_failed".to_owned()),
	]);

	ws_send(
		shared,
		protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
			message_id: protocol::MessageId {
				gateway_id,
				request_id,
				message_index: 0,
			},
			message_kind: protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
				protocol::ToRivetResponseStart {
					status: 500,
					headers,
					body: Some(body),
					stream: false,
				},
			),
		}),
	)
	.await;
}
