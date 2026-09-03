use anyhow::Result;
use vbare::OwnedVersionedData;

use super::{ToEnvoy, ToRivet};
use crate::generated::v6;

fn assert_to_envoy_round_trip(message: v6::ToEnvoy) -> Result<()> {
	let original = serde_bare::to_vec(&message)?;
	let latest = ToEnvoy::deserialize(&original, 6)?;
	let converted = ToEnvoy::wrap_latest(latest).serialize(6)?;
	assert_eq!(converted, original);
	Ok(())
}

fn assert_to_rivet_round_trip(message: v6::ToRivet) -> Result<()> {
	let original = serde_bare::to_vec(&message)?;
	let latest = ToRivet::deserialize(&original, 6)?;
	let converted = ToRivet::wrap_latest(latest).serialize(6)?;
	assert_eq!(converted, original);
	Ok(())
}

fn message_id() -> v6::MessageId {
	v6::MessageId {
		gateway_id: [1, 2, 3, 4],
		request_id: [5, 6, 7, 8],
		message_index: 9,
	}
}

#[test]
fn v6_buffered_response_round_trips_through_v7() -> Result<()> {
	assert_to_rivet_round_trip(v6::ToRivet::ToRivetTunnelMessage(
		v6::ToRivetTunnelMessage {
			message_id: message_id(),
			message_kind: v6::ToRivetTunnelMessageKind::ToRivetResponseStart(
				v6::ToRivetResponseStart {
					status: 202,
					headers: [("content-type".to_owned(), "text/plain".to_owned())]
						.into_iter()
						.collect(),
					body: Some(b"buffered".to_vec()),
					stream: false,
				},
			),
		},
	))
}

#[test]
fn v6_websocket_round_trips_through_v7() -> Result<()> {
	assert_to_envoy_round_trip(v6::ToEnvoy::ToEnvoyTunnelMessage(
		v6::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: v6::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketMessage(
				v6::ToEnvoyWebSocketMessage {
					data: vec![1, 3, 3, 7],
					binary: true,
				},
			),
		},
	))
}

#[test]
fn v6_websocket_open_round_trips_without_generation() -> Result<()> {
	assert_to_envoy_round_trip(v6::ToEnvoy::ToEnvoyTunnelMessage(
		v6::ToEnvoyTunnelMessage {
			message_id: message_id(),
			message_kind: v6::ToEnvoyTunnelMessageKind::ToEnvoyWebSocketOpen(
				v6::ToEnvoyWebSocketOpen {
					actor_id: "actor".to_owned(),
					path: "/socket".to_owned(),
					headers: Default::default(),
				},
			),
		},
	))
}

#[test]
fn v6_lifecycle_event_round_trips_through_v7() -> Result<()> {
	assert_to_rivet_round_trip(v6::ToRivet::ToRivetEvents(vec![v6::EventWrapper {
		checkpoint: v6::ActorCheckpoint {
			actor_id: "actor".to_owned(),
			generation: 3,
			index: 11,
		},
		inner: v6::Event::EventActorStateUpdate(v6::EventActorStateUpdate {
			state: v6::ActorState::ActorStateStopped(v6::ActorStateStopped {
				code: v6::StopCode::Error,
				message: Some("stopped".to_owned()),
			}),
		}),
	}]))
}

#[test]
fn v6_kv_request_round_trips_through_v7() -> Result<()> {
	assert_to_rivet_round_trip(v6::ToRivet::ToRivetKvRequest(v6::ToRivetKvRequest {
		actor_id: "actor".to_owned(),
		request_id: 12,
		data: v6::KvRequestData::KvPutRequest(v6::KvPutRequest {
			keys: vec![b"key".to_vec()],
			values: vec![b"value".to_vec()],
		}),
	}))
}

#[test]
fn v6_sqlite_request_round_trips_through_v7() -> Result<()> {
	assert_to_rivet_round_trip(v6::ToRivet::ToRivetSqliteCommitRequest(
		v6::ToRivetSqliteCommitRequest {
			request_id: 13,
			data: v6::SqliteCommitRequest {
				actor_id: "actor".to_owned(),
				dirty_pages: vec![v6::SqliteDirtyPage {
					pgno: 1,
					bytes: vec![4, 2],
				}],
				db_size_pages: 1,
				now_ms: 14,
				expected_generation: Some(3),
				expected_head_txid: Some(15),
			},
		},
	))
}
