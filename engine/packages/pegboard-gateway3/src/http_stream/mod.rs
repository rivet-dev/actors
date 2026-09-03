use rivet_envoy_protocol as protocol;

use crate::shared_state::InFlightRequestHandle;

mod handler;
mod request;
mod response;
mod response_queue;

pub(crate) use handler::PreparedHttpRequest;

pub(crate) use response_queue::{
	HttpResponseQueueBudget, HttpResponseQueueOverloaded, HttpResponseQueuePermit,
};

pub(super) type ResponseBodyError = Box<dyn std::error::Error + Send + Sync>;

pub(super) async fn send_http_request_abort(
	in_flight_req: &InFlightRequestHandle,
	kind: protocol::HttpStreamAbortReasonKind,
	detail: impl Into<Option<String>>,
) {
	let Some((actor_id, actor_generation)) = in_flight_req.begin_http_abort().await else {
		return;
	};
	crate::metrics::HTTP_STREAM_ABORT_TOTAL
		.with_label_values(&["request", "gateway", abort_kind_label(&kind)])
		.inc();
	let message =
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(protocol::ToEnvoyRequestAbort {
			actor_id: Some(actor_id),
			actor_generation,
			reason: protocol::HttpStreamAbortReason {
				kind,
				detail: detail.into(),
			},
		});
	if let Err(err) = in_flight_req.send_message(message, true).await {
		tracing::debug!(?err, "failed sending http request abort to envoy");
	}
}

pub(crate) fn abort_kind_label(kind: &protocol::HttpStreamAbortReasonKind) -> &'static str {
	match kind {
		protocol::HttpStreamAbortReasonKind::Unknown => "unknown",
		protocol::HttpStreamAbortReasonKind::Cancelled => "cancelled",
		protocol::HttpStreamAbortReasonKind::HandlerError => "handler_error",
		protocol::HttpStreamAbortReasonKind::InternalError => "internal_error",
	}
}
