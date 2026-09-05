use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use napi::bindgen_prelude::{Buffer, Promise};
use napi::threadsafe_function::{ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction};
use napi::{Env, JsFunction, JsObject};
use napi_derive::napi;
use rivet_error::{ActorSpecifier, RivetError, RivetErrorKind};
use rivetkit_core::inspector::InspectorTabEntry;
use rivetkit_core::{
	ActionDefinition, ActorConfig, ActorConfigInput, ActorContext as CoreActorContext,
	ActorFactory as CoreActorFactory, ConnHandle as CoreConnHandle, Request,
	SqliteProfilingConfigInput, WebSocket as CoreWebSocket,
};

use crate::actor_context::{ActorContext, StateDeltaPayload};
use crate::cancellation_token::CancellationToken;
use crate::connection::ConnHandle;
use crate::http::{HttpRequestPayload, build_http_request_payload, build_request_object};
use crate::napi_actor_events::run_adapter_loop;
use crate::websocket::WebSocket;
use crate::{BRIDGE_RIVET_ERROR_PREFIX, NapiInvalidArgument, napi_anyhow_error};

pub(crate) type CallbackTsfn<T> = ThreadsafeFunction<T, ErrorStrategy::CalleeHandled>;

pub(crate) trait TsfnPayloadSummary {
	fn payload_summary(&self) -> String;
}

#[derive(RivetError, serde::Serialize, serde::Deserialize)]
#[error(
	"actor",
	"js_callback_unavailable",
	"JavaScript callback unavailable",
	"JavaScript callback `{callback}` could not be invoked: {reason}"
)]
struct JsCallbackUnavailable {
	callback: String,
	reason: String,
}

#[napi(object)]
pub struct JsQueueSendResult {
	pub status: String,
	pub response: Option<Buffer>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsActionDefinition {
	pub name: String,
}

/// One entry in the actor's `inspector.tabs[]` declaration. Either a
/// custom-tab descriptor (id + label + source dir) or a built-in modifier
/// (id + hidden=true). Validation already happened on the TS side; the
/// runtime just splits the discriminator.
#[napi(object)]
#[derive(Clone)]
pub struct JsInspectorTabEntry {
	pub id: String,
	/// Required for custom entries; omitted for HideBuiltin.
	pub label: Option<String>,
	/// Required for custom entries — absolute path to the source directory.
	pub source: Option<String>,
	/// Optional icon id for custom entries. Dashboard maps strings to
	/// glyphs; unknown ids fall back to a generic icon.
	pub icon: Option<String>,
	/// Set to true for HideBuiltin entries.
	pub hidden: Option<bool>,
}

/// Experimental SQLite profiling configuration. This entire configuration
/// surface is subject to change without notice.
#[napi(object)]
#[derive(Clone, Default)]
pub struct JsSqliteProfilingConfig {
	pub enabled: Option<bool>,
	pub max_tracked_statement_fingerprints: Option<u32>,
	pub max_tracked_transaction_fingerprints: Option<u32>,
	pub max_prometheus_series: Option<u32>,
	pub max_statements_per_transaction_trace: Option<u32>,
	pub max_get_pages_requests_per_trace: Option<u32>,
	pub max_transaction_name_bytes: Option<u32>,
	pub slow_operation_threshold_ms: Option<u32>,
	pub baseline_sample_rate: Option<f64>,
	pub max_diagnostic_events_per_minute: Option<u32>,
	pub diagnostic_event_queue_capacity: Option<u32>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct JsActorConfig {
	pub name: Option<String>,
	pub icon: Option<String>,
	pub has_database: Option<bool>,
	pub remote_sqlite: Option<bool>,
	pub sqlite_profiling: Option<JsSqliteProfilingConfig>,
	pub enable_actor_runtime_socket: Option<bool>,
	pub has_state: Option<bool>,
	pub can_hibernate_websocket: Option<bool>,
	pub state_save_interval_ms: Option<u32>,
	pub create_state_timeout_ms: Option<u32>,
	pub on_create_timeout_ms: Option<u32>,
	pub create_vars_timeout_ms: Option<u32>,
	pub create_conn_state_timeout_ms: Option<u32>,
	pub on_before_connect_timeout_ms: Option<u32>,
	pub on_connect_timeout_ms: Option<u32>,
	pub on_migrate_timeout_ms: Option<u32>,
	pub on_wake_timeout_ms: Option<u32>,
	pub on_before_actor_start_timeout_ms: Option<u32>,
	pub action_timeout_ms: Option<u32>,
	pub on_request_timeout_ms: Option<u32>,
	pub sleep_timeout_ms: Option<u32>,
	pub no_sleep: Option<bool>,
	pub sleep_grace_period_ms: Option<u32>,
	pub connection_liveness_timeout_ms: Option<u32>,
	pub connection_liveness_interval_ms: Option<u32>,
	pub max_queue_size: Option<u32>,
	pub max_schedules: Option<u32>,
	pub max_queue_message_size: Option<u32>,
	pub max_incoming_message_size: Option<u32>,
	pub max_outgoing_message_size: Option<u32>,
	pub actions: Option<Vec<JsActionDefinition>>,
	pub inspector_tabs: Option<Vec<JsInspectorTabEntry>>,
}

#[derive(Clone)]
pub(crate) struct LifecyclePayload {
	pub(crate) ctx: CoreActorContext,
}

#[derive(Clone)]
pub(crate) struct CreateStatePayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) input: Option<Vec<u8>>,
}

