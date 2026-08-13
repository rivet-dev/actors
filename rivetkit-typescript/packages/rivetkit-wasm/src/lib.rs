#![cfg(target_arch = "wasm32")]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use rivet_error::{
	ActorSpecifier, MacroMarker, RivetError as RivetTransportError, RivetErrorKind,
	RivetErrorSchema,
};
use rivetkit_core::error::public_error_status_code;
use rivetkit_core::inspector::InspectorAuth;
use rivetkit_core::{
	ActorConfig, ActorConfigInput, ActorEvent, ActorFactory as CoreActorFactory, ActorStart,
	ActorWorkKind, BindParam, ColumnValue, CoreRegistry as NativeCoreRegistry,
	CoreServerlessRuntime, EngineSpawnMode, EnqueueAndWaitOpts, KeepAwakeRegion, ListOpts,
	QueueMessage, QueueNextBatchOpts, QueueSendResult, QueueSendStatus, QueueTryNextBatchOpts,
	QueueWaitOpts, Request, RequestSaveOpts, Response, RuntimeSpawner, SerializeStateReason,
	ServeConfig, ServerlessRequest, SqliteBatchStatement, StateDelta, WebSocket,
	WebSocketCallbackRegion, WorkflowKvWrite, WsMessage,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken as CoreCancellationToken;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, UnwrapThrowExt};
use wasm_bindgen_futures::{JsFuture, spawn_local};

const BRIDGE_RIVET_ERROR_PREFIX: &str = "__RIVET_ERROR_JSON__:";

#[derive(rivet_error::RivetError, serde::Serialize)]
#[error(
	"wasm",
	"invalid_state",
	"Invalid wasm state",
	"Invalid wasm state '{state}': {reason}"
)]
struct WasmInvalidState {
	state: String,
	reason: String,
}

#[derive(rivet_error::RivetError, serde::Serialize)]
#[error(
	"wasm",
	"invalid_config",
	"Invalid wasm configuration.",
	"Invalid wasm configuration field '{field}': {reason}"
)]
struct WasmInvalidConfig {
	field: String,
	reason: String,
}

#[derive(serde::Deserialize)]
struct BridgeRivetErrorPayload {
	group: String,
	code: String,
	message: String,
	metadata: Option<serde_json::Value>,
	#[serde(rename = "public")]
	public_: Option<bool>,
	#[serde(rename = "statusCode")]
	status_code: Option<u16>,
	actor: Option<ActorSpecifier>,
}

#[derive(Debug)]
struct BridgeRivetErrorContext {
	message: Option<String>,
	public_: Option<bool>,
	status_code: Option<u16>,
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

#[derive(Clone)]
struct WasmFunction(Function);

// wasm-bindgen JS handles are bound to the single JavaScript thread in our wasm
// runtime. Core callback slots are Send + Sync so native builds can move them
// across threads, but the wasm runtime always drives them on the local task set.
unsafe impl Send for WasmFunction {}
unsafe impl Sync for WasmFunction {}

impl WasmFunction {
	fn call1(&self, payload: &JsValue) -> Result<JsValue> {
		self.0
			.call1(&JsValue::UNDEFINED, payload)
			.map_err(js_value_to_anyhow)
	}
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
	console_error_panic_hook::set_once();
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmServeConfig {
	pub version: u32,
	pub endpoint: String,
	pub token: Option<String>,
	pub namespace: String,
	pub pool_name: String,
	pub engine_binary_path: Option<String>,
	pub handle_inspector_http_in_runtime: Option<bool>,
	pub inspector_test_token: Option<String>,
	pub serverless_base_path: Option<String>,
	pub serverless_package_version: String,
	pub serverless_client_endpoint: Option<String>,
	pub serverless_client_namespace: Option<String>,
	pub serverless_client_token: Option<String>,
	pub serverless_validate_endpoint: bool,
	pub serverless_max_start_payload_bytes: u32,
}

fn validate_wasm_serve_config(config: &WasmServeConfig) -> Result<()> {
	if config.engine_binary_path.is_some() {
		return Err(WasmInvalidConfig {
			field: "engine_binary_path".to_owned(),
			reason: "wasm runtimes cannot spawn an engine binary; omit this field and connect to an existing engine endpoint".to_owned(),
		}
		.build());
	}
	Ok(())
}

impl From<WasmServeConfig> for ServeConfig {
	fn from(config: WasmServeConfig) -> Self {
		Self {
			version: config.version,
			endpoint: config.endpoint,
			token: config.token,
			namespace: config.namespace,
			pool_name: config.pool_name,
			engine_binary_path: config.engine_binary_path.map(PathBuf::from),
			engine_host: None,
			engine_port: None,
			engine_spawn: EngineSpawnMode::Never,
			engine_auto_download: false,
			handle_inspector_http_in_runtime: config
				.handle_inspector_http_in_runtime
				.unwrap_or(false),
			serverless_base_path: config.serverless_base_path,
			serverless_package_version: config.serverless_package_version,
			serverless_client_endpoint: config.serverless_client_endpoint,
			serverless_client_namespace: config.serverless_client_namespace,
			serverless_client_token: config.serverless_client_token,
			serverless_validate_endpoint: config.serverless_validate_endpoint,
			serverless_max_start_payload_bytes: config.serverless_max_start_payload_bytes as usize,
			serverless_cache_envoy: false,
		}
	}
}

#[derive(Clone, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WasmActionDefinition {
	pub name: String,
}

#[derive(Clone, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WasmActorConfig {
	pub name: Option<String>,
	pub icon: Option<String>,
	pub has_database: Option<bool>,
	pub remote_sqlite: Option<bool>,
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
	pub actions: Option<Vec<WasmActionDefinition>>,
}

impl From<WasmActorConfig> for ActorConfigInput {
	fn from(config: WasmActorConfig) -> Self {
		Self {
			name: config.name,
			icon: config.icon,
			has_database: config.has_database,
			remote_sqlite: config.remote_sqlite,
			enable_actor_runtime_socket: config.enable_actor_runtime_socket,
			has_state: config.has_state,
			can_hibernate_websocket: config.can_hibernate_websocket,
			state_save_interval_ms: config.state_save_interval_ms,
			create_vars_timeout_ms: config.create_vars_timeout_ms,
			create_conn_state_timeout_ms: config.create_conn_state_timeout_ms,
			on_before_connect_timeout_ms: config.on_before_connect_timeout_ms,
			on_connect_timeout_ms: config.on_connect_timeout_ms,
			on_migrate_timeout_ms: config.on_migrate_timeout_ms,
			action_timeout_ms: config.action_timeout_ms,
			sleep_timeout_ms: config.sleep_timeout_ms,
			no_sleep: config.no_sleep,
			sleep_grace_period_ms: config.sleep_grace_period_ms,
			connection_liveness_timeout_ms: config.connection_liveness_timeout_ms,
			connection_liveness_interval_ms: config.connection_liveness_interval_ms,
			max_queue_size: config.max_queue_size,
			max_schedules: config.max_schedules,
			max_queue_message_size: config.max_queue_message_size,
			max_incoming_message_size: config.max_incoming_message_size,
			max_outgoing_message_size: config.max_outgoing_message_size,
			actions: config.actions.map(|actions| {
				actions
					.into_iter()
					.map(|action| rivetkit_core::ActionDefinition { name: action.name })
					.collect()
			}),
			// Custom inspector tabs serve assets from a filesystem `root`, which is a
			// native/server feature that has no meaning in a browser wasm host.
			inspector_tabs: None,
		}
	}
}

enum RegistryState {
	Registering(Option<NativeCoreRegistry>),
	BuildingServerless,
	Serving,
	Serverless(WasmServerlessRuntime),
	ShuttingDown,
	ShutDown,
}

#[derive(Clone)]
struct WasmServerlessRuntime {
	runtime: CoreServerlessRuntime,
}

#[wasm_bindgen(js_name = CoreRegistry)]
#[derive(Clone)]
pub struct WasmCoreRegistry {
	state: Rc<RefCell<RegistryState>>,
	shutdown_token: CoreCancellationToken,
	build_waiters: Rc<RefCell<Vec<oneshot::Sender<()>>>>,
}

#[wasm_bindgen(js_class = CoreRegistry)]
impl WasmCoreRegistry {
	#[wasm_bindgen(constructor)]
	pub fn new() -> Self {
		Self {
			state: Rc::new(RefCell::new(RegistryState::Registering(Some(
				NativeCoreRegistry::new(),
			)))),
			shutdown_token: CoreCancellationToken::new(),
			build_waiters: Rc::new(RefCell::new(Vec::new())),
		}
	}

	fn notify_serverless_build_complete(&self) {
		let waiters = std::mem::take(&mut *self.build_waiters.borrow_mut());
		for waiter in waiters {
			let _ = waiter.send(());
		}
	}

	#[wasm_bindgen]
	pub fn register(&self, name: String, factory: &WasmActorFactory) -> Result<(), JsValue> {
		let mut state = self.state.borrow_mut();
		match &mut *state {
			RegistryState::Registering(registry) => {
				let registry = registry
					.as_mut()
					.ok_or_else(|| js_error("registry is already serving"))?;
				registry.register_shared(&name, factory.inner.clone());
				Ok(())
			}
			RegistryState::BuildingServerless
			| RegistryState::Serving
			| RegistryState::Serverless(_)
			| RegistryState::ShuttingDown
			| RegistryState::ShutDown => Err(registry_not_registering_error()),
		}
	}

