use std::{
	sync::{
		Arc,
		atomic::{AtomicI64, AtomicU64, Ordering},
	},
	time::Duration,
};

use deadpool_postgres::Pool;
use futures_util::StreamExt;
use tokio::sync::{Notify, watch};

use super::transport::Transport;

/// The singleton row id of `udb_lease`.
pub const LEASE_ID: i32 = 1;

/// How often a node refreshes its cached lease row (epoch, leader id, watermark) as a backstop to the
/// watermark broadcast. A stale-but-older watermark only widens the conflict window, so this can be
/// loose.
const LEASE_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Cached view of the current leader lease, as seen by a follower.
#[derive(Clone, Debug)]
pub struct LeaseInfo {
	pub epoch: i64,
	/// Node id of the current leader, used to build its commit subject.
	pub leader_addr: String,
}

/// Process-wide state shared by the follower transaction tasks and the leader resolver. Every node is
/// both a follower (it submits its own commits) and, in multi-node mode, a candidate leader.
pub struct PostgresShared {
	pub pool: Pool,
	/// Unique per-process id. Names this node's commit subject and is the dedup `client_node_id`.
	pub node_id: String,
	/// How follower commits reach the leader (in-process channel or NATS).
	pub transport: Transport,
	/// Highest durable commit version (`udb_lease.durable_version`); the follower read version.
	durable_version: AtomicI64,
	/// Pinged whenever `durable_version` advances.
	watermark_notify: Notify,
	/// Per-process monotonic commit sequence, the dedup `client_seq`.
	commit_seq: AtomicU64,
	lease_tx: watch::Sender<Option<LeaseInfo>>,
	lease_rx: watch::Receiver<Option<LeaseInfo>>,
}

impl PostgresShared {
	pub fn new(pool: Pool, node_id: String, transport: Transport) -> Arc<Self> {
		let (lease_tx, lease_rx) = watch::channel(None);
		let shared = Arc::new(Self {
			pool,
			node_id,
			transport,
			durable_version: AtomicI64::new(0),
			watermark_notify: Notify::new(),
			commit_seq: AtomicU64::new(0),
			lease_tx,
			lease_rx,
		});

		// Single-node advances `durable_version` and the lease cache in-process, so it needs no
		// cross-process refresh. Multi-node refreshes from the NATS watermark broadcast and a lease
		// row poll.
		if matches!(shared.transport, Transport::MultiNode(_)) {
			tokio::spawn(Self::cache_refresh_task(shared.clone()));
		}

		shared
	}

	/// Whether this driver is running in multi-node mode.
	pub fn is_multi_node(&self) -> bool {
		matches!(self.transport, Transport::MultiNode(_))
	}

	/// The cached follower read version (`durable_version`).
	pub fn read_version(&self) -> i64 {
		self.durable_version.load(Ordering::SeqCst)
	}

	/// Allocate the next per-process commit sequence for the failover dedup key.
	pub fn next_commit_seq(&self) -> i64 {
		self.commit_seq.fetch_add(1, Ordering::Relaxed) as i64
	}

	/// Advance the cached watermark monotonically and wake any waiters.
	pub fn advance_durable_version(&self, version: i64) {
		let prev = self.durable_version.fetch_max(version, Ordering::SeqCst);
		if version > prev {
			self.watermark_notify.notify_waiters();
		}
	}

	/// Current cached lease, if known.
	pub fn current_lease(&self) -> Option<LeaseInfo> {
		self.lease_rx.borrow().clone()
	}

	/// Publish a freshly observed/elected lease into the cache.
	pub fn set_lease(&self, lease: LeaseInfo) {
		let changed = self
			.lease_rx
			.borrow()
			.as_ref()
			.map(|prev| prev.epoch != lease.epoch || prev.leader_addr != lease.leader_addr)
			.unwrap_or(true);
		if changed {
			tracing::debug!(
				epoch = lease.epoch,
				leader_addr = %lease.leader_addr,
				self_node = %self.node_id,
				is_self = (lease.leader_addr == self.node_id),
				"udb follower observed leader lease change"
			);
		}
		let _ = self.lease_tx.send(Some(lease));
	}

	/// Background task (multi-node only): keep `durable_version` and the cached lease fresh via the
	/// NATS watermark broadcast plus a periodic poll of `udb_lease`.
	async fn cache_refresh_task(shared: Arc<Self>) {
		let Transport::MultiNode(nats) = &shared.transport else {
			return;
		};

		let mut watermark_sub = match nats.client.subscribe(nats.subjects.watermark()).await {
			Ok(sub) => sub,
			Err(err) => {
				tracing::error!(
					?err,
					"failed to subscribe to udb watermark; relying on lease poll"
				);
				return shared.lease_poll_only().await;
			}
		};

		let mut interval = tokio::time::interval(LEASE_REFRESH_INTERVAL);
		interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

		loop {
			tokio::select! {
				msg = watermark_sub.next() => {
					match msg {
						Some(msg) => {
							match super::codec::decode_watermark(&msg.payload) {
								Ok(version) => shared.advance_durable_version(version),
								Err(err) => {
									tracing::debug!(?err, "failed to decode udb watermark")
								}
							}
						}
						// The subscription ended (client closed). Fall back to lease polling only.
						None => return shared.lease_poll_only().await,
					}
				}
				_ = interval.tick() => {
					shared.refresh_lease_row().await;
				}
			}
		}
	}

	/// Degraded refresh path: poll the lease row when the watermark subscription is unavailable.
	async fn lease_poll_only(self: Arc<Self>) {
		let mut interval = tokio::time::interval(LEASE_REFRESH_INTERVAL);
		interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
		loop {
			interval.tick().await;
			self.refresh_lease_row().await;
		}
	}

	async fn refresh_lease_row(&self) {
		let conn = match self.pool.get().await {
			Ok(conn) => conn,
			Err(err) => {
				tracing::debug!(?err, "failed to get connection for lease refresh");
				return;
			}
		};

		let row = conn
			.query_opt(
				"SELECT epoch, leader_addr, durable_version FROM udb_lease WHERE id = $1",
				&[&LEASE_ID],
			)
			.await;

		match row {
			Ok(Some(row)) => {
				let epoch: i64 = row.get(0);
				let leader_addr: String = row.get(1);
				let durable_version: i64 = row.get(2);
				self.advance_durable_version(durable_version);
				self.set_lease(LeaseInfo { epoch, leader_addr });
			}
			Ok(None) => {
				// No lease row yet; no leader elected.
			}
			Err(err) => {
				tracing::debug!(?err, "failed to refresh lease row");
			}
		}
	}
}
