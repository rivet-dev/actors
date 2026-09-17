//! Internal OpenTelemetry spans owned by the actor runtime.

#[cfg(feature = "native-runtime")]
pub mod export;
#[cfg(feature = "native-runtime")]
mod sampler;

use std::sync::Arc;

use opentelemetry::Context;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator as _};
use opentelemetry::trace::{SpanContext, TraceContextExt as _};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use parking_lot::Mutex;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::actor::queue::QueueMessage;
use crate::{ActorContext, ActorTracingConfig};

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

/// Span attribute carrying the ratio an actor configured for the trace this
/// invocation starts. The export sampler reads it, so it has to be recorded
/// before anything reads the span's context.
pub(crate) const SAMPLE_RATIO_ATTRIBUTE: &str = "rivet.sampling.ratio";

/// Name a request invocation is reported under, in place of a caller-supplied path.
const REQUEST_INVOCATION_NAME: &str = "onRequest";

/// Name a queue send invocation is reported under. The queue itself is an attribute.
const QUEUE_SEND_INVOCATION_NAME: &str = "queue.send";

/// What an invocation ran, which decides its name and the attributes that
/// identify it on the span.
#[derive(Clone, Copy, Debug)]
enum InvocationSubject<'a> {
	Action(&'a str),
	Request { method: &'a str },
	QueueSend { queue: &'a str },
}

impl<'a> InvocationSubject<'a> {
	fn name(self) -> &'a str {
		match self {
			Self::Action(name) => name,
			Self::Request { .. } => REQUEST_INVOCATION_NAME,
			Self::QueueSend { .. } => QUEUE_SEND_INVOCATION_NAME,
		}
	}

	/// Ratio the actor configured for traces this invocation starts, if any.
	fn sample_ratio(self, config: &ActorTracingConfig) -> Option<f64> {
		match self {
			Self::Action(name) => config.sample_ratio_for_action(name),
			Self::Request { .. } | Self::QueueSend { .. } => config.sample_ratio,
		}
	}

	fn record_attributes(self, span: &tracing::Span) {
		match self {
			Self::Action(name) => span.record("rivet.action.name", name),
			Self::Request { method } => span.record("http.request.method", method),
			Self::QueueSend { queue } => span.record("rivet.queue.name", queue),
		};
	}
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
	Request,
	QueueSend,
}

impl InvocationType {
	fn as_label(self) -> &'static str {
		match self {
			Self::Action => "action",
			Self::Scheduled => "scheduled",
			Self::Request => "request",
			Self::QueueSend => "queue_send",
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
/// messages so the work they cause can link back to the invocation that caused it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IncomingTraceContext {
	pub ray_id: Option<String>,
	pub traceparent: Option<String>,
	pub tracestate: Option<String>,
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

/// One call from this invocation out to another actor, held open across a
/// foreign-runtime boundary.
///
/// The call is made by the host runtime's client, so it is opened and closed by
/// two separate calls rather than by one Rust scope. Dropping this without
/// finishing records the call as cancelled, matching how a dropped SQLite span
/// is treated.
#[doc(hidden)]
pub struct OutboundCallInvocation {
	span: Option<tracing::Span>,
	context: Option<ActorInvocationSpanContext>,
}

impl ActorInvocation {
	pub(crate) fn start_action(
		ctx: &ActorContext,
		action_name: &str,
		incoming: IncomingInvocationContext,
	) -> Self {
		Self::start(
			ctx,
			InvocationSubject::Action(action_name),
			InvocationType::Action,
			incoming.ray_id,
			incoming.remote_parent,
			None,
		)
	}

	pub(crate) fn start_scheduled(
		ctx: &ActorContext,
		action_name: &str,
		trace_context: IncomingTraceContext,
	) -> Self {
		let remote_parent = parse_remote_parent(
			trace_context.traceparent.as_deref(),
			trace_context.tracestate.as_deref(),
		);
		Self::start(
			ctx,
			InvocationSubject::Action(action_name),
			InvocationType::Scheduled,
			trace_context.ray_id,
			None,
			remote_parent,
		)
	}

	/// Starts the invocation for one message sent into `queue_name` from
	/// outside the actor. It ends when the send is acknowledged, or when the
	/// sender's wait for a completion ends.
	pub(crate) fn start_queue_send(
		ctx: &ActorContext,
		queue_name: &str,
		incoming: IncomingInvocationContext,
	) -> Self {
		Self::start(
			ctx,
			InvocationSubject::QueueSend { queue: queue_name },
			InvocationType::QueueSend,
			incoming.ray_id,
			incoming.remote_parent,
			None,
		)
	}

	/// Starts the invocation for one raw HTTP request served by `onRequest`.
	/// The span is named after the handler rather than the path, because a
	/// path is caller-supplied and would make the name a cardinality surface.
	pub(crate) fn start_request(
		ctx: &ActorContext,
		request: &crate::actor::messages::Request,
		incoming: IncomingInvocationContext,
	) -> Self {
		Self::start(
			ctx,
			InvocationSubject::Request {
				method: request.method().as_str(),
			},
			InvocationType::Request,
			incoming.ray_id,
			incoming.remote_parent,
			None,
		)
	}

	fn start(
		ctx: &ActorContext,
		subject: InvocationSubject<'_>,
		invocation_type: InvocationType,
		ray_id: Option<String>,
		parent: Option<SpanContext>,
		link: Option<SpanContext>,
	) -> Self {
		let identity = ctx.telemetry_identity();
		let subject_name = subject.name();
		let span = if tracing::enabled!(target: "rivetkit::telemetry", tracing::Level::INFO) {
			let span = tracing::info_span!(
				target: "rivetkit::telemetry",
				parent: None,
				"rivet.actor.invoke",
				otel.name = %format!("{}/{}", identity.actor_name, subject_name),
				otel.kind = otel_kind(invocation_type),
				rivet.invocation.type = invocation_type.as_label(),
				rivet.actor.id = %identity.actor_id,
				rivet.actor.name = %identity.actor_name,
				rivet.actor.key = %identity.actor_key,
				rivet.action.name = tracing::field::Empty,
				rivet.ray.id = tracing::field::Empty,
				http.request.method = tracing::field::Empty,
				http.response.status_code = tracing::field::Empty,
				rivet.queue.name = tracing::field::Empty,
				rivet.sampling.ratio = tracing::field::Empty,
				otel.status_code = tracing::field::Empty,
				error.type = tracing::field::Empty,
			);
			if let Some(ray_id) = ray_id.as_deref() {
				span.record("rivet.ray.id", ray_id);
			}
			subject.record_attributes(&span);
			if let Some(sample_ratio) = subject.sample_ratio(ctx.tracing_config()) {
				span.record(SAMPLE_RATIO_ATTRIBUTE, sample_ratio);
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

	pub(crate) fn finish_request(
		self,
		response: std::result::Result<&crate::actor::messages::ActorHttpResponse, &anyhow::Error>,
	) {
		match response {
			Ok(response) => {
				let status = response.status();
				self.telemetry.finish_with(|span| {
					span.record("http.response.status_code", status);
					if status >= 500 {
						span.record("otel.status_code", "ERROR");
						span.record("error.type", status.to_string());
					} else {
						record_outcome(span, None);
					}
				});
			}
			Err(error) => self.finish(Some(error)),
		}
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
		let span = span.and_then(|span| span_context_of(&span));

		Some(ActorInvocationTraceContext {
			ray_id: self.inner.ray_id.clone(),
			span,
		})
	}

	/// Trace context that work caused by this invocation records: the invocation's
	/// ray ID, and the application span active in the host runtime at that
	/// moment, or the invocation span when there was none. Work that links
	/// back to it then points at the code that caused it rather than at the
	/// whole invocation around that code.
	pub(crate) fn incoming_trace_context(&self) -> IncomingTraceContext {
		let Some(parent) = self.parent_context() else {
			return IncomingTraceContext::default();
		};
		let parent_span = parent.span();
		let headers = w3c_trace_headers(parent_span.span_context());
		IncomingTraceContext {
			ray_id: self.inner.ray_id.clone(),
			traceparent: headers.traceparent,
			tracestate: headers.tracestate,
		}
	}

	/// Opens the span covering one call out to another actor.
	///
	/// The callee parents to this span rather than to the invocation making the
	/// call, so the time spent reaching it, which includes routing and waking a
	/// sleeping actor, is attributed to the call instead of falling in the gap
	/// between the two invocations.
	/// When this handle carries the application span active in the host
	/// runtime, the call span parents there instead, which is what puts a
	/// callee under the application span that issued the call rather than
	/// beside it.
	pub(crate) fn start_outbound_call(
		&self,
		actor_name: &str,
		action_name: &str,
	) -> Option<OutboundCallInvocation> {
		let parent = self.parent_context()?;
		let span = tracing::info_span!(
			target: "rivetkit::telemetry",
			parent: None,
			"rivet.actor.call",
			otel.name = %format!("{actor_name}/{action_name}"),
			otel.kind = "client",
			rivet.actor.name = %actor_name,
			rivet.action.name = %action_name,
			rivet.ray.id = self.inner.ray_id.as_deref(),
			otel.status_code = tracing::field::Empty,
			error.type = tracing::field::Empty,
		);
		span.set_parent(parent);
		let context = span_context_of(&span);
		Some(OutboundCallInvocation {
			span: Some(span),
			context,
		})
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

impl OutboundCallInvocation {
	/// W3C context of this call's span, to send to the callee so it parents
	/// here. Absent when the call is not sampled.
	pub fn span_context(&self) -> Option<ActorInvocationSpanContext> {
		self.context.clone()
	}

	/// Records the call's outcome. `error` is the failure the callee returned,
	/// and its group and code become the span's `error.type`.
	pub fn finish(mut self, error: Option<&anyhow::Error>) {
		let Some(span) = self.span.take() else {
			return;
		};
		record_outcome(&span, error);
	}
}

impl Drop for OutboundCallInvocation {
	fn drop(&mut self) {
		let Some(span) = self.span.take() else {
			return;
		};
		span.record("otel.status_code", "ERROR");
		span.record("error.type", OPERATION_ABANDONED_ERROR_TYPE);
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

/// An action or a raw HTTP request is entered from outside the actor, a
/// scheduled fire originates inside it, and a queue send produces a message
/// the actor consumes later.
fn otel_kind(invocation_type: InvocationType) -> &'static str {
	match invocation_type {
		InvocationType::Action | InvocationType::Request => "server",
		InvocationType::Scheduled => "internal",
		InvocationType::QueueSend => "producer",
	}
}

/// Reads a span's W3C context, or nothing when it carries no valid context to
/// propagate.
fn span_context_of(span: &tracing::Span) -> Option<ActorInvocationSpanContext> {
	let context = span.context();
	let context_span = context.span();
	w3c_span_context(context_span.span_context())
}

/// Opens the span covering the moment one queue message is handed to the
/// actor. It links to the span that sent the message, which is what connects
/// the consumer's trace to the sender's. Inside an invocation it sits under
/// that invocation's application span, or the invocation span, and carries its
/// ray ID. Outside one, as from the run handler, it is a root span carrying the
/// ray ID the message was sent under. It closes when the caller drops it, which
/// `try_receive_batch` does as it hands the message back.
pub(crate) fn start_queue_receive(ctx: &ActorContext, message: &QueueMessage) -> tracing::Span {
	if !tracing::enabled!(target: "rivetkit::telemetry", tracing::Level::INFO) {
		return tracing::Span::none();
	}
	let identity = ctx.telemetry_identity();
	let span = tracing::info_span!(
		target: "rivetkit::telemetry",
		parent: None,
		"rivet.queue.receive",
		otel.name = %format!("{}/queue.receive", identity.actor_name),
		otel.kind = "consumer",
		rivet.actor.id = %identity.actor_id,
		rivet.actor.name = %identity.actor_name,
		rivet.actor.key = %identity.actor_key,
		rivet.queue.name = message.name,
		rivet.ray.id = tracing::field::Empty,
	);
	let invocation_parent = ctx
		.invocation_telemetry()
		.and_then(|telemetry| telemetry.parent_context().map(|parent| (telemetry, parent)));
	if let Some((telemetry, parent)) = invocation_parent {
		if let Some(ray_id) = telemetry.inner.ray_id.as_deref() {
			span.record("rivet.ray.id", ray_id);
		}
		span.set_parent(parent);
	} else if let Some(ray_id) = &message.trace_context.ray_id {
		span.record("rivet.ray.id", ray_id);
	}
	if let Some(link) = parse_remote_parent(
		message.trace_context.traceparent.as_deref(),
		message.trace_context.tracestate.as_deref(),
	) {
		span.add_link(link);
	}
	span
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
