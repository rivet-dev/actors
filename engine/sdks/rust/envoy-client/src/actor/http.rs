use std::{
	collections::HashMap,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use rivet_envoy_protocol as protocol;
use tokio::{
	sync::{Mutex, mpsc, watch},
	task::{AbortHandle, JoinError, JoinSet},
};
use tracing::Instrument;

use super::{ActorContext, ToActor};
use crate::{
	async_counter::AsyncCounter,
	connection::ws_send,
	handle::EnvoyHandle,
	http::{
		HTTP_BODY_MAX_CHUNK_SIZE, HTTP_BODY_STREAM_CHANNEL_CAPACITY, HttpBodySendWindow,
		HttpRequest, HttpRequestBodyError, HttpRequestBodyQueue, HttpRequestBodyStream,
		HttpResponse, RequestBodyEvent, ResponseChunk,
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
	message_id: protocol::MessageId,
	connection_session: Option<u64>,
	body_tx: Option<mpsc::Sender<Vec<u8>>>,
	body_abort_tx: Option<watch::Sender<Option<HttpRequestBodyError>>>,
	body_queue: Option<Arc<HttpRequestBodyQueue>>,
	task_abort_handle: Option<AbortHandle>,
	body_rejected: bool,
	upload_complete: bool,
	response_complete: bool,
	tunnel_sender: HttpTunnelSender,
	request_body_window: Option<Arc<RequestBodyReceiveWindow>>,
	response_body_window: Arc<HttpBodySendWindow>,
}

#[derive(Clone)]
struct HttpTunnelSender {
	shared: Arc<crate::context::SharedContext>,
	gateway_id: protocol::GatewayId,
	request_id: protocol::RequestId,
	message_index: Arc<Mutex<protocol::MessageIndex>>,
	connection_session: Option<u64>,
	terminal_queued: Arc<AtomicBool>,
}

impl HttpTunnelSender {
	fn new(
		shared: Arc<crate::context::SharedContext>,
		gateway_id: protocol::GatewayId,
		request_id: protocol::RequestId,
		connection_session: Option<u64>,
	) -> Self {
		Self {
			shared,
			gateway_id,
			request_id,
			message_index: Arc::new(Mutex::new(0)),
			connection_session,
			terminal_queued: Arc::new(AtomicBool::new(false)),
		}
	}

	async fn send(&self, message_kind: protocol::ToRivetTunnelMessageKind) -> bool {
		let is_terminal = match &message_kind {
			protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(response) => !response.stream,
			protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => chunk.finish,
			protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_) => true,
			_ => false,
		};
		let mut message_index = self.message_index.lock().await;
		let message = protocol::ToRivet::ToRivetTunnelMessage(protocol::ToRivetTunnelMessage {
				message_id: protocol::MessageId {
					gateway_id: self.gateway_id,
					request_id: self.request_id,
					message_index: *message_index,
				},
				message_kind,
			});
		let failed = match self.connection_session {
			Some(session) => !matches!(
				crate::connection::ws_send_http_for_session(&self.shared, message, session).await,
				crate::connection::WsSendResult::Sent { .. }
			),
			None => ws_send(&self.shared, message).await,
		};
		if !failed {
			*message_index = message_index.wrapping_add(1);
			if is_terminal {
				self.terminal_queued.store(true, Ordering::Release);
			}
		}
		drop(message_index);
		if failed && self.connection_session.is_some() {
			self.queue_transport_abort().await;
		}
		failed
	}

	async fn queue_terminal(&self, message_kind: protocol::ToRivetTunnelMessageKind) {
		let mut message_index = self.message_index.lock().await;
		if self.terminal_queued.swap(true, Ordering::AcqRel) {
			return;
		}
		let msg = protocol::ToRivetTunnelMessage {
			message_id: protocol::MessageId {
				gateway_id: self.gateway_id,
				request_id: self.request_id,
				message_index: *message_index,
			},
			message_kind,
		};
		*message_index = message_index.wrapping_add(1);
		drop(message_index);

		let _ = crate::envoy::send_to_envoy_tx(
			&self.shared,
			crate::envoy::ToEnvoyMessage::SendOrBufferTunnelMsg { msg },
		);
	}

	async fn queue_transport_abort(&self) {
		self.queue_terminal(
			protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
				protocol::ToRivetResponseAbort {
					reason: protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::InternalError,
						detail: Some("gateway tunnel connection closed".to_owned()),
					},
				},
			),
		)
		.await;
	}
}

