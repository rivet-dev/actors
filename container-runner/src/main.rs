//! `rivet-container-runner` — a RivetKit serverless app that hosts a single
//! actor by spawning a child game-server process and proxying Rivet's
//! tunneled traffic to it.
//!
//! Serverless model: the engine's `POST /api/rivet/start` starts this
//! container (served by rivetkit-core's serverless runtime). On actor start
//! the `GameServer` actor spawns the child (`-- <command...>`), pipes its
//! logs to stdout prefixed with the actor id + key, and proxies inbound
//! HTTP/WebSocket (arriving over Rivet's tunnel) to the child's local port.
//! On actor stop it SIGTERMs the child.
//!
//! The runner hosts as many concurrent actors as the engine places on it,
//! each with its own child process on its own port; the pool's request
//! concurrency decides how many that is (1 in the recommended game-server
//! setup). The instance stays warm after its last actor stops and never
//! self-exits; the engine reaps it by draining the `/start` connection once
//! the request lifespan elapses, or the platform sends a SIGTERM.

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
use rivetkit::serverless_http::{self, ListenerConfig};
use rivetkit::{ActorConfig, EngineSpawnMode, Registry, ServeConfig};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::actor::GameServer;
use crate::child::ChildProcess;

/// Static runner configuration derived from the CLI/env.
pub struct RunnerConfig {
	/// Child command template (program + fixed args) from `-- <command...>`.
	pub command_template: Vec<String>,
	/// Default local port for the child when `input.port` is absent.
	pub default_child_port: u16,
	/// SIGTERM→SIGKILL grace period on stop.
	pub stop_grace: Duration,
	/// How long to wait for the child's port to open before failing the start.
	pub readiness_timeout: Duration,
}

// The `Actor` trait constructs actors without user parameters, so the runner
// configuration is ambient process state set once in `main`.
static RUNNER_CONFIG: OnceLock<Arc<RunnerConfig>> = OnceLock::new();

/// Running children keyed by actor id. Owned globally (not only by actors) so
/// the process shutdown path can stop children even if actor hooks never run,
/// and so the watchdog and stop paths can arbitrate who reports an exit.
static CHILDREN: LazyLock<scc::HashMap<String, Arc<ChildProcess>>> =
	LazyLock::new(scc::HashMap::new);

/// Child ports currently reserved by a spawning or running child. Multiple
/// actors may run concurrently (engine placement decides how many land here),
/// so each child needs its own port and concurrent starts must not race to
/// the same one.
static RESERVED_PORTS: LazyLock<scc::HashSet<u16>> = LazyLock::new(scc::HashSet::new);

/// Cancelled to bring the whole process down (actor stopped, failed start, or
/// signal). `main` owns the exit sequencing.
static EXIT: LazyLock<CancellationToken> = LazyLock::new(CancellationToken::new);

/// Set when the process is shutting down because the PLATFORM sent a signal.
/// The hosting platform gives a container only a bounded window (often ~10
/// seconds) between SIGTERM and SIGKILL, so every grace period on this path must
/// fit that budget; engine-initiated stops keep the full configured grace
/// (their budget is the pool's drain grace period instead).
static SIGNAL_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Set when shutdown was triggered by a platform SIGTERM (an instance reclaim),
/// as opposed to a local SIGINT (developer Ctrl-C). Distinguishes a reclaim
/// from a manual stop for downstream shutdown handling.
static PLATFORM_RECLAIM: AtomicBool = AtomicBool::new(false);