	#[wasm_bindgen]
	pub async fn serve(&self, config: JsValue) -> Result<(), JsValue> {
		let config: WasmServeConfig = serde_wasm_bindgen::from_value(config)?;
		validate_wasm_serve_config(&config).map_err(anyhow_to_js_error)?;
		rivetkit_core::inspector::set_test_inspector_token_override(
			config.inspector_test_token.clone(),
		);
		let registry = {
			let mut state = self.state.borrow_mut();
			match &mut *state {
				RegistryState::Registering(registry) => {
					let registry = registry.take().ok_or_else(registry_not_registering_error)?;
					*state = RegistryState::Serving;
					registry
				}
				RegistryState::BuildingServerless | RegistryState::Serverless(_) => {
					return Err(registry_wrong_mode_error());
				}
				RegistryState::Serving => return Err(registry_not_registering_error()),
				RegistryState::ShuttingDown | RegistryState::ShutDown => {
					return Err(registry_shut_down_error());
				}
			}
		};

		let local = tokio::task::LocalSet::new();
		local
			.run_until(registry.serve_with_config(config.into(), self.shutdown_token.clone()))
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn shutdown(&self) -> Result<(), JsValue> {
		self.shutdown_token.cancel();
		let serverless = {
			let mut state = self.state.borrow_mut();
			let previous = std::mem::replace(&mut *state, RegistryState::ShutDown);
			match previous {
				RegistryState::Serverless(serverless) => Some(serverless.runtime),
				RegistryState::BuildingServerless => {
					*state = RegistryState::ShuttingDown;
					None
				}
				RegistryState::Registering(_)
				| RegistryState::Serving
				| RegistryState::ShuttingDown
				| RegistryState::ShutDown => None,
			}
		};
		self.notify_serverless_build_complete();
		if let Some(serverless) = serverless {
			serverless.shutdown().await;
			*self.state.borrow_mut() = RegistryState::ShutDown;
		}
		Ok(())
	}

	#[wasm_bindgen(js_name = handleServerlessRequest)]
	pub async fn handle_serverless_request(
		&self,
		req: JsValue,
		on_stream_event: Function,
		cancel_token: &WasmCancellationToken,
		config: JsValue,
	) -> Result<JsValue, JsValue> {
		let serverless = self.serverless_runtime(config).await?;
		let req = serverless_request_from_js(req, cancel_token.inner.clone())
			.map_err(anyhow_to_js_error)?;
		start_wasm_serverless_request(serverless.runtime, req, on_stream_event).await
	}

	async fn serverless_runtime(&self, config: JsValue) -> Result<WasmServerlessRuntime, JsValue> {
		let config: WasmServeConfig = serde_wasm_bindgen::from_value(config)?;
		validate_wasm_serve_config(&config).map_err(anyhow_to_js_error)?;
		rivetkit_core::inspector::set_test_inspector_token_override(
			config.inspector_test_token.clone(),
		);
		loop {
			let maybe_registry = {
				let mut state = self.state.borrow_mut();
				match &mut *state {
					RegistryState::Registering(registry) => {
						let registry =
							registry.take().ok_or_else(registry_not_registering_error)?;
						*state = RegistryState::BuildingServerless;
						Some(registry)
					}
					RegistryState::Serverless(serverless) => return Ok(serverless.clone()),
					RegistryState::BuildingServerless => {
						let (tx, rx) = oneshot::channel();
						self.build_waiters.borrow_mut().push(tx);
						drop(state);
						let _ = rx.await;
						continue;
					}
					RegistryState::Serving => return Err(registry_wrong_mode_error()),
					RegistryState::ShuttingDown | RegistryState::ShutDown => {
						return Err(registry_shut_down_error());
					}
				}
			};

			let registry = maybe_registry.ok_or_else(registry_not_registering_error)?;
			let runtime = match registry.into_serverless_runtime(config.into()).await {
				Ok(runtime) => runtime,
				Err(error) => {
					*self.state.borrow_mut() = RegistryState::ShutDown;
					self.notify_serverless_build_complete();
					return Err(anyhow_to_js_error(error));
				}
			};
			let serverless = WasmServerlessRuntime { runtime };
			if self.shutdown_token.is_cancelled() {
				serverless.runtime.shutdown().await;
				*self.state.borrow_mut() = RegistryState::ShutDown;
				self.notify_serverless_build_complete();
				return Err(registry_shut_down_error());
			}
			{
				let mut state = self.state.borrow_mut();
				match &*state {
					RegistryState::BuildingServerless => {
						*state = RegistryState::Serverless(serverless.clone());
					}
					RegistryState::ShuttingDown | RegistryState::ShutDown => {
						drop(state);
						serverless.runtime.shutdown().await;
						*self.state.borrow_mut() = RegistryState::ShutDown;
						self.notify_serverless_build_complete();
						return Err(registry_shut_down_error());
					}
					RegistryState::Registering(_)
					| RegistryState::Serving
					| RegistryState::Serverless(_) => {
						drop(state);
						serverless.runtime.shutdown().await;
						self.notify_serverless_build_complete();
						return Err(registry_wrong_mode_error());
					}
				}
			}
			self.notify_serverless_build_complete();
			return Ok(serverless);
		}
	}
}

impl Default for WasmCoreRegistry {
	fn default() -> Self {
		Self::new()
	}
}

#[wasm_bindgen(js_name = ActorFactory)]
#[derive(Clone)]
pub struct WasmActorFactory {
	inner: Arc<CoreActorFactory>,
}

#[wasm_bindgen(js_class = ActorFactory)]
impl WasmActorFactory {
	#[wasm_bindgen(constructor)]
	pub fn new(callbacks: JsValue, config: JsValue) -> Result<WasmActorFactory, JsValue> {
		let input = if config.is_null() || config.is_undefined() {
			WasmActorConfig::default()
		} else {
			serde_wasm_bindgen::from_value(config)?
		};
		let config = ActorConfig::from_input(input.into());
		config
			.validate()
			.map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
		let callbacks = WasmCallbacks::new(callbacks);
		let factory = CoreActorFactory::new_with_manual_startup_ready(config, move |start| {
			let callbacks = callbacks.clone();
			Box::pin(async move { run_actor_adapter(callbacks, start).await })
		});
		Ok(WasmActorFactory {
			inner: Arc::new(factory),
		})
	}
}

#[derive(Clone)]
struct WasmCallbacks {
	create_state: Option<Function>,
	on_create: Option<Function>,
	create_vars: Option<Function>,
	on_migrate: Option<Function>,
	on_wake: Option<Function>,
	on_before_actor_start: Option<Function>,
	on_sleep: Option<Function>,
	on_destroy: Option<Function>,
	on_before_connect: Option<Function>,
	create_conn_state: Option<Function>,
	on_connect: Option<Function>,
	on_disconnect_final: Option<Function>,
	on_before_subscribe: Option<Function>,
	on_before_action_response: Option<Function>,
	on_request: Option<Function>,
	on_queue_send: Option<Function>,
	on_websocket: Option<Function>,
	serialize_state: Option<Function>,
	run: Option<Function>,
	get_workflow_history: Option<Function>,
	replay_workflow: Option<Function>,
	actions: JsValue,
}

impl WasmCallbacks {
	fn new(callbacks: JsValue) -> Self {
		Self {
			create_state: function_property(&callbacks, "createState"),
			on_create: function_property(&callbacks, "onCreate"),
			create_vars: function_property(&callbacks, "createVars"),
			on_migrate: function_property(&callbacks, "onMigrate"),
			on_wake: function_property(&callbacks, "onWake"),
			on_before_actor_start: function_property(&callbacks, "onBeforeActorStart"),
			on_sleep: function_property(&callbacks, "onSleep"),
			on_destroy: function_property(&callbacks, "onDestroy"),
			on_before_connect: function_property(&callbacks, "onBeforeConnect"),
			create_conn_state: function_property(&callbacks, "createConnState"),
			on_connect: function_property(&callbacks, "onConnect"),
			on_disconnect_final: function_property(&callbacks, "onDisconnectFinal")
				.or_else(|| function_property(&callbacks, "onDisconnect")),
			on_before_subscribe: function_property(&callbacks, "onBeforeSubscribe"),
			on_before_action_response: function_property(&callbacks, "onBeforeActionResponse"),
			on_request: function_property(&callbacks, "onRequest"),
			on_queue_send: function_property(&callbacks, "onQueueSend"),
			on_websocket: function_property(&callbacks, "onWebSocket"),
			serialize_state: function_property(&callbacks, "serializeState"),
			run: function_property(&callbacks, "run"),
			get_workflow_history: function_property(&callbacks, "getWorkflowHistory"),
			replay_workflow: function_property(&callbacks, "replayWorkflow"),
			actions: Reflect::get(&callbacks, &JsValue::from_str("actions"))
				.unwrap_or(JsValue::UNDEFINED),
		}
	}
}

async fn run_actor_adapter(callbacks: WasmCallbacks, start: ActorStart) -> Result<()> {
	let ActorStart {
		ctx: core_ctx,
		input,
		is_new,
		snapshot,
		hibernated: _,
		mut events,
		startup_ready,
	} = start;

	let ctx = WasmActorContext::from_core(core_ctx.clone(), callbacks.clone());
	let preamble = run_preamble(&callbacks, &ctx, input, is_new, snapshot).await;
	if let Some(reply) = startup_ready {
		let _ = reply.send(
			preamble
				.as_ref()
				.map(|_| ())
				.map_err(|error| anyhow!(RivetTransportError::extract(error))),
		);
	}
	preamble?;
	start_run_handler(&callbacks, &ctx);

	while let Some(event) = events.recv().await {
		dispatch_event(&callbacks, &ctx, event).await;
	}

	Ok(())
}

fn start_run_handler(callbacks: &WasmCallbacks, ctx: &WasmActorContext) {
	let Some(callback) = callbacks.run.clone() else {
		return;
	};
	let ctx = ctx.clone();
	ctx.inner.begin_run_handler();
	spawn_local(async move {
		let result = async {
			let payload = object();
			set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
			call_callback(&callback, &payload.into()).await?;
			Ok::<_, anyhow::Error>(())
		}
		.await;
		if let Err(error) = &result {
			console_error(&format!("wasm run callback failed: {error:#}"));
		}
		ctx.inner.end_run_handler();
	});
}

async fn run_preamble(
	callbacks: &WasmCallbacks,
	ctx: &WasmActorContext,
	input: Option<Vec<u8>>,
	is_new: bool,
	snapshot: Option<Vec<u8>>,
) -> Result<()> {
	if let Some(callback) = &callbacks.on_migrate {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		set_anyhow(&payload, "isNew", JsValue::from_bool(is_new))?;
		call_callback(callback, &payload.into()).await?;
	}

	if is_new {
		if let Some(callback) = &callbacks.create_state {
			let payload = object();
			set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
			if let Some(input) = input.as_ref() {
				set_anyhow(&payload, "input", bytes_to_js(input))?;
			}
			let state = call_callback_bytes(callback, &payload.into()).await?;
			ctx.inner.set_state_initial(state);
		}
		if let Some(callback) = &callbacks.on_create {
			let payload = object();
			set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
			if let Some(input) = input.as_ref() {
				set_anyhow(&payload, "input", bytes_to_js(input))?;
			}
			call_callback(callback, &payload.into()).await?;
		}
	} else if let Some(snapshot) = snapshot {
		ctx.inner.set_state_initial(snapshot);
	} else if let Some(callback) = &callbacks.create_state {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		if let Some(input) = input.as_ref() {
			set_anyhow(&payload, "input", bytes_to_js(input))?;
		}
		let state = call_callback_bytes(callback, &payload.into()).await?;
		ctx.inner.set_state_initial(state.clone());
		ctx.inner
			.save_state(vec![StateDelta::ActorState(state)])
			.await?;
	}

	if let Some(callback) = &callbacks.create_vars {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		call_callback(callback, &payload.into()).await?;
	}

	if let Some(callback) = &callbacks.on_wake {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		call_callback(callback, &payload.into()).await?;
	}

	if let Some(callback) = &callbacks.on_before_actor_start {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		call_callback(callback, &payload.into()).await?;
	}

	Ok(())
}

async fn dispatch_event(callbacks: &WasmCallbacks, ctx: &WasmActorContext, event: ActorEvent) {
	match event {
		ActorEvent::Action {
			name,
			args,
			conn,
			scheduled_fire,
			reply,
		} => {
			let Some(callback) = action_callback(&callbacks.actions, &name) else {
				console_error(&format!("wasm action callback `{name}` was not found"));
				reply.send(Err(rivetkit_core::error::action_not_found(name)));
				return;
			};

			let ctx = ctx.clone();
			let on_before_action_response = callbacks.on_before_action_response.clone();
			RuntimeSpawner::spawn(async move {
				let result = async {
					let payload = object();
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					set_anyhow(
						&payload,
						"conn",
						conn.clone()
							.map(WasmConnHandle::from_core)
							.map(JsValue::from)
							.unwrap_or(JsValue::NULL),
					)?;
					set_anyhow(&payload, "name", JsValue::from_str(&name))?;
					set_anyhow(&payload, "args", bytes_to_js(&args))?;
					if let Some(scheduled_fire) = &scheduled_fire {
						let scheduled_fire =
							serde_wasm_bindgen::to_value(scheduled_fire).map_err(|error| {
								anyhow!("serialize scheduled fire metadata: {error:?}")
							})?;
						set_anyhow(&payload, "scheduledFire", scheduled_fire)?;
					}
					let mut output = call_callback_bytes(&callback, &payload.into()).await?;

					if let Some(callback) = &on_before_action_response {
						let payload = object();
						set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
						set_anyhow(&payload, "name", JsValue::from_str(&name))?;
						set_anyhow(&payload, "args", bytes_to_js(&args))?;
						set_anyhow(&payload, "output", bytes_to_js(&output))?;
						output = call_callback_bytes(callback, &payload.into()).await?;
					}

					Ok(output)
				}
				.await;
				if let Err(error) = &result {
					console_error(&format!("wasm action callback `{name}` failed: {error:#}"));
				}
				reply.send(result);
			});
		}
		ActorEvent::SerializeState { reason, reply } => {
			let result = match callbacks.serialize_state.as_ref() {
				Some(callback) => serialize_state(callback, ctx, reason).await,
				None => Ok(Vec::new()),
			};
			reply.send(result);
		}
		ActorEvent::RunGracefulCleanup { reason, reply } => {
			let callback = match reason {
				rivetkit_core::ShutdownKind::Sleep => callbacks.on_sleep.as_ref(),
				rivetkit_core::ShutdownKind::Destroy => callbacks.on_destroy.as_ref(),
			};
			if let Some(callback) = callback {
				let payload = object();
				let result = async {
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					call_callback(callback, &payload.into()).await?;
					Ok(())
				}
				.await;
				reply.send(result);
			} else {
				reply.send(Ok(()));
			}
		}
		ActorEvent::WorkflowHistoryRequested { reply } => {
			let Some(callback) = callbacks.get_workflow_history.as_ref() else {
				// Dropped means unsupported; `Ok(None)` means supported without history.
				drop(reply);
				return;
			};
			let result = async {
				let payload = object();
				set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
				let value = call_callback(callback, &payload.into()).await?;
				if value.is_null() || value.is_undefined() {
					Ok(None)
				} else {
					Ok(Some(js_to_bytes(value)))
				}
			}
			.await;
			if let Err(error) = &result {
				console_error(&format!("wasm workflow history callback failed: {error:#}"));
			}
			reply.send(result);
		}
		ActorEvent::WorkflowReplayRequested { entry_id, reply } => {
			let Some(callback) = callbacks.replay_workflow.as_ref() else {
				drop(reply);
				return;
			};
			let result = async {
				let payload = object();
				set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
				if let Some(entry_id) = entry_id {
					set_anyhow(&payload, "entryId", JsValue::from_str(&entry_id))?;
				}
				let value = call_callback(callback, &payload.into()).await?;
				if value.is_null() || value.is_undefined() {
					Ok(None)
				} else {
					Ok(Some(js_to_bytes(value)))
				}
			}
			.await;
			if let Err(error) = &result {
				console_error(&format!("wasm workflow replay callback failed: {error:#}"));
			}
			reply.send(result);
		}
		ActorEvent::HttpRequest { request, reply } => {
			let callback = callbacks.on_request.clone();
			let ctx = ctx.clone();
			RuntimeSpawner::spawn(async move {
				let result = async {
					let callback = callback
						.as_ref()
						.ok_or_else(|| anyhow!("wasm onRequest callback is not implemented"))?;
					let payload = object();
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					set_anyhow(&payload, "request", request_to_js(request)?)?;
					let value = call_callback(callback, &payload.into()).await?;
					response_from_js(value)
				}
				.await;
				if let Err(error) = &result {
					console_error(&format!("wasm onRequest callback failed: {error:#}"));
				}
				reply.send(result);
			});
		}
		ActorEvent::QueueSend {
			name,
			body,
			conn,
			request,
			wait,
			timeout_ms,
			reply,
		} => {
			let callback = callbacks.on_queue_send.clone();
			let ctx = ctx.clone();
			RuntimeSpawner::spawn(async move {
				let result = async {
					let callback = callback
						.as_ref()
						.ok_or_else(|| anyhow!("wasm onQueueSend callback is not implemented"))?;
					let payload = object();
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					set_anyhow(
						&payload,
						"conn",
						JsValue::from(WasmConnHandle::from_core(conn)),
					)?;
					set_anyhow(&payload, "request", request_to_js(request)?)?;
					set_anyhow(&payload, "name", JsValue::from_str(&name))?;
					set_anyhow(&payload, "body", bytes_to_js(&body))?;
					set_anyhow(&payload, "wait", JsValue::from_bool(wait))?;
					set_anyhow(
						&payload,
						"timeoutMs",
						timeout_ms
							.map(|value| JsValue::from_f64(value as f64))
							.unwrap_or(JsValue::UNDEFINED),
					)?;
					let value = call_callback(callback, &payload.into()).await?;
					queue_send_result_from_js(value)
				}
				.await;
				if let Err(error) = &result {
					console_error(&format!("wasm onQueueSend callback failed: {error:#}"));
				}
				reply.send(result);
			});
		}
		ActorEvent::WebSocketOpen {
			conn,
			ws,
			request,
			reply,
		} => {
			let result = async {
				let callback = callbacks
					.on_websocket
					.as_ref()
					.ok_or_else(|| anyhow!("wasm onWebSocket callback is not implemented"))?;
				let payload = object();
				set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
				set_anyhow(
					&payload,
					"conn",
					JsValue::from(WasmConnHandle::from_core(conn)),
				)?;
				set_anyhow(&payload, "ws", JsValue::from(WasmWebSocket::from_core(ws)))?;
				if let Some(request) = request {
					set_anyhow(&payload, "request", request_to_js(request)?)?;
				}
				call_callback(callback, &payload.into()).await?;
				Ok(())
			}
			.await;
			if let Err(error) = &result {
				console_error(&format!("wasm websocket callback failed: {error:#}"));
			}
			reply.send(result);
		}
		ActorEvent::ConnectionPreflight {
			conn,
			params,
			request,
			reply,
		} => {
			let result = run_connection_preflight(callbacks, ctx, conn, params, request).await;
			if let Err(error) = &result {
				console_error(&format!(
					"wasm connection preflight callback failed: {error:#}"
				));
			}
			reply.send(result);
		}
		ActorEvent::ConnectionOpen {
			conn,
			request,
			reply,
		} => {
			let result = run_connection_open(callbacks, ctx, conn, request).await;
			if let Err(error) = &result {
				console_error(&format!("wasm connection open callback failed: {error:#}"));
			}
			reply.send(result);
		}
		ActorEvent::SubscribeRequest {
			conn,
			event_name,
			reply,
		} => {
			let callback = callbacks.on_before_subscribe.clone();
			let ctx = ctx.clone();
			RuntimeSpawner::spawn(async move {
				let result = async {
					let Some(callback) = callback.as_ref() else {
						return Ok(());
					};
					let payload = object();
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					set_anyhow(
						&payload,
						"conn",
						JsValue::from(WasmConnHandle::from_core(conn)),
					)?;
					set_anyhow(&payload, "eventName", JsValue::from_str(&event_name))?;
					call_callback(callback, &payload.into()).await?;
					Ok(())
				}
				.await;
				if let Err(error) = &result {
					console_error(&format!(
						"wasm onBeforeSubscribe callback failed: {error:#}"
					));
				}
				reply.send(result);
			});
		}
		ActorEvent::DisconnectConn { conn_id, reply } => {
			let result = async {
				if let Some(callback) = &callbacks.on_disconnect_final {
					let payload = object();
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					set_anyhow(&payload, "conn", JsValue::NULL)?;
					call_callback(callback, &payload.into()).await?;
				}
				ctx.inner.disconnect_conn(conn_id).await
			}
			.await;
			if let Err(error) = result {
				console_error(&format!("wasm disconnect callback failed: {error:#}"));
			}
			reply.send(Ok(()));
		}
		ActorEvent::ConnectionClosed { conn } => {
			if let Some(callback) = &callbacks.on_disconnect_final {
				let result = async {
					let payload = object();
					set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
					set_anyhow(
						&payload,
						"conn",
						JsValue::from(WasmConnHandle::from_core(conn)),
					)?;
					call_callback(callback, &payload.into()).await?;
					Ok::<_, anyhow::Error>(())
				}
				.await;
				if let Err(error) = result {
					console_error(&format!(
						"wasm connection closed callback failed: {error:#}"
					));
				}
			}
		}
	}
}

async fn run_connection_preflight(
	callbacks: &WasmCallbacks,
	ctx: &WasmActorContext,
	conn: rivetkit_core::ConnHandle,
	params: Vec<u8>,
	request: Option<Request>,
) -> Result<()> {
	if let Some(callback) = &callbacks.on_before_connect {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		set_anyhow(&payload, "params", bytes_to_js(&params))?;
		if let Some(request) = request.as_ref() {
			set_anyhow(&payload, "request", request_to_js(request.clone())?)?;
		}
		call_callback(callback, &payload.into()).await?;
	}

	if let Some(callback) = &callbacks.create_conn_state {
		let payload = object();
		set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
		set_anyhow(
			&payload,
			"conn",
			JsValue::from(WasmConnHandle::from_core(conn.clone())),
		)?;
		set_anyhow(&payload, "params", bytes_to_js(&params))?;
		if let Some(request) = request.as_ref() {
			set_anyhow(&payload, "request", request_to_js(request.clone())?)?;
		}
		let state = call_callback_bytes(callback, &payload.into()).await?;
		conn.set_state_initial(state);
	}

	Ok(())
}

async fn run_connection_open(
	callbacks: &WasmCallbacks,
	ctx: &WasmActorContext,
	conn: rivetkit_core::ConnHandle,
	request: Option<Request>,
) -> Result<()> {
	let Some(callback) = &callbacks.on_connect else {
		return Ok(());
	};
	let payload = object();
	set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
	set_anyhow(
		&payload,
		"conn",
		JsValue::from(WasmConnHandle::from_core(conn)),
	)?;
	if let Some(request) = request {
		set_anyhow(&payload, "request", request_to_js(request)?)?;
	}
	call_callback(callback, &payload.into()).await?;
	Ok(())
}

async fn serialize_state(
	callback: &Function,
	ctx: &WasmActorContext,
	reason: SerializeStateReason,
) -> Result<Vec<StateDelta>> {
	let payload = object();
	set_anyhow(&payload, "ctx", JsValue::from(ctx.clone()))?;
	set_anyhow(
		&payload,
		"reason",
		JsValue::from_str(match reason {
			SerializeStateReason::Save => "save",
			SerializeStateReason::Inspector => "inspector",
		}),
	)?;
	let value = call_callback(callback, &payload.into()).await?;
	state_delta_payload_from_js(value)
}

#[wasm_bindgen(js_name = CancellationToken)]
#[derive(Clone)]
pub struct WasmCancellationToken {
	inner: CoreCancellationToken,
}

#[wasm_bindgen(js_class = CancellationToken)]
impl WasmCancellationToken {
	#[wasm_bindgen(constructor)]
	pub fn new() -> Self {
		Self {
			inner: CoreCancellationToken::new(),
		}
	}

