use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use rivetkit_core::error::ActorRuntime;
use rivetkit_core::{ActorHttpResponse, Request, Response, WebSocket};
use serde::{Serialize, de::DeserializeOwned};

use crate::action::ActionSet;
use crate::context::{ConnCtx, Ctx};
use crate::event::EventSet;
use crate::queue::QueueSet;

/// Classifies an HTTP callback for independent bounded dispatcher pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpCallbackClass {
	Standard,
	Live,
}

/// Optional actor-owned work admitted synchronously before an HTTP callback is
/// constructed or registered.
///
/// RivetKit wraps `release` in a drop guard, so it runs after the callback reply
/// is handed to core or whenever the registered callback is otherwise dropped.
pub enum HttpCallbackAdmission {
	Untracked,
	Tracked {
		release: Box<dyn FnOnce() + Send + 'static>,
	},
	Reject(anyhow::Error),
}

#[async_trait]
pub trait Actor: Send + Sync + Sized + 'static {
	type State: Serialize + DeserializeOwned + Send + Sync + 'static;
	type Input: DeserializeOwned + Default + Send + 'static;
	type Actions: ActionSet<Self>;
	type Events: EventSet;
	type Queue: QueueSet<Self>;
	type ConnParams: DeserializeOwned + Default + Send + Sync + 'static;
	type ConnState: Serialize + DeserializeOwned + Default + Send + Sync + Clone + 'static;
	type Action: DeserializeOwned + Send + 'static;

	const HAS_DATABASE: bool = false;

	/// Opts this actor into concurrent typed HTTP callbacks.
	const CONCURRENT_HTTP_CALLBACKS: bool = false;

	/// Maximum concurrently starting standard HTTP callbacks when concurrency is enabled.
	const MAX_CONCURRENT_HTTP_CALLBACKS: usize = 128;

	/// Maximum concurrently starting live HTTP callbacks when concurrency is enabled.
	const MAX_CONCURRENT_LIVE_HTTP_CALLBACK_STARTS: usize = 0;

	fn classify_http_request(_request: &Request) -> HttpCallbackClass {
		HttpCallbackClass::Standard
	}

	fn admit_http_request(
		self: &Arc<Self>,
		_ctx: &Ctx<Self>,
		_request: &Request,
	) -> HttpCallbackAdmission {
		HttpCallbackAdmission::Untracked
	}

	async fn create_state(_ctx: &Ctx<Self>, _input: Self::Input) -> Result<Self::State> {
		bail!(
			"{}",
			ActorRuntime::NotConfigured {
				component: "actor create_state hook".to_owned(),
			}
			.build()
		)
	}

	async fn create(_ctx: &Ctx<Self>) -> Result<Self> {
		bail!(
			"{}",
			ActorRuntime::NotConfigured {
				component: "actor create hook".to_owned(),
			}
			.build()
		)
	}

	async fn run(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
		Ok(())
	}

	async fn on_create(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
		Ok(())
	}

	async fn on_start(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
		Ok(())
	}

	async fn on_state_change(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
		Ok(())
	}

	async fn create_conn_state(
		self: Arc<Self>,
		_ctx: Ctx<Self>,
		_params: Self::ConnParams,
	) -> Result<Self::ConnState> {
		Ok(Self::ConnState::default())
	}

	async fn on_before_connect(
		self: Arc<Self>,
		_ctx: Ctx<Self>,
		_params: &Self::ConnParams,
	) -> Result<()> {
		Ok(())
	}

	async fn on_connect(self: Arc<Self>, _ctx: Ctx<Self>, _conn: ConnCtx<Self>) -> Result<()> {
		Ok(())
	}

	async fn on_disconnect(self: Arc<Self>, _ctx: Ctx<Self>, _conn: ConnCtx<Self>) {}

	async fn on_subscribe(
		self: Arc<Self>,
		_ctx: Ctx<Self>,
		_conn: ConnCtx<Self>,
		_event_name: String,
	) -> Result<()> {
		Ok(())
	}

	async fn on_fetch(self: Arc<Self>, _ctx: Ctx<Self>, _req: Request) -> Result<Response> {
		Response::from_parts(404, Default::default(), Vec::new())
	}

	/// Handles an HTTP request and may return either a buffered or streaming response.
	///
	/// Existing actors keep their buffered `on_fetch` behavior through this default.
	async fn on_fetch_response(
		self: Arc<Self>,
		ctx: Ctx<Self>,
		req: Request,
	) -> Result<ActorHttpResponse> {
		self.on_fetch(ctx, req)
			.await
			.map(ActorHttpResponse::Buffered)
	}

	async fn on_websocket(
		self: Arc<Self>,
		_ctx: Ctx<Self>,
		_ws: WebSocket,
		_req: Request,
	) -> Result<()> {
		bail!("websockets not supported")
	}

	async fn on_sleep(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
		Ok(())
	}

	async fn on_destroy(self: Arc<Self>, _ctx: Ctx<Self>) -> Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::Actor;
	use crate::action;

	struct EmptyActor;

	impl Actor for EmptyActor {
		type State = ();
		type Input = ();
		type Actions = ();
		type Events = ();
		type Queue = ();
		type ConnParams = ();
		type ConnState = ();
		type Action = action::Raw;
	}

	fn assert_actor<A: Actor>() {}

	#[test]
	fn empty_actor_impl_compiles() {
		assert_actor::<EmptyActor>();
	}
}
