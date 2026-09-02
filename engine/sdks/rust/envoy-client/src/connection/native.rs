use std::sync::Arc;
use std::sync::atomic::Ordering;

use futures_util::{SinkExt, StreamExt};
use rivet_envoy_protocol as protocol;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_tungstenite::tungstenite;
use tracing::Instrument;
use vbare::OwnedVersionedData;

use crate::context::{HttpWsTxMessage, SharedContext, WsTxMessage};
use crate::envoy::ToEnvoyMessage;
use crate::handle::EnvoyHandle;
use crate::metrics::METRICS;
use crate::utils::{BackoffOptions, calculate_backoff, parse_ws_close_reason};

const STABLE_CONNECTION_MS: u64 = 60_000;

pub fn start_connection(shared: Arc<SharedContext>) {
	let span = tracing::debug_span!("envoy_connection", envoy_key = %shared.envoy_key);
	tokio::spawn(connection_loop(shared).instrument(span));
}

fn websocket_config() -> tungstenite::protocol::WebSocketConfig {
	tungstenite::protocol::WebSocketConfig::default()
		.max_message_size(None)
		.max_frame_size(None)
}

async fn connection_loop(shared: Arc<SharedContext>) {
	let mut attempt = 0u32;

	loop {
		if shared.shutting_down.load(Ordering::Acquire) {
			tracing::debug!("stopping reconnect loop because envoy is shutting down");
			return;
		}

		let connected_at = std::time::Instant::now();

		match single_connection(&shared).await {
			Ok((session, close_reason)) => {
				if let Some(reason) = &close_reason {
					if reason.group == "ws" && reason.error == "eviction" {
						tracing::debug!("connection evicted");
						let _ = crate::envoy::send_to_envoy_tx(
							&shared,
							ToEnvoyMessage::ConnClose { evict: true, session },
						);
						return;
					}
				}
				let _ = crate::envoy::send_to_envoy_tx(
					&shared,
					ToEnvoyMessage::ConnClose { evict: false, session },
				);
			}
			Err(error) => {
				tracing::error!(?error, "connection failed");
				let session = shared.connection_session.load(Ordering::Acquire);
				if session != 0 {
					super::remove_connection_for_session(&shared, session).await;
				}
				let _ = crate::envoy::send_to_envoy_tx(
					&shared,
					ToEnvoyMessage::ConnClose { evict: false, session },
				);
			}
		}

		if connected_at.elapsed().as_millis() >= STABLE_CONNECTION_MS as u128 {
			attempt = 0;
		}

		if shared.shutting_down.load(Ordering::Acquire) {
			tracing::debug!("skipping reconnect because envoy is shutting down");
			return;
		}

		let delay = calculate_backoff(attempt, &BackoffOptions::default());
		tracing::info!(attempt, delay_ms = delay.as_millis() as u64, "reconnecting");
		tokio::time::sleep(delay).await;
		attempt += 1;
	}
}