#[derive(Clone)]
pub(crate) struct CreateConnStatePayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) conn: CoreConnHandle,
	pub(crate) params: Vec<u8>,
	pub(crate) request: Option<Request>,
}

#[derive(Clone)]
pub(crate) struct MigratePayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) is_new: bool,
}

#[derive(Clone)]
pub(crate) struct QueueSendPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) conn: CoreConnHandle,
	pub(crate) request: Request,
	pub(crate) name: String,
	pub(crate) body: Vec<u8>,
	pub(crate) wait: bool,
	pub(crate) timeout_ms: Option<u64>,
	pub(crate) cancel_token: Option<tokio_util::sync::CancellationToken>,
}

#[derive(Clone)]
pub(crate) struct WebSocketPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) conn: CoreConnHandle,
	pub(crate) ws: CoreWebSocket,
	pub(crate) request: Option<Request>,
}

#[derive(Clone)]
pub(crate) struct BeforeSubscribePayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) conn: CoreConnHandle,
	pub(crate) event_name: String,
}

#[derive(Clone)]
pub(crate) struct BeforeConnectPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) params: Vec<u8>,
	pub(crate) request: Option<Request>,
}

#[derive(Clone)]
pub(crate) struct ConnectionPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) conn: CoreConnHandle,
	pub(crate) request: Option<Request>,
}

#[derive(Clone)]
pub(crate) struct ActionPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) telemetry: Option<rivetkit_core::ActorInvocationTelemetry>,
	pub(crate) conn: Option<CoreConnHandle>,
	pub(crate) name: String,
	pub(crate) args: Vec<u8>,
	pub(crate) scheduled_fire: Option<rivetkit_core::actor::schedule::ScheduledFireInfo>,
	pub(crate) cancel_token: Option<tokio_util::sync::CancellationToken>,
}

#[derive(Clone)]
pub(crate) struct BeforeActionResponsePayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) name: String,
	pub(crate) args: Vec<u8>,
	pub(crate) output: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct WorkflowHistoryPayload {
	pub(crate) ctx: CoreActorContext,
}

#[derive(Clone)]
pub(crate) struct WorkflowReplayPayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) entry_id: Option<String>,
}

#[derive(Clone)]
pub(crate) struct SerializeStatePayload {
	pub(crate) ctx: CoreActorContext,
	pub(crate) reason: String,
}

#[derive(Clone, Debug)]
pub(crate) struct AdapterConfig {
	pub(crate) create_state_timeout: Duration,
	pub(crate) on_create_timeout: Duration,
	pub(crate) create_vars_timeout: Duration,
	pub(crate) on_migrate_timeout: Duration,
	pub(crate) on_wake_timeout: Duration,
	pub(crate) on_before_actor_start_timeout: Duration,
	pub(crate) create_conn_state_timeout: Duration,
	pub(crate) on_before_connect_timeout: Duration,
	pub(crate) on_connect_timeout: Duration,
	pub(crate) action_timeout: Duration,
	pub(crate) on_request_timeout: Duration,
}

