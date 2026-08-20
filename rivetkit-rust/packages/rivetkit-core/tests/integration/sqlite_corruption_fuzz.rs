use std::collections::{HashMap, HashSet};
use std::env;
use std::io::Cursor;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use futures::future::try_join_all;
use rivetkit_core::{
	ActorConfig, ActorEvent, ActorFactory, CoreRegistry, RequestSaveOpts, SerializeStateReason,
	StateDelta,
};
use serde_json::{Value as JsonValue, json};

use crate::common::ctx::IntegrationCtx;

const ACTOR_NAME: &str = "sqlite-fuzz";
const DEFAULT_ACTOR_COUNT: usize = 6;
const DEFAULT_STEPS_PER_ACTOR: usize = 80;
const DEFAULT_RESTART_EVERY_ROUNDS: usize = 20;
const DEFAULT_ACTION_TIMEOUT_SECS: usize = 20;
const DEFAULT_MID_ROUND_RESTART_EVERY_ROUNDS: usize = 0;
const DEFAULT_MID_ROUND_RESTART_DELAY_MS: usize = 75;
const DEFAULT_SAVE_EVERY_STEPS: usize = 1;
const DEFAULT_ENGINE_RESTART_EVERY_ROUNDS: usize = 0;
const DEFAULT_MID_ROUND_ENGINE_RESTART_EVERY_ROUNDS: usize = 0;
const DEFAULT_MID_ROUND_ENGINE_RESTART_DELAY_MS: usize = 75;
const DEFAULT_SUSPECT_PROBE_ROUNDS: usize = 3;
const DEFAULT_PAYLOAD_MULTIPLIER: usize = 1;
const DEFAULT_FINAL_CHECK_TIMEOUT_SECS: usize = 30;
const DEFAULT_FINAL_CHECK_ATTEMPTS: usize = 2;

struct FuzzActor {
	actor_id: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "diagnostic fuzz harness. Run manually with RIVET_ENGINE_BINARY_PATH pointing at the target engine."]
