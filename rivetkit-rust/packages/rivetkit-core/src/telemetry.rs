//! Internal OpenTelemetry spans owned by the actor runtime.

use std::str::FromStr as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use opentelemetry::trace::{
	SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState,
};
use parking_lot::Mutex;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::ActorContext;
use crate::actor::metrics::{ActorMetrics, InvocationStatus, InvocationType};
use crate::time::Instant;

/// Correlation fields accepted at an invocation boundary.
#[derive(Debug, Default)]
pub struct IncomingInvocationContext {
	pub(crate) ray_id: Option<String>,
	remote_parent: Option<SpanContext>,
}

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
}

/// Owns the complete lifecycle of one actor invocation.
#[derive(Debug)]
pub(crate) struct ActorInvocation {
	telemetry: ActorInvocationTelemetry,
	metrics: ActorMetrics,
	action_name: String,
	invocation_type: InvocationType,
	started_at: Instant,
}

/// Opaque invocation context carried across foreign-runtime adapters.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ActorInvocationTelemetry(Arc<InvocationInner>);

/// Identity fields that do not change while an actor is alive. Built once per
/// actor and shared by every invocation, so starting one does not re-allocate
/// them.
#[derive(Debug)]
pub(crate) struct ActorTelemetryIdentity {
	pub(crate) actor_id: String,
	pub(crate) actor_name: String,
	pub(crate) actor_key: String,
}

/// Shared invocation state. Only the span slot is mutable: whichever of the
/// finish and drop paths runs first takes it, which both records the terminal
/// status once and drops the span, and dropping the span is what exports it.
/// `finished` marks the invocation closed even when tracing is off and there
/// is no span to take.
#[derive(Debug)]
struct InvocationInner {
	ray_id: String,
	span: Mutex<Option<tracing::Span>>,
	finished: AtomicBool,
	identity: Arc<ActorTelemetryIdentity>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ScheduleTraceOrigin {
	pub(crate) ray_id: Option<String>,
	pub(crate) traceparent: Option<String>,
	pub(crate) tracestate: Option<String>,
}

/// Active actor invocation fields exposed to foreign-runtime adapters.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ActorInvocationTraceContext {
	pub ray_id: String,
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
	pub traceparent: String,
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
	fn as_str(self) -> &'static str {
		match self {
			Self::Exec => "exec",
			Self::Execute => "execute",
			Self::ExecuteBatch => "execute_batch",
			Self::Query => "query",
			Self::Run => "run",
			Self::TransactionBegin => "transaction.begin",
			Self::TransactionExec => "transaction.exec",
			Self::TransactionExecute => "transaction.execute",
			Self::TransactionCommit => "transaction.commit",
			Self::TransactionRollback => "transaction.rollback",
		}
	}

	fn span_name(self) -> &'static str {
		match self {
			Self::Exec => "rivet.sqlite.exec",
			Self::Execute => "rivet.sqlite.execute",
			Self::ExecuteBatch => "rivet.sqlite.execute_batch",
			Self::Query => "rivet.sqlite.query",
			Self::Run => "rivet.sqlite.run",
			Self::TransactionBegin => "rivet.sqlite.transaction.begin",
			Self::TransactionExec => "rivet.sqlite.transaction.exec",
			Self::TransactionExecute => "rivet.sqlite.transaction.execute",
			Self::TransactionCommit => "rivet.sqlite.transaction.commit",
			Self::TransactionRollback => "rivet.sqlite.transaction.rollback",
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
		origin: ScheduleTraceOrigin,
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
		let ray_id = ray_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
		let identity = ctx.telemetry_identity();
		// Tracing backends group and chart by span name, so the name carries the
		// actor and action, and `rivet.invocation.type` still identifies
		// invocation spans for filtering. That also makes the name a cardinality
		// surface, so an undeclared action name is folded to a bounded
		// placeholder here and the same name is used for the span and the metric.
		let action_name = ctx.metrics().label_action_name(action_name).to_owned();
		let span =
			tracing::enabled!(target: "rivetkit::telemetry", tracing::Level::INFO).then(|| {
				let span = tracing::info_span!(
					target: "rivetkit::telemetry",
					parent: None,
					"rivet.actor.invoke",
					otel.name = %format!("{}/{}", identity.actor_name, action_name),
					otel.kind = invocation_type.otel_kind(),
					rivet.invocation.type = invocation_type.as_label(),
					rivet.actor.id = %identity.actor_id,
					rivet.actor.name = %identity.actor_name,
					rivet.actor.key = %identity.actor_key,
					rivet.action.name = %action_name,
					rivet.ray.id = tracing::field::Empty,
					otel.status_code = tracing::field::Empty,
					error.type = tracing::field::Empty,
				);
				span.record("rivet.ray.id", &ray_id);
				if let Some(parent) = parent {
					span.set_parent(opentelemetry::Context::new().with_remote_span_context(parent));
				}
				if let Some(link) = link {
					span.add_link(link);
				}
				span
			});

		Self {
			telemetry: ActorInvocationTelemetry::new(ray_id, span, identity),
			metrics: ctx.metrics().clone(),
			action_name,
			invocation_type,
			started_at: Instant::now(),
		}
	}

	pub(crate) fn telemetry(&self) -> ActorInvocationTelemetry {
		self.telemetry.clone()
	}

	pub(crate) fn finish(mut self, error: Option<&anyhow::Error>) {
		self.finish_with_status(
			error.map_or(InvocationStatus::Ok, InvocationStatus::from_error),
			error,
		);
	}

	fn finish_with_status(&mut self, status: InvocationStatus, error: Option<&anyhow::Error>) {
		let Some(span) = self.telemetry.take_active() else {
			return;
		};
		self.record_finished(span, status, error);
	}

	/// Records the terminal metric and span status of an invocation whose
	/// completion the caller has already claimed through `take_active`.
	fn record_finished(
		&self,
		span: Option<tracing::Span>,
		status: InvocationStatus,
		error: Option<&anyhow::Error>,
	) {
		self.metrics.record_invocation(
			&self.action_name,
			self.invocation_type,
			status,
			self.started_at.elapsed(),
		);
		if let Some(span) = span {
			record_outcome(&span, error);
		}
	}
}