#[allow(dead_code)]
pub(crate) struct CallbackBindings {
	pub(crate) create_state: Option<CallbackTsfn<CreateStatePayload>>,
	pub(crate) on_create: Option<CallbackTsfn<CreateStatePayload>>,
	pub(crate) create_conn_state: Option<CallbackTsfn<CreateConnStatePayload>>,
	pub(crate) create_vars: Option<CallbackTsfn<LifecyclePayload>>,
	pub(crate) on_migrate: Option<CallbackTsfn<MigratePayload>>,
	pub(crate) on_wake: Option<CallbackTsfn<LifecyclePayload>>,
	pub(crate) on_before_actor_start: Option<CallbackTsfn<LifecyclePayload>>,
	pub(crate) on_sleep: Option<CallbackTsfn<LifecyclePayload>>,
	pub(crate) on_destroy: Option<CallbackTsfn<LifecyclePayload>>,
	pub(crate) on_before_connect: Option<CallbackTsfn<BeforeConnectPayload>>,
	pub(crate) on_connect: Option<CallbackTsfn<ConnectionPayload>>,
	pub(crate) on_disconnect_final: Option<CallbackTsfn<ConnectionPayload>>,
	pub(crate) on_before_subscribe: Option<CallbackTsfn<BeforeSubscribePayload>>,
	pub(crate) actions: HashMap<String, CallbackTsfn<ActionPayload>>,
	pub(crate) on_before_action_response: Option<CallbackTsfn<BeforeActionResponsePayload>>,
	pub(crate) on_request: Option<CallbackTsfn<HttpRequestPayload>>,
	pub(crate) on_queue_send: Option<CallbackTsfn<QueueSendPayload>>,
	pub(crate) on_websocket: Option<CallbackTsfn<WebSocketPayload>>,
	pub(crate) run: Option<CallbackTsfn<LifecyclePayload>>,
	pub(crate) get_workflow_history: Option<CallbackTsfn<WorkflowHistoryPayload>>,
	pub(crate) replay_workflow: Option<CallbackTsfn<WorkflowReplayPayload>>,
	pub(crate) serialize_state: Option<CallbackTsfn<SerializeStatePayload>>,
}

#[derive(serde::Deserialize)]
struct BridgeRivetErrorPayload {
	group: String,
	code: String,
	message: String,
	metadata: Option<serde_json::Value>,
	#[serde(rename = "rayId")]
	ray_id: Option<String>,
	#[serde(rename = "public")]
	public_: Option<bool>,
	#[serde(rename = "statusCode")]
	status_code: Option<u16>,
	actor: Option<ActorSpecifier>,
}

#[derive(Debug)]
pub(crate) struct BridgeRivetErrorContext {
	pub message: Option<String>,
	pub public_: Option<bool>,
	pub status_code: Option<u16>,
	pub ray_id: Option<String>,
}

impl std::fmt::Display for BridgeRivetErrorContext {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match &self.message {
			Some(message) => f.write_str(message),
			None => f.write_str("bridged RivetError"),
		}
	}
}

impl std::error::Error for BridgeRivetErrorContext {}

#[napi]
pub struct NapiActorFactory {
	#[allow(dead_code)]
	_bindings: Arc<CallbackBindings>,
	#[allow(dead_code)]
	inner: Arc<CoreActorFactory>,
}

impl NapiActorFactory {
	#[allow(dead_code)]
	pub(crate) fn actor_factory(&self) -> Arc<CoreActorFactory> {
		Arc::clone(&self.inner)
	}
}

#[napi]
impl NapiActorFactory {
	#[napi(constructor)]
	pub fn constructor(callbacks: JsObject, config: Option<JsActorConfig>) -> napi::Result<Self> {
		crate::init_tracing(None);
		let bindings = Arc::new(CallbackBindings::from_js(callbacks)?);
		let js_config = config.unwrap_or_default();
		tracing::debug!(
			class = "NapiActorFactory",
			actor_name = ?js_config.name,
			can_hibernate_websocket = ?js_config.can_hibernate_websocket,
			"constructed napi class"
		);
		let adapter_config = Arc::new(AdapterConfig::from_js_config(&js_config));
		let adapter_bindings = Arc::clone(&bindings);
		let loop_config = Arc::clone(&adapter_config);
		let actor_config = ActorConfig::from_input(ActorConfigInput::from(js_config));
		// Reject malformed config (empty ids/labels, duplicate ids, custom
		// tabs colliding with built-in ids, etc.) before the actor starts.
		actor_config.validate().map_err(napi_anyhow_error)?;
		let inner = Arc::new(
			CoreActorFactory::new_with_manual_startup_ready(actor_config, move |start| {
				let bindings = Arc::clone(&adapter_bindings);
				let config = Arc::clone(&loop_config);
				Box::pin(async move { run_adapter_loop(bindings, config, start).await })
			})
			.with_streaming_request_body(),
		);

		Ok(Self {
			_bindings: bindings,
			inner,
		})
	}
}

