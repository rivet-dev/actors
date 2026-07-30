#[cfg(target_arch = "wasm32")]
mod imp {
	use std::sync::Arc;
	use std::sync::atomic::Ordering;
	use std::time::Duration;

	use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
	use rivet_envoy_protocol as protocol;
	use tokio::sync::mpsc;
	use tracing::Instrument;
	use vbare::OwnedVersionedData;
	use wasm_bindgen::{JsCast, JsValue, closure::Closure};
	use wasm_bindgen_futures::JsFuture;
	use web_sys::{BinaryType, CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

	use crate::context::{SharedContext, WsTxMessage};
	use crate::envoy::ToEnvoyMessage;
	use crate::utils::{BackoffOptions, calculate_backoff, parse_ws_close_reason};

	const STABLE_CONNECTION_MS: u64 = 60_000;
	const NORMAL_CLOSE_CODE: u16 = 1000;

	enum ConnectionEvent {
		Open,
		Message(Vec<u8>),
		Close { code: u16, reason: String },
		Error(String),
		WriteFailed,
	}

	pub fn start_connection(shared: Arc<SharedContext>) {
		let span = tracing::debug_span!("envoy_connection", envoy_key = %shared.envoy_key);
		wasm_bindgen_futures::spawn_local(connection_loop(shared).instrument(span));
	}

	async fn connection_loop(shared: Arc<SharedContext>) {
		let mut attempt = 0u32;

		loop {
			if shared.shutting_down.load(Ordering::Acquire) {
				tracing::debug!("stopping reconnect loop because envoy is shutting down");
				return;
			}

			let connected_at_ms = js_sys::Date::now();

			match single_connection(&shared).await {
				Ok(close_reason) => {
					if let Some(reason) = &close_reason {
						if reason.group == "ws" && reason.error == "eviction" {
							tracing::debug!("connection evicted");
							let _ = crate::envoy::send_to_envoy_tx(
								&shared,
								ToEnvoyMessage::ConnClose { evict: true },
							);
							return;
						}
					}
					let _ = crate::envoy::send_to_envoy_tx(
						&shared,
						ToEnvoyMessage::ConnClose { evict: false },
					);
				}
				Err(error) => {
					tracing::error!(?error, "connection failed");
					let _ = crate::envoy::send_to_envoy_tx(
						&shared,
						ToEnvoyMessage::ConnClose { evict: false },
					);
				}
			}

			if js_sys::Date::now() - connected_at_ms >= STABLE_CONNECTION_MS as f64 {
				attempt = 0;
			}

			if shared.shutting_down.load(Ordering::Acquire) {
				tracing::debug!("skipping reconnect because envoy is shutting down");
				return;
			}

			let delay = calculate_backoff(attempt, &BackoffOptions::default());
			tracing::info!(attempt, delay_ms = delay.as_millis() as u64, "reconnecting");
			sleep(delay).await;
			attempt += 1;
		}
	}

	async fn single_connection(
		shared: &Arc<SharedContext>,
	) -> anyhow::Result<Option<crate::utils::ParsedCloseReason>> {
		let url = super::super::ws_url(shared);
		let protocols = protocols(&shared.config.token);
		let ws = WebSocket::new_with_str_sequence(&url, protocols.as_ref())
			.map_err(|error| anyhow::anyhow!("failed to create websocket: {}", js_error(error)))?;
		ws.set_binary_type(BinaryType::Arraybuffer);

		let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ConnectionEvent>();

		let onopen = {
			let event_tx = event_tx.clone();
			Closure::<dyn FnMut(Event)>::wrap(Box::new(move |_| {
				let _ = event_tx.send(ConnectionEvent::Open);
			}))
		};
		ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

		let onmessage = {
			let event_tx = event_tx.clone();
			let envoy_key = shared.envoy_key.clone();
			Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new(move |event| {
				let data = event.data();
				let Some(bytes) = decode_message_data(data) else {
					tracing::warn!(%envoy_key, "received non-binary websocket message");
					return;
				};
				let _ = event_tx.send(ConnectionEvent::Message(bytes));
			}))
		};
		ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

		let onclose = {
			let event_tx = event_tx.clone();
			Closure::<dyn FnMut(CloseEvent)>::wrap(Box::new(move |event| {
				let _ = event_tx.send(ConnectionEvent::Close {
					code: event.code(),
					reason: event.reason(),
				});
			}))
		};
		ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

		let onerror = {
			let event_tx = event_tx.clone();
			Closure::<dyn FnMut(ErrorEvent)>::wrap(Box::new(move |event| {
				let _ = event_tx.send(ConnectionEvent::Error(event.message()));
			}))
		};
		ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

		match event_rx
			.recv()
			.await
			.ok_or_else(|| anyhow::anyhow!("websocket closed before opening"))?
		{
			ConnectionEvent::Open => {}
			ConnectionEvent::Close { code, reason } => {
				tracing::info!(code, reason = %reason, "websocket closed");
				return Ok(parse_ws_close_reason(&reason));
			}
			ConnectionEvent::Error(message) => {
				anyhow::bail!("websocket failed to open: {message}");
			}
			ConnectionEvent::Message(_) | ConnectionEvent::WriteFailed => {
				anyhow::bail!("websocket produced an unexpected event before opening");
			}
		}

		// TODO: Bound shared WebSocket writer memory across protocol message types.
		// https://github.com/rivet-dev/rivet/issues/5468
		let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsTxMessage>();
		super::super::install_connection(shared, ws_tx).await;

		tracing::info!(
			endpoint = %shared.config.endpoint,
			namespace = %shared.config.namespace,
			envoy_key = %shared.envoy_key,
			has_token = shared.config.token.is_some(),
			"websocket connected"
		);

		wasm_bindgen_futures::spawn_local({
			let shared = shared.clone();
			let ws = ws.clone();
			let event_tx = event_tx.clone();
			let write_span = tracing::debug_span!("envoy_ws_write", envoy_key = %shared.envoy_key);
			async move {
				super::super::send_initial_metadata(&shared).await;

				while let Some(msg) = ws_rx.recv().await {
					match msg {
						WsTxMessage::Send(data) => {
							let data = Uint8Array::from(data.as_slice());
							if let Err(error) = ws.send_with_array_buffer(&data.buffer()) {
								tracing::error!(error = %js_error(error), "failed to send ws message");
								let _ = event_tx.send(ConnectionEvent::WriteFailed);
								break;
							}
						}
						WsTxMessage::Close => {
							let _ =
								ws.close_with_code_and_reason(NORMAL_CLOSE_CODE, "envoy.shutdown");
							break;
						}
					}
				}
			}
			.instrument(write_span)
		});

		let mut result = None;
		let debug_latency_ms = shared.config.debug_latency_ms;

		while let Some(event) = event_rx.recv().await {
			match event {
				ConnectionEvent::Open => {}
				ConnectionEvent::Message(data) => {
					if let Some(ms) = debug_latency_ms {
						if ms > 0 {
							sleep(Duration::from_millis(ms)).await;
						}
					}

					let decoded = crate::protocol::versioned::ToEnvoy::deserialize(
						&data,
						protocol::PROTOCOL_VERSION,
					)?;

					super::super::forward_to_envoy(shared, decoded).await;
				}
				ConnectionEvent::Close { code, reason } => {
					tracing::info!(code, reason = %reason, "websocket closed");
					result = parse_ws_close_reason(&reason);
					break;
				}
				ConnectionEvent::Error(message) => {
					tracing::error!(message = %message, "websocket error");
					break;
				}
				ConnectionEvent::WriteFailed => {
					break;
				}
			}
		}

		super::super::remove_connection(shared).await;

		close_if_open(&ws);
		ws.set_onopen(None);
		ws.set_onmessage(None);
		ws.set_onclose(None);
		ws.set_onerror(None);
		drop((onopen, onmessage, onclose, onerror));

		Ok(result)
	}

