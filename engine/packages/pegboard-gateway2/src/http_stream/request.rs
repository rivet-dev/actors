use anyhow::{Result, anyhow};
use bytes::Bytes;
use gas::prelude::*;
use http_body_util::BodyExt;
use hyper::body::{Body, SizeHint};
use rivet_envoy_protocol as protocol;
use rivet_guard_core::errors::{
	ActorStoppedWhileWaiting, GatewayResponseStartTimeout, InvalidRequestBody,
	TunnelMessageTimeout, TunnelRequestAborted, TunnelResponseClosed,
};
use std::{
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};
use tokio::sync::{mpsc, watch};

use crate::shared_state::{InFlightRequestHandle, InFlightTunnelMessage, MsgGcReason};

const PHASE_WAITING_FOR_RESPONSE_START: &str = "waiting_for_response_start";
const HTTP_BODY_CHUNK_SIZE: usize = 64 * 1024;
const HTTP_BODY_CHUNK_FLUSH_INTERVAL: Duration = Duration::from_millis(10);

pub(super) fn should_stream_http_request_body_hint(size_hint: &SizeHint) -> bool {
	match size_hint.upper() {
		Some(body_len) => body_len as usize > HTTP_BODY_CHUNK_SIZE,
		None => true,
	}
}

#[derive(Debug, PartialEq)]
enum RequestBodySize {
	WithinLimit(usize),
	ExceedsLimit,
}

fn next_request_body_size(current: usize, chunk: usize, limit: usize) -> RequestBodySize {
	match current.checked_add(chunk) {
		Some(size) if size <= limit => RequestBodySize::WithinLimit(size),
		Some(_) | None => RequestBodySize::ExceedsLimit,
	}
}

#[derive(Default)]
struct HttpRequestBodyChunker {
	pending: Vec<u8>,
}

impl HttpRequestBodyChunker {
	fn is_empty(&self) -> bool {
		self.pending.is_empty()
	}

	fn push(&mut self, mut data: &[u8]) -> Vec<Vec<u8>> {
		let mut chunks = Vec::new();
		while !data.is_empty() {
			let remaining = HTTP_BODY_CHUNK_SIZE - self.pending.len();
			let take = remaining.min(data.len());
			self.pending.extend_from_slice(&data[..take]);
			data = &data[take..];
			if self.pending.len() == HTTP_BODY_CHUNK_SIZE {
				chunks.push(std::mem::take(&mut self.pending));
			}
		}
		chunks
	}

	fn flush(&mut self) -> Option<Vec<u8>> {
		if self.pending.is_empty() {
			None
		} else {
			Some(std::mem::take(&mut self.pending))
		}
	}
}

async fn send_streaming_http_request_body_chunks<B>(
	in_flight_req: &InFlightRequestHandle,
	mut body: B,
	max_body_size: usize,
	ingress_bytes: Arc<AtomicU64>,
) -> Result<()>
where
	B: Body<Data = Bytes> + Unpin,
	B::Error: std::fmt::Display,
{
	// Count bytes before coalescing so transport framing cannot bypass the cumulative body limit.
	let mut body_size = 0usize;
	let mut chunker = HttpRequestBodyChunker::default();
	let mut flush_deadline = None;

	// The caller polls this upload future alongside response-start/abort
	// observation. Dropping it on an early actor result immediately stops reading
	// the client. While it is active, small DATA frames are coalesced to avoid
	// protocol-message amplification, with a deadline from the first buffered
	// byte so low-volume streams still make bounded progress.
	loop {
		let frame = if let Some(deadline) = flush_deadline {
			tokio::select! {
				frame = body.frame() => frame,
				_ = tokio::time::sleep_until(deadline) => {
					if let Some(chunk) = chunker.flush() {
						send_http_request_body_chunk(
							in_flight_req,
							chunk,
							false,
						)
						.await?;
					}
					flush_deadline = None;
					continue;
				}
			}
		} else {
			body.frame().await
		};
		let data = match frame {
			// The client request body reached normal EOF.
			None => break,
			Some(Ok(frame)) => {
				let Ok(data) = frame.into_data() else {
					continue;
				};
				data
			}
			Some(Err(error)) => {
				tracing::warn!(%error, "failed to read streaming request body from client");
				super::send_http_request_abort(
					in_flight_req,
					protocol::HttpStreamAbortReasonKind::ClientDisconnect,
					Some(error.to_string()),
				)
				.await;
				return Err(anyhow!("failed to read streaming request body: {error}"));
			}
		};
		ingress_bytes.fetch_add(data.len() as u64, Ordering::AcqRel);
		body_size = match next_request_body_size(body_size, data.len(), max_body_size) {
			RequestBodySize::WithinLimit(next_body_size) => next_body_size,
			RequestBodySize::ExceedsLimit => {
				let reason = format!("request body exceeded the {max_body_size}-byte limit");
				super::send_http_request_abort(
					in_flight_req,
					protocol::HttpStreamAbortReasonKind::BodyTooLarge,
					Some(reason.clone()),
				)
				.await;
				return Err(InvalidRequestBody { reason }.build());
			}
		};

		let was_empty = chunker.is_empty();
		for chunk in chunker.push(&data) {
			send_http_request_body_chunk(in_flight_req, chunk, false).await?;
		}
		if chunker.is_empty() {
			flush_deadline = None;
		} else if was_empty {
			flush_deadline = Some(tokio::time::Instant::now() + HTTP_BODY_CHUNK_FLUSH_INTERVAL);
		}
	}

	// Normal EOF sends exactly one final protocol chunk so actor-side upload
	// state and request routing can be released.
	let final_chunk = chunker.flush().unwrap_or_default();
	send_http_request_body_chunk(in_flight_req, final_chunk, true).await
}