impl Drop for NapiActorFactory {
	fn drop(&mut self) {
		tracing::debug!(class = "NapiActorFactory", "dropped napi class");
	}
}

impl AdapterConfig {
	fn from_js_config(config: &JsActorConfig) -> Self {
		Self {
			create_state_timeout: duration_ms_or(config.create_state_timeout_ms, 5_000),
			on_create_timeout: duration_ms_or(config.on_create_timeout_ms, 5_000),
			create_vars_timeout: duration_ms_or(config.create_vars_timeout_ms, 5_000),
			on_migrate_timeout: duration_ms_or(config.on_migrate_timeout_ms, 30_000),
			on_wake_timeout: duration_ms_or(config.on_wake_timeout_ms, 30_000),
			on_before_actor_start_timeout: duration_ms_or(
				config.on_before_actor_start_timeout_ms,
				5_000,
			),
			create_conn_state_timeout: duration_ms_or(config.create_conn_state_timeout_ms, 5_000),
			on_before_connect_timeout: duration_ms_or(config.on_before_connect_timeout_ms, 5_000),
			on_connect_timeout: duration_ms_or(config.on_connect_timeout_ms, 5_000),
			action_timeout: duration_ms_or(config.action_timeout_ms, 60_000),
			on_request_timeout: duration_ms_or(
				config.on_request_timeout_ms.or(config.action_timeout_ms),
				60_000,
			),
		}
	}
}

impl CallbackBindings {
	fn from_js(callbacks: JsObject) -> napi::Result<Self> {
		let actions = if let Some(actions) = callbacks.get::<_, JsObject>("actions")? {
			let mut mapped = HashMap::new();
			for name in JsObject::keys(&actions)? {
				let callback = actions.get::<_, JsFunction>(&name)?.ok_or_else(|| {
					napi_anyhow_error(
						NapiInvalidArgument {
							argument: format!("actions.{name}"),
							reason: "must be a function".to_owned(),
						}
						.build(),
					)
				})?;
				mapped.insert(name, create_tsfn(callback, build_action_payload)?);
			}
			mapped
		} else {
			HashMap::new()
		};

		Ok(Self {
			create_state: optional_tsfn(&callbacks, "createState", build_create_state_payload)?,
			on_create: optional_tsfn(&callbacks, "onCreate", build_create_state_payload)?,
			create_conn_state: optional_tsfn(
				&callbacks,
				"createConnState",
				build_create_conn_state_payload,
			)?,
			create_vars: optional_tsfn(&callbacks, "createVars", build_lifecycle_payload)?,
			on_migrate: optional_tsfn(&callbacks, "onMigrate", build_migrate_payload)?,
			on_wake: optional_tsfn(&callbacks, "onWake", build_lifecycle_payload)?,
			on_before_actor_start: optional_tsfn(
				&callbacks,
				"onBeforeActorStart",
				build_lifecycle_payload,
			)?,
			on_sleep: optional_tsfn(&callbacks, "onSleep", build_lifecycle_payload)?,
			on_destroy: optional_tsfn(&callbacks, "onDestroy", build_lifecycle_payload)?,
			on_before_connect: optional_tsfn(
				&callbacks,
				"onBeforeConnect",
				build_before_connect_payload,
			)?,
			on_connect: optional_tsfn(&callbacks, "onConnect", build_connection_payload)?,
			on_disconnect_final: optional_tsfn(
				&callbacks,
				"onDisconnectFinal",
				build_connection_payload,
			)?
			.or(optional_tsfn(
				&callbacks,
				"onDisconnect",
				build_connection_payload,
			)?),
			on_before_subscribe: optional_tsfn(
				&callbacks,
				"onBeforeSubscribe",
				build_before_subscribe_payload,
			)?,
			actions,
			on_before_action_response: optional_tsfn(
				&callbacks,
				"onBeforeActionResponse",
				build_before_action_response_payload,
			)?,
			on_request: optional_tsfn(&callbacks, "onRequest", build_http_request_payload)?,
			on_queue_send: optional_tsfn(&callbacks, "onQueueSend", build_queue_send_payload)?,
			on_websocket: optional_tsfn(&callbacks, "onWebSocket", build_websocket_payload)?,
			run: optional_tsfn(&callbacks, "run", build_lifecycle_payload)?,
			get_workflow_history: optional_tsfn(
				&callbacks,
				"getWorkflowHistory",
				build_workflow_history_payload,
			)?,
			replay_workflow: optional_tsfn(
				&callbacks,
				"replayWorkflow",
				build_workflow_replay_payload,
			)?,
			serialize_state: optional_tsfn(
				&callbacks,
				"serializeState",
				build_serialize_state_payload,
			)?,
		})
	}
}

