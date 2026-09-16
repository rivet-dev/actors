use super::*;

#[test]
fn schema_version_is_little_endian_i64() {
	let encoded = encode_schema_version(INTERNAL_SCHEMA_VERSION);
	assert_eq!(
		decode_schema_version(&encoded).unwrap(),
		INTERNAL_SCHEMA_VERSION
	);
}

#[test]
fn ladder_version_matches_migration_count() {
	assert_eq!(MIGRATIONS.len() as i64, INTERNAL_SCHEMA_VERSION);
}

#[test]
fn schema_sql_does_not_embed_workload_annotations() {
	for sql in MIGRATIONS
		.iter()
		.flat_map(|migration| migration.iter().copied())
		.chain([CREATE_META_TABLE])
	{
		assert!(
			!sql.contains("-- W["),
			"workload annotation leaked into SQL: {sql}"
		);
	}
}

#[test]
fn unpublished_schema_has_explicit_values_and_minimal_constraints() {
	let sql = MIGRATIONS
		.iter()
		.flat_map(|migration| migration.iter().copied())
		.collect::<Vec<_>>()
		.join("\n")
		.to_ascii_lowercase();
	assert!(
		!sql.contains(" default "),
		"internal columns must not use defaults"
	);
	assert!(
		!sql.replace("check (id = 1)", "").contains("check"),
		"only the singleton id constraint is allowed"
	);
	assert!(sql.contains("kind             integer not null"));
	assert!(sql.contains("result         integer not null"));

	for statement in MIGRATIONS
		.iter()
		.flat_map(|migration| migration.iter().copied())
		.filter(|statement| statement.trim_start().starts_with("CREATE TABLE"))
	{
		assert!(
			statement.contains("STRICT"),
			"table is not STRICT: {statement}"
		);
	}
}

#[test]
fn v1_database_upgrades_to_the_current_schema() {
	let conn = rusqlite::Connection::open_in_memory().expect("open fixture database");
	conn.execute_batch(CREATE_META_TABLE).unwrap();
	for sql in MIGRATIONS[0] {
		conn.execute_batch(sql).unwrap();
	}
	conn.execute(
		UPSERT_META_TEXT_SQL,
		rusqlite::params![SCHEMA_VERSION_KEY, encode_schema_version(1)],
	)
	.unwrap();
	conn.execute(
		"INSERT INTO _rivet_schedule_events (event_id, trigger_at, action, args, kind, cron_expression, timezone, interval_ms, last_started_at, max_history) VALUES ('at:1', 5, 'tick', NULL, 0, NULL, NULL, NULL, NULL, 0)",
		[],
	)
	.unwrap();
	conn.execute(
		"INSERT INTO _rivet_queue (id, name, body, created_at) VALUES (1, 'jobs', X'00', 5)",
		[],
	)
	.unwrap();

	for statement in migration_statements(1, INTERNAL_SCHEMA_VERSION).unwrap() {
		let params = statement
			.params
			.unwrap_or_default()
			.into_iter()
			.map(|param| match param {
				BindParam::Text(text) => rusqlite::types::Value::Text(text),
				BindParam::Blob(blob) => rusqlite::types::Value::Blob(blob),
				BindParam::Integer(value) => rusqlite::types::Value::Integer(value),
				other => panic!("unexpected migration bind parameter: {other:?}"),
			})
			.collect::<Vec<_>>();
		conn.execute(&statement.sql, rusqlite::params_from_iter(params))
			.unwrap_or_else(|error| panic!("apply migration {}: {error}", statement.sql));
	}

	let stored_schema: Vec<u8> = conn
		.query_row(
			LOAD_META_TEXT_SQL,
			rusqlite::params![SCHEMA_VERSION_KEY],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(
		decode_schema_version(&stored_schema).unwrap(),
		INTERNAL_SCHEMA_VERSION
	);
	let origin: (Option<String>, Option<String>, Option<String>) = conn
		.query_row(
			"SELECT origin_ray_id, origin_traceparent, origin_tracestate FROM _rivet_schedule_events WHERE event_id = 'at:1'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.unwrap();
	assert_eq!(origin, (None, None, None));
	let origin: (Option<String>, Option<String>, Option<String>) = conn
		.query_row(
			"SELECT origin_ray_id, origin_traceparent, origin_tracestate FROM _rivet_queue WHERE id = 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.unwrap();
	assert_eq!(origin, (None, None, None));
}

#[test]
fn logical_run_wake_metadata_lives_in_the_meta_table() {
	use rivetkit_actor_persist::versioned::RunWakeAt;
	use vbare::OwnedVersionedData;

	let conn = rusqlite::Connection::open_in_memory().expect("open fixture database");
	initialize_test_schema(&conn).expect("initialize actor schema");
	let logical_wake = RunWakeAt::wrap_latest(Some(1_723_456_789_000))
		.serialize_with_embedded_version(1)
		.expect("encode logical run wake");
	conn.execute(
		"INSERT INTO _rivet_meta (key, value) VALUES (?1, ?2)",
		rusqlite::params![
			crate::actor::internal_storage::RUN_WAKE_AT_META_KEY,
			logical_wake.clone()
		],
	)
	.expect("persist reserved metadata row");
	let stored_wake: Vec<u8> = conn
		.query_row(
			LOAD_META_TEXT_SQL,
			rusqlite::params![crate::actor::internal_storage::RUN_WAKE_AT_META_KEY],
			|row| row.get(0),
		)
		.expect("preserve unknown metadata row");
	assert_eq!(stored_wake, logical_wake);
	assert_eq!(
		RunWakeAt::deserialize_with_embedded_version(&stored_wake).unwrap(),
		Some(1_723_456_789_000),
	);
}
