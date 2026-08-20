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
fn logical_run_wake_metadata_keeps_the_v1_schema_openable() {
	use rivetkit_actor_persist::versioned::RunWakeAt;
	use vbare::OwnedVersionedData;

	assert_eq!(INTERNAL_SCHEMA_VERSION, 1);
	let conn = rusqlite::Connection::open_in_memory().expect("open v1 fixture database");
	initialize_test_schema(&conn).expect("initialize v1 actor schema");
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
	conn.execute(
		"INSERT INTO _rivet_runtime (id, last_pushed_alarm, inspector_token, queue_next_id) VALUES (1, ?1, NULL, 2)",
		rusqlite::params![1_723_456_789_500_i64],
	)
	.expect("persist v1 runtime row");

	let stored_schema: Vec<u8> = conn
		.query_row(
			LOAD_META_TEXT_SQL,
			rusqlite::params![SCHEMA_VERSION_KEY],
			|row| row.get(0),
		)
		.expect("read schema version as an old runtime would");
	assert_eq!(decode_schema_version(&stored_schema).unwrap(), 1);
	let legacy_runtime: (Option<i64>, Option<String>, i64) = conn
		.query_row(
			"SELECT last_pushed_alarm, inspector_token, queue_next_id FROM _rivet_runtime WHERE id = 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.expect("open runtime through the v1 projection");
	assert_eq!(legacy_runtime, (Some(1_723_456_789_500), None, 2));
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