	#[wasm_bindgen]
	pub fn aborted(&self) -> bool {
		self.inner.is_cancelled()
	}

	#[wasm_bindgen]
	pub fn cancel(&self) {
		self.inner.cancel();
	}

	#[wasm_bindgen(js_name = onCancelled)]
	pub fn on_cancelled(&self, callback: Function) {
		let token = self.inner.clone();
		spawn_local(async move {
			token.cancelled().await;
			let _ = callback.call0(&JsValue::UNDEFINED);
		});
	}
}

impl Default for WasmCancellationToken {
	fn default() -> Self {
		Self::new()
	}
}

#[wasm_bindgen(js_name = ActorContext)]
#[derive(Clone)]
pub struct WasmActorContext {
	inner: rivetkit_core::ActorContext,
	callbacks: WasmCallbacks,
	runtime_state: JsValue,
	keep_awake_regions: Rc<RefCell<HashMap<u32, KeepAwakeRegion>>>,
	websocket_callback_regions: Rc<RefCell<HashMap<u32, WebSocketCallbackRegion>>>,
	next_keep_awake_region_id: Rc<Cell<u32>>,
	next_websocket_callback_region_id: Rc<Cell<u32>>,
}

impl WasmActorContext {
	fn from_core(inner: rivetkit_core::ActorContext, callbacks: WasmCallbacks) -> Self {
		Self {
			inner,
			callbacks,
			runtime_state: Object::new().into(),
			keep_awake_regions: Rc::new(RefCell::new(HashMap::new())),
			websocket_callback_regions: Rc::new(RefCell::new(HashMap::new())),
			next_keep_awake_region_id: Rc::new(Cell::new(0)),
			next_websocket_callback_region_id: Rc::new(Cell::new(0)),
		}
	}

	fn allocate_websocket_callback_region_id(
		&self,
		regions: &HashMap<u32, WebSocketCallbackRegion>,
	) -> Option<u32> {
		let start_id = self.next_websocket_callback_region_id.get();
		let mut region_id = start_id;

		for _ in 0..u32::MAX {
			region_id = region_id.wrapping_add(1);
			if region_id == 0 {
				region_id = 1;
			}
			if !regions.contains_key(&region_id) {
				self.next_websocket_callback_region_id.set(region_id);
				return Some(region_id);
			}
		}

		None
	}
}

#[wasm_bindgen(js_class = ActorContext)]
impl WasmActorContext {
	#[wasm_bindgen(constructor)]
	pub fn new() -> Result<WasmActorContext, JsValue> {
		Err(js_error(
			"ActorContext instances are created by rivetkit-core callbacks",
		))
	}

	#[wasm_bindgen]
	pub fn state(&self) -> Vec<u8> {
		self.inner.state()
	}

	#[wasm_bindgen(js_name = runtimeState)]
	pub fn runtime_state(&self) -> JsValue {
		self.runtime_state.clone()
	}

	#[wasm_bindgen]
	pub fn sql(&self) -> WasmSqliteDb {
		WasmSqliteDb {
			inner: self.inner.sql().clone(),
		}
	}

	fn allocate_keep_awake_region_id(
		&self,
		regions: &HashMap<u32, KeepAwakeRegion>,
	) -> Option<u32> {
		for _ in 0..=u32::MAX {
			let next = self.next_keep_awake_region_id.get().wrapping_add(1);
			self.next_keep_awake_region_id.set(next);
			if next != 0 && !regions.contains_key(&next) {
				return Some(next);
			}
		}
		None
	}

	#[wasm_bindgen]
	pub fn kv(&self) -> WasmKv {
		WasmKv {
			inner: self.inner.clone(),
		}
	}

	#[wasm_bindgen(js_name = actorId)]
	pub fn actor_id(&self) -> String {
		self.inner.actor_id().to_owned()
	}

	#[wasm_bindgen]
	pub fn name(&self) -> String {
		self.inner.name().to_owned()
	}

