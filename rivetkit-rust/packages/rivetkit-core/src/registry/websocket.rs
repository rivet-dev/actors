use super::actor_connect::*;
use super::dispatch::*;
use super::inspector::encode_json_as_cbor;
use super::*;
use crate::error::ProtocolError;
use crate::time::timeout;
use tracing::Instrument;

impl RegistryDispatcher {
	pub(super) async fn handle_websocket(
		self: &Arc<Self>,
		actor_id: &str,
		request: &HttpRequest,
		path: &str,
		headers: &HashMap<String, String>,
		gateway_id: &protocol::GatewayId,
		request_id: &protocol::RequestId,
		is_hibernatable: bool,
		is_restoring_hibernatable: bool,
		sender: WebSocketSender,
	) -> Result<WebSocketHandler> {
		tracing::info!(actor_id, path, "handle_websocket: routing");
		let instance = self.active_actor(actor_id).await?;
		if is_inspector_connect_path(path)? {
			tracing::info!(
				actor_id,
				"handle_websocket: dispatching to inspector handler"
			);
			return self
				.handle_inspector_websocket(actor_id, instance, request, headers)
				.await;
		}
		if is_actor_connect_path(path)? {
			tracing::info!(
				actor_id,
				"handle_websocket: dispatching to actor-connect handler"
			);
			return self
				.handle_actor_connect_websocket(
					actor_id,
					instance,
					request,
					path,
					headers,
					gateway_id,
					request_id,
					is_hibernatable,
					is_restoring_hibernatable,
					sender,
				)
				.await;
		}
		tracing::info!(
			actor_id,
			path,
			"handle_websocket: dispatching to raw handler"
		);
		match self
			.handle_raw_websocket(
				actor_id,
				instance,
				request,
				path,
				headers,
				gateway_id,
				request_id,
				is_hibernatable,
				is_restoring_hibernatable,
				sender,
			)
			.await
		{
			Ok(handler) => Ok(handler),
			Err(error) => {
				let rivet_error = RivetError::extract(&error);
				tracing::warn!(
					actor_id,
					group = rivet_error.group(),
					code = rivet_error.code(),
					?error,
					"failed to establish raw websocket connection"
				);
				Ok(closing_websocket_handler(
					1011,
					&format!("{}.{}", rivet_error.group(), rivet_error.code()),
				))
			}
		}
	}

