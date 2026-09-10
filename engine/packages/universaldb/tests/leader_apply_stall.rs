//! Investigation harness for leader drain-batch stalls.
//!
//! Production traces show the leader's `drain_batch` blocking for roughly seven seconds at a time on
//! batches of ten to twenty-five jobs, with no pool wait, no conflicts, and no leadership change. The
//! batch log reports only the total, so this harness drives sustained multi-node commit load in the
//! shape gasoline produces and reads the per-phase timings back out of the tracing output. It exists
//! to localize which statement in the apply is slow, not to assert a latency bound.
//!
//! Run with `--ignored --nocapture`; these boot containers and push real load, so they are not part
//! of the default suite.

use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex, OnceLock},
	time::Duration,
};

use futures_util::future::join_all;
use rivet_test_deps_docker::{TestDatabase, TestPubSub};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};
use universaldb::{
	Database,
	driver::postgres::{NatsConfig, PostgresConfig},
};
use uuid::Uuid;

/// Workflow-state values gasoline writes are chunked; this is a representative chunk size.
const CHUNK_BYTES: usize = 8 * 1024;

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

/// Collects the fields of every `udb leader processed commit batch` event so the test can report the
/// phase breakdown itself. The batch log is `debug`, and capturing it is the whole point of the
/// harness, so it is parsed rather than eyeballed.
#[derive(Default)]
struct BatchCollector {
	batches: Mutex<Vec<BTreeMap<String, String>>>,
}

impl BatchCollector {
	fn clear(&self) {
		self.batches.lock().unwrap().clear();
	}
}

/// The drain loop runs on its own spawned task, so a thread-local `set_default` subscriber never
/// sees its events. The collector is installed globally once and shared by every test here, and the
/// tests serialize on [`RUN_LOCK`] so one test's batches cannot land in another's report.
static COLLECTOR: OnceLock<Arc<BatchCollector>> = OnceLock::new();
static RUN_LOCK: Mutex<()> = Mutex::new(());

fn collector() -> Arc<BatchCollector> {
	COLLECTOR
		.get_or_init(|| {
			let collector = Arc::new(BatchCollector::default());
			tracing_subscriber::registry()
				.with(BatchLayer(collector.clone()))
				.init();
			collector
		})
		.clone()
}

struct BatchLayer(Arc<BatchCollector>);

impl<S: tracing::Subscriber> Layer<S> for BatchLayer {
	fn on_event(
		&self,
		event: &tracing::Event<'_>,
		_ctx: tracing_subscriber::layer::Context<'_, S>,
	) {
		let mut fields = BTreeMap::new();
		event.record(&mut FieldVisitor(&mut fields));
		if fields.get("message").map(String::as_str) == Some("udb leader processed commit batch") {
			self.0.batches.lock().unwrap().push(fields);
		}
	}
}

struct FieldVisitor<'a>(&'a mut BTreeMap<String, String>);

impl tracing::field::Visit for FieldVisitor<'_> {
	fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
		self.0
			.insert(field.name().to_string(), format!("{value:?}"));
	}
	fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
		self.0.insert(field.name().to_string(), value.to_string());
	}
	fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
		self.0.insert(field.name().to_string(), value.to_string());
	}
	fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
		self.0.insert(field.name().to_string(), value.to_string());
	}
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

async fn make_db(connection_string: &str, nats: Option<&NatsConfig>) -> Database {
	let mut config = PostgresConfig::new(connection_string.to_string());
	config.nats = nats.cloned();
	let driver =
		universaldb::driver::PostgresDatabaseDriver::new_with_config(test_config(), config)
			.await
			.unwrap();
	Database::new(Arc::new(driver))
}

fn state_prefix(workflow: usize) -> Vec<u8> {
	format!("wf/{workflow:08}/state/").into_bytes()
}

fn state_chunk_key(workflow: usize, chunk: usize) -> Vec<u8> {
	format!("wf/{workflow:08}/state/{chunk:04}").into_bytes()
}

fn range_end(prefix: &[u8]) -> Vec<u8> {
	let mut end = prefix.to_vec();
	end.push(0xff);
	end
}

/// One `update_workflow_state`-shaped transaction: clear the whole state subspace, then write the
/// state back as chunks. This is the write pattern that dominates gasoline's commit volume.
async fn write_state(db: &Database, workflow: usize, chunks: usize) {
	db.txn("test_update_workflow_state", move |tx| async move {
		let prefix = state_prefix(workflow);
		tx.clear_range(&prefix, &range_end(&prefix));
		for chunk in 0..chunks {
			tx.set(&state_chunk_key(workflow, chunk), &vec![b'x'; CHUNK_BYTES]);
		}
		Ok(())
	})
	.await
	.unwrap();
}

