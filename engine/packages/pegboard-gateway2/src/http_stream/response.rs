use bytes::Bytes;
use gas::prelude::*;
use rivet_envoy_protocol as protocol;
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use crate::shared_state::{
	InFlightRequestHandle, InFlightTunnelMessage, MsgGcReason, RequestStopResult,
};

use super::{ResponseBodyError, send_http_request_abort};

const HTTP_BODY_CHUNK_SIZE: usize = 64 * 1024;
const HTTP_RESPONSE_QUEUE_OVERLOADED_DETAIL: &str =
	"actor response producer outpaced downstream delivery and filled the gateway buffer";

/// Advances the expected tunnel sequence while rejecting missing or reordered response frames.
fn advance_http_stream_message_index(
	expected: protocol::MessageIndex,
	actual: protocol::MessageIndex,
) -> std::result::Result<protocol::MessageIndex, ()> {
	if actual == expected {
		Ok(expected.wrapping_add(1))
	} else {
		Err(())
	}
}

async fn send_http_response_body_bytes(
	in_flight_req: &InFlightRequestHandle,
	body_tx: &mpsc::Sender<Result<Bytes, ResponseBodyError>>,
	drop_rx: &mut watch::Receiver<Option<MsgGcReason>>,
	stopped_sub: &mut message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	actor_id: Id,
	body: Vec<u8>,
	detail: &'static str,
	egress_bytes: &AtomicU64,
) -> bool {
	let body = Bytes::from(body);
	for offset in (0..body.len()).step_by(HTTP_BODY_CHUNK_SIZE) {
		let chunk = body.slice(offset..(offset + HTTP_BODY_CHUNK_SIZE).min(body.len()));
		let chunk_len = chunk.len();
		let delivery = tokio::select! {
			biased;
			_ = body_tx.closed() => Err((RequestStopResult::ClientDisconnect, detail.to_owned())),
			_ = drop_rx.changed() => {
				let overloaded = matches!(
					drop_rx.borrow().as_ref(),
					Some(MsgGcReason::HttpResponseQueueOverloaded)
				);
				let reason = format!("{:?}", drop_rx.borrow().as_ref());
				if overloaded {
					send_http_request_abort(
						in_flight_req,
						protocol::HttpStreamAbortReasonKind::Overloaded,
						Some(HTTP_RESPONSE_QUEUE_OVERLOADED_DETAIL.to_owned()),
					)
					.await;
				}
				Err((
					RequestStopResult::RequestTimeout,
					format!("response stream garbage collected: {reason}"),
				))
			}
			_ = stopped_sub.next() => {
				tracing::debug!(%actor_id, "actor stopped while delivering streaming response");
				Err((
					RequestStopResult::EnvoyError,
					"actor stopped while streaming response".to_owned(),
				))
			}
			result = body_tx.send(Ok(chunk)) => match result {
				Ok(()) => Ok(()),
				Err(_) => Err((RequestStopResult::ClientDisconnect, detail.to_owned())),
			},
		};
		if let Err((stop_result, message)) = delivery {
			tracing::debug!(?stop_result, %message, "stopped delivering streaming http response");
			if !matches!(stop_result, RequestStopResult::ClientDisconnect) {
				send_http_body_error(body_tx, message);
			} else {
				send_http_request_abort(
					in_flight_req,
					protocol::HttpStreamAbortReasonKind::ClientDisconnect,
					Some(detail.to_owned()),
				)
				.await;
			}
			in_flight_req.stop(stop_result).await;
			return false;
		}
		egress_bytes.fetch_add(chunk_len as u64, Ordering::AcqRel);
	}

	true
}

