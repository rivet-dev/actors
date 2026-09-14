//! Follower commits whose encoded request is larger than the NATS server's `max_payload`.
//!
//! A follower sends each commit to the leader as one NATS request, and NATS rejects any message over
//! the server's `max_payload`, which defaults to 1 MiB. A transaction that writes more than that must
//! still commit, and must not disturb the follower's other commits while it does.

use std::{
	process::Command,
	sync::Arc,
	time::{Duration, Instant},
};

use rivet_test_deps_docker::{TestDatabase, TestPubSub};
use tokio::sync::watch;
use tokio_postgres::NoTls;
use universaldb::{
	Database,
	driver::postgres::{NatsConfig, PostgresConfig},
	utils::IsolationLevel::*,
};
use uuid::Uuid;

/// Enough 64 KiB values to push the encoded commit request well past the 1 MiB NATS default.
const LARGE_VALUE_BYTES: usize = 64 * 1024;
const LARGE_VALUE_COUNT: usize = 24;

/// Chunked commit requests arrived in this commit protocol version.
const CHUNKED_COMMIT_PROTOCOL_VERSION: u16 = 2;

/// A config whose fleet has negotiated `commit_protocol_version`.
fn test_config(commit_protocol_version: u16) -> rivet_config::Config {
	rivet_config::Config::from_root_with_build_meta(
		rivet_config::config::Root::default(),
		rivet_config::BuildMeta::default(),
		rivet_config::RuntimeProtocols {
			universaldb_commit: rivet_config::RuntimeProtocol::new(
				rivet_config::RuntimeProtocolKind::UniversaldbCommit,
				commit_protocol_version,
			),
			..Default::default()
		},
	)
}

async fn setup_postgres() -> (String, rivet_test_deps_docker::DockerRunConfig) {
	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();
	TestDatabase::Postgres
		.wait_for_ready(&docker_config)
		.await
		.unwrap();
	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};
	(postgres_config.url.read().clone(), docker_config)
}

/// The test image runs NATS with its default configuration, so `max_payload` is 1 MiB.
async fn setup_nats() -> (NatsConfig, rivet_test_deps_docker::DockerRunConfig) {
	let (pubsub_config, docker_config) = TestPubSub::Nats.config(Uuid::new_v4(), 1).await.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();
	tokio::time::sleep(Duration::from_secs(1)).await;
	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!();
	};
	(
		NatsConfig {
			addresses: nats.addresses.clone(),
			username: nats.username.clone(),
			password: nats.password.as_ref().map(|p| p.read().clone()),
			client_capacity: nats.client_capacity,
			subscription_capacity: nats.subscription_capacity,
		},
		docker_config,
	)
}

async fn make_db(
	connection_string: &str,
	nats: &NatsConfig,
	commit_protocol_version: u16,
) -> Database {
	let mut config = PostgresConfig::new(connection_string.to_string());
	config.nats = Some(nats.clone());
	let driver = universaldb::driver::PostgresDatabaseDriver::new_with_config(
		test_config(commit_protocol_version),
		config,
	)
	.await
	.unwrap();
	Database::new(Arc::new(driver))
}

async fn write_large(db: &Database) -> anyhow::Result<()> {
	db.txn("test_large_commit", |tx| async move {
		for i in 0..LARGE_VALUE_COUNT {
			tx.set(&large_key(i), &vec![b'x'; LARGE_VALUE_BYTES]);
		}
		Ok(())
	})
	.await
}

/// Waits for the first node's election, so the node created next runs as a follower.
async fn wait_for_leader(connection_string: &str) {
	let (client, connection) = tokio_postgres::connect(connection_string, NoTls)
		.await
		.unwrap();
	tokio::spawn(async move {
		let _ = connection.await;
	});
	let deadline = Instant::now() + Duration::from_secs(15);
	loop {
		// The lease row is written by another process, so there is no event to await here.
		let elected = client
			.query_opt("SELECT epoch FROM udb_lease WHERE id = 1", &[])
			.await
			.unwrap()
			.is_some();
		if elected {
			return;
		}
		assert!(Instant::now() < deadline, "no leader elected");
		tokio::time::sleep(Duration::from_millis(100)).await;
	}
}

fn small_key(i: u32) -> Vec<u8> {
	format!("nats_large_commit/small/{i:08}").into_bytes()
}

fn large_key(i: usize) -> Vec<u8> {
	format!("nats_large_commit/large/{i:04}").into_bytes()
}

async fn write_small(db: &Database, i: u32) -> anyhow::Result<()> {
	db.txn("test_small_commit", move |tx| async move {
		tx.set(&small_key(i), b"v");
		Ok(())
	})
	.await
}

