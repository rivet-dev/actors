pub mod actor_context;
pub mod actor_factory;
pub mod cancellation_token;
pub mod connection;
pub mod database;
mod http;
pub mod kv;
pub mod napi_actor_events;
pub mod queue;
pub mod registry;
pub mod schedule;
pub mod types;
pub mod websocket;

use std::sync::Once;

use napi_derive::napi;
use rivet_error::RivetError as RivetTransportError;
use rivetkit_core::error::public_error_status_code;
use tracing_subscriber::{Layer as _, layer::SubscriberExt, util::SubscriberInitExt};

static INIT_TRACING: Once = Once::new();
pub(crate) const BRIDGE_RIVET_ERROR_PREFIX: &str = "__RIVET_ERROR_JSON__:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
	Logfmt,
	Gcp,
}

impl LogFormat {
	fn from_env() -> Self {
		match std::env::var("RUST_LOG_FORMAT")
			.unwrap_or_default()
			.to_lowercase()
			.as_str()
		{
			"gcp" => LogFormat::Gcp,
			_ => LogFormat::Logfmt,
		}
	}
}

#[derive(rivet_error::RivetError, serde::Serialize)]
#[error(
	"napi",
	"invalid_argument",
	"Invalid native argument",
	"Invalid native argument '{argument}': {reason}"
)]
pub(crate) struct NapiInvalidArgument {
	pub(crate) argument: String,
	pub(crate) reason: String,
}

#[derive(rivet_error::RivetError, serde::Serialize)]
#[error(
	"napi",
	"invalid_state",
	"Invalid native state",
	"Invalid native state '{state}': {reason}"
)]
pub(crate) struct NapiInvalidState {
	pub(crate) state: String,
	pub(crate) reason: String,
}

pub(crate) fn napi_anyhow_error(error: anyhow::Error) -> napi::Error {
	let payload = anyhow_to_bridge_rivet_error_payload(error);
	napi::Error::from_reason(format!("{BRIDGE_RIVET_ERROR_PREFIX}{}", payload))
}

fn anyhow_to_bridge_rivet_error_payload(error: anyhow::Error) -> serde_json::Value {
	let bridge_context = error
		.chain()
		.find_map(|cause| cause.downcast_ref::<crate::actor_factory::BridgeRivetErrorContext>());
	let error = RivetTransportError::extract(&error);
	let promoted_status_code = public_error_status_code(error.group(), error.code());
	let should_promote = promoted_status_code.is_some_and(|_| match bridge_context {
		Some(context) => {
			context.public_ != Some(true)
				|| context.status_code.is_none()
				|| context.status_code == Some(500)
		}
		None => true,
	});
	let status_code = if should_promote {
		promoted_status_code
	} else {
		bridge_context.and_then(|context| context.status_code)
	};
	let public_ = if should_promote {
		Some(true)
	} else {
		bridge_context.and_then(|context| context.public_)
	};
	serde_json::json!({
		"group": error.group(),
		"code": error.code(),
		"message": error.message(),
		"metadata": error.metadata(),
		"rayId": bridge_context.and_then(|context| context.ray_id.as_deref()),
		"public": public_,
		"statusCode": status_code,
		"actor": error.actor(),
	})
}

pub(crate) fn init_tracing(log_level: Option<&str>) {
	INIT_TRACING.call_once(|| {
		// Priority: explicit config > RIVET_LOG_LEVEL > LOG_LEVEL > RUST_LOG > "warn"
		let filter = log_level
			.map(String::from)
			.or_else(|| std::env::var("RIVET_LOG_LEVEL").ok())
			.or_else(|| std::env::var("LOG_LEVEL").ok())
			.or_else(|| std::env::var("RUST_LOG").ok())
			.unwrap_or_else(|| "warn".to_string());

		let log_filter = format!("{filter},rivetkit::telemetry=off");
		let log_format = LogFormat::from_env();
		let (otel_layer, otel_error) = match rivetkit_core::telemetry::export::layer() {
			Ok(layer) => (layer, None),
			Err(error) => (None, Some(error)),
		};

		// One filter per layer and no Option layers. Either of those makes
		// tracing-subscriber create every span in the process even when no
		// layer consumes it.
		let log_layer = match log_format {
			LogFormat::Logfmt => tracing_logfmt::builder()
				.with_span_name(env_flag("RUST_LOG_SPAN_NAME"))
				.with_span_path(env_flag("RUST_LOG_SPAN_PATH"))
				.with_target(env_flag("RUST_LOG_TARGET") || env_flag("RIVET_LOG_TARGET"))
				.with_location(env_flag("RUST_LOG_LOCATION"))
				.with_module_path(env_flag("RUST_LOG_MODULE_PATH"))
				.with_ansi_color(env_flag("RUST_LOG_ANSI_COLOR"))
				.layer()
				.boxed(),
			LogFormat::Gcp => tracing_stackdriver::layer()
				.with_source_location(env_flag("RUST_LOG_LOCATION"))
				.boxed(),
		}
		.with_filter(tracing_subscriber::EnvFilter::new(&log_filter));

		let base = tracing_subscriber::registry().with(log_layer);
		match otel_layer {
			Some(otel_layer) => base.with(otel_layer).init(),
			None => base.init(),
		}

		if let Some(error) = otel_error {
			tracing::warn!(
				?error,
				"OpenTelemetry trace export could not be initialized"
			);
		}
	});
}

#[napi]
pub async fn shutdown_telemetry() {
	rivetkit_core::telemetry::export::shutdown_best_effort().await;
}

fn env_flag(name: &str) -> bool {
	std::env::var(name).map_or(false, |x| x == "1")
}
