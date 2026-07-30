use std::{
	collections::HashMap,
	future::Future,
	pin::Pin,
	sync::{Arc, Mutex},
};

use rivet_envoy_protocol as protocol;
use tokio::sync::oneshot;

use crate::{
	handle::EnvoyHandle,
	http::{HttpRequest, HttpResponse},
	websocket::{WebSocketHandler, WebSocketSender},
};

#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[cfg(target_arch = "wasm32")]
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// One-shot completion handle used to defer the final stopped event until teardown is done.
#[derive(Clone)]
pub struct ActorStopHandle {
	tx: Arc<Mutex<Option<oneshot::Sender<anyhow::Result<()>>>>>,
}

impl ActorStopHandle {
	pub(crate) fn new(tx: oneshot::Sender<anyhow::Result<()>>) -> Self {
		Self {
			tx: Arc::new(Mutex::new(Some(tx))),
		}
	}

	pub fn complete(self) -> bool {
		self.finish(Ok(()))
	}

	pub fn fail(self, error: anyhow::Error) -> bool {
		self.finish(Err(error))
	}

	pub fn finish(self, result: anyhow::Result<()>) -> bool {
		let mut guard = match self.tx.lock() {
			Ok(guard) => guard,
			Err(poisoned) => poisoned.into_inner(),
		};

		let Some(tx) = guard.take() else {
			return false;
		};

		tx.send(result).is_ok()
	}
}

/// Callbacks that the consumer of the envoy client must implement.
pub trait EnvoyCallbacks: Send + Sync + 'static {
	fn on_connect(&self, _handle: EnvoyHandle) {}

	fn on_disconnect(&self, _handle: EnvoyHandle) {}

	fn on_actor_start(
		&self,
		handle: EnvoyHandle,
		actor_id: String,
		generation: u32,
		config: protocol::ActorConfig,
		preloaded_kv: Option<protocol::PreloadedKv>,
	) -> BoxFuture<anyhow::Result<()>>;

	fn on_actor_stop(
		&self,
		_handle: EnvoyHandle,
		_actor_id: String,
		_generation: u32,
		_reason: protocol::StopActorReason,
	) -> BoxFuture<anyhow::Result<()>> {
		Box::pin(async { Ok(()) })
	}

	fn on_actor_stop_with_completion(
		&self,
		handle: EnvoyHandle,
		actor_id: String,
		generation: u32,
		reason: protocol::StopActorReason,
		stop_handle: ActorStopHandle,
	) -> BoxFuture<anyhow::Result<()>> {
		let stop_future = self.on_actor_stop(handle, actor_id, generation, reason);

		Box::pin(async move {
			stop_future.await?;
			stop_handle.complete();
			Ok(())
		})
	}

	fn on_shutdown(&self);

	fn fetch(
		&self,
		handle: EnvoyHandle,
		actor_id: String,
		gateway_id: protocol::GatewayId,
		request_id: protocol::RequestId,
		request: HttpRequest,
	) -> BoxFuture<anyhow::Result<HttpResponse>>;

	fn websocket(
		&self,
		handle: EnvoyHandle,
		actor_id: String,
		gateway_id: protocol::GatewayId,
		request_id: protocol::RequestId,
		request: HttpRequest,
		path: String,
		headers: HashMap<String, String>,
		is_hibernatable: bool,
		is_restoring_hibernatable: bool,
		sender: WebSocketSender,
	) -> BoxFuture<anyhow::Result<WebSocketHandler>>;

	fn can_hibernate(
		&self,
		actor_id: &str,
		gateway_id: &protocol::GatewayId,
		request_id: &protocol::RequestId,
		request: &HttpRequest,
	) -> BoxFuture<anyhow::Result<bool>>;
}