impl Drop for ActorInvocation {
	fn drop(&mut self) {
		// `finish` consumes the invocation, so this runs on the completed path
		// too. Claim the terminal record first, so the dropped-reply error is
		// only built for an invocation that really was dropped.
		let Some(span) = self.telemetry.take_active() else {
			return;
		};
		let error = crate::error::ActorLifecycle::DroppedReply.build();
		self.record_finished(span, InvocationStatus::Dropped, Some(&error));
	}
}

impl ActorInvocationTelemetry {
	fn new(
		ray_id: String,
		span: Option<tracing::Span>,
		identity: Arc<ActorTelemetryIdentity>,
	) -> Self {
		Self(Arc::new(InvocationInner {
			ray_id,
			span: Mutex::new(span),
			finished: AtomicBool::new(false),
			identity,
		}))
	}

	/// Returns correlation fields only while this actor invocation is active.
	#[doc(hidden)]
	pub fn trace_context(&self) -> Option<ActorInvocationTraceContext> {
		let active = self.active()?;
		let span = active.span.lock().clone().and_then(|span| {
			let context = span.context();
			let context_span = context.span();
			let span_context = context_span.span_context();
			if !span_context.is_valid() {
				return None;
			}
			let tracestate = span_context.trace_state().header();
			Some(ActorInvocationSpanContext {
				trace_id: span_context.trace_id().to_string(),
				span_id: span_context.span_id().to_string(),
				trace_flags: span_context.trace_flags().to_u8(),
				traceparent: format!(
					"00-{}-{}-{:02x}",
					span_context.trace_id(),
					span_context.span_id(),
					span_context.trace_flags().to_u8(),
				),
				tracestate: (!tracestate.is_empty()).then_some(tracestate),
			})
		});

		Some(ActorInvocationTraceContext {
			ray_id: active.ray_id.clone(),
			span,
		})
	}

	pub(crate) fn schedule_trace_origin(&self) -> ScheduleTraceOrigin {
		self.trace_context()
			.map_or_else(ScheduleTraceOrigin::default, |context| {
				let (traceparent, tracestate) = match context.span {
					Some(span) => (Some(span.traceparent), span.tracestate),
					None => (None, None),
				};
				ScheduleTraceOrigin {
					ray_id: Some(context.ray_id),
					traceparent,
					tracestate,
				}
			})
	}

	pub(crate) fn start_sqlite(&self, operation: SqliteOperation) -> Option<SqliteOperationSpan> {
		let parent = self.active()?.span.lock().clone()?;
		let span = tracing::info_span!(
			target: "rivetkit::telemetry",
			parent: &parent,
			"rivet.sqlite.operation",
			otel.name = operation.span_name(),
			otel.kind = "internal",
			rivet.operation.system = "sqlite",
			rivet.operation.name = operation.as_str(),
			rivet.ray.id = %self.0.ray_id,
			rivet.actor.id = %self.0.identity.actor_id,
			rivet.actor.name = %self.0.identity.actor_name,
			rivet.actor.key = %self.0.identity.actor_key,
			otel.status_code = tracing::field::Empty,
			error.type = tracing::field::Empty,
		);
		Some(SqliteOperationSpan { span: Some(span) })
	}

	/// Borrows the invocation while it is still open. A finished invocation
	/// yields nothing, so late SQLite work and retained handles cannot attach
	/// to a span that has already recorded its status.
	fn active(&self) -> Option<&InvocationInner> {
		(!self.0.finished.load(Ordering::Acquire)).then_some(&*self.0)
	}

	/// Claims the terminal record, so the finish and drop paths cannot both
	/// record a status for the same invocation.
	fn take_active(&self) -> Option<Option<tracing::Span>> {
		if self.0.finished.swap(true, Ordering::AcqRel) {
			return None;
		}
		Some(self.0.span.lock().take())
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
		span.record("error.type", "future.cancelled");
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
	let mut fields = traceparent?.split('-');
	let version = fields.next()?;
	let trace_id = fields.next()?;
	let span_id = fields.next()?;
	let flags = fields.next()?;
	if fields.next().is_some()
		|| version.len() != 2
		|| version.eq_ignore_ascii_case("ff")
		|| trace_id.len() != 32
		|| span_id.len() != 16
		|| flags.len() != 2
	{
		return None;
	}

	let trace_id = TraceId::from_hex(trace_id).ok()?;
	let span_id = SpanId::from_hex(span_id).ok()?;
	let flags = u8::from_str_radix(flags, 16).ok()?;
	let trace_state = tracestate
		.and_then(|value| TraceState::from_str(value).ok())
		.unwrap_or_default();
	let context = SpanContext::new(trace_id, span_id, TraceFlags::new(flags), true, trace_state);
	context.is_valid().then_some(context)
}
