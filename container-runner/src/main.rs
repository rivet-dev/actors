//! `rivet-container-runner`: a RivetKit serverless app that hosts actors by
//! spawning one child game-server process per actor and proxying Rivet's tunneled
//! traffic to it.
//!
//! The engine's `POST /api/rivet/start` boots this container. On actor start the
//! `GameServer` actor spawns the child (`-- <command...>`), pipes its logs to
//! stdout prefixed with the actor id + key, and proxies inbound HTTP/WebSocket to
//! the child's local port. On actor stop it SIGTERMs the child; once the last
//! child stops the process exits so the platform reaps the instance. The engine
//! decides how many actors land here, each on its own port (1 in the recommended
//! game-server setup).

mod actor;
mod child;
mod input;
mod monitor;
mod proxy;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use futures_util::future::join_all;
use rivetkit::serverless_http::{self, ListenerConfig};
use rivetkit::{ActorConfig, EngineSpawnMode, Registry, ServeConfig};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::actor::GameServer;
use crate::child::ChildProcess;

/// Crate version from Cargo.toml (the workspace version).
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git commit SHA baked in at build time (see `build.rs`). `"unknown"` when the
/// build had neither `OVERRIDE_GIT_SHA` nor a reachable git repo (the release
/// image build context excludes `.git`).
pub(crate) const GIT_SHA: &str = env!("CONTAINER_RUNNER_GIT_SHA");

/// The effective git SHA, or `None` when unknown. Prefers the build-time SHA,
/// falling back to the `OVERRIDE_GIT_SHA` env var for deploys that cannot inject a
/// build arg.
pub(crate) fn git_sha() -> Option<String> {
	if GIT_SHA != "unknown" {
		return Some(GIT_SHA.to_string());
	}
	std::env::var("OVERRIDE_GIT_SHA")
		.ok()
		.map(|sha| sha.trim().to_string())
		.filter(|sha| !sha.is_empty())
}

/// Static runner configuration derived from the CLI/env.
pub struct RunnerConfig {
	/// Child command template (program + fixed args) from `-- <command...>`.
	pub command_template: Vec<String>,
	/// Default local port for the child when `input.port` is absent.
	pub default_child_port: u16,
	/// How long to wait for the child's port to open before failing the start.
	pub readiness_timeout: Duration,
}

// The `Actor` trait constructs actors without user parameters, so the runner
// configuration is ambient process state set once in `main`.
static RUNNER_CONFIG: OnceLock<Arc<RunnerConfig>> = OnceLock::new();

/// Running children keyed by actor id. Global (not per-actor) so the shutdown path
/// can stop children even when hooks never run, and the watchdog can arbitrate exits.
static CHILDREN: LazyLock<scc::HashMap<String, Arc<ChildProcess>>> =
	LazyLock::new(scc::HashMap::new);

/// Ports reserved by spawning or running children. Multiple actors can run here at
/// once, so each needs its own port and concurrent starts must not race for one.
static RESERVED_PORTS: LazyLock<scc::HashSet<u16>> = LazyLock::new(scc::HashSet::new);

/// Cancelled to bring the whole process down (last child stopped or a signal).
/// `main` owns the exit sequencing.
static EXIT: LazyLock<CancellationToken> = LazyLock::new(CancellationToken::new);

/// Set when a platform signal is driving shutdown. The platform gives only a
/// bounded window (~10s) between SIGTERM and SIGKILL, so grace periods on this
/// path must fit that budget.
static SIGNAL_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Set when shutdown was a platform SIGTERM (instance reclaim) rather than a local
/// SIGINT (Ctrl-C), so the reclaim can be reported as an actor crash.
static PLATFORM_RECLAIM: AtomicBool = AtomicBool::new(false);