	#[allow(clippy::too_many_arguments)]
	async fn handle_actor_connect_websocket(
		self: &Arc<Self>,
		actor_id: &str,
		instance: Arc<ActorTaskHandle>,
		_request: &HttpRequest,
		path: &str,
		headers: &HashMap<String, String>,
		gateway_id: &protocol::GatewayId,
		request_id: &protocol::RequestId,
		is_hibernatable: bool,
		is_restoring_hibernatable: bool,
		sender: WebSocketSender,
	) -> Result<WebSocketHandler> {
		let encoding = match websocket_encoding(headers) {
			Ok(encoding) => encoding,
			Err(error) => {
				tracing::warn!(
					actor_id,
					?error,
					"rejecting unsupported actor connect encoding"
				);
				return Ok(closing_websocket_handler(
					1003,
					"actor.unsupported_websocket_encoding",
				));
			}
		};

		let max_incoming_message_size =
			instance.factory.config().max_incoming_message_size as usize;
		let max_outgoing_message_size =
			instance.factory.config().max_outgoing_message_size as usize;
		let connect_timeout = instance.factory.config().on_connect_timeout;

		let conn_params = websocket_conn_params(headers)?;
		let connect_request = Request::from_parts("GET", path, headers.clone(), Vec::new())
			.context("build actor connect request")?;
		let conn = if is_restoring_hibernatable {
			match instance
				.ctx
				.reconnect_hibernatable_conn(gateway_id, request_id)
			{
				Ok(conn) => conn,
				Err(error) => {
					let rivet_error = RivetError::extract(&error);
					tracing::warn!(
						actor_id,
						group = rivet_error.group(),
						code = rivet_error.code(),
						?error,
						"failed to restore actor websocket connection"
					);
					return Ok(closing_websocket_handler(
						1011,
						&format!("{}.{}", rivet_error.group(), rivet_error.code()),
					));
				}
			}
		} else {
			let hibernation = is_hibernatable.then(|| HibernatableConnectionMetadata {
				gateway_id: *gateway_id,
				request_id: *request_id,
				server_message_index: 0,
				client_message_index: 0,
				request_path: path.to_owned(),
				request_headers: headers
					.iter()
					.map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
					.collect(),
			});

			let prepare_sender = sender.clone();
			match timeout(
				connect_timeout,
				instance.ctx.connect_conn_with_prepare(
					conn_params,
					is_hibernatable,
					hibernation,
					Some(connect_request),
					async { Ok(Vec::new()) },
					move |conn| {
						configure_actor_connect_connection(
							conn,
							prepare_sender,
							encoding,
							max_outgoing_message_size,
						)
					},
				),
			)
			.await
			{
				Ok(Ok(conn)) => conn,
				Ok(Err(error)) => {
					let rivet_error = RivetError::extract(&error);
					tracing::warn!(
						actor_id,
						group = rivet_error.group(),
						code = rivet_error.code(),
						?error,
						"failed to establish actor websocket connection"
					);
					let metadata = rivet_error.metadata().and_then(|metadata| {
						encode_json_as_cbor(&metadata).ok().map(ByteBuf::from)
					});
					return Ok(actor_connect_error_websocket_handler(
						encoding,
						ActorConnectError {
							group: rivet_error.group().to_owned(),
							code: rivet_error.code().to_owned(),
							message: rivet_error.message().to_owned(),
							metadata,
							action_id: None,
							actor: rivet_error.actor().cloned(),
						},
						max_outgoing_message_size,
					));
				}
				Err(_) => {
					tracing::warn!(
						actor_id,
						timeout_ms = connect_timeout.as_millis(),
						"actor websocket connection setup timed out"
					);
					return Ok(actor_connect_error_websocket_handler(
						encoding,
						ActorConnectError {
							group: "actor".to_owned(),
							code: "callback_timed_out".to_owned(),
							message: format!(
								"actor websocket connection setup timed out after {} ms",
								connect_timeout.as_millis()
							),
							metadata: None,
							action_id: None,
							actor: None,
						},
						max_outgoing_message_size,
					));
				}
			}
		};

		if is_restoring_hibernatable {
			configure_actor_connect_connection(
				&conn,
				sender.clone(),
				encoding,
				max_outgoing_message_size,
			)?;
		}

		let init_actor_id = instance.ctx.actor_id().to_owned();
		let init_conn_id = conn.id().to_owned();
		let on_message_conn = conn.clone();
		let on_message_ctx = instance.ctx.clone();
		let on_message_dispatch = instance.dispatch.clone();

		let on_open: Option<Box<dyn FnOnce(WebSocketSender) -> EnvoyBoxFuture<()> + Send>> =
			if is_restoring_hibernatable {
				None
			} else {
				Some(Box::new(move |sender| {
					let actor_id = init_actor_id.clone();
					let conn_id = init_conn_id.clone();
					Box::pin(async move {
						if let Err(error) = send_actor_connect_message(
							&sender,
							encoding,
							&ActorConnectToClient::Init(ActorConnectInit {
								actor_id,
								connection_id: conn_id,
							}),
							max_outgoing_message_size,
						) {
							match error {
								ActorConnectSendError::OutgoingTooLong => {
									sender.close(
										Some(1011),
										Some("message.outgoing_too_long".to_owned()),
									);
								}
								ActorConnectSendError::Encode(error) => {
									tracing::error!(
										?error,
										"failed to send actor websocket init message"
									);
									sender.close(Some(1011), Some("actor.init_error".to_owned()));
								}
							}
						}
					})
				}))
			};

		Ok(WebSocketHandler {
			on_message: Box::new(move |message: WebSocketMessage| {
				let conn = on_message_conn.clone();
				let ctx = on_message_ctx.clone();
				let dispatch = on_message_dispatch.clone();
				Box::pin(async move {
					if message.data.len() > max_incoming_message_size {
						message
							.sender
							.close(Some(1011), Some("message.incoming_too_long".to_owned()));
						return;
					}

					let parsed = match decode_actor_connect_message(&message.data, encoding) {
						Ok(parsed) => parsed,
						Err(error) => {
							tracing::warn!(?error, "failed to decode actor websocket message");
							message
								.sender
								.close(Some(1011), Some("actor.invalid_request".to_owned()));
							return;
						}
					};

					match parsed {
						ActorConnectToServer::SubscriptionRequest(request) => {
							if conn.is_hibernatable()
								&& let Err(error) = persist_and_ack_hibernatable_actor_message(
									&ctx,
									&conn,
									message.message_index,
								)
								.await
							{
								tracing::warn!(
									?error,
									conn_id = conn.id(),
									"failed to persist and ack hibernatable actor websocket message"
								);
								message.sender.close(
									Some(1011),
									Some("actor.hibernation_persist_failed".to_owned()),
								);
								return;
							}
							if request.subscribe {
								if let Err(error) = dispatch_subscribe_request(
									&ctx,
									conn.clone(),
									request.event_name.clone(),
								)
								.await
								{
									let error = RivetError::extract(&error);
									message.sender.close(
										Some(1011),
										Some(format!("{}.{}", error.group(), error.code())),
									);
									return;
								}
								conn.subscribe(request.event_name);
							} else {
								conn.unsubscribe(&request.event_name);
							}
						}
						ActorConnectToServer::ActionRequest(request) => {
							let sender = message.sender.clone();
							let ctx = ctx.clone();
							let conn = conn.clone();
							let message_index = message.message_index;
							let actor_id = ctx.actor_id().to_owned();
							RuntimeSpawner::spawn(
								async move {
									if conn.is_hibernatable() && ctx.sleep_requested() {
										tracing::debug!(
											conn_id = conn.id(),
											message_index,
											action_name = %request.name,
											"deferring hibernatable actor websocket action while actor is entering sleep"
										);
										return;
									}

									let response = match dispatch_action_through_task(
										&dispatch,
										conn.clone(),
										request.name.clone(),
										request.args.into_vec(),
										crate::telemetry::IncomingInvocationContext::default(),
									)
									.await
									{
										Ok(output) => ActorConnectToClient::ActionResponse(
											ActorConnectActionResponse {
												id: request.id,
												output: ByteBuf::from(output),
											},
										),
										Err(error) => {
											if conn.is_hibernatable() && ctx.sleep_requested() {
												tracing::debug!(
													conn_id = conn.id(),
													message_index,
													action_name = request.name,
													"deferring hibernatable actor websocket action while actor is entering sleep"
												);
												return;
											}
											ActorConnectToClient::Error(
												action_dispatch_error_response(error, request.id),
											)
										}
									};

									if conn.is_hibernatable()
										&& let Err(error) =
											persist_and_ack_hibernatable_actor_message(
												&ctx,
												&conn,
												message_index,
											)
											.await
									{
										tracing::warn!(
											?error,
											conn_id = conn.id(),
											"failed to persist and ack hibernatable actor websocket message"
										);
										sender.close(
											Some(1011),
											Some("actor.hibernation_persist_failed".to_owned()),
										);
										return;
									}

									match send_actor_connect_message(
										&sender,
										encoding,
										&response,
										max_outgoing_message_size,
									) {
										Ok(()) => {}
										Err(ActorConnectSendError::OutgoingTooLong) => {
											let error_response =
												ActorConnectToClient::Error(ActorConnectError {
													group: "message".to_owned(),
													code: "outgoing_too_long".to_owned(),
													message: "Outgoing message too long".to_owned(),
													metadata: None,
													action_id: Some(request.id),
													actor: None,
												});
											if let Err(error) = send_actor_connect_message(
												&sender,
												encoding,
												&error_response,
												usize::MAX,
											) {
												match error {
													ActorConnectSendError::OutgoingTooLong => {
														sender.close(
															Some(1011),
															Some(
																"message.outgoing_too_long"
																	.to_owned(),
															),
														);
													}
													ActorConnectSendError::Encode(error) => {
														tracing::error!(
															?error,
															"failed to send actor websocket outgoing-size error"
														);
														sender.close(
															Some(1011),
															Some("actor.send_failed".to_owned()),
														);
													}
												}
											}
										}
										Err(ActorConnectSendError::Encode(error)) => {
											tracing::error!(
												?error,
												"failed to send actor websocket response"
											);
											sender.close(
												Some(1011),
												Some("actor.send_failed".to_owned()),
											);
										}
									}
								}
								.instrument(tracing::info_span!(
									"actor_connect_ws",
									actor_id = %actor_id,
								)),
							);
						}
					}
				})
			}),
			on_close: Box::new(move |_code, reason| {
				let conn = conn.clone();
				Box::pin(async move {
					if let Err(error) = conn.disconnect(Some(reason.as_str())).await {
						tracing::warn!(
							?error,
							conn_id = conn.id(),
							"failed to disconnect actor websocket connection"
						);
					}
				})
			}),
			on_open,
		})
	}

