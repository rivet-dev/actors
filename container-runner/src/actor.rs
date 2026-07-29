//! The `GameServer` actor: wraps one child game-server process per actor.
//!
//! Lifecycle: `on_start` reserves a port and spawns the child, waiting for
//! readiness (so the actor is never reported ready before the child listens),
//! `run` is a watchdog that reports unexpected child exits,
//! `on_fetch`/`on_websocket` proxy tunneled traffic to the child's port, and
//! `on_destroy` stops the child while the instance stays warm for the next
//! placement.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rivetkit::{Actor, ActorKeySegment, Ctx, Request, Response, WebSocket, action};
use tokio::sync::Mutex as TokioMutex;

use crate::child::{ChildProcess, SpawnSpec, log_prefix};
use crate::input::ActorInput;
use crate::{
	children, effective_stop_grace, release_child_port, reserve_child_port,
	runner_config,
};

pub struct GameServer {
	child: TokioMutex<Option<Arc<ChildProcess>>>,
}

impl GameServer {
	/// Shared teardown for sleep and destroy: for a game server the two are
	/// materially the same event, because the in-memory match state lives in
	/// the child and cannot outlive the container. The launch spec is the
	/// persisted actor state, so a later wake respawns an equivalent child.
	async fn stop_child(&self, actor_id: &str, reason: &str) {
		// Remove from the registry FIRST so the watchdog treats the exit as
		// deliberate, then stop. `stop` is idempotent if the process shutdown
		// sweep already stopped this child.
		children().remove_async(actor_id).await;
		let child = self.child.lock().await.take();
		if let Some(child) = child {
			child.stop(effective_stop_grace()).await;
			release_child_port(child.child_port).await;
		}

		// The instance stays alive and warm after its last actor stops, ready to
		// host the next placement. It is reaped by the platform's own shutdown
		// signal, not by self-exit. This keeps the serverless container long
		// lived enough for the log agent to drain its stderr, which a fast
		// self-exit could otherwise lose.
		tracing::info!(actor_id = %actor_id, reason, "actor stopped, keeping instance warm");
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

		// An engine retry for an actor that is already running here must be an
		// idempotent no-op: rejecting it would make the engine tear down a
		// healthy actor.
		if let Some(existing) = children().read_async(&actor_id, |_, c| c.clone()).await {
			if !existing.has_exited() {
				println!(
					"{} runner: actor already running, ignoring duplicate start",
					log_prefix(&actor_id, existing.key.as_deref())
				);
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
				// A failed start is this actor's alone and does not take the
				// instance down. The container stays warm and ready for the next
				// placement, and stays alive long enough for the log agent to
				// drain the failure logs before the platform reaps it.
				return Err(err);
			}
		};

		// The global registry lets the process shutdown path stop children
		// even when actor hooks never run, and arbitrates the deliberate-stop
		// vs unexpected-exit race for the watchdog in `run`.
		if children()
			.insert_async(actor_id.clone(), child.clone())
			.await
			.is_err()
		{
			// Unreachable given the duplicate-start check above; defensive.
			child.stop(cfg.stop_grace).await;
			release_child_port(child_port).await;
			anyhow::bail!("a child for actor {actor_id} is already registered");
		}
		*self.child.lock().await = Some(child);
		Ok(())
	}

	/// Watchdog: waits for the child to exit. Deliberate stops remove the
	/// child from the global registry first, so winning the `remove` race
	/// means the exit was unexpected and the actor must be torn down. A clean
	/// exit (code 0) destroys the actor; any other exit returns an error so
	/// the framework reports an errored stop and the engine records the crash.
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

	/// Engine-initiated sleep. `no_sleep` suppresses idle sleep, but the
	/// engine can still sleep an actor (dashboard, crash policy, eviction
	/// ahead of instance retirement); leaving the child running would orphan
	/// it on an instance the engine considers vacated.
	async fn on_sleep(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		self.stop_child(ctx.actor_id(), "actor sleeping").await;
		Ok(())
	}

	async fn on_destroy(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
		self.stop_child(ctx.actor_id(), "actor stopped").await;
		Ok(())
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
