use rivet_envoy_protocol as protocol;

use crate::shared_state::InFlightRequestHandle;

mod handler;
mod request;
mod response;
mod response_queue;

pub(crate) use response_queue::{
	HttpResponseQueueBudget, HttpResponseQueueOverloaded, HttpResponseQueuePermit,
};

pub(super) type ResponseBodyError = Box<dyn std::error::Error + Send + Sync>;

pub(super) async fn send_http_request_abort(
	in_flight_req: &InFlightRequestHandle,
	kind: protocol::HttpStreamAbortReasonKind,
	detail: impl Into<Option<String>>,
) {
	let message =
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(protocol::ToEnvoyRequestAbort {
			reason: protocol::HttpStreamAbortReason {
				kind,
				detail: detail.into(),
			},
		});
	if let Err(err) = in_flight_req.send_message(message, true).await {
		tracing::debug!(?err, "failed sending http request abort to envoy");
	}
}
