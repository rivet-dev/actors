use super::*;

fn decode(json: &str) -> anyhow::Result<ActorInput> {
	// Field handling (defaults, deny_unknown_fields) is format-agnostic in
	// serde, so JSON exercises the same paths as the CBOR wire encoding.
	Ok(serde_json::from_str(json)?)
}

#[test]
fn empty_object_yields_defaults() {
	let input = decode("{}").unwrap();
	assert!(input.command.is_none());
	assert!(input.args.is_empty());
	assert!(input.env.is_empty());
	assert!(input.port.is_none());
}

#[test]
fn full_input_decodes() {
	let input = decode(
		r#"{"command":["./GameServer","-batchmode"],"args":["-x"],"env":{"A":"1"},"port":7777}"#,
	)
	.unwrap();
	assert_eq!(
		input.command.as_deref(),
		Some(&["./GameServer".to_string(), "-batchmode".to_string()][..])
	);
	assert_eq!(input.args, vec!["-x".to_string()]);
	assert_eq!(input.env.get("A").map(String::as_str), Some("1"));
	assert_eq!(input.port, Some(7777));
}

#[test]
fn unknown_fields_are_ignored() {
	// Lenient by design: ActorInput doubles as persisted state, so a rollback
	// must still decode state written by a newer binary with extra fields.
	let input = decode(r#"{"port":7777,"future_field":123}"#).unwrap();
	assert_eq!(input.port, Some(7777));
}

#[test]
fn default_matches_empty() {
	let default = ActorInput::default();
	assert!(default.command.is_none());
	assert!(default.args.is_empty());
	assert!(default.env.is_empty());
	assert!(default.port.is_none());
}

fn cbor_round_trip<T, U>(value: &T) -> anyhow::Result<U>
where
	T: serde::Serialize,
	U: serde::de::DeserializeOwned,
{
	let mut buf = Vec::new();
	ciborium::into_writer(value, &mut buf)?;
	Ok(ciborium::from_reader(&buf[..])?)
}

#[test]
fn actor_state_cbor_round_trips() {
	// State persists as CBOR (ciborium), so the flattened input must survive a CBOR
	// round trip, not just the JSON the other tests use.
	let state = ActorState {
		input: ActorInput {
			port: Some(7777),
			args: vec!["-x".to_string()],
			..Default::default()
		},
		started_once: true,
	};
	let decoded: ActorState = cbor_round_trip(&state).unwrap();
	assert_eq!(decoded.input.port, Some(7777));
	assert_eq!(decoded.input.args, vec!["-x".to_string()]);
	assert!(decoded.started_once);
}

#[test]
fn new_actor_state_decodes_into_legacy_bare_input() {
	// Rollback direction: state written by a newer binary (ActorState with
	// `started_once`) must still decode on an older binary whose state was a bare
	// ActorInput. The old decoder ignores `started_once` and keeps the launch spec.
	let state = ActorState {
		input: ActorInput {
			command: Some(vec!["./GameServer".to_string()]),
			args: vec!["-x".to_string()],
			env: std::collections::HashMap::from([("A".to_string(), "1".to_string())]),
			port: Some(7777),
		},
		started_once: true,
	};
	let decoded: ActorInput = cbor_round_trip(&state).unwrap();
	assert_eq!(
		decoded.command.as_deref(),
		Some(&["./GameServer".to_string()][..])
	);
	assert_eq!(decoded.args, vec!["-x".to_string()]);
	assert_eq!(decoded.env.get("A").map(String::as_str), Some("1"));
	assert_eq!(decoded.port, Some(7777));
}

#[test]
fn legacy_bare_input_state_decodes_into_actor_state() {
	// State written before `started_once` existed was a bare ActorInput. The
	// flattened input must still decode, defaulting `started_once` to false.
	let legacy = ActorInput {
		port: Some(7777),
		..Default::default()
	};
	let decoded: ActorState = cbor_round_trip(&legacy).unwrap();
	assert_eq!(decoded.input.port, Some(7777));
	assert!(!decoded.started_once);
}
