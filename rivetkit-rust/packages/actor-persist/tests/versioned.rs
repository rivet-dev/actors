use rivetkit_actor_persist::versioned;
use vbare::OwnedVersionedData;

#[test]
fn actor_decodes_legacy_raw_v4_schedule_args() {
	let encoded = b"\x04\x00\x01\x05input\x01\x05state\x01\x07event-1\x2a\x00\x00\x00\x00\x00\x00\x00\x0fhandleTurnTimer\x01\x80";

	let decoded = versioned::Actor::deserialize_with_embedded_version(encoded)
		.expect("legacy raw v4 actor should decode");

	assert_eq!(decoded.input, Some(b"input".to_vec()));
	assert!(decoded.has_initialized);
	assert_eq!(decoded.state, b"state");
	assert_eq!(decoded.scheduled_events.len(), 1);
	assert_eq!(decoded.scheduled_events[0].event_id, "event-1");
	assert_eq!(decoded.scheduled_events[0].timestamp, 42);
	assert_eq!(decoded.scheduled_events[0].action, "handleTurnTimer");
	assert_eq!(decoded.scheduled_events[0].args, Some(vec![0x80]));
}

#[test]
fn actor_decodes_legacy_raw_v4_when_current_v4_accepts_bytes() {
	let encoded = b"\x04\x00\x00\x01\x05state\x01\x07event-1\x2a\x00\x00\x00\x00\x00\x00\x00\x0fhandleTurnTimer\x02\x01\x99";

	let decoded = versioned::Actor::deserialize_with_embedded_version(encoded)
		.expect("legacy raw v4 actor accepted by current v4 should decode");

	assert_eq!(decoded.scheduled_events[0].args, Some(vec![0x01, 0x99]));
}

#[test]
fn run_wake_deadline_round_trips_with_embedded_version() {
	let encoded = versioned::RunWakeAt::wrap_latest(Some(1_725_000_000_123))
		.serialize_with_embedded_version(1)
		.expect("encode run wake deadline");
	assert_eq!(&encoded[..2], &[1, 0]);

	let decoded = versioned::RunWakeAt::deserialize_with_embedded_version(&encoded)
		.expect("decode run wake deadline");
	assert_eq!(decoded, Some(1_725_000_000_123));
}