/// SIGTERM→SIGKILL window the platform gives this container. Defaults to 9s, just
/// under the common ~10s budget; override with RIVET_SIGTERM_BUDGET_SECS. On the
/// signal path the engine drain and the child kills share this budget concurrently.
static SIGTERM_BUDGET: LazyLock<Duration> = LazyLock::new(|| {
	let secs = std::env::var("RIVET_SIGTERM_BUDGET_SECS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.unwrap_or(9)
		.max(3);
	Duration::from_secs(secs)
});

/// On an actor-driven exit, how long to wait for in-flight actor stops to flush their
/// `Stopped` to the engine before the envoy announces it is going away. Bounded so a
/// stuck actor cannot hang the exit; the actor normally stops in well under a second.
const ACTOR_STOP_FLUSH_BUDGET: Duration = Duration::from_secs(10);

/// Cancelled when a platform signal (SIGTERM/SIGINT) starts shutdown. Cuts the
/// actor-driven drain wait short so a reclaim arriving mid-drain is not delayed by
/// [`ACTOR_STOP_FLUSH_BUDGET`] and cannot overrun the SIGTERM→SIGKILL budget.
static SIGNAL_TOKEN: LazyLock<CancellationToken> = LazyLock::new(CancellationToken::new);

/// How long an engine pause (sleep, lost, going-away) lets the child keep serving
/// and exit on its own before we SIGTERM it. Defaults to 15 min (RIVET_DRAIN_GRACE_SECS);
/// must fit inside the engine's per-runner `drain_grace_period` or a reclaim cuts it short.
static DRAIN_GRACE: LazyLock<Duration> = LazyLock::new(|| {
	let secs = std::env::var("RIVET_DRAIN_GRACE_SECS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.unwrap_or(900);
	Duration::from_secs(secs)
});

/// The drain window for an engine pause. See [`DRAIN_GRACE`].
pub fn drain_grace() -> Duration {
	*DRAIN_GRACE
}

/// One-shot startup idle timeout. If the actor receives no request within this
/// window of starting, it sleeps (and the container exits). `None` when
/// RIVET_IDLE_TIMEOUT_SECS is unset or 0 (disabled); the first request disarms it.
static IDLE_TIMEOUT: LazyLock<Option<Duration>> = LazyLock::new(|| {
	let secs = std::env::var("RIVET_IDLE_TIMEOUT_SECS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.unwrap_or(0);
	(secs > 0).then(|| Duration::from_secs(secs))
});

/// The one-shot startup idle-sleep window, or `None` when disabled. See [`IDLE_TIMEOUT`].
pub fn idle_timeout() -> Option<Duration> {
	*IDLE_TIMEOUT
}

/// The idle window plus up to 20% jitter (capped at 60s), so instances armed at the
/// same time do not all sleep in the same instant and tear down in a wave. Jitter is
/// only ever added, never subtracted, so an actor never sleeps before its window.
pub fn idle_timeout_with_jitter(base: Duration) -> Duration {
	let max_jitter = base.mul_f64(0.2).min(Duration::from_secs(60));
	base + random_duration_up_to(max_jitter)
}

/// A `Duration` uniformly in `[0, max]`, drawn from the OS CSPRNG. Falls back to no
/// jitter when the CSPRNG is unavailable.
fn random_duration_up_to(max: Duration) -> Duration {
	let max_ms = max.as_millis() as u64;
	if max_ms == 0 {
		return Duration::ZERO;
	}
	let mut buf = [0u8; 8];
	match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)) {
		Ok(()) => Duration::from_millis(u64::from_le_bytes(buf) % (max_ms + 1)),
		Err(_) => Duration::ZERO,
	}
}

/// When set, an actor that starts a second time self-sleeps instead of running
/// again; its persisted `started_once` records the first real start. Configured via
/// RIVET_REJECT_SECOND_START (truthy `1`/`true`/`yes`/`on`). Off by default.
static REJECT_SECOND_START: LazyLock<bool> = LazyLock::new(|| {
	std::env::var("RIVET_REJECT_SECOND_START")
		.map(|value| {
			matches!(
				value.trim().to_ascii_lowercase().as_str(),
				"1" | "true" | "yes" | "on"
			)
		})
		.unwrap_or(false)
});

/// Whether the reject-second-start guard is enabled. See [`REJECT_SECOND_START`].
pub fn reject_second_start() -> bool {
	*REJECT_SECOND_START
}

/// Non-idle mode commits `started_once` only after the actor survives this window,
/// so a child that crashes within it is not treated as a real start and the retry
/// may run. Configured via RIVET_SECOND_START_GRACE_SECS, default 10s.
static SECOND_START_GRACE: LazyLock<Duration> = LazyLock::new(|| {
	let secs = std::env::var("RIVET_SECOND_START_GRACE_SECS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.unwrap_or(10);
	Duration::from_secs(secs)
});

/// Grace an actor must survive before its start is committed. See [`SECOND_START_GRACE`].
pub fn second_start_grace() -> Duration {
	*SECOND_START_GRACE
}

/// Token that fires on a platform shutdown signal. Cuts a drain wait short so the
/// platform's SIGTERM→SIGKILL budget is honored.
pub fn exit_token() -> &'static CancellationToken {
	&EXIT
}

pub fn runner_config() -> Arc<RunnerConfig> {
	RUNNER_CONFIG
		.get()
		.expect("runner config is set in main before the runtime serves")
		.clone()
}

pub fn children() -> &'static scc::HashMap<String, Arc<ChildProcess>> {
	&CHILDREN
}

