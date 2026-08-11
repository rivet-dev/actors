use tokio::sync::{mpsc, oneshot};

use crate::{options::ConflictRangeType, tx_ops::Operation};

use super::{codec, nats::NatsTransport};

/// Bound on the in-process commit queue feeding the leader drain loop. The leader drains in large
/// batches, so this only needs to absorb a brief burst between batches.
pub const COMMIT_QUEUE_BOUND: usize = 4096;

/// How the follower commit path reaches the leader resolver.
///
/// Single-node keeps an in-process channel directly into the leader drain loop (this node is always
/// the leader). Multi-node sends the commit to the elected leader over NATS request/reply; the reply
/// carries the commit result.
pub enum Transport {
	SingleNode {
		/// Sender into the leader drain loop's job queue. The matching receiver is owned by the
		/// resolver task spawned at startup.
		commit_tx: mpsc::Sender<CommitJob>,
	},
	MultiNode(NatsTransport),
}

/// The outcome of resolving a single commit.
#[derive(Clone, Copy, Debug)]
pub enum CommitOutcome {
	/// The commit won resolution and its writes were durably applied at `commit_version`.
	Committed { commit_version: i64 },
	/// The commit lost resolution (read-write conflict or cold-window reject). Maps to the
	/// retryable `DatabaseError::NotCommitted` on the follower.
	Conflict,
}

/// How the leader delivers a commit result back to the waiting follower.
pub enum Responder {
	/// Single-node: resolve the follower's `oneshot` directly.
	Local(oneshot::Sender<CommitOutcome>),
	/// Multi-node: publish the encoded outcome to the NATS request's reply inbox.
	Nats {
		client: async_nats::Client,
		reply: async_nats::Subject,
	},
}

impl Responder {
	/// Deliver the outcome to the follower. Best-effort: a follower that already gave up (oneshot
	/// dropped, or no NATS responder) is covered by its own retry path.
	pub async fn respond(self, outcome: CommitOutcome) {
		match self {
			Responder::Local(tx) => {
				let _ = tx.send(outcome);
			}
			Responder::Nats { client, reply } => {
				let payload = match codec::encode_commit_reply(outcome) {
					Ok(payload) => payload,
					Err(err) => {
						tracing::error!(?err, "failed to encode udb commit reply");
						return;
					}
				};
				if let Err(err) = client.publish(reply, payload.into()).await {
					tracing::debug!(?err, "failed to publish udb commit reply");
				}
			}
		}
	}
}

/// A single commit handed to the leader drain loop, transport-agnostic. Single-node jobs are built by
/// the follower commit path; multi-node jobs are built by the NATS commit subscriber from a decoded
/// request.
pub struct CommitJob {
	pub read_version: u64,
	pub conflict_ranges: Vec<(Vec<u8>, Vec<u8>, ConflictRangeType)>,
	pub operations: Vec<Operation>,
	/// Failover dedup key, present only in multi-node (single-node has no lost-reply window).
	pub dedup_key: Option<DedupKey>,
	pub responder: Responder,
}

/// Identifies a single logical commit across follower resends so the leader applies it exactly once.
#[derive(Clone, Debug)]
pub struct DedupKey {
	pub client_node_id: Vec<u8>,
	pub client_seq: i64,
}
