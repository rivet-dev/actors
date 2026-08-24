use anyhow::Context;
use bytes::Bytes;
use futures_util::TryStreamExt;
use gas::prelude::Id;
use gas::prelude::*;
use hyper_tungstenite::tungstenite::Message;
use pegboard::actor_kv;
use pegboard::pubsub_subjects::GatewayReceiverSubject;
use rivet_envoy_protocol as ep;
use rivet_guard_core::websocket_handle::WebSocketReceiver;
use rivet_runner_protocol::{self as protocol, PROTOCOL_MK2_VERSION, versioned};
use scc::HashMap;
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::{Mutex, MutexGuard, watch};
use universaldb::utils::end_of_key_range;
use universalpubsub::Subscriber;
use universalpubsub::{PubSub, PublishOpts};
use vbare::OwnedVersionedData;

use crate::{LifecycleResult, actor_event_demuxer::ActorEventDemuxer, conn::Conn, errors, metrics};

#[tracing::instrument(name="ws_to_tunnel_task", skip_all, fields(ray_id=?ctx.ray_id(), req_id=?ctx.req_id(), runner_id=?conn.runner_id, workflow_id=?conn.workflow_id, protocol_version=%conn.protocol_version))]
pub async fn task(
	ctx: StandaloneCtx,
	conn: Arc<Conn>,
	ws_rx: Arc<Mutex<WebSocketReceiver>>,
	eviction_sub2: Subscriber,
	ws_to_tunnel_abort_rx: watch::Receiver<()>,
) -> Result<LifecycleResult> {
	let mut event_demuxer = ActorEventDemuxer::new(ctx.clone(), conn.runner_id);

	let res = task_inner(
		ctx,
		conn,
		ws_rx,
		eviction_sub2,
		ws_to_tunnel_abort_rx,
		&mut event_demuxer,
	)
	.await;

	// Must shutdown demuxer to allow for all in-flight events to finish
	event_demuxer.shutdown().await;

	res
}

#[tracing::instrument(skip_all, fields(runner_id=?conn.runner_id, workflow_id=?conn.workflow_id, protocol_version=%conn.protocol_version))]
pub async fn task_inner(
	ctx: StandaloneCtx,
	conn: Arc<Conn>,
	ws_rx: Arc<Mutex<WebSocketReceiver>>,
	mut eviction_sub2: Subscriber,
	mut ws_to_tunnel_abort_rx: watch::Receiver<()>,
	event_demuxer: &mut ActorEventDemuxer,
) -> Result<LifecycleResult> {
	let mut ws_rx = ws_rx.lock().await;
	let mut term_signal = rivet_runtime::TermSignal::get();

	loop {
		match recv_msg(
			&conn,
			&mut ws_rx,
			&mut eviction_sub2,
			&mut ws_to_tunnel_abort_rx,
			&mut term_signal,
		)
		.await?
		{
			Ok(Some(msg)) => {
				if protocol::is_mk2(conn.protocol_version) {
					handle_message_mk2(&ctx, &conn, event_demuxer, msg).await?;
				} else {
					handle_message_mk1(&ctx, &conn, msg).await?;
				}
			}
			Ok(None) => {}
			Err(lifecycle_res) => return Ok(lifecycle_res),
		}
	}
}

async fn recv_msg(
	conn: &Conn,
	ws_rx: &mut MutexGuard<'_, WebSocketReceiver>,
	eviction_sub2: &mut Subscriber,
	ws_to_tunnel_abort_rx: &mut watch::Receiver<()>,
	term_signal: &mut rivet_runtime::TermSignal,
) -> Result<std::result::Result<Option<Bytes>, LifecycleResult>> {
	let msg = tokio::select! {
		res = ws_rx.try_next() => {
			if let Some(msg) = res? {
				msg
			} else {
				tracing::debug!("websocket closed");
				return Ok(Err(LifecycleResult::Closed));
			}
		}
		_ = eviction_sub2.next() => {
			tracing::debug!("runner evicted");

			metrics::EVICTION_TOTAL
				.with_label_values(&[
					conn.namespace_id.to_string().as_str(),
					&conn.runner_name,
					conn.protocol_version.to_string().as_str(),
				])
				.inc();

			return Ok(Err(LifecycleResult::Evicted));
		}
		_ = ws_to_tunnel_abort_rx.changed() => {
			tracing::debug!("task aborted");
			return Ok(Err(LifecycleResult::Aborted));
		}
		_ = term_signal.recv() => {
			return Err(errors::WsError::GoingAway.build());
		}
	};

	match msg {
		Message::Binary(data) => {
			tracing::trace!(
				data_len = data.len(),
				"received binary message from WebSocket"
			);

			Ok(Ok(Some(data)))
		}
		Message::Close(_) => {
			tracing::debug!("websocket closed");
			return Ok(Err(LifecycleResult::Closed));
		}
		_ => {
			// Ignore other message types
			Ok(Ok(None))
		}
	}
}

