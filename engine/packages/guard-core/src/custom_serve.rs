use anyhow::{Result, bail};
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming as BodyIncoming};
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;

use crate::WebSocketHandle;
use crate::request_context::RequestContext;
use crate::response_body::ResponseBody;

pub enum HibernationResult {
	Continue,
	Close,
}

/// Trait for custom request serving logic that can handle both HTTP and WebSocket requests
#[async_trait]
pub trait CustomServeTrait: Send + Sync {
	/// Returns true when this service wants the original request body stream.
	/// The default buffered path keeps retry semantics for existing custom routes.
	fn streams_request_body(&self) -> bool {
		false
	}

	/// Handle a regular HTTP request
	async fn handle_request(
		&self,
		req: Request<Full<Bytes>>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>>;

	/// Handle a regular HTTP request with the original inbound body stream.
	async fn handle_streaming_request(
		&self,
		_req: Request<BodyIncoming>,
		_req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>> {
		bail!("service does not support streaming request bodies");
	}

	/// Handle a WebSocket connection after upgrade. Supports connection retries.
	async fn handle_websocket(
		&self,
		_req_ctx: &mut RequestContext,
		_websocket: WebSocketHandle,
		// True if this websocket is reconnecting after hibernation.
		_after_hibernation: bool,
	) -> Result<Option<CloseFrame>> {
		bail!("service does not support websockets");
	}

	// TODO: Combine into handle_websocket, remove hibernation from guard
	/// Returns true if the websocket should close.
	async fn handle_websocket_hibernation(
		&self,
		_req_ctx: &mut RequestContext,
		_websocket: WebSocketHandle,
	) -> Result<HibernationResult> {
		bail!("service does not support websocket hibernation");
	}
}