fn optional_tsfn<T, F>(
	callbacks: &JsObject,
	name: &str,
	build_args: F,
) -> napi::Result<Option<CallbackTsfn<T>>>
where
	T: Send + 'static,
	F: Fn(&Env, T) -> napi::Result<Vec<napi::JsUnknown>> + Send + Sync + 'static,
{
	let Some(callback) = callbacks.get::<_, JsFunction>(name)? else {
		return Ok(None);
	};
	create_tsfn(callback, build_args).map(Some)
}

fn create_tsfn<T, F>(callback: JsFunction, build_args: F) -> napi::Result<CallbackTsfn<T>>
where
	T: Send + 'static,
	F: Fn(&Env, T) -> napi::Result<Vec<napi::JsUnknown>> + Send + Sync + 'static,
{
	let build_args = Arc::new(build_args);
	callback.create_threadsafe_function(0, move |ctx: ThreadSafeCallContext<T>| {
		build_args(&ctx.env, ctx.value)
	})
}

#[allow(dead_code)]
pub(crate) async fn call_void<T>(
	callback_name: &str,
	callback: &CallbackTsfn<T>,
	payload: T,
) -> Result<()>
where
	T: Send + TsfnPayloadSummary + 'static,
{
	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<()>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	promise
		.await
		.map_err(|error| callback_error(callback_name, error))
}

pub(crate) async fn call_bool<T>(
	callback_name: &str,
	callback: &CallbackTsfn<T>,
	payload: T,
) -> Result<bool>
where
	T: Send + TsfnPayloadSummary + 'static,
{
	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<bool>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	promise
		.await
		.map_err(|error| callback_error(callback_name, error))
}

#[allow(dead_code)]
pub(crate) async fn call_buffer<T>(
	callback_name: &str,
	callback: &CallbackTsfn<T>,
	payload: T,
) -> Result<Vec<u8>>
where
	T: Send + TsfnPayloadSummary + 'static,
{
	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<Buffer>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	let buffer = promise
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	Ok(buffer.to_vec())
}

#[allow(dead_code)]
pub(crate) async fn call_optional_buffer<T>(
	callback_name: &str,
	callback: &CallbackTsfn<T>,
	payload: T,
) -> Result<Option<Vec<u8>>>
where
	T: Send + TsfnPayloadSummary + 'static,
{
	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<Option<Buffer>>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	let buffer = promise
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	Ok(buffer.map(|buffer| buffer.to_vec()))
}

#[allow(dead_code)]
pub(crate) async fn call_queue_send(
	callback_name: &str,
	callback: &CallbackTsfn<QueueSendPayload>,
	payload: QueueSendPayload,
) -> Result<JsQueueSendResult> {
	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<JsQueueSendResult>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	promise
		.await
		.map_err(|error| callback_error(callback_name, error))
}

#[allow(dead_code)]
pub(crate) async fn call_state_delta_payload(
	callback_name: &str,
	callback: &CallbackTsfn<SerializeStatePayload>,
	payload: SerializeStatePayload,
) -> Result<StateDeltaPayload> {
	log_tsfn_invocation(callback_name, &payload);
	let promise = callback
		.call_async::<Promise<StateDeltaPayload>>(Ok(payload))
		.await
		.map_err(|error| callback_error(callback_name, error))?;
	promise
		.await
		.map_err(|error| callback_error(callback_name, error))
}

pub(crate) fn log_tsfn_invocation<T>(kind: &str, payload: &T)
where
	T: TsfnPayloadSummary,
{
	let payload_summary = payload.payload_summary();
	tracing::debug!(
		kind,
		payload_summary = %payload_summary,
		"invoking napi TSF callback"
	);
}

impl TsfnPayloadSummary for LifecyclePayload {
	fn payload_summary(&self) -> String {
		format!("actor_id={}", self.ctx.actor_id())
	}
}

impl TsfnPayloadSummary for CreateStatePayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} input_bytes={}",
			self.ctx.actor_id(),
			self.input.as_ref().map(Vec::len).unwrap_or(0)
		)
	}
}

