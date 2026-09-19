use rivet_envoy_protocol as protocol;
use tokio::sync::oneshot;

use crate::connection::ws_send;
use crate::envoy::EnvoyContext;
use crate::metrics::METRICS;

pub struct KvRequestEntry {
	pub actor_id: String,
	pub data: protocol::KvRequestData,
	pub response_tx: oneshot::Sender<anyhow::Result<protocol::KvResponseData>>,
	pub sent: bool,
	pub timestamp: crate::time::Instant,
}

pub const KV_EXPIRE_MS: u64 = 30_000;
pub const KV_CLEANUP_INTERVAL_MS: u64 = 15_000;

pub async fn handle_kv_request(
	ctx: &mut EnvoyContext,
	actor_id: String,
	data: protocol::KvRequestData,
	response_tx: oneshot::Sender<anyhow::Result<protocol::KvResponseData>>,
) {
	let request_id = ctx.next_kv_request_id;
	ctx.next_kv_request_id += 1;

	let entry = KvRequestEntry {
		actor_id,
		data,
		response_tx,
		sent: false,
		timestamp: crate::time::Instant::now(),
	};

	ctx.kv_requests.insert(request_id, entry);
	METRICS.kv_requests_inflight.inc();

	let ws_available = {
		let guard = ctx.shared.ws_tx.lock().await;
		guard.is_some()
	};

	if ws_available {
		send_single_kv_request(ctx, request_id).await;
	}
}

pub async fn handle_kv_response(ctx: &mut EnvoyContext, response: protocol::ToEnvoyKvResponse) {
	let request = ctx.kv_requests.remove(&response.request_id);

	if let Some(request) = request {
		METRICS.kv_requests_inflight.dec();
		match response.data {
			protocol::KvResponseData::KvErrorResponse(ref e) => {
				let _ = request
					.response_tx
					.send(Err(anyhow::anyhow!("{}", e.message)));
			}
			_ => {
				let _ = request.response_tx.send(Ok(response.data));
			}
		}
	} else {
		tracing::error!(
			request_id = response.request_id,
			"received kv response for unknown request id"
		);
	}
}

pub async fn send_single_kv_request(ctx: &mut EnvoyContext, request_id: u32) {
	let request = ctx.kv_requests.get_mut(&request_id);
	let Some(request) = request else { return };
	if request.sent {
		return;
	}

	let failed = ws_send(
		&ctx.shared,
		protocol::ToRivet::ToRivetKvRequest(protocol::ToRivetKvRequest {
			actor_id: request.actor_id.clone(),
			request_id,
			data: request.data.clone(),
		}),
	)
	.await;
	if failed {
		return;
	}

	// Re-get after async call
	if let Some(request) = ctx.kv_requests.get_mut(&request_id) {
		request.sent = true;
		request.timestamp = crate::time::Instant::now();
	}
}

pub async fn process_unsent_kv_requests(ctx: &mut EnvoyContext) {
	let ws_available = {
		let guard = ctx.shared.ws_tx.lock().await;
		guard.is_some()
	};

	if !ws_available {
		return;
	}

	let unsent: Vec<u32> = ctx
		.kv_requests
		.iter()
		.filter(|(_, req)| !req.sent)
		.map(|(id, _)| *id)
		.collect();

	for request_id in unsent {
		send_single_kv_request(ctx, request_id).await;
	}
}

pub fn cleanup_old_kv_requests(ctx: &mut EnvoyContext) {
	let now = crate::time::Instant::now();
	let mut to_delete = Vec::new();

	for (request_id, request) in &ctx.kv_requests {
		if now.duration_since(request.timestamp).as_millis() > KV_EXPIRE_MS as u128 {
			to_delete.push(*request_id);
		}
	}

	for request_id in to_delete {
		if let Some(request) = ctx.kv_requests.remove(&request_id) {
			METRICS.kv_requests_inflight.dec();
			tracing::warn!(
				request_id,
				was_sent = request.sent,
				age_ms = now.duration_since(request.timestamp).as_millis() as u64,
				"kv request expired by cleanup"
			);
			let _ = request
				.response_tx
				.send(Err(anyhow::anyhow!("KV request timed out")));
		}
	}
}