async fn sqlite_lifecycle_fuzz_real_engine() -> Result<()> {
	let mut ctx = IntegrationCtx::builder().start().await?;
	ctx.create_default_namespace().await?;
	let actor_count = env_usize("SQLITE_CORRUPTION_FUZZ_ACTORS", DEFAULT_ACTOR_COUNT);
	let steps_per_actor = env_usize("SQLITE_CORRUPTION_FUZZ_STEPS", DEFAULT_STEPS_PER_ACTOR);
	let restart_every_rounds = env_usize(
		"SQLITE_CORRUPTION_FUZZ_RESTART_EVERY_ROUNDS",
		DEFAULT_RESTART_EVERY_ROUNDS,
	);
	let action_timeout = Duration::from_secs(env_usize(
		"SQLITE_CORRUPTION_FUZZ_ACTION_TIMEOUT_SECS",
		DEFAULT_ACTION_TIMEOUT_SECS,
	) as u64);
	let final_check_timeout = Duration::from_secs(env_usize(
		"SQLITE_CORRUPTION_FUZZ_FINAL_CHECK_TIMEOUT_SECS",
		DEFAULT_FINAL_CHECK_TIMEOUT_SECS,
	) as u64);
	let final_check_attempts = env_usize(
		"SQLITE_CORRUPTION_FUZZ_FINAL_CHECK_ATTEMPTS",
		DEFAULT_FINAL_CHECK_ATTEMPTS,
	)
	.max(1);
	let mid_round_restart_every_rounds = env_usize(
		"SQLITE_CORRUPTION_FUZZ_MID_ROUND_RESTART_EVERY_ROUNDS",
		DEFAULT_MID_ROUND_RESTART_EVERY_ROUNDS,
	);
	let mid_round_restart_delay = Duration::from_millis(env_usize(
		"SQLITE_CORRUPTION_FUZZ_MID_ROUND_RESTART_DELAY_MS",
		DEFAULT_MID_ROUND_RESTART_DELAY_MS,
	) as u64);
	let engine_restart_every_rounds = env_usize(
		"SQLITE_CORRUPTION_FUZZ_ENGINE_RESTART_EVERY_ROUNDS",
		DEFAULT_ENGINE_RESTART_EVERY_ROUNDS,
	);
	let mid_round_engine_restart_every_rounds = env_usize(
		"SQLITE_CORRUPTION_FUZZ_MID_ROUND_ENGINE_RESTART_EVERY_ROUNDS",
		DEFAULT_MID_ROUND_ENGINE_RESTART_EVERY_ROUNDS,
	);
	let mid_round_engine_restart_delay = Duration::from_millis(env_usize(
		"SQLITE_CORRUPTION_FUZZ_MID_ROUND_ENGINE_RESTART_DELAY_MS",
		DEFAULT_MID_ROUND_ENGINE_RESTART_DELAY_MS,
	) as u64);
	let suspect_probe_rounds = env_usize(
		"SQLITE_CORRUPTION_FUZZ_SUSPECT_PROBE_ROUNDS",
		DEFAULT_SUSPECT_PROBE_ROUNDS,
	);
	let run_id = fuzz_run_id();
	tracing::info!(run_id, "starting sqlite corruption fuzz run");
	let mut registry_task = ctx.serve_registry(sqlite_fuzz_registry());

	ctx.wait_for_envoy_ready().await?;
	let mut actors = Vec::new();
	let mut all_actor_ids = HashSet::new();
	let mut all_actor_keys = HashSet::new();
	for index in 0..actor_count {
		let key = format!("sqlite-fuzz-{run_id}-{index}");
		let actor = ctx
			.create_actor_with_key(ACTOR_NAME, Some(&key))
			.await
			.with_context(|| format!("create fuzz actor {key}"))?;
		all_actor_ids.insert(actor.actor_id.clone());
		all_actor_keys.insert(key.clone());
		actors.push(FuzzActor {
			actor_id: actor.actor_id,
		});
	}

	let mut replacement_index = 0;
	let mut suspect_unavailable_counts = HashMap::new();
	for round in 0..steps_per_actor {
		let mut targets = Vec::with_capacity(actors.len() + actors.len() / 2);
		targets.extend(actors.iter().map(|actor| actor.actor_id.clone()));
		if round % 7 == 0 {
			targets.extend(actors.iter().step_by(2).map(|actor| actor.actor_id.clone()));
		}

		let client = ctx.client();
		let endpoint = ctx.endpoint().to_owned();
		let action_futures = targets.into_iter().map(|actor_id| {
			let client = client.clone();
			let endpoint = endpoint.clone();
			async move { run_actor_step_direct(client, endpoint, actor_id, action_timeout).await }
		});
		let attempts_future = try_join_all(action_futures);
		let should_restart_mid_engine = mid_round_engine_restart_every_rounds != 0
			&& (round + 1) % mid_round_engine_restart_every_rounds == 0
			&& round + 1 < steps_per_actor;
		let should_restart_mid_round = mid_round_restart_every_rounds != 0
			&& (round + 1) % mid_round_restart_every_rounds == 0
			&& round + 1 < steps_per_actor;
		let mut restarted_mid_round = false;
		let attempts = if should_restart_mid_engine {
			tokio::pin!(attempts_future);
			tokio::select! {
				attempts = &mut attempts_future => attempts?,
				_ = tokio::time::sleep(mid_round_engine_restart_delay) => {
					restarted_mid_round = true;
					tracing::warn!(
						round,
						delay_ms = mid_round_engine_restart_delay.as_millis(),
						"restarting sqlite fuzz engine child while actions are in flight"
					);
					registry_task.shutdown().await?;
					ctx.restart_engine().await?;
					ctx.create_default_namespace().await?;
					registry_task = ctx.serve_registry(sqlite_fuzz_registry());
					ctx.wait_for_envoy_ready().await?;
					attempts_future.await?
				}
			}
		} else if should_restart_mid_round {
			tokio::pin!(attempts_future);
			tokio::select! {
				attempts = &mut attempts_future => attempts?,
				_ = tokio::time::sleep(mid_round_restart_delay) => {
					restarted_mid_round = true;
					tracing::warn!(
						round,
						delay_ms = mid_round_restart_delay.as_millis(),
						"restarting sqlite fuzz registry while actions are in flight"
					);
					registry_task.shutdown().await?;
					tokio::time::sleep(Duration::from_millis(150)).await;
					registry_task = ctx.serve_registry(sqlite_fuzz_registry());
					ctx.wait_for_envoy_ready().await?;
					attempts_future.await?
				}
			}
		} else {
			attempts_future.await?
		};
		let unavailable = attempts
			.into_iter()
			.filter_map(|attempt| match attempt {
				ActionAttempt::Ok => None,
				ActionAttempt::Unavailable { actor_id, reason } => Some((actor_id, reason)),
			})
			.collect::<Vec<_>>();
		let unavailable_actor_ids = unavailable
			.iter()
			.map(|(actor_id, _)| actor_id.clone())
			.collect::<HashSet<_>>();
		if !unavailable_actor_ids.is_empty() {
			tracing::warn!(
				?unavailable,
				"sqlite fuzz actor unavailable after lifecycle churn"
			);
			let mut replacement_actor_ids = HashSet::new();
			for actor_id in unavailable.iter().map(|(actor_id, _)| actor_id) {
				let client = ctx.client();
				let endpoint = ctx.endpoint().to_owned();
				match run_actor_check_direct(client, endpoint, actor_id.clone(), action_timeout)
					.await?
				{
					ActionAttempt::Ok => {
						suspect_unavailable_counts.remove(actor_id);
					}
					ActionAttempt::Unavailable { .. } => {
						let unavailable_count = suspect_unavailable_counts
							.entry(actor_id.clone())
							.or_insert(0);
						*unavailable_count += 1;
						if *unavailable_count >= suspect_probe_rounds {
							replacement_actor_ids.insert(actor_id.clone());
						} else {
							tracing::warn!(
								actor_id,
								unavailable_count = *unavailable_count,
								suspect_probe_rounds,
								"retaining sqlite fuzz suspect actor for later recheck"
							);
						}
					}
				}
			}
			for actor_id in &replacement_actor_ids {
				suspect_unavailable_counts.remove(actor_id);
			}
			actors.retain(|actor| !replacement_actor_ids.contains(&actor.actor_id));
			for _ in 0..replacement_actor_ids.len() {
				let key = format!("sqlite-fuzz-{run_id}-replacement-{round}-{replacement_index}");
				replacement_index += 1;
				let actor = ctx
					.create_actor_with_key(ACTOR_NAME, Some(&key))
					.await
					.with_context(|| format!("create replacement fuzz actor {key}"))?;
				all_actor_ids.insert(actor.actor_id.clone());
				all_actor_keys.insert(key.clone());
				actors.push(FuzzActor {
					actor_id: actor.actor_id,
				});
			}
		}

		if restart_every_rounds != 0
			&& (round + 1) % restart_every_rounds == 0
			&& round + 1 < steps_per_actor
			&& !restarted_mid_round
			&& !(engine_restart_every_rounds != 0 && (round + 1) % engine_restart_every_rounds == 0)
		{
			registry_task.shutdown().await?;
			tokio::time::sleep(Duration::from_millis(150)).await;
			registry_task = ctx.serve_registry(sqlite_fuzz_registry());
			ctx.wait_for_envoy_ready().await?;
		}

		if engine_restart_every_rounds != 0
			&& (round + 1) % engine_restart_every_rounds == 0
			&& round + 1 < steps_per_actor
		{
			tracing::warn!(round, "restarting sqlite fuzz engine child between rounds");
			registry_task.shutdown().await?;
			ctx.restart_engine().await?;
			ctx.create_default_namespace().await?;
			registry_task = ctx.serve_registry(sqlite_fuzz_registry());
			ctx.wait_for_envoy_ready().await?;
		}
	}

	registry_task.shutdown().await?;
	ctx.restart_engine().await?;
	ctx.create_default_namespace().await?;
	registry_task = ctx.serve_registry(sqlite_fuzz_registry());
	ctx.wait_for_envoy_ready().await?;

	for actor_id in all_actor_ids {
		let client = ctx.client();
		let endpoint = ctx.endpoint().to_owned();
		match run_actor_check_with_retries(
			client,
			endpoint,
			actor_id.clone(),
			final_check_timeout,
			final_check_attempts,
		)
		.await?
		{
			ActionAttempt::Ok => {}
			ActionAttempt::Unavailable { reason, .. } => {
				tracing::warn!(
					actor_id,
					reason,
					"sqlite fuzz final actor check unavailable"
				);
			}
		}
	}

	for key in all_actor_keys {
		let actor = match create_or_get_actor_with_retries(&ctx, &key, final_check_attempts).await {
			Ok(actor) => actor,
			Err(err) => {
				tracing::warn!(
					key,
					error = format!("{err:#}"),
					"sqlite fuzz final actor key resolve failed"
				);
				continue;
			}
		};
		let client = ctx.client();
		let endpoint = ctx.endpoint().to_owned();
		match run_actor_check_with_retries(
			client,
			endpoint,
			actor.actor_id.clone(),
			final_check_timeout,
			final_check_attempts,
		)
		.await?
		{
			ActionAttempt::Ok => {}
			ActionAttempt::Unavailable { reason, .. } => {
				tracing::warn!(
					key,
					actor_id = actor.actor_id,
					reason,
					"sqlite fuzz final actor key check unavailable"
				);
			}
		}
	}

	registry_task.shutdown().await?;
	ctx.shutdown().await?;

	Ok(())
}

