//! Internal OpenTelemetry spans owned by the actor runtime.

#[cfg(feature = "native-runtime")]
pub mod export;

use std::sync::Arc;

use opentelemetry::Context;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator as _};
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

pub(crate) use rivetkit_client_protocol::telemetry_headers::HEADER_RIVET_RAY_ID;

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

/// Reads the caller's ray ID. The header is untrusted, so it is bounded by
/// the rule shared with the clients that send it; anything else counts as
/// absent.
fn invocation_ray_id(headers: &http::HeaderMap) -> Option<String> {
	let value = headers.get(HEADER_RIVET_RAY_ID)?.to_str().ok()?;
	rivetkit_client_protocol::telemetry_headers::bounded_ray_id(value).map(str::to_owned)
}

/// Owns the complete lifecycle of one actor invocation.
#[derive(Debug)]
pub(crate) struct ActorInvocation {
	telemetry: ActorInvocationTelemetry,
}

#[derive(Clone, Copy, Debug)]
enum InvocationType {
	Action,
	Scheduled,
}

impl InvocationType {
	fn as_label(self) -> &'static str {
		match self {
			Self::Action => "action",
			Self::Scheduled => "scheduled",
		}
	}
}

/// Opaque invocation context carried across foreign-runtime adapters.
///
/// Every clone of a handle shares one invocation. `application_span` is the
/// span the host runtime had active when it resolved this handle; Core cannot
/// see the host's span stack, so spans opened through the handle parent there
/// when it is set and to the invocation span otherwise.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ActorInvocationTelemetry {
	inner: Arc<InvocationInner>,
	application_span: Option<SpanContext>,
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
	ray_id: Option<String>,
	// This lock is used from Drop paths, and its guard never crosses an await.
	state: Mutex<InvocationState>,
	identity: Arc<ActorTelemetryIdentity>,
}

#[derive(Debug)]
struct InvocationState {
	span: Option<tracing::Span>,
	finished: bool,
	pending_work: usize,
}

const OPERATION_ABANDONED_ERROR_TYPE: &str = "actor.operation_abandoned";

/// Keeps an invocation open while one piece of `wait_until` work runs. The
/// span is released when the last guard drops after the terminal status has
/// been recorded, so work that settles before the reply changes nothing.
pub(crate) struct InvocationWorkGuard(ActorInvocationTelemetry);

/// Where later work came from: the ray ID of the invocation that caused it and
/// the span that was active there. Persisted beside schedules and queue
/// messages so the work they cause can link back to its origin.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TraceOrigin {
	pub(crate) ray_id: Option<String>,
	pub(crate) traceparent: Option<String>,
	pub(crate) tracestate: Option<String>,
}

/// Active actor invocation fields exposed to foreign-runtime adapters.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ActorInvocationTraceContext {
	pub ray_id: Option<String>,
	/// Present only while the invocation runs inside a valid span.
	pub span: Option<ActorInvocationSpanContext>,
}

/// W3C span context of the current invocation span. A span context is either
/// complete or absent, so these fields are never optional individually.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ActorInvocationSpanContext {
	pub trace_id: String,
	pub span_id: String,
	pub trace_flags: u8,
	pub tracestate: Option<String>,
}

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

impl ActorInvocation {
	pub(crate) fn start_action(
		ctx: &ActorContext,
		action_name: &str,
		incoming: IncomingInvocationContext,
	) -> Self {
		Self::start(
			ctx,
			action_name,
			InvocationType::Action,
			incoming.ray_id,
			incoming.remote_parent,
			None,
		)
	}

	pub(crate) fn start_scheduled(
		ctx: &ActorContext,
		action_name: &str,
		origin: TraceOrigin,
	) -> Self {
		let origin_parent =
			parse_remote_parent(origin.traceparent.as_deref(), origin.tracestate.as_deref());
		Self::start(
			ctx,
			action_name,
			InvocationType::Scheduled,
			origin.ray_id,
			None,
			origin_parent,
		)
	}

