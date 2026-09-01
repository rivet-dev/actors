//! Headers that carry a caller's ray ID and W3C trace context into an actor,
//! shared by every Rust peer of the actor HTTP surface: the runtime that reads
//! them and the clients that send them.

/// Bounded correlation string that follows work across actors, surviving
/// sampling and trace-root boundaries.
pub const HEADER_RIVET_RAY_ID: &str = "x-rivet-ray-id";
pub const HEADER_TRACEPARENT: &str = "traceparent";
pub const HEADER_TRACESTATE: &str = "tracestate";

/// W3C Baggage key that carries a ray ID through application code, so a request
/// handler that received a ray ID can hand it to the actors it calls.
pub const RAY_BAGGAGE_KEY: &str = "rivet.ray.id";

const RAY_ID_MAX_LEN: usize = 128;

/// Returns `value` when it is a ray ID the runtime accepts: 1 to 128 characters
/// of `[A-Za-z0-9_-]`. The value arrives from a caller, so anything else
/// counts as absent.
pub fn bounded_ray_id(value: &str) -> Option<&str> {
	let valid = !value.is_empty()
		&& value.len() <= RAY_ID_MAX_LEN
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
	if valid { Some(value) } else { None }
}
