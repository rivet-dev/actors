use anyhow::Result;
use rivet_envoy_protocol::{
	generated::{v4, v8},
	versioned::{
		ProtocolCompatibilityDirection, ProtocolCompatibilityError, ProtocolCompatibilityFeature,
		ToEnvoy, ToRivet,
	},
};
use vbare::OwnedVersionedData;

fn remote_sql_request_exec() -> v8::ToRivet {
	v8::ToRivet::ToRivetSqliteExecRequest(v8::ToRivetSqliteExecRequest {
		request_id: 1,
		data: v8::SqliteExecRequest {
			namespace_id: "namespace".into(),
			actor_id: "actor".into(),
			generation: 7,
			sql: "select 1".into(),
		},
	})
}

fn remote_sql_request_execute() -> v8::ToRivet {
	v8::ToRivet::ToRivetSqliteExecuteRequest(v8::ToRivetSqliteExecuteRequest {
		request_id: 2,
		data: v8::SqliteExecuteRequest {
			namespace_id: "namespace".into(),
			actor_id: "actor".into(),
			generation: 7,
			sql: "select ?".into(),
			params: Some(vec![v8::SqliteBindParam::SqliteValueInteger(
				v8::SqliteValueInteger { value: 1 },
			)]),
		},
	})
}

fn remote_sql_response_exec() -> v8::ToEnvoy {
	v8::ToEnvoy::ToEnvoySqliteExecResponse(v8::ToEnvoySqliteExecResponse {
		request_id: 1,
		data: v8::SqliteExecResponse::SqliteErrorResponse(v8::SqliteErrorResponse {
			group: "sqlite".into(),
			code: "remote_unavailable".into(),
			message: "remote sql execution is unavailable".into(),
		}),
	})
}

fn remote_sql_response_execute() -> v8::ToEnvoy {
	v8::ToEnvoy::ToEnvoySqliteExecuteResponse(v8::ToEnvoySqliteExecuteResponse {
		request_id: 2,
		data: v8::SqliteExecuteResponse::SqliteErrorResponse(v8::SqliteErrorResponse {
			group: "sqlite".into(),
			code: "remote_unavailable".into(),
			message: "remote sql execution is unavailable".into(),
		}),
	})
}

fn remote_sql_request_execute_batch() -> v8::ToRivet {
	v8::ToRivet::ToRivetSqliteExecuteBatchRequest(v8::ToRivetSqliteExecuteBatchRequest {
		request_id: 3,
		data: v8::SqliteExecuteBatchRequest {
			namespace_id: "namespace".into(),
			actor_id: "actor".into(),
			generation: 7,
			statements: vec![v8::SqliteBatchStatement {
				sql: "insert into t values (?)".into(),
				params: Some(vec![v8::SqliteBindParam::SqliteValueInteger(
					v8::SqliteValueInteger { value: 1 },
				)]),
			}],
		},
	})
}

fn remote_sql_response_execute_batch() -> v8::ToEnvoy {
	v8::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(v8::ToEnvoySqliteExecuteBatchResponse {
		request_id: 3,
		data: v8::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(v8::SqliteExecuteBatchOk {
			results: Vec::new(),
		}),
	})
}

fn assert_compatibility_error(
	err: anyhow::Error,
	direction: ProtocolCompatibilityDirection,
	target_version: u16,
) {
	let err = err
		.downcast_ref::<ProtocolCompatibilityError>()
		.expect("expected structured protocol compatibility error");

	assert_eq!(
		err.feature,
		ProtocolCompatibilityFeature::RemoteSqliteExecution
	);
	assert_eq!(err.direction, direction);
	assert_eq!(err.required_version, 4);
	assert_eq!(err.target_version, target_version);
}

#[test]
fn old_core_new_pegboard_envoy_rejects_remote_sql_request() {
	let err = ToRivet::wrap_latest(remote_sql_request_exec())
		.serialize(3)
		.expect_err("remote SQL requests must not serialize below v4");

	assert_compatibility_error(err, ProtocolCompatibilityDirection::ToRivet, 3);
}

#[test]
fn new_core_old_pegboard_envoy_rejects_remote_sql_response() {
	let err = ToEnvoy::wrap_latest(remote_sql_response_exec())
		.serialize(3)
		.expect_err("remote SQL responses must not serialize below v4");

	assert_compatibility_error(err, ProtocolCompatibilityDirection::ToEnvoy, 3);
}

