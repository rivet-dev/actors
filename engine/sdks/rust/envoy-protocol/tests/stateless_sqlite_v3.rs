use std::path::Path;

use rivet_envoy_protocol::{self as protocol, PROTOCOL_VERSION, versioned};
use vbare::OwnedVersionedData;

fn roundtrip_to_rivet(message: protocol::ToRivet) -> anyhow::Result<protocol::ToRivet> {
	let encoded = versioned::ToRivet::wrap_latest(message)
		.serialize_with_embedded_version(PROTOCOL_VERSION)?;
	versioned::ToRivet::deserialize_with_embedded_version(&encoded)
}

fn roundtrip_to_envoy(message: protocol::ToEnvoy) -> anyhow::Result<protocol::ToEnvoy> {
	let encoded = versioned::ToEnvoy::wrap_latest(message)
		.serialize_with_embedded_version(PROTOCOL_VERSION)?;
	versioned::ToEnvoy::deserialize_with_embedded_version(&encoded)
}

#[test]
fn get_pages_request_roundtrip() -> anyhow::Result<()> {
	for pgnos in [Vec::new(), vec![7], (1..=1000).collect::<Vec<_>>()] {
		let decoded = roundtrip_to_rivet(protocol::ToRivet::ToRivetSqliteGetPagesRequest(
			protocol::ToRivetSqliteGetPagesRequest {
				request_id: 42,
				data: protocol::SqliteGetPagesRequest {
					actor_id: "actor-a".into(),
					pgnos: pgnos.clone(),
					expected_generation: None,
					expected_head_txid: None,
				},
			},
		))?;

		let protocol::ToRivet::ToRivetSqliteGetPagesRequest(decoded) = decoded else {
			panic!("expected get_pages request");
		};
		assert_eq!(decoded.request_id, 42);
		assert_eq!(decoded.data.actor_id, "actor-a");
		assert_eq!(decoded.data.pgnos, pgnos);
		assert_eq!(decoded.data.expected_generation, None);
		assert_eq!(decoded.data.expected_head_txid, None);
	}

	Ok(())
}

#[test]
fn commit_request_roundtrip() -> anyhow::Result<()> {
	for (dirty_pages, db_size_pages, now_ms) in [
		(Vec::new(), 1, 0),
		(vec![dirty_page(1, 1)], 5, 1234),
		(
			(1..=1000).map(|pgno| dirty_page(pgno, 9)).collect(),
			1000,
			i64::MAX - 7,
		),
	] {
		let decoded = roundtrip_to_rivet(protocol::ToRivet::ToRivetSqliteCommitRequest(
			protocol::ToRivetSqliteCommitRequest {
				request_id: 9,
				data: protocol::SqliteCommitRequest {
					actor_id: "actor-b".into(),
					dirty_pages: dirty_pages.clone(),
					db_size_pages,
					now_ms,
					expected_generation: None,
					expected_head_txid: None,
				},
			},
		))?;

		let protocol::ToRivet::ToRivetSqliteCommitRequest(decoded) = decoded else {
			panic!("expected commit request");
		};
		assert_eq!(decoded.request_id, 9);
		assert_eq!(decoded.data.actor_id, "actor-b");
		assert_eq!(decoded.data.dirty_pages, dirty_pages);
		assert_eq!(decoded.data.db_size_pages, db_size_pages);
		assert_eq!(decoded.data.now_ms, now_ms);
	}

	Ok(())
}

#[test]
fn commit_response_ok_and_err_roundtrip() -> anyhow::Result<()> {
	let ok = roundtrip_to_envoy(protocol::ToEnvoy::ToEnvoySqliteCommitResponse(
		protocol::ToEnvoySqliteCommitResponse {
			request_id: 1,
			data: protocol::SqliteCommitResponse::SqliteCommitOk(protocol::SqliteCommitOk {
				head_txid: Some(7),
			}),
		},
	))?;
	let protocol::ToEnvoy::ToEnvoySqliteCommitResponse(ok) = ok else {
		panic!("expected commit response");
	};
	assert_eq!(ok.request_id, 1);
	let protocol::SqliteCommitResponse::SqliteCommitOk(ok) = ok.data else {
		panic!("expected ok response");
	};
	assert_eq!(ok.head_txid, Some(7));

	let err = roundtrip_to_envoy(protocol::ToEnvoy::ToEnvoySqliteCommitResponse(
		protocol::ToEnvoySqliteCommitResponse {
			request_id: 2,
			data: protocol::SqliteCommitResponse::SqliteErrorResponse(
				protocol::SqliteErrorResponse {
					group: "depot".into(),
					code: "quota_exceeded".into(),
					message: "quota exceeded".into(),
				},
			),
		},
	))?;
	let protocol::ToEnvoy::ToEnvoySqliteCommitResponse(err) = err else {
		panic!("expected commit response");
	};
	let protocol::SqliteCommitResponse::SqliteErrorResponse(err) = err.data else {
		panic!("expected error response");
	};
	assert_eq!(err.message, "quota exceeded");

	Ok(())
}

#[test]
fn expected_generation_optional_present_and_absent() -> anyhow::Result<()> {
	for (expected_generation, expected_head_txid) in [(None, None), (Some(7), Some(11))] {
		let decoded = roundtrip_to_rivet(protocol::ToRivet::ToRivetSqliteGetPagesRequest(
			protocol::ToRivetSqliteGetPagesRequest {
				request_id: 3,
				data: protocol::SqliteGetPagesRequest {
					actor_id: "actor-c".into(),
					pgnos: vec![1],
					expected_generation,
					expected_head_txid,
				},
			},
		))?;
		let protocol::ToRivet::ToRivetSqliteGetPagesRequest(decoded) = decoded else {
			panic!("expected get_pages request");
		};
		assert_eq!(decoded.data.expected_generation, expected_generation);
		assert_eq!(decoded.data.expected_head_txid, expected_head_txid);

		let decoded = roundtrip_to_rivet(protocol::ToRivet::ToRivetSqliteCommitRequest(
			protocol::ToRivetSqliteCommitRequest {
				request_id: 4,
				data: protocol::SqliteCommitRequest {
					actor_id: "actor-c".into(),
					dirty_pages: vec![dirty_page(1, 2)],
					db_size_pages: 1,
					now_ms: 99,
					expected_generation,
					expected_head_txid,
				},
			},
		))?;
		let protocol::ToRivet::ToRivetSqliteCommitRequest(decoded) = decoded else {
			panic!("expected commit request");
		};
		assert_eq!(decoded.data.expected_generation, expected_generation);
		assert_eq!(decoded.data.expected_head_txid, expected_head_txid);
	}

	Ok(())
}

#[test]
fn protocol_version_constant_matches_schema_version() {
	assert_eq!(PROTOCOL_VERSION, 8);
}

#[test]
fn removed_op_types_not_in_module_namespace() {
	let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
	let schema = manifest_dir
		.parent()
		.and_then(Path::parent)
		.and_then(Path::parent)
		.expect("workspace root")
		.join("sdks/schemas/envoy-protocol/v3.bare");
	let schema = std::fs::read_to_string(schema).expect("read v3 schema");

	for removed in [
		"OpenRequest",
		"CloseRequest",
		"CommitStageBegin",
		"CommitStageRequest",
		"CommitFinalize",
		"ForceCloseRequest",
	] {
		assert!(
			!schema.contains(removed),
			"{removed} still exists in v3 schema"
		);
	}
}

fn dirty_page(pgno: u32, byte: u8) -> protocol::SqliteDirtyPage {
	protocol::SqliteDirtyPage {
		pgno,
		bytes: vec![byte; 4096],
	}
}