enum ActionAttempt {
	Ok,
	Unavailable { actor_id: String, reason: String },
}

async fn run_actor_step_direct(
	client: reqwest::Client,
	endpoint: String,
	actor_id: String,
	action_timeout: Duration,
) -> Result<ActionAttempt> {
	run_actor_action_direct(client, endpoint, actor_id, "step", action_timeout).await
}

async fn run_actor_check_direct(
	client: reqwest::Client,
	endpoint: String,
	actor_id: String,
	action_timeout: Duration,
) -> Result<ActionAttempt> {
	run_actor_action_direct(client, endpoint, actor_id, "check", action_timeout).await
}

async fn run_actor_check_with_retries(
	client: reqwest::Client,
	endpoint: String,
	actor_id: String,
	action_timeout: Duration,
	attempts: usize,
) -> Result<ActionAttempt> {
	let mut last_unavailable = None;
	for attempt in 0..attempts {
		match run_actor_check_direct(
			client.clone(),
			endpoint.clone(),
			actor_id.clone(),
			action_timeout,
		)
		.await?
		{
			ActionAttempt::Ok => return Ok(ActionAttempt::Ok),
			ActionAttempt::Unavailable { reason, .. } => {
				last_unavailable = Some(reason);
				if attempt + 1 < attempts {
					tokio::time::sleep(Duration::from_secs(1)).await;
				}
			}
		}
	}

	Ok(ActionAttempt::Unavailable {
		actor_id,
		reason: last_unavailable.unwrap_or_else(|| "final check unavailable".to_owned()),
	})
}