impl TsfnPayloadSummary for CreateConnStatePayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} conn_id={} params_bytes={} has_request={}",
			self.ctx.actor_id(),
			self.conn.id(),
			self.params.len(),
			self.request.is_some()
		)
	}
}

impl TsfnPayloadSummary for MigratePayload {
	fn payload_summary(&self) -> String {
		format!("actor_id={} is_new={}", self.ctx.actor_id(), self.is_new)
	}
}

impl TsfnPayloadSummary for QueueSendPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} conn_id={} queue={} body_bytes={} wait={} timeout_ms={:?} has_cancel_token={}",
			self.ctx.actor_id(),
			self.conn.id(),
			self.name,
			self.body.len(),
			self.wait,
			self.timeout_ms,
			self.cancel_token.is_some()
		)
	}
}

impl TsfnPayloadSummary for WebSocketPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} has_request={}",
			self.ctx.actor_id(),
			self.request.is_some()
		)
	}
}

impl TsfnPayloadSummary for BeforeSubscribePayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} conn_id={} event_name={}",
			self.ctx.actor_id(),
			self.conn.id(),
			self.event_name
		)
	}
}

impl TsfnPayloadSummary for BeforeConnectPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} params_bytes={} has_request={}",
			self.ctx.actor_id(),
			self.params.len(),
			self.request.is_some()
		)
	}
}

impl TsfnPayloadSummary for ConnectionPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} conn_id={} has_request={}",
			self.ctx.actor_id(),
			self.conn.id(),
			self.request.is_some()
		)
	}
}

impl TsfnPayloadSummary for ActionPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} action={} args_bytes={} conn_id={} has_cancel_token={}",
			self.ctx.actor_id(),
			self.name,
			self.args.len(),
			self.conn.as_ref().map(|conn| conn.id()).unwrap_or("<none>"),
			self.cancel_token.is_some()
		)
	}
}

impl TsfnPayloadSummary for BeforeActionResponsePayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} action={} args_bytes={} output_bytes={}",
			self.ctx.actor_id(),
			self.name,
			self.args.len(),
			self.output.len()
		)
	}
}

impl TsfnPayloadSummary for WorkflowHistoryPayload {
	fn payload_summary(&self) -> String {
		format!("actor_id={}", self.ctx.actor_id())
	}
}

impl TsfnPayloadSummary for WorkflowReplayPayload {
	fn payload_summary(&self) -> String {
		format!(
			"actor_id={} entry_id={}",
			self.ctx.actor_id(),
			self.entry_id.as_deref().unwrap_or("<beginning>")
		)
	}
}

impl TsfnPayloadSummary for SerializeStatePayload {
	fn payload_summary(&self) -> String {
		format!("actor_id={} reason={}", self.ctx.actor_id(), self.reason)
	}
}

fn build_lifecycle_payload(
	env: &Env,
	payload: LifecyclePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	Ok(vec![object.into_unknown()])
}

fn build_create_state_payload(
	env: &Env,
	payload: CreateStatePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("input", payload.input.map(Buffer::from))?;
	Ok(vec![object.into_unknown()])
}

fn build_create_conn_state_payload(
	env: &Env,
	payload: CreateConnStatePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("conn", ConnHandle::new(payload.conn))?;
	object.set("params", Buffer::from(payload.params))?;
	if let Some(request) = payload.request {
		object.set("request", build_request_object(env, request)?)?;
	}
	Ok(vec![object.into_unknown()])
}

fn build_migrate_payload(env: &Env, payload: MigratePayload) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("isNew", payload.is_new)?;
	Ok(vec![object.into_unknown()])
}

fn build_queue_send_payload(
	env: &Env,
	payload: QueueSendPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("conn", ConnHandle::new(payload.conn))?;
	object.set("request", build_request_object(env, payload.request)?)?;
	object.set("name", payload.name)?;
	object.set("body", Buffer::from(payload.body))?;
	object.set("wait", payload.wait)?;
	object.set("timeoutMs", payload.timeout_ms)?;
	match payload.cancel_token {
		Some(cancel_token) => object.set("cancelToken", CancellationToken::new(cancel_token))?,
		None => object.set("cancelToken", env.get_undefined()?)?,
	}
	Ok(vec![object.into_unknown()])
}

fn build_websocket_payload(
	env: &Env,
	payload: WebSocketPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("conn", ConnHandle::new(payload.conn))?;
	object.set("ws", WebSocket::new(payload.ws))?;
	if let Some(request) = payload.request {
		object.set("request", build_request_object(env, request)?)?;
	}
	Ok(vec![object.into_unknown()])
}

