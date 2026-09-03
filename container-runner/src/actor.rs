//! The `GameServer` actor: wraps one child game-server process per actor.
//!
//! `on_start` reserves a port and spawns the child (waiting for readiness), `run`
//! watchdogs unexpected child exits, `on_fetch`/`on_websocket` proxy tunneled
//! traffic to the child, and `on_sleep`/`on_destroy` stop it.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use async_trait::async_trait;
use rivetkit::{Actor, ActorKeySegment, Ctx, Request, Response, WebSocket, action};
use tokio::sync::Mutex as TokioMutex;

use crate::child::{ChildProcess, SpawnSpec, log_prefix};
use crate::input::{ActorInput, ActorState};
use crate::{
	children, drain_grace, effective_stop_grace, exit_token, idle_timeout,
	idle_timeout_with_jitter, reject_second_start, release_child_port, request_exit,
	reserve_child_port, runner_config, second_start_grace,
};

/// Live actor contexts keyed by actor id, so the shutdown path can report actors
/// as crashed when the platform reclaims the container.
static ACTOR_CTXS: LazyLock<scc::HashMap<String, Ctx<GameServer>>> =
	LazyLock::new(scc::HashMap::new);

/// One-shot idle lifecycle for a generation, in a single atomic. The startup timer sleeps
/// only while `ARMED`; a request moves it to `REQUESTED` (no sleep), and the timer firing with
/// no request moves it to `IDLE_SLEEPING`, which `on_sleep` reads to skip the drain.
const IDLE_ARMED: u8 = 0;
const IDLE_REQUESTED: u8 = 1;
const IDLE_SLEEPING: u8 = 2;

pub struct GameServer {
	child: TokioMutex<Option<Arc<ChildProcess>>>,
	/// One-shot idle state: `IDLE_ARMED` / `IDLE_REQUESTED` / `IDLE_SLEEPING`.
	idle_state: AtomicU8,
	/// Set when `on_start` detected a repeat start and skipped spawning a child, so
	/// `run` destroys the actor instead of running. See [`reject_second_start`].
	reject_start: AtomicBool,
}

impl GameServer {
	/// Shared teardown for sleep and destroy. For a game server they are the same
	/// event: match state lives in the child and cannot outlive the container, and
	/// a later wake respawns an equivalent child from the persisted launch spec.
	async fn stop_child(&self, actor_id: &str, reason: &str) {
		// Remove from the registry first so the watchdog treats the exit as
		// deliberate. `stop` is idempotent if the shutdown sweep already ran.
		children().remove_async(actor_id).await;
		ACTOR_CTXS.remove_async(actor_id).await;
		let child = self.child.lock().await.take();
		if let Some(child) = child {
			child.stop(effective_stop_grace()).await;
			release_child_port(child.child_port).await;
		}

		// Exit the process once the last child stops: `request_exit` cancels
		// `EXIT`, waking `main` to close the envoy and return (the runner is
		// PID 1). Guarded on an empty registry so siblings survive.
		if children().is_empty() {
			request_exit(actor_id, reason);
		} else {
			tracing::info!(
				actor_id = %actor_id,
				reason,
				"actor stopped, other actors still running on this instance"
			);
		}
	}

	/// Engine pause (sleep, lost, going-away): let the child finish and exit on its
	/// own for up to `DRAIN_GRACE` before forcing a stop. A child exit or a platform
	/// SIGTERM (which cancels the exit token) ends the wait early.
	async fn drain_then_stop_child(&self, actor_id: &str, reason: &str) {
		let child = self.child.lock().await.clone();
		if let Some(child) = child {
			if !child.has_exited() {
				let prefix = log_prefix(actor_id, child.key.as_deref());
				println!(
					"{prefix} runner: draining child for up to {:?} before stopping",
					drain_grace()
				);
				tokio::select! {
					_ = child.wait_exit() => {}
					_ = tokio::time::sleep(drain_grace()) => {}
					_ = exit_token().cancelled() => {}
				}
			}
		}
		self.stop_child(actor_id, reason).await;
	}