	fn start(
		ctx: &ActorContext,
		action_name: &str,
		invocation_type: InvocationType,
		ray_id: Option<String>,
		parent: Option<SpanContext>,
		link: Option<SpanContext>,
	) -> Self {
		let identity = ctx.telemetry_identity();
		let span = if tracing::enabled!(target: "rivetkit::telemetry", tracing::Level::INFO) {
			let span = tracing::info_span!(
				target: "rivetkit::telemetry",
				parent: None,
				"rivet.actor.invoke",
				otel.name = %format!("{}/{}", identity.actor_name, action_name),
				otel.kind = otel_kind(invocation_type),
				rivet.invocation.type = invocation_type.as_label(),
				rivet.actor.id = %identity.actor_id,
				rivet.actor.name = %identity.actor_name,
				rivet.actor.key = %identity.actor_key,
				rivet.action.name = %action_name,
				rivet.ray.id = tracing::field::Empty,
				otel.status_code = tracing::field::Empty,
				error.type = tracing::field::Empty,
			);
			if let Some(ray_id) = ray_id.as_deref() {
				span.record("rivet.ray.id", ray_id);
			}
			if let Some(parent) = parent {
				span.set_parent(Context::new().with_remote_span_context(parent));
			}
			if let Some(link) = link {
				span.add_link(link);
			}
			Some(span)
		} else {
			None
		};

		Self {
			telemetry: ActorInvocationTelemetry::new(ray_id, span, identity),
		}
	}

	pub(crate) fn telemetry(&self) -> ActorInvocationTelemetry {
		self.telemetry.clone()
	}

	pub(crate) fn finish(self, error: Option<&anyhow::Error>) {
		self.telemetry.finish(error);
	}
}

impl Drop for ActorInvocation {
	fn drop(&mut self) {
		self.telemetry.finish_dropped();
	}
}

impl ActorInvocationTelemetry {
	fn new(
		ray_id: Option<String>,
		span: Option<tracing::Span>,
		identity: Arc<ActorTelemetryIdentity>,
	) -> Self {
		Self {
			inner: Arc::new(InvocationInner {
				ray_id,
				state: Mutex::new(InvocationState {
					span,
					finished: false,
					pending_work: 0,
				}),
				identity,
			}),
			application_span: None,
		}
	}

	/// Returns a handle for the same invocation whose spans parent to the
	/// application span identified by `traceparent` and `tracestate`. Absent
	/// or invalid context, or the invocation span itself, which the host sees
	/// as active when no application span is open, yields a handle that
	/// parents to the invocation span.
	pub(crate) fn with_application_span(
		&self,
		traceparent: Option<&str>,
		tracestate: Option<&str>,
	) -> Self {
		let application_span = parse_remote_parent(traceparent, tracestate).filter(|parent| {
			self.inner
				.state
				.lock()
				.span
				.as_ref()
				.and_then(otel_span_context_of)
				.map_or(true, |own| own.span_id() != parent.span_id())
		});
		Self {
			inner: self.inner.clone(),
			application_span,
		}
	}

	/// The context a span opened through this handle parents to: the
	/// application span when set, else the invocation span while it is open.
	fn parent_context(&self) -> Option<Context> {
		let state = self.inner.state.lock();
		if state.finished && state.pending_work == 0 {
			return None;
		}
		match &self.application_span {
			Some(application_span) => {
				Some(Context::new().with_remote_span_context(application_span.clone()))
			}
			None => state.span.as_ref().map(tracing::Span::context),
		}
	}

	/// Registers work that outlives the reply, so the invocation span stays
	/// open and keeps parenting operations until the returned guard drops.
	pub(crate) fn hold_open(&self) -> Option<InvocationWorkGuard> {
		let mut state = self.inner.state.lock();
		if state.finished {
			return None;
		}
		state.pending_work += 1;
		Some(InvocationWorkGuard(self.clone()))
	}

	/// Returns correlation fields only while this actor invocation is active.
	#[doc(hidden)]
	pub fn trace_context(&self) -> Option<ActorInvocationTraceContext> {
		let span = {
			let state = self.inner.state.lock();
			if state.finished && state.pending_work == 0 {
				return None;
			}
			state.span.clone()
		};
		let span = span.and_then(|span| {
			let context = span.context();
			let context_span = context.span();
			w3c_span_context(context_span.span_context())
		});

		Some(ActorInvocationTraceContext {
			ray_id: self.inner.ray_id.clone(),
			span,
		})
	}

