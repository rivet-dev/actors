//! Native OTLP export of the runtime's spans.
//!
//! Configuration comes entirely from the standard OpenTelemetry environment
//! variables, so every host that embeds core gets the same behavior by adding
//! [`layer`] to its subscriber and calling [`shutdown_best_effort`] after the
//! runtime has drained.
//! Core never installs a subscriber itself; which log layers surround the span
//! layer is the host's decision.

use std::time::Duration;

use anyhow::{Context, Result};
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig as _};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{Config, SdkTracer, SdkTracerProvider};
use parking_lot::Mutex;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{EnvFilter, Layer};

use super::sampler::ActorSampler;

enum ProviderState {
	Uninitialized,
	Active(SdkTracerProvider),
	ShutDown,
}

static PROVIDER: Mutex<ProviderState> = Mutex::new(ProviderState::Uninitialized);

/// Longest a collector that stops answering may hold up process shutdown.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Builds the span layer when standard OTel environment variables opt in, or
/// nothing when they do not. The layer only sees the runtime's own spans, so a
/// host's log filters do not decide what gets exported.
pub fn layer<S>() -> Result<Option<impl Layer<S>>>
where
	S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
	let Some(tracer) = initialize_if_configured()? else {
		return Ok(None);
	};
	Ok(Some(
		tracing_opentelemetry::layer()
			.with_tracer(tracer)
			.with_location(false)
			.with_threads(false)
			.with_tracked_inactivity(false)
			.with_filter(EnvFilter::new("rivetkit::telemetry=info")),
	))
}

/// Builds the OTLP exporter once. A second call reuses the provider so that a
/// host initializing tracing more than once does not open a second pipeline.
fn initialize_if_configured() -> Result<Option<SdkTracer>> {
	if !export_is_configured() {
		return Ok(None);
	}
	let mut stored_provider = PROVIDER.lock();
	match &*stored_provider {
		ProviderState::Active(provider) => return Ok(Some(provider.tracer("rivetkit"))),
		ProviderState::ShutDown => return Ok(None),
		ProviderState::Uninitialized => {}
	}

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
		.with_attribute(KeyValue::new("rivetkit.version", env!("CARGO_PKG_VERSION")))
		.build();
	// The builder only reads `OTEL_TRACES_SAMPLER` when no sampler is given, so
	// the process sampler is read here and wrapped.
	let provider = SdkTracerProvider::builder()
		.with_sampler(ActorSampler::new(Config::default().sampler))
		.with_resource(resource)
		.with_batch_exporter(exporter)
		.build();
	let tracer = provider.tracer("rivetkit");
	*stored_provider = ProviderState::Active(provider);
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

/// Terminates the process-global trace provider after runtime work has drained.
/// Export failures are logged because telemetry must not affect runtime shutdown.
pub async fn shutdown_best_effort() {
	let provider = {
		let mut state = PROVIDER.lock();
		match std::mem::replace(&mut *state, ProviderState::ShutDown) {
			ProviderState::Active(provider) => Some(provider),
			ProviderState::Uninitialized | ProviderState::ShutDown => None,
		}
	};
	let Some(provider) = provider else {
		return;
	};
	let shutdown = tokio::task::spawn_blocking(move || provider.shutdown());
	match tokio::time::timeout(SHUTDOWN_TIMEOUT, shutdown).await {
		Ok(Ok(Ok(()))) => {}
		Ok(Ok(Err(error))) => tracing::warn!(
			?error,
			"OpenTelemetry trace provider shutdown failed; queued spans may be lost"
		),
		Ok(Err(error)) => tracing::warn!(
			?error,
			"OpenTelemetry trace provider shutdown task failed; queued spans may be lost"
		),
		Err(_) => tracing::warn!(
			"OpenTelemetry trace provider shutdown timed out; queued spans may be lost"
		),
	}
}