	async fn handle_raw_websocket(
		self: &Arc<Self>,
		actor_id: &str,
		instance: Arc<ActorTaskHandle>,
		request: &HttpRequest,
		path: &str,
		headers: &HashMap<String, String>,
		gateway_id: &protocol::GatewayId,
		request_id: &protocol::RequestId,
		is_hibernatable: bool,
		is_restoring_hibernatable: bool,
		_sender: WebSocketSender,
	) -> Result<WebSocketHandler> {
		let conn_params = websocket_conn_params(headers)?;
		let websocket_request = Request::from_parts(
			&request.method,
			path,
			headers.clone(),
			request.body.clone().unwrap_or_default(),
		)
		.context("build actor websocket request")?;
		let conn = if is_restoring_hibernatable {
			instance
				.ctx
				.reconnect_hibernatable_conn(gateway_id, request_id)?
		} else {
			let hibernation = is_hibernatable.then(|| HibernatableConnectionMetadata {
				gateway_id: *gateway_id,
				request_id: *request_id,
				server_message_index: 0,
				client_message_index: 0,
				request_path: path.to_owned(),
				request_headers: headers
					.iter()
					.map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
					.collect(),
			});

			instance
				.ctx
				.connect_conn(
					conn_params,
					is_hibernatable,
					hibernation,
					Some(websocket_request.clone()),
					async { Ok(Vec::new()) },
				)
				.await?
		};
		let ctx = instance.ctx.clone();
		let dispatch = instance.dispatch.clone();
		let conn_for_close = conn.clone();
		let conn_for_message = conn.clone();
		let conn_for_open = conn.clone();
		let ctx_for_message = ctx.clone();
		let ctx_for_close = ctx.clone();
		let ws = WebSocket::new();
		let ctx_for_close_event_region = ctx.clone();
		ws.configure_close_event_callback_region(Some(Arc::new(move || {
			ctx_for_close_event_region.websocket_callback_region()
		})));
		let ws_for_open = ws.clone();
		let ws_for_message = ws.clone();
		let ws_for_close = ws.clone();
		let request_for_open = websocket_request.clone();
		let actor_id = actor_id.to_owned();
		let actor_id_for_close = actor_id.clone();
		let actor_id_for_open = actor_id.clone();
		let ack_test_state = Arc::new(Mutex::new(RawHibernatableAckTestState::default()));
		let (closed_tx, _closed_rx) = oneshot::channel();
		// Forced-sync: close notification is a small sync slot consumed once
		// from the WebSocket close callback.
		let closed_tx = Arc::new(Mutex::new(Some(closed_tx)));

		Ok(WebSocketHandler {
			on_message: Box::new(move |message: WebSocketMessage| {
				let ctx = ctx_for_message.clone();
				let conn = conn_for_message.clone();
				let ws = ws_for_message.clone();
				let ack_test_state = ack_test_state.clone();
				Box::pin(async move {
					let callback_ctx = ctx.clone();
					ctx.with_websocket_callback(|| async move {
						if is_hibernatable
							&& maybe_respond_to_raw_hibernatable_ack_state_probe(
								&ws,
								&message,
								&ack_test_state,
							) {
							return;
						}

						let payload = if message.binary {
							WsMessage::Binary(message.data)
						} else {
							match String::from_utf8(message.data) {
								Ok(text) => WsMessage::Text(text),
								Err(error) => {
									tracing::warn!(
										?error,
										"raw websocket message was not valid utf-8"
									);
									ws.close(Some(1007), Some("message.invalid_utf8".to_owned()))
										.await;
									return;
								}
							}
						};
						ws.dispatch_message_event(payload, Some(message.message_index));
						if is_hibernatable
							&& let Err(error) = persist_and_ack_hibernatable_actor_message(
								&callback_ctx,
								&conn,
								message.message_index,
							)
							.await
						{
							tracing::warn!(
								?error,
								conn_id = conn.id(),
								"failed to persist and ack hibernatable raw websocket message"
							);
							ws.close(
								Some(1011),
								Some("actor.hibernation_persist_failed".to_owned()),
							)
							.await;
							return;
						}
						if is_hibernatable {
							ack_test_state.lock().record(message.message_index);
						}
					})
					.await;
				})
			}),
			on_close: Box::new(move |code, reason| {
				let conn = conn_for_close.clone();
				let ws = ws_for_close.clone();
				let actor_id = actor_id_for_close.clone();
				let ctx = ctx_for_close.clone();
				let closed_tx = closed_tx.clone();
				Box::pin(async move {
					ws.close(Some(1000), Some("hack_force_close".to_owned()))
						.await;
					ctx.with_websocket_callback(|| async move {
						ws.dispatch_close_event(code, reason.clone(), code == 1000)
							.await;
						if let Err(error) = conn.disconnect(Some(reason.as_str())).await {
							tracing::warn!(
								actor_id,
								?error,
								conn_id = conn.id(),
								"failed to disconnect raw websocket connection"
							);
						}
					})
					.await;
					if let Some(closed_tx) = closed_tx.lock().take() {
						let _ = closed_tx.send(());
					}
				})
			}),
			on_open: Some(Box::new(move |sender| {
				let request = request_for_open.clone();
				let conn = conn_for_open.clone();
				let ws = ws_for_open.clone();
				let actor_id = actor_id_for_open.clone();
				let dispatch = dispatch.clone();
				Box::pin(async move {
					let close_sender = sender.clone();
					ws.configure_sender(sender);
					let result = dispatch_websocket_open_through_task(
						&dispatch,
						conn,
						ws.clone(),
						Some(request),
					)
					.await;
					if let Err(error) = result {
						let error = RivetError::extract(&error);
						tracing::error!(actor_id, ?error, "actor raw websocket callback failed");
						close_sender.close(
							Some(1011),
							Some(format!("{}.{}", error.group(), error.code())),
						);
					}
				})
			})),
		})
	}
}

