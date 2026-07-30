use rivet_envoy_protocol as protocol;
use std::collections::HashMap;

use crate::utils::id_to_str;

fn stringify_bytes(data: &[u8]) -> String {
	format!("Bytes({})", data.len())
}

fn stringify_map(map: &HashMap<String, String>) -> String {
	let entries: Vec<String> = map
		.iter()
		.map(|(k, v)| format!("\"{k}\": \"{v}\""))
		.collect();
	format!("Map({}){{{}}}", map.len(), entries.join(", "))
}

fn stringify_message_id(msg_id: &protocol::MessageId) -> String {
	format!(
		"MessageId{{gatewayId: {}, requestId: {}, messageIndex: {}}}",
		id_to_str(&msg_id.gateway_id),
		id_to_str(&msg_id.request_id),
		msg_id.message_index
	)
}

pub fn stringify_to_rivet_tunnel_message_kind(kind: &protocol::ToRivetTunnelMessageKind) -> String {
	match kind {
		protocol::ToRivetTunnelMessageKind::ToRivetResponseStart(val) => {
			let body_str = match &val.body {
				Some(b) => stringify_bytes(b),
				None => "null".to_string(),
			};
			format!(
				"ToRivetResponseStart{{status: {}, headers: {}, body: {}, stream: {}}}",
				val.status,
				stringify_map(&val.headers),
				body_str,
				val.stream
			)
		}
		protocol::ToRivetTunnelMessageKind::ToRivetResponseChunk(val) => {
			format!(
				"ToRivetResponseChunk{{body: {}, finish: {}}}",
				stringify_bytes(&val.body),
				val.finish
			)
		}
		protocol::ToRivetTunnelMessageKind::ToRivetResponseAbort(_) => {
			"ToRivetResponseAbort".to_string()
		}
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketOpen(val) => {
			format!(
				"ToRivetWebSocketOpen{{canHibernate: {}}}",
				val.can_hibernate
			)
		}
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessage(val) => {
			format!(
				"ToRivetWebSocketMessage{{data: {}, binary: {}}}",
				stringify_bytes(&val.data),
				val.binary
			)
		}
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketMessageAck(val) => {
			format!("ToRivetWebSocketMessageAck{{index: {}}}", val.index)
		}
		protocol::ToRivetTunnelMessageKind::ToRivetWebSocketClose(val) => {
			let code_str = match &val.code {
				Some(c) => c.to_string(),
				None => "null".to_string(),
			};
			let reason_str = match &val.reason {
				Some(r) => format!("\"{r}\""),
				None => "null".to_string(),
			};
			format!(
				"ToRivetWebSocketClose{{code: {code_str}, reason: {reason_str}, hibernate: {}}}",
				val.hibernate
			)
		}
	}
}