async fn send_http_request_body_chunk(
	in_flight_req: &InFlightRequestHandle,
	body: Vec<u8>,
	finish: bool,
) -> Result<()> {
	let message =
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(protocol::ToEnvoyRequestChunk {
			body,
			finish,
		});
	in_flight_req.send_message(message, false).await
}

pub(super) async fn wait_for_http_response_start(
	msg_rx: &mut mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	drop_rx: &mut watch::Receiver<Option<MsgGcReason>>,
	stopped_sub: &mut message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	actor_id: Id,
	request_id: protocol::RequestId,
	deadline: tokio::time::Instant,
	timeout: Duration,
) -> Result<(protocol::MessageId, protocol::ToRivetResponseStart)> {
	loop {
		tokio::select! {
			res = msg_rx.recv() => {
				let Some(msg) = res else {
					tracing::warn!(
						request_id=%protocol::util::id_to_string(&request_id),
						"received empty message response during request init",
					);
					return Err(TunnelResponseClosed {
						phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
					}
					.build());
				};

				match msg.message_kind {
					protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(response_start) => {
						return Ok((msg.message_id, response_start));
					}
					protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(abort) => {
						tracing::warn!(
							reason_kind = ?abort.reason.kind,
							reason_detail = ?abort.reason.detail,
							"request aborted"
						);
						return Err(TunnelRequestAborted {
							phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
						}
						.build());
					}
					_ => {
						tracing::warn!("received non-response message from pubsub");
					}
				}
			}
			_ = drop_rx.changed() => {
				tracing::warn!(reason=?drop_rx.borrow(), "tunnel message timeout");
				return Err(TunnelMessageTimeout {
					phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
					reason: format!("{:?}", drop_rx.borrow().as_ref()),
				}
				.build());
			}
			_ = stopped_sub.next() => {
				tracing::debug!("actor stopped while waiting for request response");
				return Err(ActorStoppedWhileWaiting {
					actor_id: actor_id.to_string(),
					phase: PHASE_WAITING_FOR_RESPONSE_START.to_owned(),
				}.build());
			}
			_ = tokio::time::sleep_until(deadline) => {
				tracing::warn!("timed out waiting for response start from envoy");
				return Err(GatewayResponseStartTimeout {
					phase: "response_start".to_owned(),
					timeout_ms: timeout.as_millis() as u64,
				}
				.build());
			}
		}
	}
}

pub(super) async fn stream_http_request_and_wait_for_response<B>(
	in_flight_req: &InFlightRequestHandle,
	msg_rx: &mut mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	drop_rx: &mut watch::Receiver<Option<MsgGcReason>>,
	stopped_sub: &mut message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	actor_id: Id,
	request_id: protocol::RequestId,
	body: B,
	max_body_size: usize,
	ingress_bytes: Arc<AtomicU64>,
	response_start_deadline: tokio::time::Instant,
	response_start_timeout: Duration,
) -> Result<(protocol::MessageId, protocol::ToRivetResponseStart)>
where
	B: Body<Data = Bytes> + Unpin,
	B::Error: std::fmt::Display,
{
	// Upload ingress, cumulative byte accounting, and bounded-latency
	// coalescing run in one future. Response start/abort is observed
	// concurrently so a slow or rejected upload cannot retain the request.
	let upload =
		send_streaming_http_request_body_chunks(in_flight_req, body, max_body_size, ingress_bytes);
	tokio::pin!(upload);
	let response_start = wait_for_http_response_start(
		msg_rx,
		drop_rx,
		stopped_sub,
		actor_id,
		request_id,
		response_start_deadline,
		response_start_timeout,
	);
	tokio::pin!(response_start);
	tokio::select! {
		upload_result = &mut upload => {
			// Normal completion already sent the final upload marker. Continue
			// polling the same response future under the original deadline.
			upload_result?;
			response_start.await
		}
		response_start = &mut response_start => {
			let response_start = response_start?;
			// HTTP handlers may intentionally respond before consuming the full
			// upload. Stop reading the client and send a clean final marker so
			// actor request routing can be released.
			send_http_request_body_chunk(in_flight_req, Vec::new(), true).await?;
			Ok(response_start)
		}
	}
}

#[cfg(test)]
#[path = "../../tests/support/http_stream_request.rs"]
mod tests;