	/// Arm the one-shot startup idle timer when [`idle_timeout`] is set. After the
	/// window, if no request has arrived, ask the actor to sleep (`stop_child` then
	/// exits the container). Cancelled early if the actor starts shutting down.
	fn arm_idle_timeout(self: &Arc<Self>, ctx: &Ctx<Self>, actor_id: String) {
		let Some(base) = idle_timeout() else {
			return;
		};
		// Jitter the window so instances started together do not sleep in lockstep.
		let delay = idle_timeout_with_jitter(base);
		let this = self.clone();
		let ctx = ctx.clone();
		tokio::spawn(async move {
			let abort = ctx.abort_signal();
			tokio::select! {
				_ = tokio::time::sleep(delay) => {}
				_ = abort.cancelled() => return,
			}
			// Sleep only if still armed. If a request raced in, the CAS fails and we do
			// nothing; on success the state records this as an idle-timer sleep so
			// `on_sleep` skips the drain.
			if this
				.idle_state
				.compare_exchange(
					IDLE_ARMED,
					IDLE_SLEEPING,
					Ordering::SeqCst,
					Ordering::SeqCst,
				)
				.is_err()
			{
				return;
			}
			tracing::info!(actor_id = %actor_id, ?delay, "no request within idle timeout, sleeping");
			if let Err(err) = ctx.sleep() {
				tracing::debug!(error = ?err, actor_id = %actor_id, "idle sleep request failed");
			}
		});
	}

	/// Record that the actor received a request: disarms the one-shot idle timer,
	/// and in idle mode marks the real start on the first request so an idle-slept
	/// actor that never served one can wake without tripping the second-start guard.
	fn note_request(&self, ctx: &Ctx<Self>) {
		// Mark the actor active so the idle timer will not sleep this generation.
		let _ = self.idle_state.compare_exchange(
			IDLE_ARMED,
			IDLE_REQUESTED,
			Ordering::SeqCst,
			Ordering::SeqCst,
		);
		// In idle mode the real start is recorded on the first request. `mark_started_once`
		// is idempotent, so calling it on every request is fine.
		if reject_second_start() && idle_timeout().is_some() {
			mark_started_once(ctx);
		}
	}
}

#[async_trait]
impl Actor for GameServer {
	// The persisted state (launch spec plus `started_once`) is restored on wake, so a
	// woken actor keeps its spec without the engine re-sending input.
	type State = ActorState;
	type Input = ActorInput;
	type Actions = ();
	type Events = ();
	type Queue = ();
	type ConnParams = ();
	type ConnState = ();
	type Action = action::Raw;

	async fn create_state(_ctx: &Ctx<Self>, input: Self::Input) -> Result<Self::State> {
		Ok(ActorState {
			input,
			started_once: false,
		})
	}