fn build_before_subscribe_payload(
	env: &Env,
	payload: BeforeSubscribePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("conn", ConnHandle::new(payload.conn))?;
	object.set("eventName", payload.event_name)?;
	Ok(vec![object.into_unknown()])
}

fn build_before_connect_payload(
	env: &Env,
	payload: BeforeConnectPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("params", Buffer::from(payload.params))?;
	if let Some(request) = payload.request {
		object.set("request", build_request_object(env, request)?)?;
	}
	Ok(vec![object.into_unknown()])
}

fn build_connection_payload(
	env: &Env,
	payload: ConnectionPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("conn", ConnHandle::new(payload.conn))?;
	if let Some(request) = payload.request {
		object.set("request", build_request_object(env, request)?)?;
	}
	Ok(vec![object.into_unknown()])
}

fn build_action_payload(env: &Env, payload: ActionPayload) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set(
		"ctx",
		ActorContext::new(payload.ctx.with_invocation_telemetry(payload.telemetry)),
	)?;
	match payload.conn {
		Some(conn) => object.set("conn", ConnHandle::new(conn))?,
		None => object.set("conn", env.get_null()?)?,
	}
	object.set("args", Buffer::from(payload.args))?;
	if let Some(fire) = payload.scheduled_fire {
		let mut fire_object = env.create_object()?;
		fire_object.set("kind", fire.kind.as_str())?;
		fire_object.set("id", fire.id)?;
		match fire.name {
			Some(name) => fire_object.set("name", name)?,
			None => fire_object.set("name", env.get_undefined()?)?,
		}
		fire_object.set("scheduledAt", fire.scheduled_at)?;
		fire_object.set("firedAt", fire.fired_at)?;
		object.set("scheduledFire", fire_object)?;
	} else {
		object.set("scheduledFire", env.get_undefined()?)?;
	}
	match payload.cancel_token {
		Some(cancel_token) => object.set("cancelToken", CancellationToken::new(cancel_token))?,
		None => object.set("cancelToken", env.get_undefined()?)?,
	}
	Ok(vec![object.into_unknown()])
}

fn build_before_action_response_payload(
	env: &Env,
	payload: BeforeActionResponsePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("name", payload.name)?;
	object.set("args", Buffer::from(payload.args))?;
	object.set("output", Buffer::from(payload.output))?;
	Ok(vec![object.into_unknown()])
}

fn build_workflow_history_payload(
	env: &Env,
	payload: WorkflowHistoryPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	Ok(vec![object.into_unknown()])
}

fn build_workflow_replay_payload(
	env: &Env,
	payload: WorkflowReplayPayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("entryId", payload.entry_id)?;
	Ok(vec![object.into_unknown()])
}

fn build_serialize_state_payload(
	env: &Env,
	payload: SerializeStatePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("ctx", ActorContext::new(payload.ctx))?;
	object.set("reason", env.create_string_from_std(payload.reason)?)?;
	Ok(vec![object.into_unknown()])
}

fn parse_bridge_rivet_error(reason: &str) -> Option<anyhow::Error> {
	let prefix_index = reason.find(BRIDGE_RIVET_ERROR_PREFIX)?;
	let payload = &reason[prefix_index + BRIDGE_RIVET_ERROR_PREFIX.len()..];
	let payload: BridgeRivetErrorPayload = match serde_json::from_str(payload) {
		Ok(payload) => payload,
		Err(parse_err) => {
			tracing::warn!(%reason, ?parse_err, "malformed BridgeRivetErrorPayload");
			return None;
		}
	};
	let meta = payload
		.metadata
		.as_ref()
		.and_then(|metadata| serde_json::value::to_raw_value(metadata).ok());
	let message = payload.message;
	let error = anyhow::Error::new(rivet_error::RivetError {
		kind: RivetErrorKind::Dynamic {
			group: payload.group,
			code: payload.code,
			default_message: message.clone(),
		},
		meta,
		message: Some(message.clone()),
		actor: payload.actor,
	});
	Some(error.context(BridgeRivetErrorContext {
		message: Some(message),
		public_: payload.public_,
		status_code: payload.status_code,
		ray_id: payload.ray_id,
	}))
}

