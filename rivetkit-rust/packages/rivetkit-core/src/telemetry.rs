//! Internal OpenTelemetry spans owned by the actor runtime.

#[cfg(feature = "native-runtime")]
pub mod export;

use opentelemetry::propagation::{Extractor, TextMapPropagator as _};
use opentelemetry::trace::{SpanContext, TraceContextExt as _};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::{ActorContext, format_actor_key};

/// Correlation fields accepted at an invocation boundary.
#[derive(Debug, Default)]
pub struct IncomingInvocationContext {
	pub(crate) ray_id: Option<String>,
	remote_parent: Option<SpanContext>,
}

/// Header carrying the caller's ray ID into an actor.
pub(crate) const HEADER_RIVET_RAY_ID: &str = "x-rivet-ray-id";

impl IncomingInvocationContext {
	pub(crate) fn from_headers(
		ray_id: Option<String>,
		traceparent: Option<&str>,
		tracestate: Option<&str>,
	) -> Self {
		Self {
			ray_id,
			remote_parent: parse_remote_parent(traceparent, tracestate),
		}
	}

	/// Reads the ray ID and W3C trace context an HTTP request carries. Every
	/// HTTP entry point into an actor reads them through here, so they all
	/// apply the same bounds.
	pub(crate) fn from_http_headers(headers: &http::HeaderMap) -> Self {
		Self::from_headers(
			invocation_ray_id(headers),
			headers
				.get("traceparent")
				.and_then(|value| value.to_str().ok()),
			headers
				.get("tracestate")
				.and_then(|value| value.to_str().ok()),
		)
	}
}

/// Reads the caller's ray ID. The header is untrusted, so it is bounded to
/// 30 characters of `[A-Za-z0-9_-]`; anything else counts as absent.
fn invocation_ray_id(headers: &http::HeaderMap) -> Option<String> {
	headers
		.get(HEADER_RIVET_RAY_ID)?
		.to_str()
		.ok()
		.filter(|value| {
			!value.is_empty()
				&& value.len() <= 30
				&& value
					.bytes()
					.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
		})
		.map(str::to_owned)
}

/// The single root span for one client action invocation.
#[derive(Debug)]
pub(crate) struct ActionInvocationSpan {
	span: Option<tracing::Span>,
}

impl ActionInvocationSpan {
	pub(crate) fn start(
		ctx: &ActorContext,
		action_name: &str,
		incoming: IncomingInvocationContext,
	) -> Self {
		if !tracing::enabled!(target: "rivetkit::telemetry", tracing::Level::INFO) {
			return Self { span: None };
		}

		let span = tracing::info_span!(
			target: "rivetkit::telemetry",
			parent: None,
			"rivet.actor.invoke",
			otel.kind = "server",
			rivet.invocation.type = "action",
			rivet.actor.id = %ctx.actor_id(),
			rivet.actor.name = %ctx.name(),
			rivet.actor.key = %format_actor_key(ctx.key()),
			rivet.action.name = %action_name,
			rivet.ray.id = tracing::field::Empty,
			otel.status_code = tracing::field::Empty,
			error.type = tracing::field::Empty,
		);
		if let Some(ray_id) = incoming.ray_id.as_deref() {
			span.record("rivet.ray.id", ray_id);
		}
		if let Some(parent) = incoming.remote_parent {
			span.set_parent(opentelemetry::Context::new().with_remote_span_context(parent));
		}

		Self { span: Some(span) }
	}

	pub(crate) fn finish(mut self, error: Option<&anyhow::Error>) {
		let Some(span) = self.span.take() else {
			return;
		};
		span.record(
			"otel.status_code",
			if error.is_none() { "OK" } else { "ERROR" },
		);
		if let Some(error) = error {
			let error = rivet_error::RivetError::extract(error);
			span.record("error.type", format!("{}.{}", error.group(), error.code()));
		}
	}
}

impl Drop for ActionInvocationSpan {
	fn drop(&mut self) {
		let Some(span) = self.span.take() else {
			return;
		};
		span.record("otel.status_code", "ERROR");
		span.record("error.type", "actor.dropped_reply");
	}
}

fn parse_remote_parent(traceparent: Option<&str>, tracestate: Option<&str>) -> Option<SpanContext> {
	traceparent?;
	let context = TraceContextPropagator::new().extract(&TraceHeaders {
		traceparent,
		tracestate,
	});
	let span = context.span();
	if span.span_context().is_valid() {
		Some(span.span_context().clone())
	} else {
		None
	}
}

struct TraceHeaders<'a> {
	traceparent: Option<&'a str>,
	tracestate: Option<&'a str>,
}

impl Extractor for TraceHeaders<'_> {
	fn get(&self, key: &str) -> Option<&str> {
		match key {
			"traceparent" => self.traceparent,
			"tracestate" => self.tracestate,
			_ => None,
		}
	}

	fn keys(&self) -> Vec<&str> {
		let mut keys = Vec::with_capacity(2);
		if self.traceparent.is_some() {
			keys.push("traceparent");
		}
		if self.tracestate.is_some() {
			keys.push("tracestate");
		}
		keys
	}
}
