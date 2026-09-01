//! Internal OpenTelemetry spans owned by the actor runtime.

#[cfg(feature = "native-runtime")]
pub mod export;

use std::sync::Arc;

use opentelemetry::propagation::{Extractor, TextMapPropagator as _};
use opentelemetry::trace::{SpanContext, TraceContextExt as _};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use parking_lot::Mutex;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::ActorContext;

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
/// 128 characters of `[A-Za-z0-9_-]`; anything else counts as absent.
fn invocation_ray_id(headers: &http::HeaderMap) -> Option<String> {
	headers
		.get(HEADER_RIVET_RAY_ID)?
		.to_str()
		.ok()
		.filter(|value| {
			!value.is_empty()
				&& value.len() <= 128
				&& value
					.bytes()
					.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
		})
		.map(str::to_owned)
}

/// The single root span for one client action invocation.
#[derive(Debug)]
pub(crate) struct ActionInvocationSpan {
	telemetry: ActorInvocationTelemetry,
}

/// Opaque invocation context carried across foreign-runtime adapters.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ActorInvocationTelemetry {
	inner: Arc<InvocationInner>,
}

/// Identity fields that do not change while an actor is alive. Built once per
/// actor and shared by every invocation, so starting one does not re-allocate
/// them.
#[derive(Debug)]
pub(crate) struct ActorTelemetryIdentity {
	pub(crate) actor_id: String,
	pub(crate) actor_name: String,
	pub(crate) actor_key: String,
}

#[derive(Debug)]
struct InvocationInner {
	// This lock is used from Drop paths, and its guard never crosses an await.
	state: Mutex<InvocationState>,
	identity: Arc<ActorTelemetryIdentity>,
}

#[derive(Debug)]
struct InvocationState {
	span: Option<tracing::Span>,
	finished: bool,
}

const OPERATION_ABANDONED_ERROR_TYPE: &str = "actor.operation_abandoned";

/// The closed set of SQLite operations that get a span.
///
/// Both names are `&'static str`, so starting one of these spans allocates
/// nothing. Adding an operation is a compile error here rather than a silently
/// wrong span name.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SqliteOperation {
	Exec,
	Execute,
	ExecuteBatch,
	Query,
	Run,
	TransactionBegin,
	TransactionExec,
	TransactionExecute,
	TransactionCommit,
	TransactionRollback,
}

impl SqliteOperation {
	fn names(self) -> (&'static str, &'static str) {
		match self {
			Self::Exec => ("rivet.sqlite.exec", "exec"),
			Self::Execute => ("rivet.sqlite.execute", "execute"),
			Self::ExecuteBatch => ("rivet.sqlite.execute_batch", "execute_batch"),
			Self::Query => ("rivet.sqlite.query", "query"),
			Self::Run => ("rivet.sqlite.run", "run"),
			Self::TransactionBegin => ("rivet.sqlite.transaction.begin", "transaction.begin"),
			Self::TransactionExec => ("rivet.sqlite.transaction.exec", "transaction.exec"),
			Self::TransactionExecute => ("rivet.sqlite.transaction.execute", "transaction.execute"),
			Self::TransactionCommit => ("rivet.sqlite.transaction.commit", "transaction.commit"),
			Self::TransactionRollback => {
				("rivet.sqlite.transaction.rollback", "transaction.rollback")
			}
		}
	}
}

pub(crate) struct SqliteOperationSpan {
	span: Option<tracing::Span>,
}

impl ActionInvocationSpan {
	pub(crate) fn start(
		ctx: &ActorContext,
		action_name: &str,
		incoming: IncomingInvocationContext,
	) -> Self {
		let identity = ctx.telemetry_identity();
		let span = if tracing::enabled!(target: "rivetkit::telemetry", tracing::Level::INFO) {
			let span = tracing::info_span!(
				target: "rivetkit::telemetry",
				parent: None,
				"rivet.actor.invoke",
				otel.kind = "server",
				rivet.invocation.type = "action",
				rivet.actor.id = %identity.actor_id,
				rivet.actor.name = %identity.actor_name,
				rivet.actor.key = %identity.actor_key,
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
			Some(span)
		} else {
			None
		};

		Self {
			telemetry: ActorInvocationTelemetry::new(span, identity),
		}
	}

	pub(crate) fn telemetry(&self) -> ActorInvocationTelemetry {
		self.telemetry.clone()
	}

	pub(crate) fn finish(self, error: Option<&anyhow::Error>) {
		self.telemetry.finish(error);
	}
}

impl Drop for ActionInvocationSpan {
	fn drop(&mut self) {
		self.telemetry.finish_dropped();
	}
}

impl ActorInvocationTelemetry {
	fn new(span: Option<tracing::Span>, identity: Arc<ActorTelemetryIdentity>) -> Self {
		Self {
			inner: Arc::new(InvocationInner {
				state: Mutex::new(InvocationState {
					span,
					finished: false,
				}),
				identity,
			}),
		}
	}

	pub(crate) fn start_sqlite(&self, operation: SqliteOperation) -> Option<SqliteOperationSpan> {
		let parent = {
			let state = self.inner.state.lock();
			if state.finished {
				return None;
			}
			state.span.clone()?
		};
		let (span_name, operation_name) = operation.names();
		let span = tracing::info_span!(
			target: "rivetkit::telemetry",
			parent: &parent,
			"rivet.sqlite.operation",
			otel.name = span_name,
			otel.kind = "internal",
			rivet.operation.system = "sqlite",
			rivet.operation.name = operation_name,
			rivet.actor.id = %self.inner.identity.actor_id,
			rivet.actor.name = %self.inner.identity.actor_name,
			rivet.actor.key = %self.inner.identity.actor_key,
			otel.status_code = tracing::field::Empty,
			error.type = tracing::field::Empty,
		);
		Some(SqliteOperationSpan { span: Some(span) })
	}

	fn finish(&self, error: Option<&anyhow::Error>) {
		let Some(span) = self.take_span() else {
			return;
		};
		record_outcome(&span, error);
	}

	fn finish_dropped(&self) {
		let Some(span) = self.take_span() else {
			return;
		};
		span.record("otel.status_code", "ERROR");
		span.record("error.type", "actor.dropped_reply");
	}

	/// Claims the terminal record, so the finish and drop paths cannot both
	/// record a status for the same invocation.
	fn take_span(&self) -> Option<tracing::Span> {
		let mut state = self.inner.state.lock();
		if state.finished {
			return None;
		}
		state.finished = true;
		state.span.take()
	}
}

impl SqliteOperationSpan {
	pub(crate) fn span(&self) -> tracing::Span {
		self.span.as_ref().expect("sqlite span is present").clone()
	}

	pub(crate) fn finish(&mut self, error: Option<&anyhow::Error>) {
		let Some(span) = self.span.take() else {
			return;
		};
		record_outcome(&span, error);
	}
}

impl Drop for SqliteOperationSpan {
	fn drop(&mut self) {
		let Some(span) = self.span.take() else {
			return;
		};
		span.record("otel.status_code", "ERROR");
		span.record("error.type", OPERATION_ABANDONED_ERROR_TYPE);
	}
}

/// Records the terminal status and error identity of a finished span.
fn record_outcome(span: &tracing::Span, error: Option<&anyhow::Error>) {
	span.record(
		"otel.status_code",
		if error.is_none() { "OK" } else { "ERROR" },
	);
	if let Some(error) = error {
		let error = rivet_error::RivetError::extract(error);
		span.record("error.type", format!("{}.{}", error.group(), error.code()));
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