/// Actor ids currently hosting a running child on this instance. Used to
/// attribute the instance-wide resource samples to the running actor(s).
pub async fn active_actor_ids() -> Vec<String> {
	let mut ids = Vec::new();
	// `retain_async` returning `true` keeps every entry: a read-only scan.
	CHILDREN
		.retain_async(|actor_id, _| {
			ids.push(actor_id.clone());
			true
		})
		.await;
	ids
}

/// Reserve a local port for a new child. An explicit `input.port` is honored (or
/// refused if held); otherwise the first free port at or above the default is used.
/// Guards selection-to-bind; release via [`release_child_port`] once the child is gone.
pub async fn reserve_child_port(preferred: Option<u16>, default: u16) -> Result<u16> {
	if let Some(port) = preferred {
		if RESERVED_PORTS.insert_async(port).await.is_err() {
			anyhow::bail!(
				"input.port {port} is already reserved by another actor's child on this instance"
			);
		}
		return Ok(port);
	}

	// Probe upward from the default. The registry reservation is the atomic
	// arbiter between concurrent starts; the TCP check below it catches ports
	// held by foreign processes.
	for offset in 0..=256u16 {
		let Some(port) = default.checked_add(offset) else {
			break;
		};
		if RESERVED_PORTS.insert_async(port).await.is_err() {
			continue;
		}
		if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
			.await
			.is_ok()
		{
			// Something outside our registry is listening on it; skip.
			RESERVED_PORTS.remove_async(&port).await;
			continue;
		}
		return Ok(port);
	}
	anyhow::bail!(
		"no free child port found in {default}..={}",
		default.saturating_add(256)
	)
}

pub async fn release_child_port(port: u16) {
	RESERVED_PORTS.remove_async(&port).await;
}

/// The SIGTERM→SIGKILL window for the child: always `SIGTERM_BUDGET`, whatever
/// triggered the stop. The wait before SIGTERM (an engine pause) is separate; see
/// [`drain_grace`].
pub fn effective_stop_grace() -> Duration {
	*SIGTERM_BUDGET
}

/// End the process. Driven by a platform signal or by the last child stopping (see
/// `stop_child`). The runner is PID 1, so cancelling `EXIT` wakes `main` to run the
/// graceful envoy close and return, which stops the container.
pub fn request_exit(actor_id: &str, reason: &str) {
	tracing::info!(actor_id = %actor_id, reason, "shutting down container");
	EXIT.cancel();
}

/// Stable per-process id, generated once. The runner is PID 1, so one boot id ==
/// one container instance. Logged per actor start so an actor can be attributed to
/// an instance (the log stream carries no instance id).
pub fn boot_id() -> &'static str {
	static BOOT_ID: OnceLock<String> = OnceLock::new();
	BOOT_ID.get_or_init(|| {
		// 9 random bytes -> 12 chars. Falls back to a marker if the CSPRNG is
		// unavailable, which is itself worth seeing in logs.
		let mut buf = [0u8; 9];
		match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)) {
			Ok(()) => base64url_nopad(&buf),
			Err(_) => "no-urandom".to_string(),
		}
	})
}

/// Minimal URL-safe base64 encoder (RFC 4648 §5) without padding. Kept local
/// to avoid adding a dependency just for the boot id.
fn base64url_nopad(input: &[u8]) -> String {
	const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
	let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
	for chunk in input.chunks(3) {
		let b0 = chunk[0] as u32;
		let b1 = *chunk.get(1).unwrap_or(&0) as u32;
		let b2 = *chunk.get(2).unwrap_or(&0) as u32;
		let n = (b0 << 16) | (b1 << 8) | b2;
		out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
		out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
		if chunk.len() > 1 {
			out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
		}
		if chunk.len() > 2 {
			out.push(ALPHABET[(n & 0x3f) as usize] as char);
		}
	}
	out
}

#[derive(Parser, Debug)]
#[command(
    name = "rivet-container-runner",
    about = "Rivet serverless runner: spawns a child game server and proxies tunneled traffic to it.",
    long_about = None,
)]
struct Args {
	/// Serverless HTTP front-door port. Resolved in `main` as
	/// --port > RIVET_PORT > PORT > 8080.
	#[arg(long)]
	port: Option<u16>,

