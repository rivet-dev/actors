use rivet_envoy_protocol as protocol;

use crate::callbacks::BoxFuture;

/// Handler returned by the websocket callback for receiving WebSocket events.
pub struct WebSocketHandler {
	pub on_message: Box<dyn Fn(WebSocketMessage) -> BoxFuture<()> + Send + Sync>,
	pub on_close: Box<dyn Fn(u16, String) -> BoxFuture<()> + Send + Sync>,
	pub on_open: Option<Box<dyn FnOnce(WebSocketSender) -> BoxFuture<()> + Send>>,
}

pub struct WebSocketMessage {
	pub data: Vec<u8>,
	pub binary: bool,
	pub gateway_id: protocol::GatewayId,
	pub request_id: protocol::RequestId,
	pub message_index: u16,
	/// Send data back on this WebSocket connection.
	pub sender: WebSocketSender,
}

/// Allows sending messages back on a WebSocket connection from within the on_message callback.
#[derive(Clone)]
pub struct WebSocketSender {
	pub(crate) tx: tokio::sync::mpsc::UnboundedSender<WsOutgoing>,
}

pub(crate) enum WsOutgoing {
	Message {
		data: Vec<u8>,
		binary: bool,
	},
	Flush {
		tx: tokio::sync::oneshot::Sender<()>,
	},
	Close {
		code: Option<u16>,
		reason: Option<String>,
	},
}

impl WebSocketSender {
	pub fn send(&self, data: Vec<u8>, binary: bool) {
		let _ = self.tx.send(WsOutgoing::Message { data, binary });
	}

	pub fn send_text(&self, text: &str) {
		self.send(text.as_bytes().to_vec(), false);
	}

	pub async fn flush(&self) {
		let (tx, rx) = tokio::sync::oneshot::channel();
		if self.tx.send(WsOutgoing::Flush { tx }).is_ok() {
			let _ = rx.await;
		}
	}

	pub fn close(&self, code: Option<u16>, reason: Option<String>) {
		let _ = self.tx.send(WsOutgoing::Close { code, reason });
	}
}
