use hyper::HeaderMap;
use hyper::header::ACCEPT;
use rivet_config::config::features::{GuardGatewayV3, GuardGatewayV3Mode};
use rivet_envoy_protocol::PROTOCOL_VERSION;
use rivet_guard_core::request_context::RequestContext;
use rivet_util::Id;
use xxhash_rust::xxh3::xxh3_128_with_seed;

use crate::metrics;

const MIN_GATEWAY3_PROTOCOL_VERSION: u16 = 7;
const SAMPLE_BUCKETS: u128 = 10_000;
const SAMPLE_DOMAIN: &[u8] = b"pegboard-gateway3-rollout-v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestKind {
	WebSocket,
	HttpSse,
	HttpUnknownLength,
	HttpOther,
}

impl RequestKind {
	fn label(self) -> &'static str {
		match self {
			Self::WebSocket => "websocket",
			Self::HttpSse => "http_sse",
			Self::HttpUnknownLength => "http_unknown_length",
			Self::HttpOther => "http_other",
		}
	}

	fn is_opportunistic_candidate(self) -> bool {
		matches!(self, Self::HttpSse | Self::HttpUnknownLength)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
	Disabled,
	WebSocket,
	ProtocolUnknown,
	ProtocolIncompatible,
	NotCandidate,
	SampledOut,
	SampledIn,
}

