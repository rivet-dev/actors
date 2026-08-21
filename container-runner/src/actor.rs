//! The `GameServer` actor: wraps one child game-server process per actor.
//!
//! `on_start` reserves a port and spawns the child (waiting for readiness), `run`
//! watchdogs unexpected child exits, `on_fetch`/`on_websocket` proxy tunneled
//! traffic to the child, and `on_sleep`/`on_destroy` stop it.

use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use async_trait::async_trait;
use rivetkit::{Actor, ActorKeySegment, Ctx, Request, Response, WebSocket, action};
use tokio::sync::Mutex as TokioMutex;

use crate::child::{ChildProcess, SpawnSpec, log_prefix};
use crate::input::ActorInput;
use crate::{
	children, drain_grace, effective_stop_grace, exit_token, release_child_port,
	request_exit, reserve_child_port, runner_config,
};

/// Live actor contexts keyed by actor id, so the shutdown path can report actors
/// as crashed when the platform reclaims the container.
static ACTOR_CTXS: LazyLock<scc::HashMap<String, Ctx<GameServer>>> =
	LazyLock::new(scc::HashMap::new);

pub struct GameServer {
	child: TokioMutex<Option<Arc<ChildProcess>>>,
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
}

#[async_trait]
impl Actor for GameServer {
	// The launch spec is the persisted state: a woken actor restores the same
	// spec without the engine re-sending input.
	type State = ActorInput;
	type Input = ActorInput;
	type Actions = ();
	type Events = ();
	type Queue = ();
	type ConnParams = ();
	type ConnState = ();
	type Action = action::Raw;

	async fn create_state(_ctx: &Ctx<Self>, input: Self::Input) -> Result<Self::State> {
		Ok(input)
	}

	async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
		Ok(Self {
			child: TokioMutex::new(None),
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

		// Copy the launch spec out of the state guard before any await.
		let (input_port, mut parts, env) = {
			let input = ctx.state();
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
		Ok(())
	}

	/// Watchdog for the child exiting. Deliberate stops remove it from the registry
	/// first, so winning the `remove` race means the exit was unexpected: a clean
	/// exit destroys the actor, any other reports an errored stop (a crash).
	async fn run(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
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

	/// Engine-initiated sleep. `no_sleep` blocks only idle sleep; the engine can
	/// still sleep an actor (dashboard, crash policy, eviction), so we stop the child.
	async fn on_sleep(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		self.drain_then_stop_child(ctx.actor_id(), "actor sleeping").await;
		Ok(())
	}

	async fn on_destroy(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		self.stop_child(ctx.actor_id(), "actor stopped").await;
		Ok(())
	}
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