	async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
		Ok(Self {
			child: TokioMutex::new(None),
			idle_state: AtomicU8::new(IDLE_ARMED),
			reject_start: AtomicBool::new(false),
		})
	}

	async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		let cfg = runner_config();
		let actor_id = ctx.actor_id().to_string();
		let key = actor_key_string(&ctx);

		// Tagged with the actor id so it shows in actor-scoped log views; the
		// monitor's own process-level enable/disable logs are filtered out there.
		tracing::info!(
			actor_id = %actor_id,
			resource_monitor_enabled = crate::monitor::enabled(),
			resource_monitor_source = crate::monitor::sampling_source(),
			"resource monitor status"
		);

		// An engine retry for an already-running actor must be an idempotent no-op;
		// rejecting it would make the engine tear down a healthy actor.
		if let Some(existing) = children().read_async(&actor_id, |_, c| c.clone()).await {
			if !existing.has_exited() {
				println!(
					"{} runner: actor already running, ignoring duplicate start",
					log_prefix(&actor_id, existing.key.as_deref())
				);
				register_ctx(&actor_id, &ctx).await;
				*self.child.lock().await = Some(existing);
				return Ok(());
			}
		}

		// Second-start guard: if this actor already did its real start (a persisted
		// flag that survives sleep), do not run again. Skip spawning a child; `run`
		// destroys the actor.
		if reject_second_start() && ctx.state().started_once {
			tracing::warn!(actor_id = %actor_id, "actor tried a second-start");
			self.reject_start.store(true, Ordering::Relaxed);
			return Ok(());
		}

		// Copy the launch spec out of the state guard before any await.
		let (input_port, mut parts, env) = {
			let state = ctx.state();
			let input = &state.input;
			// input.command overrides the CLI template; input.args are appended.
			let mut parts = input
				.command
				.clone()
				.unwrap_or_else(|| cfg.command_template.clone());
			parts.extend(input.args.clone());
			(input.port, parts, input.env.clone())
		};
		if parts.is_empty() {
			anyhow::bail!(
				"no child command: CLI template is empty and input.command was not provided"
			);
		}
		let program = parts.remove(0);

		let child_port = reserve_child_port(input_port, cfg.default_child_port).await?;
		let spec = SpawnSpec {
			program,
			args: parts,
			env,
			child_port,
			actor_id: actor_id.clone(),
			key: key.clone(),
		};

		// Tagged with the actor id for actor-scoped logs; `git_sha` is omitted when
		// unknown rather than logged as "unknown".
		match crate::git_sha() {
			Some(git_sha) => tracing::info!(
				actor_id = %actor_id,
				version = crate::VERSION,
				git_sha = %git_sha,
				"container-runner build"
			),
			None => tracing::info!(
				actor_id = %actor_id,
				version = crate::VERSION,
				"container-runner build"
			),
		}

		tracing::info!(
			boot_id = crate::boot_id(),
			actor_id = %actor_id,
			child_port,
			"actor starting on this container instance"
		);

		let child = match ChildProcess::spawn(spec, cfg.readiness_timeout).await {
			Ok(child) => Arc::new(child),
			Err(err) => {
				release_child_port(child_port).await;
				// A failed start is this actor's alone; it does not take down others.
				return Err(err);
			}
		};

		// The global registry lets the shutdown path stop children when hooks never
		// run, and arbitrates the deliberate-stop vs unexpected-exit race in `run`.
		if children()
			.insert_async(actor_id.clone(), child.clone())
			.await
			.is_err()
		{
			// Unreachable given the duplicate-start check above; defensive.
			child.stop(effective_stop_grace()).await;
			release_child_port(child_port).await;
			anyhow::bail!("a child for actor {actor_id} is already registered");
		}
		// Register only after startup succeeds; a failed start never runs a stop
		// hook to remove the entry, so registering earlier would leak it.
		register_ctx(&actor_id, &ctx).await;
		*self.child.lock().await = Some(child);
		// Non-idle mode commits the real start only after the actor survives the
		// second-start grace, so a fast crash-restart is not rejected. Idle mode defers
		// this to the first request (see `note_request`) so an idle-slept actor can wake.
		arm_second_start_mark(&ctx);
		self.arm_idle_timeout(&ctx, actor_id);
		Ok(())
	}

	/// Watchdog for the child exiting. Deliberate stops remove it from the registry
	/// first, so winning the `remove` race means the exit was unexpected: a clean
	/// exit destroys the actor, any other reports an errored stop (a crash).
	async fn run(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		// A rejected repeat start spawned no child; destroy the actor so it tombstones
		// instead of parking. The child-exit destroy below has no pending request.
		if self.reject_start.load(Ordering::Relaxed) {
			if let Err(err) = ctx.destroy() {
				tracing::debug!(error = ?err, actor_id = %ctx.actor_id(), "reject-start destroy failed");
			}
			return Ok(());
		}

		let Some(child) = self.child.lock().await.clone() else {
			anyhow::bail!("run: child process was never spawned");
		};

		let exit = child.wait_exit().await;

		let actor_id = ctx.actor_id().to_string();
		if children().remove_async(&actor_id).await.is_some() {
			release_child_port(child.child_port).await;
			let prefix = log_prefix(&actor_id, child.key.as_deref());
			if !exit.success {
				println!(
					"{prefix} runner: child exited unexpectedly ({exit}), reporting errored stop"
				);
				anyhow::bail!("child exited unexpectedly ({exit})");
			}
			println!(
				"{prefix} runner: child exited unexpectedly ({exit}), reporting actor stopped"
			);
			if let Err(err) = ctx.destroy() {
				// The actor may already be stopping if the engine beat us to it.
				tracing::debug!(error = ?err, actor_id = %actor_id, "destroy after child exit failed");
			}
		}
		Ok(())
	}

	async fn on_fetch(self: Arc<Self>, ctx: Ctx<Self>, req: Request) -> Result<Response> {
		self.note_request(&ctx);
		let child_port = self
			.child
			.lock()
			.await
			.as_ref()
			.map(|child| child.child_port)
			.with_context(|| format!("fetch: no running child for actor {}", ctx.actor_id()))?;
		crate::proxy::http_proxy(child_port, req).await
	}

	async fn on_websocket(
		self: Arc<Self>,
		ctx: Ctx<Self>,
		ws: WebSocket,
		req: Request,
	) -> Result<()> {
		self.note_request(&ctx);
		let child_port = self
			.child
			.lock()
			.await
			.as_ref()
			.map(|child| child.child_port)
			.with_context(|| format!("websocket: no running child for actor {}", ctx.actor_id()))?;
		let path = req
			.uri()
			.path_and_query()
			.map(|pq| pq.as_str().to_string())
			.unwrap_or_else(|| "/".to_string());
		crate::proxy::ws_proxy(child_port, path, ws).await
	}

	/// Engine-initiated sleep. An idle-timer sleep with no request has nothing to drain, so it
	/// stops promptly; every other sleep (an active actor, or idle timeout disabled) drains
	/// in-flight work first. `no_sleep` blocks only idle sleep, not engine-driven sleeps.
	async fn on_sleep(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		if self.idle_state.load(Ordering::SeqCst) == IDLE_SLEEPING {
			self.stop_child(ctx.actor_id(), "actor sleeping (idle)")
				.await;
		} else {
			self.drain_then_stop_child(ctx.actor_id(), "actor sleeping")
				.await;
		}
		Ok(())
	}

	async fn on_destroy(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		self.stop_child(ctx.actor_id(), "actor stopped").await;
		Ok(())
	}
}