fn configure_actor_connect_connection(
	conn: &ConnHandle,
	sender: WebSocketSender,
	encoding: ActorConnectEncoding,
	max_outgoing_message_size: usize,
) -> Result<()> {
	let managed_disconnect = conn
		.managed_disconnect_handler()
		.context("get actor websocket disconnect handler")?;
	let transport_closed = Arc::new(AtomicBool::new(false));
	let transport_disconnect_sender = sender.clone();
	conn.configure_transport_disconnect_handler(Some(Arc::new(move |reason| {
		let transport_closed = transport_closed.clone();
		let transport_disconnect_sender = transport_disconnect_sender.clone();
		Box::pin(async move {
			if !transport_closed.swap(true, Ordering::SeqCst) {
				transport_disconnect_sender.close(Some(1000), reason);
			}
			Ok(())
		})
	})));
	conn.configure_disconnect_handler(Some(managed_disconnect));

	let event_sender = sender.clone();
	conn.configure_event_sender(Some(Arc::new(
		move |event| match send_actor_connect_message(
			&event_sender,
			encoding,
			&ActorConnectToClient::Event(ActorConnectEvent {
				name: event.name,
				args: ByteBuf::from(event.args),
			}),
			max_outgoing_message_size,
		) {
			Ok(()) => Ok(()),
			Err(ActorConnectSendError::OutgoingTooLong) => {
				event_sender.close(Some(1011), Some("message.outgoing_too_long".to_owned()));
				Ok(())
			}
			Err(ActorConnectSendError::Encode(error)) => Err(error),
		},
	)));
	Ok(())
}