	fn protocols(token: &Option<String>) -> JsValue {
		let protocols = Array::new();
		protocols.push(&JsValue::from_str("rivet"));
		if let Some(token) = token {
			protocols.push(&JsValue::from_str(&format!("rivet_token.{token}")));
		}
		protocols.into()
	}

	fn decode_message_data(data: JsValue) -> Option<Vec<u8>> {
		if let Some(buffer) = data.dyn_ref::<js_sys::ArrayBuffer>() {
			return Some(uint8_array_to_vec(&Uint8Array::new(buffer)));
		}

		if let Some(array) = data.dyn_ref::<Uint8Array>() {
			return Some(uint8_array_to_vec(array));
		}

		None
	}

	fn uint8_array_to_vec(array: &Uint8Array) -> Vec<u8> {
		let mut bytes = vec![0; array.length() as usize];
		array.copy_to(&mut bytes);
		bytes
	}

	fn close_if_open(ws: &WebSocket) {
		let state = ws.ready_state();
		if state == WebSocket::CONNECTING || state == WebSocket::OPEN {
			let _ = ws.close();
		}
	}

	async fn sleep(delay: Duration) {
		let delay_ms = delay.as_millis().min(u32::MAX as u128) as f64;
		let promise = Promise::new(&mut |resolve, _reject| {
			let global = js_sys::global();
			let set_timeout = Reflect::get(&global, &JsValue::from_str("setTimeout"))
				.ok()
				.and_then(|value| value.dyn_into::<Function>().ok());

			if let Some(set_timeout) = set_timeout {
				let _ = set_timeout.call2(&global, &resolve, &JsValue::from_f64(delay_ms));
			} else {
				let _ = resolve.call0(&JsValue::UNDEFINED);
			}
		});

		let _ = JsFuture::from(promise).await;
	}

	fn js_error(error: JsValue) -> String {
		error
			.as_string()
			.or_else(|| {
				js_sys::JSON::stringify(&error)
					.ok()
					.and_then(|s| s.as_string())
			})
			.unwrap_or_else(|| "unknown JavaScript error".to_string())
	}
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
	use std::sync::Arc;

	use crate::context::SharedContext;
	use crate::envoy::ToEnvoyMessage;

	pub fn start_connection(shared: Arc<SharedContext>) {
		let _ = crate::envoy::send_to_envoy_tx(&shared, ToEnvoyMessage::ConnClose { evict: false });
		tracing::error!("wasm envoy transport requires the wasm32 target");
	}
}

pub use imp::start_connection;
