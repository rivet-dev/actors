//! Concurrent reads inside one Postgres transaction.
//!
//! Every read in a transaction runs against the same pinned snapshot connection. Callers issue reads
//! concurrently, as gasoline does when it loads the history of every workflow a pull leased, so the
//! driver must send them together instead of waiting out a network round trip per read.

use std::{
	process::Command,
	sync::Arc,
	time::{Duration, Instant},
};

use futures_util::future::try_join_all;
use rivet_test_deps_docker::TestDatabase;
use universaldb::{
	Database, RangeOption, driver::postgres::PostgresConfig, options::StreamingMode,
	utils::IsolationLevel::*,
};
use uuid::Uuid;

const KEY_COUNT: usize = 400;
const RANGE_READS: usize = 8;
const RTT: Duration = Duration::from_millis(5);

fn test_config() -> rivet_config::Config {
	rivet_config::Config::from_root_with_build_meta(
		rivet_config::config::Root::default(),
		rivet_config::BuildMeta::default(),
		rivet_config::RuntimeProtocols {
			universaldb_commit: rivet_config::RuntimeProtocol::new(
				rivet_config::RuntimeProtocolKind::UniversaldbCommit,
				rivet_universaldb_commit::PROTOCOL_VERSION,
			),
			..Default::default()
		},
	)
}

fn key(i: usize) -> Vec<u8> {
	format!("concurrent_reads/{i:06}").into_bytes()
}

/// Adds `rtt` of egress delay to a container's network, which every response then pays. The sidecar
/// shares the container's network namespace, so the database image needs no tools.
fn add_network_latency(container_name: &str, rtt: Duration) {
	let output = Command::new("docker")
		.args([
			"run",
			"--rm",
			"--net",
			&format!("container:{container_name}"),
			"--cap-add",
			"NET_ADMIN",
			"alpine:3",
			"sh",
			"-c",
			&format!(
				"apk add --no-cache iproute2-tc >/dev/null && tc qdisc add dev eth0 root netem delay {}ms",
				rtt.as_millis()
			),
		])
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"netem sidecar failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reads_share_round_trips() {
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
	let driver = universaldb::driver::PostgresDatabaseDriver::new_with_config(
		test_config(),
		PostgresConfig::new(postgres_config.url.read().clone()),
	)
	.await
	.unwrap();
	let db = Database::new(Arc::new(driver));

	db.txn("test_write_keys", |tx| async move {
		for i in 0..KEY_COUNT {
			tx.set(&key(i), &i.to_le_bytes());
		}
		Ok(())
	})
	.await
	.unwrap();

	add_network_latency(&docker_config.container_name, RTT);

	let elapsed = db
		.txn("test_concurrent_reads", |tx| async move {
			let keys: Vec<Vec<u8>> = (0..KEY_COUNT).map(key).collect();
			let range_width = KEY_COUNT / RANGE_READS;
			let start = Instant::now();

			let (values, ranges) = tokio::try_join!(
				try_join_all(keys.iter().map(|k| tx.get(k, Serializable))),
				try_join_all((0..RANGE_READS).map(|r| {
					let begin = key(r * range_width);
					let end = key((r + 1) * range_width);
					let tx = &tx;
					async move {
						tx.get_range(
							&RangeOption {
								mode: StreamingMode::WantAll,
								..(begin.as_slice(), end.as_slice()).into()
							},
							1,
							Serializable,
						)
						.await
					}
				})),
			)?;
			let elapsed = start.elapsed();

			for (i, value) in values.iter().enumerate() {
				assert_eq!(
					value.as_deref().map(Vec::as_slice),
					Some(&i.to_le_bytes()[..])
				);
			}
			for range in &ranges {
				assert_eq!(range.len(), range_width);
			}

			Ok(elapsed)
		})
		.await
		.unwrap();

	let serial = RTT * (KEY_COUNT + RANGE_READS) as u32;
	println!(
		"{} concurrent reads with {RTT:?} rtt took {elapsed:?}; one round trip per read would be at least {serial:?}",
		KEY_COUNT + RANGE_READS
	);
	assert!(
		elapsed < Duration::from_secs(1),
		"concurrent reads took {elapsed:?}, so the driver is waiting a round trip per read"
	);
}