/// How long the platform gives this container between SIGTERM and SIGKILL.
/// Defaults to 10 seconds; keep this in sync with the platform's actual budget
/// via RIVET_SIGTERM_BUDGET_SECS.
/// The signal-path teardown splits the budget: ~60% for the engine drain
/// (whose per-actor stops SIGTERM children with a grace capped at ~40%), 1s
/// for the straggler sweep, and the rest as margin.
static SIGTERM_BUDGET: LazyLock<Duration> = LazyLock::new(|| {
	let secs = std::env::var("RIVET_SIGTERM_BUDGET_SECS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.unwrap_or(10)
		.max(3);
	Duration::from_secs(secs)
});

fn signal_drain_timeout() -> Duration {
	SIGTERM_BUDGET.mul_f64(0.6)
}

fn signal_child_stop_grace() -> Duration {
	SIGTERM_BUDGET.mul_f64(0.4)
}

const SIGNAL_SWEEP_GRACE: Duration = Duration::from_secs(1);

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

/// Reserve a local port for a new child. An explicit `input.port` is honored
/// or refused if another child holds it; otherwise the first free port at or
/// above the CLI default is picked. The reservation guards the window between
/// port selection and the child actually binding; release it via
/// [`release_child_port`] once the child is gone.
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

/// The SIGTERM→SIGKILL grace to give a child right now: the configured
/// `--stop-grace-secs` normally, capped to the platform budget while the
/// process is shutting down due to an OS signal.
pub fn effective_stop_grace() -> Duration {
	let grace = runner_config().stop_grace;
	if SIGNAL_SHUTDOWN.load(Ordering::Acquire) {
		grace.min(signal_child_stop_grace())
	} else {
		grace
	}
}

/// End the process. Only the platform shutdown signal drives this now: actors
/// stopping or failing to start no longer exit the instance, so it stays warm
/// and reusable and its logs have time to drain. The runner is PID 1 in the
/// image, so exiting stops the container and the platform reaps the instance.
pub fn request_exit(actor_id: &str, reason: &str) {
	tracing::info!(actor_id = %actor_id, reason, "shutting down container");
	EXIT.cancel();
}

/// Stable per-process identifier, generated once. The runner is PID 1 in the
/// image, so one boot id == one container instance. Logged at startup and on
/// every actor start so an actor can be attributed to an instance (the log
/// stream carries no instance id).
pub fn boot_id() -> &'static str {
	static BOOT_ID: OnceLock<String> = OnceLock::new();
	BOOT_ID.get_or_init(|| {
		// 9 random bytes -> 12 chars. Falls back to a fixed marker if the
		// CSPRNG is unavailable, which would itself be worth seeing in logs.
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
	/// Serverless HTTP front-door port. Rivet Compute injects RIVET_PORT (other
	/// serverless platforms use the conventional PORT); resolved in `main` as
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

	/// SIGTERM→SIGKILL grace period (seconds) when stopping the child.
	#[arg(long, env = "RIVET_STOP_GRACE_SECS", default_value_t = 10)]
	stop_grace_secs: u64,

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

	// Front-door port: Rivet Compute injects RIVET_PORT; other serverless platforms use the conventional PORT.
	let port = args
		.port
		.or_else(|| env_u16("RIVET_PORT"))
		.or_else(|| env_u16("PORT"))
		.unwrap_or(8080);

	let stop_grace = Duration::from_secs(args.stop_grace_secs);
	RUNNER_CONFIG
		.set(Arc::new(RunnerConfig {
			command_template: args.command.clone(),
			default_child_port: args.child_port,
			stop_grace,
			readiness_timeout: Duration::from_secs(args.readiness_timeout_secs),
		}))
		.map_err(|_| anyhow::anyhow!("runner config already set"))?;

	let mut registry = Registry::new();
	registry.register_actor_with::<GameServer>(
		&args.actor_name,
		ActorConfig {
			// Game servers hold live in-memory state; never idle-sleep the actor.
			no_sleep: true,
			// The destroy grace deadline must outlast the child's SIGTERM→SIGKILL
			// window or core aborts `on_destroy` mid-stop and leaks the child.
			sleep_grace_period: stop_grace + Duration::from_secs(10),
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

	// Wait for an exit request, then tear down. Only the signal path is live
	// today: nothing calls `request_exit` except `spawn_signal_handler`, which
	// sets `SIGNAL_SHUTDOWN` before cancelling `EXIT`, so the `else` branch is
	// currently unreachable and kept only as a fallback for a future
	// actor-driven exit.
	//
	// Signal (platform is reclaiming the instance): tell the engine FIRST so
	// it can start re-placing actors immediately. Its per-actor stops run our
	// on_destroy hooks, which SIGTERM children with the capped signal grace.
	// The drain is bounded so an unreachable engine cannot eat the whole
	// platform budget; the sweep then catches any child whose hooks never ran.
	//
	// Fallback actor-driven exit (unreachable today): no platform deadline.
	// Children are already reaped by the hooks (the sweep is a no-op backstop),
	// and the runtime drains unbounded so the /start SSE flushes cleanly.
	EXIT.cancelled().await;
	if SIGNAL_SHUTDOWN.load(Ordering::Acquire) {
		// A platform SIGTERM reclaims this instance. Report every actor as crashed
		// before draining so an unexpected SIGTERM (OOM or the ~60 minute request
		// cap) surfaces as a crash on the engine instead of a silent reallocation.
		// This runs while the envoy is still connected so the crash reaches the
		// engine. A local SIGINT (Ctrl-C) drains gracefully without a crash.
		if PLATFORM_RECLAIM.load(Ordering::Acquire) {
			crate::actor::crash_all_actors(
				"runner received unexpected platform SIGTERM, likely OOM or running longer than 60 minutes",
			)
			.await;
		}
		if tokio::time::timeout(signal_drain_timeout(), runtime.shutdown())
			.await
			.is_err()
		{
			tracing::warn!("engine drain exceeded the signal budget, sweeping children directly");
		}
		stop_all_children(SIGNAL_SWEEP_GRACE).await;
	} else {
		stop_all_children(stop_grace).await;
		runtime.shutdown().await;
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

/// Stop every child still in the registry. Actor `on_destroy` normally reaps
/// its own child first; this is the belt-and-suspenders sweep for the signal
/// path so children are never orphaned.
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
	for child in children {
		child.stop(grace).await;
		release_child_port(child.child_port).await;
	}
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
				// Attribute the reclaim to each running actor so it is visible in
				// actor-scoped logs, not only the process-level log stream.
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
		request_exit("-", "signal");
	});
}

#[cfg(test)]
#[path = "../tests/inline/boot_id.rs"]
mod tests;