	/// Trace origin work caused by this invocation records: the invocation's
	/// ray ID, and the application span active in the host runtime at that
	/// moment, or the invocation span when there was none. Work that links
	/// back to it then points at the code that caused it rather than at the
	/// whole invocation around that code.
	pub(crate) fn trace_origin(&self) -> TraceOrigin {
		let Some(parent) = self.parent_context() else {
			return TraceOrigin::default();
		};
		let parent_span = parent.span();
		let headers = w3c_trace_headers(parent_span.span_context());
		TraceOrigin {
			ray_id: self.inner.ray_id.clone(),
			traceparent: headers.traceparent,
			tracestate: headers.tracestate,
		}
	}

	pub(crate) fn start_sqlite(&self, operation: SqliteOperation) -> Option<SqliteOperationSpan> {
		let parent = self.parent_context()?;
		let (span_name, operation_name) = operation.names();
		let span = tracing::info_span!(
			target: "rivetkit::telemetry",
			parent: None,
			"rivet.sqlite.operation",
			otel.name = span_name,
			otel.kind = "internal",
			rivet.operation.system = "sqlite",
			rivet.operation.name = operation_name,
			rivet.ray.id = self.inner.ray_id.as_deref(),
			rivet.actor.id = %self.inner.identity.actor_id,
			rivet.actor.name = %self.inner.identity.actor_name,
			rivet.actor.key = %self.inner.identity.actor_key,
			otel.status_code = tracing::field::Empty,
			error.type = tracing::field::Empty,
		);
		span.set_parent(parent);
		Some(SqliteOperationSpan { span: Some(span) })
	}

	fn finish(&self, error: Option<&anyhow::Error>) {
		self.finish_with(|span| record_outcome(span, error));
	}

	fn finish_dropped(&self) {
		self.finish_with(|span| {
			span.record("otel.status_code", "ERROR");
			span.record("error.type", "actor.dropped_reply");
		});
	}

	fn finish_with(&self, record: impl FnOnce(&tracing::Span)) {
		let mut state = self.inner.state.lock();
		if state.finished {
			return;
		}
		if let Some(span) = state.span.as_ref() {
			record(span);
			if state.pending_work > 0 {
				tracing::info!(target: "rivetkit::telemetry", parent: span, "reply sent");
			}
		}
		state.finished = true;
		if state.pending_work == 0 {
			state.span.take();
		}
	}
}

impl Drop for InvocationWorkGuard {
	fn drop(&mut self) {
		let mut state = self.0.inner.state.lock();
		state.pending_work -= 1;
		if state.pending_work == 0 && state.finished {
			state.span.take();
		}
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

fn otel_span_context_of(span: &tracing::Span) -> Option<SpanContext> {
	let context = span.context();
	let context_span = context.span();
	let span_context = context_span.span_context();
	if span_context.is_valid() {
		Some(span_context.clone())
	} else {
		None
	}
}

/// W3C fields of a span context, or nothing when it is not valid and so
/// carries nothing worth propagating.
fn w3c_span_context(span_context: &SpanContext) -> Option<ActorInvocationSpanContext> {
	if !span_context.is_valid() {
		return None;
	}
	let tracestate = span_context.trace_state().header();
	Some(ActorInvocationSpanContext {
		trace_id: span_context.trace_id().to_string(),
		span_id: span_context.span_id().to_string(),
		trace_flags: span_context.trace_flags().to_u8(),
		tracestate: if tracestate.is_empty() {
			None
		} else {
			Some(tracestate)
		},
	})
}

fn w3c_trace_headers(span_context: &SpanContext) -> OwnedTraceHeaders {
	if !span_context.is_valid() {
		return OwnedTraceHeaders::default();
	}
	let context = Context::new().with_remote_span_context(span_context.clone());
	let mut headers = OwnedTraceHeaders::default();
	TraceContextPropagator::new().inject_context(&context, &mut headers);
	headers
}

/// An action is entered from outside the actor; a scheduled fire originates
/// inside it.
fn otel_kind(invocation_type: InvocationType) -> &'static str {
	match invocation_type {
		InvocationType::Action => "server",
		InvocationType::Scheduled => "internal",
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

#[derive(Default)]
struct OwnedTraceHeaders {
	traceparent: Option<String>,
	tracestate: Option<String>,
}

impl Injector for OwnedTraceHeaders {
	fn set(&mut self, key: &str, value: String) {
		match key {
			"traceparent" => self.traceparent = Some(value),
			"tracestate" if !value.is_empty() => self.tracestate = Some(value),
			_ => {}
		}
	}
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
