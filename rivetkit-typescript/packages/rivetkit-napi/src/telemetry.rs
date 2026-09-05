//! Native OpenTelemetry exporter composition.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig as _};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};

static PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// Builds the OTLP exporter when standard OTel environment variables opt in.
/// Called once from tracing initialization, so no extra synchronization is needed.
pub(crate) fn initialize_if_configured() -> Result<Option<SdkTracer>> {
	if !export_is_configured() {
		return Ok(None);
	}
	if let Some(provider) = PROVIDER.get() {
		return Ok(Some(provider.tracer("rivetkit")));
	}

	// gRPC and HTTP are different builders, so the transport is chosen here
	// rather than by passing a protocol into one of them.
	let exporter = match configured_protocol()? {
		Protocol::Grpc => SpanExporter::builder()
			.with_tonic()
			.build()
			.context("build otlp span exporter")?,
		protocol @ (Protocol::HttpBinary | Protocol::HttpJson) => SpanExporter::builder()
			.with_http()
			.with_protocol(protocol)
			.build()
			.context("build otlp span exporter")?,
	};
	let resource = Resource::builder()
		.with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
		.build();
	let provider = SdkTracerProvider::builder()
		.with_resource(resource)
		.with_batch_exporter(exporter)
		.build();
	let tracer = provider.tracer("rivetkit");
	PROVIDER
		.set(provider)
		.ok()
		.context("tracer provider already initialized")?;
	Ok(Some(tracer))
}

/// Reads the standard OTLP protocol variables.
///
/// The exporter's own default comes from a compile-time constant chosen by the
/// enabled cargo features, and neither of its builders reads these variables,
/// so selecting the protocol has to happen here. Enabling `http-json` would
/// otherwise make JSON that compile-time default, which the OTLP specification
/// does not list among the usual defaults.
fn configured_protocol() -> Result<Protocol> {
	let configured = std::env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL")
		.or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
		.unwrap_or_else(|_| "http/protobuf".to_owned());
	match configured.as_str() {
		"grpc" => Ok(Protocol::Grpc),
		"http/protobuf" => Ok(Protocol::HttpBinary),
		"http/json" => Ok(Protocol::HttpJson),
		other => anyhow::bail!(
			"native trace export supports grpc, http/protobuf and http/json, got {other:?}"
		),
	}
}

fn export_is_configured() -> bool {
	if std::env::var("OTEL_SDK_DISABLED").is_ok_and(|value| value.eq_ignore_ascii_case("true"))
		|| std::env::var("OTEL_TRACES_EXPORTER")
			.is_ok_and(|value| value.eq_ignore_ascii_case("none"))
	{
		return false;
	}

	[
		"OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
		"OTEL_EXPORTER_OTLP_ENDPOINT",
	]
	.into_iter()
	.any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

pub(crate) async fn flush_best_effort(timeout: Duration) {
	let Some(provider) = PROVIDER.get().cloned() else {
		return;
	};
	let flush = tokio::task::spawn_blocking(move || provider.force_flush());
	match tokio::time::timeout(timeout, flush).await {
		Ok(Ok(Ok(()))) => {}
		Ok(Ok(Err(_))) => tracing::warn!("OpenTelemetry trace flush failed"),
		Ok(Err(_)) => tracing::warn!("OpenTelemetry trace flush task failed"),
		Err(_) => tracing::warn!("OpenTelemetry trace flush timed out"),
	}
}

/// Forwards the OpenTelemetry SDK's own diagnostics to the JavaScript logger.
///
/// The SDK reports dropped spans and export failures through Rust `tracing`,
/// which prints to stdout in a different format from the actor's Pino logs.
/// This layer hands those events to a JS callback instead, so an operator sees
/// them alongside everything else the actor logs.
pub(crate) mod sdk_log_bridge {
	use std::sync::OnceLock;

	use napi::bindgen_prelude::*;
	use napi::threadsafe_function::{ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction};
	use tracing::field::{Field, Visit};
	use tracing_subscriber::Layer;
	use tracing_subscriber::layer::Context;

	/// One SDK diagnostic, flattened for the JavaScript side.
	pub(crate) struct SdkLogEvent {
		pub(crate) level: &'static str,
		pub(crate) name: String,
		pub(crate) message: String,
	}

	static SINK: OnceLock<ThreadsafeFunction<SdkLogEvent, ErrorStrategy::Fatal>> = OnceLock::new();

	/// Installs the JavaScript sink. Only the first call takes effect, matching
	/// the one-shot initialization of the tracing subscriber itself.
	///
	/// The threadsafe function is unreferenced. A referenced one counts as live
	/// work on the Node event loop, so a process that had registered the sink
	/// would never exit on its own. Warnings still cross while the application
	/// is running; the sink just stops being a reason to keep running.
	pub(crate) fn install(env: Env, callback: JsFunction) -> Result<()> {
		let mut tsfn =
			callback.create_threadsafe_function(0, |ctx: ThreadSafeCallContext<SdkLogEvent>| {
				let mut object = ctx.env.create_object()?;
				object.set("level", ctx.value.level)?;
				object.set("name", ctx.value.name)?;
				object.set("message", ctx.value.message)?;
				Ok(vec![object.into_unknown()])
			})?;
		tsfn.unref(&env)?;
		let _ = SINK.set(tsfn);
		Ok(())
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
			let rendered = format!("{value:?}");
			match field.name() {
				"name" => self.name = rendered,
				"message" => self.message = rendered,
				_ => {}
			}
		}
	}

	pub(crate) struct SdkLogLayer;

	impl<S: tracing::Subscriber> Layer<S> for SdkLogLayer {
		fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
			let Some(sink) = SINK.get() else {
				return;
			};
			let mut fields = FieldCollector::default();
			event.record(&mut fields);
			sink.call(
				SdkLogEvent {
					level: event.metadata().level().as_str(),
					name: fields.name,
					message: fields.message,
				},
				napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
			);
		}
	}
}