#[tracing::instrument(skip_all)]
async fn handle_message_mk2(
	ctx: &StandaloneCtx,
	conn: &Conn,
	event_demuxer: &mut ActorEventDemuxer,
	msg: Bytes,
) -> Result<()> {
	// Parse message
	let msg = match versioned::ToServerMk2::deserialize(&msg, conn.protocol_version) {
		Ok(x) => x,
		Err(err) => {
			tracing::warn!(?err, msg_len = msg.len(), "failed to deserialize message");
			return Ok(());
		}
	};

	tracing::debug!(?msg, "received message from runner");

	match msg {
		protocol::mk2::ToServer::ToServerPong(pong) => {
			let now = util::timestamp::now();
			let rtt = now.saturating_sub(pong.ts);

			let rtt = if let Ok(rtt) = u32::try_from(rtt) {
				rtt
			} else {
				tracing::debug!("ping ts in the future, ignoring");
				u32::MAX
			};

			conn.last_rtt.store(rtt, Ordering::Relaxed);
			conn.last_ping_ts
				.store(util::timestamp::now(), Ordering::Relaxed);
		}
		// Process KV request
		protocol::mk2::ToServer::ToServerKvRequest(req) => {
			let actor_id = match Id::parse(&req.actor_id) {
				Ok(actor_id) => actor_id,
				Err(err) => {
					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: protocol::mk2::KvResponseData::KvErrorResponse(
									protocol::mk2::KvErrorResponse {
										message: err.to_string(),
									},
								),
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV error response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV error response to client")?;

					return Ok(());
				}
			};

			let actor_res = ctx
				.op(pegboard::ops::actor::get_for_runner::Input { actor_id })
				.await
				.with_context(|| format!("failed to get runner for actor: {}", actor_id))?;

			let Some(actor) = actor_res else {
				send_actor_kv_error(&conn, req.request_id, "actor does not exist").await?;
				return Ok(());
			};

			// Verify actor belongs to this runner
			if actor.runner_id != conn.runner_id {
				send_actor_kv_error(&conn, req.request_id, "actor does not belong to runner")
					.await?;
				return Ok(());
			}

			let recipient = actor_kv::Recipient {
				actor_id,
				namespace_id: conn.namespace_id,
				name: actor.name,
			};

			// TODO: Add queue and bg thread for processing kv ops
			// Run kv operation
			match req.data {
				protocol::mk2::KvRequestData::KvGetRequest(body) => {
					let res = actor_kv::get(&*ctx.udb()?, &recipient, body.keys).await;

					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: match res {
									Ok((keys, values, metadata)) => {
										protocol::mk2::KvResponseData::KvGetResponse(
											protocol::mk2::KvGetResponse {
												keys,
												values,
												metadata: metadata
													.into_iter()
													.map(|m| protocol::mk2::KvMetadata {
														version: m.version,
														update_ts: m.update_ts,
													})
													.collect(),
											},
										)
									}
									Err(err) => protocol::mk2::KvResponseData::KvErrorResponse(
										protocol::mk2::KvErrorResponse {
											// TODO: Don't return actual error?
											message: err.to_string(),
										},
									),
								},
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV get response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV get response to client")?;
				}
				protocol::mk2::KvRequestData::KvListRequest(body) => {
					let res = actor_kv::list(
						&*ctx.udb()?,
						&recipient,
						match body.query {
							protocol::mk2::KvListQuery::KvListAllQuery => {
								ep::KvListQuery::KvListAllQuery
							}
							protocol::mk2::KvListQuery::KvListRangeQuery(x) => {
								ep::KvListQuery::KvListRangeQuery(ep::KvListRangeQuery {
									start: x.start,
									end: x.end,
									exclusive: x.exclusive,
								})
							}
							protocol::mk2::KvListQuery::KvListPrefixQuery(x) => {
								ep::KvListQuery::KvListPrefixQuery(ep::KvListPrefixQuery {
									key: x.key,
								})
							}
						},
						body.reverse.unwrap_or_default(),
						body.limit
							.map(TryInto::try_into)
							.transpose()
							.context("KV list limit value overflow")?,
					)
					.await;

					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: match res {
									Ok((keys, values, metadata)) => {
										protocol::mk2::KvResponseData::KvListResponse(
											protocol::mk2::KvListResponse {
												keys,
												values,
												metadata: metadata
													.into_iter()
													.map(|m| protocol::mk2::KvMetadata {
														version: m.version,
														update_ts: m.update_ts,
													})
													.collect(),
											},
										)
									}
									Err(err) => protocol::mk2::KvResponseData::KvErrorResponse(
										protocol::mk2::KvErrorResponse {
											// TODO: Don't return actual error?
											message: err.to_string(),
										},
									),
								},
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV list response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV list response to client")?;
				}
				protocol::mk2::KvRequestData::KvPutRequest(body) => {
					let res = actor_kv::put(&*ctx.udb()?, &recipient, body.keys, body.values).await;

					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: match res {
									Ok(()) => protocol::mk2::KvResponseData::KvPutResponse,
									Err(err) => {
										protocol::mk2::KvResponseData::KvErrorResponse(
											protocol::mk2::KvErrorResponse {
												// TODO: Don't return actual error?
												message: err.to_string(),
											},
										)
									}
								},
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV put response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV put response to client")?;
				}
				protocol::mk2::KvRequestData::KvDeleteRequest(body) => {
					let res = actor_kv::delete(&*ctx.udb()?, &recipient, body.keys).await;

					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: match res {
									Ok(()) => protocol::mk2::KvResponseData::KvDeleteResponse,
									Err(err) => protocol::mk2::KvResponseData::KvErrorResponse(
										protocol::mk2::KvErrorResponse {
											// TODO: Don't return actual error?
											message: err.to_string(),
										},
									),
								},
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV delete response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV delete response to client")?;
				}
				protocol::mk2::KvRequestData::KvDeleteRangeRequest(body) => {
					let res =
						actor_kv::delete_range(&*ctx.udb()?, &recipient, body.start, body.end)
							.await;

					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: match res {
									Ok(()) => protocol::mk2::KvResponseData::KvDeleteResponse,
									Err(err) => protocol::mk2::KvResponseData::KvErrorResponse(
										protocol::mk2::KvErrorResponse {
											message: err.to_string(),
										},
									),
								},
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV delete range response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV delete range response to client")?;
				}
				protocol::mk2::KvRequestData::KvDropRequest => {
					let res = actor_kv::delete_all(&*ctx.udb()?, &recipient).await;

					let res_msg = versioned::ToClientMk2::wrap_latest(
						protocol::mk2::ToClient::ToClientKvResponse(
							protocol::mk2::ToClientKvResponse {
								request_id: req.request_id,
								data: match res {
									Ok(()) => protocol::mk2::KvResponseData::KvDropResponse,
									Err(err) => protocol::mk2::KvResponseData::KvErrorResponse(
										protocol::mk2::KvErrorResponse {
											// TODO: Don't return actual error?
											message: err.to_string(),
										},
									),
								},
							},
						),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV drop response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV drop response to client")?;
				}
			}
		}
		protocol::mk2::ToServer::ToServerTunnelMessage(tunnel_msg) => {
			handle_tunnel_message_mk2(
				&ctx.ups()
					.context("failed to get UPS instance for tunnel message")?,
				ctx.config()
					.pegboard()
					.runner_max_response_payload_body_size(),
				&conn.authorized_tunnel_routes,
				tunnel_msg,
			)
			.await
			.context("failed to handle tunnel message")?;
		}
		// NOTE: This does not process the first init event. See `conn::init_conn`
		protocol::mk2::ToServer::ToServerInit(_) => {
			tracing::debug!("received additional init packet, ignoring");
		}
		// Forward to actor wf
		protocol::mk2::ToServer::ToServerEvents(events) => {
			for event in events {
				event_demuxer.ingest(Id::parse(&event.checkpoint.actor_id)?, event);
			}
		}
		protocol::mk2::ToServer::ToServerAckCommands(ack) => {
			ack_commands(&ctx, conn.runner_id, ack).await?;
		}
		protocol::mk2::ToServer::ToServerStopping => {
			ctx.signal(pegboard::workflows::runner2::Stop {
				reset_actor_rescheduling: false,
			})
			.to_workflow_id(conn.workflow_id)
			.send()
			.await
			.with_context(|| {
				format!("failed to forward signal to workflow: {}", conn.workflow_id)
			})?;
		}
	}

	Ok(())
}

#[tracing::instrument(skip_all)]
async fn handle_message_mk1(ctx: &StandaloneCtx, conn: &Conn, msg: Bytes) -> Result<()> {
	// HACK: Decode v2 to handle tunnel ack
	if rivet_runner_protocol::compat::version_needs_tunnel_ack(conn.protocol_version) {
		match compat_ack_tunnel_message(&conn, &msg[..]).await {
			Ok(_) => {}
			Err(err) => {
				tracing::error!(?err, "failed to send compat ack tunnel message")
			}
		}
	}

	// Parse message
	let msg = match versioned::ToServer::deserialize(&msg, conn.protocol_version) {
		Ok(x) => x,
		Err(err) => {
			tracing::warn!(?err, msg_len = msg.len(), "failed to deserialize message");
			return Ok(());
		}
	};

	match msg {
		protocol::ToServer::ToServerPing(ping) => {
			let now = util::timestamp::now();

			let delta = if ping.ts <= now {
				// Calculate delta, clamping to u32::MAX if too large
				let delta_ms = now.saturating_sub(ping.ts);
				delta_ms.min(u32::MAX as i64) as u32
			} else {
				// If ping timestamp is in the future (clock skew), default to 0
				tracing::warn!(
					ping_ts = ping.ts,
					now_ts = now,
					"ping timestamp is in the future, possibly due to clock skew"
				);
				0
			};

			// Assuming symmetric delta
			let rtt = delta * 2;

			conn.last_rtt.store(rtt, Ordering::Relaxed);
		}
		// Process KV request
		protocol::ToServer::ToServerKvRequest(req) => {
			let actor_id = match Id::parse(&req.actor_id) {
				Ok(actor_id) => actor_id,
				Err(err) => {
					let res_msg = versioned::ToClient::wrap_latest(
						protocol::ToClient::ToClientKvResponse(protocol::ToClientKvResponse {
							request_id: req.request_id,
							data: protocol::KvResponseData::KvErrorResponse(
								protocol::KvErrorResponse {
									message: err.to_string(),
								},
							),
						}),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV error response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV error response to client")?;

					return Ok(());
				}
			};

			let actor_res = ctx
				.op(pegboard::ops::actor::get_for_runner::Input { actor_id })
				.await
				.with_context(|| format!("failed to get runner for actor: {}", actor_id))?;

			let Some(actor) = actor_res else {
				send_actor_kv_error_mk1(&conn, req.request_id, "actor does not exist").await?;
				return Ok(());
			};

			// Verify actor belongs to this runner
			if actor.runner_id != conn.runner_id {
				send_actor_kv_error_mk1(&conn, req.request_id, "actor does not belong to runner")
					.await?;
				return Ok(());
			}

			let recipient = actor_kv::Recipient {
				actor_id,
				namespace_id: conn.namespace_id,
				name: actor.name,
			};

			// TODO: Add queue and bg thread for processing kv ops
			// Run kv operation
			match req.data {
				protocol::KvRequestData::KvGetRequest(body) => {
					let res = actor_kv::get(&*ctx.udb()?, &recipient, body.keys).await;

					let res_msg = versioned::ToClient::wrap_latest(
						protocol::ToClient::ToClientKvResponse(protocol::ToClientKvResponse {
							request_id: req.request_id,
							data: match res {
								Ok((keys, values, metadata)) => {
									protocol::KvResponseData::KvGetResponse(
										protocol::KvGetResponse {
											keys,
											values,
											metadata: metadata
												.into_iter()
												.map(|x| protocol::KvMetadata {
													version: x.version,
													create_ts: x.update_ts,
												})
												.collect(),
										},
									)
								}
								Err(err) => protocol::KvResponseData::KvErrorResponse(
									protocol::KvErrorResponse {
										// TODO: Don't return actual error?
										message: err.to_string(),
									},
								),
							},
						}),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV get response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV get response to client")?;
				}
				protocol::KvRequestData::KvListRequest(body) => {
					let res = actor_kv::list(
						&*ctx.udb()?,
						&recipient,
						match body.query {
							protocol::KvListQuery::KvListAllQuery => {
								ep::KvListQuery::KvListAllQuery
							}
							protocol::KvListQuery::KvListRangeQuery(q) => {
								ep::KvListQuery::KvListRangeQuery(ep::KvListRangeQuery {
									start: q.start,
									end: q.end,
									exclusive: q.exclusive,
								})
							}
							protocol::KvListQuery::KvListPrefixQuery(q) => {
								ep::KvListQuery::KvListPrefixQuery(ep::KvListPrefixQuery {
									key: q.key,
								})
							}
						},
						body.reverse.unwrap_or_default(),
						body.limit
							.map(TryInto::try_into)
							.transpose()
							.context("KV list limit value overflow")?,
					)
					.await;

					let res_msg = versioned::ToClient::wrap_latest(
						protocol::ToClient::ToClientKvResponse(protocol::ToClientKvResponse {
							request_id: req.request_id,
							data: match res {
								Ok((keys, values, metadata)) => {
									protocol::KvResponseData::KvListResponse(
										protocol::KvListResponse {
											keys,
											values,
											metadata: metadata
												.into_iter()
												.map(|x| protocol::KvMetadata {
													version: x.version,
													create_ts: x.update_ts,
												})
												.collect(),
										},
									)
								}
								Err(err) => protocol::KvResponseData::KvErrorResponse(
									protocol::KvErrorResponse {
										// TODO: Don't return actual error?
										message: err.to_string(),
									},
								),
							},
						}),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV list response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV list response to client")?;
				}
				protocol::KvRequestData::KvPutRequest(body) => {
					let res = actor_kv::put(&*ctx.udb()?, &recipient, body.keys, body.values).await;

					let res_msg = versioned::ToClient::wrap_latest(
						protocol::ToClient::ToClientKvResponse(protocol::ToClientKvResponse {
							request_id: req.request_id,
							data: match res {
								Ok(()) => protocol::KvResponseData::KvPutResponse,
								Err(err) => {
									protocol::KvResponseData::KvErrorResponse(
										protocol::KvErrorResponse {
											// TODO: Don't return actual error?
											message: err.to_string(),
										},
									)
								}
							},
						}),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV put response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV put response to client")?;
				}
				protocol::KvRequestData::KvDeleteRequest(body) => {
					let res = actor_kv::delete(&*ctx.udb()?, &recipient, body.keys).await;

					let res_msg = versioned::ToClient::wrap_latest(
						protocol::ToClient::ToClientKvResponse(protocol::ToClientKvResponse {
							request_id: req.request_id,
							data: match res {
								Ok(()) => protocol::KvResponseData::KvDeleteResponse,
								Err(err) => protocol::KvResponseData::KvErrorResponse(
									protocol::KvErrorResponse {
										// TODO: Don't return actual error?
										message: err.to_string(),
									},
								),
							},
						}),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV delete response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV delete response to client")?;
				}
				protocol::KvRequestData::KvDropRequest => {
					let res = actor_kv::delete_all(&*ctx.udb()?, &recipient).await;

					let res_msg = versioned::ToClient::wrap_latest(
						protocol::ToClient::ToClientKvResponse(protocol::ToClientKvResponse {
							request_id: req.request_id,
							data: match res {
								Ok(()) => protocol::KvResponseData::KvDropResponse,
								Err(err) => protocol::KvResponseData::KvErrorResponse(
									protocol::KvErrorResponse {
										// TODO: Don't return actual error?
										message: err.to_string(),
									},
								),
							},
						}),
					);

					let res_msg_serialized = res_msg
						.serialize(conn.protocol_version)
						.context("failed to serialize KV drop response")?;
					conn.ws_handle
						.send(Message::Binary(res_msg_serialized.into()))
						.await
						.context("failed to send KV drop response to client")?;
				}
			}
		}
		protocol::ToServer::ToServerTunnelMessage(tunnel_msg) => {
			handle_tunnel_message_mk1(
				&ctx.ups()
					.context("failed to get UPS instance for tunnel message")?,
				ctx.config()
					.pegboard()
					.runner_max_response_payload_body_size(),
				&conn.authorized_tunnel_routes,
				tunnel_msg,
			)
			.await
			.context("failed to handle tunnel message")?;
		}
		// Forward to runner wf
		protocol::ToServer::ToServerInit(_)
		| protocol::ToServer::ToServerEvents(_)
		| protocol::ToServer::ToServerAckCommands(_)
		| protocol::ToServer::ToServerStopping => {
			ctx.signal(pegboard::workflows::runner::Forward {
				inner: protocol::ToServer::try_from(msg)
					.context("failed to convert message for workflow forwarding")?,
			})
			.to_workflow_id(conn.workflow_id)
			.send()
			.await
			.with_context(|| {
				format!("failed to forward signal to workflow: {}", conn.workflow_id)
			})?;
		}
	}

	Ok(())
}

async fn ack_commands(
	ctx: &StandaloneCtx,
	runner_id: Id,
	ack: protocol::mk2::ToServerAckCommands,
) -> Result<()> {
	ctx.udb()?
		.txn("runner_ack_commands", |tx| {
			let ack = ack.clone();
			async move {
				let tx = tx.with_subspace(pegboard::keys::subspace());

				for checkpoint in &ack.last_command_checkpoints {
					let start = tx.pack(
						&pegboard::keys::runner::ActorCommandKey::subspace_with_actor(
							runner_id,
							Id::parse(&checkpoint.actor_id)?,
							checkpoint.generation,
						),
					);
					let end = end_of_key_range(&tx.pack(
						&pegboard::keys::runner::ActorCommandKey::subspace_with_index(
							runner_id,
							Id::parse(&checkpoint.actor_id)?,
							checkpoint.generation,
							checkpoint.index,
						),
					));
					tx.clear_range(&start, &end);
				}

				Ok(())
			}
		})
		.await
}

#[tracing::instrument(skip_all)]
async fn handle_tunnel_message_mk2(
	ups: &PubSub,
	max_payload_size: usize,
	authorized_tunnel_routes: &HashMap<(protocol::mk2::GatewayId, protocol::mk2::RequestId), ()>,
	msg: protocol::mk2::ToServerTunnelMessage,
) -> Result<()> {
	let route = (msg.message_id.gateway_id, msg.message_id.request_id);
	let clear_route = should_clear_tunnel_route_mk2(&msg.message_kind);

	// Extract inner data length before consuming msg
	let inner_data_len = tunnel_message_inner_data_len_mk2(&msg.message_kind);

	// Enforce incoming payload size
	if inner_data_len > max_payload_size {
		return Err(errors::WsError::InvalidPacket("payload too large".to_string()).build());
	}

	// if !authorized_tunnel_routes.contains_async(&route).await {
	// 	return Err(
	// 		errors::WsError::InvalidPacket("unauthorized tunnel message".to_string()).build(),
	// 	);
	// }

	let gateway_reply_to = GatewayReceiverSubject::new(msg.message_id.gateway_id);
	let msg_serialized =
		versioned::ToGateway::wrap_latest(protocol::mk2::ToGateway::ToServerTunnelMessage(msg))
			.serialize_with_embedded_version(PROTOCOL_MK2_VERSION)
			.context("failed to serialize tunnel message for gateway")?;

	tracing::trace!(
		inner_data_len = inner_data_len,
		serialized_len = msg_serialized.len(),
		"publishing tunnel message to gateway"
	);

	// Publish message to UPS
	ups.publish(&gateway_reply_to, &msg_serialized, PublishOpts::one())
		.await
		.with_context(|| {
			format!(
				"failed to publish tunnel message to gateway reply topic: {}",
				gateway_reply_to
			)
		})?;

	if clear_route {
		authorized_tunnel_routes.remove_async(&route).await;
	}

	Ok(())
}

#[tracing::instrument(skip_all)]
async fn handle_tunnel_message_mk1(
	ups: &PubSub,
	max_payload_size: usize,
	authorized_tunnel_routes: &HashMap<(protocol::mk2::GatewayId, protocol::mk2::RequestId), ()>,
	msg: protocol::ToServerTunnelMessage,
) -> Result<()> {
	let route = (msg.message_id.gateway_id, msg.message_id.request_id);
	let clear_route = should_clear_tunnel_route_mk1(&msg.message_kind);

	// Ignore DeprecatedTunnelAck messages (used only for backwards compatibility)
	if matches!(
		msg.message_kind,
		protocol::ToServerTunnelMessageKind::DeprecatedTunnelAck
	) {
		return Ok(());
	}

	// Extract inner data length before consuming msg
	let inner_data_len = tunnel_message_inner_data_len_mk1(&msg.message_kind);

	// Enforce incoming payload size
	if inner_data_len > max_payload_size {
		return Err(errors::WsError::InvalidPacket("payload too large".to_string()).build());
	}

	// if !authorized_tunnel_routes.contains_async(&route).await {
	// 	return Err(
	// 		errors::WsError::InvalidPacket("unauthorized tunnel message".to_string()).build(),
	// 	);
	// }

	// Publish message to UPS
	let gateway_reply_to = GatewayReceiverSubject::new(msg.message_id.gateway_id);
	let msg_serialized = versioned::ToGateway::v3_to_v7(versioned::ToGateway::V3(
		protocol::ToGateway::ToServerTunnelMessage(msg),
	))?
	.serialize_with_embedded_version(PROTOCOL_MK2_VERSION)
	.context("failed to serialize tunnel message for gateway")?;
	ups.publish(&gateway_reply_to, &msg_serialized, PublishOpts::one())
		.await
		.with_context(|| {
			format!(
				"failed to publish tunnel message to gateway reply topic: {}",
				gateway_reply_to
			)
		})?;

	if clear_route {
		authorized_tunnel_routes.remove_async(&route).await;
	}

	Ok(())
}

fn should_clear_tunnel_route_mk2(msg_kind: &protocol::mk2::ToServerTunnelMessageKind) -> bool {
	match msg_kind {
		protocol::mk2::ToServerTunnelMessageKind::ToServerResponseStart(response) => {
			!response.stream
		}
		protocol::mk2::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => chunk.finish,
		protocol::mk2::ToServerTunnelMessageKind::ToServerResponseAbort
		| protocol::mk2::ToServerTunnelMessageKind::ToServerWebSocketClose(_) => true,
		_ => false,
	}
}

fn should_clear_tunnel_route_mk1(msg_kind: &protocol::ToServerTunnelMessageKind) -> bool {
	match msg_kind {
		protocol::ToServerTunnelMessageKind::ToServerResponseStart(response) => !response.stream,
		protocol::ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => chunk.finish,
		protocol::ToServerTunnelMessageKind::ToServerResponseAbort
		| protocol::ToServerTunnelMessageKind::ToServerWebSocketClose(_) => true,
		_ => false,
	}
}

/// Returns the length of the inner data payload for a tunnel message kind.
fn tunnel_message_inner_data_len_mk2(kind: &protocol::mk2::ToServerTunnelMessageKind) -> usize {
	use protocol::mk2::ToServerTunnelMessageKind;
	match kind {
		ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			resp.body.as_ref().map_or(0, |b| b.len())
		}
		ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => chunk.body.len(),
		ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => msg.data.len(),
		ToServerTunnelMessageKind::ToServerResponseAbort
		| ToServerTunnelMessageKind::ToServerWebSocketOpen(_)
		| ToServerTunnelMessageKind::ToServerWebSocketMessageAck(_)
		| ToServerTunnelMessageKind::ToServerWebSocketClose(_) => 0,
	}
}

/// Returns the length of the inner data payload for a tunnel message kind.
fn tunnel_message_inner_data_len_mk1(kind: &protocol::ToServerTunnelMessageKind) -> usize {
	use protocol::ToServerTunnelMessageKind;
	match kind {
		ToServerTunnelMessageKind::ToServerResponseStart(resp) => {
			resp.body.as_ref().map_or(0, |b| b.len())
		}
		ToServerTunnelMessageKind::ToServerResponseChunk(chunk) => chunk.body.len(),
		ToServerTunnelMessageKind::ToServerWebSocketMessage(msg) => msg.data.len(),
		ToServerTunnelMessageKind::ToServerResponseAbort
		| ToServerTunnelMessageKind::ToServerWebSocketOpen(_)
		| ToServerTunnelMessageKind::ToServerWebSocketMessageAck(_)
		| ToServerTunnelMessageKind::ToServerWebSocketClose(_)
		| ToServerTunnelMessageKind::DeprecatedTunnelAck => 0,
	}
}

#[cfg(test)]
#[path = "../tests/support/ws_to_tunnel_task.rs"]
mod tests;

/// Send ack message for deprecated tunnel versions.
///
/// We have to parse as specifically a v2 message since we need the exact request & message ID
/// provided by the user and not available in v3.
async fn compat_ack_tunnel_message(conn: &Conn, payload: &[u8]) -> Result<()> {
	use rivet_runner_protocol::generated::v2 as protocol_v2;

	// Parse payload
	let msg: protocol_v2::ToServer = rivet_util::serde::bare_from_slice!(&payload)?;
	let protocol_v2::ToServer::ToServerTunnelMessage(msg) = msg else {
		return Ok(());
	};

	tracing::debug!(?msg.request_id, ?msg.message_id, "sending v2 compat tunnel ack");

	// Serialize response
	let ack_msg = rivet_util::serde::bare_to_vec!(&protocol_v2::ToClient::ToClientTunnelMessage(
		protocol_v2::ToClientTunnelMessage {
			request_id: msg.request_id,
			message_id: msg.message_id,
			message_kind: protocol_v2::ToClientTunnelMessageKind::TunnelAck,
			gateway_reply_to: None,
		},
	))?;

	conn.ws_handle
		.send(hyper_tungstenite::tungstenite::Message::Binary(
			ack_msg.into(),
		))
		.await
		.context("failed to send DeprecatedTunnelAck to runner")?;

	Ok(())
}

async fn send_actor_kv_error(conn: &Conn, request_id: u32, message: &str) -> Result<()> {
	let res_msg = versioned::ToClientMk2::wrap_latest(protocol::mk2::ToClient::ToClientKvResponse(
		protocol::mk2::ToClientKvResponse {
			request_id,
			data: protocol::mk2::KvResponseData::KvErrorResponse(protocol::mk2::KvErrorResponse {
				message: message.to_string(),
			}),
		},
	));

	let res_msg_serialized = res_msg
		.serialize(conn.protocol_version)
		.context("failed to serialize KV actor validation error")?;
	conn.ws_handle
		.send(Message::Binary(res_msg_serialized.into()))
		.await
		.context("failed to send KV actor validation error to client")?;

	Ok(())
}

async fn send_actor_kv_error_mk1(conn: &Conn, request_id: u32, message: &str) -> Result<()> {
	let res_msg = versioned::ToClient::wrap_latest(protocol::ToClient::ToClientKvResponse(
		protocol::ToClientKvResponse {
			request_id,
			data: protocol::KvResponseData::KvErrorResponse(protocol::KvErrorResponse {
				message: message.to_string(),
			}),
		},
	));

	let res_msg_serialized = res_msg
		.serialize(conn.protocol_version)
		.context("failed to serialize KV actor validation error")?;
	conn.ws_handle
		.send(Message::Binary(res_msg_serialized.into()))
		.await
		.context("failed to send KV actor validation error to client")?;

	Ok(())
}
