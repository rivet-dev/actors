use std::{
	sync::Arc,
	time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use futures_util::FutureExt;
use tokio::sync::oneshot;

use crate::{
	error::DatabaseError,
	options::ConflictRangeType,
	tx_ops::{self, Operation},
};

use super::{
	codec,
	shared::{LeaseInfo, PostgresShared},
	transport::{CommitJob, CommitOutcome, Responder, Transport},
};

/// How long to wait for a leader to be elected before giving up a submit as retryable.
const LEADER_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Poll cadence while waiting for a leader to appear in the cache.
const LEADER_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Per-attempt timeout for a NATS commit request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How many times a multi-node commit resends the same request (same dedup key) across leader
/// failover / indeterminate failures before giving up as retryable. The dedup table makes the
/// resends exactly-once.
const MAX_SUBMIT_ATTEMPTS: usize = 8;
/// Backoff between multi-node resends.
const RESEND_BACKOFF: Duration = Duration::from_millis(100);
/// The NATS server's `max_payload` when it is not configured.
const NATS_DEFAULT_MAX_PAYLOAD: usize = 1024 * 1024;

/// Submit a follower transaction's commit to the leader and await the result.
///
/// `read_version` is the watermark captured when this transaction opened its read snapshot. A
/// read-only transaction submits nothing: its reads already came from one pinned snapshot, so there
/// is nothing to order or validate and nothing for a later transaction to conflict against.
pub async fn submit(
	shared: &Arc<PostgresShared>,
	read_version: i64,
	operations: Vec<Operation>,
	conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
) -> Result<()> {
	if tx_ops::is_read_only(&operations, &conflict_ranges) {
		return Ok(());
	}

	match &shared.transport {
		Transport::SingleNode { commit_tx } => {
			submit_local(commit_tx, read_version, operations, conflict_ranges).await
		}
		Transport::MultiNode(_) => {
			submit_nats(shared, read_version, operations, conflict_ranges).await
		}
	}
}

/// Single-node: hand the job straight to the in-process leader drain loop and await its result.
async fn submit_local(
	commit_tx: &tokio::sync::mpsc::Sender<CommitJob>,
	read_version: i64,
	operations: Vec<Operation>,
	conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
) -> Result<()> {
	let (response_tx, response_rx) = oneshot::channel();
	let job = CommitJob {
		read_version: read_version.max(0) as u64,
		conflict_ranges,
		operations,
		dedup_key: None,
		responder: Responder::Local(response_tx),
	};

	if commit_tx.send(job).await.is_err() {
		// The leader drain loop is gone (driver shutting down). Retryable.
		return Err(
			anyhow::Error::from(DatabaseError::NotCommitted).context("leader drain loop is gone")
		);
	}

	match response_rx.await {
		Ok(CommitOutcome::Committed { .. }) => Ok(()),
		// The leader resolved this commit as a loser. A cold-window rejection during leader recovery
		// arrives as the same outcome, so the leader's batch log is what separates the two.
		Ok(CommitOutcome::Conflict) => Err(anyhow::Error::from(DatabaseError::NotCommitted)
			.context("leader resolved the commit as a conflict")),
		// The leader dropped the job without responding; it was not applied.
		Err(_) => Err(anyhow::Error::from(DatabaseError::NotCommitted)
			.context("leader dropped the commit without responding")),
	}
}

/// Multi-node: send the commit to the elected leader over NATS request/reply, resending the same
/// request (same dedup key) across leader failover. The reply carries the commit result directly.
async fn submit_nats(
	shared: &Arc<PostgresShared>,
	read_version: i64,
	operations: Vec<Operation>,
	conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
) -> Result<()> {
	let Transport::MultiNode(nats) = &shared.transport else {
		unreachable!("submit_nats requires the multi-node transport");
	};

	// One dedup key for this logical commit, reused across every resend so the leader applies it at
	// most once even if an earlier attempt was applied but its reply was lost to a failover.
	let client_seq = shared.next_commit_seq();
	let protocol_version = shared.commit_protocol_version();
	let payload = codec::encode_commit_request(
		read_version.max(0) as u64,
		&conflict_ranges,
		&operations,
		shared.node_id.as_bytes(),
		client_seq as u64,
		protocol_version,
	)
	.context("failed to encode commit request")?;

	let submit_start = Instant::now();
	for attempt in 0..MAX_SUBMIT_ATTEMPTS {
		let lease = wait_for_leader(shared).await?;

		// async-nats does not check a request against the server's max_payload, and the server
		// answers an oversized message by closing the whole connection. A request that would not fit
		// is split into chunks instead, which only a leader at the chunked protocol version accepts.
		let max_payload = nats_max_payload(&nats.client);
		let request = if payload.len() <= max_payload {
			let subject = nats.subjects.commit(&lease.leader_addr);
			let request = nats.client.request(subject, payload.clone().into());
			async { request.await.map_err(anyhow::Error::from) }.boxed()
		} else {
			if protocol_version < codec::CHUNKED_COMMIT_PROTOCOL_VERSION {
				bail!(
					"commit request is {} bytes, over the nats server max_payload of {max_payload} bytes, \
					 and the fleet has not negotiated chunked commit requests (protocol version \
					 {protocol_version}); raise the nats max_payload or finish upgrading every node",
					payload.len()
				);
			}
			let chunks = codec::encode_commit_request_chunks(
				&payload,
				shared.node_id.as_bytes(),
				client_seq as u64,
				attempt as u32,
				max_payload,
				protocol_version,
			)
			.context("failed to chunk commit request")?;
			let subject = nats.subjects.commit_chunk(&lease.leader_addr);
			send_chunks(&nats.client, subject, chunks).boxed()
		};

		match tokio::time::timeout(REQUEST_TIMEOUT, request).await {
			Ok(Ok(msg)) => match codec::decode_commit_reply(&msg.payload) {
				Ok(CommitOutcome::Committed { .. }) => {
					tracing::debug!(
						client_seq,
						attempt,
						wait_ms = submit_start.elapsed().as_millis() as u64,
						"udb commit resolved: committed"
					);
					return Ok(());
				}
				Ok(CommitOutcome::Conflict) => {
					// As in the single-node path, a cold-window rejection is reported as a conflict.
					return Err(anyhow::Error::from(DatabaseError::NotCommitted)
						.context("leader resolved the commit as a conflict"));
				}
				Err(err) => {
					tracing::warn!(?err, client_seq, "malformed udb commit reply; resending");
				}
			},
			// Indeterminate (no responder / transport error / timeout): the leader may have died
			// before or after applying. Resend the same dedup key; the leader dedups any double apply.
			Ok(Err(err)) => {
				tracing::debug!(
					?err,
					client_seq,
					attempt,
					"udb commit request errored; resending"
				);
			}
			Err(_) => {
				tracing::debug!(
					client_seq,
					attempt,
					"udb commit request timed out; resending"
				);
			}
		}

		tokio::time::sleep(RESEND_BACKOFF).await;
	}

	tracing::warn!(
		client_seq,
		wait_ms = submit_start.elapsed().as_millis() as u64,
		"udb commit exhausted resend attempts; treating as not committed"
	);
	Err(
		anyhow::Error::from(DatabaseError::NotCommitted).context(format!(
			"exhausted {MAX_SUBMIT_ATTEMPTS} commit resend attempts without a determinate reply"
		)),
	)
}

/// The largest message the connected NATS server accepts.
fn nats_max_payload(client: &async_nats::Client) -> usize {
	match client.server_info().max_payload {
		// A client that has not received the server's INFO yet reports zero, so assume the server
		// default rather than refusing to send.
		0 => NATS_DEFAULT_MAX_PAYLOAD,
		max_payload => max_payload,
	}
}

/// Send every chunk except the last as a plain publish and the last as the request, so the reply
/// arrives once the leader holds the whole commit.
async fn send_chunks(
	client: &async_nats::Client,
	subject: String,
	mut chunks: Vec<Vec<u8>>,
) -> Result<async_nats::Message> {
	let last = chunks
		.pop()
		.context("chunked commit request has no chunks")?;
	for chunk in chunks {
		client
			.publish(subject.clone(), chunk.into())
			.await
			.context("failed to publish commit request chunk")?;
	}
	client
		.request(subject, last.into())
		.await
		.context("commit request chunk got no reply")
}

/// Wait for a known leader, returning a retryable error if none is elected in time.
async fn wait_for_leader(shared: &Arc<PostgresShared>) -> Result<LeaseInfo> {
	let deadline = Instant::now() + LEADER_WAIT_TIMEOUT;
	loop {
		if let Some(lease) = shared.current_lease() {
			return Ok(lease);
		}
		if Instant::now() >= deadline {
			return Err(
				anyhow::Error::from(DatabaseError::NotCommitted).context(format!(
					"no leader elected within {}s",
					LEADER_WAIT_TIMEOUT.as_secs()
				)),
			);
		}
		tokio::time::sleep(LEADER_POLL_INTERVAL).await;
	}
}