pub(super) async fn persist_and_ack_hibernatable_actor_message(
	ctx: &ActorContext,
	conn: &ConnHandle,
	message_index: u16,
) -> Result<()> {
	let Some(hibernation) = conn.set_server_message_index(message_index) else {
		return Ok(());
	};
	ctx.request_hibernation_transport_save(conn.id());
	ctx.ack_hibernatable_websocket_message(
		&hibernation.gateway_id,
		&hibernation.request_id,
		message_index,
	)?;
	Ok(())
}

#[derive(Default)]
struct RawHibernatableAckTestState {
	last_sent_index: u16,
	last_acked_index: u16,
}

impl RawHibernatableAckTestState {
	fn record(&mut self, message_index: u16) {
		self.last_sent_index = self.last_sent_index.max(message_index);
		self.last_acked_index = self.last_acked_index.max(message_index);
	}
}

fn maybe_respond_to_raw_hibernatable_ack_state_probe(
	ws: &WebSocket,
	message: &WebSocketMessage,
	state: &Arc<Mutex<RawHibernatableAckTestState>>,
) -> bool {
	if env::var_os("VITEST").is_none() || message.binary {
		return false;
	}

	let Ok(value) = serde_json::from_slice::<JsonValue>(&message.data) else {
		return false;
	};
	if value
		.get("__rivetkitTestHibernatableAckStateV1")
		.and_then(JsonValue::as_bool)
		!= Some(true)
	{
		return false;
	}

	let state = state.lock();
	ws.send(WsMessage::Text(
		json!({
			"__rivetkitTestHibernatableAckStateV1": true,
			"lastSentIndex": state.last_sent_index,
			"lastAckedIndex": state.last_acked_index,
			"pendingIndexes": [],
		})
		.to_string(),
	));
	true
}

