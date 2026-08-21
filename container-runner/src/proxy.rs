//! Bridges Rivet's decoded tunnel traffic to the child game server's local port.
//!
//! The rivetkit runtime reassembles tunnel frames into decoded `Request`s and
//! `WebSocket` streams (`Actor::on_fetch`/`::on_websocket`); this module forwards
//! them to `127.0.0.1:<child_port>` and back, the glue the runtime leaves to the actor.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use rivetkit::{Request, Response, WebSocket, WsMessage};

/// Proxy a decoded inbound HTTP request to the child and map the response back.
///
/// The rivetkit actor surface delivers `/request/*` paths to `on_fetch` with
/// the prefix already stripped, so `uri` here is the child-relative path.
pub async fn http_proxy(child_port: u16, req: Request) -> Result<Response> {
	let (method, uri, headers, body) = req.to_parts();

	let path = if uri.starts_with('/') {
		uri
	} else {
		format!("/{uri}")
	};
	let url = format!("http://127.0.0.1:{child_port}{path}");
	let method = reqwest::Method::from_bytes(method.as_bytes())?;

	let client = reqwest::Client::new();
	let mut rb = client.request(method, &url);
	for (k, v) in &headers {
		// Drop hop-by-hop / addressing headers that don't apply to the local hop.
		if k.eq_ignore_ascii_case("host") || k.eq_ignore_ascii_case("content-length") {
			continue;
		}
		rb = rb.header(k, v);
	}
	if !body.is_empty() {
		rb = rb.body(body);
	}

	let resp = rb.send().await?;
	let status = resp.status().as_u16();
	let mut out_headers = HashMap::new();
	for (k, v) in resp.headers() {
		let name = k.as_str();
		// Drop hop-by-hop / framing headers: the tunnel re-frames the body and sets its
		// own content-length, so forwarding the child's transfer-encoding/content-length
		// would conflict and truncate the response.
		if name.eq_ignore_ascii_case("transfer-encoding")
			|| name.eq_ignore_ascii_case("content-length")
			|| name.eq_ignore_ascii_case("connection")
		{
			continue;
		}
		if let Ok(s) = v.to_str() {
			out_headers.insert(name.to_string(), s.to_string());
		}
	}
	let bytes = resp.bytes().await?.to_vec();

	Response::from_parts(status, out_headers, bytes)
}

/// Client -> child traffic forwarded off the runtime's event callbacks.
enum ClientEvent {
	Message(WsMessage),
	Close(u16, String),
}

/// Cap on client frames buffered toward a slow child. The sync message callback
/// cannot await for backpressure, so overflow fails the connection loudly instead
/// of buffering unbounded.
const CLIENT_TO_CHILD_QUEUE: usize = 1024;

/// Dial the child's WebSocket endpoint and pump frames in both directions:
/// child -> Rivet client via `ws.send`, and client -> child via the
/// WebSocket's message/close event callbacks.
pub async fn ws_proxy(child_port: u16, path: String, ws: WebSocket) -> Result<()> {
	let url = format!("ws://127.0.0.1:{child_port}{path}");
	let (child_ws, _resp) = tokio_tungstenite::connect_async(&url).await?;
	let (sink, mut stream) = child_ws.split();
	let sink = Arc::new(TokioMutex::new(sink));

	// client -> child. The message event callback is synchronous and invoked
	// from the runtime's envoy pump, so it must never block: enqueue and let a
	// spawned task own the child sink.
	let (tx, mut rx) = mpsc::channel::<ClientEvent>(CLIENT_TO_CHILD_QUEUE);
	{
		let tx = tx.clone();
		let ws = ws.clone();
		ws.clone()
			.configure_message_event_callback(Some(Arc::new(move |msg, _message_index| {
				match tx.try_send(ClientEvent::Message(msg)) {
					Ok(()) => Ok(()),
					Err(mpsc::error::TrySendError::Full(_)) => {
						// The child stopped draining; dropping frames would
						// corrupt the stream, so fail the connection loudly.
						let ws = ws.clone();
						tokio::spawn(async move {
							ws.close(
								Some(1011),
								Some("child not draining proxied frames".to_string()),
							)
							.await;
						});
						Err(anyhow::anyhow!(
							"client->child queue overflowed ({CLIENT_TO_CHILD_QUEUE} frames); closing"
						))
					}
					Err(mpsc::error::TrySendError::Closed(_)) => {
						Err(anyhow::anyhow!("client->child pump ended"))
					}
				}
			})));
	}
	ws.configure_close_event_callback(Some(Arc::new(move |code, reason, _was_clean| {
		let tx = tx.clone();
		Box::pin(async move {
			// Async context: awaiting the bounded send applies backpressure.
			tx.send(ClientEvent::Close(code, reason))
				.await
				.map_err(|_| anyhow::anyhow!("client->child pump ended"))
		})
	})));

	{
		let sink = sink.clone();
		tokio::spawn(async move {
			while let Some(event) = rx.recv().await {
				match event {
					ClientEvent::Message(msg) => {
						let frame = match msg {
							WsMessage::Binary(b) => Message::Binary(b.into()),
							WsMessage::Text(t) => Message::Text(t.into()),
						};
						let mut s = sink.lock().await;
						if s.send(frame).await.is_err() {
							break;
						}
					}
					ClientEvent::Close(code, reason) => {
						let frame = Message::Close(Some(CloseFrame {
							code: CloseCode::from(code),
							reason: reason.into(),
						}));
						let mut s = sink.lock().await;
						let _ = s.send(frame).await;
						let _ = s.close().await;
						break;
					}
				}
			}
		});
	}

	// child -> client pump.
	tokio::spawn(async move {
		while let Some(msg) = stream.next().await {
			match msg {
				Ok(Message::Binary(b)) => ws.send(WsMessage::Binary(b.to_vec())),
				Ok(Message::Text(t)) => ws.send(WsMessage::Text(t.as_str().to_string())),
				Ok(Message::Close(cf)) => {
					let (code, reason) = match cf {
						Some(c) => (Some(u16::from(c.code)), Some(c.reason.to_string())),
						None => (None, None),
					};
					ws.close(code, reason).await;
					break;
				}
				// Ping/Pong/Frame handled by tungstenite.
				Ok(_) => {}
				Err(e) => {
					ws.close(Some(1011), Some(format!("child ws error: {e}")))
						.await;
					break;
				}
			}
		}
	});

	Ok(())
}