	#[wasm_bindgen]
	pub fn key(&self) -> Result<JsValue, JsValue> {
		let segments: Vec<WasmActorKeySegment> = self
			.inner
			.key()
			.iter()
			.map(|segment| match segment {
				rivetkit_core::ActorKeySegment::String(value) => WasmActorKeySegment {
					kind: "string".to_owned(),
					string_value: Some(value.clone()),
					number_value: None,
				},
				rivetkit_core::ActorKeySegment::Number(value) => WasmActorKeySegment {
					kind: "number".to_owned(),
					string_value: None,
					number_value: Some(*value),
				},
			})
			.collect();
		serde_wasm_bindgen::to_value(&segments).map_err(Into::into)
	}

	#[wasm_bindgen]
	pub fn region(&self) -> String {
		self.inner.region().to_owned()
	}

	#[wasm_bindgen(js_name = beginOnStateChange)]
	pub fn begin_on_state_change(&self) {
		self.inner.on_state_change_started();
	}

	#[wasm_bindgen(js_name = endOnStateChange)]
	pub fn end_on_state_change(&self) {
		self.inner.on_state_change_finished();
	}

	#[wasm_bindgen(js_name = requestSave)]
	pub fn request_save(&self, opts: JsValue) {
		let opts: WasmRequestSaveOpts = if opts.is_null() || opts.is_undefined() {
			WasmRequestSaveOpts::default()
		} else {
			serde_wasm_bindgen::from_value(opts).unwrap_or_default()
		};
		self.inner.request_save(RequestSaveOpts {
			immediate: opts.immediate.unwrap_or(false),
			max_wait_ms: opts.max_wait_ms,
		});
	}

	#[wasm_bindgen(js_name = requestSaveAndWait)]
	pub async fn request_save_and_wait(&self, opts: JsValue) -> Result<(), JsValue> {
		let opts: WasmRequestSaveOpts = if opts.is_null() || opts.is_undefined() {
			WasmRequestSaveOpts::default()
		} else {
			serde_wasm_bindgen::from_value(opts)?
		};
		self.inner
			.request_save_and_wait(RequestSaveOpts {
				immediate: opts.immediate.unwrap_or(false),
				max_wait_ms: opts.max_wait_ms,
			})
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = saveState)]
	pub async fn save_state(&self, payload: JsValue) -> Result<(), JsValue> {
		let deltas = state_delta_payload_from_js(payload).map_err(anyhow_to_js_error)?;
		self.inner
			.save_state(deltas)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = saveStateAndWorkflowBatch)]
	pub async fn save_state_and_workflow_batch(&self, writes: JsValue) -> Result<(), JsValue> {
		let writes: Vec<WasmWorkflowKvWrite> = serde_wasm_bindgen::from_value(writes)?;
		self.inner
			.save_state_and_workflow_batch(
				writes
					.into_iter()
					.map(|write| WorkflowKvWrite {
						key: write.key,
						value: write.value,
					})
					.collect(),
			)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = verifyInspectorAuth)]
	pub async fn verify_inspector_auth(&self, bearer_token: Option<String>) -> Result<(), JsValue> {
		InspectorAuth::new()
			.verify(&self.inner, bearer_token.as_deref())
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = inspectorSnapshot)]
	pub fn inspector_snapshot(&self) -> Result<Object, JsValue> {
		let snapshot = self.inner.inspector_snapshot();
		let object = object();
		set(
			&object,
			"stateRevision",
			JsValue::from_f64(snapshot.state_revision as f64),
		)?;
		set(
			&object,
			"connectionsRevision",
			JsValue::from_f64(snapshot.connections_revision as f64),
		)?;
		set(
			&object,
			"queueRevision",
			JsValue::from_f64(snapshot.queue_revision as f64),
		)?;
		set(
			&object,
			"activeConnections",
			JsValue::from_f64(snapshot.active_connections as f64),
		)?;
		set(
			&object,
			"queueSize",
			JsValue::from_f64(snapshot.queue_size as f64),
		)?;
		set(
			&object,
			"connectedClients",
			JsValue::from_f64(snapshot.connected_clients as f64),
		)?;
		Ok(object)
	}

	#[wasm_bindgen(js_name = takePendingHibernationChanges)]
	pub fn take_pending_hibernation_changes(&self) -> Array {
		let array = Array::new();
		for conn_id in self.inner.take_pending_hibernation_changes() {
			array.push(&JsValue::from_str(&conn_id));
		}
		array
	}

	#[wasm_bindgen(js_name = dirtyHibernatableConns)]
	pub fn dirty_hibernatable_conns(&self) -> Array {
		let array = Array::new();
		for conn in self.inner.dirty_hibernatable_conns() {
			array.push(&JsValue::from(WasmConnHandle::from_core(conn)));
		}
		array
	}

	#[wasm_bindgen]
	pub fn conns(&self) -> Array {
		let array = Array::new();
		for conn in self.inner.conns() {
			array.push(&JsValue::from(WasmConnHandle::from_core(conn)));
		}
		array
	}

	#[wasm_bindgen(js_name = connectConn)]
	pub async fn connect_conn(
		&self,
		params: Vec<u8>,
		request: JsValue,
	) -> Result<WasmConnHandle, JsValue> {
		let request = request_from_js(request).map_err(anyhow_to_js_error)?;
		self.inner
			.connect_conn_with_request(params, request, async { Ok(Vec::new()) })
			.await
			.map(WasmConnHandle::from_core)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = setAlarm)]
	pub fn set_alarm(&self, timestamp_ms: Option<f64>) -> Result<(), JsValue> {
		let timestamp_ms = timestamp_ms
			.filter(|value| value.is_finite())
			.map(|value| value.trunc() as i64);
		self.inner
			.set_alarm(timestamp_ms)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub fn sleep(&self) -> Result<(), JsValue> {
		self.inner.sleep().map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub fn destroy(&self) -> Result<(), JsValue> {
		self.inner.destroy().map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = abortSignal)]
	pub fn abort_signal(&self) -> Result<JsValue, JsValue> {
		let controller = new_js_class("AbortController")?;
		if self.inner.actor_aborted() {
			call_js_method0(&controller, "abort")?;
		} else {
			let token = self.inner.actor_abort_signal();
			let controller_for_task = controller.clone();
			spawn_local(async move {
				token.cancelled().await;
				if let Err(error) = call_js_method0(&controller_for_task, "abort") {
					console_error(&format!(
						"failed to abort wasm actor abort signal: {}",
						js_value_to_anyhow(error)
					));
				}
			});
		}
		Reflect::get(&controller, &JsValue::from_str("signal"))
	}

	#[wasm_bindgen]
	pub fn broadcast(&self, name: String, args: Vec<u8>) {
		self.inner.broadcast(&name, &args);
	}

	#[wasm_bindgen(js_name = waitUntil)]
	pub fn wait_until(&self, promise: Promise) {
		let actor_id = self.inner.actor_id().to_owned();
		self.inner.wait_until(async move {
			if let Err(error) = JsFuture::from(promise).await {
				console_error(&format!(
					"actor wait_until promise rejected for actor {actor_id}: {}",
					js_value_to_anyhow(error)
				));
			}
		});
	}

	#[wasm_bindgen(js_name = waitForTrackedShutdownWork)]
	pub async fn wait_for_tracked_shutdown_work(&self) -> bool {
		self.inner.wait_for_tracked_shutdown_work().await
	}

	#[wasm_bindgen(js_name = waitForTrackedShutdownWorkUnbounded)]
	pub async fn wait_for_tracked_shutdown_work_unbounded(&self) {
		self.inner.wait_for_tracked_shutdown_work_unbounded().await;
	}

	#[wasm_bindgen(js_name = keepAwake)]
	pub fn keep_awake(&self, promise: Promise) {
		console_error("keepAwake binding is deprecated; use beginKeepAwake/endKeepAwake");
		let actor_id = self.inner.actor_id().to_owned();
		self.inner.spawn_work(ActorWorkKind::KeepAwake, async move {
			if let Err(error) = JsFuture::from(promise).await {
				console_error(&format!(
					"actor keepAwake promise rejected for actor {actor_id}: {}",
					js_value_to_anyhow(error)
				));
			}
		});
	}

	#[wasm_bindgen(js_name = registerTask)]
	pub fn register_task(&self, promise: Promise) {
		let actor_id = self.inner.actor_id().to_owned();
		self.inner.register_task(async move {
			if let Err(error) = JsFuture::from(promise).await {
				console_error(&format!(
					"actor registered task promise rejected for actor {actor_id}: {}",
					js_value_to_anyhow(error)
				));
			}
		});
	}

	#[wasm_bindgen(js_name = restartRunHandler)]
	pub fn restart_run_handler(&self) {
		start_run_handler(&self.callbacks, self);
	}

	#[wasm_bindgen(js_name = beginKeepAwake)]
	pub fn begin_keep_awake(&self) -> u32 {
		let mut regions = self.keep_awake_regions.borrow_mut();
		let Some(region_id) = self.allocate_keep_awake_region_id(&regions) else {
			console_error("failed to begin keep-awake region: no region ids available");
			return 0;
		};
		regions.insert(region_id, self.inner.keep_awake_region());
		region_id
	}

	#[wasm_bindgen(js_name = endKeepAwake)]
	pub fn end_keep_awake(&self, region_id: u32) {
		if region_id == 0 {
			return;
		}
		self.keep_awake_regions.borrow_mut().remove(&region_id);
	}

	#[wasm_bindgen(js_name = beginWebsocketCallback)]
	pub fn begin_websocket_callback(&self) -> u32 {
		let mut regions = self.websocket_callback_regions.borrow_mut();
		let Some(region_id) = self.allocate_websocket_callback_region_id(&regions) else {
			console_error("failed to begin websocket callback region: no region ids available");
			return 0;
		};
		regions.insert(region_id, self.inner.websocket_callback_region());
		region_id
	}

	#[wasm_bindgen(js_name = endWebsocketCallback)]
	pub fn end_websocket_callback(&self, region_id: u32) {
		if region_id == 0 {
			return;
		}
		self.websocket_callback_regions
			.borrow_mut()
			.remove(&region_id);
	}

	#[wasm_bindgen]
	pub fn schedule(&self) -> WasmSchedule {
		WasmSchedule {
			inner: self.inner.clone(),
		}
	}

	#[wasm_bindgen]
	pub fn queue(&self) -> WasmQueue {
		WasmQueue {
			inner: self.inner.clone(),
		}
	}
}

#[wasm_bindgen(js_name = ConnHandle)]
#[derive(Clone)]
pub struct WasmConnHandle {
	inner: rivetkit_core::ConnHandle,
}

#[wasm_bindgen(js_name = WebSocketHandle)]
#[derive(Clone)]
pub struct WasmWebSocket {
	inner: WebSocket,
}

impl WasmConnHandle {
	fn from_core(inner: rivetkit_core::ConnHandle) -> Self {
		Self { inner }
	}
}

#[wasm_bindgen(js_class = ConnHandle)]
impl WasmConnHandle {
	#[wasm_bindgen]
	pub fn id(&self) -> String {
		self.inner.id().to_owned()
	}

	#[wasm_bindgen]
	pub fn params(&self) -> Vec<u8> {
		self.inner.params()
	}

	#[wasm_bindgen]
	pub fn state(&self) -> Vec<u8> {
		self.inner.state()
	}

	#[wasm_bindgen(js_name = setState)]
	pub fn set_state(&self, state: Vec<u8>) {
		self.inner.set_state(state);
	}

	#[wasm_bindgen(js_name = isHibernatable)]
	pub fn is_hibernatable(&self) -> bool {
		self.inner.is_hibernatable()
	}

	#[wasm_bindgen]
	pub fn send(&self, name: String, args: Vec<u8>) {
		self.inner.send(&name, &args);
	}

	#[wasm_bindgen]
	pub async fn disconnect(&self, reason: Option<String>) -> Result<(), JsValue> {
		self.inner
			.disconnect(reason.as_deref())
			.await
			.map_err(anyhow_to_js_error)
	}
}

impl WasmWebSocket {
	fn from_core(inner: WebSocket) -> Self {
		Self { inner }
	}
}

#[wasm_bindgen(js_class = WebSocketHandle)]
impl WasmWebSocket {
	#[wasm_bindgen]
	pub fn send(&self, data: Vec<u8>, binary: bool) -> Result<(), JsValue> {
		let message = if binary {
			WsMessage::Binary(data)
		} else {
			WsMessage::Text(String::from_utf8(data).map_err(|error| {
				js_error(&format!("websocket text frame is not valid utf-8: {error}"))
			})?)
		};
		self.inner.send(message);
		Ok(())
	}

	#[wasm_bindgen]
	pub async fn close(&self, code: Option<u16>, reason: Option<String>) -> Result<(), JsValue> {
		self.inner.close(code, reason).await;
		Ok(())
	}