pub(crate) fn callback_error(callback_name: &str, error: napi::Error) -> anyhow::Error {
	let reason = error.reason;
	if let Some(error) = parse_bridge_rivet_error(&reason) {
		return error;
	}
	if error.status == napi::Status::Closing {
		return JsCallbackUnavailable {
			callback: callback_name.to_owned(),
			reason,
		}
		.build();
	}

	anyhow::anyhow!(reason)
}

impl From<JsActorConfig> for ActorConfigInput {
	fn from(value: JsActorConfig) -> Self {
		Self {
			name: value.name,
			icon: value.icon,
			has_database: value.has_database,
			remote_sqlite: value.remote_sqlite,
			sqlite_profiling: value.sqlite_profiling.map(Into::into),
			enable_actor_runtime_socket: value.enable_actor_runtime_socket,
			has_state: value.has_state,
			can_hibernate_websocket: value.can_hibernate_websocket,
			state_save_interval_ms: value.state_save_interval_ms,
			create_vars_timeout_ms: value.create_vars_timeout_ms,
			create_conn_state_timeout_ms: value.create_conn_state_timeout_ms,
			on_before_connect_timeout_ms: value.on_before_connect_timeout_ms,
			on_connect_timeout_ms: value.on_connect_timeout_ms,
			on_migrate_timeout_ms: value.on_migrate_timeout_ms,
			action_timeout_ms: value.action_timeout_ms,
			sleep_timeout_ms: value.sleep_timeout_ms,
			no_sleep: value.no_sleep,
			sleep_grace_period_ms: value.sleep_grace_period_ms,
			connection_liveness_timeout_ms: value.connection_liveness_timeout_ms,
			connection_liveness_interval_ms: value.connection_liveness_interval_ms,
			max_queue_size: value.max_queue_size,
			max_schedules: value.max_schedules,
			max_queue_message_size: value.max_queue_message_size,
			max_incoming_message_size: value.max_incoming_message_size,
			max_outgoing_message_size: value.max_outgoing_message_size,
			actions: value.actions.map(|actions| {
				actions
					.into_iter()
					.map(|action| ActionDefinition { name: action.name })
					.collect()
			}),
			inspector_tabs: value.inspector_tabs.map(|tabs| {
				tabs.into_iter()
					.map(|tab| {
						if tab.hidden == Some(true) {
							InspectorTabEntry::HideBuiltin { id: tab.id }
						} else {
							// `label` and `source` are required on the TS
							// side but typed `Option<String>` on the NAPI
							// boundary to share one struct with the
							// HideBuiltin variant. Surface the real cause
							// instead of silently defaulting to "" and
							// letting downstream validation report an
							// empty-label/empty-source error.
							let id = tab.id.clone();
							let label = tab.label.unwrap_or_else(|| {
								panic!(
									"inspector tab `{id}` is missing `label` at the NAPI boundary; this indicates a TS-to-NAPI encoding bug"
								)
							});
							let source = tab.source.unwrap_or_else(|| {
								panic!(
									"inspector tab `{id}` is missing `source` at the NAPI boundary; this indicates a TS-to-NAPI encoding bug"
								)
							});
							InspectorTabEntry::Custom {
								id: tab.id,
								label,
								icon: tab.icon,
								root: std::path::PathBuf::from(source),
							}
						}
					})
					.collect()
			}),
		}
	}
}

impl From<JsSqliteProfilingConfig> for SqliteProfilingConfigInput {
	fn from(value: JsSqliteProfilingConfig) -> Self {
		Self {
			enabled: value.enabled,
			max_tracked_statement_fingerprints: value.max_tracked_statement_fingerprints,
			max_tracked_transaction_fingerprints: value.max_tracked_transaction_fingerprints,
			max_prometheus_series: value.max_prometheus_series,
			max_statements_per_transaction_trace: value.max_statements_per_transaction_trace,
			max_get_pages_requests_per_trace: value.max_get_pages_requests_per_trace,
			max_transaction_name_bytes: value.max_transaction_name_bytes,
			slow_operation_threshold_ms: value.slow_operation_threshold_ms,
			baseline_sample_rate: value.baseline_sample_rate,
			max_diagnostic_events_per_minute: value.max_diagnostic_events_per_minute,
			diagnostic_event_queue_capacity: value.diagnostic_event_queue_capacity,
		}
	}
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../tests/actor_factory.rs"]
mod tests;

fn duration_ms_or(value: Option<u32>, default_ms: u64) -> Duration {
	Duration::from_millis(value.map(u64::from).unwrap_or(default_ms))
}
