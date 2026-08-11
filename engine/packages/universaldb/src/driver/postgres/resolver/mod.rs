mod apply;
mod lease;

use std::{
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::task::AbortOnDropHandle;

use crate::{conflict_tracker::TransactionConflictTracker, transaction::TXN_TIMEOUT};

use lease::LEASE_TTL_SECS;

use super::{
	shared::{LEASE_ID, LeaseInfo, PostgresShared},
	transport::{CommitJob, CommitOutcome, Transport},
};

/// Max commits resolved+applied per batch (group commit). Amortizes the resolver, Postgres
/// round-trips, and fsync across the batch.
const DRAIN_BATCH_SIZE: usize = 256;

/// How often a leader renews its lease. Must be comfortably under `LEASE_TTL_SECS`.
const RENEW_INTERVAL: Duration = Duration::from_secs(3);

/// How long a candidate waits before retrying election when another node holds the lease.
const ELECTION_RETRY: Duration = Duration::from_secs(2);

/// Single-node leader-acquire gate: total time to keep retrying before giving up and failing
/// startup. Must exceed the lease TTL so a crashed predecessor's lease has time to expire.
const GATE_TOTAL: Duration = Duration::from_secs((LEASE_TTL_SECS as u64) * 2 + 5);
/// Backoff between single-node gate attempts.
const GATE_RETRY: Duration = Duration::from_secs(1);

/// What feeds the leader drain loop. Single-node owns the process-wide commit receiver and an
/// already-acquired lease epoch (the startup gate ran before this task spawned). Multi-node creates a
/// fresh NATS-fed receiver each time it wins an election.
pub enum ResolverInput {
	SingleNode {
		rx: mpsc::Receiver<CommitJob>,
		initial_epoch: i64,
	},
	MultiNode,
}

/// Single-node startup gate: acquire the leader lease, retrying with backoff. Fails if it cannot be
/// acquired within [`GATE_TOTAL`], which means either another engine instance is running against this
/// Postgres (a real misconfiguration in single-node mode) or a previous instance crashed without
/// releasing its lease and it has not yet expired. Each failed attempt warns.
pub async fn acquire_single_node_gate(shared: &Arc<PostgresShared>) -> Result<i64> {
	let deadline = Instant::now() + GATE_TOTAL;
	let mut attempt = 0u32;
	loop {
		attempt += 1;
		match lease::try_acquire(&shared.pool, &shared.node_id).await {
			Ok(Some(acquired)) => {
				tracing::debug!(
					epoch = acquired.epoch,
					attempt,
					node_id = %shared.node_id,
					"acquired udb postgres single-node leader lease"
				);
				return Ok(acquired.epoch);
			}
			Ok(None) => {
				tracing::warn!(
					attempt,
					"udb postgres single-node could not acquire leader lease. another instance may hold it \
					or a previous instance did not release it; backing off"
				);
			}
			Err(err) => {
				tracing::warn!(
					?err,
					attempt,
					"udb postgres single-node lease acquire errored, retrying"
				);
			}
		}

		if Instant::now() >= deadline {
			bail!(
				"udb postgres single-node failed to acquire leader lease after {attempt} attempts; refusing \
				to start. another engine instance may be running against this postgres, or a \
				previous instance crashed without releasing its lease (wait for it to expire). if you intend \
				to run a multi-node setup you must configure NATS."
			);
		}

		tokio::time::sleep(GATE_RETRY).await;
	}
}

/// Spawn the per-process resolver task. The returned handle is aborted when the owning driver drops,
/// which stops lease renewal so the lease expires and another node can take over.
pub fn spawn(shared: Arc<PostgresShared>, input: ResolverInput) -> tokio::task::JoinHandle<()> {
	tokio::spawn(run(shared, input))
}

async fn run(shared: Arc<PostgresShared>, input: ResolverInput) {
	match input {
		ResolverInput::SingleNode { rx, initial_epoch } => {
			run_single_node(shared, rx, initial_epoch).await
		}
		ResolverInput::MultiNode => run_multi_node(shared).await,
	}
}

/// Single-node: this node is the only node and is always the leader. The startup gate already
/// acquired the lease, so lead immediately. If the lease is ever lost (another node appeared, a real
/// misconfiguration), log loudly and re-acquire through the gate.
async fn run_single_node(
	shared: Arc<PostgresShared>,
	mut rx: mpsc::Receiver<CommitJob>,
	initial_epoch: i64,
) {
	let mut epoch = initial_epoch;
	loop {
		if let Err(err) = lead(&shared, epoch, &mut rx).await {
			tracing::error!(?err, "udb postgres single-node leader loop errored");
		}
		tracing::error!(
			epoch,
			"udb postgres single-node lost the leader lease; re-acquiring (another engine instance may be \
			running against this postgres)"
		);
		epoch = loop {
			match acquire_single_node_gate(&shared).await {
				Ok(epoch) => break epoch,
				Err(err) => {
					tracing::error!(?err, "udb postgres single-node re-acquire failed; retrying");
				}
			}
		};
	}
}

/// Multi-node: race the lease against other nodes; whoever wins leads until it loses the lease.
async fn run_multi_node(shared: Arc<PostgresShared>) {
	loop {
		match lease::try_acquire(&shared.pool, &shared.node_id).await {
			Ok(Some(acquired)) => {
				tracing::info!(epoch = acquired.epoch, node_id = %shared.node_id, "acquired udb postgres leader lease");

				// Each leadership term gets its own NATS-fed commit queue. The subscriber forwards
				// decoded commit requests into `rx`; aborting it on step-down stops accepting commits.
				let (tx, mut rx) = mpsc::channel(super::transport::COMMIT_QUEUE_BOUND);
				let subscriber = spawn_commit_subscriber(&shared, tx);

				if let Err(err) = lead(&shared, acquired.epoch, &mut rx).await {
					tracing::error!(?err, "udb leader loop errored, stepping down");
				}
				if let Some(handle) = subscriber {
					handle.abort();
				}
				tracing::info!(
					epoch = acquired.epoch,
					"stepped down from udb postgres leader"
				);
			}
			Ok(None) => wait_for_election_retry(&shared).await,
			Err(err) => {
				tracing::warn!(?err, "failed udb lease acquire attempt");
				wait_for_election_retry(&shared).await;
			}
		}
	}
}

/// Spawn the leader's NATS commit subscriber for multi-node. Returns `None` in single-node (no NATS).
fn spawn_commit_subscriber(
	shared: &Arc<PostgresShared>,
	tx: mpsc::Sender<CommitJob>,
) -> Option<AbortOnDropHandle<()>> {
	let Transport::MultiNode(nats) = &shared.transport else {
		return None;
	};
	let client = nats.client.clone();
	let subject = nats.subjects.commit(&shared.node_id);
	Some(AbortOnDropHandle::new(tokio::spawn(async move {
		if let Err(err) = super::nats::run_commit_subscriber(client, subject, tx).await {
			tracing::warn!(?err, "udb commit subscriber ended");
		}
	})))
}

/// Wait before retrying the election: either the `ELECTION_RETRY` backstop elapses, or a departing
/// leader wakes us via the election broadcast so handoff is near-instant.
async fn wait_for_election_retry(shared: &Arc<PostgresShared>) {
	let Transport::MultiNode(nats) = &shared.transport else {
		tokio::time::sleep(ELECTION_RETRY).await;
		return;
	};

	let election = nats.client.subscribe(nats.subjects.election()).await;
	match election {
		Ok(mut sub) => {
			tokio::select! {
				_ = tokio::time::sleep(ELECTION_RETRY) => {}
				_ = sub.next() => {}
			}
		}
		Err(_) => tokio::time::sleep(ELECTION_RETRY).await,
	}
}

/// Best-effort graceful leadership handoff invoked on shutdown. If this node holds the lease, expire
/// it and wake standbys so they take over immediately instead of waiting out the TTL. Safe to call on
/// a follower. The caller must already have stopped lease renewal before calling this.
pub async fn handoff(shared: &Arc<PostgresShared>) {
	match lease::release(&shared.pool, &shared.node_id).await {
		Ok(true) => {
			tracing::info!(node_id = %shared.node_id, "released udb postgres leader lease for graceful handoff");
			if let Transport::MultiNode(nats) = &shared.transport {
				if let Err(err) = nats
					.client
					.publish(nats.subjects.election(), Vec::new().into())
					.await
				{
					tracing::debug!(?err, "failed to publish udb election wake");
				}
			}
		}
		Ok(false) => {}
		Err(err) => tracing::warn!(?err, "failed to release udb lease on shutdown"),
	}
}

/// Leader entry point: publish our lease, compute the cold-window floor, then run renewal and
/// draining. Renewal runs on its own task so a long drain cannot starve it past the lease TTL; the
/// drain loop runs inline so it can borrow the commit receiver. Returns when the lease is lost (renew
/// reports it gone, or an apply is epoch-fenced).
async fn lead(
	shared: &Arc<PostgresShared>,
	epoch: i64,
	rx: &mut mpsc::Receiver<CommitJob>,
) -> Result<()> {
	// Publish our own lease into the cache immediately so our local commits route to us.
	shared.set_lease(LeaseInfo {
		epoch,
		leader_addr: shared.node_id.clone(),
	});

	// The recovery floor: a freshly elected leader has a cold conflict window, so reject commits whose
	// read_version predates the floor until the window warms (one TXN_TIMEOUT), forcing those
	// followers to take a fresh read_version.
	let recovery_version = recovery_floor(shared).await?;
	let recovery_deadline = Instant::now() + TXN_TIMEOUT;

	// Seed our read-version cache to the durable floor so our own follower reads are not cold-window
	// rejected (a read at `read_version < recovery_version` is rejected during the cold window, and the
	// cache otherwise starts at 0). Followers learn the floor from the watermark broadcast / lease poll.
	shared.advance_durable_version(recovery_version as i64);

	tracing::debug!(
		epoch,
		recovery_version,
		cold_window_ms = TXN_TIMEOUT.as_millis() as u64,
		multi_node = shared.is_multi_node(),
		"udb leader entering lead loop"
	);

	let tracker = TransactionConflictTracker::new();
	let mut renew = AbortOnDropHandle::new(tokio::spawn(renew_loop(shared.clone(), epoch)));

	let drain = drain_loop(
		shared,
		epoch,
		&tracker,
		recovery_version,
		recovery_deadline,
		rx,
	);
	tokio::pin!(drain);

	// Whichever finishes first (lease lost via renew, or epoch fenced during a drain apply) ends the
	// leadership term. The renew handle yields a join result; the inline drain yields directly.
	tokio::select! {
		res = &mut renew => res?,
		res = &mut drain => res,
	}
}

/// Lease-renewal loop. Runs on its own task and pool connection so it cannot be starved by drain
/// work. Returns when the lease is definitively gone.
async fn renew_loop(shared: Arc<PostgresShared>, epoch: i64) -> Result<()> {
	let mut interval = tokio::time::interval(RENEW_INTERVAL);
	interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
	// The lease was just acquired (expires_at = now + TTL), so consume the immediate first tick and
	// renew after one interval.
	interval.tick().await;

	let mut last_renew = Instant::now();

	loop {
		interval.tick().await;

		match lease::renew(&shared.pool, &shared.node_id, epoch).await {
			Ok(true) => last_renew = Instant::now(),
			Ok(false) => {
				tracing::warn!(
					epoch,
					"udb leader lost lease on renew (epoch bumped); stepping down"
				);
				return Ok(());
			}
			Err(err) => {
				if last_renew.elapsed() >= Duration::from_secs(LEASE_TTL_SECS as u64) {
					tracing::warn!(
						?err,
						epoch,
						"udb leader renew failing past lease TTL; assuming lease lost and stepping down"
					);
					return Ok(());
				}
				tracing::warn!(?err, epoch, "udb leader lease renew errored; will retry");
			}
		}
	}
}

/// Drain loop. Collects one batch of commit jobs from the queue and processes it per iteration.
/// Returns when this leader's epoch is fenced out during an apply, or the queue closes.
async fn drain_loop(
	shared: &Arc<PostgresShared>,
	epoch: i64,
	tracker: &TransactionConflictTracker,
	recovery_version: u64,
	recovery_deadline: Instant,
	rx: &mut mpsc::Receiver<CommitJob>,
) -> Result<()> {
	loop {
		let batch = collect_batch(rx).await;
		if batch.is_empty() {
			// The queue closed (all senders dropped). Step down.
			return Ok(());
		}

		match drain_batch(
			shared,
			epoch,
			tracker,
			recovery_version,
			recovery_deadline,
			batch,
		)
		.await?
		{
			BatchOutcome::LostLease => {
				tracing::warn!(epoch, "udb leader stepping down: epoch fenced during apply");
				return Ok(());
			}
			BatchOutcome::Processed => continue,
		}
	}
}

/// Collect up to [`DRAIN_BATCH_SIZE`] jobs from the queue, blocking for the first one and then
/// draining any immediately-available followers without waiting. Returns an empty batch only when the
/// queue is closed and drained.
async fn collect_batch(rx: &mut mpsc::Receiver<CommitJob>) -> Vec<CommitJob> {
	let mut batch = Vec::with_capacity(DRAIN_BATCH_SIZE);
	rx.recv_many(&mut batch, DRAIN_BATCH_SIZE).await;
	batch
}

/// The cold-window rejection floor for a freshly elected leader: the durable watermark at election
/// time. A new leader's tracker is empty, so it cannot detect a conflict against a previous leader's
/// winner; every such winner has `commit_version <= durable_version` (applied and folded into
/// `durable_version` in one txn), so a commit is safe if `read_version >= durable_version`.
async fn recovery_floor(shared: &Arc<PostgresShared>) -> Result<u64> {
	let durable = lease::current_durable_version(&shared.pool).await?;
	Ok(durable.max(0) as u64)
}

enum BatchOutcome {
	Processed,
	LostLease,
}

async fn drain_batch(
	shared: &Arc<PostgresShared>,
	epoch: i64,
	tracker: &TransactionConflictTracker,
	recovery_version: u64,
	recovery_deadline: Instant,
	mut jobs: Vec<CommitJob>,
) -> Result<BatchOutcome> {
	let batch_start = Instant::now();
	let batch_len = jobs.len();

	let mut conn = shared
		.pool
		.get()
		.await
		.context("failed to get connection for drain batch")?;
	let txn = conn
		.build_transaction()
		.start()
		.await
		.context("failed to start drain batch txn")?;

	// Build the failover dedup keys: a job whose (client_node_id, client_seq) is already recorded in
	// udb_applied was committed by a prior leader; respond with the recorded version and do not
	// re-apply. Single-node jobs carry no dedup key and never hit this path.
	let mut dedup_nids: Vec<Vec<u8>> = Vec::new();
	let mut dedup_seqs: Vec<i64> = Vec::new();
	for job in &jobs {
		if let Some(key) = &job.dedup_key {
			dedup_nids.push(key.client_node_id.clone());
			dedup_seqs.push(key.client_seq);
		}
	}

	// Pipeline the dedup pre-check and commit-version allocation on the same connection. Versions are
	// allocated for every job rather than only to-resolve jobs, so the version count no longer depends
	// on the dedup result and the two queries have no data dependency; tokio-postgres pipelines them
	// into a single round-trip. A dedup hit wastes its allocated version, but sequence gaps are already
	// expected (conflict losers and rolled-back batches burn versions too) and do not affect
	// versionstamp monotonicity.
	let job_count = jobs.len() as i64;
	let dedup_fut = async {
		if dedup_nids.is_empty() {
			return anyhow::Ok(HashMap::<(Vec<u8>, i64), i64>::new());
		}
		let applied = txn
			.query(
				"SELECT a.client_node_id, a.client_seq, a.commit_version
				 FROM udb_applied a
				 JOIN unnest($1::bytea[], $2::bigint[]) AS q(nid, seq)
				   ON a.client_node_id = q.nid AND a.client_seq = q.seq",
				&[&dedup_nids, &dedup_seqs],
			)
			.await
			.context("failed dedup pre-check")?
			.into_iter()
			.map(|row| {
				(
					(row.get::<_, Vec<u8>>(0), row.get::<_, i64>(1)),
					row.get::<_, i64>(2),
				)
			})
			.collect();
		anyhow::Ok(applied)
	};
	let versions_fut = async {
		let versions = txn
			.query(
				"SELECT nextval('udb_version_seq') FROM generate_series(1, $1::bigint)",
				&[&job_count],
			)
			.await
			.context("failed to allocate commit versions")?
			.into_iter()
			.map(|row| row.get::<_, i64>(0))
			.collect::<Vec<i64>>();
		anyhow::Ok(versions)
	};
	let (applied, mut versions) = tokio::try_join!(dedup_fut, versions_fut)?;

	// Postgres does not guarantee nextval is evaluated in row order, so the versions are sorted and
	// assigned to to-resolve jobs in arrival order to keep versionstamps monotonic with commit order
	// (load-bearing for epoxy changelog catch-up + depot PITR).
	versions.sort_unstable();

	// Classify each job: a dedup hit resolves immediately; everything else needs version assignment
	// and conflict resolution.
	let mut outcomes: Vec<Option<CommitOutcome>> = vec![None; jobs.len()];
	let mut resolve_indices: Vec<usize> = Vec::with_capacity(jobs.len());
	for (i, job) in jobs.iter().enumerate() {
		if let Some(key) = &job.dedup_key {
			if let Some(&cv) = applied.get(&(key.client_node_id.clone(), key.client_seq)) {
				outcomes[i] = Some(CommitOutcome::Committed { commit_version: cv });
				continue;
			}
		}
		resolve_indices.push(i);
	}

	let cold_window = Instant::now() < recovery_deadline;
	let mut winners: Vec<apply::Winner> = Vec::new();
	let mut winner_dedup_nids: Vec<Vec<u8>> = Vec::new();
	let mut winner_dedup_seqs: Vec<i64> = Vec::new();
	let mut winner_dedup_cvs: Vec<i64> = Vec::new();
	let mut max_winner_cv: i64 = 0;
	let mut committed_count = 0u32;
	let mut conflict_count = 0u32;
	let mut cold_reject_count = 0u32;

	for (slot, &i) in resolve_indices.iter().enumerate() {
		let commit_version = versions[slot];
		let job = &mut jobs[i];
		let start_version = job.read_version;
		let conflict_ranges = std::mem::take(&mut job.conflict_ranges);

		let cold_rejected = cold_window && start_version < recovery_version;
		let conflicted = if cold_rejected {
			cold_reject_count += 1;
			true
		} else {
			tracker
				.check_and_insert(start_version, commit_version.max(0) as u64, conflict_ranges)
				.await
		};

		if conflicted {
			if !cold_rejected {
				conflict_count += 1;
			}
			outcomes[i] = Some(CommitOutcome::Conflict);
		} else {
			committed_count += 1;
			outcomes[i] = Some(CommitOutcome::Committed { commit_version });
			max_winner_cv = max_winner_cv.max(commit_version);
			if let Some(key) = &job.dedup_key {
				winner_dedup_nids.push(key.client_node_id.clone());
				winner_dedup_seqs.push(key.client_seq);
				winner_dedup_cvs.push(commit_version);
			}
			winners.push(apply::Winner {
				commit_version: commit_version.max(0) as u64,
				operations: std::mem::take(&mut job.operations),
			});
		}
	}

	// Bulk-read the pre-batch value of every key a winner's atomic op reads, then fold all winners
	// into one materialized write-set in memory.
	let atomic_keys = apply::atomic_read_keys(&winners);
	let base = if atomic_keys.is_empty() {
		HashMap::new()
	} else {
		txn.query(
			"SELECT key, value FROM kv WHERE key = ANY($1::bytea[])",
			&[&atomic_keys],
		)
		.await
		.context("failed to bulk-read atomic op keys")?
		.into_iter()
		.map(|row| (row.get::<_, Vec<u8>>(0), row.get::<_, Vec<u8>>(1)))
		.collect()
	};

	let apply::WriteSet {
		upserts,
		point_deletes,
		range_deletes,
	} = apply::fold_winners(winners, &base).context("failed to fold batch winners")?;

	let (upsert_keys, upsert_values): (Vec<Vec<u8>>, Vec<Vec<u8>>) = upserts.into_iter().unzip();
	let (range_begins, range_ends): (Vec<Vec<u8>>, Vec<Vec<u8>>) =
		range_deletes.into_iter().unzip();

	// Range deletes run in their own statement before the apply CTE: a range delete and an in-range
	// upsert in one CTE would have unspecified ordering, so the clear must commit its effect first and
	// the upsert then re-inserts the key.
	if !range_begins.is_empty() {
		txn.execute(
			"DELETE FROM kv USING unnest($1::bytea[], $2::bytea[]) AS r(b, e)
			 WHERE key >= r.b AND key < r.e",
			&[&range_begins, &range_ends],
		)
		.await
		.context("failed to clear ranges")?;
	}

	// Apply the rest of the batch in one CTE: point deletes, the kv upsert, the dedup records for
	// multi-node winners, and the epoch-fenced watermark advance. A zombie old leader whose epoch was
	// bumped sees zero rows from the lease UPDATE and steps down before any write becomes visible.
	let new_durable: i64 = match txn
		.query_opt(
			"WITH pdel AS (
				DELETE FROM kv WHERE key = ANY($1::bytea[])
			), up AS (
				INSERT INTO kv (key, value)
				SELECT * FROM unnest($2::bytea[], $3::bytea[])
				ON CONFLICT (key) DO UPDATE SET value = excluded.value
			), applied AS (
				INSERT INTO udb_applied (client_node_id, client_seq, commit_version)
				SELECT * FROM unnest($4::bytea[], $5::bigint[], $6::bigint[])
				ON CONFLICT (client_node_id, client_seq) DO NOTHING
			)
			UPDATE udb_lease
			   SET durable_version = GREATEST(durable_version, $7)
			 WHERE id = $8 AND epoch = $9
			 RETURNING durable_version",
			&[
				&point_deletes,
				&upsert_keys,
				&upsert_values,
				&winner_dedup_nids,
				&winner_dedup_seqs,
				&winner_dedup_cvs,
				&max_winner_cv,
				&LEASE_ID,
				&epoch,
			],
		)
		.await
		.context("failed to apply batch and advance watermark")?
	{
		Some(row) => row.get(0),
		None => {
			txn.rollback().await.ok();
			return Ok(BatchOutcome::LostLease);
		}
	};

	txn.commit().await.context("failed to commit drain batch")?;

	// The watermark advances strictly after the apply txn is durably committed and visible, so a
	// reader handed this read_version can never miss a write with commit_version <= read_version.
	shared.advance_durable_version(new_durable);

	if let Transport::MultiNode(nats) = &shared.transport {
		match super::codec::encode_watermark(new_durable) {
			Ok(payload) => {
				if let Err(err) = nats
					.client
					.publish(nats.subjects.watermark(), payload.into())
					.await
				{
					tracing::debug!(?err, "failed to publish udb watermark");
				}
			}
			Err(err) => tracing::error!(?err, "failed to encode udb watermark"),
		}
	}

	// Respond to every job (dedup hits, winners, losers). Responses are independent per job, so fan the
	// replies out concurrently instead of awaiting each publish in series.
	futures_util::stream::iter(jobs.into_iter().enumerate())
		.for_each_concurrent(None, |(i, job)| {
			let outcome = outcomes[i].expect("every job must be resolved");
			async move {
				job.responder.respond(outcome).await;
			}
		})
		.await;

	tracing::debug!(
		epoch,
		batch_len,
		committed = committed_count,
		conflict = conflict_count,
		cold_reject = cold_reject_count,
		cold_window,
		new_durable,
		batch_ms = batch_start.elapsed().as_millis() as u64,
		"udb leader processed commit batch"
	);

	Ok(BatchOutcome::Processed)
}