async fn single_connection(
	shared: &Arc<SharedContext>,
) -> anyhow::Result<(u64, Option<crate::utils::ParsedCloseReason>)> {
	let url = super::ws_url(shared);
	let protocols = {
		let mut p = vec!["rivet".to_string()];
		if let Some(token) = &shared.config.token {
			p.push(format!("rivet_token.{token}"));
		}
		p
	};

	// Initialize with a default CryptoProvider for rustls
	let provider = rustls::crypto::ring::default_provider();
	if provider.install_default().is_err() {
		tracing::debug!("crypto provider already installed in this process");
	}

	let request = tungstenite::http::Request::builder()
		.uri(&url)
		.header("Sec-WebSocket-Protocol", protocols.join(", "))
		.header("Connection", "Upgrade")
		.header("Upgrade", "websocket")
		.header(
			"Sec-WebSocket-Key",
			tungstenite::handshake::client::generate_key(),
		)
		.header("Sec-WebSocket-Version", "13")
		.header("Host", extract_host(&url))
		.body(())
		.map_err(|e| anyhow::anyhow!("failed to build ws request: {e}"))?;

	let (ws_stream, _) =
		tokio_tungstenite::connect_async_with_config(request, Some(websocket_config()), false)
			.await?;
	let (mut write, mut read) = ws_stream.split();

	// TODO: Bound shared WebSocket writer memory across protocol message types.
	// https://github.com/rivet-dev/rivet/issues/5468
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsTxMessage>();
	let (http_ws_tx, mut http_ws_rx) =
		mpsc::channel::<HttpWsTxMessage>(super::HTTP_WS_MESSAGE_CAPACITY);
	let http_byte_budget = Arc::new(Semaphore::new(super::HTTP_WS_BYTE_CAPACITY));
	let session =
		super::install_connection_with_http(shared, ws_tx, http_ws_tx, http_byte_budget).await;

	tracing::info!(
		endpoint = %shared.config.endpoint,
		namespace = %shared.config.namespace,
		envoy_key = %shared.envoy_key,
		has_token = shared.config.token.is_some(),
		"websocket connected"
	);
	shared
		.config
		.callbacks
		.on_connect(EnvoyHandle::from_shared(shared.clone()));

	let session_start = std::time::Instant::now();
	let mut disconnect_reason: &'static str = "stream_end";

	// Spawn write task
	let shared2 = shared.clone();
	let write_span = tracing::debug_span!("envoy_ws_write", envoy_key = %shared2.envoy_key);
	let (write_failed_tx, mut write_failed_rx) = oneshot::channel();
	let write_handle = tokio::spawn(
		async move {
			let mut write_failed_tx = Some(write_failed_tx);
			super::send_initial_metadata(&shared2).await;

			loop {
				let msg = tokio::select! {
					msg = ws_rx.recv() => msg.map(EitherWrite::Legacy),
					msg = http_ws_rx.recv() => msg.map(EitherWrite::Http),
				};
				let Some(msg) = msg else { break };
				match msg {
					EitherWrite::Legacy(WsTxMessage::Send(data)) => {
						let result = write.send(tungstenite::Message::Binary(data.into())).await;
						if let Err(e) = result {
							tracing::error!(?e, "failed to send ws message");
							if let Some(write_failed_tx) = write_failed_tx.take() {
								let _ = write_failed_tx.send(());
							}
							break;
						}
					}
					EitherWrite::Legacy(WsTxMessage::Close) => {
						let _ = write
							.send(tungstenite::Message::Close(Some(
								tungstenite::protocol::CloseFrame {
									code: tungstenite::protocol::frame::coding::CloseCode::Normal,
									reason: "envoy.shutdown".into(),
								},
							)))
							.await;
						break;
					}
					EitherWrite::Http(message) => {
						let result = write
							.send(tungstenite::Message::Binary(message.data.into()))
							.await
							.map_err(anyhow::Error::from);
						let failed = result.is_err();
						let _ = message.written.send(result);
						if failed {
							if let Some(write_failed_tx) = write_failed_tx.take() {
								let _ = write_failed_tx.send(());
							}
							break;
						}
					}
				}
			}
		}
		.instrument(write_span),
	);

	let mut result = None;

	let debug_latency_ms = shared.config.debug_latency_ms;

	loop {
		let msg = tokio::select! {
			msg = read.next() => msg,
			_ = &mut write_failed_rx => {
				disconnect_reason = "write_error";
				break;
			}
		};
		let Some(msg) = msg else { break };
		match msg {
			Ok(tungstenite::Message::Binary(data)) => {
				crate::utils::inject_latency(debug_latency_ms).await;

				let decoded = crate::protocol::versioned::ToEnvoy::deserialize(
					&data,
					protocol::PROTOCOL_VERSION,
				)?;

				super::forward_to_envoy(shared, session, decoded).await;
			}
			Ok(tungstenite::Message::Close(frame)) => {
				disconnect_reason = "close";
				if let Some(frame) = frame {
					let reason_str = frame.reason.to_string();
					let code: u16 = frame.code.into();
					tracing::info!(
						code,
						reason = %reason_str,
						"websocket closed"
					);
					result = parse_ws_close_reason(&reason_str);
				}
				break;
			}
			Err(e) => {
				disconnect_reason = "error";
				tracing::error!(?e, "websocket error");
				break;
			}
			_ => {}
		}
	}

	let session_duration = session_start.elapsed();
	METRICS
		.ws_session_duration_seconds
		.observe(session_duration.as_secs_f64());
	METRICS
		.ws_reconnect_total
		.with_label_values(&[disconnect_reason])
		.inc();
	tracing::info!(
		envoy_key = %shared.envoy_key,
		reason = disconnect_reason,
		session_duration_ms = session_duration.as_millis() as u64,
		"websocket session ended"
	);

	// Clean up
	super::remove_connection_for_session(shared, session).await;
	write_handle.abort();
	shared
		.config
		.callbacks
		.on_disconnect(EnvoyHandle::from_shared(shared.clone()));

	Ok((session, result))
}

enum EitherWrite {
	Legacy(WsTxMessage),
	Http(HttpWsTxMessage),
}

fn extract_host(url: &str) -> String {
	url.replace("ws://", "")
		.replace("wss://", "")
		.split('/')
		.next()
		.unwrap_or("localhost")
		.to_string()
}

#[cfg(test)]
mod tests {
	use futures_util::{SinkExt, StreamExt};
	use tokio::net::TcpListener;

	use super::*;

	#[test]
	fn websocket_config_has_no_input_size_limits() {
		let config = websocket_config();

		assert_eq!(config.max_frame_size, None);
		assert_eq!(config.max_message_size, None);
	}

	#[tokio::test]
	async fn receives_frame_larger_than_default_limit() {
		const FRAME_SIZE: usize = (16 << 20) + 64 * 1024;
		const FRAME_BYTE: u8 = 0xa5;

		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();
		let server = tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
			websocket
				.send(tungstenite::Message::Binary(
					vec![FRAME_BYTE; FRAME_SIZE].into(),
				))
				.await
				.unwrap();
		});

		let (mut client, _) = tokio_tungstenite::connect_async_with_config(
			format!("ws://{addr}"),
			Some(websocket_config()),
			false,
		)
		.await
		.unwrap();
		let message = client.next().await.unwrap().unwrap();
		let tungstenite::Message::Binary(data) = message else {
			panic!("expected a binary websocket message");
		};

		assert_eq!(data.len(), FRAME_SIZE);
		assert!(data.iter().all(|&byte| byte == FRAME_BYTE));
		server.await.unwrap();
	}
}
