// State management contract:
// docs-internal/engine/rivetkit-core-state-management.md
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryFrom;
use std::future::Future;
use std::future::pending;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Weak};

use anyhow::Error;
use napi::bindgen_prelude::{Buffer, Promise};
use napi::threadsafe_function::{
	ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi::{Env, JsFunction, JsObject, Ref};
use napi_derive::napi;
use parking_lot::Mutex;
use rivetkit_core::types::ActorKeySegment;
use rivetkit_core::{
	ActorContext as CoreActorContext, ActorInvocationSpanContext, ActorInvocationTraceContext,
	ActorWorkKind, ConnHandle as CoreConnHandle, KeepAwakeRegion, Request as CoreRequest,
	RequestSaveOpts, StateDelta, WebSocketCallbackRegion, WorkflowKvWrite,
};
use scc::HashMap as SccHashMap;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken as CoreCancellationToken;

use crate::actor_factory::BridgeRivetErrorContext;
use crate::connection::ConnHandle;
use crate::database::{JsActorStateTransaction, JsNativeDatabase, transaction_timeout};
use crate::kv::Kv;
use crate::queue::Queue;
use crate::schedule::Schedule;
use crate::{NapiInvalidArgument, NapiInvalidState, napi_anyhow_error};

type AbortSignalTsfn = ThreadsafeFunction<(), ErrorStrategy::CalleeHandled>;
type DisconnectPredicateTsfn =
	ThreadsafeFunction<DisconnectPredicatePayload, ErrorStrategy::CalleeHandled>;
type RunRestartHook = Arc<dyn Fn(Option<(i64, u64)>) -> anyhow::Result<()> + Send + Sync + 'static>;
pub(crate) type RegisteredTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

static ACTOR_CONTEXT_SHARED: LazyLock<SccHashMap<String, Weak<ActorContextShared>>> =
	LazyLock::new(SccHashMap::new);

/// N-API wrapper around `rivetkit-core::ActorContext`.
#[derive(Clone)]
#[napi]
pub struct ActorContext {
	inner: CoreActorContext,
	shared: Arc<ActorContextShared>,
}

#[derive(Default)]
struct ActorContextShared {
	// Runtime slots are touched from synchronous N-API methods and TSF callback
	// paths; locks stay short and are never held across awaits.
	abort_token: Mutex<Option<CoreCancellationToken>>,
	run_restart: Mutex<Option<RunRestartHook>>,
	task_sender: Mutex<Option<UnboundedSender<RegisteredTask>>>,
	runtime_state: Mutex<Option<Ref<()>>>,
	end_reason: Mutex<Option<EndReason>>,
	keep_awake_regions: Mutex<BTreeMap<u32, KeepAwakeRegion>>,
	websocket_callback_regions: Mutex<BTreeMap<u32, WebSocketCallbackRegion>>,
	next_keep_awake_region_id: AtomicU32,
	next_websocket_callback_region_id: AtomicU32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum EndReason {
	Sleep,
	Destroy,
}

#[napi(object)]
pub struct JsActorKeySegment {
	pub kind: String,
	pub string_value: Option<String>,
	pub number_value: Option<f64>,
}

/// Active actor invocation correlation exposed to the TypeScript runtime adapter.
#[napi(object)]
pub struct JsActorInvocationTraceContext {
	pub ray_id: String,
	pub span: Option<JsActorInvocationSpanContext>,
}

/// W3C span context of the current invocation span, present only when tracing is active.
#[napi(object)]
pub struct JsActorInvocationSpanContext {
	pub trace_id: String,
	pub span_id: String,
	pub trace_flags: u8,
	pub traceparent: String,
	pub tracestate: Option<String>,
}

impl From<ActorInvocationTraceContext> for JsActorInvocationTraceContext {
	fn from(value: ActorInvocationTraceContext) -> Self {
		Self {
			ray_id: value.ray_id,
			span: value.span.map(JsActorInvocationSpanContext::from),
		}
	}
}

impl From<ActorInvocationSpanContext> for JsActorInvocationSpanContext {
	fn from(value: ActorInvocationSpanContext) -> Self {
		Self {
			trace_id: value.trace_id,
			span_id: value.span_id,
			trace_flags: value.trace_flags,
			traceparent: value.traceparent,
			tracestate: value.tracestate,
		}
	}
}

#[napi(object)]
pub struct JsHttpRequest {
	pub method: String,
	pub uri: String,
	pub headers: Option<std::collections::HashMap<String, String>>,
	pub body: Option<Buffer>,
}

#[napi(object)]
pub struct StateDeltaConnHibernationEntry {
	pub conn_id: String,
	pub bytes: Buffer,
}

#[napi(object)]
pub struct StateDeltaPayload {
	pub state: Option<Buffer>,
	pub conn_hibernation: Vec<StateDeltaConnHibernationEntry>,
	pub conn_hibernation_removed: Vec<String>,
}

#[napi(object)]
pub struct WorkflowKvWritePayload {
	pub key: Buffer,
	pub value: Buffer,
}

#[napi(object)]
pub struct JsRequestSaveOpts {
	pub immediate: Option<bool>,
	pub max_wait_ms: Option<u32>,
}

#[napi]
pub fn decode_inspector_request(bytes: Buffer, advertised_version: u32) -> napi::Result<Buffer> {
	let advertised_version = u16::try_from(advertised_version)
		.map_err(|_| inspector_version_error("advertisedVersion"))?;
	rivetkit_core::inspector::decode_request_payload(bytes.as_ref(), advertised_version)
		.map(Buffer::from)
		.map_err(napi_anyhow_error)
}

#[napi]
pub fn encode_inspector_response(bytes: Buffer, target_version: u32) -> napi::Result<Buffer> {
	let target_version =
		u16::try_from(target_version).map_err(|_| inspector_version_error("targetVersion"))?;
	rivetkit_core::inspector::encode_response_payload(bytes.as_ref(), target_version)
		.map(Buffer::from)
		.map_err(napi_anyhow_error)
}

#[napi(object)]
pub struct JsInspectorSnapshot {
	pub state_revision: i64,
	pub connections_revision: i64,
	pub queue_revision: i64,
	pub active_connections: u32,
	pub queue_size: u32,
	pub connected_clients: u32,
}

#[napi(object)]
pub struct JsActorRuntimeSocketEndpointInfo {
	pub path: String,
}

#[derive(Clone)]
struct DisconnectPredicatePayload {
	conn: CoreConnHandle,
}

impl ActorContext {
	pub(crate) fn new(inner: CoreActorContext) -> Self {
		let actor_id = inner.actor_id().to_owned();
		let shared = actor_context_shared(&actor_id);
		tracing::debug!(
			class = "ActorContext",
			%actor_id,
			shared_strong_count = Arc::strong_count(&shared),
			"constructed napi class"
		);
		Self { inner, shared }
	}

	#[allow(dead_code)]
	pub(crate) fn inner(&self) -> &CoreActorContext {
		&self.inner
	}

	pub(crate) fn attach_napi_abort_token(&self, token: CoreCancellationToken) {
		tracing::debug!(
			actor_id = %self.inner.actor_id(),
			"attached napi abort cancellation token"
		);
		self.shared.set_abort_token(token);
	}

	pub(crate) fn reset_runtime_shared_state(&self) {
		tracing::debug!(
			actor_id = %self.inner.actor_id(),
			"reset actor context shared runtime state"
		);
		self.shared.reset_runtime_state();
	}

	pub(crate) fn attach_run_restart<F>(&self, restart: F)
	where
		F: Fn(Option<(i64, u64)>) -> anyhow::Result<()> + Send + Sync + 'static,
	{
		self.shared.set_run_restart(Arc::new(restart));
	}

	pub(crate) fn attach_task_sender(&self, sender: UnboundedSender<RegisteredTask>) {
		self.shared.set_task_sender(sender);
	}

	#[allow(dead_code)]
	pub(crate) fn set_end_reason(&self, reason: EndReason) {
		self.shared.set_end_reason(reason);
	}

	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn take_end_reason(&self) -> Option<EndReason> {
		self.shared.take_end_reason()
	}

	pub(crate) fn has_end_reason(&self) -> bool {
		self.shared.has_end_reason()
	}

	pub(crate) fn set_state_initial(&self, state: Vec<u8>) -> anyhow::Result<()> {
		self.inner.set_state_initial(state);
		Ok(())
	}

	pub(crate) async fn mark_has_initialized_and_flush(&self) -> anyhow::Result<()> {
		self.inner.set_has_initialized(true);
		self.inner
			.save_state(vec![StateDelta::ActorState(self.inner.state())])
			.await
	}

	pub(crate) fn restore_hibernatable_conn(
		&self,
		conn: CoreConnHandle,
		bytes: Vec<u8>,
	) -> anyhow::Result<()> {
		conn.set_state_initial(bytes);
		Ok(())
	}

	pub(crate) fn set_conn_state_initial(
		&self,
		conn: &CoreConnHandle,
		bytes: Vec<u8>,
	) -> anyhow::Result<()> {
		conn.set_state_initial(bytes);
		Ok(())
	}

	#[allow(dead_code)]
	pub(crate) fn has_conn_changes(&self) -> bool {
		self.inner.conns().any(|conn| conn.is_hibernatable())
	}
}

#[napi]
impl ActorContext {
	#[napi]
	pub fn state(&self) -> Buffer {
		Buffer::from(self.inner.state())
	}

	#[napi]
	pub fn begin_on_state_change(&self) {
		self.inner.on_state_change_started();
	}

	#[napi]
	pub fn end_on_state_change(&self) {
		self.inner.on_state_change_finished();
	}

	#[napi]
	// The NAPI bridge intentionally keeps the deprecated KV surface available for
	// TypeScript compatibility while steering new code to SQLite or actor state.
	#[allow(deprecated)]
	pub fn kv(&self) -> Kv {
		Kv::new(self.inner.clone())
	}

	#[napi]
	pub fn sql(&self) -> JsNativeDatabase {
		JsNativeDatabase::new(
			self.inner.invocation_sql(),
			Some(self.inner.actor_id().to_owned()),
		)
	}

	#[napi]
	pub fn same_actor_instance(&self, other: &ActorContext) -> bool {
		self.inner.is_same_instance(&other.inner)
	}

	#[napi]
	pub fn invocation_trace_context(&self) -> Option<JsActorInvocationTraceContext> {
		self.inner.invocation_trace_context().map(Into::into)
	}

	#[napi]
	pub async fn provision_actor_runtime_socket(
		&self,
	) -> napi::Result<JsActorRuntimeSocketEndpointInfo> {
		let info = self
			.inner
			.provision_actor_runtime_socket()
			.await
			.map_err(napi_anyhow_error)?;
		Ok(JsActorRuntimeSocketEndpointInfo { path: info.path })
	}

	#[napi]
	pub fn schedule(&self) -> Schedule {
		Schedule::new(self.inner.clone())
	}

	#[napi]
	pub fn queue(&self) -> Queue {
		Queue::new(self.inner.clone())
	}

	#[napi]
	pub fn set_alarm(&self, timestamp_ms: Option<i64>) -> napi::Result<()> {
		self.inner
			.set_alarm(timestamp_ms)
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn request_save(&self, opts: Option<JsRequestSaveOpts>) {
		let opts = opts.unwrap_or(JsRequestSaveOpts {
			immediate: None,
			max_wait_ms: None,
		});
		self.inner.request_save(RequestSaveOpts {
			immediate: opts.immediate.unwrap_or(false),
			max_wait_ms: opts.max_wait_ms,
		});
	}

	#[napi]
	pub async fn request_save_and_wait(&self, opts: Option<JsRequestSaveOpts>) -> napi::Result<()> {
		let opts = opts.unwrap_or(JsRequestSaveOpts {
			immediate: None,
			max_wait_ms: None,
		});
		self.inner
			.request_save_and_wait(RequestSaveOpts {
				immediate: opts.immediate.unwrap_or(false),
				max_wait_ms: opts.max_wait_ms,
			})
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn decode_inspector_request(
		&self,
		bytes: Buffer,
		advertised_version: u32,
	) -> napi::Result<Buffer> {
		decode_inspector_request(bytes, advertised_version)
	}

	#[napi]
	pub fn encode_inspector_response(
		&self,
		bytes: Buffer,
		target_version: u32,
	) -> napi::Result<Buffer> {
		encode_inspector_response(bytes, target_version)
	}

	#[napi]
	pub fn inspector_snapshot(&self) -> JsInspectorSnapshot {
		let snapshot = self.inner.inspector_snapshot();

		JsInspectorSnapshot {
			state_revision: u64_to_i64(snapshot.state_revision),
			connections_revision: u64_to_i64(snapshot.connections_revision),
			queue_revision: u64_to_i64(snapshot.queue_revision),
			active_connections: snapshot.active_connections,
			queue_size: snapshot.queue_size,
			connected_clients: usize_to_u32(snapshot.connected_clients),
		}
	}

	#[napi(js_name = "verifyInspectorAuth")]
	pub async fn verify_inspector_auth_js(&self, bearer_token: Option<String>) -> napi::Result<()> {
		rivetkit_core::inspector::InspectorAuth::new()
			.verify(&self.inner, bearer_token.as_deref())
			.await
			.map_err(|error| {
				let message = error.to_string();
				napi_anyhow_error(error.context(BridgeRivetErrorContext {
					message: Some(message),
					public_: Some(true),
					status_code: Some(401),
					ray_id: None,
				}))
			})
	}

	#[napi]
	pub fn queue_hibernation_removal(&self, conn_id: String) {
		self.inner.queue_hibernation_removal(conn_id);
	}

	#[napi]
	pub fn has_pending_hibernation_changes(&self) -> bool {
		self.inner.has_pending_hibernation_changes()
	}

	#[napi]
	pub fn take_pending_hibernation_changes(&self) -> Vec<String> {
		self.inner.take_pending_hibernation_changes()
	}

	#[napi]
	pub fn dirty_hibernatable_conns(&self) -> Vec<ConnHandle> {
		self.inner
			.dirty_hibernatable_conns()
			.into_iter()
			.map(ConnHandle::new)
			.collect()
	}

	#[napi]
	pub async fn save_state(&self, payload: StateDeltaPayload) -> napi::Result<()> {
		self.inner
			.save_state(state_deltas_from_payload(payload))
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn begin_state_transaction(
		&self,
		timeout_ms: Option<f64>,
	) -> napi::Result<JsActorStateTransaction> {
		let timeout = timeout_ms.map(transaction_timeout).transpose()?;
		self.inner
			.begin_state_transaction(timeout)
			.await
			.map(JsActorStateTransaction::new)
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn save_state_and_workflow_batch(
		&self,
		writes: Vec<WorkflowKvWritePayload>,
	) -> napi::Result<()> {
		self.inner
			.save_state_and_workflow_batch(
				writes
					.into_iter()
					.map(|write| WorkflowKvWrite {
						key: write.key.to_vec(),
						value: write.value.to_vec(),
					})
					.collect(),
			)
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn actor_id(&self) -> String {
		self.inner.actor_id().to_owned()
	}

	#[napi]
	pub fn name(&self) -> String {
		self.inner.name().to_owned()
	}

	#[napi]
	pub fn key(&self) -> Vec<JsActorKeySegment> {
		self.inner
			.key()
			.iter()
			.map(|segment| match segment {
				ActorKeySegment::String(value) => JsActorKeySegment {
					kind: "string".to_owned(),
					string_value: Some(value.clone()),
					number_value: None,
				},
				ActorKeySegment::Number(value) => JsActorKeySegment {
					kind: "number".to_owned(),
					string_value: None,
					number_value: Some(*value),
				},
			})
			.collect()
	}

	#[napi]
	pub fn region(&self) -> String {
		self.inner.region().to_owned()
	}

	#[napi]
	pub fn sleep(&self) -> napi::Result<()> {
		self.inner.sleep().map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn destroy(&self) -> napi::Result<()> {
		self.inner.destroy().map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn destroy_requested(&self) -> bool {
		self.inner.is_destroy_requested()
	}

	#[napi]
	pub async fn wait_for_destroy_completion(&self) {
		self.inner.wait_for_destroy_completion_public().await;
	}

	#[napi]
	#[allow(deprecated)]
	pub fn set_prevent_sleep(&self, prevent_sleep: bool) {
		self.inner.set_prevent_sleep(prevent_sleep);
	}

	#[napi]
	#[allow(deprecated)]
	pub fn prevent_sleep(&self) -> bool {
		self.inner.prevent_sleep()
	}

	#[napi]
	pub fn aborted(&self) -> bool {
		self.inner.actor_aborted()
			|| self
				.shared
				.configured_abort_token()
				.is_some_and(|token| token.is_cancelled())
	}

	#[napi]
	pub fn run_handler_active(&self) -> bool {
		self.shared.run_restart_configured()
	}

	#[napi]
	pub fn restart_run_handler(&self) -> napi::Result<()> {
		self.shared.run_restart(None).map_err(napi_anyhow_error)
	}

	pub(crate) fn restart_run_handler_for_wake(
		&self,
		wake_at: i64,
		wake_revision: u64,
	) -> anyhow::Result<()> {
		self.shared.run_restart(Some((wake_at, wake_revision)))
	}

	#[napi]
	pub async fn set_run_wake_at(&self, timestamp_ms: Option<i64>) -> napi::Result<()> {
		self.inner
			.set_run_wake_at(timestamp_ms)
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn begin_keep_awake(&self) -> u32 {
		self.shared.begin_keep_awake(self.inner.keep_awake_region())
	}

	#[napi]
	pub fn end_keep_awake(&self, region_id: u32) {
		self.shared.end_keep_awake(region_id);
	}

	#[napi]
	pub fn keep_awake(&self, promise: Promise<serde_json::Value>) -> napi::Result<()> {
		self.inner.spawn_work(ActorWorkKind::KeepAwake, async move {
			if let Err(error) = promise.await {
				tracing::warn!(?error, "actor keep_awake promise rejected");
			}
		});
		Ok(())
	}

	#[napi]
	pub fn begin_websocket_callback(&self) -> u32 {
		self.shared
			.begin_websocket_callback(self.inner.websocket_callback_region())
	}

	#[napi]
	pub fn end_websocket_callback(&self, region_id: u32) {
		self.shared.end_websocket_callback(region_id);
	}

	#[napi(ts_return_type = "AbortSignal")]
	pub fn abort_signal(&self, env: Env) -> napi::Result<JsObject> {
		let (signal, abort) = create_abort_signal(env)?;
		let actor_token = self.inner.actor_abort_signal();
		let runtime_token = self.shared.configured_abort_token();
		let actor_id = self.inner.actor_id().to_owned();

		napi::bindgen_prelude::spawn(async move {
			let runtime_cancelled = async move {
				if let Some(token) = runtime_token {
					token.cancelled().await;
				} else {
					pending::<()>().await;
				}
			};
			tokio::select! {
				_ = actor_token.cancelled() => {}
				_ = runtime_cancelled => {}
			}
			tracing::debug!(
				kind = "abortSignal",
				payload_summary = %format!("actor_id={actor_id}"),
				"invoking napi TSF callback"
			);
			let status = abort.call(Ok(()), ThreadsafeFunctionCallMode::NonBlocking);
			tracing::debug!(kind = "abortSignal", ?status, "napi TSF callback returned");
			if status != napi::Status::Ok {
				tracing::warn!(actor_id, ?status, "failed to deliver abort signal");
			}
		});

		Ok(signal)
	}

	#[napi]
	pub fn conns(&self) -> Vec<ConnHandle> {
		self.inner.conns().map(ConnHandle::new).collect()
	}

	#[napi]
	pub async fn connect_conn(
		&self,
		params: Buffer,
		request: Option<JsHttpRequest>,
	) -> napi::Result<ConnHandle> {
		let request = request.map(js_http_request_to_core_request).transpose()?;
		let conn = self
			.inner
			.connect_conn_with_request(params.to_vec(), request, async {
				Ok::<Vec<u8>, Error>(Vec::new())
			})
			.await
			.map_err(napi_anyhow_error)?;
		Ok(ConnHandle::new(conn))
	}

	#[napi]
	pub async fn disconnect_conn(&self, id: String) -> napi::Result<()> {
		self.inner
			.disconnect_conn(id)
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi(ts_return_type = "Promise<void>")]
	pub fn disconnect_conns(
		&self,
		env: Env,
		predicate: JsFunction,
	) -> napi::Result<napi::JsObject> {
		let predicate = create_disconnect_predicate(&env, predicate)?;
		let ctx = self.inner.clone();

		env.execute_tokio_future(
			async move {
				let mut ids = BTreeSet::new();
				let conns = ctx.conns().collect::<Vec<_>>();

				for conn in conns {
					if call_disconnect_predicate(&predicate, conn.clone()).await? {
						ids.insert(conn.id().to_owned());
					}
				}

				ctx.disconnect_conns(move |conn| ids.contains(conn.id()))
					.await
					.map_err(napi_anyhow_error)?;
				Ok(())
			},
			|env, ()| env.get_undefined(),
		)
	}

	#[napi]
	pub fn broadcast(&self, name: String, args: Buffer) {
		self.inner.broadcast(&name, args.as_ref());
	}

	#[napi]
	pub fn wait_until(&self, promise: Promise<serde_json::Value>) -> napi::Result<()> {
		self.inner.wait_until(async move {
			if let Err(error) = promise.await {
				tracing::warn!(?error, "actor wait_until promise rejected");
			}
		});
		Ok(())
	}

	#[napi]
	pub async fn wait_for_tracked_shutdown_work(&self) -> bool {
		self.inner.wait_for_tracked_shutdown_work().await
	}

	#[napi]
	pub async fn wait_for_tracked_shutdown_work_unbounded(&self) {
		self.inner.wait_for_tracked_shutdown_work_unbounded().await;
	}

	#[napi]
	pub fn register_task(&self, promise: Promise<serde_json::Value>) -> napi::Result<()> {
		self.shared
			.register_task(Box::pin(async move {
				if let Err(error) = promise.await {
					tracing::warn!(?error, "actor keep_awake promise rejected");
				}
			}))
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub fn runtime_state(&self, env: Env) -> napi::Result<JsObject> {
		self.shared.runtime_state(env)
	}

	#[napi]
	pub fn clear_runtime_state(&self, env: Env) -> napi::Result<()> {
		self.shared.clear_runtime_state(env)
	}
}

impl Drop for ActorContext {
	fn drop(&mut self) {
		tracing::debug!(
			class = "ActorContext",
			actor_id = %self.inner.actor_id(),
			shared_strong_count = Arc::strong_count(&self.shared),
			"dropped napi class"
		);
	}
}

impl ActorContextShared {
	fn configured_abort_token(&self) -> Option<CoreCancellationToken> {
		self.abort_token.lock().clone()
	}

	fn set_abort_token(&self, token: CoreCancellationToken) {
		*self.abort_token.lock() = Some(token);
	}

	fn set_run_restart(&self, restart: RunRestartHook) {
		*self.run_restart.lock() = Some(restart);
	}

	fn set_task_sender(&self, sender: UnboundedSender<RegisteredTask>) {
		*self.task_sender.lock() = Some(sender);
	}

	fn register_task(&self, task: RegisteredTask) -> anyhow::Result<()> {
		let sender = self.task_sender.lock().clone().ok_or_else(|| {
			NapiInvalidState {
				state: "actor task registration".to_owned(),
				reason: "not configured".to_owned(),
			}
			.build()
		})?;
		sender.send(task).map_err(|_| {
			NapiInvalidState {
				state: "actor task registration".to_owned(),
				reason: "closed".to_owned(),
			}
			.build()
		})
	}

	fn run_restart(&self, wake: Option<(i64, u64)>) -> anyhow::Result<()> {
		let restart = self.run_restart.lock().clone().ok_or_else(|| {
			NapiInvalidState {
				state: "run handler restart".to_owned(),
				reason: "not configured".to_owned(),
			}
			.build()
		})?;
		restart(wake)
	}

	fn run_restart_configured(&self) -> bool {
		self.run_restart.lock().is_some()
	}

	fn runtime_state(&self, env: Env) -> napi::Result<JsObject> {
		let mut runtime_state = self.runtime_state.lock();
		if let Some(reference) = runtime_state.as_ref() {
			return env.get_reference_value(reference);
		}

		let reference = env.create_reference(env.create_object()?)?;
		let state = env.get_reference_value(&reference)?;
		*runtime_state = Some(reference);
		Ok(state)
	}

	fn clear_runtime_state(&self, env: Env) -> napi::Result<()> {
		let Some(mut reference) = self.runtime_state.lock().take() else {
			return Ok(());
		};

		reference.unref(env)?;
		Ok(())
	}

	#[allow(dead_code)]
	fn set_end_reason(&self, reason: EndReason) {
		*self.end_reason.lock() = Some(reason);
	}

	fn begin_websocket_callback(&self, region: WebSocketCallbackRegion) -> u32 {
		let id = self
			.next_websocket_callback_region_id
			.fetch_add(1, Ordering::SeqCst)
			.wrapping_add(1);
		self.websocket_callback_regions.lock().insert(id, region);
		id
	}

	fn begin_keep_awake(&self, region: KeepAwakeRegion) -> u32 {
		let mut regions = self.keep_awake_regions.lock();
		let Some(id) = self.allocate_keep_awake_region_id(&regions) else {
			tracing::error!("failed to begin keep-awake region: no region ids available");
			return 0;
		};
		regions.insert(id, region);
		id
	}

	fn end_keep_awake(&self, region_id: u32) {
		if region_id == 0 {
			return;
		}
		self.keep_awake_regions.lock().remove(&region_id);
	}

	fn allocate_keep_awake_region_id(
		&self,
		regions: &BTreeMap<u32, KeepAwakeRegion>,
	) -> Option<u32> {
		for _ in 0..=u32::MAX {
			let next = self
				.next_keep_awake_region_id
				.fetch_add(1, Ordering::SeqCst)
				.wrapping_add(1);
			if next != 0 && !regions.contains_key(&next) {
				return Some(next);
			}
		}
		None
	}

	fn end_websocket_callback(&self, region_id: u32) {
		self.websocket_callback_regions.lock().remove(&region_id);
	}

	#[cfg_attr(not(test), allow(dead_code))]
	fn take_end_reason(&self) -> Option<EndReason> {
		self.end_reason.lock().take()
	}

	fn has_end_reason(&self) -> bool {
		self.end_reason.lock().is_some()
	}

	fn reset_runtime_state(&self) {
		*self.abort_token.lock() = None;
		*self.run_restart.lock() = None;
		*self.task_sender.lock() = None;
		// napi Ref::unref requires an Env; this function runs on tokio workers
		// with no Env available. Dropping without unref panics a debug_assert in
		// napi-rs and silently leaks the napi reference slot in release. Forget
		// instead so debug matches release behavior. Leak is bounded to one
		// JsObject per actor wake cycle until the process exits.
		if let Some(old) = self.runtime_state.lock().take() {
			std::mem::forget(old);
		}
		*self.end_reason.lock() = None;
		*self.keep_awake_regions.lock() = BTreeMap::new();
		*self.websocket_callback_regions.lock() = BTreeMap::new();
		self.next_keep_awake_region_id.store(0, Ordering::SeqCst);
		self.next_websocket_callback_region_id
			.store(0, Ordering::SeqCst);
	}
}

impl Drop for ActorContextShared {
	fn drop(&mut self) {
		// Same Env-less drop problem as reset_runtime_state. See comment there.
		if let Some(old) = self.runtime_state.lock().take() {
			std::mem::forget(old);
		}
	}
}

fn actor_context_shared(actor_id: &str) -> Arc<ActorContextShared> {
	ACTOR_CONTEXT_SHARED.retain_sync(|_, shared| shared.strong_count() > 0);

	match ACTOR_CONTEXT_SHARED.entry_sync(actor_id.to_owned()) {
		scc::hash_map::Entry::Occupied(mut entry) => {
			if let Some(shared) = entry.get().upgrade() {
				tracing::debug!(
					%actor_id,
					outcome = "hit",
					strong_count = Arc::strong_count(&shared),
					"actor context shared-state cache lookup"
				);
				return shared;
			}

			let shared = Arc::new(ActorContextShared::default());
			*entry.get_mut() = Arc::downgrade(&shared);
			tracing::debug!(
				%actor_id,
				outcome = "stale",
				"actor context shared-state cache lookup"
			);
			shared
		}
		scc::hash_map::Entry::Vacant(entry) => {
			let shared = Arc::new(ActorContextShared::default());
			entry.insert_entry(Arc::downgrade(&shared));
			tracing::debug!(
				%actor_id,
				outcome = "miss",
				"actor context shared-state cache lookup"
			);
			shared
		}
	}
}

fn u64_to_i64(value: u64) -> i64 {
	value.min(i64::MAX as u64) as i64
}

fn usize_to_u32(value: usize) -> u32 {
	value.min(u32::MAX as usize) as u32
}

pub(crate) fn state_deltas_from_payload(payload: StateDeltaPayload) -> Vec<StateDelta> {
	let mut deltas = Vec::new();

	if let Some(state) = payload.state {
		deltas.push(StateDelta::ActorState(state.to_vec()));
	}

	deltas.extend(
		payload
			.conn_hibernation
			.into_iter()
			.map(|entry| StateDelta::ConnHibernation {
				conn: entry.conn_id,
				bytes: entry.bytes.to_vec(),
			}),
	);

	deltas.extend(
		payload
			.conn_hibernation_removed
			.into_iter()
			.map(StateDelta::ConnHibernationRemoved),
	);

	deltas
}

fn create_disconnect_predicate(
	env: &Env,
	callback: JsFunction,
) -> napi::Result<DisconnectPredicateTsfn> {
	let wrap_predicate: JsFunction =
		env.run_script("(callback => async payload => Boolean(await callback(payload)))")?;
	let wrapped = JsFunction::try_from(wrap_predicate.call(None, &[callback])?)?;

	wrapped.create_threadsafe_function(
		0,
		|ctx: ThreadSafeCallContext<DisconnectPredicatePayload>| {
			build_disconnect_predicate_payload(&ctx.env, ctx.value)
		},
	)
}

async fn call_disconnect_predicate(
	callback: &DisconnectPredicateTsfn,
	conn: CoreConnHandle,
) -> napi::Result<bool> {
	let payload_summary = format!("conn_id={}", conn.id());
	tracing::debug!(
		kind = "disconnectPredicate",
		payload_summary = %payload_summary,
		"invoking napi TSF callback"
	);
	let promise = callback
		.call_async::<Promise<bool>>(Ok(DisconnectPredicatePayload { conn }))
		.await
		.map_err(disconnect_predicate_error)?;

	promise.await.map_err(disconnect_predicate_error)
}

fn inspector_version_error(argument: &str) -> napi::Error {
	napi_anyhow_error(
		NapiInvalidArgument {
			argument: argument.to_owned(),
			reason: "exceeds u16".to_owned(),
		}
		.build(),
	)
}

fn disconnect_predicate_error(error: napi::Error) -> napi::Error {
	napi_anyhow_error(
		NapiInvalidState {
			state: "disconnect predicate".to_owned(),
			reason: error.to_string(),
		}
		.build(),
	)
}

fn build_disconnect_predicate_payload(
	env: &Env,
	payload: DisconnectPredicatePayload,
) -> napi::Result<Vec<napi::JsUnknown>> {
	let mut object = env.create_object()?;
	object.set("conn", ConnHandle::new(payload.conn))?;
	Ok(vec![object.into_unknown()])
}

fn create_abort_signal(env: Env) -> napi::Result<(JsObject, AbortSignalTsfn)> {
	let bridge: JsObject = env.run_script(
		"(() => { \
			const controller = new AbortController(); \
			return { signal: controller.signal, abort: () => controller.abort() }; \
		})()",
	)?;
	let signal = bridge.get_named_property::<JsObject>("signal")?;
	let abort = bridge.get_named_property::<JsFunction>("abort")?;
	let mut abort = abort.create_threadsafe_function(0, |_ctx: ThreadSafeCallContext<()>| {
		Ok(Vec::<napi::JsUnknown>::new())
	})?;
	abort.unref(&env)?;

	Ok((signal, abort))
}

fn js_http_request_to_core_request(request: JsHttpRequest) -> napi::Result<CoreRequest> {
	CoreRequest::from_parts(
		&request.method,
		&request.uri,
		request.headers.unwrap_or_default(),
		request.body.map(|body| body.to_vec()).unwrap_or_default(),
	)
	.map_err(napi_anyhow_error)
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../tests/actor_context.rs"]
mod tests;