async fn create_or_get_actor_with_retries(
	ctx: &IntegrationCtx,
	key: &str,
	attempts: usize,
) -> Result<crate::common::ctx::ApiActor> {
	let mut last_error = None;
	for attempt in 0..attempts {
		match ctx.create_or_get_actor_with_key(ACTOR_NAME, key).await {
			Ok(actor) => return Ok(actor),
			Err(err) => {
				last_error = Some(err);
				if attempt + 1 < attempts {
					tokio::time::sleep(Duration::from_secs(1)).await;
				}
			}
		}
	}

	Err(last_error.expect("at least one create-or-get attempt should have run"))
}

async fn run_actor_action_direct(
	client: reqwest::Client,
	endpoint: String,
	actor_id: String,
	action: &'static str,
	action_timeout: Duration,
) -> Result<ActionAttempt> {
	let action_result = tokio::time::timeout(
		action_timeout,
		wait_for_json_action_direct(&client, &endpoint, &actor_id, action),
	)
	.await;
	let body = match action_result {
		Ok(Ok(body)) => body,
		Ok(Err(err)) if is_actor_unavailable(&err) => {
			return Ok(ActionAttempt::Unavailable {
				actor_id,
				reason: format!("{err:#}"),
			});
		}
		Ok(Err(err)) => {
			return Err(err)
				.with_context(|| format!("run sqlite fuzz {action} for actor {actor_id}"));
		}
		Err(_) => {
			return Ok(ActionAttempt::Unavailable {
				actor_id,
				reason: format!("action timed out after {action_timeout:?}"),
			});
		}
	};
	let output = action_output(&body)?;
	assert_eq!(
		output.get("integrity").and_then(JsonValue::as_str),
		Some("ok")
	);
	assert_eq!(
		output.get("quick_check").and_then(JsonValue::as_str),
		Some("ok")
	);
	Ok(ActionAttempt::Ok)
}

