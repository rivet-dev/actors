use anyhow::{Context, Result, bail};
use epoxy_protocol::{
	PROTOCOL_VERSION,
	protocol::{self, ReplicaId},
};
use futures_util::{StreamExt, stream::FuturesUnordered};
use gas::prelude::*;
use rivet_api_builder::ApiCtx;
use std::future::Future;

use crate::utils;

/// Find the API replica URL for a given replica ID in the topology
fn find_replica_address(
	config: &protocol::ClusterConfig,
	target_replica_id: ReplicaId,
) -> Result<String> {
	config
		.replicas
		.iter()
		.find(|x| x.replica_id == target_replica_id)
		.with_context(|| format!("replica {} not found in topology", target_replica_id))
		.map(|r| r.api_peer_url.clone())
}

/// Fans out a best-effort request to remote replicas and returns only the successful responses.
///
/// This helper never returns an error just because one or more replicas could not be reached,
/// timed out, or returned an error. Those failures are logged and dropped, and the caller gets
/// whatever successful responses arrived before the fanout quorum target was met or the pending
/// requests were exhausted.
///
/// This is intentional. Quorum-sensitive consensus paths must enforce quorum separately instead of
/// relying on this helper to fail fast on an individual replica miss.
#[tracing::instrument(skip_all, fields(%from_replica_id, ?replica_ids, ?quorum_type))]
pub async fn fanout_to_replicas<F, Fut, T>(
	from_replica_id: ReplicaId,
	replica_ids: &[ReplicaId],
	quorum_type: utils::QuorumType,
	request_builder: F,
) -> Result<Vec<T>>
where
	F: Fn(ReplicaId) -> Fut + Clone,
	Fut: Future<Output = Result<T>> + Send,
	T: Send,
{
	let target_responses = utils::calculate_fanout_quorum(replica_ids.len(), quorum_type);

	if target_responses == 0 {
		tracing::debug!("no fanout, target is 0");

		return Ok(Vec::new());
	}

	// Create futures for all replicas (excluding the sender)
	let mut responses = futures_util::stream::iter(
		replica_ids
			.iter()
			.filter(|&&replica_id| replica_id != from_replica_id)
			.map(|&to_replica_id| {
				let request_builder = request_builder.clone();
				async move {
					tokio::time::timeout(
						crate::consts::REQUEST_TIMEOUT,
						request_builder(to_replica_id),
					)
					.await
				}
			}),
	)
	.collect::<FuturesUnordered<_>>()
	.await;
	tracing::debug!(?target_responses, len=?responses.len(), "fanout target");

	// Collect responses until we reach quorum or all futures complete
	let mut successful_responses = Vec::new();
	while successful_responses.len() < target_responses {
		if let Some(response) = responses.next().await {
			match response {
				Ok(result) => match result {
					Ok(response) => {
						successful_responses.push(response);
					}
					Err(err) => {
						tracing::warn!(?err, "received error from replica");
					}
				},
				Err(err) => {
					tracing::warn!(?err, "received timeout from replica");
				}
			}
		} else {
			// No more responses available
			break;
		}
	}

	Ok(successful_responses)
}

#[tracing::instrument(skip_all)]
pub async fn send_message(
	ctx: &ApiCtx,
	config: &protocol::ClusterConfig,
	request: protocol::Request,
) -> Result<protocol::Response> {
	let replica_url = find_replica_address(config, request.to_replica_id)?;
	send_message_to_address(ctx, replica_url, request).await
}

#[tracing::instrument(skip_all, fields(%from_replica_id, %to_replica_id, count))]
pub async fn read_changelog(
	ctx: &ApiCtx,
	config: &protocol::ClusterConfig,
	from_replica_id: ReplicaId,
	to_replica_id: ReplicaId,
	after_versionstamp: Option<Vec<u8>>,
	count: u64,
) -> Result<protocol::ChangelogReadResponse> {
	let replica_url = find_replica_address(config, to_replica_id)?;
	let response = send_request_to_address(
		ctx,
		replica_url,
		"changelog-read",
		protocol::Request {
			from_replica_id,
			to_replica_id,
			kind: protocol::RequestKind::ChangelogReadRequest(protocol::ChangelogReadRequest {
				after_versionstamp,
				count,
			}),
		},
	)
	.await?;

	match response.kind {
		protocol::ResponseKind::ChangelogReadResponse(response) => Ok(response),
		_ => bail!("unexpected response type for changelog read request"),
	}
}

#[tracing::instrument(skip_all, fields(%replica_url))]
pub async fn send_message_to_address(
	ctx: &ApiCtx,
	replica_url: String,
	request: protocol::Request,
) -> Result<protocol::Response> {
	send_request_to_address(ctx, replica_url, "message", request).await
}

#[tracing::instrument(skip_all, fields(%replica_url, endpoint))]
async fn send_request_to_address(
	ctx: &ApiCtx,
	replica_url: String,
	endpoint: &'static str,
	request: protocol::Request,
) -> Result<protocol::Response> {
	let from_replica_id = request.from_replica_id;
	let to_replica_id = request.to_replica_id;

	if from_replica_id == to_replica_id {
		tracing::debug!(
			to_replica = to_replica_id,
			"sending message to replica directly"
		);

		return crate::replica::message_request::message_request(&ctx, request).await;
	}

	let mut replica_url = url::Url::parse(&replica_url)?;
	replica_url.set_path(&format!("/v{PROTOCOL_VERSION}/epoxy/{endpoint}"));

	tracing::debug!(
		to_replica = to_replica_id,
		%replica_url,
		"sending message to replica via http"
	);

	let client = rivet_pools::reqwest::client().await?;

	// Create the request
	let request =
		rivet_util::serde::bare_to_vec!(&request).context("failed to serialize epoxy request")?;

	// Send the request
	let response_result = client
		.post(replica_url.to_string())
		.body(request)
		.send()
		.custom_instrument(tracing::info_span!("http_request"))
		.await;

	let response = match response_result {
		Ok(resp) => resp,
		Err(e) => {
			tracing::error!(
				to_replica = to_replica_id,
				replica_url = %replica_url,
				error = %e,
				error_debug = ?e,
				"failed to send HTTP request to replica"
			);
			bail!(
				"failed to send HTTP request to replica {}: {}",
				to_replica_id,
				e
			);
		}
	};

	// Check if the request was successful
	if !response.status().is_success() {
		tracing::warn!(
			status = %response.status(),
			to_replica = to_replica_id,
			replica_url = %replica_url,
			"message send failed with non-success status"
		);
		bail!(
			"message send to replica {} failed with status: {}",
			to_replica_id,
			response.status()
		);
	}

	let body = response.bytes().await?;
	let response_body = rivet_util::serde::bare_from_slice!(&body)?;

	tracing::debug!(
		to_replica = to_replica_id,
		"successfully sent message via http"
	);

	Ok(response_body)
}
