//! Child game-server process management: spawn, log piping, readiness, SIGTERM stop.
//!
//! A dedicated reaper task owns the `tokio::process::Child` and publishes its exit on a
//! `watch` channel; `stop()` and readiness checks signal/observe via the pid and channel,
//! so they never contend for the child handle (which would deadlock the reaper's `wait()`).

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::{Instant, sleep};

/// Terminal state of a child process.
#[derive(Clone)]
pub struct ChildExit {
	/// True when the child exited on its own with code 0. Signal terminations,
	/// nonzero exits, and wait errors are all failures.
	pub success: bool,
	/// Human-readable status, e.g. "exit status: 1" or "signal: 9 (SIGKILL)".
	pub status: String,
}

impl std::fmt::Display for ChildExit {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.status)
	}
}

/// A spawned child process, its listening port, and the actor identity used to
/// prefix its logs.
pub struct ChildProcess {
	pub actor_id: String,
	pub key: Option<String>,
	pub child_port: u16,
	pid: u32,
	/// `None` while running, `Some(exit)` once the child has exited.
	exited_rx: watch::Receiver<Option<ChildExit>>,
}

/// Everything needed to launch the child.
pub struct SpawnSpec {
	pub program: String,
	pub args: Vec<String>,
	pub env: HashMap<String, String>,
	pub child_port: u16,
	pub actor_id: String,
	pub key: Option<String>,
}

impl ChildProcess {
	/// Spawn the child with piped stdout+stderr, start log-pump tasks that re-emit
	/// each line to the runner's stdout prefixed with `[actor_id=<id> key=<key>]`, start
	/// a reaper task, and wait until the child's TCP port accepts connections.
	pub async fn spawn(spec: SpawnSpec, readiness_timeout: Duration) -> Result<Self> {
		let SpawnSpec {
			program,
			args,
			env,
			child_port,
			actor_id,
			key,
		} = spec;

		let prefix = log_prefix(&actor_id, key.as_deref());

		// Refuse to spawn if the port is already taken. A stale child from a prior start
		// still holding it would make `wait_until_ready` false-positive on the OLD listener
		// while the new child dies with `Address already in use`.
		if TcpStream::connect((Ipv4Addr::LOCALHOST, child_port))
			.await
			.is_ok()
		{
			anyhow::bail!(
				"child port {child_port} is already in use before spawning `{program}`: a \
                 previous game server is still running in this container. container-runner \
                 hosts one actor per container; configure the serverless runner with \
                 max_concurrent_actors=1 and platform request concurrency=1."
			);
		}

		let mut child = Command::new(&program)
			.args(&args)
			.env("PORT", child_port.to_string())
			.envs(&env)
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.kill_on_drop(true)
			.spawn()
			.with_context(|| format!("failed to spawn child process `{program}`"))?;

		let pid = child
			.id()
			.context("child process has no pid immediately after spawn")?;

		// Pump stdout + stderr to the runner's stdout with the actor prefix.
		if let Some(stdout) = child.stdout.take() {
			spawn_log_pump(stdout, prefix.clone());
		}
		if let Some(stderr) = child.stderr.take() {
			spawn_log_pump(stderr, prefix.clone());
		}

		println!("{prefix} runner: spawned `{program}` (pid={pid}) on child port {child_port}");

		// Reaper task owns the Child and reports its exit status.
		let (exited_tx, exited_rx) = watch::channel::<Option<ChildExit>>(None);
		{
			let prefix = prefix.clone();
			tokio::spawn(async move {
				let exit = match child.wait().await {
					Ok(status) => ChildExit {
						success: status.success(),
						status: status.to_string(),
					},
					Err(e) => ChildExit {
						success: false,
						status: format!("wait error: {e}"),
					},
				};
				println!("{prefix} runner: child process exited (status: {exit})");
				let _ = exited_tx.send(Some(exit));
			});
		}

		let this = ChildProcess {
			actor_id,
			key,
			child_port,
			pid,
			exited_rx,
		};

		// Kill the child before surfacing a readiness failure; dropping `this` alone would
		// not (the reaper owns the tokio Child), leaving a hung child running.
		if let Err(err) = this.wait_until_ready(readiness_timeout).await {
			this.stop(Duration::from_secs(2)).await;
			return Err(err);
		}
		Ok(this)
	}

	/// Poll the child's local TCP port until it accepts a connection or times out.
	/// If the child exits before becoming ready, fail fast.
	async fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
		let deadline = Instant::now() + timeout;
		let addr = (Ipv4Addr::LOCALHOST, self.child_port);
		let prefix = log_prefix(&self.actor_id, self.key.as_deref());
		loop {
			if let Some(exit) = self.exited_rx.borrow().clone() {
				anyhow::bail!("child exited before becoming ready (status: {exit})");
			}
			if TcpStream::connect(addr).await.is_ok() {
				println!(
					"{prefix} runner: child is ready (port {} open)",
					self.child_port
				);
				return Ok(());
			}
			if Instant::now() >= deadline {
				anyhow::bail!(
					"child did not open port {} within {:?}",
					self.child_port,
					timeout
				);
			}
			sleep(Duration::from_millis(150)).await;
		}
	}

	pub fn has_exited(&self) -> bool {
		self.exited_rx.borrow().is_some()
	}

	/// Wait until the child exits (on its own or via `stop`), returning its exit state.
	pub async fn wait_exit(&self) -> ChildExit {
		let mut rx = self.exited_rx.clone();
		loop {
			if let Some(exit) = rx.borrow().clone() {
				return exit;
			}
			if rx.changed().await.is_err() {
				return ChildExit {
					success: false,
					status: "reaper task ended".to_string(),
				};
			}
		}
	}

	/// Gracefully stop the child: SIGTERM, wait up to `grace`, then SIGKILL. Reaping
	/// is handled by the reaper task.
	pub async fn stop(&self, grace: Duration) {
		let prefix = log_prefix(&self.actor_id, self.key.as_deref());

		if self.has_exited() {
			return;
		}

		println!("{prefix} runner: sending SIGTERM to pid {}", self.pid);
		let _ = signal::kill(Pid::from_raw(self.pid as i32), Signal::SIGTERM);

		match tokio::time::timeout(grace, self.wait_exit()).await {
			Ok(status) => {
				println!("{prefix} runner: child stopped gracefully (status: {status})");
			}
			Err(_) => {
				println!(
					"{prefix} runner: grace elapsed, sending SIGKILL to pid {}",
					self.pid
				);
				let _ = signal::kill(Pid::from_raw(self.pid as i32), Signal::SIGKILL);
				let status = self.wait_exit().await;
				println!("{prefix} runner: child killed (status: {status})");
			}
		}
	}
}

fn spawn_log_pump<R>(reader: R, prefix: String)
where
	R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
	tokio::spawn(async move {
		let mut lines = BufReader::new(reader).lines();
		loop {
			match lines.next_line().await {
				Ok(Some(line)) => println!("{prefix} {line}"),
				Ok(None) => break,
				Err(e) => {
					eprintln!("{prefix} runner: error reading child log stream: {e}");
					break;
				}
			}
		}
	});
}

/// The per-line log prefix: `[actor_id=<id> key=<key>]`. The dashboard filters on the
/// `actor_id=<id>` token, so the field name must be `actor_id`, not `actor`.
pub fn log_prefix(actor_id: &str, key: Option<&str>) -> String {
	match key {
		Some(key) => format!("[actor_id={actor_id} key={key}]"),
		None => format!("[actor_id={actor_id} key=]"),
	}
}