	/// Local port the child game server listens on (proxy target + child's $PORT).
	#[arg(long, env = "CHILD_PORT", default_value_t = 7770)]
	child_port: u16,

	/// Runner version reported to the engine (used for draining on deploy).
	#[arg(long, env = "RIVET_RUNNER_VERSION", default_value_t = 1)]
	runner_version: u32,

	/// Actor name this runner advertises/serves (repeatable).
	#[arg(long = "actor-name", env = "RIVET_ACTOR_NAME", default_value = "game")]
	actor_name: String,

	/// Base path the engine calls for serverless start.
	#[arg(long, env = "RIVET_SERVERLESS_BASE_PATH", default_value = "/api/rivet")]
	base_path: String,

	/// How long (seconds) to wait for the child's port to open before failing start.
	#[arg(long, env = "RIVET_READINESS_TIMEOUT_SECS", default_value_t = 30)]
	readiness_timeout_secs: u64,

	/// The child command to run, after `--`. e.g. `-- node /app/server.mjs`.
	#[arg(last = true, required = true)]
	command: Vec<String>,
}

fn main() -> Result<()> {
	// Rivet Compute documents RIVET_PORT for RivetKit apps; accept it for the
	// front-door port resolution below.
	if std::env::var_os("PORT").is_none() {
		if let Some(port) = std::env::var_os("RIVET_PORT") {
			// SAFETY: no other threads exist yet; the tokio runtime starts below.
			unsafe { std::env::set_var("PORT", port) };
		}
	}

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()?
		.block_on(async_main())
}

async fn async_main() -> Result<()> {
	// Runner's own logs go to stderr; child logs go to stdout with the actor prefix.
	tracing_subscriber::fmt()
		.with_writer(std::io::stderr)
		.with_env_filter(
			EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
		)
		.init();

	let args = Args::parse();
	let boot_id = boot_id();
	tracing::info!(?args, %boot_id, "starting container-runner");

	// Front-door port: Rivet Compute injects RIVET_PORT; other platforms use PORT.
	let port = args
		.port
		.or_else(|| env_u16("RIVET_PORT"))
		.or_else(|| env_u16("PORT"))
		.unwrap_or(8080);

	RUNNER_CONFIG
		.set(Arc::new(RunnerConfig {
			command_template: args.command.clone(),
			default_child_port: args.child_port,
			readiness_timeout: Duration::from_secs(args.readiness_timeout_secs),
		}))
		.map_err(|_| anyhow::anyhow!("runner config already set"))?;

	let mut registry = Registry::new();
	registry.register_actor_with::<GameServer>(
		&args.actor_name,
		ActorConfig {
			// Game servers hold live in-memory state; never idle-sleep the actor.
			no_sleep: true,
			// Core force-aborts the stop hook at this deadline, so it must outlast the
			// full drain window plus the SIGTERM budget (with a small margin) or the
			// drain is cut short and the child leaks.
			sleep_grace_period: *DRAIN_GRACE + *SIGTERM_BUDGET + Duration::from_secs(5),
			sleep_grace_period_overridden: true,
			..Default::default()
		},
	);

	let mut config = ServeConfig::from_env();
	config.version = args.runner_version;
	config.serverless_base_path = Some(args.base_path.clone());
	// The engine passes its endpoint per /start request in headers; never
	// spawn a local engine, and don't reject starts whose header endpoint
	// differs from the env-default one.
	config.engine_spawn = EngineSpawnMode::Never;
	config.serverless_validate_endpoint = false;

	let runtime = registry.into_serverless_runtime(config).await?;

	let serve_shutdown = CancellationToken::new();
	spawn_signal_handler();
	monitor::spawn_resource_monitor();

	let serve = tokio::spawn(serverless_http::serve(
		runtime.clone(),
		ListenerConfig {
			// Bind dual-stack ([::]) so the front door accepts both IPv4 (as
			// IPv4-mapped) and IPv6 loopback: the engine's metadata client may
			// connect via `localhost`/::1.
			host: Some("::".to_string()),
			port,
			public_dir: None,
			application: None,
		},
		serve_shutdown.clone(),
	));
	tracing::info!(port, "container-runner serverless front door listening");

	// Wait for an exit request, then tear down. A platform reclaim (signal) kills children
	// and notifies the engine at once, each bounded by the SIGTERM budget. An actor-driven
	// exit (last child stopped) has no deadline: the child is already reaped, drain unbounded.
	EXIT.cancelled().await;
	if SIGNAL_SHUTDOWN.load(Ordering::Acquire) {
		// A platform SIGTERM reclaims this instance. Report actors as crashed while
		// the envoy is still connected so the reclaim (OOM or the ~60 min request
		// cap) surfaces as a crash, not a silent reallocation. SIGINT drains cleanly.
		if PLATFORM_RECLAIM.load(Ordering::Acquire) {
			crate::actor::crash_all_actors(
				"runner received unexpected platform SIGTERM, likely OOM or running longer than 60 minutes",
			)
			.await;
		}
		let drain = async {
			if tokio::time::timeout(*SIGTERM_BUDGET, runtime.shutdown())
				.await
				.is_err()
			{
				tracing::warn!("engine drain exceeded the signal budget");
			}
		};
		tokio::join!(drain, stop_all_children(*SIGTERM_BUDGET));
	} else {
		stop_all_children(*SIGTERM_BUDGET).await;
		// Flush an in-flight actor's `Stopped` to the engine before the envoy announces
		// it is going away, or that announcement becomes a per-actor `GoingAway` that
		// overrides the destroy and reallocates it. A signal cuts the wait short.
		let signaled = tokio::select! {
			_ = runtime.wait_actors_drained(ACTOR_STOP_FLUSH_BUDGET) => false,
			_ = SIGNAL_TOKEN.cancelled() => {
				tracing::warn!("platform signal during actor drain, proceeding to shutdown");
				true
			}
		};
		// A clean actor-driven exit has no deadline, but if a platform signal arrived
		// mid-drain the reclaim clock is running, so bound the envoy shutdown by the
		// SIGTERM budget like the signal-driven branch does.
		if signaled || SIGNAL_SHUTDOWN.load(Ordering::Acquire) {
			if tokio::time::timeout(*SIGTERM_BUDGET, runtime.shutdown())
				.await
				.is_err()
			{
				tracing::warn!("engine drain exceeded the signal budget");
			}
		} else {
			runtime.shutdown().await;
		}
	}
	serve_shutdown.cancel();

	match serve.await {
		Ok(Ok(())) => {}
		Ok(Err(e)) => tracing::error!(error = ?e, "server error"),
		Err(e) => tracing::error!(error = ?e, "server task join error"),
	}

	tracing::info!("container-runner stopped");
	Ok(())
}

