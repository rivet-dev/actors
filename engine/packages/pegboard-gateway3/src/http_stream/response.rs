use bytes::Bytes;
use gas::prelude::*;
use rivet_envoy_protocol as protocol;
use rivet_guard_core::ResponseBodyTerminal;
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use crate::shared_state::{
	InFlightRequestHandle, InFlightTunnelMessage, MsgGcReason, RequestStopResult,
};

use crate::metrics::HttpResponseFlowMetrics;

use super::{ResponseBodyError, send_http_request_abort};

const HTTP_BODY_CHUNK_SIZE: usize = 64 * 1024;
const HTTP_RESPONSE_QUEUE_OVERLOADED_DETAIL: &str =
	"actor response producer outpaced downstream delivery and filled the gateway buffer";

struct HttpResponseWindowStall(std::time::Instant);

#[derive(Debug)]
enum ResponseChunkDelivery {
	Delivered,
	ClientDisconnected,
	EnvoyAborted(Option<protocol::HttpStreamAbortReason>),
}

fn delivery_failure_abort_kind(
	stop_result: RequestStopResult,
) -> protocol::HttpStreamAbortReasonKind {
	match stop_result {
		RequestStopResult::EnvoyError
		| RequestStopResult::ActorReadyTimeout
		| RequestStopResult::RequestTimeout => protocol::HttpStreamAbortReasonKind::InternalError,
		RequestStopResult::ClientDisconnect => protocol::HttpStreamAbortReasonKind::Cancelled,
		RequestStopResult::Success => unreachable!("failed delivery cannot be successful"),
	}
}

impl Drop for HttpResponseWindowStall {
	fn drop(&mut self) {
		crate::metrics::HTTP_BODY_WINDOW_STALL_DURATION_SECONDS
			.with_label_values(&["response"])
			.observe(self.0.elapsed().as_secs_f64());
	}
}