	#[wasm_bindgen(js_name = setEventCallback)]
	pub fn set_event_callback(&self, callback: Function) {
		let callback = Arc::new(WasmFunction(callback));
		let message_callback = callback.clone();
		self.inner.configure_message_event_callback(Some(Arc::new(
			move |message, message_index| {
				let event = websocket_message_event_to_js(message, message_index)
					.map_err(js_value_to_anyhow)?;
				message_callback.call1(&event)?;
				Ok(())
			},
		)));

		let callback = callback.clone();
		self.inner.configure_close_event_callback(Some(Arc::new(
			move |code, reason, was_clean| {
				let callback = callback.clone();
				let result = (|| {
					let event = websocket_close_event_to_js(code, reason, was_clean)
						.map_err(js_value_to_anyhow)?;
					callback.call1(&event).map(|_| ())
				})();
				Box::pin(async move { result })
			},
		)));
	}
}

#[wasm_bindgen(js_name = Schedule)]
pub struct WasmSchedule {
	inner: rivetkit_core::ActorContext,
}

#[wasm_bindgen(js_class = Schedule)]
impl WasmSchedule {
	#[wasm_bindgen]
	pub async fn after(
		&self,
		duration_ms: f64,
		action_name: String,
		args: Vec<u8>,
	) -> Result<String, JsValue> {
		let duration = if duration_ms.is_finite() && duration_ms > 0.0 {
			Duration::from_millis(duration_ms as u64)
		} else {
			Duration::from_millis(0)
		};
		self.inner
			.after(duration, &action_name, &args)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn at(
		&self,
		timestamp_ms: f64,
		action_name: String,
		args: Vec<u8>,
	) -> Result<String, JsValue> {
		self.inner
			.at(timestamp_ms as i64, &action_name, &args)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn cancel(&self, id: String) -> Result<bool, JsValue> {
		self.inner
			.cancel_schedule(&id)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn get(&self, id: String) -> Result<JsValue, JsValue> {
		let event = self
			.inner
			.get_scheduled_event(&id)
			.await
			.map_err(anyhow_to_js_error)?;
		serde_wasm_bindgen::to_value(&event).map_err(Into::into)
	}

	#[wasm_bindgen]
	pub async fn list(&self) -> Result<JsValue, JsValue> {
		let events = self
			.inner
			.list_scheduled_events()
			.await
			.map_err(anyhow_to_js_error)?;
		serde_wasm_bindgen::to_value(&events).map_err(Into::into)
	}

	#[wasm_bindgen(js_name = cronSet)]
	pub async fn cron_set(
		&self,
		name: String,
		expression: String,
		timezone: Option<String>,
		action_name: String,
		args: Vec<u8>,
		max_history: Option<f64>,
	) -> Result<(), JsValue> {
		self.inner
			.cron_set(
				&name,
				&expression,
				timezone.as_deref(),
				&action_name,
				&args,
				max_history.map(|value| value as i64),
			)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = cronEvery)]
	pub async fn cron_every(
		&self,
		name: String,
		interval_ms: f64,
		action_name: String,
		args: Vec<u8>,
		max_history: Option<f64>,
	) -> Result<(), JsValue> {
		self.inner
			.cron_every(
				&name,
				interval_ms as i64,
				&action_name,
				&args,
				max_history.map(|value| value as i64),
			)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = cronGet)]
	pub async fn cron_get(&self, name: String) -> Result<JsValue, JsValue> {
		let job = self
			.inner
			.cron_get(&name)
			.await
			.map_err(anyhow_to_js_error)?;
		serde_wasm_bindgen::to_value(&job).map_err(Into::into)
	}

	#[wasm_bindgen(js_name = cronList)]
	pub async fn cron_list(&self) -> Result<JsValue, JsValue> {
		let jobs = self.inner.cron_list().await.map_err(anyhow_to_js_error)?;
		serde_wasm_bindgen::to_value(&jobs).map_err(Into::into)
	}

	#[wasm_bindgen(js_name = cronDelete)]
	pub async fn cron_delete(&self, name: String) -> Result<bool, JsValue> {
		self.inner
			.cron_delete(&name)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = cronHistory)]
	pub async fn cron_history(&self, name: String, limit: Option<f64>) -> Result<JsValue, JsValue> {
		let history = self
			.inner
			.cron_history(&name, limit.map(|value| value as i64))
			.await
			.map_err(anyhow_to_js_error)?;
		serde_wasm_bindgen::to_value(&history).map_err(Into::into)
	}
}

#[wasm_bindgen(js_name = Kv)]
pub struct WasmKv {
	inner: rivetkit_core::ActorContext,
}

#[wasm_bindgen(js_class = Kv)]
#[allow(deprecated)]
impl WasmKv {
	#[wasm_bindgen]
	pub async fn get(&self, key: Vec<u8>) -> Result<JsValue, JsValue> {
		self.inner
			.kv_batch_get(&[key.as_slice()])
			.await
			.map(|mut values| match values.pop().flatten() {
				Some(value) => bytes_to_js(&value),
				None => JsValue::NULL,
			})
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), JsValue> {
		self.inner
			.kv_batch_put(&[(key.as_slice(), value.as_slice())])
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = delete)]
	pub async fn delete_key(&self, key: Vec<u8>) -> Result<(), JsValue> {
		self.inner
			.kv_batch_delete(&[key.as_slice()])
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = deleteRange)]
	pub async fn delete_range(&self, start: Vec<u8>, end: Vec<u8>) -> Result<(), JsValue> {
		self.inner
			.kv_delete_range(&start, &end)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = listPrefix)]
	pub async fn list_prefix(&self, prefix: Vec<u8>, options: JsValue) -> Result<JsValue, JsValue> {
		self.inner
			.kv_list_prefix(&prefix, list_opts_from_js(options)?)
			.await
			.map(kv_entries_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = listRange)]
	pub async fn list_range(
		&self,
		start: Vec<u8>,
		end: Vec<u8>,
		options: JsValue,
	) -> Result<JsValue, JsValue> {
		self.inner
			.kv_list_range(&start, &end, list_opts_from_js(options)?)
			.await
			.map(kv_entries_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = batchGet)]
	pub async fn batch_get(&self, keys: Array) -> Result<JsValue, JsValue> {
		let keys = bytes_array_from_js(keys);
		let key_refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
		self.inner
			.kv_batch_get(&key_refs)
			.await
			.map(|values| {
				let array = Array::new();
				for value in values {
					array.push(
						&value
							.map(|value| bytes_to_js(&value))
							.unwrap_or(JsValue::NULL),
					);
				}
				array.into()
			})
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = batchPut)]
	pub async fn batch_put(&self, entries: Array) -> Result<(), JsValue> {
		let entries = kv_entries_from_js(entries)?;
		let entry_refs: Vec<(&[u8], &[u8])> = entries
			.iter()
			.map(|(key, value)| (key.as_slice(), value.as_slice()))
			.collect();
		self.inner
			.kv_batch_put(&entry_refs)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = batchDelete)]
	pub async fn batch_delete(&self, keys: Array) -> Result<(), JsValue> {
		let keys = bytes_array_from_js(keys);
		let key_refs: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
		self.inner
			.kv_batch_delete(&key_refs)
			.await
			.map_err(anyhow_to_js_error)
	}
}

#[wasm_bindgen(js_name = Queue)]
pub struct WasmQueue {
	inner: rivetkit_core::ActorContext,
}

#[wasm_bindgen(js_class = Queue)]
impl WasmQueue {
	#[wasm_bindgen]
	pub async fn send(&self, name: String, body: Vec<u8>) -> Result<WasmQueueMessage, JsValue> {
		self.inner
			.send(&name, &body)
			.await
			.map(WasmQueueMessage::from_core)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = nextBatch)]
	pub async fn next_batch(
		&self,
		options: JsValue,
		signal: Option<WasmCancellationToken>,
	) -> Result<Array, JsValue> {
		let mut options = queue_next_batch_options(options)?;
		options.signal = signal.map(|signal| signal.inner);
		let messages = self
			.inner
			.next_batch(options)
			.await
			.map_err(anyhow_to_js_error)?;
		queue_messages_to_js(messages)
	}

	#[wasm_bindgen(js_name = waitForNames)]
	pub async fn wait_for_names(
		&self,
		names: JsValue,
		options: JsValue,
		signal: Option<WasmCancellationToken>,
	) -> Result<WasmQueueMessage, JsValue> {
		let names: Vec<String> = serde_wasm_bindgen::from_value(names)?;
		let mut options = queue_wait_options(options)?;
		options.signal = signal.map(|signal| signal.inner);
		self.inner
			.wait_for_names(names, options)
			.await
			.map(WasmQueueMessage::from_core)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = waitForNamesAvailable)]
	pub async fn wait_for_names_available(
		&self,
		names: JsValue,
		options: JsValue,
	) -> Result<(), JsValue> {
		let names: Vec<String> = serde_wasm_bindgen::from_value(names)?;
		self.inner
			.wait_for_names_available(names, queue_wait_options(options)?)
			.await
			.map_err(anyhow_to_js_error)?;
		Ok(())
	}

	#[wasm_bindgen(js_name = enqueueAndWait)]
	pub async fn enqueue_and_wait(
		&self,
		name: String,
		body: Vec<u8>,
		options: JsValue,
		signal: Option<WasmCancellationToken>,
	) -> Result<Option<Vec<u8>>, JsValue> {
		let mut options = enqueue_and_wait_options(options)?;
		options.signal = signal.map(|signal| signal.inner);
		self.inner
			.enqueue_and_wait(&name, &body, options)
			.await
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = tryNextBatch)]
	pub fn try_next_batch(&self, options: JsValue) -> Result<Array, JsValue> {
		let options = queue_try_next_batch_options(options)?;
		let messages = self
			.inner
			.try_next_batch(options)
			.map_err(anyhow_to_js_error)?;
		queue_messages_to_js(messages)
	}

	#[wasm_bindgen(js_name = maxSize)]
	pub fn max_size(&self) -> u32 {
		self.inner.queue().max_size()
	}

	#[wasm_bindgen]
	pub async fn reset(&self) -> Result<(), JsValue> {
		self.inner.queue().reset().await.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = inspectMessages)]
	pub async fn inspect_messages(&self) -> Result<Array, JsValue> {
		let messages = self
			.inner
			.inspect_messages()
			.await
			.map_err(anyhow_to_js_error)?;
		let array = Array::new();
		for message in messages {
			let object = object();
			set(&object, "id", JsValue::from_f64(message.id as f64))?;
			set(&object, "name", JsValue::from_str(&message.name))?;
			set(
				&object,
				"createdAtMs",
				JsValue::from_f64(message.created_at as f64),
			)?;
			array.push(&object.into());
		}
		Ok(array)
	}
}

#[wasm_bindgen(js_name = QueueMessage)]
pub struct WasmQueueMessage {
	inner: Option<QueueMessage>,
}

impl WasmQueueMessage {
	fn from_core(inner: QueueMessage) -> Self {
		Self { inner: Some(inner) }
	}

	fn inner(&self) -> &QueueMessage {
		self.inner
			.as_ref()
			.expect_throw("queue message already completed")
	}
}

#[wasm_bindgen(js_class = QueueMessage)]
impl WasmQueueMessage {
	#[wasm_bindgen]
	pub fn id(&self) -> u64 {
		self.inner().id
	}

	#[wasm_bindgen]
	pub fn name(&self) -> String {
		self.inner().name.clone()
	}

	#[wasm_bindgen]
	pub fn body(&self) -> Vec<u8> {
		self.inner().body.clone()
	}

	#[wasm_bindgen(js_name = createdAt)]
	pub fn created_at(&self) -> f64 {
		self.inner().created_at as f64
	}

	#[wasm_bindgen(js_name = isCompletable)]
	pub fn is_completable(&self) -> bool {
		self.inner().clone().into_completable().is_ok()
	}

	#[wasm_bindgen]
	pub async fn complete(&mut self, response: JsValue) -> Result<(), JsValue> {
		let message = self
			.inner
			.take()
			.ok_or_else(|| js_error("queue message already completed"))?;
		let response = if response.is_null() || response.is_undefined() {
			None
		} else {
			Some(js_to_bytes(response))
		};
		message.complete(response).await.map_err(anyhow_to_js_error)
	}
}

#[wasm_bindgen(js_name = SqliteDb)]
pub struct WasmSqliteDb {
	inner: rivetkit_core::SqliteDb,
}