/// Report the phase breakdown of collected batches, sorted by total time.
fn report(collector: &BatchCollector, label: &str) {
	let batches = collector.batches.lock().unwrap();
	let num = |b: &BTreeMap<String, String>, k: &str| -> u64 {
		b.get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
	};
	let mut sorted: Vec<_> = batches.iter().collect();
	sorted.sort_by_key(|b| std::cmp::Reverse(num(b, "batch_ms")));

	let total = batches.len();
	let slow = batches
		.iter()
		.filter(|b| num(b, "batch_ms") >= 1000)
		.count();
	println!("\n===== {label} =====");
	println!("batches={total} slow(>=1s)={slow}");
	println!(
		"{:>8} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7} {:>6} {:>6} {:>8} {:>7}",
		"batch_ms",
		"pool",
		"begin",
		"prep",
		"resol",
		"atomi",
		"fold",
		"rdel",
		"apply",
		"commit",
		"len",
		"upserts",
		"bytes"
	);
	for b in sorted.iter().take(10) {
		println!(
			"{:>8} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7} {:>6} {:>6} {:>8} {:>7}",
			num(b, "batch_ms"),
			num(b, "pool_wait_ms"),
			num(b, "begin_ms"),
			num(b, "prepare_ms"),
			num(b, "resolve_ms"),
			num(b, "atomic_ms"),
			num(b, "fold_ms"),
			num(b, "range_delete_ms"),
			num(b, "apply_ms"),
			num(b, "commit_ms"),
			num(b, "batch_len"),
			num(b, "upserts"),
			num(b, "upsert_bytes"),
		);
	}
	let sum = |k: &str| -> u64 { batches.iter().map(|b| num(b, k)).sum() };
	println!(
		"totals: batch={} pool={} prepare={} resolve={} atomic={} fold={} range_delete={} apply={} commit={}",
		sum("batch_ms"),
		sum("pool_wait_ms"),
		sum("prepare_ms"),
		sum("resolve_ms"),
		sum("atomic_ms"),
		sum("fold_ms"),
		sum("range_delete_ms"),
		sum("apply_ms"),
		sum("commit_ms"),
	);
}

/// Drive sustained multi-node write load and report where the leader's apply time goes.
///
/// `workflows` sets how many distinct state subspaces churn, `chunks` how many chunks each state
/// carries, and `rounds` how many times every workflow rewrites its state.
async fn run_load(workflows: usize, chunks: usize, rounds: usize, concurrency: usize, label: &str) {
	let _serialized = RUN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
	let collector = collector();
	collector.clear();

	let (connection_string, _pg) = setup_postgres().await;
	let (nats, _nats_docker) = setup_nats().await;

	let leader = Arc::new(make_db(&connection_string, Some(&nats)).await);
	let follower = Arc::new(make_db(&connection_string, Some(&nats)).await);
	// Let one node win the lease before load starts, so the run measures steady-state apply cost
	// rather than election.
	tokio::time::sleep(Duration::from_secs(2)).await;

	for round in 0..rounds {
		let mut handles = Vec::new();
		for batch in 0..concurrency {
			let db = if batch % 2 == 0 {
				leader.clone()
			} else {
				follower.clone()
			};
			let start = (round * concurrency + batch) % workflows;
			handles.push(tokio::spawn(async move {
				write_state(&db, start, chunks).await;
			}));
		}
		join_all(handles).await;
	}

	report(&collector, label);
}

/// Baseline: small states, high transaction rate. Establishes what a healthy apply costs.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn leader_apply_small_states() {
	run_load(512, 1, 40, 64, "small states (1 chunk)").await;
}

/// Large states: the same transaction shape, but each one clears and rewrites a much bigger
/// subspace. Tests whether apply cost tracks byte volume rather than job count.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn leader_apply_large_states() {
	run_load(128, 64, 20, 64, "large states (64 chunks)").await;
}

/// Accumulated table: many distinct workflows churn so `kv` grows and range deletes scan more, which
/// is closer to a long-lived production table than a freshly created one.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn leader_apply_wide_table() {
	run_load(8192, 8, 12, 96, "wide table (8192 workflows)").await;
}