/// Non-idle second-start guard: commit `started_once` only after the actor survives
/// [`second_start_grace`], so a child that crashes within the window is not committed
/// and its retry may run. No-op when the guard is off or idle mode is on; cancelled on shutdown.
fn arm_second_start_mark(ctx: &Ctx<GameServer>) {
	if !reject_second_start() || idle_timeout().is_some() {
		return;
	}
	let ctx = ctx.clone();
	let grace = second_start_grace();
	tokio::spawn(async move {
		let abort = ctx.abort_signal();
		tokio::select! {
			_ = tokio::time::sleep(grace) => {}
			_ = abort.cancelled() => return,
		}
		mark_started_once(&ctx);
	});
}

/// Persist `started_once` so a later start is treated as a repeat. Idempotent: a
/// no-op when already set. The read guard is released before the write.
fn mark_started_once(ctx: &Ctx<GameServer>) {
	if ctx.state().started_once {
		return;
	}
	ctx.state_mut().started_once = true;
	ctx.request_save();
}

/// Register an actor context for crash-on-shutdown reporting. Overwrites any
/// stale entry left by a prior generation with the same id.
async fn register_ctx(actor_id: &str, ctx: &Ctx<GameServer>) {
	ACTOR_CTXS.remove_async(actor_id).await;
	let _ = ACTOR_CTXS
		.insert_async(actor_id.to_string(), ctx.clone())
		.await;
}

/// Report every live actor as crashed. Called when the platform reclaims the
/// container (unexpected SIGTERM), while the envoy is still connected, so the
/// reclaim surfaces as a crash on the engine instead of a silent reallocation.
pub async fn crash_all_actors(message: &str) {
	let mut ctxs = Vec::new();
	ACTOR_CTXS
		.retain_async(|_, ctx| {
			ctxs.push(ctx.clone());
			false
		})
		.await;
	for ctx in ctxs {
		if let Err(err) = ctx.stop_with_error(message) {
			tracing::debug!(
				actor_id = %ctx.actor_id(),
				error = ?err,
				"crash-on-shutdown stop_with_error failed"
			);
		}
	}
}

fn actor_key_string(ctx: &Ctx<GameServer>) -> Option<String> {
	let key = ctx.key();
	if key.is_empty() {
		None
	} else {
		Some(
			key.iter()
				.map(|segment| match segment {
					ActorKeySegment::String(value) => value.clone(),
					ActorKeySegment::Number(value) => value.to_string(),
				})
				.collect::<Vec<_>>()
				.join(","),
		)
	}
}
