use std::collections::HashMap;

use rivet_envoy_protocol as protocol;

use crate::actor::create_actor;
use crate::connection::ws_send;
use crate::envoy::EnvoyContext;
use crate::stringify::stringify_command_wrapper;

pub const ACK_COMMANDS_INTERVAL_MS: u64 = 5 * 60 * 1000;

pub async fn handle_commands(ctx: &mut EnvoyContext, commands: Vec<protocol::CommandWrapper>) {
	tracing::info!(command_count = commands.len(), "received commands");
	for command_wrapper in &commands {
		tracing::info!(
			command = %stringify_command_wrapper(command_wrapper),
			"received command"
		);
	}

	// Collect actors with a stop in the raw batch before dedup, so a replayed
	// (skipped) stop is still re-acked instead of being replayed forever.
	let stopped_actors: Vec<(String, u32)> = commands
		.iter()
		.filter(|c| matches!(c.inner, protocol::Command::CommandStopActor(_)))
		.map(|c| (c.checkpoint.actor_id.clone(), c.checkpoint.generation))
		.collect();

	for command_wrapper in commands {
		let checkpoint = command_wrapper.checkpoint;
		let dedup_key = (checkpoint.actor_id.clone(), checkpoint.generation);

		// Drop replayed commands. `pegboard-envoy` re-streams every unacked
		// command on reconnect, and the command index is monotonic per
		// `(actor_id, generation)`, so any index at or below the highest one
		// we have already processed is a duplicate.
		if let Some(&last_idx) = ctx.processed_command_idx.get(&dedup_key) {
			if checkpoint.index <= last_idx {
				tracing::debug!(
					actor_id = %checkpoint.actor_id,
					generation = checkpoint.generation,
					index = checkpoint.index,
					last_idx,
					"skipping replayed command"
				);
				continue;
			}
		}
		ctx.processed_command_idx
			.insert(dedup_key, checkpoint.index);

		match command_wrapper.inner {
			protocol::Command::CommandStartActor(val) => {
				let actor_name = val.config.name.clone();
				let (handle, active_http_request_count) = create_actor(
					ctx.shared.clone(),
					checkpoint.actor_id.clone(),
					checkpoint.generation,
					val.config,
					val.hibernating_requests,
					val.preloaded_kv,
				);

				ctx.insert_actor(
					checkpoint.actor_id.clone(),
					checkpoint.generation,
					handle,
					active_http_request_count,
					actor_name,
					checkpoint.index,
				);
			}
			protocol::Command::CommandStopActor(val) => {
				let entry = ctx.get_actor_entry_mut(&checkpoint.actor_id, checkpoint.generation);

				if let Some(entry) = entry {
					entry.received_stop = true;
					entry.last_command_idx = checkpoint.index;
					let _ = entry.handle.send(crate::actor::ToActor::Stop {
						command_idx: checkpoint.index,
						reason: val.reason,
					});
				} else {
					tracing::warn!(
						actor_id = %checkpoint.actor_id,
						generation = checkpoint.generation,
						"received stop actor command for unknown actor"
					);
				}
			}
		}
	}

	// Ack stops immediately since their actors are removed before the periodic
	// tick. Scope to just the stopped actors instead of a full-state ack, and do
	// not clear dedup; the tick handles full re-acks, recovery, and clearing.
	if !stopped_actors.is_empty() {
		send_stop_command_acks(ctx, &stopped_actors).await;
	}
}

/// Ack only the given actors' latest processed command index. Used for the
/// immediate stop ack. Does not clear dedup (see the race note in
/// `send_command_ack`); a failed send is retried by the replayed batch or tick.
async fn send_stop_command_acks(ctx: &EnvoyContext, actors: &[(String, u32)]) {
	let mut highest: HashMap<(String, u32), i64> = HashMap::new();
	for key in actors {
		if let Some(&index) = ctx.processed_command_idx.get(key) {
			highest.insert(key.clone(), index);
		}
	}

	if highest.is_empty() {
		return;
	}

	send_ack_checkpoints(ctx, checkpoints_from(highest)).await;
}

pub async fn send_command_ack(ctx: &mut EnvoyContext) {
	// Merge live actors and the dedup map, highest index per actor-generation.
	// Live actors are re-acked every tick (recovers an ack accepted locally but
	// never committed by the server); the dedup map covers stops whose actor was
	// already removed and is cleared once acked.
	let mut highest: HashMap<(String, u32), i64> = HashMap::new();
	for (actor_id, generations) in &ctx.actors {
		for (generation, entry) in generations {
			if entry.last_command_idx >= 0 {
				highest.insert((actor_id.clone(), *generation), entry.last_command_idx);
			}
		}
	}
	for ((actor_id, generation), &index) in &ctx.processed_command_idx {
		highest
			.entry((actor_id.clone(), *generation))
			.and_modify(|existing| *existing = (*existing).max(index))
			.or_insert(index);
	}

	if highest.is_empty() {
		return;
	}

	let last_command_checkpoints = checkpoints_from(highest);
	let send_failed = send_ack_checkpoints(ctx, last_command_checkpoints.clone()).await;

	// Skip the dedup clear if the ack never left this process. Otherwise
	// `pegboard-envoy` would replay the commands on reconnect with no dedup
	// state to suppress them.
	if send_failed {
		return;
	}

	// TODO: Race condition. We clear `processed_command_idx` as soon as the
	// ack bytes leave this process, not when `pegboard-envoy` actually
	// commits the matching `clear_range` over `ActorCommandKey` entries. If
	// the WS drops between `ws_send` returning and the server applying the
	// ack, on reconnect `pegboard-envoy` will replay these commands and the
	// dedup map will no longer be populated to drop them, allowing a
	// stopped actor to be resurrected or a live actor to be replaced. The
	// window is narrow (the gap between OS-accepted bytes and the FDB
	// commit), but a strictly correct fix needs an ack-of-ack from
	// `pegboard-envoy` so we only clear after positive confirmation.
	// This now also applies to removed actors whose stops are acked here: a
	// short-lived actor can be resurrected in the same window. Same fix.
	for cp in &last_command_checkpoints {
		ctx.processed_command_idx
			.remove(&(cp.actor_id.clone(), cp.generation));
	}
}

fn checkpoints_from(highest: HashMap<(String, u32), i64>) -> Vec<protocol::ActorCheckpoint> {
	highest
		.into_iter()
		.map(
			|((actor_id, generation), index)| protocol::ActorCheckpoint {
				actor_id,
				generation,
				index,
			},
		)
		.collect()
}

/// Send an ack for the given checkpoints. Returns whether the send failed.
async fn send_ack_checkpoints(
	ctx: &EnvoyContext,
	last_command_checkpoints: Vec<protocol::ActorCheckpoint>,
) -> bool {
	ws_send(
		&ctx.shared,
		protocol::ToRivet::ToRivetAckCommands(protocol::ToRivetAckCommands {
			last_command_checkpoints,
		}),
	)
	.await
}
