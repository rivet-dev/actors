//! Investigation harness for workflow pulls against a large wake backlog.
//!
//! A worker gives up when `pull_workflows` takes longer than its fixed pull timeout, and giving up
//! stops every workflow it is running. This harness builds a backlog of awake workflows, adds network
//! round-trip latency to the database the way a managed Postgres has it, and times one pull phase by
//! phase.
//!
//! Run with:
//!
//! ```text
//! RIVET_TEST_DATABASE=postgres cargo test -p gasoline --test pull_backlog -- --ignored --nocapture
//! ```
//!
//! `PULL_BACKLOG_WORKFLOWS` sets the backlog size and `PULL_BACKLOG_RTT_MS` the added latency.

use std::{
	collections::HashMap,
	process::Command,
	time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use futures_util::{StreamExt, TryStreamExt};
use gas::prelude::Id;
use gasoline as gas;
use gasoline::db::{Database, DatabaseKv};
use serde_json::json;
use tracing::{Subscriber, span};
use tracing_subscriber::{
	Layer, filter::filter_fn, layer::SubscriberExt, registry::LookupSpan, util::SubscriberInitExt,
};
use uuid::Uuid;

const WORKFLOW_NAME: &str = "pull_backlog_test";
/// The worker's pull timeout in `gasoline::worker`.
const PULL_WORKFLOWS_TIMEOUT: Duration = Duration::from_secs(10);
/// Spans that bound the phases of `pull_workflows`.
const PHASE_SPANS: &[&str] = &[
	"pull_workflows",
	"read_wake_conditions",
	"map_to_leased_workflows",
	"pull_workflows_tx",
	"clear_workflow_secondary_idx_tx",
	"pull_workflow_history_tx",
];

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
	std::env::var(name)
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(default)
}

/// Prints how long each pull phase span was open.
struct PhaseTimer;

impl<S> Layer<S> for PhaseTimer
where
	S: Subscriber + for<'a> LookupSpan<'a>,
{
	fn on_new_span(
		&self,
		_attrs: &span::Attributes<'_>,
		id: &span::Id,
		ctx: tracing_subscriber::layer::Context<'_, S>,
	) {
		if let Some(span) = ctx.span(id) {
			span.extensions_mut().insert(Instant::now());
		}
	}

	fn on_close(&self, id: span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
		if let Some(span) = ctx.span(&id) {
			if let Some(opened_at) = span.extensions().get::<Instant>() {
				println!("phase {:<32} {:?}", span.name(), opened_at.elapsed());
			}
		}
	}
}

/// Adds `rtt_ms` of egress delay to a container's network, which every response to the engine then
/// pays. The sidecar shares the container's network namespace, so the database image needs no tools.
fn add_network_latency(container_name: &str, rtt_ms: u64) -> Result<()> {
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
				"apk add --no-cache iproute2-tc >/dev/null && tc qdisc add dev eth0 root netem delay {rtt_ms}ms"
			),
		])
		.output()
		.context("failed to run the netem sidecar")?;
	ensure!(
		output.status.success(),
		"netem sidecar failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "investigation harness; run explicitly with --ignored --nocapture"]
async fn pull_workflows_against_wake_backlog() -> Result<()> {
	tracing_subscriber::registry()
		.with(PhaseTimer.with_filter(filter_fn(|metadata| {
			metadata.is_span() && PHASE_SPANS.contains(&metadata.name())
		})))
		.init();

	let backlog: usize = env_or("PULL_BACKLOG_WORKFLOWS", 5_000);
	let rtt_ms: u64 = env_or("PULL_BACKLOG_RTT_MS", 1);
	let on_postgres = std::env::var("RIVET_TEST_DATABASE").as_deref() == Ok("postgres");

	let test_id = Uuid::new_v4();
	let test_deps = rivet_test_deps::TestDeps::new_with_test_id(test_id).await?;
	let config = test_deps.config().clone();
	let db = <DatabaseKv as Database>::new(config.clone(), test_deps.pools().clone()).await?;

	let dispatch_start = Instant::now();
	let input = serde_json::value::to_raw_value(&json!({}))?;
	futures_util::stream::iter(0..backlog)
		.map(|_| {
			db.dispatch_workflow(
				Id::new_v1(config.dc_label()),
				Id::new_v1(config.dc_label()),
				WORKFLOW_NAME,
				None,
				input.as_ref(),
				false,
			)
		})
		.buffer_unordered(128)
		.try_collect::<Vec<_>>()
		.await?;
	println!(
		"dispatched {backlog} workflows in {:?}",
		dispatch_start.elapsed()
	);

	// Latency is added after the backlog exists so building it stays fast.
	if on_postgres {
		add_network_latency(&format!("test-postgres-{test_id}-1"), rtt_ms)?;
	} else {
		println!("not on postgres, so no network latency was added");
	}

	let worker_id = Id::new_v1(config.dc_label());
	db.update_worker_ping(worker_id, 1, true).await?;

	let pull_start = Instant::now();
	let pulled = db
		.pull_workflows(worker_id, 1, &[WORKFLOW_NAME], &HashMap::new())
		.await?;
	let pull_elapsed = pull_start.elapsed();

	println!(
		"pulled {} of {backlog} workflows in {pull_elapsed:?} (rtt {}ms)",
		pulled.len(),
		if on_postgres { rtt_ms } else { 0 }
	);
	ensure!(
		pull_elapsed < PULL_WORKFLOWS_TIMEOUT,
		"pull took {pull_elapsed:?}, past the worker's {PULL_WORKFLOWS_TIMEOUT:?} pull timeout"
	);

	Ok(())
}
