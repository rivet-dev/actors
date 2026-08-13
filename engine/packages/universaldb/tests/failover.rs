use std::{sync::Arc, time::Duration};

use rivet_test_deps_docker::{TestDatabase, TestPubSub};
use tokio_postgres::NoTls;
use universaldb::{Database, driver::postgres::NatsConfig, utils::IsolationLevel::*};
use uuid::Uuid;

const ALPHA_KEY: &[u8] = b"failover/alpha";
const BETA_KEY: &[u8] = b"failover/beta";

/// Boot a NATS container and return the multi-node UniversalDB NATS config plus the docker handle
/// (kept alive for the test duration). Leader failover is a multi-node scenario, so the drivers must
/// share a NATS deployment for follower-to-leader commit transport and watermark/election broadcast.
async fn setup_nats() -> (NatsConfig, rivet_test_deps_docker::DockerRunConfig) {
	let (pubsub_config, docker_config) = TestPubSub::Nats.config(Uuid::new_v4(), 1).await.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();
	tokio::time::sleep(Duration::from_secs(1)).await;

	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!();
	};
	let config = NatsConfig {
		addresses: nats.addresses.clone(),
		username: nats.username.clone(),
		password: nats.password.as_ref().map(|p| p.read().clone()),
		client_capacity: nats.client_capacity,
		subscription_capacity: nats.subscription_capacity,
	};
	(config, docker_config)
}

/// Build a fresh multi-node Postgres-backed `Database`. Each call spins up an independent driver (its
/// own pool, node id, NATS client, and resolver), so two of them against one Postgres + one NATS model
/// two engine nodes.
async fn make_db(connection_string: &str, nats: &NatsConfig) -> Database {
	let mut config =
		universaldb::driver::postgres::PostgresConfig::new(connection_string.to_string());
	config.nats = Some(nats.clone());
	let driver = universaldb::driver::PostgresDatabaseDriver::new_with_config(config)
		.await
		.unwrap();
	Database::new(Arc::new(driver))
}

/// Raw verification connection used to inspect leader/lease/version state out of band.
async fn connect_raw(connection_string: &str) -> tokio_postgres::Client {
	let (client, connection) = tokio_postgres::connect(connection_string, NoTls)
		.await
		.unwrap();
	tokio::spawn(async move {
		let _ = connection.await;
	});
	client
}

struct LeaseRow {
	epoch: i64,
	leader_addr: String,
	durable_version: i64,
}

async fn read_lease(client: &tokio_postgres::Client) -> Option<LeaseRow> {
	let row = client
		.query_opt(
			"SELECT epoch, leader_addr, durable_version FROM udb_lease WHERE id = 1",
			&[],
		)
		.await
		.unwrap()?;
	Some(LeaseRow {
		epoch: row.get(0),
		leader_addr: row.get(1),
		durable_version: row.get(2),
	})
}

/// High-water of the LOGGED version sequence. A freshly elected leader must continue from at least
/// this value, never regress below it.
async fn read_seq_high(client: &tokio_postgres::Client) -> i64 {
	client
		.query_one("SELECT last_value FROM udb_version_seq", &[])
		.await
		.unwrap()
		.get(0)
}

/// Poll `udb_lease` until `pred` holds or the deadline passes.
async fn wait_for_lease<F: Fn(&LeaseRow) -> bool>(
	client: &tokio_postgres::Client,
	timeout: Duration,
	pred: F,
) -> LeaseRow {
	let deadline = tokio::time::Instant::now() + timeout;
	loop {
		if let Some(lease) = read_lease(client).await {
			if pred(&lease) {
				return lease;
			}
		}
		if tokio::time::Instant::now() >= deadline {
			panic!("timed out waiting for lease condition");
		}
		tokio::time::sleep(Duration::from_millis(200)).await;
	}
}

async fn write_key(db: &Database, key: &'static [u8], value: &'static [u8]) {
	db.txn("test_failover", move |tx| async move {
		tx.set(key, value);
		Ok(())
	})
	.await
	.unwrap();
}

async fn read_key(db: &Database, key: &'static [u8]) -> Option<Vec<u8>> {
	db.txn("test_failover", move |tx| async move {
		let val = tx.get(key, Serializable).await?;
		Ok(val)
	})
	.await
	.unwrap()
	.map(|slice| slice.to_vec())
}

