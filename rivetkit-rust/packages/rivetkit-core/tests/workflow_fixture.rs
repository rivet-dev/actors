use super::*;
use crate::ActorKey;
use crate::testing::ActorContextHarness;
use rivetkit_actor_persist::versioned::RunWakeAt;
use vbare::OwnedVersionedData;

fn metadata() -> WorkflowFixtureMetadata {
	WorkflowFixtureMetadata {
		fixture_name: "typed-roundtrip".to_owned(),
		source_rivetkit_version: "2.3.7".to_owned(),
		source_workflow_version: "2.3.7".to_owned(),
		source_revision: "legacy-revision".to_owned(),
		actor_id: "workflow-fixture".to_owned(),
		registry_key: "workflowFixture".to_owned(),
		internal_schema_version: 1,
		fake_clock_seed: 1_723_456_789_000,
		generated_id_seed: 42,
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn logical_fixture_roundtrips_every_persisted_workflow_row_type() {
	let source_harness = ActorContextHarness::new();
	let source = source_harness.context(
		"workflow-fixture",
		"workflowFixture",
		ActorKey::default(),
		"local",
	);
	let statements = vec![
		statement(
			"INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, ?, ?, ?)",
			Some(vec![
				BindParam::Integer(1_723_456_790_000),
				BindParam::Null,
				BindParam::Integer(10),
			]),
		),
		statement(
			"INSERT INTO _rivet_meta (key, value) VALUES (?, ?)",
			Some(vec![
				BindParam::Text("run_wake_at".to_owned()),
				BindParam::Blob(
					RunWakeAt::wrap_latest(Some(1_723_456_789_500))
						.serialize_with_embedded_version(1)
						.expect("encode logical run wake"),
				),
			]),
		),
		statement(
			"INSERT INTO _rivet_actor (id, has_initialized, input) VALUES (1, ?, ?)",
			Some(vec![BindParam::Integer(1), BindParam::Null]),
		),
		statement(
			"INSERT INTO _rivet_actor_state (id, state) VALUES (1, ?)",
			Some(vec![BindParam::Blob(vec![0, 0xff, 7, 0])]),
		),
		statement(
			"INSERT INTO _rivet_wf_kv (key, value) VALUES (?, ?)",
			Some(vec![
				BindParam::Blob(vec![6, 1, 0xff]),
				BindParam::Blob(vec![0, 1, 0xff]),
			]),
		),
		statement(
			"INSERT INTO _rivet_wf_kv (key, value) VALUES (?, ?)",
			Some(vec![
				BindParam::Blob(vec![6, 1, 0]),
				BindParam::Blob(Vec::new()),
			]),
		),
		statement(
			"INSERT INTO _rivet_queue (id, name, body, created_at) VALUES (?, ?, ?, ?)",
			Some(vec![
				BindParam::Integer(9),
				BindParam::Text("approval".to_owned()),
				BindParam::Blob(vec![0xd9, 0x01, 0x02]),
				BindParam::Integer(1_723_456_789_100),
			]),
		),
		statement(
			"INSERT INTO _rivet_schedule_events (event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
			Some(vec![
				BindParam::Text("scheduled-action".to_owned()),
				BindParam::Integer(1_723_456_790_000),
				BindParam::Text("tick".to_owned()),
				BindParam::Blob(vec![0x81, 0x01]),
				BindParam::Integer(0),
				BindParam::Null,
				BindParam::Text("UTC".to_owned()),
				BindParam::Null,
				BindParam::Integer(1_723_456_788_000),
				BindParam::Integer(3),
			]),
		),
		statement(
			"INSERT INTO _rivet_schedule_history (id, schedule_id, action, scheduled_at, fired_at, finished_at, result, error_group, error_code, error_message, error_metadata) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
			Some(vec![
				BindParam::Integer(1),
				BindParam::Text("scheduled-action".to_owned()),
				BindParam::Text("tick".to_owned()),
				BindParam::Integer(1_723_456_788_000),
				BindParam::Integer(1_723_456_788_010),
				BindParam::Null,
				BindParam::Integer(0),
				BindParam::Text("schedule".to_owned()),
				BindParam::Text("still_running".to_owned()),
				BindParam::Null,
				BindParam::Blob(vec![0xa0]),
			]),
		),
	];
	source
		.sql()
		.execute_batch(statements)
		.await
		.expect("seed source actor database");

	let fixture = dump_workflow_fixture(&source, metadata())
		.await
		.expect("dump source fixture");
	assert_eq!(
		fixture
			.workflow_rows
			.iter()
			.map(|row| row.key.clone())
			.collect::<Vec<_>>(),
		vec![vec![6, 1, 0], vec![6, 1, 0xff]],
		"workflow rows must use bytewise canonical ordering and retain the hidden namespace",
	);
	let bytes = fixture.encode().expect("encode fixture");
	assert_eq!(&bytes[..2], &[1, 0], "fixture needs a vbare version header");
	let decoded = WorkflowFixture::decode(&bytes).expect("decode fixture");
	assert_eq!(decoded, fixture);

	let target_harness = ActorContextHarness::new();
	let target = target_harness.context(
		"workflow-fixture",
		"workflowFixture",
		ActorKey::default(),
		"local",
	);
	restore_workflow_fixture(&target, &decoded)
		.await
		.expect("restore fixture");
	let roundtrip = dump_workflow_fixture(&target, metadata())
		.await
		.expect("dump restored fixture");
	assert_eq!(roundtrip, fixture);
	assert_eq!(roundtrip.actor_state, Some(vec![0, 0xff, 7, 0]));
	assert_eq!(roundtrip.runtime.as_ref().unwrap().inspector_token, None);
	let run_wake_row = roundtrip
		.meta_rows
		.iter()
		.find(|row| row.key == "run_wake_at")
		.expect("logical run wake metadata row");
	assert_eq!(
		RunWakeAt::deserialize_with_embedded_version(&run_wake_row.value)
			.expect("decode logical run wake"),
		Some(1_723_456_789_500),
	);
	assert_eq!(roundtrip.schedule_events[0].args, Some(vec![0x81, 0x01]));
	assert_eq!(roundtrip.schedule_events[0].interval_ms, None);
	assert_eq!(roundtrip.schedule_history[0].finished_at, None);
	assert_eq!(roundtrip.schedule_history[0].error_message, None);
}

#[test]
fn fixture_rejects_unknown_embedded_version() {
	let fixture = WorkflowFixture {
		metadata: metadata(),
		meta_rows: Vec::new(),
		runtime: None,
		actor: None,
		actor_state: None,
		workflow_rows: Vec::new(),
		queue_rows: Vec::new(),
		schedule_events: Vec::new(),
		schedule_history: Vec::new(),
	};
	let mut bytes = fixture.encode().expect("encode fixture");
	bytes[..2].copy_from_slice(&2u16.to_le_bytes());
	assert!(
		WorkflowFixture::decode(&bytes)
			.unwrap_err()
			.to_string()
			.contains("unsupported workflow fixture version 2")
	);
}

#[test]
fn fixture_restore_validation_rejects_schema_and_namespace_ambiguity() {
	let base = WorkflowFixture {
		metadata: metadata(),
		meta_rows: vec![WorkflowFixtureMetaRow {
			key: "schema_version".to_owned(),
			value: 1_i64.to_le_bytes().to_vec(),
		}],
		runtime: None,
		actor: None,
		actor_state: None,
		workflow_rows: Vec::new(),
		queue_rows: Vec::new(),
		schedule_events: Vec::new(),
		schedule_history: Vec::new(),
	};
	validate_fixture_for_restore(&base).expect("valid v1 fixture");

	let mut wrong_schema = base.clone();
	wrong_schema.meta_rows[0].value = 2_i64.to_le_bytes().to_vec();
	assert!(
		validate_fixture_for_restore(&wrong_schema)
			.unwrap_err()
			.to_string()
			.contains("schema metadata mismatch")
	);

	let mut wrong_namespace = base;
	wrong_namespace
		.workflow_rows
		.push(WorkflowFixtureWorkflowRow {
			key: vec![6, 2, 0],
			value: vec![1],
		});
	assert!(
		validate_fixture_for_restore(&wrong_namespace)
			.unwrap_err()
			.to_string()
			.contains("escaped the [6, 1] namespace")
	);
}
