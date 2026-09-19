//! Forwards the OpenTelemetry SDK's own diagnostics to the JavaScript logger.
//!
//! The SDK reports dropped spans and export failures through Rust `tracing`.
//! Without this bridge they print in the Rust log format, apart from the
//! actor's Pino logs. With a sink installed they reach the JavaScript logger
//! instead, and the Rust log layers stop printing them so each warning shows
//! up once.

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
	ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use parking_lot::RwLock;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Tracing target of the SDK's internal logs.
const SDK_LOG_TARGET: &str = "opentelemetry_sdk";

/// One SDK diagnostic, flattened for the JavaScript side. `level` is the
/// SDK's own severity so the JavaScript logger keeps export failures at error.
pub(crate) struct SdkLogEvent {
	pub(crate) name: String,
	pub(crate) message: String,
	pub(crate) level: &'static str,
}

type Sink = ThreadsafeFunction<SdkLogEvent, ErrorStrategy::Fatal>;

/// The installed sink. The tracing subscriber initializes once per process,
/// so the sink is process-wide as well. It is replaced on install and cleared
/// on `uninstall` so the JavaScript callback is released at telemetry
/// shutdown rather than held for the life of the process.
static SINK: RwLock<Option<Sink>> = RwLock::new(None);

/// Installs the JavaScript sink, replacing any earlier one.
///
/// The threadsafe function is unreferenced. A referenced one counts as live
/// work on the Node event loop, so a process that had installed the sink
/// would never exit on its own.
pub(crate) fn install(env: Env, callback: JsFunction) -> Result<()> {
	let mut sink: Sink =
		callback.create_threadsafe_function(0, |ctx: ThreadSafeCallContext<SdkLogEvent>| {
			let mut object = ctx.env.create_object()?;
			object.set("name", ctx.value.name)?;
			object.set("message", ctx.value.message)?;
			object.set("level", ctx.value.level)?;
			Ok(vec![object.into_unknown()])
		})?;
	sink.unref(&env)?;
	*SINK.write() = Some(sink);
	Ok(())
}

/// Releases the JavaScript sink. SDK warnings print through the Rust log
/// layers again afterwards.
pub(crate) fn uninstall() {
	*SINK.write() = None;
}

/// Whether the Rust log layers should skip this event because the sink
/// delivers it to JavaScript instead.
pub(crate) fn delivered_to_sink(metadata: &tracing::Metadata<'_>) -> bool {
	if !is_sdk_warning(metadata) {
		return false;
	}
	SINK.read().as_ref().is_some_and(|sink| !sink.aborted())
}

fn is_sdk_warning(metadata: &tracing::Metadata<'_>) -> bool {
	metadata.target().starts_with(SDK_LOG_TARGET) && *metadata.level() <= tracing::Level::WARN
}

#[derive(Default)]
struct FieldCollector {
	name: String,
	message: String,
}

impl Visit for FieldCollector {
	fn record_str(&mut self, field: &Field, value: &str) {
		match field.name() {
			"name" => self.name = value.to_owned(),
			"message" => self.message = value.to_owned(),
			_ => {}
		}
	}

	fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
		match field.name() {
			"name" => self.name = format!("{value:?}"),
			"message" => self.message = format!("{value:?}"),
			_ => {}
		}
	}
}

/// Tracing layer that hands SDK warnings to the installed sink. Filter it to
/// the SDK target when attaching it to the subscriber.
pub(crate) struct SdkLogLayer;

impl<S: tracing::Subscriber> Layer<S> for SdkLogLayer {
	fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
		let sink = SINK.read();
		let Some(sink) = sink.as_ref().filter(|sink| !sink.aborted()) else {
			return;
		};
		let mut fields = FieldCollector::default();
		event.record(&mut fields);
		let level = if *event.metadata().level() == tracing::Level::ERROR {
			"error"
		} else {
			"warn"
		};
		sink.call(
			SdkLogEvent {
				name: fields.name,
				message: fields.message,
				level,
			},
			ThreadsafeFunctionCallMode::NonBlocking,
		);
	}
}
