use anyhow::{Ok, Result, bail};
use vbare::OwnedVersionedData;

use crate::PROTOCOL_MK1_VERSION;
use crate::generated::{v1, v2, v3, v4, v5, v6, v7};
use crate::uuid_compat::{decode_bytes_from_uuid, encode_bytes_to_uuid};

pub enum ToClientMk2 {
	V4(v4::ToClient),
	V5(v5::ToClient),
	V7(v7::ToClient),
}

impl OwnedVersionedData for ToClientMk2 {
	type Latest = v7::ToClient;

	fn wrap_latest(latest: v7::ToClient) -> Self {
		ToClientMk2::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let ToClientMk2::V7(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			4 => Ok(ToClientMk2::V4(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			5 => Ok(ToClientMk2::V5(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			6 | 7 => Ok(ToClientMk2::V7(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToClientMk2::V4(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToClientMk2::V5(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToClientMk2::V7(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Ok, Self::v4_to_v5, Self::v5_to_v7, Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Self::v7_to_v5, Self::v5_to_v4, Ok, Ok, Ok]
	}
}

impl ToClientMk2 {
	fn v4_to_v5(self) -> Result<Self> {
		if let ToClientMk2::V4(x) = self {
			let inner = match x {
				v4::ToClient::ToClientInit(init) => v5::ToClient::ToClientInit(v5::ToClientInit {
					runner_id: init.runner_id,
					metadata: v5::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v4::ToClient::ToClientCommands(commands) => v5::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v5::CommandWrapper {
							checkpoint: v5::ActorCheckpoint {
								actor_id: cmd.checkpoint.actor_id,
								generation: match &cmd.inner {
									v4::Command::CommandStartActor(start) => start.generation,
									v4::Command::CommandStopActor(stop) => stop.generation,
								},
								index: cmd.checkpoint.index,
							},
							inner: match cmd.inner {
								v4::Command::CommandStartActor(start) => {
									v5::Command::CommandStartActor(v5::CommandStartActor {
										config: v5::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: start
											.hibernating_requests
											.into_iter()
											.map(|req| v5::HibernatingRequest {
												gateway_id: req.gateway_id,
												request_id: req.request_id,
											})
											.collect(),
									})
								}
								v4::Command::CommandStopActor(_) => v5::Command::CommandStopActor,
							},
						})
						.collect(),
				),
				v4::ToClient::ToClientAckEvents(ack) => {
					v5::ToClient::ToClientAckEvents(v5::ToClientAckEvents {
						last_event_checkpoints: ack
							.last_event_checkpoints
							.into_iter()
							.map(|cp| v5::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: 0, // Unknown in v4, use default
								index: cp.index,
							})
							.collect(),
					})
				}
				v4::ToClient::ToClientKvResponse(resp) => {
					v5::ToClient::ToClientKvResponse(v5::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v4_to_v5(resp.data),
					})
				}
				v4::ToClient::ToClientTunnelMessage(msg) => {
					v5::ToClient::ToClientTunnelMessage(v5::ToClientTunnelMessage {
						message_id: v5::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v4_to_v5(
							msg.message_kind,
						),
					})
				}
				v4::ToClient::ToClientPing(ping) => {
					v5::ToClient::ToClientPing(v5::ToClientPing { ts: ping.ts })
				}
			};

			Ok(ToClientMk2::V5(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v5_to_v4(self) -> Result<Self> {
		if let ToClientMk2::V5(x) = self {
			let inner = match x {
				v5::ToClient::ToClientInit(init) => v4::ToClient::ToClientInit(v4::ToClientInit {
					runner_id: init.runner_id,
					metadata: v4::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v5::ToClient::ToClientCommands(commands) => v4::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v4::CommandWrapper {
							checkpoint: v4::ActorCheckpoint {
								actor_id: cmd.checkpoint.actor_id,
								index: cmd.checkpoint.index,
							},
							inner: match cmd.inner {
								v5::Command::CommandStartActor(start) => {
									v4::Command::CommandStartActor(v4::CommandStartActor {
										generation: cmd.checkpoint.generation,
										config: v4::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: start
											.hibernating_requests
											.into_iter()
											.map(|req| v4::HibernatingRequest {
												gateway_id: req.gateway_id,
												request_id: req.request_id,
											})
											.collect(),
									})
								}
								v5::Command::CommandStopActor => {
									v4::Command::CommandStopActor(v4::CommandStopActor {
										generation: cmd.checkpoint.generation,
									})
								}
							},
						})
						.collect(),
				),
				v5::ToClient::ToClientAckEvents(ack) => {
					v4::ToClient::ToClientAckEvents(v4::ToClientAckEvents {
						last_event_checkpoints: ack
							.last_event_checkpoints
							.into_iter()
							.map(|cp| v4::ActorCheckpoint {
								actor_id: cp.actor_id,
								index: cp.index,
							})
							.collect(),
					})
				}
				v5::ToClient::ToClientKvResponse(resp) => {
					v4::ToClient::ToClientKvResponse(v4::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v5_to_v4(resp.data),
					})
				}
				v5::ToClient::ToClientTunnelMessage(msg) => {
					v4::ToClient::ToClientTunnelMessage(v4::ToClientTunnelMessage {
						message_id: v4::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v5_to_v4(
							msg.message_kind,
						),
					})
				}
				v5::ToClient::ToClientPing(ping) => {
					v4::ToClient::ToClientPing(v4::ToClientPing { ts: ping.ts })
				}
			};

			Ok(ToClientMk2::V4(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v5_to_v7(self) -> Result<Self> {
		if let ToClientMk2::V5(x) = self {
			let inner = match x {
				v5::ToClient::ToClientInit(init) => v7::ToClient::ToClientInit(v7::ToClientInit {
					runner_id: init.runner_id,
					metadata: v7::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
						actor_stop_threshold: 0,
						serverless_drain_grace_period: None,
					},
				}),
				v5::ToClient::ToClientCommands(commands) => v7::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v7::CommandWrapper {
							checkpoint: v7::ActorCheckpoint {
								actor_id: cmd.checkpoint.actor_id,
								generation: cmd.checkpoint.generation,
								index: cmd.checkpoint.index,
							},
							inner: match cmd.inner {
								v5::Command::CommandStartActor(start) => {
									v7::Command::CommandStartActor(v7::CommandStartActor {
										config: v7::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: start
											.hibernating_requests
											.into_iter()
											.map(|req| v7::HibernatingRequest {
												gateway_id: req.gateway_id,
												request_id: req.request_id,
											})
											.collect(),
									})
								}
								v5::Command::CommandStopActor => v7::Command::CommandStopActor,
							},
						})
						.collect(),
				),
				v5::ToClient::ToClientAckEvents(ack) => {
					v7::ToClient::ToClientAckEvents(v7::ToClientAckEvents {
						last_event_checkpoints: ack
							.last_event_checkpoints
							.into_iter()
							.map(|cp| v7::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: cp.generation,
								index: cp.index,
							})
							.collect(),
					})
				}
				v5::ToClient::ToClientKvResponse(resp) => {
					v7::ToClient::ToClientKvResponse(v7::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v5_to_v7(resp.data),
					})
				}
				v5::ToClient::ToClientTunnelMessage(msg) => {
					v7::ToClient::ToClientTunnelMessage(v7::ToClientTunnelMessage {
						message_id: v7::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v5_to_v7(
							msg.message_kind,
						),
					})
				}
				v5::ToClient::ToClientPing(ping) => {
					v7::ToClient::ToClientPing(v7::ToClientPing { ts: ping.ts })
				}
			};

			Ok(ToClientMk2::V7(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v7_to_v5(self) -> Result<Self> {
		if let ToClientMk2::V7(x) = self {
			let inner = match x {
				v7::ToClient::ToClientInit(init) => v5::ToClient::ToClientInit(v5::ToClientInit {
					runner_id: init.runner_id,
					metadata: v5::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v7::ToClient::ToClientCommands(commands) => v5::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v5::CommandWrapper {
							checkpoint: v5::ActorCheckpoint {
								actor_id: cmd.checkpoint.actor_id,
								generation: cmd.checkpoint.generation,
								index: cmd.checkpoint.index,
							},
							inner: match cmd.inner {
								v7::Command::CommandStartActor(start) => {
									v5::Command::CommandStartActor(v5::CommandStartActor {
										config: v5::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: start
											.hibernating_requests
											.into_iter()
											.map(|req| v5::HibernatingRequest {
												gateway_id: req.gateway_id,
												request_id: req.request_id,
											})
											.collect(),
									})
								}
								v7::Command::CommandStopActor => v5::Command::CommandStopActor,
							},
						})
						.collect(),
				),
				v7::ToClient::ToClientAckEvents(ack) => {
					v5::ToClient::ToClientAckEvents(v5::ToClientAckEvents {
						last_event_checkpoints: ack
							.last_event_checkpoints
							.into_iter()
							.map(|cp| v5::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: cp.generation,
								index: cp.index,
							})
							.collect(),
					})
				}
				v7::ToClient::ToClientKvResponse(resp) => {
					v5::ToClient::ToClientKvResponse(v5::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v7_to_v5(resp.data),
					})
				}
				v7::ToClient::ToClientTunnelMessage(msg) => {
					v5::ToClient::ToClientTunnelMessage(v5::ToClientTunnelMessage {
						message_id: v5::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v7_to_v5(
							msg.message_kind,
						),
					})
				}
				v7::ToClient::ToClientPing(ping) => {
					v5::ToClient::ToClientPing(v5::ToClientPing { ts: ping.ts })
				}
			};

			Ok(ToClientMk2::V5(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ToServerMk2 {
	V4(v4::ToServer),
	V6(v6::ToServer),
	V7(v7::ToServer),
}

impl OwnedVersionedData for ToServerMk2 {
	type Latest = v7::ToServer;

	fn wrap_latest(latest: v7::ToServer) -> Self {
		ToServerMk2::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let ToServerMk2::V7(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			4 => Ok(ToServerMk2::V4(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			// v5 and v6 have the same ToServer binary format
			5 | 6 => Ok(ToServerMk2::V6(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			7 => Ok(ToServerMk2::V7(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToServerMk2::V4(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToServerMk2::V6(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToServerMk2::V7(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		// No changes between v1 and v4, no changes between v5 and v6
		vec![Ok, Ok, Ok, Self::v4_to_v6, Ok, Self::v6_to_v7]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		// No changes between v1 and v4, no changes between v5 and v6
		vec![Self::v7_to_v6, Ok, Self::v6_to_v4, Ok, Ok, Ok]
	}
}

impl ToServerMk2 {
	fn v4_to_v6(self) -> Result<Self> {
		if let ToServerMk2::V4(x) = self {
			let inner = match x {
				v4::ToServer::ToServerInit(init) => v6::ToServer::ToServerInit(v6::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v6::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v4::ToServer::ToServerEvents(events) => v6::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| {
							let generation = match &event.inner {
								v4::Event::EventActorIntent(intent) => intent.generation,
								v4::Event::EventActorStateUpdate(state) => state.generation,
								v4::Event::EventActorSetAlarm(alarm) => alarm.generation,
							};

							v6::EventWrapper {
								checkpoint: v6::ActorCheckpoint {
									actor_id: event.checkpoint.actor_id,
									generation,
									index: event.checkpoint.index,
								},
								inner: match event.inner {
									v4::Event::EventActorIntent(intent) => {
										v6::Event::EventActorIntent(v6::EventActorIntent {
											intent: convert_actor_intent_v4_to_v6(intent.intent),
										})
									}
									v4::Event::EventActorStateUpdate(state) => {
										v6::Event::EventActorStateUpdate(
											v6::EventActorStateUpdate {
												state: convert_actor_state_v4_to_v6(state.state),
											},
										)
									}
									v4::Event::EventActorSetAlarm(alarm) => {
										v6::Event::EventActorSetAlarm(v6::EventActorSetAlarm {
											alarm_ts: alarm.alarm_ts,
										})
									}
								},
							}
						})
						.collect(),
				),
				v4::ToServer::ToServerAckCommands(ack) => {
					v6::ToServer::ToServerAckCommands(v6::ToServerAckCommands {
						last_command_checkpoints: ack
							.last_command_checkpoints
							.into_iter()
							.map(|cp| v6::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: 0, // Unknown in v4, use default
								index: cp.index,
							})
							.collect(),
					})
				}
				v4::ToServer::ToServerStopping => v6::ToServer::ToServerStopping,
				v4::ToServer::ToServerPong(pong) => {
					v6::ToServer::ToServerPong(v6::ToServerPong { ts: pong.ts })
				}
				v4::ToServer::ToServerKvRequest(req) => {
					v6::ToServer::ToServerKvRequest(v6::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v4_to_v6(req.data),
					})
				}
				v4::ToServer::ToServerTunnelMessage(msg) => {
					v6::ToServer::ToServerTunnelMessage(v6::ToServerTunnelMessage {
						message_id: v6::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_server_tunnel_message_kind_v4_to_v6(
							msg.message_kind,
						),
					})
				}
			};

			Ok(ToServerMk2::V6(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v6_to_v4(self) -> Result<Self> {
		if let ToServerMk2::V6(x) = self {
			let inner = match x {
				v6::ToServer::ToServerInit(init) => v4::ToServer::ToServerInit(v4::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v4::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v6::ToServer::ToServerEvents(events) => v4::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v4::EventWrapper {
							checkpoint: v4::ActorCheckpoint {
								actor_id: event.checkpoint.actor_id.clone(),
								index: event.checkpoint.index,
							},
							inner: match event.inner {
								v6::Event::EventActorIntent(intent) => {
									v4::Event::EventActorIntent(v4::EventActorIntent {
										actor_id: event.checkpoint.actor_id,
										generation: event.checkpoint.generation,
										intent: convert_actor_intent_v6_to_v4(intent.intent),
									})
								}
								v6::Event::EventActorStateUpdate(state) => {
									v4::Event::EventActorStateUpdate(v4::EventActorStateUpdate {
										actor_id: event.checkpoint.actor_id,
										generation: event.checkpoint.generation,
										state: convert_actor_state_v6_to_v4(state.state),
									})
								}
								v6::Event::EventActorSetAlarm(alarm) => {
									v4::Event::EventActorSetAlarm(v4::EventActorSetAlarm {
										actor_id: event.checkpoint.actor_id,
										generation: event.checkpoint.generation,
										alarm_ts: alarm.alarm_ts,
									})
								}
							},
						})
						.collect(),
				),
				v6::ToServer::ToServerAckCommands(ack) => {
					v4::ToServer::ToServerAckCommands(v4::ToServerAckCommands {
						last_command_checkpoints: ack
							.last_command_checkpoints
							.into_iter()
							.map(|cp| v4::ActorCheckpoint {
								actor_id: cp.actor_id,
								index: cp.index,
							})
							.collect(),
					})
				}
				v6::ToServer::ToServerStopping => v4::ToServer::ToServerStopping,
				v6::ToServer::ToServerPong(pong) => {
					v4::ToServer::ToServerPong(v4::ToServerPong { ts: pong.ts })
				}
				v6::ToServer::ToServerKvRequest(req) => {
					v4::ToServer::ToServerKvRequest(v4::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v6_to_v4(req.data),
					})
				}
				v6::ToServer::ToServerTunnelMessage(msg) => {
					v4::ToServer::ToServerTunnelMessage(v4::ToServerTunnelMessage {
						message_id: v4::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_server_tunnel_message_kind_v6_to_v4(
							msg.message_kind,
						)?,
					})
				}
			};

			Ok(ToServerMk2::V4(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v6_to_v7(self) -> Result<Self> {
		if let ToServerMk2::V6(x) = self {
			let inner = match x {
				v6::ToServer::ToServerInit(init) => v7::ToServer::ToServerInit(v7::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v7::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v6::ToServer::ToServerEvents(events) => v7::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v7::EventWrapper {
							checkpoint: v7::ActorCheckpoint {
								actor_id: event.checkpoint.actor_id,
								generation: event.checkpoint.generation,
								index: event.checkpoint.index,
							},
							inner: match event.inner {
								v6::Event::EventActorIntent(intent) => {
									v7::Event::EventActorIntent(v7::EventActorIntent {
										intent: match intent.intent {
											v6::ActorIntent::ActorIntentSleep => {
												v7::ActorIntent::ActorIntentSleep
											}
											v6::ActorIntent::ActorIntentStop => {
												v7::ActorIntent::ActorIntentStop
											}
										},
									})
								}
								v6::Event::EventActorStateUpdate(state) => {
									v7::Event::EventActorStateUpdate(v7::EventActorStateUpdate {
										state: match state.state {
											v6::ActorState::ActorStateRunning => {
												v7::ActorState::ActorStateRunning
											}
											v6::ActorState::ActorStateStopped(stopped) => {
												v7::ActorState::ActorStateStopped(
													v7::ActorStateStopped {
														code: match stopped.code {
															v6::StopCode::Ok => v7::StopCode::Ok,
															v6::StopCode::Error => {
																v7::StopCode::Error
															}
														},
														message: stopped.message,
													},
												)
											}
										},
									})
								}
								v6::Event::EventActorSetAlarm(alarm) => {
									v7::Event::EventActorSetAlarm(v7::EventActorSetAlarm {
										alarm_ts: alarm.alarm_ts,
									})
								}
							},
						})
						.collect(),
				),
				v6::ToServer::ToServerAckCommands(ack) => {
					v7::ToServer::ToServerAckCommands(v7::ToServerAckCommands {
						last_command_checkpoints: ack
							.last_command_checkpoints
							.into_iter()
							.map(|cp| v7::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: cp.generation,
								index: cp.index,
							})
							.collect(),
					})
				}
				v6::ToServer::ToServerStopping => v7::ToServer::ToServerStopping,
				v6::ToServer::ToServerPong(pong) => {
					v7::ToServer::ToServerPong(v7::ToServerPong { ts: pong.ts })
				}
				v6::ToServer::ToServerKvRequest(req) => {
					v7::ToServer::ToServerKvRequest(v7::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v6_to_v7(req.data),
					})
				}
				v6::ToServer::ToServerTunnelMessage(msg) => {
					v7::ToServer::ToServerTunnelMessage(v7::ToServerTunnelMessage {
						message_id: v7::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: match msg.message_kind {
							v6::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
								v7::ToServerTunnelMessageKind::ToServerResponseStart(
									v7::ToServerResponseStart {
										status: resp.status,
										headers: resp.headers,
										body: resp.body,
										stream: resp.stream,
									},
								)
							}
							v6::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
								v7::ToServerTunnelMessageKind::ToServerResponseChunk(
									v7::ToServerResponseChunk {
										body: chunk.body,
										finish: chunk.finish,
									},
								)
							}
							v6::ToServerTunnelMessageKind::ToServerResponseAbort => {
								v7::ToServerTunnelMessageKind::ToServerResponseAbort
							}
							v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
								v7::ToServerTunnelMessageKind::ToServerWebSocketOpen(
									v7::ToServerWebSocketOpen {
										can_hibernate: open.can_hibernate,
									},
								)
							}
							v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(message) => {
								v7::ToServerTunnelMessageKind::ToServerWebSocketMessage(
									v7::ToServerWebSocketMessage {
										data: message.data,
										binary: message.binary,
									},
								)
							}
							v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
								v7::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
									v7::ToServerWebSocketMessageAck { index: ack.index },
								)
							}
							v6::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
								v7::ToServerTunnelMessageKind::ToServerWebSocketClose(
									v7::ToServerWebSocketClose {
										code: close.code,
										reason: close.reason,
										hibernate: close.hibernate,
									},
								)
							}
						},
					})
				}
			};

			Ok(ToServerMk2::V7(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v7_to_v6(self) -> Result<Self> {
		if let ToServerMk2::V7(x) = self {
			let inner = match x {
				v7::ToServer::ToServerInit(init) => v6::ToServer::ToServerInit(v6::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v6::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v7::ToServer::ToServerEvents(events) => v6::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v6::EventWrapper {
							checkpoint: v6::ActorCheckpoint {
								actor_id: event.checkpoint.actor_id,
								generation: event.checkpoint.generation,
								index: event.checkpoint.index,
							},
							inner: match event.inner {
								v7::Event::EventActorIntent(intent) => {
									v6::Event::EventActorIntent(v6::EventActorIntent {
										intent: match intent.intent {
											v7::ActorIntent::ActorIntentSleep => {
												v6::ActorIntent::ActorIntentSleep
											}
											v7::ActorIntent::ActorIntentStop => {
												v6::ActorIntent::ActorIntentStop
											}
										},
									})
								}
								v7::Event::EventActorStateUpdate(state) => {
									v6::Event::EventActorStateUpdate(v6::EventActorStateUpdate {
										state: match state.state {
											v7::ActorState::ActorStateRunning => {
												v6::ActorState::ActorStateRunning
											}
											v7::ActorState::ActorStateStopped(stopped) => {
												v6::ActorState::ActorStateStopped(
													v6::ActorStateStopped {
														code: match stopped.code {
															v7::StopCode::Ok => v6::StopCode::Ok,
															v7::StopCode::Error => {
																v6::StopCode::Error
															}
														},
														message: stopped.message,
													},
												)
											}
										},
									})
								}
								v7::Event::EventActorSetAlarm(alarm) => {
									v6::Event::EventActorSetAlarm(v6::EventActorSetAlarm {
										alarm_ts: alarm.alarm_ts,
									})
								}
							},
						})
						.collect(),
				),
				v7::ToServer::ToServerAckCommands(ack) => {
					v6::ToServer::ToServerAckCommands(v6::ToServerAckCommands {
						last_command_checkpoints: ack
							.last_command_checkpoints
							.into_iter()
							.map(|cp| v6::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: cp.generation,
								index: cp.index,
							})
							.collect(),
					})
				}
				v7::ToServer::ToServerStopping => v6::ToServer::ToServerStopping,
				v7::ToServer::ToServerPong(pong) => {
					v6::ToServer::ToServerPong(v6::ToServerPong { ts: pong.ts })
				}
				v7::ToServer::ToServerKvRequest(req) => {
					v6::ToServer::ToServerKvRequest(v6::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v7_to_v6(req.data)?,
					})
				}
				v7::ToServer::ToServerTunnelMessage(msg) => {
					v6::ToServer::ToServerTunnelMessage(v6::ToServerTunnelMessage {
						message_id: v6::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: match msg.message_kind {
							v7::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
								v6::ToServerTunnelMessageKind::ToServerResponseStart(
									v6::ToServerResponseStart {
										status: resp.status,
										headers: resp.headers,
										body: resp.body,
										stream: resp.stream,
									},
								)
							}
							v7::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
								v6::ToServerTunnelMessageKind::ToServerResponseChunk(
									v6::ToServerResponseChunk {
										body: chunk.body,
										finish: chunk.finish,
									},
								)
							}
							v7::ToServerTunnelMessageKind::ToServerResponseAbort => {
								v6::ToServerTunnelMessageKind::ToServerResponseAbort
							}
							v7::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
								v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(
									v6::ToServerWebSocketOpen {
										can_hibernate: open.can_hibernate,
									},
								)
							}
							v7::ToServerTunnelMessageKind::ToServerWebSocketMessage(message) => {
								v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(
									v6::ToServerWebSocketMessage {
										data: message.data,
										binary: message.binary,
									},
								)
							}
							v7::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
								v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
									v6::ToServerWebSocketMessageAck { index: ack.index },
								)
							}
							v7::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
								v6::ToServerTunnelMessageKind::ToServerWebSocketClose(
									v6::ToServerWebSocketClose {
										code: close.code,
										reason: close.reason,
										hibernate: close.hibernate,
									},
								)
							}
						},
					})
				}
			};

			Ok(ToServerMk2::V6(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ToRunnerMk2 {
	V4(v4::ToRunner),
	V7(v7::ToRunner),
}

impl OwnedVersionedData for ToRunnerMk2 {
	type Latest = v7::ToRunner;

	fn wrap_latest(latest: v7::ToRunner) -> Self {
		ToRunnerMk2::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let ToRunnerMk2::V7(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			4 => Ok(ToRunnerMk2::V4(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			5 | 6 | 7 => Ok(ToRunnerMk2::V7(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToRunnerMk2::V4(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToRunnerMk2::V7(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Ok, Self::v4_to_v7, Ok, Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Self::v7_to_v4, Ok, Ok, Ok]
	}
}

impl ToRunnerMk2 {
	fn v4_to_v7(self) -> Result<Self> {
		if let ToRunnerMk2::V4(x) = self {
			let inner = match x {
				v4::ToRunner::ToRunnerPing(ping) => v7::ToRunner::ToRunnerPing(v7::ToRunnerPing {
					gateway_id: ping.gateway_id,
					request_id: ping.request_id,
					ts: ping.ts,
				}),
				v4::ToRunner::ToRunnerClose => v7::ToRunner::ToRunnerClose,
				v4::ToRunner::ToClientCommands(commands) => v7::ToRunner::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v7::CommandWrapper {
							checkpoint: v7::ActorCheckpoint {
								actor_id: cmd.checkpoint.actor_id,
								generation: match &cmd.inner {
									v4::Command::CommandStartActor(start) => start.generation,
									v4::Command::CommandStopActor(stop) => stop.generation,
								},
								index: cmd.checkpoint.index,
							},
							inner: match cmd.inner {
								v4::Command::CommandStartActor(start) => {
									v7::Command::CommandStartActor(v7::CommandStartActor {
										config: v7::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: start
											.hibernating_requests
											.into_iter()
											.map(|req| v7::HibernatingRequest {
												gateway_id: req.gateway_id,
												request_id: req.request_id,
											})
											.collect(),
									})
								}
								v4::Command::CommandStopActor(_) => v7::Command::CommandStopActor,
							},
						})
						.collect(),
				),
				v4::ToRunner::ToClientAckEvents(ack) => {
					v7::ToRunner::ToClientAckEvents(v7::ToClientAckEvents {
						last_event_checkpoints: ack
							.last_event_checkpoints
							.into_iter()
							.map(|cp| v7::ActorCheckpoint {
								actor_id: cp.actor_id,
								generation: 0, // Unknown in v4, use default
								index: cp.index,
							})
							.collect(),
					})
				}
				v4::ToRunner::ToClientTunnelMessage(msg) => {
					v7::ToRunner::ToClientTunnelMessage(v7::ToClientTunnelMessage {
						message_id: v7::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v4_to_v7(
							msg.message_kind,
						),
					})
				}
			};

			Ok(ToRunnerMk2::V7(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v7_to_v4(self) -> Result<Self> {
		if let ToRunnerMk2::V7(x) = self {
			let inner = match x {
				v7::ToRunner::ToRunnerPing(ping) => v4::ToRunner::ToRunnerPing(v4::ToRunnerPing {
					gateway_id: ping.gateway_id,
					request_id: ping.request_id,
					ts: ping.ts,
				}),
				v7::ToRunner::ToRunnerClose => v4::ToRunner::ToRunnerClose,
				v7::ToRunner::ToClientCommands(commands) => v4::ToRunner::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v4::CommandWrapper {
							checkpoint: v4::ActorCheckpoint {
								actor_id: cmd.checkpoint.actor_id,
								index: cmd.checkpoint.index,
							},
							inner: match cmd.inner {
								v7::Command::CommandStartActor(start) => {
									v4::Command::CommandStartActor(v4::CommandStartActor {
										generation: cmd.checkpoint.generation,
										config: v4::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: start
											.hibernating_requests
											.into_iter()
											.map(|req| v4::HibernatingRequest {
												gateway_id: req.gateway_id,
												request_id: req.request_id,
											})
											.collect(),
									})
								}
								v7::Command::CommandStopActor => {
									v4::Command::CommandStopActor(v4::CommandStopActor {
										generation: cmd.checkpoint.generation,
									})
								}
							},
						})
						.collect(),
				),
				v7::ToRunner::ToClientAckEvents(ack) => {
					v4::ToRunner::ToClientAckEvents(v4::ToClientAckEvents {
						last_event_checkpoints: ack
							.last_event_checkpoints
							.into_iter()
							.map(|cp| v4::ActorCheckpoint {
								actor_id: cp.actor_id,
								index: cp.index,
							})
							.collect(),
					})
				}
				v7::ToRunner::ToClientTunnelMessage(msg) => {
					v4::ToRunner::ToClientTunnelMessage(v4::ToClientTunnelMessage {
						message_id: v4::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v7_to_v4(
							msg.message_kind,
						),
					})
				}
			};

			Ok(ToRunnerMk2::V4(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ToClient {
	V1(v1::ToClient),
	V2(v2::ToClient),
	V3(v3::ToClient),
}

impl OwnedVersionedData for ToClient {
	type Latest = v3::ToClient;

	fn wrap_latest(latest: v3::ToClient) -> Self {
		ToClient::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let ToClient::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(ToClient::V1(rivet_util::serde::bare_from_slice!(payload)?)),
			2 => Ok(ToClient::V2(rivet_util::serde::bare_from_slice!(payload)?)),
			3 => Ok(ToClient::V3(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToClient::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToClient::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToClient::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Self::v2_to_v1]
	}
}

impl ToClient {
	fn v1_to_v2(self) -> Result<Self> {
		if let ToClient::V1(x) = self {
			let inner = match x {
				v1::ToClient::ToClientInit(init) => v2::ToClient::ToClientInit(v2::ToClientInit {
					runner_id: init.runner_id,
					last_event_idx: init.last_event_idx,
					metadata: v2::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v1::ToClient::ToClientClose => v2::ToClient::ToClientClose,
				v1::ToClient::ToClientCommands(commands) => v2::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v2::CommandWrapper {
							index: cmd.index,
							inner: match cmd.inner {
								v1::Command::CommandStartActor(start) => {
									v2::Command::CommandStartActor(v2::CommandStartActor {
										actor_id: start.actor_id,
										generation: start.generation,
										config: v2::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
									})
								}
								v1::Command::CommandStopActor(stop) => {
									v2::Command::CommandStopActor(v2::CommandStopActor {
										actor_id: stop.actor_id,
										generation: stop.generation,
									})
								}
							},
						})
						.collect(),
				),
				v1::ToClient::ToClientAckEvents(ack) => {
					v2::ToClient::ToClientAckEvents(v2::ToClientAckEvents {
						last_event_idx: ack.last_event_idx,
					})
				}
				v1::ToClient::ToClientKvResponse(resp) => {
					v2::ToClient::ToClientKvResponse(v2::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v1_to_v2(resp.data),
					})
				}
				v1::ToClient::ToClientTunnelMessage(msg) => {
					v2::ToClient::ToClientTunnelMessage(v2::ToClientTunnelMessage {
						request_id: msg.request_id,
						message_id: msg.message_id,
						message_kind: convert_to_client_tunnel_message_kind_v1_to_v2(
							msg.message_kind,
						),
						gateway_reply_to: msg.gateway_reply_to,
					})
				}
			};

			Ok(ToClient::V2(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v3(self) -> Result<Self> {
		if let ToClient::V2(x) = self {
			let inner = match x {
				v2::ToClient::ToClientInit(init) => v3::ToClient::ToClientInit(v3::ToClientInit {
					runner_id: init.runner_id,
					last_event_idx: init.last_event_idx,
					metadata: v3::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v2::ToClient::ToClientClose => v3::ToClient::ToClientClose,
				v2::ToClient::ToClientCommands(commands) => v3::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v3::CommandWrapper {
							index: cmd.index,
							inner: match cmd.inner {
								v2::Command::CommandStartActor(start) => {
									v3::Command::CommandStartActor(v3::CommandStartActor {
										actor_id: start.actor_id,
										generation: start.generation,
										config: v3::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
										hibernating_requests: Vec::new(),
									})
								}
								v2::Command::CommandStopActor(stop) => {
									v3::Command::CommandStopActor(v3::CommandStopActor {
										actor_id: stop.actor_id,
										generation: stop.generation,
									})
								}
							},
						})
						.collect(),
				),
				v2::ToClient::ToClientAckEvents(ack) => {
					v3::ToClient::ToClientAckEvents(v3::ToClientAckEvents {
						last_event_idx: ack.last_event_idx,
					})
				}
				v2::ToClient::ToClientKvResponse(resp) => {
					v3::ToClient::ToClientKvResponse(v3::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v2_to_v3(resp.data),
					})
				}
				v2::ToClient::ToClientTunnelMessage(msg) => {
					// Extract v3 message_id from v2's UUIDs
					// v2.message_id (UUID) contains: gateway_id (4) + request_id (4) + message_index (2) = 10 bytes
					let decoded = decode_bytes_from_uuid(&msg.request_id);

					let mut gateway_id = [0u8; 4];
					gateway_id.copy_from_slice(&decoded[..4]);
					let mut request_id = [0u8; 4];
					request_id.copy_from_slice(&decoded[4..8]);
					let message_index = u16::from_le_bytes([decoded[8], decoded[9]]);

					v3::ToClient::ToClientTunnelMessage(v3::ToClientTunnelMessage {
						message_id: v3::MessageId {
							gateway_id,
							request_id,
							message_index,
						},
						message_kind: convert_to_client_tunnel_message_kind_v2_to_v3(
							msg.message_kind,
						),
					})
				}
			};

			Ok(ToClient::V3(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		if let ToClient::V3(x) = self {
			let inner = match x {
				v3::ToClient::ToClientInit(init) => v2::ToClient::ToClientInit(v2::ToClientInit {
					runner_id: init.runner_id,
					last_event_idx: init.last_event_idx,
					metadata: v2::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v3::ToClient::ToClientClose => v2::ToClient::ToClientClose,
				v3::ToClient::ToClientCommands(commands) => v2::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v2::CommandWrapper {
							index: cmd.index,
							inner: match cmd.inner {
								v3::Command::CommandStartActor(start) => {
									v2::Command::CommandStartActor(v2::CommandStartActor {
										actor_id: start.actor_id,
										generation: start.generation,
										config: v2::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
									})
								}
								v3::Command::CommandStopActor(stop) => {
									v2::Command::CommandStopActor(v2::CommandStopActor {
										actor_id: stop.actor_id,
										generation: stop.generation,
									})
								}
							},
						})
						.collect(),
				),
				v3::ToClient::ToClientAckEvents(ack) => {
					v2::ToClient::ToClientAckEvents(v2::ToClientAckEvents {
						last_event_idx: ack.last_event_idx,
					})
				}
				v3::ToClient::ToClientKvResponse(resp) => {
					v2::ToClient::ToClientKvResponse(v2::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v3_to_v2(resp.data),
					})
				}
				v3::ToClient::ToClientTunnelMessage(msg) => {
					// Encode v3 message_id into v2's UUIDs
					// v3: gateway_id (4) + request_id (4) + message_index (2) = 10 bytes
					let mut data = [0u8; 10];
					data[..4].copy_from_slice(&msg.message_id.gateway_id);
					data[4..8].copy_from_slice(&msg.message_id.request_id);
					data[8..10].copy_from_slice(&msg.message_id.message_index.to_le_bytes());

					let message_id = encode_bytes_to_uuid(&data);

					// request_id contains gateway_id + request_id for backwards compatibility
					let mut request_id_data = [0u8; 8];
					request_id_data[..4].copy_from_slice(&msg.message_id.gateway_id);
					request_id_data[4..8].copy_from_slice(&msg.message_id.request_id);
					let request_id = encode_bytes_to_uuid(&request_id_data);

					v2::ToClient::ToClientTunnelMessage(v2::ToClientTunnelMessage {
						request_id,
						message_id,
						message_kind: convert_to_client_tunnel_message_kind_v3_to_v2(
							msg.message_kind,
							&msg.message_id,
						)?,
						gateway_reply_to: None,
					})
				}
			};

			Ok(ToClient::V2(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		if let ToClient::V2(x) = self {
			let inner = match x {
				v2::ToClient::ToClientInit(init) => v1::ToClient::ToClientInit(v1::ToClientInit {
					runner_id: init.runner_id,
					last_event_idx: init.last_event_idx,
					metadata: v1::ProtocolMetadata {
						runner_lost_threshold: init.metadata.runner_lost_threshold,
					},
				}),
				v2::ToClient::ToClientClose => v1::ToClient::ToClientClose,
				v2::ToClient::ToClientCommands(commands) => v1::ToClient::ToClientCommands(
					commands
						.into_iter()
						.map(|cmd| v1::CommandWrapper {
							index: cmd.index,
							inner: match cmd.inner {
								v2::Command::CommandStartActor(start) => {
									v1::Command::CommandStartActor(v1::CommandStartActor {
										actor_id: start.actor_id,
										generation: start.generation,
										config: v1::ActorConfig {
											name: start.config.name,
											key: start.config.key,
											create_ts: start.config.create_ts,
											input: start.config.input,
										},
									})
								}
								v2::Command::CommandStopActor(stop) => {
									v1::Command::CommandStopActor(v1::CommandStopActor {
										actor_id: stop.actor_id,
										generation: stop.generation,
									})
								}
							},
						})
						.collect(),
				),
				v2::ToClient::ToClientAckEvents(ack) => {
					v1::ToClient::ToClientAckEvents(v1::ToClientAckEvents {
						last_event_idx: ack.last_event_idx,
					})
				}
				v2::ToClient::ToClientKvResponse(resp) => {
					v1::ToClient::ToClientKvResponse(v1::ToClientKvResponse {
						request_id: resp.request_id,
						data: convert_kv_response_data_v2_to_v1(resp.data),
					})
				}
				v2::ToClient::ToClientTunnelMessage(msg) => {
					v1::ToClient::ToClientTunnelMessage(v1::ToClientTunnelMessage {
						request_id: msg.request_id,
						message_id: msg.message_id,
						message_kind: convert_to_client_tunnel_message_kind_v2_to_v1(
							msg.message_kind,
						)?,
						gateway_reply_to: msg.gateway_reply_to,
					})
				}
			};

			Ok(ToClient::V1(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ToServer {
	V1(v1::ToServer),
	V2(v2::ToServer),
	V3(v3::ToServer),
}

impl OwnedVersionedData for ToServer {
	type Latest = v3::ToServer;

	fn wrap_latest(latest: v3::ToServer) -> Self {
		ToServer::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let ToServer::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(ToServer::V1(rivet_util::serde::bare_from_slice!(payload)?)),
			2 => Ok(ToServer::V2(rivet_util::serde::bare_from_slice!(payload)?)),
			3 => Ok(ToServer::V3(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToServer::V1(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToServer::V2(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToServer::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v1_to_v2, Self::v2_to_v3]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Self::v3_to_v2, Self::v2_to_v1]
	}
}

impl ToServer {
	fn v1_to_v2(self) -> Result<Self> {
		if let ToServer::V1(x) = self {
			let inner = match x {
				v1::ToServer::ToServerInit(init) => v2::ToServer::ToServerInit(v2::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					last_command_idx: init.last_command_idx,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v2::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v1::ToServer::ToServerEvents(events) => v2::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v2::EventWrapper {
							index: event.index,
							inner: convert_event_v1_to_v2(event.inner),
						})
						.collect(),
				),
				v1::ToServer::ToServerAckCommands(ack) => {
					v2::ToServer::ToServerAckCommands(v2::ToServerAckCommands {
						last_command_idx: ack.last_command_idx,
					})
				}
				v1::ToServer::ToServerStopping => v2::ToServer::ToServerStopping,
				v1::ToServer::ToServerPing(ping) => {
					v2::ToServer::ToServerPing(v2::ToServerPing { ts: ping.ts })
				}
				v1::ToServer::ToServerKvRequest(req) => {
					v2::ToServer::ToServerKvRequest(v2::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v1_to_v2(req.data),
					})
				}
				v1::ToServer::ToServerTunnelMessage(msg) => {
					v2::ToServer::ToServerTunnelMessage(v2::ToServerTunnelMessage {
						request_id: msg.request_id,
						message_id: msg.message_id,
						message_kind: convert_to_server_tunnel_message_kind_v1_to_v2(
							msg.message_kind,
						),
					})
				}
			};

			Ok(ToServer::V2(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v3(self) -> Result<Self> {
		if let ToServer::V2(x) = self {
			let inner = match x {
				v2::ToServer::ToServerInit(init) => v3::ToServer::ToServerInit(v3::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					last_command_idx: init.last_command_idx,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v3::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v2::ToServer::ToServerEvents(events) => v3::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v3::EventWrapper {
							index: event.index,
							inner: convert_event_v2_to_v3(event.inner),
						})
						.collect(),
				),
				v2::ToServer::ToServerAckCommands(ack) => {
					v3::ToServer::ToServerAckCommands(v3::ToServerAckCommands {
						last_command_idx: ack.last_command_idx,
					})
				}
				v2::ToServer::ToServerStopping => v3::ToServer::ToServerStopping,
				v2::ToServer::ToServerPing(ping) => {
					v3::ToServer::ToServerPing(v3::ToServerPing { ts: ping.ts })
				}
				v2::ToServer::ToServerKvRequest(req) => {
					v3::ToServer::ToServerKvRequest(v3::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v2_to_v3(req.data),
					})
				}
				v2::ToServer::ToServerTunnelMessage(msg) => {
					// Extract v3 message_id from v2's UUIDs
					// v2.message_id (UUID) contains: gateway_id (4) + request_id (4) + message_index (2) = 10 bytes
					let decoded = decode_bytes_from_uuid(&msg.request_id);

					let mut gateway_id = [0u8; 4];
					gateway_id.copy_from_slice(&decoded[..4]);
					let mut request_id = [0u8; 4];
					request_id.copy_from_slice(&decoded[4..8]);
					let message_index = u16::from_le_bytes([decoded[8], decoded[9]]);

					v3::ToServer::ToServerTunnelMessage(v3::ToServerTunnelMessage {
						message_id: v3::MessageId {
							gateway_id,
							request_id,
							message_index,
						},
						message_kind: convert_to_server_tunnel_message_kind_v2_to_v3(
							msg.message_kind,
						),
					})
				}
			};

			Ok(ToServer::V3(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		if let ToServer::V3(x) = self {
			let inner = match x {
				v3::ToServer::ToServerInit(init) => v2::ToServer::ToServerInit(v2::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					last_command_idx: init.last_command_idx,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v2::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v3::ToServer::ToServerEvents(events) => v2::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v2::EventWrapper {
							index: event.index,
							inner: convert_event_v3_to_v2(event.inner),
						})
						.collect(),
				),
				v3::ToServer::ToServerAckCommands(ack) => {
					v2::ToServer::ToServerAckCommands(v2::ToServerAckCommands {
						last_command_idx: ack.last_command_idx,
					})
				}
				v3::ToServer::ToServerStopping => v2::ToServer::ToServerStopping,
				v3::ToServer::ToServerPing(ping) => {
					v2::ToServer::ToServerPing(v2::ToServerPing { ts: ping.ts })
				}
				v3::ToServer::ToServerKvRequest(req) => {
					v2::ToServer::ToServerKvRequest(v2::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v3_to_v2(req.data),
					})
				}
				v3::ToServer::ToServerTunnelMessage(msg) => {
					// Encode v3 message_id into v2's UUIDs
					// v3: gateway_id (4) + request_id (4) + message_index (2) = 10 bytes
					let mut data = [0u8; 10];
					data[..4].copy_from_slice(&msg.message_id.gateway_id);
					data[4..8].copy_from_slice(&msg.message_id.request_id);
					data[8..10].copy_from_slice(&msg.message_id.message_index.to_le_bytes());

					let message_id = encode_bytes_to_uuid(&data);

					// request_id contains gateway_id + request_id for backwards compatibility
					let mut request_id_data = [0u8; 8];
					request_id_data[..4].copy_from_slice(&msg.message_id.gateway_id);
					request_id_data[4..8].copy_from_slice(&msg.message_id.request_id);
					let request_id = encode_bytes_to_uuid(&request_id_data);

					v2::ToServer::ToServerTunnelMessage(v2::ToServerTunnelMessage {
						request_id,
						message_id,
						message_kind: convert_to_server_tunnel_message_kind_v3_to_v2(
							msg.message_kind,
						)?,
					})
				}
			};

			Ok(ToServer::V2(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		if let ToServer::V2(x) = self {
			let inner = match x {
				v2::ToServer::ToServerInit(init) => v1::ToServer::ToServerInit(v1::ToServerInit {
					name: init.name,
					version: init.version,
					total_slots: init.total_slots,
					last_command_idx: init.last_command_idx,
					prepopulate_actor_names: init.prepopulate_actor_names.map(|map| {
						map.into_iter()
							.map(|(k, v)| {
								(
									k,
									v1::ActorName {
										metadata: v.metadata,
									},
								)
							})
							.collect()
					}),
					metadata: init.metadata,
				}),
				v2::ToServer::ToServerEvents(events) => v1::ToServer::ToServerEvents(
					events
						.into_iter()
						.map(|event| v1::EventWrapper {
							index: event.index,
							inner: convert_event_v2_to_v1(event.inner),
						})
						.collect(),
				),
				v2::ToServer::ToServerAckCommands(ack) => {
					v1::ToServer::ToServerAckCommands(v1::ToServerAckCommands {
						last_command_idx: ack.last_command_idx,
					})
				}
				v2::ToServer::ToServerStopping => v1::ToServer::ToServerStopping,
				v2::ToServer::ToServerPing(ping) => {
					v1::ToServer::ToServerPing(v1::ToServerPing { ts: ping.ts })
				}
				v2::ToServer::ToServerKvRequest(req) => {
					v1::ToServer::ToServerKvRequest(v1::ToServerKvRequest {
						actor_id: req.actor_id,
						request_id: req.request_id,
						data: convert_kv_request_data_v2_to_v1(req.data),
					})
				}
				v2::ToServer::ToServerTunnelMessage(msg) => {
					v1::ToServer::ToServerTunnelMessage(v1::ToServerTunnelMessage {
						request_id: msg.request_id,
						message_id: msg.message_id,
						message_kind: convert_to_server_tunnel_message_kind_v2_to_v1(
							msg.message_kind,
						)?,
					})
				}
			};

			Ok(ToServer::V1(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ToRunner {
	// Only in v3
	V3(v3::ToRunner),
}

impl OwnedVersionedData for ToRunner {
	type Latest = v3::ToRunner;

	fn wrap_latest(latest: v3::ToRunner) -> Self {
		ToRunner::V3(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let ToRunner::V3(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 | 2 | 3 => Ok(ToRunner::V3(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToRunner::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		// No changes between v1 and v3
		vec![Ok, Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		// No changes between v1 and v3
		vec![Ok, Ok]
	}
}

pub enum ToGateway {
	V3(v3::ToGateway),
	V7(v7::ToGateway),
}

impl OwnedVersionedData for ToGateway {
	type Latest = v7::ToGateway;

	fn wrap_latest(latest: v7::ToGateway) -> Self {
		ToGateway::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let ToGateway::V7(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 | 2 | 3 => Ok(ToGateway::V3(rivet_util::serde::bare_from_slice!(payload)?)),
			4 | 5 | 6 | 7 => Ok(ToGateway::V7(rivet_util::serde::bare_from_slice!(payload)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToGateway::V3(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
			ToGateway::V7(data) => rivet_util::serde::bare_to_vec!(&data).map_err(Into::into),
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Self::v3_to_v7, Ok, Ok, Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Ok, Self::v7_to_v3, Ok, Ok]
	}
}

impl ToGateway {
	pub fn v3_to_v7(self) -> Result<Self> {
		if let ToGateway::V3(x) = self {
			let inner = match x {
				v3::ToGateway::ToGatewayPong(pong) => {
					v7::ToGateway::ToGatewayPong(v7::ToGatewayPong {
						request_id: pong.request_id,
						ts: pong.ts,
					})
				}
				v3::ToGateway::ToServerTunnelMessage(msg) => {
					v7::ToGateway::ToServerTunnelMessage(v7::ToServerTunnelMessage {
						message_id: v7::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_server_tunnel_message_kind_v6_to_v7(
							convert_to_server_tunnel_message_kind_v3_to_v4(msg.message_kind),
						),
					})
				}
			};

			Ok(ToGateway::V7(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v7_to_v3(self) -> Result<Self> {
		if let ToGateway::V7(x) = self {
			let inner = match x {
				v7::ToGateway::ToGatewayPong(pong) => {
					v3::ToGateway::ToGatewayPong(v3::ToGatewayPong {
						request_id: pong.request_id,
						ts: pong.ts,
					})
				}
				v7::ToGateway::ToServerTunnelMessage(msg) => {
					v3::ToGateway::ToServerTunnelMessage(v3::ToServerTunnelMessage {
						message_id: v3::MessageId {
							gateway_id: msg.message_id.gateway_id,
							request_id: msg.message_id.request_id,
							message_index: msg.message_id.message_index,
						},
						message_kind: convert_to_server_tunnel_message_kind_v4_to_v3(
							convert_to_server_tunnel_message_kind_v7_to_v6(msg.message_kind),
						)?,
					})
				}
			};

			Ok(ToGateway::V3(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ToServerlessServer {
	V3(v3::ToServerlessServer),
	V7(v7::ToServerlessServer),
}

impl OwnedVersionedData for ToServerlessServer {
	type Latest = v7::ToServerlessServer;

	fn wrap_latest(latest: v7::ToServerlessServer) -> Self {
		ToServerlessServer::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let ToServerlessServer::V7(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 | 2 | 3 => Ok(ToServerlessServer::V3(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			4 | 5 | 6 | 7 => Ok(ToServerlessServer::V7(rivet_util::serde::bare_from_slice!(
				payload
			)?)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ToServerlessServer::V3(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			ToServerlessServer::V7(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Self::v3_to_v7, Ok, Ok, Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Ok, Self::v7_to_v3, Ok, Ok]
	}
}

impl ToServerlessServer {
	fn v3_to_v7(self) -> Result<Self> {
		if let ToServerlessServer::V3(x) = self {
			let inner = match x {
				v3::ToServerlessServer::ToServerlessServerInit(init) => {
					v7::ToServerlessServer::ToServerlessServerInit(v7::ToServerlessServerInit {
						runner_id: init.runner_id,
						runner_protocol_version: PROTOCOL_MK1_VERSION,
					})
				}
			};

			Ok(ToServerlessServer::V7(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v7_to_v3(self) -> Result<Self> {
		if let ToServerlessServer::V7(x) = self {
			let inner = match x {
				v7::ToServerlessServer::ToServerlessServerInit(init) => {
					v3::ToServerlessServer::ToServerlessServerInit(v3::ToServerlessServerInit {
						runner_id: init.runner_id,
					})
				}
			};

			Ok(ToServerlessServer::V3(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

pub enum ActorCommandKeyData {
	V4(v4::ActorCommandKeyData),
	V7(v7::ActorCommandKeyData),
}

impl OwnedVersionedData for ActorCommandKeyData {
	type Latest = v7::ActorCommandKeyData;

	fn wrap_latest(latest: v7::ActorCommandKeyData) -> Self {
		ActorCommandKeyData::V7(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		if let ActorCommandKeyData::V7(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			4 => Ok(ActorCommandKeyData::V4(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			5 | 6 | 7 => Ok(ActorCommandKeyData::V7(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			ActorCommandKeyData::V4(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			ActorCommandKeyData::V7(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Ok, Self::v4_to_v7, Ok, Ok]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![Ok, Ok, Self::v7_to_v4, Ok, Ok, Ok]
	}
}

impl ActorCommandKeyData {
	fn v4_to_v7(self) -> Result<Self> {
		if let ActorCommandKeyData::V4(x) = self {
			let inner = match x {
				v4::ActorCommandKeyData::CommandStartActor(start) => {
					v7::ActorCommandKeyData::CommandStartActor(v7::CommandStartActor {
						config: v7::ActorConfig {
							name: start.config.name,
							key: start.config.key,
							create_ts: start.config.create_ts,
							input: start.config.input,
						},
						hibernating_requests: start
							.hibernating_requests
							.into_iter()
							.map(|req| v7::HibernatingRequest {
								gateway_id: req.gateway_id,
								request_id: req.request_id,
							})
							.collect(),
					})
				}
				v4::ActorCommandKeyData::CommandStopActor(_) => {
					v7::ActorCommandKeyData::CommandStopActor
				}
			};

			Ok(ActorCommandKeyData::V7(inner))
		} else {
			bail!("unexpected version");
		}
	}

	fn v7_to_v4(self) -> Result<Self> {
		if let ActorCommandKeyData::V7(x) = self {
			// Since v4 commands have generation but v7 doesn't, use generation 0 as a placeholder
			let inner = match x {
				v7::ActorCommandKeyData::CommandStartActor(start) => {
					v4::ActorCommandKeyData::CommandStartActor(v4::CommandStartActor {
						generation: 0, // Lost during conversion
						config: v4::ActorConfig {
							name: start.config.name,
							key: start.config.key,
							create_ts: start.config.create_ts,
							input: start.config.input,
						},
						hibernating_requests: start
							.hibernating_requests
							.into_iter()
							.map(|req| v4::HibernatingRequest {
								gateway_id: req.gateway_id,
								request_id: req.request_id,
							})
							.collect(),
					})
				}
				v7::ActorCommandKeyData::CommandStopActor => {
					v4::ActorCommandKeyData::CommandStopActor(v4::CommandStopActor {
						generation: 0, // Lost during conversion
					})
				}
			};

			Ok(ActorCommandKeyData::V4(inner))
		} else {
			bail!("unexpected version");
		}
	}
}

// Helper conversion functions
fn convert_to_client_tunnel_message_kind_v1_to_v2(
	kind: v1::ToClientTunnelMessageKind,
) -> v2::ToClientTunnelMessageKind {
	match kind {
		v1::ToClientTunnelMessageKind::TunnelAck => v2::ToClientTunnelMessageKind::TunnelAck,
		v1::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v2::ToClientTunnelMessageKind::ToClientRequestStart(v2::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v1::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v2::ToClientTunnelMessageKind::ToClientRequestChunk(v2::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v1::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v2::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v1::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v2::ToClientTunnelMessageKind::ToClientWebSocketOpen(v2::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v1::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v2::ToClientTunnelMessageKind::ToClientWebSocketMessage(v2::ToClientWebSocketMessage {
				// Default to 0 for v1 messages (hibernation disabled by default)
				index: 0,
				data: msg.data,
				binary: msg.binary,
			})
		}
		v1::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v2::ToClientTunnelMessageKind::ToClientWebSocketClose(v2::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_client_tunnel_message_kind_v2_to_v1(
	kind: v2::ToClientTunnelMessageKind,
) -> Result<v1::ToClientTunnelMessageKind> {
	Ok(match kind {
		v2::ToClientTunnelMessageKind::TunnelAck => v1::ToClientTunnelMessageKind::TunnelAck,
		v2::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v1::ToClientTunnelMessageKind::ToClientRequestStart(v1::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v1::ToClientTunnelMessageKind::ToClientRequestChunk(v1::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v1::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v2::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v1::ToClientTunnelMessageKind::ToClientWebSocketOpen(v1::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v1::ToClientTunnelMessageKind::ToClientWebSocketMessage(v1::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v1::ToClientTunnelMessageKind::ToClientWebSocketClose(v1::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	})
}

fn convert_to_server_tunnel_message_kind_v1_to_v2(
	kind: v1::ToServerTunnelMessageKind,
) -> v2::ToServerTunnelMessageKind {
	match kind {
		v1::ToServerTunnelMessageKind::TunnelAck => v2::ToServerTunnelMessageKind::TunnelAck,
		v1::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v2::ToServerTunnelMessageKind::ToServerResponseStart(v2::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v1::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v2::ToServerTunnelMessageKind::ToServerResponseChunk(v2::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v1::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v2::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v1::ToServerTunnelMessageKind::ToServerWebSocketOpen => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketOpen(v2::ToServerWebSocketOpen {
				can_hibernate: false,
				last_msg_index: -1,
			})
		}
		v1::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketMessage(v2::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v1::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketClose(v2::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				retry: false,
			})
		}
	}
}

fn convert_to_server_tunnel_message_kind_v2_to_v1(
	kind: v2::ToServerTunnelMessageKind,
) -> Result<v1::ToServerTunnelMessageKind> {
	Ok(match kind {
		v2::ToServerTunnelMessageKind::TunnelAck => v1::ToServerTunnelMessageKind::TunnelAck,
		v2::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v1::ToServerTunnelMessageKind::ToServerResponseStart(v1::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v1::ToServerTunnelMessageKind::ToServerResponseChunk(v1::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v1::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketOpen(_) => {
			v1::ToServerTunnelMessageKind::ToServerWebSocketOpen
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v1::ToServerTunnelMessageKind::ToServerWebSocketMessage(v1::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(_) => {
			// v1 doesn't have MessageAck, this is a v2-only feature
			bail!("ToServerWebSocketMessageAck is not supported in v1");
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v1::ToServerTunnelMessageKind::ToServerWebSocketClose(v1::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	})
}

fn convert_event_v1_to_v2(event: v1::Event) -> v2::Event {
	match event {
		v1::Event::EventActorIntent(intent) => v2::Event::EventActorIntent(v2::EventActorIntent {
			actor_id: intent.actor_id,
			generation: intent.generation,
			intent: convert_actor_intent_v1_to_v2(intent.intent),
		}),
		v1::Event::EventActorStateUpdate(state) => {
			v2::Event::EventActorStateUpdate(v2::EventActorStateUpdate {
				actor_id: state.actor_id,
				generation: state.generation,
				state: convert_actor_state_v1_to_v2(state.state),
			})
		}
		v1::Event::EventActorSetAlarm(alarm) => {
			v2::Event::EventActorSetAlarm(v2::EventActorSetAlarm {
				actor_id: alarm.actor_id,
				generation: alarm.generation,
				alarm_ts: alarm.alarm_ts,
			})
		}
	}
}

fn convert_event_v2_to_v1(event: v2::Event) -> v1::Event {
	match event {
		v2::Event::EventActorIntent(intent) => v1::Event::EventActorIntent(v1::EventActorIntent {
			actor_id: intent.actor_id,
			generation: intent.generation,
			intent: convert_actor_intent_v2_to_v1(intent.intent),
		}),
		v2::Event::EventActorStateUpdate(state) => {
			v1::Event::EventActorStateUpdate(v1::EventActorStateUpdate {
				actor_id: state.actor_id,
				generation: state.generation,
				state: convert_actor_state_v2_to_v1(state.state),
			})
		}
		v2::Event::EventActorSetAlarm(alarm) => {
			v1::Event::EventActorSetAlarm(v1::EventActorSetAlarm {
				actor_id: alarm.actor_id,
				generation: alarm.generation,
				alarm_ts: alarm.alarm_ts,
			})
		}
	}
}

fn convert_actor_intent_v1_to_v2(intent: v1::ActorIntent) -> v2::ActorIntent {
	match intent {
		v1::ActorIntent::ActorIntentSleep => v2::ActorIntent::ActorIntentSleep,
		v1::ActorIntent::ActorIntentStop => v2::ActorIntent::ActorIntentStop,
	}
}

fn convert_actor_intent_v2_to_v1(intent: v2::ActorIntent) -> v1::ActorIntent {
	match intent {
		v2::ActorIntent::ActorIntentSleep => v1::ActorIntent::ActorIntentSleep,
		v2::ActorIntent::ActorIntentStop => v1::ActorIntent::ActorIntentStop,
	}
}

fn convert_actor_state_v1_to_v2(state: v1::ActorState) -> v2::ActorState {
	match state {
		v1::ActorState::ActorStateRunning => v2::ActorState::ActorStateRunning,
		v1::ActorState::ActorStateStopped(stopped) => {
			v2::ActorState::ActorStateStopped(v2::ActorStateStopped {
				code: convert_stop_code_v1_to_v2(stopped.code),
				message: stopped.message,
			})
		}
	}
}

fn convert_actor_state_v2_to_v1(state: v2::ActorState) -> v1::ActorState {
	match state {
		v2::ActorState::ActorStateRunning => v1::ActorState::ActorStateRunning,
		v2::ActorState::ActorStateStopped(stopped) => {
			v1::ActorState::ActorStateStopped(v1::ActorStateStopped {
				code: convert_stop_code_v2_to_v1(stopped.code),
				message: stopped.message,
			})
		}
	}
}

fn convert_stop_code_v1_to_v2(code: v1::StopCode) -> v2::StopCode {
	match code {
		v1::StopCode::Ok => v2::StopCode::Ok,
		v1::StopCode::Error => v2::StopCode::Error,
	}
}

fn convert_stop_code_v2_to_v1(code: v2::StopCode) -> v1::StopCode {
	match code {
		v2::StopCode::Ok => v1::StopCode::Ok,
		v2::StopCode::Error => v1::StopCode::Error,
	}
}

fn convert_kv_request_data_v1_to_v2(data: v1::KvRequestData) -> v2::KvRequestData {
	match data {
		v1::KvRequestData::KvGetRequest(req) => {
			v2::KvRequestData::KvGetRequest(v2::KvGetRequest { keys: req.keys })
		}
		v1::KvRequestData::KvListRequest(req) => {
			v2::KvRequestData::KvListRequest(v2::KvListRequest {
				query: convert_kv_list_query_v1_to_v2(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v1::KvRequestData::KvPutRequest(req) => v2::KvRequestData::KvPutRequest(v2::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v1::KvRequestData::KvDeleteRequest(req) => {
			v2::KvRequestData::KvDeleteRequest(v2::KvDeleteRequest { keys: req.keys })
		}
		v1::KvRequestData::KvDropRequest => v2::KvRequestData::KvDropRequest,
	}
}

fn convert_kv_request_data_v2_to_v1(data: v2::KvRequestData) -> v1::KvRequestData {
	match data {
		v2::KvRequestData::KvGetRequest(req) => {
			v1::KvRequestData::KvGetRequest(v1::KvGetRequest { keys: req.keys })
		}
		v2::KvRequestData::KvListRequest(req) => {
			v1::KvRequestData::KvListRequest(v1::KvListRequest {
				query: convert_kv_list_query_v2_to_v1(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v2::KvRequestData::KvPutRequest(req) => v1::KvRequestData::KvPutRequest(v1::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v2::KvRequestData::KvDeleteRequest(req) => {
			v1::KvRequestData::KvDeleteRequest(v1::KvDeleteRequest { keys: req.keys })
		}
		v2::KvRequestData::KvDropRequest => v1::KvRequestData::KvDropRequest,
	}
}

fn convert_kv_response_data_v1_to_v2(data: v1::KvResponseData) -> v2::KvResponseData {
	match data {
		v1::KvResponseData::KvErrorResponse(err) => {
			v2::KvResponseData::KvErrorResponse(v2::KvErrorResponse {
				message: err.message,
			})
		}
		v1::KvResponseData::KvGetResponse(resp) => {
			v2::KvResponseData::KvGetResponse(v2::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v1_to_v2)
					.collect(),
			})
		}
		v1::KvResponseData::KvListResponse(resp) => {
			v2::KvResponseData::KvListResponse(v2::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v1_to_v2)
					.collect(),
			})
		}
		v1::KvResponseData::KvPutResponse => v2::KvResponseData::KvPutResponse,
		v1::KvResponseData::KvDeleteResponse => v2::KvResponseData::KvDeleteResponse,
		v1::KvResponseData::KvDropResponse => v2::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_response_data_v2_to_v1(data: v2::KvResponseData) -> v1::KvResponseData {
	match data {
		v2::KvResponseData::KvErrorResponse(err) => {
			v1::KvResponseData::KvErrorResponse(v1::KvErrorResponse {
				message: err.message,
			})
		}
		v2::KvResponseData::KvGetResponse(resp) => {
			v1::KvResponseData::KvGetResponse(v1::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v2_to_v1)
					.collect(),
			})
		}
		v2::KvResponseData::KvListResponse(resp) => {
			v1::KvResponseData::KvListResponse(v1::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v2_to_v1)
					.collect(),
			})
		}
		v2::KvResponseData::KvPutResponse => v1::KvResponseData::KvPutResponse,
		v2::KvResponseData::KvDeleteResponse => v1::KvResponseData::KvDeleteResponse,
		v2::KvResponseData::KvDropResponse => v1::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_list_query_v1_to_v2(query: v1::KvListQuery) -> v2::KvListQuery {
	match query {
		v1::KvListQuery::KvListAllQuery => v2::KvListQuery::KvListAllQuery,
		v1::KvListQuery::KvListRangeQuery(range) => {
			v2::KvListQuery::KvListRangeQuery(v2::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v1::KvListQuery::KvListPrefixQuery(prefix) => {
			v2::KvListQuery::KvListPrefixQuery(v2::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_list_query_v2_to_v1(query: v2::KvListQuery) -> v1::KvListQuery {
	match query {
		v2::KvListQuery::KvListAllQuery => v1::KvListQuery::KvListAllQuery,
		v2::KvListQuery::KvListRangeQuery(range) => {
			v1::KvListQuery::KvListRangeQuery(v1::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v2::KvListQuery::KvListPrefixQuery(prefix) => {
			v1::KvListQuery::KvListPrefixQuery(v1::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_metadata_v1_to_v2(metadata: v1::KvMetadata) -> v2::KvMetadata {
	v2::KvMetadata {
		version: metadata.version,
		create_ts: metadata.create_ts,
	}
}

fn convert_kv_metadata_v2_to_v1(metadata: v2::KvMetadata) -> v1::KvMetadata {
	v1::KvMetadata {
		version: metadata.version,
		create_ts: metadata.create_ts,
	}
}

fn convert_to_client_tunnel_message_kind_v2_to_v3(
	kind: v2::ToClientTunnelMessageKind,
) -> v3::ToClientTunnelMessageKind {
	match kind {
		v2::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v3::ToClientTunnelMessageKind::ToClientRequestStart(v3::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v3::ToClientTunnelMessageKind::ToClientRequestChunk(v3::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v3::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v2::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v3::ToClientTunnelMessageKind::ToClientWebSocketOpen(v3::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v3::ToClientTunnelMessageKind::ToClientWebSocketMessage(v3::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v2::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v3::ToClientTunnelMessageKind::ToClientWebSocketClose(v3::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
		// DeprecatedTunnelAck is kept for backwards compatibility
		v2::ToClientTunnelMessageKind::TunnelAck => {
			v3::ToClientTunnelMessageKind::DeprecatedTunnelAck
		}
	}
}

fn convert_to_client_tunnel_message_kind_v3_to_v2(
	kind: v3::ToClientTunnelMessageKind,
	message_id: &v3::MessageId,
) -> Result<v2::ToClientTunnelMessageKind> {
	Ok(match kind {
		v3::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v2::ToClientTunnelMessageKind::ToClientRequestStart(v2::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v3::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v2::ToClientTunnelMessageKind::ToClientRequestChunk(v2::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v3::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v2::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v3::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v2::ToClientTunnelMessageKind::ToClientWebSocketOpen(v2::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v3::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v2::ToClientTunnelMessageKind::ToClientWebSocketMessage(v2::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
				index: message_id.message_index,
			})
		}
		v3::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v2::ToClientTunnelMessageKind::ToClientWebSocketClose(v2::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
		v3::ToClientTunnelMessageKind::DeprecatedTunnelAck => {
			v2::ToClientTunnelMessageKind::TunnelAck
		}
	})
}

fn convert_to_server_tunnel_message_kind_v2_to_v3(
	kind: v2::ToServerTunnelMessageKind,
) -> v3::ToServerTunnelMessageKind {
	match kind {
		v2::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v3::ToServerTunnelMessageKind::ToServerResponseStart(v3::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v3::ToServerTunnelMessageKind::ToServerResponseChunk(v3::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v3::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketOpen(v3::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketMessage(v3::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v3::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v2::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketClose(v3::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.retry,
			})
		}
		// DeprecatedTunnelAck is kept for backwards compatibility
		v2::ToServerTunnelMessageKind::TunnelAck => {
			v3::ToServerTunnelMessageKind::DeprecatedTunnelAck
		}
	}
}

fn convert_to_server_tunnel_message_kind_v3_to_v2(
	kind: v3::ToServerTunnelMessageKind,
) -> Result<v2::ToServerTunnelMessageKind> {
	Ok(match kind {
		v3::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v2::ToServerTunnelMessageKind::ToServerResponseStart(v2::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v2::ToServerTunnelMessageKind::ToServerResponseChunk(v2::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v2::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketOpen(v2::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
				last_msg_index: -1,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketMessage(v2::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v2::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v2::ToServerTunnelMessageKind::ToServerWebSocketClose(v2::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				retry: close.hibernate,
			})
		}
		v3::ToServerTunnelMessageKind::DeprecatedTunnelAck => {
			v2::ToServerTunnelMessageKind::TunnelAck
		}
	})
}

fn convert_event_v2_to_v3(event: v2::Event) -> v3::Event {
	match event {
		v2::Event::EventActorIntent(intent) => v3::Event::EventActorIntent(v3::EventActorIntent {
			actor_id: intent.actor_id,
			generation: intent.generation,
			intent: convert_actor_intent_v2_to_v3(intent.intent),
		}),
		v2::Event::EventActorStateUpdate(state) => {
			v3::Event::EventActorStateUpdate(v3::EventActorStateUpdate {
				actor_id: state.actor_id,
				generation: state.generation,
				state: convert_actor_state_v2_to_v3(state.state),
			})
		}
		v2::Event::EventActorSetAlarm(alarm) => {
			v3::Event::EventActorSetAlarm(v3::EventActorSetAlarm {
				actor_id: alarm.actor_id,
				generation: alarm.generation,
				alarm_ts: alarm.alarm_ts,
			})
		}
	}
}

fn convert_event_v3_to_v2(event: v3::Event) -> v2::Event {
	match event {
		v3::Event::EventActorIntent(intent) => v2::Event::EventActorIntent(v2::EventActorIntent {
			actor_id: intent.actor_id,
			generation: intent.generation,
			intent: convert_actor_intent_v3_to_v2(intent.intent),
		}),
		v3::Event::EventActorStateUpdate(state) => {
			v2::Event::EventActorStateUpdate(v2::EventActorStateUpdate {
				actor_id: state.actor_id,
				generation: state.generation,
				state: convert_actor_state_v3_to_v2(state.state),
			})
		}
		v3::Event::EventActorSetAlarm(alarm) => {
			v2::Event::EventActorSetAlarm(v2::EventActorSetAlarm {
				actor_id: alarm.actor_id,
				generation: alarm.generation,
				alarm_ts: alarm.alarm_ts,
			})
		}
	}
}

fn convert_actor_intent_v2_to_v3(intent: v2::ActorIntent) -> v3::ActorIntent {
	match intent {
		v2::ActorIntent::ActorIntentSleep => v3::ActorIntent::ActorIntentSleep,
		v2::ActorIntent::ActorIntentStop => v3::ActorIntent::ActorIntentStop,
	}
}

fn convert_actor_intent_v3_to_v2(intent: v3::ActorIntent) -> v2::ActorIntent {
	match intent {
		v3::ActorIntent::ActorIntentSleep => v2::ActorIntent::ActorIntentSleep,
		v3::ActorIntent::ActorIntentStop => v2::ActorIntent::ActorIntentStop,
	}
}

fn convert_actor_state_v2_to_v3(state: v2::ActorState) -> v3::ActorState {
	match state {
		v2::ActorState::ActorStateRunning => v3::ActorState::ActorStateRunning,
		v2::ActorState::ActorStateStopped(stopped) => {
			v3::ActorState::ActorStateStopped(v3::ActorStateStopped {
				code: convert_stop_code_v2_to_v3(stopped.code),
				message: stopped.message,
			})
		}
	}
}

fn convert_actor_state_v3_to_v2(state: v3::ActorState) -> v2::ActorState {
	match state {
		v3::ActorState::ActorStateRunning => v2::ActorState::ActorStateRunning,
		v3::ActorState::ActorStateStopped(stopped) => {
			v2::ActorState::ActorStateStopped(v2::ActorStateStopped {
				code: convert_stop_code_v3_to_v2(stopped.code),
				message: stopped.message,
			})
		}
	}
}

fn convert_stop_code_v2_to_v3(code: v2::StopCode) -> v3::StopCode {
	match code {
		v2::StopCode::Ok => v3::StopCode::Ok,
		v2::StopCode::Error => v3::StopCode::Error,
	}
}

fn convert_stop_code_v3_to_v2(code: v3::StopCode) -> v2::StopCode {
	match code {
		v3::StopCode::Ok => v2::StopCode::Ok,
		v3::StopCode::Error => v2::StopCode::Error,
	}
}

fn convert_kv_request_data_v2_to_v3(data: v2::KvRequestData) -> v3::KvRequestData {
	match data {
		v2::KvRequestData::KvGetRequest(req) => {
			v3::KvRequestData::KvGetRequest(v3::KvGetRequest { keys: req.keys })
		}
		v2::KvRequestData::KvListRequest(req) => {
			v3::KvRequestData::KvListRequest(v3::KvListRequest {
				query: convert_kv_list_query_v2_to_v3(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v2::KvRequestData::KvPutRequest(req) => v3::KvRequestData::KvPutRequest(v3::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v2::KvRequestData::KvDeleteRequest(req) => {
			v3::KvRequestData::KvDeleteRequest(v3::KvDeleteRequest { keys: req.keys })
		}
		v2::KvRequestData::KvDropRequest => v3::KvRequestData::KvDropRequest,
	}
}

fn convert_kv_request_data_v3_to_v2(data: v3::KvRequestData) -> v2::KvRequestData {
	match data {
		v3::KvRequestData::KvGetRequest(req) => {
			v2::KvRequestData::KvGetRequest(v2::KvGetRequest { keys: req.keys })
		}
		v3::KvRequestData::KvListRequest(req) => {
			v2::KvRequestData::KvListRequest(v2::KvListRequest {
				query: convert_kv_list_query_v3_to_v2(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v3::KvRequestData::KvPutRequest(req) => v2::KvRequestData::KvPutRequest(v2::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v3::KvRequestData::KvDeleteRequest(req) => {
			v2::KvRequestData::KvDeleteRequest(v2::KvDeleteRequest { keys: req.keys })
		}
		v3::KvRequestData::KvDropRequest => v2::KvRequestData::KvDropRequest,
	}
}

fn convert_kv_response_data_v2_to_v3(data: v2::KvResponseData) -> v3::KvResponseData {
	match data {
		v2::KvResponseData::KvErrorResponse(err) => {
			v3::KvResponseData::KvErrorResponse(v3::KvErrorResponse {
				message: err.message,
			})
		}
		v2::KvResponseData::KvGetResponse(resp) => {
			v3::KvResponseData::KvGetResponse(v3::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v2_to_v3)
					.collect(),
			})
		}
		v2::KvResponseData::KvListResponse(resp) => {
			v3::KvResponseData::KvListResponse(v3::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v2_to_v3)
					.collect(),
			})
		}
		v2::KvResponseData::KvPutResponse => v3::KvResponseData::KvPutResponse,
		v2::KvResponseData::KvDeleteResponse => v3::KvResponseData::KvDeleteResponse,
		v2::KvResponseData::KvDropResponse => v3::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_response_data_v3_to_v2(data: v3::KvResponseData) -> v2::KvResponseData {
	match data {
		v3::KvResponseData::KvErrorResponse(err) => {
			v2::KvResponseData::KvErrorResponse(v2::KvErrorResponse {
				message: err.message,
			})
		}
		v3::KvResponseData::KvGetResponse(resp) => {
			v2::KvResponseData::KvGetResponse(v2::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v3_to_v2)
					.collect(),
			})
		}
		v3::KvResponseData::KvListResponse(resp) => {
			v2::KvResponseData::KvListResponse(v2::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v3_to_v2)
					.collect(),
			})
		}
		v3::KvResponseData::KvPutResponse => v2::KvResponseData::KvPutResponse,
		v3::KvResponseData::KvDeleteResponse => v2::KvResponseData::KvDeleteResponse,
		v3::KvResponseData::KvDropResponse => v2::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_list_query_v2_to_v3(query: v2::KvListQuery) -> v3::KvListQuery {
	match query {
		v2::KvListQuery::KvListAllQuery => v3::KvListQuery::KvListAllQuery,
		v2::KvListQuery::KvListRangeQuery(range) => {
			v3::KvListQuery::KvListRangeQuery(v3::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v2::KvListQuery::KvListPrefixQuery(prefix) => {
			v3::KvListQuery::KvListPrefixQuery(v3::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_list_query_v3_to_v2(query: v3::KvListQuery) -> v2::KvListQuery {
	match query {
		v3::KvListQuery::KvListAllQuery => v2::KvListQuery::KvListAllQuery,
		v3::KvListQuery::KvListRangeQuery(range) => {
			v2::KvListQuery::KvListRangeQuery(v2::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v3::KvListQuery::KvListPrefixQuery(prefix) => {
			v2::KvListQuery::KvListPrefixQuery(v2::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_metadata_v2_to_v3(metadata: v2::KvMetadata) -> v3::KvMetadata {
	v3::KvMetadata {
		version: metadata.version,
		create_ts: metadata.create_ts,
	}
}

fn convert_kv_metadata_v3_to_v2(metadata: v3::KvMetadata) -> v2::KvMetadata {
	v2::KvMetadata {
		version: metadata.version,
		create_ts: metadata.create_ts,
	}
}

fn convert_to_server_tunnel_message_kind_v3_to_v4(
	kind: v3::ToServerTunnelMessageKind,
) -> v6::ToServerTunnelMessageKind {
	match kind {
		v3::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v6::ToServerTunnelMessageKind::ToServerResponseStart(v6::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v6::ToServerTunnelMessageKind::ToServerResponseChunk(v6::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v6::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(v6::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(v6::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v6::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v3::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketClose(v6::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.hibernate,
			})
		}
		v3::ToServerTunnelMessageKind::DeprecatedTunnelAck => {
			// v4 removed DeprecatedTunnelAck, this should not occur in practice
			// but if it does, we'll convert it to a response abort as a safe fallback
			v6::ToServerTunnelMessageKind::ToServerResponseAbort
		}
	}
}

fn convert_to_server_tunnel_message_kind_v4_to_v3(
	kind: v6::ToServerTunnelMessageKind,
) -> Result<v3::ToServerTunnelMessageKind> {
	Ok(match kind {
		v6::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v3::ToServerTunnelMessageKind::ToServerResponseStart(v3::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v3::ToServerTunnelMessageKind::ToServerResponseChunk(v3::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v3::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketOpen(v3::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketMessage(v3::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v3::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v3::ToServerTunnelMessageKind::ToServerWebSocketClose(v3::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.hibernate,
			})
		}
	})
}

// Used specifically for the gateway because there were no changes between mk2 and mk1 for the tunnel messages
pub fn to_client_tunnel_message_mk2_to_mk1(
	msg: v7::ToClientTunnelMessage,
) -> v3::ToClientTunnelMessage {
	v3::ToClientTunnelMessage {
		message_id: v3::MessageId {
			gateway_id: msg.message_id.gateway_id,
			request_id: msg.message_id.request_id,
			message_index: msg.message_id.message_index,
		},
		message_kind: convert_to_client_tunnel_message_kind_mk2_to_mk1(msg.message_kind),
	}
}

fn convert_to_client_tunnel_message_kind_mk2_to_mk1(
	kind: v7::ToClientTunnelMessageKind,
) -> v3::ToClientTunnelMessageKind {
	match kind {
		v7::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v3::ToClientTunnelMessageKind::ToClientRequestStart(v3::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v3::ToClientTunnelMessageKind::ToClientRequestChunk(v3::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v3::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v7::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v3::ToClientTunnelMessageKind::ToClientWebSocketOpen(v3::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v3::ToClientTunnelMessageKind::ToClientWebSocketMessage(v3::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v3::ToClientTunnelMessageKind::ToClientWebSocketClose(v3::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_kv_response_data_v4_to_v5(data: v4::KvResponseData) -> v5::KvResponseData {
	match data {
		v4::KvResponseData::KvErrorResponse(err) => {
			v5::KvResponseData::KvErrorResponse(v5::KvErrorResponse {
				message: err.message,
			})
		}
		v4::KvResponseData::KvGetResponse(resp) => {
			v5::KvResponseData::KvGetResponse(v5::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v4_to_v5)
					.collect(),
			})
		}
		v4::KvResponseData::KvListResponse(resp) => {
			v5::KvResponseData::KvListResponse(v5::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v4_to_v5)
					.collect(),
			})
		}
		v4::KvResponseData::KvPutResponse => v5::KvResponseData::KvPutResponse,
		v4::KvResponseData::KvDeleteResponse => v5::KvResponseData::KvDeleteResponse,
		v4::KvResponseData::KvDropResponse => v5::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_response_data_v5_to_v4(data: v5::KvResponseData) -> v4::KvResponseData {
	match data {
		v5::KvResponseData::KvErrorResponse(err) => {
			v4::KvResponseData::KvErrorResponse(v4::KvErrorResponse {
				message: err.message,
			})
		}
		v5::KvResponseData::KvGetResponse(resp) => {
			v4::KvResponseData::KvGetResponse(v4::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v5_to_v4)
					.collect(),
			})
		}
		v5::KvResponseData::KvListResponse(resp) => {
			v4::KvResponseData::KvListResponse(v4::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v5_to_v4)
					.collect(),
			})
		}
		v5::KvResponseData::KvPutResponse => v4::KvResponseData::KvPutResponse,
		v5::KvResponseData::KvDeleteResponse => v4::KvResponseData::KvDeleteResponse,
		v5::KvResponseData::KvDropResponse => v4::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_metadata_v4_to_v5(metadata: v4::KvMetadata) -> v5::KvMetadata {
	v5::KvMetadata {
		version: metadata.version,
		update_ts: metadata.update_ts,
	}
}

fn convert_kv_metadata_v5_to_v4(metadata: v5::KvMetadata) -> v4::KvMetadata {
	v4::KvMetadata {
		version: metadata.version,
		update_ts: metadata.update_ts,
	}
}

fn convert_to_client_tunnel_message_kind_v4_to_v5(
	kind: v4::ToClientTunnelMessageKind,
) -> v5::ToClientTunnelMessageKind {
	match kind {
		v4::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v5::ToClientTunnelMessageKind::ToClientRequestStart(v5::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v5::ToClientTunnelMessageKind::ToClientRequestChunk(v5::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v5::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v4::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v5::ToClientTunnelMessageKind::ToClientWebSocketOpen(v5::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v5::ToClientTunnelMessageKind::ToClientWebSocketMessage(v5::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v5::ToClientTunnelMessageKind::ToClientWebSocketClose(v5::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_client_tunnel_message_kind_v5_to_v4(
	kind: v5::ToClientTunnelMessageKind,
) -> v4::ToClientTunnelMessageKind {
	match kind {
		v5::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v4::ToClientTunnelMessageKind::ToClientRequestStart(v4::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v4::ToClientTunnelMessageKind::ToClientRequestChunk(v4::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v4::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v5::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v4::ToClientTunnelMessageKind::ToClientWebSocketOpen(v4::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v4::ToClientTunnelMessageKind::ToClientWebSocketMessage(v4::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v4::ToClientTunnelMessageKind::ToClientWebSocketClose(v4::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

// MARK: v4 <-> v6 helpers (ToServer and ToRunner; v5 and v6 are structurally identical)

fn convert_actor_intent_v4_to_v6(intent: v4::ActorIntent) -> v6::ActorIntent {
	match intent {
		v4::ActorIntent::ActorIntentSleep => v6::ActorIntent::ActorIntentSleep,
		v4::ActorIntent::ActorIntentStop => v6::ActorIntent::ActorIntentStop,
	}
}

fn convert_actor_intent_v6_to_v4(intent: v6::ActorIntent) -> v4::ActorIntent {
	match intent {
		v6::ActorIntent::ActorIntentSleep => v4::ActorIntent::ActorIntentSleep,
		v6::ActorIntent::ActorIntentStop => v4::ActorIntent::ActorIntentStop,
	}
}

fn convert_actor_state_v4_to_v6(state: v4::ActorState) -> v6::ActorState {
	match state {
		v4::ActorState::ActorStateRunning => v6::ActorState::ActorStateRunning,
		v4::ActorState::ActorStateStopped(stopped) => {
			v6::ActorState::ActorStateStopped(v6::ActorStateStopped {
				code: convert_stop_code_v4_to_v6(stopped.code),
				message: stopped.message,
			})
		}
	}
}

fn convert_actor_state_v6_to_v4(state: v6::ActorState) -> v4::ActorState {
	match state {
		v6::ActorState::ActorStateRunning => v4::ActorState::ActorStateRunning,
		v6::ActorState::ActorStateStopped(stopped) => {
			v4::ActorState::ActorStateStopped(v4::ActorStateStopped {
				code: convert_stop_code_v6_to_v4(stopped.code),
				message: stopped.message,
			})
		}
	}
}

fn convert_stop_code_v4_to_v6(code: v4::StopCode) -> v6::StopCode {
	match code {
		v4::StopCode::Ok => v6::StopCode::Ok,
		v4::StopCode::Error => v6::StopCode::Error,
	}
}

fn convert_stop_code_v6_to_v4(code: v6::StopCode) -> v4::StopCode {
	match code {
		v6::StopCode::Ok => v4::StopCode::Ok,
		v6::StopCode::Error => v4::StopCode::Error,
	}
}

fn convert_kv_request_data_v4_to_v6(data: v4::KvRequestData) -> v6::KvRequestData {
	match data {
		v4::KvRequestData::KvGetRequest(req) => {
			v6::KvRequestData::KvGetRequest(v6::KvGetRequest { keys: req.keys })
		}
		v4::KvRequestData::KvListRequest(req) => {
			v6::KvRequestData::KvListRequest(v6::KvListRequest {
				query: convert_kv_list_query_v4_to_v6(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v4::KvRequestData::KvPutRequest(req) => v6::KvRequestData::KvPutRequest(v6::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v4::KvRequestData::KvDeleteRequest(req) => {
			v6::KvRequestData::KvDeleteRequest(v6::KvDeleteRequest { keys: req.keys })
		}
		v4::KvRequestData::KvDropRequest => v6::KvRequestData::KvDropRequest,
	}
}

fn convert_kv_request_data_v6_to_v4(data: v6::KvRequestData) -> v4::KvRequestData {
	match data {
		v6::KvRequestData::KvGetRequest(req) => {
			v4::KvRequestData::KvGetRequest(v4::KvGetRequest { keys: req.keys })
		}
		v6::KvRequestData::KvListRequest(req) => {
			v4::KvRequestData::KvListRequest(v4::KvListRequest {
				query: convert_kv_list_query_v6_to_v4(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v6::KvRequestData::KvPutRequest(req) => v4::KvRequestData::KvPutRequest(v4::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v6::KvRequestData::KvDeleteRequest(req) => {
			v4::KvRequestData::KvDeleteRequest(v4::KvDeleteRequest { keys: req.keys })
		}
		v6::KvRequestData::KvDropRequest => v4::KvRequestData::KvDropRequest,
	}
}

// MARK: v6 <-> v7 helpers (KvDeleteRangeRequest was introduced in v7)

fn convert_kv_request_data_v6_to_v7(data: v6::KvRequestData) -> v7::KvRequestData {
	match data {
		v6::KvRequestData::KvGetRequest(req) => {
			v7::KvRequestData::KvGetRequest(v7::KvGetRequest { keys: req.keys })
		}
		v6::KvRequestData::KvListRequest(req) => {
			v7::KvRequestData::KvListRequest(v7::KvListRequest {
				query: convert_kv_list_query_v6_to_v7(req.query),
				reverse: req.reverse,
				limit: req.limit,
			})
		}
		v6::KvRequestData::KvPutRequest(req) => v7::KvRequestData::KvPutRequest(v7::KvPutRequest {
			keys: req.keys,
			values: req.values,
		}),
		v6::KvRequestData::KvDeleteRequest(req) => {
			v7::KvRequestData::KvDeleteRequest(v7::KvDeleteRequest { keys: req.keys })
		}
		v6::KvRequestData::KvDropRequest => v7::KvRequestData::KvDropRequest,
	}
}

fn convert_kv_request_data_v7_to_v6(data: v7::KvRequestData) -> Result<v6::KvRequestData> {
	match data {
		v7::KvRequestData::KvGetRequest(req) => {
			Ok(v6::KvRequestData::KvGetRequest(v6::KvGetRequest {
				keys: req.keys,
			}))
		}
		v7::KvRequestData::KvListRequest(req) => {
			Ok(v6::KvRequestData::KvListRequest(v6::KvListRequest {
				query: convert_kv_list_query_v7_to_v6(req.query),
				reverse: req.reverse,
				limit: req.limit,
			}))
		}
		v7::KvRequestData::KvPutRequest(req) => {
			Ok(v6::KvRequestData::KvPutRequest(v6::KvPutRequest {
				keys: req.keys,
				values: req.values,
			}))
		}
		v7::KvRequestData::KvDeleteRequest(req) => {
			Ok(v6::KvRequestData::KvDeleteRequest(v6::KvDeleteRequest {
				keys: req.keys,
			}))
		}
		v7::KvRequestData::KvDeleteRangeRequest(_) => {
			bail!("KvDeleteRangeRequest requires runner protocol v7")
		}
		v7::KvRequestData::KvDropRequest => Ok(v6::KvRequestData::KvDropRequest),
	}
}

fn convert_kv_list_query_v4_to_v6(query: v4::KvListQuery) -> v6::KvListQuery {
	match query {
		v4::KvListQuery::KvListAllQuery => v6::KvListQuery::KvListAllQuery,
		v4::KvListQuery::KvListRangeQuery(range) => {
			v6::KvListQuery::KvListRangeQuery(v6::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v4::KvListQuery::KvListPrefixQuery(prefix) => {
			v6::KvListQuery::KvListPrefixQuery(v6::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_list_query_v6_to_v4(query: v6::KvListQuery) -> v4::KvListQuery {
	match query {
		v6::KvListQuery::KvListAllQuery => v4::KvListQuery::KvListAllQuery,
		v6::KvListQuery::KvListRangeQuery(range) => {
			v4::KvListQuery::KvListRangeQuery(v4::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v6::KvListQuery::KvListPrefixQuery(prefix) => {
			v4::KvListQuery::KvListPrefixQuery(v4::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_list_query_v6_to_v7(query: v6::KvListQuery) -> v7::KvListQuery {
	match query {
		v6::KvListQuery::KvListAllQuery => v7::KvListQuery::KvListAllQuery,
		v6::KvListQuery::KvListRangeQuery(range) => {
			v7::KvListQuery::KvListRangeQuery(v7::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v6::KvListQuery::KvListPrefixQuery(prefix) => {
			v7::KvListQuery::KvListPrefixQuery(v7::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_kv_list_query_v7_to_v6(query: v7::KvListQuery) -> v6::KvListQuery {
	match query {
		v7::KvListQuery::KvListAllQuery => v6::KvListQuery::KvListAllQuery,
		v7::KvListQuery::KvListRangeQuery(range) => {
			v6::KvListQuery::KvListRangeQuery(v6::KvListRangeQuery {
				start: range.start,
				end: range.end,
				exclusive: range.exclusive,
			})
		}
		v7::KvListQuery::KvListPrefixQuery(prefix) => {
			v6::KvListQuery::KvListPrefixQuery(v6::KvListPrefixQuery { key: prefix.key })
		}
	}
}

fn convert_to_client_tunnel_message_kind_v4_to_v6(
	kind: v4::ToClientTunnelMessageKind,
) -> v6::ToClientTunnelMessageKind {
	match kind {
		v4::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v6::ToClientTunnelMessageKind::ToClientRequestStart(v6::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v6::ToClientTunnelMessageKind::ToClientRequestChunk(v6::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v6::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v4::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketOpen(v6::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketMessage(v6::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v4::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketClose(v6::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_client_tunnel_message_kind_v6_to_v4(
	kind: v6::ToClientTunnelMessageKind,
) -> v4::ToClientTunnelMessageKind {
	match kind {
		v6::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v4::ToClientTunnelMessageKind::ToClientRequestStart(v4::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v4::ToClientTunnelMessageKind::ToClientRequestChunk(v4::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v4::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v4::ToClientTunnelMessageKind::ToClientWebSocketOpen(v4::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v4::ToClientTunnelMessageKind::ToClientWebSocketMessage(v4::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v4::ToClientTunnelMessageKind::ToClientWebSocketClose(v4::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_server_tunnel_message_kind_v4_to_v6(
	kind: v4::ToServerTunnelMessageKind,
) -> v6::ToServerTunnelMessageKind {
	match kind {
		v4::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v6::ToServerTunnelMessageKind::ToServerResponseStart(v6::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v4::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v6::ToServerTunnelMessageKind::ToServerResponseChunk(v6::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v4::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v6::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v4::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(v6::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v4::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(v6::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v4::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v6::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v4::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketClose(v6::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.hibernate,
			})
		}
	}
}

fn convert_to_server_tunnel_message_kind_v6_to_v4(
	kind: v6::ToServerTunnelMessageKind,
) -> Result<v4::ToServerTunnelMessageKind> {
	Ok(match kind {
		v6::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v4::ToServerTunnelMessageKind::ToServerResponseStart(v4::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v4::ToServerTunnelMessageKind::ToServerResponseChunk(v4::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v4::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v4::ToServerTunnelMessageKind::ToServerWebSocketOpen(v4::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v4::ToServerTunnelMessageKind::ToServerWebSocketMessage(v4::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v4::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v4::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v4::ToServerTunnelMessageKind::ToServerWebSocketClose(v4::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.hibernate,
			})
		}
	})
}

// MARK: v5 <-> v6 helpers (ToClient; only ProtocolMetadata changed, other types are identical)

fn convert_kv_response_data_v5_to_v6(data: v5::KvResponseData) -> v6::KvResponseData {
	match data {
		v5::KvResponseData::KvErrorResponse(err) => {
			v6::KvResponseData::KvErrorResponse(v6::KvErrorResponse {
				message: err.message,
			})
		}
		v5::KvResponseData::KvGetResponse(resp) => {
			v6::KvResponseData::KvGetResponse(v6::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v5_to_v6)
					.collect(),
			})
		}
		v5::KvResponseData::KvListResponse(resp) => {
			v6::KvResponseData::KvListResponse(v6::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v5_to_v6)
					.collect(),
			})
		}
		v5::KvResponseData::KvPutResponse => v6::KvResponseData::KvPutResponse,
		v5::KvResponseData::KvDeleteResponse => v6::KvResponseData::KvDeleteResponse,
		v5::KvResponseData::KvDropResponse => v6::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_response_data_v6_to_v5(data: v6::KvResponseData) -> v5::KvResponseData {
	match data {
		v6::KvResponseData::KvErrorResponse(err) => {
			v5::KvResponseData::KvErrorResponse(v5::KvErrorResponse {
				message: err.message,
			})
		}
		v6::KvResponseData::KvGetResponse(resp) => {
			v5::KvResponseData::KvGetResponse(v5::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v6_to_v5)
					.collect(),
			})
		}
		v6::KvResponseData::KvListResponse(resp) => {
			v5::KvResponseData::KvListResponse(v5::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v6_to_v5)
					.collect(),
			})
		}
		v6::KvResponseData::KvPutResponse => v5::KvResponseData::KvPutResponse,
		v6::KvResponseData::KvDeleteResponse => v5::KvResponseData::KvDeleteResponse,
		v6::KvResponseData::KvDropResponse => v5::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_metadata_v5_to_v6(metadata: v5::KvMetadata) -> v6::KvMetadata {
	v6::KvMetadata {
		version: metadata.version,
		update_ts: metadata.update_ts,
	}
}

fn convert_kv_metadata_v6_to_v5(metadata: v6::KvMetadata) -> v5::KvMetadata {
	v5::KvMetadata {
		version: metadata.version,
		update_ts: metadata.update_ts,
	}
}

fn convert_kv_response_data_v5_to_v7(data: v5::KvResponseData) -> v7::KvResponseData {
	convert_kv_response_data_v6_to_v7(convert_kv_response_data_v5_to_v6(data))
}

fn convert_kv_response_data_v7_to_v5(data: v7::KvResponseData) -> v5::KvResponseData {
	convert_kv_response_data_v6_to_v5(convert_kv_response_data_v7_to_v6(data))
}

fn convert_kv_response_data_v6_to_v7(data: v6::KvResponseData) -> v7::KvResponseData {
	match data {
		v6::KvResponseData::KvErrorResponse(err) => {
			v7::KvResponseData::KvErrorResponse(v7::KvErrorResponse {
				message: err.message,
			})
		}
		v6::KvResponseData::KvGetResponse(resp) => {
			v7::KvResponseData::KvGetResponse(v7::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v6_to_v7)
					.collect(),
			})
		}
		v6::KvResponseData::KvListResponse(resp) => {
			v7::KvResponseData::KvListResponse(v7::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v6_to_v7)
					.collect(),
			})
		}
		v6::KvResponseData::KvPutResponse => v7::KvResponseData::KvPutResponse,
		v6::KvResponseData::KvDeleteResponse => v7::KvResponseData::KvDeleteResponse,
		v6::KvResponseData::KvDropResponse => v7::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_response_data_v7_to_v6(data: v7::KvResponseData) -> v6::KvResponseData {
	match data {
		v7::KvResponseData::KvErrorResponse(err) => {
			v6::KvResponseData::KvErrorResponse(v6::KvErrorResponse {
				message: err.message,
			})
		}
		v7::KvResponseData::KvGetResponse(resp) => {
			v6::KvResponseData::KvGetResponse(v6::KvGetResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v7_to_v6)
					.collect(),
			})
		}
		v7::KvResponseData::KvListResponse(resp) => {
			v6::KvResponseData::KvListResponse(v6::KvListResponse {
				keys: resp.keys,
				values: resp.values,
				metadata: resp
					.metadata
					.into_iter()
					.map(convert_kv_metadata_v7_to_v6)
					.collect(),
			})
		}
		v7::KvResponseData::KvPutResponse => v6::KvResponseData::KvPutResponse,
		v7::KvResponseData::KvDeleteResponse => v6::KvResponseData::KvDeleteResponse,
		v7::KvResponseData::KvDropResponse => v6::KvResponseData::KvDropResponse,
	}
}

fn convert_kv_metadata_v6_to_v7(metadata: v6::KvMetadata) -> v7::KvMetadata {
	v7::KvMetadata {
		version: metadata.version,
		update_ts: metadata.update_ts,
	}
}

fn convert_kv_metadata_v7_to_v6(metadata: v7::KvMetadata) -> v6::KvMetadata {
	v6::KvMetadata {
		version: metadata.version,
		update_ts: metadata.update_ts,
	}
}

fn convert_to_client_tunnel_message_kind_v5_to_v6(
	kind: v5::ToClientTunnelMessageKind,
) -> v6::ToClientTunnelMessageKind {
	match kind {
		v5::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v6::ToClientTunnelMessageKind::ToClientRequestStart(v6::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v6::ToClientTunnelMessageKind::ToClientRequestChunk(v6::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v6::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v5::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketOpen(v6::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketMessage(v6::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v5::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketClose(v6::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_client_tunnel_message_kind_v6_to_v5(
	kind: v6::ToClientTunnelMessageKind,
) -> v5::ToClientTunnelMessageKind {
	match kind {
		v6::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v5::ToClientTunnelMessageKind::ToClientRequestStart(v5::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v5::ToClientTunnelMessageKind::ToClientRequestChunk(v5::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v5::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v5::ToClientTunnelMessageKind::ToClientWebSocketOpen(v5::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v5::ToClientTunnelMessageKind::ToClientWebSocketMessage(v5::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v5::ToClientTunnelMessageKind::ToClientWebSocketClose(v5::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_client_tunnel_message_kind_v5_to_v7(
	kind: v5::ToClientTunnelMessageKind,
) -> v7::ToClientTunnelMessageKind {
	convert_to_client_tunnel_message_kind_v6_to_v7(convert_to_client_tunnel_message_kind_v5_to_v6(
		kind,
	))
}

fn convert_to_client_tunnel_message_kind_v7_to_v5(
	kind: v7::ToClientTunnelMessageKind,
) -> v5::ToClientTunnelMessageKind {
	convert_to_client_tunnel_message_kind_v6_to_v5(convert_to_client_tunnel_message_kind_v7_to_v6(
		kind,
	))
}

fn convert_to_client_tunnel_message_kind_v4_to_v7(
	kind: v4::ToClientTunnelMessageKind,
) -> v7::ToClientTunnelMessageKind {
	convert_to_client_tunnel_message_kind_v6_to_v7(convert_to_client_tunnel_message_kind_v4_to_v6(
		kind,
	))
}

fn convert_to_client_tunnel_message_kind_v7_to_v4(
	kind: v7::ToClientTunnelMessageKind,
) -> v4::ToClientTunnelMessageKind {
	convert_to_client_tunnel_message_kind_v6_to_v4(convert_to_client_tunnel_message_kind_v7_to_v6(
		kind,
	))
}

fn convert_to_client_tunnel_message_kind_v6_to_v7(
	kind: v6::ToClientTunnelMessageKind,
) -> v7::ToClientTunnelMessageKind {
	match kind {
		v6::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v7::ToClientTunnelMessageKind::ToClientRequestStart(v7::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v7::ToClientTunnelMessageKind::ToClientRequestChunk(v7::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v7::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v7::ToClientTunnelMessageKind::ToClientWebSocketOpen(v7::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v7::ToClientTunnelMessageKind::ToClientWebSocketMessage(v7::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v6::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v7::ToClientTunnelMessageKind::ToClientWebSocketClose(v7::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_client_tunnel_message_kind_v7_to_v6(
	kind: v7::ToClientTunnelMessageKind,
) -> v6::ToClientTunnelMessageKind {
	match kind {
		v7::ToClientTunnelMessageKind::ToClientRequestStart(req) => {
			v6::ToClientTunnelMessageKind::ToClientRequestStart(v6::ToClientRequestStart {
				actor_id: req.actor_id,
				method: req.method,
				path: req.path,
				headers: req.headers,
				body: req.body,
				stream: req.stream,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientRequestChunk(chunk) => {
			v6::ToClientTunnelMessageKind::ToClientRequestChunk(v6::ToClientRequestChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientRequestAbort => {
			v6::ToClientTunnelMessageKind::ToClientRequestAbort
		}
		v7::ToClientTunnelMessageKind::ToClientWebSocketOpen(ws) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketOpen(v6::ToClientWebSocketOpen {
				actor_id: ws.actor_id,
				path: ws.path,
				headers: ws.headers,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientWebSocketMessage(msg) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketMessage(v6::ToClientWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v7::ToClientTunnelMessageKind::ToClientWebSocketClose(close) => {
			v6::ToClientTunnelMessageKind::ToClientWebSocketClose(v6::ToClientWebSocketClose {
				code: close.code,
				reason: close.reason,
			})
		}
	}
}

fn convert_to_server_tunnel_message_kind_v6_to_v7(
	kind: v6::ToServerTunnelMessageKind,
) -> v7::ToServerTunnelMessageKind {
	match kind {
		v6::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v7::ToServerTunnelMessageKind::ToServerResponseStart(v7::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v7::ToServerTunnelMessageKind::ToServerResponseChunk(v7::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v7::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v7::ToServerTunnelMessageKind::ToServerWebSocketOpen(v7::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v7::ToServerTunnelMessageKind::ToServerWebSocketMessage(v7::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v7::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v7::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v6::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v7::ToServerTunnelMessageKind::ToServerWebSocketClose(v7::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.hibernate,
			})
		}
	}
}

fn convert_to_server_tunnel_message_kind_v7_to_v6(
	kind: v7::ToServerTunnelMessageKind,
) -> v6::ToServerTunnelMessageKind {
	match kind {
		v7::ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			v6::ToServerTunnelMessageKind::ToServerResponseStart(v6::ToServerResponseStart {
				status: resp.status,
				headers: resp.headers,
				body: resp.body,
				stream: resp.stream,
			})
		}
		v7::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => {
			v6::ToServerTunnelMessageKind::ToServerResponseChunk(v6::ToServerResponseChunk {
				body: chunk.body,
				finish: chunk.finish,
			})
		}
		v7::ToServerTunnelMessageKind::ToServerResponseAbort => {
			v6::ToServerTunnelMessageKind::ToServerResponseAbort
		}
		v7::ToServerTunnelMessageKind::ToServerWebSocketOpen(open) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketOpen(v6::ToServerWebSocketOpen {
				can_hibernate: open.can_hibernate,
			})
		}
		v7::ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketMessage(v6::ToServerWebSocketMessage {
				data: msg.data,
				binary: msg.binary,
			})
		}
		v7::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(ack) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketMessageAck(
				v6::ToServerWebSocketMessageAck { index: ack.index },
			)
		}
		v7::ToServerTunnelMessageKind::ToServerWebSocketClose(close) => {
			v6::ToServerTunnelMessageKind::ToServerWebSocketClose(v6::ToServerWebSocketClose {
				code: close.code,
				reason: close.reason,
				hibernate: close.hibernate,
			})
		}
	}
}