async fn wait_for_json_action_direct(
	client: &reqwest::Client,
	endpoint: &str,
	actor_id: &str,
	action: &str,
) -> Result<String> {
	let deadline = Instant::now() + Duration::from_secs(30);
	let mut last_error = None;
	while Instant::now() < deadline {
		match send_json_action_direct(client, endpoint, actor_id, action).await {
			Ok(body) => return Ok(body),
			Err(err) => {
				let message = format!("{err:#}");
				if !message.contains("actor_ready_timeout")
					&& !message.contains("Service Unavailable")
					&& !message.contains("error sending request")
					&& !message.contains("connection closed")
					&& !message.contains("Connection reset")
					&& !message.contains("dropped_reply")
					&& !message.contains("Actor is not ready")
				{
					return Err(err).context("actor action returned non-readiness error");
				}
				last_error = Some(err);
				tokio::time::sleep(Duration::from_millis(250)).await;
			}
		}
	}

	match last_error {
		Some(err) => Err(err).context("timed out waiting for actor action"),
		None => bail!("timed out waiting for actor action"),
	}
}

async fn send_json_action_direct(
	client: &reqwest::Client,
	endpoint: &str,
	actor_id: &str,
	action: &str,
) -> Result<String> {
	let response = client
		.post(format!("{endpoint}/gateway/{actor_id}/action/{action}"))
		.header("x-rivet-encoding", "json")
		.header("content-type", "application/json")
		.body(r#"{"args":[]}"#)
		.send()
		.await
		.context("send actor action")?;
	let status = response.status();
	let body = response
		.text()
		.await
		.context("read actor action response")?;
	if !status.is_success() {
		bail!("actor action failed with {status}: {body}");
	}
	Ok(body)
}

fn is_actor_unavailable(err: &anyhow::Error) -> bool {
	let message = format!("{err:#}");
	message.contains("actor_ready_timeout")
		|| message.contains("Service Unavailable")
		|| message.contains("error sending request")
		|| message.contains("connection closed")
		|| message.contains("Connection reset")
		|| message.contains("dropped_reply")
		|| message.contains("Actor is not ready")
}

fn sqlite_fuzz_registry() -> CoreRegistry {
	let mut registry = CoreRegistry::new();
	registry.register(ACTOR_NAME, sqlite_fuzz_factory());
	registry
}

fn sqlite_fuzz_factory() -> ActorFactory {
	let config = ActorConfig {
		has_database: true,
		remote_sqlite: false,
		sleep_timeout: Duration::from_millis(100),
		sleep_grace_period: Duration::from_millis(500),
		sleep_grace_period_overridden: true,
		..ActorConfig::default()
	};
	let save_every_steps = env_usize(
		"SQLITE_CORRUPTION_FUZZ_SAVE_EVERY_STEPS",
		DEFAULT_SAVE_EVERY_STEPS,
	) as i64;
	ActorFactory::new(config, move |start| {
		Box::pin(async move {
			let ctx = start.ctx;
			let mut step = read_step(&ctx.state());
			let mut events = start.events;
			while let Some(event) = events.recv().await {
				match event {
					ActorEvent::Action {
						name,
						args: _,
						conn: _,
						reply,
						..
					} => match name.as_str() {
						"step" => {
							step += 1;
							let result = run_sqlite_step(&ctx, step).await;
							if result.is_ok()
								&& save_every_steps != 0 && step % save_every_steps == 0
							{
								ctx.request_save(RequestSaveOpts::default());
							}
							reply.send(result.map(|summary| encode_json(&summary)));
						}
						"check" => {
							reply.send(
								run_sqlite_check(&ctx)
									.await
									.map(|summary| encode_json(&summary)),
							);
						}
						name => {
							reply.send(Err(anyhow::anyhow!("unknown action `{name}`")));
						}
					},
					ActorEvent::SerializeState { reason, reply } => match reason {
						SerializeStateReason::Save | SerializeStateReason::Inspector => {
							reply.send(Ok(vec![StateDelta::ActorState(encode_json(&json!({
								"step": step,
							})))]));
						}
					},
					ActorEvent::RunGracefulCleanup { reason: _, reply } => {
						reply.send(Ok(()));
					}
					ActorEvent::HttpRequest { request: _, reply } => {
						reply.send(Err(anyhow::anyhow!("http requests are not handled")));
					}
					ActorEvent::QueueSend {
						name: _,
						body: _,
						conn: _,
						request: _,
						wait: _,
						timeout_ms: _,
						reply,
					} => {
						reply.send(Err(anyhow::anyhow!("queue sends are not handled")));
					}
					ActorEvent::WebSocketOpen {
						ws: _,
						conn: _,
						request: _,
						reply,
					} => {
						reply.send(Err(anyhow::anyhow!("websockets are not handled")));
					}
					ActorEvent::ConnectionPreflight {
						conn: _,
						params: _,
						request: _,
						reply,
					} => {
						reply.send(Ok(()));
					}
					ActorEvent::ConnectionOpen { reply, .. } => {
						reply.send(Ok(()));
					}
					ActorEvent::ConnectionClosed { conn: _ } => {}
					ActorEvent::SubscribeRequest {
						conn: _,
						event_name: _,
						reply,
					} => {
						reply.send(Err(anyhow::anyhow!("subscriptions are not handled")));
					}
					ActorEvent::DisconnectConn { conn_id: _, reply } => {
						reply.send(Ok(()));
					}
					ActorEvent::WorkflowHistoryRequested { reply } => {
						reply.send(Ok(None));
					}
					ActorEvent::WorkflowReplayRequested { entry_id: _, reply } => {
						reply.send(Ok(None));
					}
					ActorEvent::RunWake { .. } => {}
				}
			}

			Ok(())
		})
	})
}

async fn run_sqlite_step(ctx: &rivetkit_core::ActorContext, step: i64) -> Result<JsonValue> {
	let plan = StepPlan::new(ctx.actor_id(), step);
	ensure_schema(ctx).await?;

	if plan.schema_churn {
		ctx.db_run("DROP INDEX IF EXISTS idx_fuzz_rows_note", None)
			.await?;
		ctx.db_run(
			"CREATE INDEX IF NOT EXISTS idx_fuzz_rows_note
				ON fuzz_rows(note, id)",
			None,
		)
		.await?;
	}

	ctx.db_run("BEGIN IMMEDIATE", None).await?;
	let transaction_result = run_write_transaction(ctx, step, &plan).await;
	if transaction_result.is_err() {
		let _ = ctx.db_run("ROLLBACK", None).await;
	}
	transaction_result?;
	ctx.db_run("COMMIT", None).await?;

	if plan.reindex {
		ctx.db_run("REINDEX", None).await?;
	}
	if plan.analyze {
		ctx.db_run("ANALYZE", None).await?;
	}
	if plan.vacuum {
		ctx.db_run("VACUUM", None).await?;
	}

	run_read_checks(ctx, step, &plan).await?;

	let integrity = assert_pragma_ok(ctx, "PRAGMA integrity_check", "integrity_check").await?;
	let quick_check = assert_pragma_ok(ctx, "PRAGMA quick_check", "quick_check").await?;
	if plan.close_after {
		ctx.sql().close().await?;
	}
	if plan.sleep_after {
		ctx.sleep()?;
	}

	Ok(json!({
		"step": step,
		"op": plan.op,
		"row_id": plan.row_id,
		"payload_size": plan.payload_size,
		"integrity": integrity,
		"quick_check": quick_check,
		"sleep_after": plan.sleep_after,
	}))
}

async fn run_sqlite_check(ctx: &rivetkit_core::ActorContext) -> Result<JsonValue> {
	let integrity = assert_pragma_ok(ctx, "PRAGMA integrity_check", "integrity_check").await?;
	let quick_check = assert_pragma_ok(ctx, "PRAGMA quick_check", "quick_check").await?;
	ctx.sql().close().await?;
	Ok(json!({
		"integrity": integrity,
		"quick_check": quick_check,
	}))
}

async fn ensure_schema(ctx: &rivetkit_core::ActorContext) -> Result<()> {
	ctx.db_run(
		"CREATE TABLE IF NOT EXISTS fuzz_rows (
			id INTEGER PRIMARY KEY,
			bucket INTEGER NOT NULL,
			revision INTEGER NOT NULL,
			payload BLOB NOT NULL,
			note TEXT NOT NULL
		)",
		None,
	)
	.await?;
	ctx.db_run(
		"CREATE TABLE IF NOT EXISTS fuzz_log (
			step INTEGER PRIMARY KEY,
			op INTEGER NOT NULL,
			row_id INTEGER NOT NULL,
			payload_size INTEGER NOT NULL
		)",
		None,
	)
	.await?;
	ctx.db_run(
		"CREATE INDEX IF NOT EXISTS idx_fuzz_rows_bucket_payload
			ON fuzz_rows(bucket, note)",
		None,
	)
	.await?;
	ctx.db_run(
		"CREATE INDEX IF NOT EXISTS idx_fuzz_rows_revision
			ON fuzz_rows(revision)",
		None,
	)
	.await?;
	Ok(())
}