/// Lines of the NATS server log that report a message over `max_payload`.
fn nats_payload_violations(container_name: &str) -> Vec<String> {
	let output = Command::new("docker")
		.args(["logs", container_name])
		.output()
		.unwrap();
	let mut logs = String::from_utf8_lossy(&output.stdout).into_owned();
	logs.push_str(&String::from_utf8_lossy(&output.stderr));
	logs.lines()
		.filter(|line| line.to_lowercase().contains("payload"))
		.map(str::to_string)
		.collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follower_commit_larger_than_nats_max_payload() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("warn")
		.with_test_writer()
		.try_init();

	let (connection_string, _postgres_docker) = setup_postgres().await;
	let (nats, nats_docker) = setup_nats().await;

	let leader = make_db(
		&connection_string,
		&nats,
		rivet_universaldb_commit::PROTOCOL_VERSION,
	)
	.await;
	wait_for_leader(&connection_string).await;
	let follower = Arc::new(
		make_db(
			&connection_string,
			&nats,
			rivet_universaldb_commit::PROTOCOL_VERSION,
		)
		.await,
	);

	write_small(&follower, 0)
		.await
		.expect("follower commits normally before the large commit");

	// Small commits keep flowing from the same follower for as long as the large commit is in flight,
	// so any disruption the large request causes to the shared NATS connection shows up here.
	let (stop_tx, mut stop_rx) = watch::channel(false);
	let small_writer = tokio::spawn({
		let follower = follower.clone();
		async move {
			let mut slowest = Duration::ZERO;
			let mut failures = Vec::new();
			let mut completed = 0u32;
			loop {
				let start = Instant::now();
				tokio::select! {
					res = write_small(&follower, completed + 1) => {
						slowest = slowest.max(start.elapsed());
						if let Err(err) = res {
							failures.push(format!("{err:#}"));
						}
						completed += 1;
					}
					_ = stop_rx.changed() => break,
				}
			}
			(completed, slowest, failures)
		}
	});

	let large_start = Instant::now();
	let large = tokio::time::timeout(
		Duration::from_secs(120),
		follower.txn("test_large_commit", |tx| async move {
			// One attempt is enough to show whether the request can be delivered at all.
			tx.retry_limit(0)?;
			for i in 0..LARGE_VALUE_COUNT {
				tx.set(&large_key(i), &vec![b'x'; LARGE_VALUE_BYTES]);
			}
			Ok(())
		}),
	)
	.await;
	let large_elapsed = large_start.elapsed();

	stop_tx.send(true).unwrap();
	let (small_completed, small_slowest, small_failures) = small_writer.await.unwrap();
	let violations = nats_payload_violations(&nats_docker.container_name);

	let large_outcome = match &large {
		Ok(Ok(())) => "committed".to_string(),
		Ok(Err(err)) => format!("failed: {err:#}"),
		Err(_) => "timed out".to_string(),
	};
	println!(
		"large commit: {large_outcome} after {large_elapsed:?}\n\
		 small commits during it: completed={small_completed} slowest={small_slowest:?} failures={small_failures:#?}\n\
		 nats payload violations: {violations:#?}"
	);

	assert!(
		matches!(large, Ok(Ok(()))),
		"large follower commit did not commit: {large_outcome}"
	);
	assert!(
		violations.is_empty(),
		"a commit request exceeded the NATS max_payload: {violations:#?}"
	);
	assert!(
		small_failures.is_empty(),
		"small commits failed while the large commit was in flight: {small_failures:#?}"
	);

	let stored = leader
		.txn("test_read_large_commit", |tx| async move {
			let mut total = 0usize;
			for i in 0..LARGE_VALUE_COUNT {
				if let Some(value) = tx.get(&large_key(i), Serializable).await? {
					total += value.len();
				}
			}
			Ok(total)
		})
		.await
		.unwrap();
	assert_eq!(stored, LARGE_VALUE_BYTES * LARGE_VALUE_COUNT);
}

/// Until every node understands chunked commit requests, an oversized commit must fail right away
/// with an error naming the limit, rather than sending a message the NATS server answers by closing
/// the follower's connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oversized_commit_fails_fast_before_chunking_is_negotiated() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("warn")
		.with_test_writer()
		.try_init();

	let (connection_string, _postgres_docker) = setup_postgres().await;
	let (nats, nats_docker) = setup_nats().await;

	let _leader = make_db(
		&connection_string,
		&nats,
		CHUNKED_COMMIT_PROTOCOL_VERSION - 1,
	)
	.await;
	wait_for_leader(&connection_string).await;
	let follower = make_db(
		&connection_string,
		&nats,
		CHUNKED_COMMIT_PROTOCOL_VERSION - 1,
	)
	.await;

	// No retry limit is set, so a retryable failure would keep resending well past one request
	// timeout and fail the elapsed check below.
	let start = Instant::now();
	let err = write_large(&follower)
		.await
		.expect_err("an oversized commit cannot be delivered before chunking is negotiated");
	let elapsed = start.elapsed();

	let message = format!("{err:#}");
	assert!(
		message.contains("max_payload"),
		"error should name the nats limit: {message}"
	);
	assert!(
		elapsed < Duration::from_secs(5),
		"oversized commit took {elapsed:?} to fail"
	);
	let violations = nats_payload_violations(&nats_docker.container_name);
	assert!(
		violations.is_empty(),
		"the oversized request reached the nats server: {violations:#?}"
	);
}