/// Stop every child still in the registry, concurrently so each gets the full
/// `grace`. Actor hooks normally reap their own child first; this is the sweep for
/// the signal path so children are never orphaned.
async fn stop_all_children(grace: Duration) {
	let mut children: Vec<Arc<ChildProcess>> = Vec::new();
	CHILDREN
		.retain_async(|_, child| {
			children.push(child.clone());
			false
		})
		.await;
	if children.is_empty() {
		return;
	}
	println!("runner: shutdown, stopping {} child(ren)", children.len());
	join_all(children.into_iter().map(|child| async move {
		child.stop(grace).await;
		release_child_port(child.child_port).await;
	}))
	.await;
}

fn env_u16(key: &str) -> Option<u16> {
	std::env::var(key).ok().and_then(|s| s.parse().ok())
}

fn spawn_signal_handler() {
	tokio::spawn(async move {
		use tokio::signal::unix::{SignalKind, signal};
		let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
		let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
		tokio::select! {
			_ = sigterm.recv() => {
				PLATFORM_RECLAIM.store(true, Ordering::Release);
				// Attribute the reclaim to each running actor so it shows in
				// actor-scoped logs, not only the process-level stream.
				let mut actor_ids = Vec::new();
				CHILDREN
					.retain_async(|actor_id, _| {
						actor_ids.push(actor_id.clone());
						true
					})
					.await;
				if actor_ids.is_empty() {
					tracing::error!(
						"unexpected platform SIGTERM received, likely hitting OOM or running longer than 60 minutes"
					);
				} else {
					for actor_id in actor_ids {
						tracing::error!(
							actor_id = %actor_id,
							"unexpected platform SIGTERM received, likely hitting OOM or running longer than 60 minutes"
						);
					}
				}
			}
			_ = sigint.recv() => tracing::info!("received SIGINT"),
		}
		SIGNAL_SHUTDOWN.store(true, Ordering::Release);
		SIGNAL_TOKEN.cancel();
		request_exit("-", "signal");
	});
}

#[cfg(test)]
#[path = "../tests/inline/boot_id.rs"]
mod tests;