pub(super) async fn drain_http_response_stream(
	in_flight_req: InFlightRequestHandle,
	mut msg_rx: mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	mut drop_rx: watch::Receiver<Option<MsgGcReason>>,
	mut stopped_sub: message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	body_tx: mpsc::Sender<Result<Bytes, ResponseBodyError>>,
	initial_body: Option<Vec<u8>>,
	mut expected_message_index: protocol::MessageIndex,
	actor_id: Id,
	idle_timeout: Option<Duration>,
	egress_bytes: Arc<AtomicU64>,
) {
	if let Some(body) = initial_body.filter(|body| !body.is_empty()) {
		if !send_http_response_body_bytes(
			&in_flight_req,
			&body_tx,
			&mut drop_rx,
			&mut stopped_sub,
			actor_id,
			body,
			"client dropped response before initial body was sent",
			&egress_bytes,
		)
		.await
		{
			return;
		}
	}

	loop {
		tokio::select! {
			res = msg_rx.recv() => {
				let Some(msg) = res else {
					tracing::warn!("streaming response tunnel channel closed");
					send_http_body_error(&body_tx, "response stream closed before finish");
					in_flight_req.stop(RequestStopResult::EnvoyError).await;
					return;
				};

				match advance_http_stream_message_index(
					expected_message_index,
					msg.message_id.message_index,
				) {
					Ok(next_message_index) => expected_message_index = next_message_index,
					Err(()) => {
						tracing::warn!(
							expected_message_index,
							actual_message_index = msg.message_id.message_index,
							"streaming response message index gap"
						);
						send_http_request_abort(
							&in_flight_req,
							protocol::HttpStreamAbortReasonKind::InternalError,
							Some("gateway detected response stream message index gap".to_owned()),
						)
						.await;
						send_http_body_error(&body_tx, "response stream message index gap");
						in_flight_req.stop(RequestStopResult::EnvoyError).await;
						return;
					}
				}

				match msg.message_kind {
					protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => {
						if !chunk.body.is_empty() {
							let delivered = send_http_response_body_bytes(
								&in_flight_req,
								&body_tx,
								&mut drop_rx,
								&mut stopped_sub,
								actor_id,
								chunk.body,
								"client dropped streaming response body",
								&egress_bytes,
							)
							.await;
							if !delivered {
								return;
							}
						}

						if chunk.finish {
							in_flight_req.stop(RequestStopResult::Success).await;
							return;
						}
					}
					protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(abort) => {
						let message = match &abort.reason.detail {
							Some(detail) => format!("{:?}: {detail}", abort.reason.kind),
							None => format!("{:?}", abort.reason.kind),
						};
						tracing::warn!(
							reason_kind = ?abort.reason.kind,
							reason_detail = ?abort.reason.detail,
							"streaming http response aborted by envoy"
						);
						send_http_body_error(
							&body_tx,
							format!("response stream aborted: {message}"),
						);
						in_flight_req.stop(RequestStopResult::EnvoyError).await;
						return;
					}
					other => {
						tracing::warn!(
							message_kind = ?other,
							"unexpected message while streaming http response"
						);
						send_http_request_abort(
							&in_flight_req,
							protocol::HttpStreamAbortReasonKind::InternalError,
							Some("gateway received unexpected response stream message".to_owned()),
						)
						.await;
						send_http_body_error(&body_tx, "unexpected response stream message");
						in_flight_req.stop(RequestStopResult::EnvoyError).await;
						return;
					}
				}
			}
			_ = drop_rx.changed() => {
				let overloaded = matches!(
					drop_rx.borrow().as_ref(),
					Some(MsgGcReason::HttpResponseQueueOverloaded)
				);
				let reason = format!("{:?}", drop_rx.borrow().as_ref());
				tracing::warn!(reason, "streaming response tunnel channel dropped");
				if overloaded {
					send_http_request_abort(
						&in_flight_req,
						protocol::HttpStreamAbortReasonKind::Overloaded,
						Some(HTTP_RESPONSE_QUEUE_OVERLOADED_DETAIL.to_owned()),
					)
					.await;
				}
				send_http_body_error(&body_tx, format!("response stream garbage collected: {reason}"));
				in_flight_req.stop(RequestStopResult::RequestTimeout).await;
				return;
			}
			_ = stopped_sub.next() => {
				tracing::debug!(%actor_id, "actor stopped while streaming response");
				send_http_body_error(&body_tx, "actor stopped while streaming response");
				in_flight_req.stop(RequestStopResult::EnvoyError).await;
				return;
			}
			_ = body_tx.closed() => {
				tracing::debug!("client dropped idle streaming http response body");
				send_http_request_abort(
					&in_flight_req,
					protocol::HttpStreamAbortReasonKind::ClientDisconnect,
					Some("client dropped streaming response body".to_owned()),
				)
				.await;
				in_flight_req.stop(RequestStopResult::ClientDisconnect).await;
				return;
			}
			_ = async {
				match idle_timeout {
					Some(timeout) => tokio::time::sleep(timeout).await,
					None => std::future::pending().await,
				}
			} => {
				let idle_timeout = idle_timeout.expect("idle timeout branch cannot run when disabled");
				tracing::warn!(
					timeout_ms = idle_timeout.as_millis() as u64,
					"timed out waiting for streaming response chunk"
				);
				send_http_request_abort(
					&in_flight_req,
					protocol::HttpStreamAbortReasonKind::IdleTimeout,
					Some("gateway timed out waiting for response stream chunk".to_owned()),
				)
				.await;
				send_http_body_error(&body_tx, "response stream idle timeout");
				in_flight_req.stop(RequestStopResult::RequestTimeout).await;
				return;
			}
		}
	}
}

fn send_http_body_error(
	body_tx: &mpsc::Sender<Result<Bytes, ResponseBodyError>>,
	message: impl Into<String>,
) {
	let error: Result<Bytes, ResponseBodyError> =
		Err(Box::new(std::io::Error::other(message.into())));
	match body_tx.try_send(error) {
		Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
		Err(mpsc::error::TrySendError::Full(_)) => {
			tracing::warn!(
				"could not deliver streaming response error because the downstream buffer is full"
			);
		}
	}
}

#[cfg(test)]
#[path = "../../tests/support/http_stream_response.rs"]
mod tests;