async fn run_write_transaction(
	ctx: &rivetkit_core::ActorContext,
	step: i64,
	plan: &StepPlan,
) -> Result<()> {
	for offset in 0..plan.burst_rows {
		let row_id = (plan.row_id + offset as i64 * 17) % 4099;
		let bucket = (plan.bucket + offset as i64) % 31;
		let payload_size = plan.payload_size + offset as i64 * 97;
		ctx.db_run(
			&format!(
				"INSERT INTO fuzz_rows (id, bucket, revision, payload, note)
					VALUES ({row_id}, {bucket}, {step}, zeroblob({payload_size}), 'note-{step}-{row_id}')
					ON CONFLICT(id) DO UPDATE SET
						bucket = excluded.bucket,
						revision = excluded.revision,
						payload = excluded.payload,
						note = excluded.note"
			),
			None,
		)
		.await?;
	}

	match plan.op {
		0 => {
			ctx.db_run(
				&format!(
					"UPDATE fuzz_rows
						SET payload = zeroblob({}), revision = {step}, note = note || ':u{step}'
						WHERE id % 23 = {}",
					plan.payload_size / 2 + 64,
					step % 23
				),
				None,
			)
			.await?;
		}
		1 => {
			ctx.db_run(
				&format!(
					"DELETE FROM fuzz_rows
						WHERE id % 37 = {} AND id <> {}",
					step % 37,
					plan.row_id
				),
				None,
			)
			.await?;
		}
		2 => {
			ctx.db_run(
				&format!(
					"INSERT INTO fuzz_rows (id, bucket, revision, payload, note)
						SELECT id + 5000, bucket, {step}, zeroblob({}), note || ':copy'
						FROM fuzz_rows
						WHERE bucket = {}
						ORDER BY id
						LIMIT 3
						ON CONFLICT(id) DO UPDATE SET
							revision = excluded.revision,
							payload = excluded.payload,
							note = excluded.note",
					plan.payload_size + 128,
					plan.bucket
				),
				None,
			)
			.await?;
		}
		3 => {
			ctx.db_run(
				&format!(
					"UPDATE fuzz_rows
						SET bucket = (bucket + 7) % 31, revision = {step}
						WHERE id BETWEEN {} AND {}",
					plan.row_id.saturating_sub(11),
					plan.row_id + 11
				),
				None,
			)
			.await?;
		}
		4 => {
			ctx.db_execute(
				&format!(
					"SELECT id, note FROM fuzz_rows
						WHERE bucket = {}
						ORDER BY revision DESC, id
						LIMIT 6",
					plan.bucket
				),
				None,
			)
			.await?;
		}
		5 => {
			ctx.db_run(
				&format!(
					"UPDATE fuzz_rows
						SET payload = zeroblob({}), revision = {step}
						WHERE bucket BETWEEN {} AND {}",
					plan.payload_size + 256,
					plan.bucket.saturating_sub(1),
					plan.bucket + 1
				),
				None,
			)
			.await?;
		}
		_ => unreachable!("plan op should be modulo 6"),
	}

	ctx.db_run(
		&format!(
			"INSERT INTO fuzz_log (step, op, row_id, payload_size)
				VALUES ({step}, {}, {}, {})
				ON CONFLICT(step) DO UPDATE SET
					op = excluded.op,
					row_id = excluded.row_id,
					payload_size = excluded.payload_size",
			plan.op, plan.row_id, plan.payload_size
		),
		None,
	)
	.await?;

	Ok(())
}