pub fn stringify_to_envoy_tunnel_message_kind(kind: &protocol::ToEnvoyTunnelMessageKind) -> String {
	match kind {
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestStart(val) => {
			let body_str = match &val.body {
				Some(b) => stringify_bytes(b),
				None => "null".to_string(),
			};
			format!(
				"ToEnvoyRequestStart{{actorId: \"{}\", method: \"{}\", path: \"{}\", headers: {}, body: {}, stream: {}}}",
				val.actor_id,
				val.method,
				val.path,
				stringify_map(&val.headers),
				body_str,
				val.stream
			)
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestChunk(val) => {
			format!(
				"ToEnvoyRequestChunk{{body: {}, finish: {}}}",
				stringify_bytes(&val.body),
				val.finish
			)
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyRequestAbort(_) => {
			"ToEnvoyRequestAbort".to_string()
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(val) => {
			format!(
				"ToEnvoyWebSocketOpen{{actorId: \"{}\", path: \"{}\", headers: {}}}",
				val.actor_id,
				val.path,
				stringify_map(&val.headers)
			)
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(val) => {
			format!(
				"ToEnvoyWebSocketMessage{{data: {}, binary: {}}}",
				stringify_bytes(&val.data),
				val.binary
			)
		}
		protocol::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketClose(val) => {
			let code_str = match &val.code {
				Some(c) => c.to_string(),
				None => "null".to_string(),
			};
			let reason_str = match &val.reason {
				Some(r) => format!("\"{r}\""),
				None => "null".to_string(),
			};
			format!("ToEnvoyWebSocketClose{{code: {code_str}, reason: {reason_str}}}")
		}
	}
}

pub fn stringify_command(command: &protocol::Command) -> String {
	match command {
		protocol::Command::CommandStartActor(val) => {
			let key_str = match &val.config.key {
				Some(k) => format!("\"{k}\""),
				None => "null".to_string(),
			};
			let input_str = match &val.config.input {
				Some(i) => stringify_bytes(i),
				None => "null".to_string(),
			};
			let hib_str = if val.hibernating_requests.is_empty() {
				"[]".to_string()
			} else {
				let entries: Vec<String> = val
					.hibernating_requests
					.iter()
					.map(|hr| {
						format!(
							"{{gatewayId: {}, requestId: {}}}",
							id_to_str(&hr.gateway_id),
							id_to_str(&hr.request_id)
						)
					})
					.collect();
				format!("[{}]", entries.join(", "))
			};
			format!(
				"CommandStartActor{{config: {{name: \"{}\", key: {key_str}, createTs: {}, input: {input_str}}}, hibernatingRequests: {hib_str}}}",
				val.config.name, val.config.create_ts
			)
		}
		protocol::Command::CommandStopActor(val) => {
			format!("CommandStopActor{{reason: {:?}}}", val.reason)
		}
	}
}

pub fn stringify_command_wrapper(wrapper: &protocol::CommandWrapper) -> String {
	format!(
		"CommandWrapper{{actorId: \"{}\", generation: {}, index: {}, inner: {}}}",
		wrapper.checkpoint.actor_id,
		wrapper.checkpoint.generation,
		wrapper.checkpoint.index,
		stringify_command(&wrapper.inner)
	)
}

pub fn stringify_event(event: &protocol::Event) -> String {
	match event {
		protocol::Event::EventActorIntent(val) => {
			let intent_str = match &val.intent {
				protocol::ActorIntent::ActorIntentSleep => "Sleep",
				protocol::ActorIntent::ActorIntentStop => "Stop",
			};
			format!("EventActorIntent{{intent: {intent_str}}}")
		}
		protocol::Event::EventActorStateUpdate(val) => {
			let state_str = match &val.state {
				protocol::ActorState::ActorStateRunning => "Running".to_string(),
				protocol::ActorState::ActorStateStopped(stopped) => {
					let message_str = match &stopped.message {
						Some(m) => format!("\"{m}\""),
						None => "null".to_string(),
					};
					format!(
						"Stopped{{code: {:?}, message: {message_str}}}",
						stopped.code
					)
				}
			};
			format!("EventActorStateUpdate{{state: {state_str}}}")
		}
		protocol::Event::EventActorSetAlarm(val) => {
			let alarm_str = match val.alarm_ts {
				Some(ts) => ts.to_string(),
				None => "null".to_string(),
			};
			format!("EventActorSetAlarm{{alarmTs: {alarm_str}}}")
		}
	}
}

pub fn stringify_event_wrapper(wrapper: &protocol::EventWrapper) -> String {
	format!(
		"EventWrapper{{actorId: {}, generation: {}, index: {}, inner: {}}}",
		wrapper.checkpoint.actor_id,
		wrapper.checkpoint.generation,
		wrapper.checkpoint.index,
		stringify_event(&wrapper.inner)
	)
}

pub fn stringify_to_rivet(message: &protocol::ToRivet) -> String {
	match message {
		protocol::ToRivet::ToRivetMetadata(_) => "ToRivetMetadata".to_string(),
		protocol::ToRivet::ToRivetEvents(events) => {
			let event_strs: Vec<String> = events.iter().map(stringify_event_wrapper).collect();
			format!(
				"ToRivetEvents{{count: {}, events: [{}]}}",
				events.len(),
				event_strs.join(", ")
			)
		}
		protocol::ToRivet::ToRivetAckCommands(val) => {
			let checkpoints: Vec<String> = val
				.last_command_checkpoints
				.iter()
				.map(|cp| format!("{{actorId: \"{}\", index: {}}}", cp.actor_id, cp.index))
				.collect();
			format!(
				"ToRivetAckCommands{{lastCommandCheckpoints: [{}]}}",
				checkpoints.join(", ")
			)
		}
		protocol::ToRivet::ToRivetStopping => "ToRivetStopping".to_string(),
		protocol::ToRivet::ToRivetPong(val) => {
			format!("ToRivetPong{{ts: {}}}", val.ts)
		}
		protocol::ToRivet::ToRivetKvRequest(val) => {
			format!(
				"ToRivetKvRequest{{actorId: \"{}\", requestId: {}}}",
				val.actor_id, val.request_id
			)
		}
		protocol::ToRivet::ToRivetSqliteGetPagesRequest(val) => {
			format!(
				"ToRivetSqliteGetPagesRequest{{requestId: {}}}",
				val.request_id
			)
		}
		protocol::ToRivet::ToRivetSqliteCommitRequest(val) => {
			format!(
				"ToRivetSqliteCommitRequest{{requestId: {}}}",
				val.request_id
			)
		}
		protocol::ToRivet::ToRivetSqliteExecRequest(val) => {
			format!(
				"ToRivetSqliteExecRequest{{requestId: {}, actorId: \"{}\", generation: {}}}",
				val.request_id, val.data.actor_id, val.data.generation
			)
		}
		protocol::ToRivet::ToRivetSqliteExecuteRequest(val) => {
			format!(
				"ToRivetSqliteExecuteRequest{{requestId: {}, actorId: \"{}\", generation: {}}}",
				val.request_id, val.data.actor_id, val.data.generation
			)
		}
		protocol::ToRivet::ToRivetSqliteExecuteBatchRequest(val) => {
			format!(
				"ToRivetSqliteExecuteBatchRequest{{requestId: {}, actorId: \"{}\", generation: {}, statements: {}}}",
				val.request_id,
				val.data.actor_id,
				val.data.generation,
				val.data.statements.len()
			)
		}
		protocol::ToRivet::ToRivetTunnelMessage(val) => {
			format!(
				"ToRivetTunnelMessage{{messageId: {}, messageKind: {}}}",
				stringify_message_id(&val.message_id),
				stringify_to_rivet_tunnel_message_kind(&val.message_kind)
			)
		}
	}
}

pub fn stringify_to_envoy(message: &protocol::ToEnvoy) -> String {
	match message {
		protocol::ToEnvoy::ToEnvoyInit(val) => {
			format!(
				"ToEnvoyInit{{metadata: {{envoyLostThreshold: {}, actorStopThreshold: {}}}}}",
				val.metadata.envoy_lost_threshold, val.metadata.actor_stop_threshold
			)
		}
		protocol::ToEnvoy::ToEnvoyCommands(commands) => {
			let cmd_strs: Vec<String> = commands.iter().map(stringify_command_wrapper).collect();
			format!(
				"ToEnvoyCommands{{count: {}, commands: [{}]}}",
				commands.len(),
				cmd_strs.join(", ")
			)
		}
		protocol::ToEnvoy::ToEnvoyAckEvents(val) => {
			let checkpoints: Vec<String> = val
				.last_event_checkpoints
				.iter()
				.map(|cp| format!("{{actorId: \"{}\", index: {}}}", cp.actor_id, cp.index))
				.collect();
			format!(
				"ToEnvoyAckEvents{{lastEventCheckpoints: [{}]}}",
				checkpoints.join(", ")
			)
		}
		protocol::ToEnvoy::ToEnvoyKvResponse(val) => {
			format!("ToEnvoyKvResponse{{requestId: {}}}", val.request_id)
		}
		protocol::ToEnvoy::ToEnvoySqliteGetPagesResponse(val) => {
			format!(
				"ToEnvoySqliteGetPagesResponse{{requestId: {}}}",
				val.request_id
			)
		}
		protocol::ToEnvoy::ToEnvoySqliteCommitResponse(val) => {
			format!(
				"ToEnvoySqliteCommitResponse{{requestId: {}}}",
				val.request_id
			)
		}
		protocol::ToEnvoy::ToEnvoySqliteExecResponse(val) => {
			format!("ToEnvoySqliteExecResponse{{requestId: {}}}", val.request_id)
		}
		protocol::ToEnvoy::ToEnvoySqliteExecuteResponse(val) => {
			format!(
				"ToEnvoySqliteExecuteResponse{{requestId: {}}}",
				val.request_id
			)
		}
		protocol::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(val) => {
			format!(
				"ToEnvoySqliteExecuteBatchResponse{{requestId: {}}}",
				val.request_id
			)
		}
		protocol::ToEnvoy::ToEnvoyTunnelMessage(val) => {
			format!(
				"ToEnvoyTunnelMessage{{messageId: {}, messageKind: {}}}",
				stringify_message_id(&val.message_id),
				stringify_to_envoy_tunnel_message_kind(&val.message_kind)
			)
		}
		protocol::ToEnvoy::ToEnvoyPing(val) => {
			format!("ToEnvoyPing{{ts: {}}}", val.ts)
		}
	}
}