#[wasm_bindgen(js_class = SqliteDb)]
impl WasmSqliteDb {
	#[wasm_bindgen]
	pub async fn exec(&self, sql: String) -> Result<JsValue, JsValue> {
		self.inner
			.exec(sql)
			.await
			.map(query_result_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn execute(&self, sql: String, params: JsValue) -> Result<JsValue, JsValue> {
		self.inner
			.execute(sql, bind_params_from_js(params)?)
			.await
			.map(execute_result_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = executeBatch)]
	pub async fn execute_batch(&self, statements: JsValue) -> Result<JsValue, JsValue> {
		let statements: Vec<WasmSqliteBatchStatement> = serde_wasm_bindgen::from_value(statements)?;
		let statements = statements
			.into_iter()
			.map(|statement| {
				Ok(SqliteBatchStatement {
					sql: statement.sql,
					params: statement
						.params
						.map(|params| {
							params
								.into_iter()
								.map(bind_param_from_wasm)
								.collect::<Result<Vec<_>, _>>()
						})
						.transpose()?,
				})
			})
			.collect::<Result<Vec<_>, JsValue>>()?;
		let results = self
			.inner
			.execute_batch(statements)
			.await
			.map_err(anyhow_to_js_error)?;
		let values = Array::new();
		for result in results {
			values.push(&execute_result_to_js(result));
		}
		Ok(values.into())
	}

	#[wasm_bindgen]
	pub async fn query(&self, sql: String, params: JsValue) -> Result<JsValue, JsValue> {
		self.inner
			.query(sql, bind_params_from_js(params)?)
			.await
			.map(query_result_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn run(&self, sql: String, params: JsValue) -> Result<JsValue, JsValue> {
		let result = self
			.inner
			.run(sql, bind_params_from_js(params)?)
			.await
			.map_err(anyhow_to_js_error)?;
		let object = object();
		set(&object, "changes", JsValue::from_f64(result.changes as f64))?;
		Ok(object.into())
	}

	#[wasm_bindgen]
	pub async fn close(&self) -> Result<(), JsValue> {
		self.inner.close().await.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen(js_name = beginTransaction)]
	pub async fn begin_transaction(
		&self,
		timeout_ms: Option<f64>,
	) -> Result<WasmSqliteTransaction, JsValue> {
		let timeout = timeout_ms.map(transaction_timeout).transpose()?;
		self.inner
			.begin_transaction(timeout)
			.await
			.map(|inner| WasmSqliteTransaction { inner })
			.map_err(anyhow_to_js_error)
	}
}

#[wasm_bindgen(js_name = SqliteTransaction)]
pub struct WasmSqliteTransaction {
	inner: rivetkit_core::SqliteTransaction,
}

#[wasm_bindgen(js_class = SqliteTransaction)]
impl WasmSqliteTransaction {
	#[wasm_bindgen]
	pub async fn exec(&self, sql: String) -> Result<JsValue, JsValue> {
		self.inner
			.exec(sql)
			.await
			.map(query_result_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn execute(&self, sql: String, params: JsValue) -> Result<JsValue, JsValue> {
		self.inner
			.execute(sql, bind_params_from_js(params)?)
			.await
			.map(execute_result_to_js)
			.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn commit(&self) -> Result<(), JsValue> {
		self.inner.commit().await.map_err(anyhow_to_js_error)
	}

	#[wasm_bindgen]
	pub async fn rollback(&self) -> Result<(), JsValue> {
		self.inner.rollback().await.map_err(anyhow_to_js_error)
	}
}

fn transaction_timeout(timeout_ms: f64) -> Result<std::time::Duration, JsValue> {
	if !timeout_ms.is_finite() || timeout_ms <= 0.0 {
		return Err(js_error(
			"transaction timeout must be a positive finite number of milliseconds",
		));
	}
	std::time::Duration::try_from_secs_f64(timeout_ms / 1_000.0)
		.map_err(|_| js_error("transaction timeout is too large to represent"))
}

#[wasm_bindgen(js_name = bridgeRivetErrorPrefix)]
pub fn bridge_rivet_error_prefix() -> String {
	BRIDGE_RIVET_ERROR_PREFIX.to_string()
}

#[wasm_bindgen(js_name = roundTripBytes)]
pub fn round_trip_bytes(bytes: Vec<u8>) -> Vec<u8> {
	bytes
}

#[wasm_bindgen(js_name = uint8ArrayFromBytes)]
pub fn uint8_array_from_bytes(bytes: Vec<u8>) -> Uint8Array {
	Uint8Array::from(bytes.as_slice())
}

#[wasm_bindgen(js_name = awaitPromise)]
pub async fn await_promise(promise: Promise) -> Result<JsValue, JsValue> {
	JsFuture::from(promise).await
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WasmRequestSaveOpts {
	immediate: Option<bool>,
	max_wait_ms: Option<u32>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WasmQueueNextBatchOptions {
	names: Option<Vec<String>>,
	count: Option<u32>,
	timeout_ms: Option<f64>,
	completable: Option<bool>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WasmQueueWaitOptions {
	timeout_ms: Option<f64>,
	completable: Option<bool>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WasmQueueEnqueueAndWaitOptions {
	timeout_ms: Option<f64>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WasmQueueTryNextBatchOptions {
	names: Option<Vec<String>>,
	count: Option<u32>,
	completable: Option<bool>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WasmActorKeySegment {
	kind: String,
	string_value: Option<String>,
	number_value: Option<f64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmStateDeltaPayload {
	state: Option<Vec<u8>>,
	conn_hibernation: Vec<WasmConnHibernationEntry>,
	conn_hibernation_removed: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmWorkflowKvWrite {
	key: Vec<u8>,
	value: Vec<u8>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmConnHibernationEntry {
	conn_id: String,
	bytes: Vec<u8>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmBindParam {
	kind: String,
	int_value: Option<f64>,
	float_value: Option<f64>,
	text_value: Option<String>,
	blob_value: Option<Vec<u8>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmSqliteBatchStatement {
	sql: String,
	params: Option<Vec<WasmBindParam>>,
}

fn optional_timeout_ms(timeout_ms: Option<f64>) -> Option<Duration> {
	let timeout_ms = timeout_ms?;
	if !timeout_ms.is_finite() || timeout_ms < 0.0 {
		return None;
	}
	Some(Duration::from_millis(timeout_ms as u64))
}

fn queue_next_batch_options(value: JsValue) -> Result<QueueNextBatchOpts, JsValue> {
	let options: WasmQueueNextBatchOptions = if value.is_null() || value.is_undefined() {
		WasmQueueNextBatchOptions::default()
	} else {
		serde_wasm_bindgen::from_value(value)?
	};
	Ok(QueueNextBatchOpts {
		names: options.names,
		count: options.count.unwrap_or(1),
		timeout: optional_timeout_ms(options.timeout_ms),
		signal: None,
		completable: options.completable.unwrap_or(false),
	})
}

fn queue_wait_options(value: JsValue) -> Result<QueueWaitOpts, JsValue> {
	let options: WasmQueueWaitOptions = if value.is_null() || value.is_undefined() {
		WasmQueueWaitOptions::default()
	} else {
		serde_wasm_bindgen::from_value(value)?
	};
	Ok(QueueWaitOpts {
		timeout: optional_timeout_ms(options.timeout_ms),
		signal: None,
		completable: options.completable.unwrap_or(false),
	})
}

fn enqueue_and_wait_options(value: JsValue) -> Result<EnqueueAndWaitOpts, JsValue> {
	let options: WasmQueueEnqueueAndWaitOptions = if value.is_null() || value.is_undefined() {
		WasmQueueEnqueueAndWaitOptions::default()
	} else {
		serde_wasm_bindgen::from_value(value)?
	};
	Ok(EnqueueAndWaitOpts {
		timeout: optional_timeout_ms(options.timeout_ms),
		signal: None,
	})
}

fn queue_try_next_batch_options(value: JsValue) -> Result<QueueTryNextBatchOpts, JsValue> {
	let options: WasmQueueTryNextBatchOptions = if value.is_null() || value.is_undefined() {
		WasmQueueTryNextBatchOptions::default()
	} else {
		serde_wasm_bindgen::from_value(value)?
	};
	Ok(QueueTryNextBatchOpts {
		names: options.names,
		count: options.count.unwrap_or(1),
		completable: options.completable.unwrap_or(false),
	})
}

fn queue_messages_to_js(messages: Vec<QueueMessage>) -> Result<Array, JsValue> {
	let array = Array::new();
	for message in messages {
		array.push(&JsValue::from(WasmQueueMessage::from_core(message)));
	}
	Ok(array)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WasmQueueSendResult {
	status: String,
	response: Option<Vec<u8>>,
}

fn request_to_js(request: Request) -> Result<JsValue> {
	let (method, uri, headers, body) = request.to_parts();
	let request_object = object();
	set_anyhow(&request_object, "method", JsValue::from_str(&method))?;
	set_anyhow(&request_object, "uri", JsValue::from_str(&uri))?;
	let headers_object = object();
	for (name, value) in headers {
		set_anyhow(&headers_object, &name, JsValue::from_str(&value))?;
	}
	set_anyhow(&request_object, "headers", headers_object.into())?;
	set_anyhow(&request_object, "body", bytes_to_js(&body))?;
	Ok(request_object.into())
}

fn request_from_js(value: JsValue) -> Result<Option<Request>> {
	if value.is_null() || value.is_undefined() {
		return Ok(None);
	}
	let method = js_string_property(&value, "method")?.unwrap_or_else(|| "GET".to_owned());
	let uri = js_string_property(&value, "uri")?.unwrap_or_else(|| "/".to_owned());
	Ok(Some(Request::from_parts(
		&method,
		&uri,
		js_string_map_property(&value, "headers")?,
		js_bytes_property(&value, "body")?.unwrap_or_default(),
	)?))
}

fn response_from_js(value: JsValue) -> Result<Response> {
	Response::from_parts(
		js_number_property(&value, "status")?.unwrap_or(200.0) as u16,
		js_string_map_property(&value, "headers")?,
		js_bytes_property(&value, "body")?.unwrap_or_default(),
	)
}

fn queue_send_result_from_js(value: JsValue) -> Result<QueueSendResult> {
	let result: WasmQueueSendResult = serde_wasm_bindgen::from_value(value)
		.map_err(|error| anyhow!("decode queue send result: {error}"))?;
	let status = match result.status.as_str() {
		"completed" => QueueSendStatus::Completed,
		"timedOut" => QueueSendStatus::TimedOut,
		other => return Err(anyhow!("invalid queue send status `{other}`")),
	};
	Ok(QueueSendResult {
		status,
		response: result.response,
	})
}

fn websocket_message_event_to_js(
	message: WsMessage,
	message_index: Option<u16>,
) -> Result<JsValue, JsValue> {
	let object = object();
	set(&object, "kind", JsValue::from_str("message"))?;
	match message {
		WsMessage::Text(text) => {
			set(&object, "binary", JsValue::FALSE)?;
			set(&object, "data", JsValue::from_str(&text))?;
		}
		WsMessage::Binary(bytes) => {
			set(&object, "binary", JsValue::TRUE)?;
			set(&object, "data", bytes_to_js(&bytes))?;
		}
	}
	if let Some(message_index) = message_index {
		set(
			&object,
			"messageIndex",
			JsValue::from_f64(message_index as f64),
		)?;
	}
	Ok(object.into())
}

fn websocket_close_event_to_js(
	code: u16,
	reason: String,
	was_clean: bool,
) -> Result<JsValue, JsValue> {
	let object = object();
	set(&object, "kind", JsValue::from_str("close"))?;
	set(&object, "code", JsValue::from_f64(code as f64))?;
	set(&object, "reason", JsValue::from_str(&reason))?;
	set(&object, "wasClean", JsValue::from_bool(was_clean))?;
	Ok(object.into())
}

fn object() -> Object {
	Object::new()
}

fn set(object: &Object, key: &str, value: JsValue) -> Result<(), JsValue> {
	Reflect::set(object, &JsValue::from_str(key), &value).map(|_| ())
}

fn set_anyhow(object: &Object, key: &str, value: JsValue) -> Result<()> {
	set(object, key, value).map_err(js_value_to_anyhow)
}

fn bytes_to_js(bytes: &[u8]) -> JsValue {
	Uint8Array::from(bytes).into()
}

fn js_to_bytes(value: JsValue) -> Vec<u8> {
	if value.is_null() || value.is_undefined() {
		return Vec::new();
	}
	Uint8Array::new(&value).to_vec()
}

fn function_property(target: &JsValue, name: &str) -> Option<Function> {
	Reflect::get(target, &JsValue::from_str(name))
		.ok()
		.and_then(|value| {
			if value.is_null() || value.is_undefined() {
				None
			} else {
				value.dyn_into::<Function>().ok()
			}
		})
}

fn js_property(target: &JsValue, name: &str) -> Result<JsValue> {
	Reflect::get(target, &JsValue::from_str(name)).map_err(js_value_to_anyhow)
}

fn js_string_property(target: &JsValue, name: &str) -> Result<Option<String>> {
	let value = js_property(target, name)?;
	if value.is_null() || value.is_undefined() {
		return Ok(None);
	}
	value
		.as_string()
		.map(Some)
		.ok_or_else(|| anyhow!("property `{name}` must be a string"))
}

fn js_number_property(target: &JsValue, name: &str) -> Result<Option<f64>> {
	let value = js_property(target, name)?;
	if value.is_null() || value.is_undefined() {
		return Ok(None);
	}
	value
		.as_f64()
		.map(Some)
		.ok_or_else(|| anyhow!("property `{name}` must be a number"))
}

fn js_bool_property(target: &JsValue, name: &str) -> Result<Option<bool>> {
	let value = js_property(target, name)?;
	if value.is_null() || value.is_undefined() {
		return Ok(None);
	}
	value
		.as_bool()
		.map(Some)
		.ok_or_else(|| anyhow!("property `{name}` must be a boolean"))
}

fn js_bytes_property(target: &JsValue, name: &str) -> Result<Option<Vec<u8>>> {
	let value = js_property(target, name)?;
	if value.is_null() || value.is_undefined() {
		return Ok(None);
	}
	Ok(Some(js_to_bytes(value)))
}

fn js_string_map_property(target: &JsValue, name: &str) -> Result<HashMap<String, String>> {
	let value = js_property(target, name)?;
	if value.is_null() || value.is_undefined() {
		return Ok(HashMap::new());
	}
	let object = value
		.dyn_into::<Object>()
		.map_err(|_| anyhow!("property `{name}` must be an object"))?;
	let keys = Object::keys(&object);
	let mut map = HashMap::new();
	for index in 0..keys.length() {
		let key = keys
			.get(index)
			.as_string()
			.ok_or_else(|| anyhow!("property `{name}` contains a non-string key"))?;
		let value = Reflect::get(&object, &JsValue::from_str(&key))
			.map_err(js_value_to_anyhow)?
			.as_string()
			.ok_or_else(|| anyhow!("property `{name}.{key}` must be a string"))?;
		map.insert(key, value);
	}
	Ok(map)
}

fn required_js_string_property(target: &JsValue, name: &str) -> Result<String> {
	js_string_property(target, name)?.ok_or_else(|| anyhow!("property `{name}` must be a string"))
}

fn serverless_request_from_js(
	value: JsValue,
	cancel_token: CoreCancellationToken,
) -> Result<ServerlessRequest> {
	let method = required_js_string_property(&value, "method")?;
	let url = required_js_string_property(&value, "url")?;
	let headers = js_string_map_property(&value, "headers")?;
	let body = js_bytes_property(&value, "body")?.unwrap_or_default();
	Ok(ServerlessRequest {
		method,
		url,
		headers,
		body,
		cancel_token,
	})
}

fn serverless_response_head_to_js(
	status: u16,
	headers: HashMap<String, String>,
) -> Result<JsValue> {
	let head = object();
	set_anyhow(&head, "status", JsValue::from_f64(status as f64))?;
	let header_object = object();
	for (key, value) in headers {
		set_anyhow(&header_object, &key, JsValue::from_str(&value))?;
	}
	set_anyhow(&head, "headers", header_object.into())?;
	Ok(head.into())
}

fn serverless_stream_chunk_event(chunk: Vec<u8>) -> Result<JsValue> {
	let event = object();
	set_anyhow(&event, "kind", JsValue::from_str("chunk"))?;
	set_anyhow(&event, "chunk", bytes_to_js(&chunk))?;
	Ok(event.into())
}

fn serverless_stream_end_event(
	error: Option<rivetkit_core::serverless::ServerlessStreamError>,
) -> Result<JsValue> {
	let event = object();
	set_anyhow(&event, "kind", JsValue::from_str("end"))?;
	if let Some(error) = error {
		let error_object = object();
		set_anyhow(&error_object, "group", JsValue::from_str(&error.group))?;
		set_anyhow(&error_object, "code", JsValue::from_str(&error.code))?;
		set_anyhow(&error_object, "message", JsValue::from_str(&error.message))?;
		set_anyhow(&event, "error", error_object.into())?;
	}
	Ok(event.into())
}

async fn call_serverless_stream_callback(callback: &Function, event: JsValue) -> Result<()> {
	let value = callback
		.call2(&JsValue::UNDEFINED, &JsValue::NULL, &event)
		.map_err(js_value_to_anyhow)?;
	if value.is_instance_of::<Promise>() {
		JsFuture::from(Promise::unchecked_from_js(value))
			.await
			.map_err(js_value_to_anyhow)?;
	}
	Ok(())
}

async fn start_wasm_serverless_request(
	runtime: CoreServerlessRuntime,
	req: ServerlessRequest,
	on_stream_event: Function,
) -> Result<JsValue, JsValue> {
	let (head_tx, head_rx) = oneshot::channel::<Result<JsValue, String>>();
	let (done_tx, done_rx) = oneshot::channel::<()>();
	let local = tokio::task::LocalSet::new();
	local.spawn_local(async move {
		let response = runtime.handle_request(req).await;
		match serverless_response_head_to_js(response.status, response.headers) {
			Ok(head) => {
				if head_tx.send(Ok(head)).is_err() {
					let _ = done_tx.send(());
					return;
				}
			}
			Err(error) => {
				let _ = head_tx.send(Err(format!("{error:#}")));
				let _ = done_tx.send(());
				return;
			}
		}

		let mut body = response.body;
		let mut sent_end = false;
		while let Some(chunk) = body.recv().await {
			let event = match chunk {
				Ok(chunk) => serverless_stream_chunk_event(chunk),
				Err(error) => {
					sent_end = true;
					serverless_stream_end_event(Some(error))
				}
			};
			match event {
				Ok(event) => {
					if let Err(error) =
						call_serverless_stream_callback(&on_stream_event, event).await
					{
						console_error(&format!(
							"wasm serverless stream callback failed: {error:#}"
						));
						break;
					}
				}
				Err(error) => {
					console_error(&format!(
						"wasm serverless stream event encode failed: {error:#}"
					));
					break;
				}
			}
			if sent_end {
				break;
			}
		}

		if !sent_end {
			match serverless_stream_end_event(None) {
				Ok(event) => {
					if let Err(error) =
						call_serverless_stream_callback(&on_stream_event, event).await
					{
						console_error(&format!(
							"wasm serverless stream end callback failed: {error:#}"
						));
					}
				}
				Err(error) => {
					console_error(&format!(
						"wasm serverless stream end event encode failed: {error:#}"
					));
				}
			}
		}

		let _ = done_tx.send(());
	});
	spawn_local(async move {
		local
			.run_until(async {
				let _ = done_rx.await;
			})
			.await;
	});

	match head_rx.await {
		Ok(Ok(head)) => Ok(head),
		Ok(Err(error)) => Err(js_error(&error)),
		Err(_) => Err(js_error("serverless request driver dropped response head")),
	}
}

fn list_opts_from_js(value: JsValue) -> Result<ListOpts, JsValue> {
	if value.is_null() || value.is_undefined() {
		return Ok(ListOpts::default());
	}
	let reverse = js_bool_property(&value, "reverse").map_err(anyhow_to_js_error)?;
	let limit = js_number_property(&value, "limit").map_err(anyhow_to_js_error)?;
	Ok(ListOpts {
		reverse: reverse.unwrap_or(false),
		limit: limit.map(|value| value.max(0.0).trunc() as u32),
	})
}

fn bytes_array_from_js(values: Array) -> Vec<Vec<u8>> {
	(0..values.length())
		.map(|index| js_to_bytes(values.get(index)))
		.collect()
}

fn kv_entries_from_js(entries: Array) -> Result<Vec<(Vec<u8>, Vec<u8>)>, JsValue> {
	let mut decoded = Vec::with_capacity(entries.length() as usize);
	for index in 0..entries.length() {
		let entry = entries.get(index);
		let key = js_bytes_property(&entry, "key")
			.map_err(anyhow_to_js_error)?
			.ok_or_else(|| js_error("kv entry missing key"))?;
		let value = js_bytes_property(&entry, "value")
			.map_err(anyhow_to_js_error)?
			.ok_or_else(|| js_error("kv entry missing value"))?;
		decoded.push((key, value));
	}
	Ok(decoded)
}

fn kv_entries_to_js(entries: Vec<(Vec<u8>, Vec<u8>)>) -> JsValue {
	let array = Array::new();
	for (key, value) in entries {
		let entry = object();
		set(&entry, "key", bytes_to_js(&key)).unwrap_throw();
		set(&entry, "value", bytes_to_js(&value)).unwrap_throw();
		array.push(&entry.into());
	}
	array.into()
}

fn action_callback(actions: &JsValue, name: &str) -> Option<Function> {
	function_property(actions, name)
}

async fn call_callback(callback: &Function, payload: &JsValue) -> Result<JsValue> {
	let value = callback
		.call2(&JsValue::UNDEFINED, &JsValue::NULL, payload)
		.map_err(js_value_to_anyhow)?;
	if value.is_instance_of::<Promise>() {
		JsFuture::from(Promise::unchecked_from_js(value))
			.await
			.map_err(js_value_to_anyhow)
	} else {
		Ok(value)
	}
}

async fn call_callback_bytes(callback: &Function, payload: &JsValue) -> Result<Vec<u8>> {
	call_callback(callback, payload).await.map(js_to_bytes)
}

fn state_delta_payload_from_js(value: JsValue) -> Result<Vec<StateDelta>> {
	if value.is_null() || value.is_undefined() {
		return Ok(Vec::new());
	}

	let payload: WasmStateDeltaPayload = serde_wasm_bindgen::from_value(value)
		.map_err(|error| anyhow!("decode state delta payload: {error}"))?;
	let mut deltas = Vec::new();
	if let Some(state) = payload.state {
		deltas.push(StateDelta::ActorState(state));
	}
	for entry in payload.conn_hibernation {
		deltas.push(StateDelta::ConnHibernation {
			conn: entry.conn_id,
			bytes: entry.bytes,
		});
	}
	for conn_id in payload.conn_hibernation_removed {
		deltas.push(StateDelta::ConnHibernationRemoved(conn_id));
	}
	Ok(deltas)
}

fn bind_params_from_js(value: JsValue) -> Result<Option<Vec<BindParam>>, JsValue> {
	if value.is_null() || value.is_undefined() {
		return Ok(None);
	}

	let params: Vec<WasmBindParam> = serde_wasm_bindgen::from_value(value)?;
	params
		.into_iter()
		.map(bind_param_from_wasm)
		.collect::<Result<Vec<_>, _>>()
		.map(Some)
}

fn bind_param_from_wasm(param: WasmBindParam) -> Result<BindParam, JsValue> {
	match param.kind.as_str() {
		"null" => Ok(BindParam::Null),
		"int" => Ok(BindParam::Integer(param.int_value.unwrap_or(0.0) as i64)),
		"float" => Ok(BindParam::Float(param.float_value.unwrap_or(0.0))),
		"text" => Ok(BindParam::Text(param.text_value.unwrap_or_default())),
		"blob" => Ok(BindParam::Blob(param.blob_value.unwrap_or_default())),
		kind => Err(js_error(&format!(
			"unsupported bind parameter kind: {kind}"
		))),
	}
}

fn query_result_to_js(result: rivetkit_core::QueryResult) -> JsValue {
	let object = object();
	set(
		&object,
		"columns",
		strings_to_js_array(result.columns).into(),
	)
	.unwrap_throw();
	set(&object, "rows", rows_to_js_array(result.rows).into()).unwrap_throw();
	object.into()
}

fn execute_result_to_js(result: rivetkit_core::ExecuteResult) -> JsValue {
	let object = object();
	set(
		&object,
		"columns",
		strings_to_js_array(result.columns).into(),
	)
	.unwrap_throw();
	set(&object, "rows", rows_to_js_array(result.rows).into()).unwrap_throw();
	set(&object, "changes", JsValue::from_f64(result.changes as f64)).unwrap_throw();
	if let Some(last_insert_row_id) = result.last_insert_row_id {
		set(
			&object,
			"lastInsertRowId",
			JsValue::from_f64(last_insert_row_id as f64),
		)
		.unwrap_throw();
	}
	object.into()
}

fn strings_to_js_array(values: Vec<String>) -> Array {
	let array = Array::new();
	for value in values {
		array.push(&JsValue::from_str(&value));
	}
	array
}

fn rows_to_js_array(rows: Vec<Vec<ColumnValue>>) -> Array {
	let array = Array::new();
	for row in rows {
		let row_array = Array::new();
		for value in row {
			row_array.push(&column_value_to_js(value));
		}
		array.push(&row_array);
	}
	array
}

fn column_value_to_js(value: ColumnValue) -> JsValue {
	match value {
		ColumnValue::Null => JsValue::NULL,
		ColumnValue::Integer(value) => JsValue::from_f64(value as f64),
		ColumnValue::Float(value) => JsValue::from_f64(value),
		ColumnValue::Text(value) => JsValue::from_str(&value),
		ColumnValue::Blob(value) => bytes_to_js(&value),
	}
}

fn new_js_class(name: &str) -> Result<JsValue, JsValue> {
	let constructor = Reflect::get(&js_sys::global(), &JsValue::from_str(name))?
		.dyn_into::<Function>()
		.map_err(|_| js_error(&format!("{name} is not a constructor")))?;
	Reflect::construct(&constructor, &Array::new())
}

fn call_js_method0(target: &JsValue, name: &str) -> Result<JsValue, JsValue> {
	let method = Reflect::get(target, &JsValue::from_str(name))?
		.dyn_into::<Function>()
		.map_err(|_| js_error(&format!("{name} is not a function")))?;
	method.call0(target)
}

fn js_value_to_anyhow(value: JsValue) -> anyhow::Error {
	if let Some(error) = value.dyn_ref::<js_sys::Error>() {
		let message = error
			.message()
			.as_string()
			.unwrap_or_else(|| "JavaScript error".to_owned());
		return parse_bridge_rivet_error(&message).unwrap_or_else(|| anyhow!(message));
	}
	if let Some(message) = value.as_string() {
		return parse_bridge_rivet_error(&message).unwrap_or_else(|| anyhow!(message));
	}
	anyhow!("JavaScript callback failed")
}

fn parse_bridge_rivet_error(reason: &str) -> Option<anyhow::Error> {
	let prefix_index = reason.find(BRIDGE_RIVET_ERROR_PREFIX)?;
	let payload = &reason[prefix_index + BRIDGE_RIVET_ERROR_PREFIX.len()..];
	let payload: BridgeRivetErrorPayload = match serde_json::from_str(payload) {
		Ok(payload) => payload,
		Err(parse_err) => {
			console_error(&format!("malformed BridgeRivetErrorPayload: {parse_err}"));
			return None;
		}
	};
	let meta = payload
		.metadata
		.as_ref()
		.and_then(|metadata| serde_json::value::to_raw_value(metadata).ok());
	let message = payload.message;
	let error = anyhow::Error::new(RivetTransportError {
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
	}))
}

fn console_error(message: &str) {
	let global = js_sys::global();
	let Ok(console) = Reflect::get(&global, &JsValue::from_str("console")) else {
		return;
	};
	let Ok(error_fn) = Reflect::get(&console, &JsValue::from_str("error")) else {
		return;
	};
	let Ok(error_fn) = error_fn.dyn_into::<Function>() else {
		return;
	};
	let _ = error_fn.call1(&console, &JsValue::from_str(message));
}

fn js_error(message: &str) -> JsValue {
	js_sys::Error::new(message).into()
}

fn registry_not_registering_error() -> JsValue {
	anyhow_to_js_error(
		WasmInvalidState {
			state: "core registry".to_owned(),
			reason: "already serving or shut down".to_owned(),
		}
		.build(),
	)
}

fn registry_wrong_mode_error() -> JsValue {
	anyhow_to_js_error(
		WasmInvalidState {
			state: "core registry".to_owned(),
			reason: "mode conflict: another run mode is already active".to_owned(),
		}
		.build(),
	)
}

fn registry_shut_down_error() -> JsValue {
	anyhow_to_js_error(
		WasmInvalidState {
			state: "core registry".to_owned(),
			reason: "shut down".to_owned(),
		}
		.build(),
	)
}

fn anyhow_to_js_error(error: anyhow::Error) -> JsValue {
	let payload = anyhow_to_bridge_rivet_error_payload(error);
	js_sys::Error::new(&format!("{BRIDGE_RIVET_ERROR_PREFIX}{payload}")).into()
}

fn anyhow_to_bridge_rivet_error_payload(error: anyhow::Error) -> serde_json::Value {
	let bridge_context = error
		.chain()
		.find_map(|cause| cause.downcast_ref::<BridgeRivetErrorContext>());
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
		"public": public_,
		"statusCode": status_code,
		"actor": error.actor(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use rivet_error::{MacroMarker, RivetErrorSchema};

	fn test_serve_config(engine_binary_path: Option<String>) -> WasmServeConfig {
		WasmServeConfig {
			version: 1,
			endpoint: "http://127.0.0.1:6420".to_owned(),
			token: None,
			namespace: "default".to_owned(),
			pool_name: "default".to_owned(),
			engine_binary_path,
			handle_inspector_http_in_runtime: None,
			inspector_test_token: None,
			serverless_base_path: None,
			serverless_package_version: "0.0.0-test".to_owned(),
			serverless_client_endpoint: None,
			serverless_client_namespace: None,
			serverless_client_token: None,
			serverless_validate_endpoint: true,
			serverless_max_start_payload_bytes: 1024,
		}
	}

	fn bridge_reason(group: &str, code: &str, message: &str) -> String {
		let payload = serde_json::json!({
			"group": group,
			"code": code,
			"message": message,
			"metadata": { "case": "schema-cache" },
			"public": true,
			"statusCode": 418,
		});
		format!("{BRIDGE_RIVET_ERROR_PREFIX}{payload}")
	}

	static AUTH_FORBIDDEN_SCHEMA: RivetErrorSchema = RivetErrorSchema {
		group: "auth",
		code: "forbidden",
		default_message: "Forbidden",
		meta_type: None,
		_macro_marker: MacroMarker { _private: () },
	};

	fn transport_error(error: &anyhow::Error) -> &RivetTransportError {
		error
			.chain()
			.find_map(|cause| cause.downcast_ref::<RivetTransportError>())
			.expect("bridge error should carry RivetTransportError")
	}

	fn transport_message(error: &anyhow::Error) -> String {
		transport_error(error).message().to_owned()
	}

	#[test]
	fn parse_bridge_rivet_error_uses_dynamic_error_kind() {
		let reason = bridge_reason(
			"wasm_schema_cache_test",
			"same_payload",
			"same payload message",
		);
		let first = parse_bridge_rivet_error(&reason).expect("bridge error should decode");

		assert_eq!(first.to_string(), "same payload message");
		assert!(transport_error(&first).schema().is_none());
		assert_eq!(transport_error(&first).group(), "wasm_schema_cache_test");
		assert_eq!(transport_error(&first).code(), "same_payload");

		for _ in 0..100 {
			let error = parse_bridge_rivet_error(&reason).expect("bridge error should decode");
			assert!(transport_error(&error).schema().is_none());
		}

		let different_message = bridge_reason(
			"wasm_schema_cache_test",
			"same_payload",
			"different default message",
		);
		let error =
			parse_bridge_rivet_error(&different_message).expect("bridge error should decode");

		assert_eq!(transport_message(&error), "different default message");

		let different_code = bridge_reason(
			"wasm_schema_cache_test",
			"different_code",
			"different code message",
		);
		let error = parse_bridge_rivet_error(&different_code).expect("bridge error should decode");

		assert_eq!(transport_error(&error).code(), "different_code");
	}

	#[test]
	fn wasm_bridge_payload_promotes_known_core_error_status() {
		let payload =
			anyhow_to_bridge_rivet_error_payload(anyhow::Error::new(RivetTransportError {
				kind: RivetErrorKind::Static(&AUTH_FORBIDDEN_SCHEMA),
				meta: None,
				message: None,
				actor: None,
			}));

		assert_eq!(
			payload.get("group").and_then(|value| value.as_str()),
			Some("auth")
		);
		assert_eq!(
			payload.get("code").and_then(|value| value.as_str()),
			Some("forbidden")
		);
		assert_eq!(
			payload.get("public").and_then(|value| value.as_bool()),
			Some(true)
		);
		assert_eq!(
			payload.get("statusCode").and_then(|value| value.as_u64()),
			Some(403)
		);
	}

	#[cfg(target_arch = "wasm32")]
	#[test]
	fn websocket_callback_regions_are_removed_after_end() {
		let context = WasmActorContext::from_core(
			rivetkit_core::ActorContext::new(
				"websocket-region-test",
				"websocket-region-test",
				Vec::new(),
				"local",
			),
			WasmCallbacks::new(Object::new().into()),
		);
		let mut previous_id = 0;

		for _ in 0..1000 {
			let region_id = context.begin_websocket_callback();
			assert!(region_id > previous_id);
			assert_eq!(context.websocket_callback_regions.borrow().len(), 1);

			context.end_websocket_callback(region_id);
			assert!(context.websocket_callback_regions.borrow().is_empty());

			previous_id = region_id;
		}
	}

	#[cfg(target_arch = "wasm32")]
	#[test]
	fn websocket_callback_region_ids_wrap_without_collision() {
		let context = WasmActorContext::from_core(
			rivetkit_core::ActorContext::new(
				"websocket-region-wrap-test",
				"websocket-region-wrap-test",
				Vec::new(),
				"local",
			),
			WasmCallbacks::new(Object::new().into()),
		);

		let first_id = context.begin_websocket_callback();
		context.next_websocket_callback_region_id.set(u32::MAX - 1);
		let max_id = context.begin_websocket_callback();
		let wrapped_id = context.begin_websocket_callback();

		assert_eq!(first_id, 1);
		assert_eq!(max_id, u32::MAX);
		assert_eq!(wrapped_id, 2);
		assert_eq!(context.websocket_callback_regions.borrow().len(), 3);
		assert!(
			context
				.websocket_callback_regions
				.borrow()
				.contains_key(&first_id)
		);
		assert!(
			context
				.websocket_callback_regions
				.borrow()
				.contains_key(&max_id)
		);
		assert!(
			context
				.websocket_callback_regions
				.borrow()
				.contains_key(&wrapped_id)
		);

		context.end_websocket_callback(first_id);
		context.end_websocket_callback(max_id);
		context.end_websocket_callback(wrapped_id);
		assert!(context.websocket_callback_regions.borrow().is_empty());
	}

	#[test]
	fn engine_binary_path_error_is_typed_with_field_metadata() {
		let error = validate_wasm_serve_config(&test_serve_config(Some(
			"/usr/local/bin/rivet-engine".to_owned(),
		)))
		.expect_err("engine_binary_path should be rejected");
		let error = RivetTransportError::extract(&error);

		assert_eq!(error.group(), "wasm");
		assert_eq!(error.code(), "invalid_config");
		assert_eq!(error.message(), "Invalid wasm configuration.");
		assert_eq!(
			error
				.metadata()
				.as_ref()
				.and_then(|metadata| metadata.get("field"))
				.and_then(|field| field.as_str()),
			Some("engine_binary_path")
		);
	}

	#[cfg(target_arch = "wasm32")]
	#[test]
	fn serve_rejects_engine_binary_path_before_core_setup() {
		use std::future::Future;
		use std::pin::pin;
		use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

		fn raw_waker() -> RawWaker {
			fn clone(_: *const ()) -> RawWaker {
				raw_waker()
			}
			fn wake(_: *const ()) {}
			fn wake_by_ref(_: *const ()) {}
			fn drop(_: *const ()) {}
			RawWaker::new(
				std::ptr::null(),
				&RawWakerVTable::new(clone, wake, wake_by_ref, drop),
			)
		}

		let registry = WasmCoreRegistry::new();
		let config = serde_wasm_bindgen::to_value(&test_serve_config(Some(
			"/usr/local/bin/rivet-engine".to_owned(),
		)))
		.expect("serve config should encode");
		let future = registry.serve(config);
		let mut future = pin!(future);
		let waker = unsafe { Waker::from_raw(raw_waker()) };
		let mut cx = Context::from_waker(&waker);

		match Future::poll(future.as_mut(), &mut cx) {
			Poll::Ready(Err(error)) => {
				let error = js_value_to_anyhow(error);
				let error = RivetTransportError::extract(&error);
				assert_eq!(error.group(), "wasm");
				assert_eq!(error.code(), "invalid_config");
				assert_eq!(
					error
						.metadata()
						.as_ref()
						.and_then(|metadata| metadata.get("field"))
						.and_then(|field| field.as_str()),
					Some("engine_binary_path")
				);
			}
			Poll::Ready(Ok(())) => panic!("serve should reject engine_binary_path"),
			Poll::Pending => panic!("serve should reject before awaiting core setup"),
		}
	}
}