async fn run_read_checks(
	ctx: &rivetkit_core::ActorContext,
	step: i64,
	plan: &StepPlan,
) -> Result<()> {
	ctx.db_query(
		&format!(
			"SELECT COUNT(*) AS count FROM fuzz_rows WHERE bucket = {}",
			plan.bucket
		),
		None,
	)
	.await?;
	ctx.db_query(
		&format!(
			"SELECT id, note FROM fuzz_rows INDEXED BY idx_fuzz_rows_bucket_payload
				WHERE bucket BETWEEN {} AND {}
				ORDER BY note, id
				LIMIT 16",
			plan.bucket.saturating_sub(2),
			plan.bucket + 2
		),
		None,
	)
	.await?;
	ctx.db_query(
		&format!(
			"SELECT id, length(payload) AS payload_len
				FROM fuzz_rows
				WHERE revision BETWEEN {} AND {}
				ORDER BY revision DESC, id
				LIMIT 12",
			step.saturating_sub(20),
			step + 20
		),
		None,
	)
	.await?;
	ctx.db_query(
		"SELECT l.step, r.id
			FROM fuzz_log l
			LEFT JOIN fuzz_rows r ON r.id = l.row_id
			ORDER BY l.step DESC
			LIMIT 8",
		None,
	)
	.await?;
	Ok(())
}