async fn deliver_response_chunk(
	body_tx: &mpsc::Sender<Result<Bytes, ResponseBodyError>>,
	http_response_abort_rx: &mut watch::Receiver<Option<protocol::HttpStreamAbortReason>>,
	chunk: Bytes,
) -> ResponseChunkDelivery {
	if let Some(reason) = http_response_abort_rx.borrow().clone() {
		return ResponseChunkDelivery::EnvoyAborted(Some(reason));
	}

	tokio::select! {
		biased;
		_ = body_tx.closed() => ResponseChunkDelivery::ClientDisconnected,
		changed = http_response_abort_rx.changed() => {
			let reason = changed
				.ok()
				.and_then(|()| http_response_abort_rx.borrow().clone());
			ResponseChunkDelivery::EnvoyAborted(reason)
		}
		result = body_tx.send(Ok(chunk)) => match result {
			Ok(()) => ResponseChunkDelivery::Delivered,
			Err(_) => ResponseChunkDelivery::ClientDisconnected,
		},
	}
}

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
	http_response_abort_rx:
		&mut watch::Receiver<Option<protocol::HttpStreamAbortReason>>,
	stopped_sub: &mut message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	actor_id: Id,
	body: Vec<u8>,
	detail: &'static str,
	response_consumed_bytes: &AtomicU64,
	received_bytes: &mut u64,
	terminal_error: &ResponseBodyTerminal,
	flow_metrics: &Arc<HttpResponseFlowMetrics>,
) -> bool {
	let body = Bytes::from(body);
	for offset in (0..body.len()).step_by(HTTP_BODY_CHUNK_SIZE) {
		let chunk = body.slice(offset..(offset + HTTP_BODY_CHUNK_SIZE).min(body.len()));
		let chunk_len = chunk.len();
		let consumed_bytes = response_consumed_bytes.load(Ordering::Acquire);
		let Some((next_received_bytes, outstanding_bytes)) = received_bytes
			.checked_add(chunk_len as u64)
			.and_then(|next| {
				next.checked_sub(consumed_bytes)
					.map(|outstanding| (next, outstanding))
			})
		else {
			fail_response_window(
				in_flight_req,
				"invalid HTTP response body window accounting",
				terminal_error,
			)
			.await;
			return false;
		};
		if outstanding_bytes > protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES {
			crate::metrics::HTTP_PROTOCOL_VIOLATION_TOTAL
				.with_label_values(&["response", "window_exceeded"])
				.inc();
			tracing::warn!(
				outstanding_bytes,
				window_bytes = protocol::HTTP_STREAM_INITIAL_WINDOW_BYTES,
				"envoy exceeded the HTTP response body window"
			);
			fail_response_window(
				in_flight_req,
				"envoy exceeded the HTTP response body window",
				terminal_error,
			)
			.await;
			return false;
		}
		*received_bytes = next_received_bytes;
		flow_metrics.enqueue(chunk_len);
		let _stall = if body_tx.capacity() == 0 {
			crate::metrics::HTTP_BODY_WINDOW_EXHAUSTED_TOTAL
				.with_label_values(&["response"])
				.inc();
			Some(HttpResponseWindowStall(std::time::Instant::now()))
		} else {
			None
		};
		let delivery = tokio::select! {
			biased;
			_ = drop_rx.changed() => {
				let overloaded = matches!(
					drop_rx.borrow().as_ref(),
					Some(MsgGcReason::HttpResponseQueueOverloaded)
				);
				let reason = format!("{:?}", drop_rx.borrow().as_ref());
				if overloaded {
					send_http_request_abort(
						in_flight_req,
						protocol::HttpStreamAbortReasonKind::InternalError,
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
			result = deliver_response_chunk(body_tx, http_response_abort_rx, chunk) => match result {
				ResponseChunkDelivery::Delivered => Ok(()),
				ResponseChunkDelivery::ClientDisconnected => {
					Err((RequestStopResult::ClientDisconnect, detail.to_owned()))
				}
				ResponseChunkDelivery::EnvoyAborted(reason) => {
					flow_metrics.consume(chunk_len);
					finish_envoy_response_abort(in_flight_req, terminal_error, reason).await;
					return false;
				}
			},
		};
		if let Err((stop_result, message)) = delivery {
			flow_metrics.consume(chunk_len);
			tracing::debug!(?stop_result, %message, "stopped delivering streaming http response");
			let abort_kind = delivery_failure_abort_kind(stop_result);
			send_http_request_abort(in_flight_req, abort_kind, Some(message.clone())).await;
			if !matches!(stop_result, RequestStopResult::ClientDisconnect) {
				send_http_body_error(terminal_error, message);
			}
			in_flight_req.stop(stop_result).await;
			return false;
		}
	}

	true
}

async fn fail_response_window(
	in_flight_req: &InFlightRequestHandle,
	detail: &'static str,
	terminal_error: &ResponseBodyTerminal,
) {
	send_http_request_abort(
		in_flight_req,
		protocol::HttpStreamAbortReasonKind::InternalError,
		Some(detail.to_owned()),
	)
	.await;
	send_http_body_error(terminal_error, detail);
	in_flight_req.stop(RequestStopResult::EnvoyError).await;
}

pub(super) async fn drain_http_response_stream(
	in_flight_req: InFlightRequestHandle,
	mut msg_rx: mpsc::UnboundedReceiver<InFlightTunnelMessage>,
	mut drop_rx: watch::Receiver<Option<MsgGcReason>>,
	mut http_response_abort_rx:
		watch::Receiver<Option<protocol::HttpStreamAbortReason>>,
	mut stopped_sub: message::SubscriptionHandle<pegboard::workflows::actor2::Stopped>,
	body_tx: mpsc::Sender<Result<Bytes, ResponseBodyError>>,
	initial_body: Option<Vec<u8>>,
	mut expected_message_index: protocol::MessageIndex,
	actor_id: Id,
	idle_timeout: Option<Duration>,
	tunnel_ping_interval: Duration,
	response_consumed_bytes: Arc<AtomicU64>,
	terminal_error: ResponseBodyTerminal,
	flow_metrics: Arc<HttpResponseFlowMetrics>,
) {
	let mut received_bytes = 0u64;
	let mut idle_deadline = idle_timeout.map(|timeout| tokio::time::Instant::now() + timeout);
	let mut tunnel_ping = tokio::time::interval_at(
		tokio::time::Instant::now() + tunnel_ping_interval,
		tunnel_ping_interval,
	);
	tunnel_ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
	if let Some(body) = initial_body.filter(|body| !body.is_empty()) {
		if !send_http_response_body_bytes(
			&in_flight_req,
			&body_tx,
			&mut drop_rx,
			&mut http_response_abort_rx,
			&mut stopped_sub,
			actor_id,
			body,
			"client dropped response before initial body was sent",
			&response_consumed_bytes,
			&mut received_bytes,
			&terminal_error,
			&flow_metrics,
		)
		.await
		{
			return;
		}
	}

	loop {
		tokio::select! {
			biased;
			_ = tunnel_ping.tick() => {
				if let Err(error) = in_flight_req.send_and_check_ping().await {
					crate::metrics::HTTP_POST_COMMIT_FAILURE_TOTAL
						.with_label_values(&["tunnel_liveness"])
						.inc();
					tracing::warn!(?error, "lost Envoy tunnel while streaming HTTP response");
					send_http_body_error(&terminal_error, "Envoy tunnel lost while streaming response");
					in_flight_req.stop(RequestStopResult::EnvoyError).await;
					return;
				}
			}
			res = msg_rx.recv() => {
				let Some(msg) = res else {
					tracing::warn!("streaming response tunnel channel closed");
					send_http_body_error(&terminal_error, "response stream closed before finish");
					in_flight_req.stop(RequestStopResult::EnvoyError).await;
					return;
				};

				match advance_http_stream_message_index(
					expected_message_index,
					msg.message_id.message_index,
				) {
					Ok(next_message_index) => expected_message_index = next_message_index,
					Err(()) => {
						crate::metrics::HTTP_PROTOCOL_VIOLATION_TOTAL
							.with_label_values(&["response", "message_index"])
							.inc();
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
						send_http_body_error(&terminal_error, "response stream message index gap");
						in_flight_req.stop(RequestStopResult::EnvoyError).await;
						return;
					}
				}

				match msg.message_kind {
					protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyWindowUpdate(update) => {
						if let Err(error) = in_flight_req
							.update_request_body_consumed(update.consumed_bytes)
							.await
						{
							crate::metrics::HTTP_PROTOCOL_VIOLATION_TOTAL
								.with_label_values(&["request", "invalid_credit"])
								.inc();
							tracing::warn!(?error, "invalid request body window update");
							in_flight_req.stop(RequestStopResult::EnvoyError).await;
							return;
						}
					}
					protocol::ToRivetTunnelMessageKind::ToRivetRequestBodyCancel => {
						in_flight_req.cancel_upload();
					}
					protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(chunk) => {
						if !chunk.body.is_empty() {
							let delivered = send_http_response_body_bytes(
								&in_flight_req,
								&body_tx,
								&mut drop_rx,
								&mut http_response_abort_rx,
								&mut stopped_sub,
								actor_id,
								chunk.body,
								"client dropped streaming response body",
								&response_consumed_bytes,
								&mut received_bytes,
								&terminal_error,
								&flow_metrics,
							)
							.await;
							if !delivered {
								return;
							}
							idle_deadline = idle_timeout
								.map(|timeout| tokio::time::Instant::now() + timeout);
						}

						if chunk.finish {
							in_flight_req.stop(RequestStopResult::Success).await;
							return;
						}
					}
					protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(abort) => {
						finish_envoy_response_abort(
							&in_flight_req,
							&terminal_error,
							Some(abort.reason),
						)
						.await;
						return;
					}
					other => {
						crate::metrics::HTTP_PROTOCOL_VIOLATION_TOTAL
							.with_label_values(&["response", "unexpected_message"])
							.inc();
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
						send_http_body_error(&terminal_error, "unexpected response stream message");
						in_flight_req.stop(RequestStopResult::EnvoyError).await;
						return;
					}
				}
			}
			changed = http_response_abort_rx.changed() => {
				let reason = changed
					.ok()
					.and_then(|()| http_response_abort_rx.borrow().clone());
				finish_envoy_response_abort(&in_flight_req, &terminal_error, reason).await;
				return;
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
						protocol::HttpStreamAbortReasonKind::InternalError,
						Some(HTTP_RESPONSE_QUEUE_OVERLOADED_DETAIL.to_owned()),
					)
					.await;
				}
				send_http_body_error(&terminal_error, format!("response stream garbage collected: {reason}"));
				in_flight_req.stop(RequestStopResult::RequestTimeout).await;
				return;
			}
			_ = stopped_sub.next() => {
				tracing::debug!(%actor_id, "actor stopped while streaming response");
				send_http_request_abort(
					&in_flight_req,
					protocol::HttpStreamAbortReasonKind::Cancelled,
					Some("actor stopped while streaming response".to_owned()),
				)
				.await;
				send_http_body_error(&terminal_error, "actor stopped while streaming response");
				in_flight_req.stop(RequestStopResult::EnvoyError).await;
				return;
			}
			_ = body_tx.closed() => {
				tracing::debug!("client dropped idle streaming http response body");
				send_http_request_abort(
					&in_flight_req,
					protocol::HttpStreamAbortReasonKind::Cancelled,
					Some("client dropped streaming response body".to_owned()),
				)
				.await;
				in_flight_req.stop(RequestStopResult::ClientDisconnect).await;
				return;
			}
			_ = async {
				match idle_deadline {
					Some(deadline) => tokio::time::sleep_until(deadline).await,
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
					protocol::HttpStreamAbortReasonKind::Cancelled,
					Some("gateway timed out waiting for response stream chunk".to_owned()),
				)
				.await;
				send_http_body_error(&terminal_error, "response stream idle timeout");
				in_flight_req.stop(RequestStopResult::RequestTimeout).await;
				return;
			}
		}
	}
}

async fn finish_envoy_response_abort(
	in_flight_req: &InFlightRequestHandle,
	terminal_error: &ResponseBodyTerminal,
	reason: Option<protocol::HttpStreamAbortReason>,
) {
	if let Some(reason) = &reason {
		crate::metrics::HTTP_STREAM_ABORT_TOTAL
			.with_label_values(&[
				"response",
				"actor",
				super::abort_kind_label(&reason.kind),
			])
			.inc();
	}
	let message = match &reason {
		Some(reason) => match &reason.detail {
			Some(detail) => format!("{:?}: {detail}", reason.kind),
			None => format!("{:?}", reason.kind),
		},
		None => "Envoy response abort signal closed".to_owned(),
	};
	tracing::warn!(
		reason_kind = ?reason.as_ref().map(|reason| &reason.kind),
		reason_detail = ?reason.as_ref().and_then(|reason| reason.detail.as_deref()),
		"streaming http response aborted by envoy"
	);
	send_http_body_error(
		terminal_error,
		format!("response stream aborted: {message}"),
	);
	in_flight_req.stop(RequestStopResult::EnvoyError).await;
}

pub(super) async fn send_http_response_window_updates(
	in_flight_req: InFlightRequestHandle,
	mut consumed_rx: mpsc::UnboundedReceiver<u64>,
) {
	while let Some(mut consumed_bytes) = consumed_rx.recv().await {
		while let Ok(next) = consumed_rx.try_recv() {
			consumed_bytes = next;
		}

		let message = protocol::ToEnvoyTunnelMessageKind::ToEnvoyResponseBodyWindowUpdate(
			protocol::ToEnvoyResponseBodyWindowUpdate { consumed_bytes },
		);
		if let Err(error) = in_flight_req.send_message(message, false).await {
			tracing::debug!(?error, "failed to return HTTP response body credit");
			in_flight_req.stop(RequestStopResult::EnvoyError).await;
			return;
		}
	}
}

fn send_http_body_error(terminal_error: &ResponseBodyTerminal, message: impl Into<String>) {
	terminal_error.fail(Box::new(std::io::Error::other(message.into())));
}

#[cfg(test)]
#[path = "../../tests/support/http_stream_response.rs"]
mod tests;
