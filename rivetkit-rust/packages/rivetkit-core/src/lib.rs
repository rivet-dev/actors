#[cfg(all(feature = "native-runtime", feature = "wasm-runtime"))]
compile_error!(
	"`native-runtime` and `wasm-runtime` are mutually exclusive. Enable exactly one rivetkit-core runtime."
);

pub mod actor;
#[cfg(feature = "native-runtime")]
mod development_process;
#[cfg(feature = "native-runtime")]
pub mod engine_process;
pub mod error;
pub mod inspector;
pub mod inspector_bundle;
pub mod metrics_endpoint;
pub mod registry;
pub mod runtime;
pub(crate) mod serde_metrics;
pub mod serverless;
pub(crate) mod telemetry;
// Internal bridge types consumed by the NAPI and Wasm runtime adapters.
#[doc(hidden)]
pub use telemetry::{
	ActorInvocationSpanContext, ActorInvocationTelemetry, ActorInvocationTraceContext,
};
#[cfg(feature = "native-runtime")]
pub mod serverless_http;
#[cfg(feature = "native-runtime")]
mod services_process;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub(crate) mod time {
	use std::fmt;
	use std::future::Future;
	use std::time::Duration;

	#[cfg(target_arch = "wasm32")]
	use futures::FutureExt;
	#[cfg(target_arch = "wasm32")]
	use wasm_bindgen::{JsCast, JsValue};
	#[cfg(target_arch = "wasm32")]
	use wasm_bindgen_futures::JsFuture;

	#[cfg(not(target_arch = "wasm32"))]
	pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
	#[cfg(target_arch = "wasm32")]
	pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

	#[derive(Debug, Clone, Copy)]
	pub struct TimeoutError;

	impl fmt::Display for TimeoutError {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.write_str("operation timed out")
		}
	}

	impl std::error::Error for TimeoutError {}

	#[cfg(not(target_arch = "wasm32"))]
	pub fn tokio_deadline(deadline: Instant) -> tokio::time::Instant {
		deadline.into()
	}

	#[cfg(target_arch = "wasm32")]
	pub async fn sleep(duration: Duration) {
		let delay_ms = duration.as_millis().min(u32::MAX as u128) as f64;
		let promise = js_sys::Promise::new(&mut |resolve, _reject| {
			let global = js_sys::global();
			let set_timeout = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
				.ok()
				.and_then(|value| value.dyn_into::<js_sys::Function>().ok());

			if let Some(set_timeout) = set_timeout {
				let _ = set_timeout.call2(&global, &resolve, &JsValue::from_f64(delay_ms));
			} else {
				let _ = resolve.call0(&JsValue::UNDEFINED);
			}
		});

		let _ = JsFuture::from(promise).await;
	}

	#[cfg(not(target_arch = "wasm32"))]
	pub async fn sleep(duration: Duration) {
		tokio::time::sleep(duration).await;
	}

	#[cfg(not(target_arch = "wasm32"))]
	pub async fn sleep_until(deadline: Instant) {
		tokio::time::sleep_until(tokio_deadline(deadline)).await;
	}

	#[cfg(target_arch = "wasm32")]
	pub async fn sleep_until(deadline: Instant) {
		let remaining = deadline
			.checked_duration_since(Instant::now())
			.unwrap_or(Duration::ZERO);
		sleep(remaining).await;
	}

	#[cfg(not(target_arch = "wasm32"))]
	pub async fn timeout<F>(duration: Duration, future: F) -> Result<F::Output, TimeoutError>
	where
		F: Future,
	{
		tokio::time::timeout(duration, future)
			.await
			.map_err(|_| TimeoutError)
	}

	#[cfg(target_arch = "wasm32")]
	pub async fn timeout<F>(duration: Duration, future: F) -> Result<F::Output, TimeoutError>
	where
		F: Future,
	{
		futures::pin_mut!(future);
		let timer = sleep(duration);
		futures::pin_mut!(timer);

		futures::select! {
			result = future.fuse() => Ok(result),
			_ = timer.fuse() => Err(TimeoutError),
		}
	}
}
pub mod types;
pub mod websocket;
pub use actor::{kv, sqlite};

pub use actor::action::ActionDispatchError;
pub use actor::config::{
	ActionDefinition, ActorConfig, ActorConfigInput, ActorConfigOverrides, CanHibernateWebSocket,
	SqliteProfilingConfig, SqliteProfilingConfigInput,
};
pub use actor::connection::ConnHandle;
pub use actor::context::{
	ActorContext, ActorKv, ActorWorkRegion, KeepAwakeRegion, WebSocketCallbackRegion,
};
pub use actor::factory::{ActorEntryFn, ActorFactory};
pub use actor::lifecycle_hooks::{ActorEvents, ActorStart, Reply};
pub use actor::messages::{
	ActorEvent, ActorHttpResponse, QueueSendResult, QueueSendStatus, Request, Response,
	SerializeStateReason, StateDelta, StreamingResponse, WorkflowKvWrite,
};
pub use actor::queue::{
	CompletableQueueMessage, EnqueueAndWaitOpts, QueueMessage, QueueNextBatchOpts, QueueNextOpts,
	QueueTryNextBatchOpts, QueueTryNextOpts, QueueWaitOpts,
};
pub use actor::sqlite::{
	BindParam, ColumnValue, ExecResult, ExecuteResult, QueryResult, SqliteBackend,
	SqliteBatchStatement, SqliteDb, SqliteTransaction,
};
pub use actor::state::{ActorStateTransaction, RequestSaveOpts};
pub use actor::task::{
	ActionDispatchResult, ActorTask, DispatchCommand, HttpDispatchResult, LifecycleCommand,
	LifecycleEvent, LifecycleState,
};
pub use actor::task_types::ShutdownKind;
pub use actor::work_registry::{ActorWorkKind, ActorWorkPolicy};
pub use error::ActorLifecycle;
pub use inspector::{Inspector, InspectorSnapshot};
pub use registry::{CoreRegistry, EngineSpawnMode, ServeConfig};
pub use rivet_envoy_client::config::{
	HTTP_BODY_MAX_CHUNK_SIZE, HTTP_BODY_STREAM_CHANNEL_CAPACITY, HttpRequestBodyStream,
	ResponseChunk,
};
pub use runtime::{RuntimeBoxFuture, RuntimeSpawner, boxed_runtime_future};
pub use serverless::{CoreServerlessRuntime, ServerlessRequest, ServerlessResponse};
pub use types::{
	ActorKey, ActorKeySegment, ConnId, ListOpts, SaveStateOpts, WsMessage, format_actor_key,
};
pub use websocket::WebSocket;