#[test]
fn old_core_old_pegboard_envoy_rejects_remote_sql_both_directions() {
	let request_err = ToRivet::wrap_latest(remote_sql_request_exec())
		.serialize(3)
		.expect_err("remote SQL requests must not serialize below v4");
	let response_err = ToEnvoy::wrap_latest(remote_sql_response_exec())
		.serialize(3)
		.expect_err("remote SQL responses must not serialize below v4");

	assert_compatibility_error(request_err, ProtocolCompatibilityDirection::ToRivet, 3);
	assert_compatibility_error(response_err, ProtocolCompatibilityDirection::ToEnvoy, 3);
}

#[test]
fn new_core_new_pegboard_envoy_allows_remote_sql_both_directions() -> Result<()> {
	let request = ToRivet::wrap_latest(remote_sql_request_exec()).serialize(4)?;
	let response = ToEnvoy::wrap_latest(remote_sql_response_exec()).serialize(4)?;

	assert!(matches!(
		ToRivet::deserialize(&request, 4)?,
		v8::ToRivet::ToRivetSqliteExecRequest(_)
	));
	assert!(matches!(
		ToEnvoy::deserialize(&response, 4)?,
		v8::ToEnvoy::ToEnvoySqliteExecResponse(_)
	));

	Ok(())
}

#[test]
fn v4_remote_sql_payloads_do_not_decode_as_v3() -> Result<()> {
	let request = serde_bare::to_vec(&v4::ToRivet::ToRivetSqliteExecRequest(
		v4::ToRivetSqliteExecRequest {
			request_id: 1,
			data: v4::SqliteExecRequest {
				namespace_id: "namespace".into(),
				actor_id: "actor".into(),
				generation: 7,
				sql: "select 1".into(),
			},
		},
	))?;
	let response = serde_bare::to_vec(&v4::ToEnvoy::ToEnvoySqliteExecResponse(
		v4::ToEnvoySqliteExecResponse {
			request_id: 1,
			data: v4::SqliteExecResponse::SqliteErrorResponse(v4::SqliteErrorResponse {
				message: "remote sql execution is unavailable".into(),
			}),
		},
	))?;

	assert!(ToRivet::deserialize(&request, 3).is_err());
	assert!(ToEnvoy::deserialize(&response, 3).is_err());

	Ok(())
}

#[test]
fn all_remote_sql_request_variants_require_v4() {
	// The remote SQL feature drops at the v4 -> v3 boundary, so the chain
	// reports target_version = 3 regardless of how much further down we are
	// asking serialize() to walk.
	for version in 1..4 {
		for request in [remote_sql_request_exec(), remote_sql_request_execute()] {
			let err = ToRivet::wrap_latest(request)
				.serialize(version)
				.expect_err("remote SQL request variant must not serialize below v4");

			assert_compatibility_error(err, ProtocolCompatibilityDirection::ToRivet, 3);
		}
	}
}

#[test]
fn all_remote_sql_response_variants_require_v4() {
	for version in 1..4 {
		for response in [remote_sql_response_exec(), remote_sql_response_execute()] {
			let err = ToEnvoy::wrap_latest(response)
				.serialize(version)
				.expect_err("remote SQL response variant must not serialize below v4");

			assert_compatibility_error(err, ProtocolCompatibilityDirection::ToEnvoy, 3);
		}
	}
}

#[test]
fn remote_sql_batch_requires_v6() -> Result<()> {
	let request_error = ToRivet::wrap_latest(remote_sql_request_execute_batch())
		.serialize(5)
		.expect_err("remote SQL batch requests must not serialize below v6");
	let response_error = ToEnvoy::wrap_latest(remote_sql_response_execute_batch())
		.serialize(5)
		.expect_err("remote SQL batch responses must not serialize below v6");

	for (error, direction) in [
		(request_error, ProtocolCompatibilityDirection::ToRivet),
		(response_error, ProtocolCompatibilityDirection::ToEnvoy),
	] {
		let error = error
			.downcast_ref::<ProtocolCompatibilityError>()
			.expect("expected structured protocol compatibility error");
		assert_eq!(
			error.feature,
			ProtocolCompatibilityFeature::RemoteSqliteBatchExecution
		);
		assert_eq!(error.direction, direction);
		assert_eq!(error.required_version, 6);
		assert_eq!(error.target_version, 5);
	}

	let request = ToRivet::wrap_latest(remote_sql_request_execute_batch()).serialize(6)?;
	let response = ToEnvoy::wrap_latest(remote_sql_response_execute_batch()).serialize(6)?;
	assert!(matches!(
		ToRivet::deserialize(&request, 6)?,
		v8::ToRivet::ToRivetSqliteExecuteBatchRequest(_)
	));
	assert!(matches!(
		ToEnvoy::deserialize(&response, 6)?,
		v8::ToEnvoy::ToEnvoySqliteExecuteBatchResponse(_)
	));
	Ok(())
}