async fn assert_pragma_ok(
	ctx: &rivetkit_core::ActorContext,
	sql: &str,
	column: &str,
) -> Result<&'static str> {
	let rows = ctx.db_query(sql, None).await?;
	let value: JsonValue =
		ciborium::from_reader(Cursor::new(rows)).context("decode pragma rows from cbor")?;
	let status = value
		.as_array()
		.and_then(|rows| rows.first())
		.and_then(|row| row.get(column))
		.and_then(JsonValue::as_str)
		.with_context(|| format!("read {column} status"))?;
	if status != "ok" {
		bail!("{sql} failed: {status}");
	}
	Ok("ok")
}

struct StepPlan {
	op: u64,
	row_id: i64,
	bucket: i64,
	payload_size: i64,
	burst_rows: usize,
	schema_churn: bool,
	reindex: bool,
	analyze: bool,
	vacuum: bool,
	close_after: bool,
	sleep_after: bool,
}

impl StepPlan {
	fn new(actor_id: &str, step: i64) -> Self {
		let seed = hash64(actor_id.as_bytes()) ^ ((step as u64).wrapping_mul(0x9e3779b97f4a7c15));
		let mixed = mix64(seed);
		let payload_multiplier = env_usize(
			"SQLITE_CORRUPTION_FUZZ_PAYLOAD_MULTIPLIER",
			DEFAULT_PAYLOAD_MULTIPLIER,
		)
		.max(1) as i64;
		let payload_size = (256 + (mixed % 16_384) as i64) * payload_multiplier;

		Self {
			op: mixed % 6,
			row_id: ((mixed >> 8) % 4099) as i64,
			bucket: ((mixed >> 19) % 31) as i64,
			payload_size,
			burst_rows: 1 + ((mixed >> 27) % 5) as usize,
			schema_churn: step % 11 == 0,
			reindex: step % 17 == 0,
			analyze: step % 19 == 0,
			vacuum: step % 37 == 0,
			close_after: step % 3 == 0 || mixed & 0x20 != 0,
			sleep_after: step % 13 == 0,
		}
	}
}

fn env_usize(name: &str, default: usize) -> usize {
	env::var(name)
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(default)
}

fn fuzz_run_id() -> String {
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis();
	format!("{}-{now}", std::process::id())
}

fn hash64(bytes: &[u8]) -> u64 {
	let mut hash = 0xcbf29ce484222325;
	for byte in bytes {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x100000001b3);
	}
	hash
}

fn mix64(mut value: u64) -> u64 {
	value ^= value >> 30;
	value = value.wrapping_mul(0xbf58476d1ce4e5b9);
	value ^= value >> 27;
	value = value.wrapping_mul(0x94d049bb133111eb);
	value ^ (value >> 31)
}

fn action_output(body: &str) -> Result<JsonValue> {
	let value: JsonValue = serde_json::from_str(body).context("decode action response")?;
	Ok(value.get("output").cloned().unwrap_or(JsonValue::Null))
}

fn read_step(state: &[u8]) -> i64 {
	if state.is_empty() {
		return 0;
	}

	let value: JsonValue = ciborium::from_reader(Cursor::new(state)).unwrap_or(JsonValue::Null);
	value
		.get("step")
		.and_then(JsonValue::as_i64)
		.unwrap_or_default()
}

fn encode_json(value: &JsonValue) -> Vec<u8> {
	let mut out = Vec::new();
	ciborium::into_writer(value, &mut out).expect("encode cbor json");
	out
}