impl Decision {
	fn label(self) -> &'static str {
		match self {
			Self::Disabled => "disabled",
			Self::WebSocket => "websocket",
			Self::ProtocolUnknown => "protocol_unknown",
			Self::ProtocolIncompatible => "protocol_incompatible",
			Self::NotCandidate => "not_candidate",
			Self::SampledOut => "sampled_out",
			Self::SampledIn => "sampled_in",
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(super) struct GatewaySelection {
	request_kind: RequestKind,
	mode: GuardGatewayV3Mode,
	protocol_version: Option<u16>,
	decision: Decision,
}

impl GatewaySelection {
	pub(super) fn use_gateway3(self) -> bool {
		self.decision == Decision::SampledIn
	}

	pub(super) fn record(self) {
		let gateway = if self.use_gateway3() {
			"gateway3"
		} else {
			"gateway2"
		};
		let envoy_protocol = match self.protocol_version {
			None => "unknown",
			Some(version) if version < MIN_GATEWAY3_PROTOCOL_VERSION => "legacy",
			Some(_) => "streaming",
		};
		let mode = match self.mode {
			GuardGatewayV3Mode::Off => "off",
			GuardGatewayV3Mode::Opportunistic => "opportunistic",
			GuardGatewayV3Mode::On => "on",
		};

		metrics::PEGBOARD_GATEWAY_ROUTE_TOTAL
			.with_label_values(&[
				gateway,
				envoy_protocol,
				self.request_kind.label(),
				mode,
				self.decision.label(),
			])
			.inc();

		let span = tracing::Span::current();
		span.record("gateway", gateway);
		span.record("envoy_protocol", envoy_protocol);
		span.record("request_kind", self.request_kind.label());
		span.record("mode", mode);
		span.record("decision", self.decision.label());

		tracing::debug!(
			gateway,
			envoy_protocol,
			request_kind = self.request_kind.label(),
			mode,
			decision = self.decision.label(),
			protocol_version = self.protocol_version,
			"selected pegboard gateway"
		);
	}
}

pub(super) fn select_gateway(
	config: &GuardGatewayV3,
	req_ctx: &RequestContext,
	namespace_id: Id,
	actor_id: Id,
	protocol_version: Option<u16>,
) -> GatewaySelection {
	let request_kind = classify_request(req_ctx);
	select_gateway_from_parts(
		config,
		request_kind,
		namespace_id,
		actor_id,
		protocol_version,
	)
}

fn select_gateway_from_parts(
	config: &GuardGatewayV3,
	request_kind: RequestKind,
	namespace_id: Id,
	actor_id: Id,
	protocol_version: Option<u16>,
) -> GatewaySelection {
	let decision = if config.mode == GuardGatewayV3Mode::Off {
		Decision::Disabled
	} else if request_kind == RequestKind::WebSocket {
		Decision::WebSocket
	} else if protocol_version.is_none() {
		Decision::ProtocolUnknown
	} else if !protocol_version.is_some_and(|version| {
		(MIN_GATEWAY3_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&version)
	}) {
		Decision::ProtocolIncompatible
	} else if config.mode == GuardGatewayV3Mode::Opportunistic
		&& !request_kind.is_opportunistic_candidate()
	{
		Decision::NotCandidate
	} else if sampled_in(namespace_id, actor_id, config.percentage) {
		Decision::SampledIn
	} else {
		Decision::SampledOut
	};

	GatewaySelection {
		request_kind,
		mode: config.mode,
		protocol_version,
		decision,
	}
}

fn classify_request(req_ctx: &RequestContext) -> RequestKind {
	classify_request_parts(
		req_ctx.is_websocket(),
		req_ctx.headers(),
		req_ctx.request_body_exact_size(),
		req_ctx.request_body_is_end_stream(),
	)
}

fn classify_request_parts(
	is_websocket: bool,
	headers: &HeaderMap,
	body_exact_size: Option<u64>,
	body_is_end_stream: bool,
) -> RequestKind {
	if is_websocket {
		RequestKind::WebSocket
	} else if accepts_event_stream(headers) {
		RequestKind::HttpSse
	} else if !body_is_end_stream && body_exact_size.is_none() {
		RequestKind::HttpUnknownLength
	} else {
		RequestKind::HttpOther
	}
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
	headers
		.get_all(ACCEPT)
		.iter()
		.filter_map(|value| value.to_str().ok())
		.flat_map(|value| value.split(','))
		.any(|range| {
			let mut parts = range.split(';');
			if !parts
				.next()
				.is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
			{
				return false;
			}

			for parameter in parts {
				let Some((name, value)) = parameter.split_once('=') else {
					continue;
				};
				if name.trim().eq_ignore_ascii_case("q") {
					return value
						.trim()
						.parse::<f32>()
						.is_ok_and(|quality| quality > 0.0 && quality <= 1.0);
				}
			}

			true
		})
}

fn sampled_in(namespace_id: Id, actor_id: Id, percentage: u8) -> bool {
	match percentage {
		0 => false,
		100 => true,
		percent => sample_bucket(namespace_id, actor_id) < u128::from(percent) * 100,
	}
}

fn sample_bucket(namespace_id: Id, actor_id: Id) -> u128 {
	let mut bytes = Vec::with_capacity(SAMPLE_DOMAIN.len() + 80);
	bytes.extend_from_slice(SAMPLE_DOMAIN);
	bytes.extend_from_slice(namespace_id.to_string().as_bytes());
	bytes.push(0);
	bytes.extend_from_slice(actor_id.to_string().as_bytes());
	xxh3_128_with_seed(&bytes, 0) % SAMPLE_BUCKETS
}

#[cfg(test)]
mod tests {
	use hyper::{HeaderMap, header::HeaderValue};

	use super::*;

	fn id(data: [u8; 18]) -> Id {
		let mut bytes = [0; 19];
		bytes[0] = 1;
		bytes[1..].copy_from_slice(&data);
		Id::from_slice(&bytes).unwrap()
	}

	fn config(mode: GuardGatewayV3Mode, percentage: u8) -> GuardGatewayV3 {
		GuardGatewayV3 { mode, percentage }
	}

	fn decision(
		mode: GuardGatewayV3Mode,
		percentage: u8,
		request_kind: RequestKind,
		protocol_version: Option<u16>,
	) -> Decision {
		select_gateway_from_parts(
			&config(mode, percentage),
			request_kind,
			Id::nil(),
			Id::nil(),
			protocol_version,
		)
		.decision
	}

	#[test]
	fn sampling_has_stable_golden_vectors() {
		assert_eq!(sample_bucket(Id::nil(), Id::nil()), 1513);
		assert_eq!(
			sample_bucket(id(std::array::from_fn(|i| i as u8)), id([255; 18])),
			558
		);
	}

	#[test]
	fn sampling_boundaries_are_explicit() {
		assert!(!sampled_in(Id::nil(), Id::nil(), 0));
		assert!(sampled_in(Id::nil(), Id::nil(), 100));
		assert!(sampled_in(Id::nil(), Id::nil(), 16));
		assert!(!sampled_in(Id::nil(), Id::nil(), 15));
	}

	#[test]
	fn mode_protocol_and_websocket_gates_are_ordered() {
		assert_eq!(
			decision(GuardGatewayV3Mode::Off, 100, RequestKind::HttpSse, Some(7)),
			Decision::Disabled
		);
		assert_eq!(
			decision(GuardGatewayV3Mode::On, 100, RequestKind::WebSocket, Some(7)),
			Decision::WebSocket
		);
		assert_eq!(
			decision(GuardGatewayV3Mode::On, 100, RequestKind::HttpSse, None),
			Decision::ProtocolUnknown
		);
		assert_eq!(
			decision(GuardGatewayV3Mode::On, 100, RequestKind::HttpSse, Some(6)),
			Decision::ProtocolIncompatible
		);
		assert_eq!(
			decision(
				GuardGatewayV3Mode::On,
				100,
				RequestKind::HttpSse,
				Some(PROTOCOL_VERSION + 1),
			),
			Decision::ProtocolIncompatible
		);
		assert_eq!(
			decision(
				GuardGatewayV3Mode::Opportunistic,
				100,
				RequestKind::HttpOther,
				Some(7),
			),
			Decision::NotCandidate
		);
		assert_eq!(
			decision(GuardGatewayV3Mode::On, 0, RequestKind::HttpOther, Some(7)),
			Decision::SampledOut
		);
		assert_eq!(
			decision(GuardGatewayV3Mode::On, 100, RequestKind::HttpOther, Some(7)),
			Decision::SampledIn
		);
	}

	#[test]
	fn request_classification_uses_pre_dispatch_metadata() {
		let empty = HeaderMap::new();
		assert_eq!(
			classify_request_parts(false, &empty, None, false),
			RequestKind::HttpUnknownLength
		);
		assert_eq!(
			classify_request_parts(false, &empty, Some(10), false),
			RequestKind::HttpOther
		);
		assert_eq!(
			classify_request_parts(false, &empty, None, true),
			RequestKind::HttpOther
		);
		assert_eq!(
			classify_request_parts(true, &empty, None, false),
			RequestKind::WebSocket
		);
	}

	#[test]
	fn event_stream_accept_parsing_is_strict() {
		let accepts = |value: &'static str| {
			let mut headers = HeaderMap::new();
			headers.insert(ACCEPT, HeaderValue::from_static(value));
			accepts_event_stream(&headers)
		};

		assert!(accepts("text/event-stream"));
		assert!(accepts(
			"application/json, TEXT/EVENT-STREAM; charset=utf-8; q=0.5"
		));
		assert!(!accepts("text/event-stream;q=0"));
		assert!(!accepts("text/event-stream;q=garbage"));
		assert!(!accepts("application/x-text/event-stream"));
	}
}