/// Exercises leader failover: two nodes share one Postgres, the elected leader is killed, the
/// survivor must take over the lease (new epoch), continue the crash-safe version sequence without
/// regression, preserve the dead leader's committed data, and resume accepting commits.
#[tokio::test]
async fn test_postgres_leader_failover() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("info")
		.with_test_writer()
		.try_init();

	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();

	tokio::time::sleep(Duration::from_secs(4)).await;

	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};
	let connection_string = postgres_config.url.read().clone();

	let (nats_config, _nats_docker) = setup_nats().await;

	let raw = connect_raw(&connection_string).await;

	// Node 1 comes up first and deterministically wins the first election (epoch 1).
	let db1 = make_db(&connection_string, &nats_config).await;
	let lease1 = wait_for_lease(&raw, Duration::from_secs(15), |l| l.epoch == 1).await;
	let leader1_addr = lease1.leader_addr.clone();

	// Node 2 joins while node 1 holds a valid lease, so it loses the election and runs as a
	// follower.
	let db2 = make_db(&connection_string, &nats_config).await;

	// Leader (node 1) commits data. The version sequence and watermark advance.
	write_key(&db1, ALPHA_KEY, b"1").await;

	// The follower (node 2) reads through its own snapshot and sees the leader's committed write,
	// proving cross-node reads work before any failover.
	assert_eq!(
		read_key(&db2, ALPHA_KEY).await,
		Some(b"1".to_vec()),
		"follower must see the leader's committed write"
	);

	let lease_before = read_lease(&raw).await.unwrap();
	let seq_before = read_seq_high(&raw).await;
	assert!(
		lease_before.durable_version >= 1,
		"durable_version must have advanced after the first commit"
	);

	// Kill node 1. Dropping the driver aborts its resolver, so it stops renewing the lease.
	drop(db1);

	// Node 2 must take over once node 1's lease expires (TTL is 10s). The epoch is bumped and the
	// leader address changes to node 2.
	let lease_after = wait_for_lease(&raw, Duration::from_secs(40), |l| {
		l.epoch > lease_before.epoch
	})
	.await;
	assert!(
		lease_after.epoch > lease_before.epoch,
		"new leader must bump the epoch (was {}, now {})",
		lease_before.epoch,
		lease_after.epoch
	);
	assert_ne!(
		lease_after.leader_addr, leader1_addr,
		"the surviving node must become the new leader"
	);

	// The crash-safe LOGGED sequence continues from the prior high-water; it never regresses.
	let seq_after_takeover = read_seq_high(&raw).await;
	assert!(
		seq_after_takeover >= seq_before,
		"version sequence regressed across failover ({} -> {})",
		seq_before,
		seq_after_takeover
	);
	assert!(
		lease_after.durable_version >= lease_before.durable_version,
		"durable_version regressed across failover ({} -> {})",
		lease_before.durable_version,
		lease_after.durable_version
	);

	// The data the dead leader committed survives the failover.
	assert_eq!(
		read_key(&db2, ALPHA_KEY).await,
		Some(b"1".to_vec()),
		"committed data must survive leader failover"
	);

	// The new leader resumes accepting commits.
	write_key(&db2, BETA_KEY, b"2").await;
	assert_eq!(
		read_key(&db2, BETA_KEY).await,
		Some(b"2".to_vec()),
		"new leader must accept and durably apply commits"
	);

	// The new commit advanced the version sequence and watermark past the pre-failover floor,
	// confirming the new leader sequences from a strictly higher version.
	let lease_final = read_lease(&raw).await.unwrap();
	assert!(
		read_seq_high(&raw).await > seq_before,
		"a post-failover commit must advance the version sequence"
	);
	assert!(
		lease_final.durable_version > lease_before.durable_version,
		"a post-failover commit must advance the durable watermark"
	);

	drop(db2);
}

/// Exercises graceful leader handoff: a leader that is shut down cleanly (SIGTERM path) releases its
/// lease immediately instead of letting it expire, so a standby takes over well within the lease TTL
/// rather than after it. This is what turns a rolling deploy from a ~TTL commit stall into a
/// near-instant handoff.
#[tokio::test]
async fn test_postgres_graceful_handoff() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("info")
		.with_test_writer()
		.try_init();

	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();

	tokio::time::sleep(Duration::from_secs(4)).await;

	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};
	let connection_string = postgres_config.url.read().clone();

	let (nats_config, _nats_docker) = setup_nats().await;

	let raw = connect_raw(&connection_string).await;

	// Node 1 wins the first election; node 2 joins as a follower.
	let db1 = make_db(&connection_string, &nats_config).await;
	let lease1 = wait_for_lease(&raw, Duration::from_secs(15), |l| l.epoch == 1).await;
	let leader1_addr = lease1.leader_addr.clone();
	let db2 = make_db(&connection_string, &nats_config).await;

	write_key(&db1, ALPHA_KEY, b"1").await;
	let lease_before = read_lease(&raw).await.unwrap();

	// Gracefully shut down the leader. Unlike a hard drop, this releases the lease in place and
	// wakes the standby, so takeover must complete in well under the 10s TTL.
	let handoff_start = tokio::time::Instant::now();
	db1.shutdown().await;

	// The lease TTL is 10s; a graceful handoff must take over well under that. The 5s deadline here
	// is itself below the TTL, so reaching this line already proves the lease was not waited out.
	let lease_after = wait_for_lease(&raw, Duration::from_secs(5), |l| {
		l.epoch > lease_before.epoch
	})
	.await;
	let handoff_elapsed = handoff_start.elapsed();
	assert!(
		handoff_elapsed < Duration::from_secs(8),
		"graceful handoff must beat the lease TTL (took {handoff_elapsed:?})"
	);
	assert_ne!(
		lease_after.leader_addr, leader1_addr,
		"the standby must become the new leader after a graceful handoff"
	);

	// The new leader serves the old leader's data and accepts fresh commits.
	assert_eq!(
		read_key(&db2, ALPHA_KEY).await,
		Some(b"1".to_vec()),
		"committed data must survive graceful handoff"
	);
	write_key(&db2, BETA_KEY, b"2").await;
	assert_eq!(read_key(&db2, BETA_KEY).await, Some(b"2".to_vec()));

	drop(db1);
	drop(db2);
}