#[derive(Debug)]
struct RequestBodyReceiveWindow {
	received_bytes: std::sync::atomic::AtomicU64,
	consumed_bytes: std::sync::atomic::AtomicU64,
}

impl RequestBodyReceiveWindow {
	fn new() -> Arc<Self> {
		Arc::new(Self {
			received_bytes: std::sync::atomic::AtomicU64::new(0),
			consumed_bytes: std::sync::atomic::AtomicU64::new(0),
		})
	}

	fn receive(&self, bytes: u64) -> anyhow::Result<()> {
		anyhow::ensure!(
			bytes <= HTTP_BODY_MAX_CHUNK_SIZE as u64,
			"HTTP request body frame exceeds maximum size"
		);
		self.received_bytes
			.fetch_update(
				std::sync::atomic::Ordering::AcqRel,
				std::sync::atomic::Ordering::Acquire,
				|received| {
					let next = received.checked_add(bytes)?;
					let consumed = self
						.consumed_bytes
						.load(std::sync::atomic::Ordering::Acquire);
					(next.checked_sub(consumed)?
						<= protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES)
						.then_some(next)
				},
			)
			.map_err(|_| anyhow::anyhow!("HTTP request body window exceeded"))?;
		Ok(())
	}

	fn consume(&self, bytes: u64) -> anyhow::Result<u64> {
		let previous = self
			.consumed_bytes
			.fetch_update(
				std::sync::atomic::Ordering::AcqRel,
				std::sync::atomic::Ordering::Acquire,
				|consumed| {
					let next = consumed.checked_add(bytes)?;
					(next
						<= self
							.received_bytes
							.load(std::sync::atomic::Ordering::Acquire))
					.then_some(next)
				},
			)
			.map_err(|_| anyhow::anyhow!("HTTP request body consumed-byte counter is invalid"))?;
		previous
			.checked_add(bytes)
			.ok_or_else(|| anyhow::anyhow!("HTTP request body consumed-byte counter overflow"))
	}
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
	connection_session: u64,
) {
	let tunnel_sender = HttpTunnelSender::new(
		ctx.shared.clone(),
		message_id.gateway_id,
		message_id.request_id,
		req.response_stream.then_some(connection_session),
	);
	let response_stream = req.response_stream;
	let request_body_window = (req.stream && response_stream).then(RequestBodyReceiveWindow::new);
	let response_body_window = HttpBodySendWindow::new();
	let pending = PendingHttpRequest {
		message_id: message_id.clone(),
		connection_session: response_stream.then_some(connection_session),
		body_tx: None,
		body_abort_tx: None,
		body_queue: None,
		task_abort_handle: None,
		body_rejected: false,
		upload_complete: !req.stream,
		response_complete: false,
		tunnel_sender: tunnel_sender.clone(),
		request_body_window: request_body_window.clone(),
		response_body_window: response_body_window.clone(),
	};
	ctx.http_requests
		.pending
		.insert(&[&message_id.gateway_id, &message_id.request_id], pending);

	let headers: HashMap<String, String> = req.headers.into_iter().collect();
	let body_stream = if req.stream {
		let (body_event_tx, body_event_rx) = mpsc::unbounded_channel();
		if let Some(request_body_window) = request_body_window.clone() {
			let body_queue = HttpRequestBodyQueue::new();
			if let Some(pending) = ctx
				.http_requests
				.pending
				.get_mut(&[&message_id.gateway_id, &message_id.request_id])
			{
				pending.body_queue = Some(body_queue.clone());
			}
			let actor_tx = ctx.tx.clone();
			let completion_message_id = message_id.clone();
			spawn_detached(handle_request_body_events(
				tunnel_sender.clone(),
				request_body_window,
				body_event_rx,
				actor_tx,
				completion_message_id,
			));
			Some(HttpRequestBodyStream::new_with_flow_control(
				body_queue,
				body_event_tx,
			))
		} else {
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
		}
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
			Ok(response) => {
				send_response(
					&tunnel_sender,
					response_stream,
					&response_body_window,
					response,
				)
				.await
			}
			Err(error) => {
				tracing::error!(?error, "fetch failed");
				send_fetch_error_response(&tunnel_sender).await;
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

async fn handle_request_body_events(
	tunnel_sender: HttpTunnelSender,
	window: Arc<RequestBodyReceiveWindow>,
	mut event_rx: mpsc::UnboundedReceiver<RequestBodyEvent>,
	actor_tx: mpsc::UnboundedSender<ToActor>,
	message_id: protocol::MessageId,
) {
	while let Some(event) = event_rx.recv().await {
		match event {
			RequestBodyEvent::Consumed(bytes) => {
				let consumed_bytes = match window.consume(bytes) {
					Ok(consumed_bytes) => consumed_bytes,
					Err(error) => {
						tracing::warn!(?error, "invalid HTTP request body consumption");
						return;
					}
				};
				if tunnel_sender
					.send(
						protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyWindowUpdate(
							protocol::ToRivetRequestBodyWindowUpdate { consumed_bytes },
						),
					)
					.await
				{
					return;
				}
			}
			RequestBodyEvent::Cancelled => {
				let _ = tunnel_sender
					.send(protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyCancel)
					.await;
				let _ = actor_tx.send(ToActor::ReqBodyCancelled { message_id });
				return;
			}
		}
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
				if let Some(window) = &pending.request_body_window
					&& let Err(error) = window.receive(chunk.body.len() as u64)
				{
					let reason = protocol::HttpStreamAbortReason {
						kind: protocol::HttpStreamAbortReasonKind::InternalError,
						detail: Some(error.to_string()),
					};
					let tunnel_sender = pending.tunnel_sender.clone();
					reject_pending_request(pending, &reason);
					spawn_detached(async move {
						send_response_abort(&tunnel_sender, reason).await;
					});
					complete_phase(ctx, message_id.clone(), RequestPhase::Upload);
					complete_phase(ctx, message_id, RequestPhase::Response);
					return;
				}
				if let Some(body_queue) = &pending.body_queue {
					if !body_queue.push(chunk.body) {
						tracing::debug!("streamed request body consumer closed");
						pending.body_queue = None;
					}
				} else if let Some(body_tx) = pending.body_tx.clone() {
					match body_tx.try_send(chunk.body) {
						Ok(()) => {}
						Err(mpsc::error::TrySendError::Closed(_)) => {
							tracing::debug!("streamed request body consumer closed");
							pending.body_tx = None;
						}
						Err(mpsc::error::TrySendError::Full(_)) => {
							let reason = protocol::HttpStreamAbortReason {
								kind: protocol::HttpStreamAbortReasonKind::InternalError,
								detail: Some(
									"request body consumer did not keep up with the upload"
										.to_owned(),
								),
							};
							tracing::warn!("streamed request body channel overloaded");
							reject_pending_request(pending, &reason);
							response_aborted = true;

							let tunnel_sender = pending.tunnel_sender.clone();
							spawn_detached(async move {
								send_response_abort(&tunnel_sender, reason).await;
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
			return;
		}
	}

	if finish {
		if let Some(pending) = ctx.http_requests.pending.get_mut(&key)
			&& let Some(body_queue) = &pending.body_queue
		{
			body_queue.finish();
		}
		complete_phase(ctx, message_id.clone(), RequestPhase::Upload);
	}
	if response_aborted {
		complete_phase(ctx, message_id, RequestPhase::Response);
	}
}

pub(super) fn handle_req_body_cancelled(ctx: &mut ActorContext, message_id: protocol::MessageId) {
	complete_phase(ctx, message_id, RequestPhase::Upload);
}

pub(super) async fn handle_response_body_window_update(
	ctx: &mut ActorContext,
	message_id: protocol::MessageId,
	consumed_bytes: u64,
) {
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	let (window, tunnel_sender) = {
		let Some(pending) = ctx.http_requests.pending.get_mut(&key) else {
			return;
		};
		(
			pending.response_body_window.clone(),
			pending.tunnel_sender.clone(),
		)
	};

	if let Err(error) = window.update_consumed(consumed_bytes).await {
		tracing::warn!(?error, "invalid HTTP response body window update");
		let reason = protocol::HttpStreamAbortReason {
			kind: protocol::HttpStreamAbortReasonKind::InternalError,
			detail: Some(error.to_string()),
		};
		handle_req_abort(ctx, message_id, reason.clone());
		send_response_abort(&tunnel_sender, reason).await;
	}
}

pub(super) fn handle_req_body_cancel(
	ctx: &mut ActorContext,
	message_id: protocol::MessageId,
) {
	let key: [&[u8]; 2] = [&message_id.gateway_id, &message_id.request_id];
	if let Some(pending) = ctx.http_requests.pending.get_mut(&key) {
		let reason = protocol::HttpStreamAbortReason {
			kind: protocol::HttpStreamAbortReasonKind::Cancelled,
			detail: Some("gateway cancelled the request body".to_owned()),
		};
		if let Some(body_queue) = pending.body_queue.take() {
			body_queue.abort(HttpRequestBodyError {
				reason: reason.clone(),
			});
		}
		if let Some(body_abort_tx) = pending.body_abort_tx.take() {
			body_abort_tx.send_replace(Some(HttpRequestBodyError {
				reason,
			}));
		}
		pending.body_tx = None;
	}
	complete_phase(ctx, message_id, RequestPhase::Upload);
}

fn reject_pending_request(
	pending: &mut PendingHttpRequest,
	reason: &protocol::HttpStreamAbortReason,
) {
	if let Some(body_queue) = pending.body_queue.take() {
		body_queue.abort(HttpRequestBodyError {
			reason: reason.clone(),
		});
	}
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
			pending.body_queue = None;
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
		if let Some(body_queue) = pending.body_queue.take() {
			body_queue.abort(HttpRequestBodyError {
				reason: reason.clone(),
			});
		}
		if let Some(body_abort_tx) = pending.body_abort_tx.take() {
			body_abort_tx.send_replace(Some(HttpRequestBodyError { reason }));
		}
		if let Some(task_abort_handle) = pending.task_abort_handle.take() {
			task_abort_handle.abort();
		}
	}
}

pub(super) async fn handle_protocol_violation(
	ctx: &mut ActorContext,
	message_id: protocol::MessageId,
	detail: String,
) {
	let tunnel_sender = ctx
		.http_requests
		.pending
		.get(&[&message_id.gateway_id, &message_id.request_id])
		.map(|pending| pending.tunnel_sender.clone());
	let reason = protocol::HttpStreamAbortReason {
		kind: protocol::HttpStreamAbortReasonKind::InternalError,
		detail: Some(detail),
	};
	handle_req_abort(ctx, message_id, reason.clone());
	if let Some(tunnel_sender) = tunnel_sender {
		send_response_abort(&tunnel_sender, reason).await;
	}
}

pub(super) async fn handle_connection_closed(ctx: &mut ActorContext, session: u64) {
	let reason = protocol::HttpStreamAbortReason {
		kind: protocol::HttpStreamAbortReasonKind::InternalError,
		detail: Some("gateway tunnel connection closed".to_owned()),
	};
	let requests = ctx
		.http_requests
		.pending
		.remove_where(|pending| pending.connection_session == Some(session));

	for mut pending in requests {
		if let Some(task_abort_handle) = pending.task_abort_handle.take() {
			task_abort_handle.abort();
		}
		if let Some(body_queue) = pending.body_queue.take() {
			body_queue.abort(HttpRequestBodyError {
				reason: reason.clone(),
			});
		}
		if let Some(body_abort_tx) = pending.body_abort_tx.take() {
			body_abort_tx.send_replace(Some(HttpRequestBodyError {
				reason: reason.clone(),
			}));
		}
		pending.tunnel_sender.queue_transport_abort().await;
		let _ = crate::envoy::send_to_envoy_tx(
			&ctx.shared,
			crate::envoy::ToEnvoyMessage::HttpRequestComplete {
				gateway_id: pending.message_id.gateway_id,
				request_id: pending.message_id.request_id,
			},
		);
	}
}

async fn send_response(
	tunnel_sender: &HttpTunnelSender,
	response_stream_supported: bool,
	response_body_window: &HttpBodySendWindow,
	response: HttpResponse,
) {
	let HttpResponse {
		status,
		mut headers,
		body,
		mut body_stream,
	} = response;
	if body_stream.is_some() && !response_stream_supported {
		let mut buffered = body.unwrap_or_default();
		let mut saw_finish = false;
		while let Some(chunk) = body_stream.as_mut().unwrap().recv().await {
			match chunk {
				ResponseChunk::Data { data, finish } => {
					let Some(next_len) = buffered.len().checked_add(data.len()) else {
						send_fetch_error_response(tunnel_sender).await;
						return;
					};
					if next_len > 20 * 1024 * 1024 {
						send_fetch_error_response(tunnel_sender).await;
						return;
					}
					buffered.extend_from_slice(&data);
					if finish {
						saw_finish = true;
						break;
					}
				}
				ResponseChunk::Error(_) => {
					send_fetch_error_response(tunnel_sender).await;
					return;
				}
			}
		}
		if !saw_finish {
			send_fetch_error_response(tunnel_sender).await;
			return;
		}
		if !headers.contains_key("content-length") {
			headers.insert("content-length".to_owned(), buffered.len().to_string());
		}
		let _ = tunnel_sender
			.send(
				protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
					protocol::ToRivetResponseStart {
						status,
						headers,
						body: Some(buffered),
						stream: false,
					},
				),
			)
			.await;
		return;
	}

	let is_streaming = body_stream.is_some();
	if !is_streaming {
		if let Some(body) = &body {
			if !headers.contains_key("content-length") {
				headers.insert("content-length".to_owned(), body.len().to_string());
			}
		}
	}

	if tunnel_sender
		.send(
			protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
				protocol::ToRivetResponseStart {
					status,
					headers,
					body: if is_streaming { None } else { body.clone() },
					stream: is_streaming,
				},
			),
		)
		.await
	{
		return;
	}

	let Some(body_stream) = body_stream.as_mut() else {
		return;
	};
	if let Some(body) = body.filter(|body| !body.is_empty())
		&& send_response_data_chunks(tunnel_sender, response_body_window, body, false).await
	{
		return;
	}
	let mut saw_finish = false;
	while let Some(chunk) = body_stream.recv().await {
		let finish = match chunk {
			ResponseChunk::Data { data, finish } => {
				if send_response_data_chunks(
					tunnel_sender,
					response_body_window,
					data,
					finish,
				)
				.await
				{
					return;
				}
				finish
			}
			ResponseChunk::Error(detail) => {
				send_response_abort(
					tunnel_sender,
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
			tunnel_sender,
			protocol::HttpStreamAbortReason {
				kind: protocol::HttpStreamAbortReasonKind::HandlerError,
				detail: Some("response body stream closed before finish".to_owned()),
			},
		)
		.await;
	}
}

async fn send_response_data_chunks(
	tunnel_sender: &HttpTunnelSender,
	response_body_window: &HttpBodySendWindow,
	data: Vec<u8>,
	finish: bool,
) -> bool {
	if data.is_empty() {
		return send_response_chunk(tunnel_sender, data, finish).await;
	}

	let total_len = data.len();
	for (idx, chunk) in data.chunks(HTTP_BODY_MAX_CHUNK_SIZE).enumerate() {
		let chunk_finish = finish && (idx + 1) * HTTP_BODY_MAX_CHUNK_SIZE >= total_len;
		if response_body_window.reserve(chunk.len() as u64).await.is_err()
			|| send_response_chunk(tunnel_sender, chunk.to_vec(), chunk_finish).await
		{
			return true;
		}
	}
	false
}

async fn send_response_chunk(
	tunnel_sender: &HttpTunnelSender,
	body: Vec<u8>,
	finish: bool,
) -> bool {
	tunnel_sender
		.send(
			protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(
				protocol::ToRivetResponseChunk { body, finish },
			),
		)
		.await
}

async fn send_response_abort(
	tunnel_sender: &HttpTunnelSender,
	reason: protocol::HttpStreamAbortReason,
) {
	let _ = tunnel_sender
		.send(
			protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(
				protocol::ToRivetResponseAbort { reason },
			),
		)
		.await;
}

async fn send_fetch_error_response(tunnel_sender: &HttpTunnelSender) {
	let body =
		br#"{"group":"envoy","code":"fetch_failed","message":"actor fetch failed","metadata":{}}"#
			.to_vec();
	let headers = HashMap::from([
		("content-type".to_owned(), "application/json".to_owned()),
		("content-length".to_owned(), body.len().to_string()),
		("x-rivet-error".to_owned(), "envoy.fetch_failed".to_owned()),
	]);

	let _ = tunnel_sender
		.send(
			protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(
				protocol::ToRivetResponseStart {
					status: 500,
					headers,
					body: Some(body),
					stream: false,
				},
			),
		)
		.await;
}