pub(super) fn websocket_inspector_token(headers: &HashMap<String, String>) -> Option<&str> {
	headers
		.iter()
		.find(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-protocol"))
		.and_then(|(_, value)| {
			value
				.split(',')
				.map(str::trim)
				.find_map(|protocol| protocol.strip_prefix("rivet_inspector_token."))
		})
}

pub(super) fn is_inspector_connect_path(path: &str) -> Result<bool> {
	Ok(Url::parse(&format!("http://inspector{path}"))
		.context("parse inspector websocket path")?
		.path()
		== "/inspector/connect")
}

pub(super) fn is_actor_connect_path(path: &str) -> Result<bool> {
	Ok(Url::parse(&format!("http://actor{path}"))
		.context("parse actor websocket path")?
		.path()
		== "/connect")
}

pub(super) fn websocket_protocols(headers: &HashMap<String, String>) -> impl Iterator<Item = &str> {
	headers
		.iter()
		.find(|(name, _)| name.eq_ignore_ascii_case("sec-websocket-protocol"))
		.map(|(_, value)| value.split(',').map(str::trim))
		.into_iter()
		.flatten()
}

pub(super) fn websocket_encoding(
	headers: &HashMap<String, String>,
) -> Result<ActorConnectEncoding> {
	match websocket_protocols(headers)
		.find_map(|protocol| protocol.strip_prefix(WS_PROTOCOL_ENCODING))
		.unwrap_or("json")
	{
		"json" => Ok(ActorConnectEncoding::Json),
		"cbor" => Ok(ActorConnectEncoding::Cbor),
		"bare" => Ok(ActorConnectEncoding::Bare),
		encoding => Err(ProtocolError::UnsupportedEncoding {
			encoding: encoding.to_owned(),
		}
		.build()),
	}
}

pub(super) fn websocket_conn_params(headers: &HashMap<String, String>) -> Result<Vec<u8>> {
	let Some(encoded_params) = websocket_protocols(headers)
		.find_map(|protocol| protocol.strip_prefix(WS_PROTOCOL_CONN_PARAMS))
	else {
		return encode_json_as_cbor(&serde_json::Value::Null);
	};

	let decoded = Url::parse(&format!("http://actor/?value={encoded_params}"))
		.context("decode websocket connection parameters")?
		.query_pairs()
		.find_map(|(name, value)| (name == "value").then_some(value.into_owned()))
		.ok_or_else(|| {
			ProtocolError::InvalidActorConnectRequest {
				field: "connection parameters".to_owned(),
				reason: "missing decoded value".to_owned(),
			}
			.build()
		})?;
	let parsed: JsonValue =
		serde_json::from_str(&decoded).context("parse websocket connection parameters")?;
	encode_json_as_cbor(&parsed)
}

pub(super) fn closing_websocket_handler(code: u16, reason: &str) -> WebSocketHandler {
	let reason = reason.to_owned();
	WebSocketHandler {
		on_message: Box::new(|_message: WebSocketMessage| Box::pin(async {})),
		on_close: Box::new(|_code, _reason| Box::pin(async {})),
		on_open: Some(Box::new(move |sender| {
			let reason = reason.clone();
			Box::pin(async move {
				sender.close(Some(code), Some(reason));
			})
		})),
	}
}

pub(super) fn actor_connect_error_websocket_handler(
	encoding: ActorConnectEncoding,
	error: ActorConnectError,
	max_outgoing_message_size: usize,
) -> WebSocketHandler {
	let close_reason = format!("{}.{}", error.group, error.code);
	WebSocketHandler {
		on_message: Box::new(|_message: WebSocketMessage| Box::pin(async {})),
		on_close: Box::new(|_code, _reason| Box::pin(async {})),
		on_open: Some(Box::new(move |sender| {
			let close_reason = close_reason.clone();
			Box::pin(async move {
				let message = ActorConnectToClient::Error(error);
				if let Err(send_error) = send_actor_connect_message(
					&sender,
					encoding,
					&message,
					max_outgoing_message_size,
				) {
					tracing::error!(
						?send_error,
						"failed to send actor websocket connection error"
					);
				}
				// Ensure the structured error frame is queued before the close
				// frame terminates the client connection.
				sender.flush().await;
				sender.close(Some(1011), Some(close_reason));
			})
		})),
	}
}
