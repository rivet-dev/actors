use anyhow::{Context, Result};
use deadpool_postgres::Pool;

use crate::driver::postgres::shared::LEASE_ID;

/// Lease time-to-live. A leader renews well within this; a candidate may take over only after it
/// expires.
pub const LEASE_TTL_SECS: i64 = 10;

/// Outcome of a leadership acquisition attempt.
pub struct Acquired {
	pub epoch: i64,
}

/// Attempt to acquire or take over the leader lease via an epoch CAS. Succeeds if there is no lease
/// row yet, or the existing lease has expired. Bumps `epoch` on every successful acquisition so a
/// superseded old leader is fenced out.
pub async fn try_acquire(pool: &Pool, node_id: &str) -> Result<Option<Acquired>> {
	let conn = pool
		.get()
		.await
		.context("failed to get connection for lease acquire")?;

	// Take over an expired (or absent) lease. The INSERT seeds the singleton row on first ever
	// election; thereafter the UPDATE path runs.
	let row = conn
		.query_opt(
			"INSERT INTO udb_lease (id, epoch, leader_addr, durable_version, expires_at)
			 VALUES ($1, 1, $2, 0, now() + ($3 || ' seconds')::interval)
			 ON CONFLICT (id) DO UPDATE
			   SET epoch = udb_lease.epoch + 1,
			       leader_addr = EXCLUDED.leader_addr,
			       expires_at = now() + ($3 || ' seconds')::interval
			   WHERE udb_lease.expires_at < now()
			 RETURNING epoch",
			&[&LEASE_ID, &node_id, &LEASE_TTL_SECS.to_string()],
		)
		.await
		.context("failed to run lease acquire query")?;

	Ok(row.map(|row| Acquired { epoch: row.get(0) }))
}

/// Renew the lease, fenced on this leader's epoch. Returns `false` if the lease was lost (another
/// node took over, bumping the epoch), in which case the caller must step down.
pub async fn renew(pool: &Pool, node_id: &str, epoch: i64) -> Result<bool> {
	let conn = pool
		.get()
		.await
		.context("failed to get connection for lease renew")?;

	let updated = conn
		.execute(
			"UPDATE udb_lease
			   SET expires_at = now() + ($3 || ' seconds')::interval
			 WHERE id = $1 AND epoch = $2 AND leader_addr = $4",
			&[&LEASE_ID, &epoch, &LEASE_TTL_SECS.to_string(), &node_id],
		)
		.await
		.context("failed to renew lease")?;

	Ok(updated == 1)
}

/// Gracefully release the lease so a standby node can take over immediately instead of waiting out
/// the TTL. Expires the lease in place, fenced on this node's address so it never clobbers a
/// successor that already took over. Returns `true` if our lease was released (i.e. we were the
/// leader); `false` is the normal no-op when this node is a follower. Renewal must already be
/// stopped before calling this, otherwise a racing renew could re-extend the lease.
pub async fn release(pool: &Pool, node_id: &str) -> Result<bool> {
	let conn = pool
		.get()
		.await
		.context("failed to get connection for lease release")?;

	let updated = conn
		.execute(
			"UPDATE udb_lease
			   SET expires_at = now()
			 WHERE id = $1 AND leader_addr = $2",
			&[&LEASE_ID, &node_id],
		)
		.await
		.context("failed to release lease")?;

	Ok(updated == 1)
}

/// Read the current durable version (`udb_lease.durable_version`). Used by a freshly elected leader
/// to learn the watermark floor it must continue from.
pub async fn current_durable_version(pool: &Pool) -> Result<i64> {
	let conn = pool
		.get()
		.await
		.context("failed to get connection for durable version read")?;

	let row = conn
		.query_opt(
			"SELECT durable_version FROM udb_lease WHERE id = $1",
			&[&LEASE_ID],
		)
		.await
		.context("failed to read durable version")?;

	Ok(row.map(|row| row.get::<_, i64>(0)).unwrap_or(0))
}
