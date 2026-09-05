use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use parking_lot::Mutex;
use rivet_error::{
	MacroMarker, RivetError as RivetTransportError, RivetErrorKind, RivetErrorSchema,
};
use rivetkit_core::{
	ActorContext as CoreActorContext, ActorEvent, ActorEvents, ActorLifecycle, ActorStart,
	QueueSendResult, QueueSendStatus, Reply, SerializeStateReason, StateDelta,
};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::NapiInvalidState;
#[cfg(test)]
use crate::actor_context::EndReason;
use crate::actor_context::{ActorContext, RegisteredTask, state_deltas_from_payload};
use crate::actor_factory::{
	ActionPayload, AdapterConfig, BeforeActionResponsePayload, BeforeConnectPayload,
	BeforeSubscribePayload, CallbackBindings, ConnectionPayload, CreateConnStatePayload,
	CreateStatePayload, LifecyclePayload, MigratePayload, QueueSendPayload, SerializeStatePayload,
	WebSocketPayload, WorkflowHistoryPayload, WorkflowReplayPayload, call_buffer,
	call_optional_buffer, call_queue_send, call_state_delta_payload, call_void,
};
use crate::http::{HttpRequestPayload, call_request};

// Restart hooks are synchronous callback slots; the guard is only held while
// swapping task handles, never while awaiting a task shutdown.
type RunHandlerSlot = Arc<Mutex<Option<JoinHandle<()>>>>;

struct RunHandlerActiveGuard {
	ctx: CoreActorContext,
}

struct DispatchCancelGuard {
	token: CancellationToken,
}

impl RunHandlerActiveGuard {
	fn new(ctx: CoreActorContext) -> Self {
		ctx.begin_run_handler();
		Self { ctx }
	}
}

impl DispatchCancelGuard {
	fn new() -> Self {
		Self {
			token: CancellationToken::new(),
		}
	}

	fn token(&self) -> CancellationToken {
		self.token.clone()
	}
}

impl Drop for RunHandlerActiveGuard {
	fn drop(&mut self) {
		self.ctx.end_run_handler();
	}
}

async fn restore_suppressed_run_wake<F, Fut>(
	run_handler_active: RunHandlerActiveGuard,
	callback_accepted: bool,
	run_wake: Option<(i64, u64)>,
	restore: F,
) -> Result<()>
where
	F: FnOnce(i64, u64) -> Fut,
	Fut: std::future::Future<Output = Result<()>>,
{
	// Restore only after clearing the active marker. Otherwise an immediately
	// due alarm can be consumed under the assumption that the run is active.
	drop(run_handler_active);
	if !callback_accepted && let Some((wake_at, wake_revision)) = run_wake {
		restore(wake_at, wake_revision).await?;
	}
	Ok(())
}

impl Drop for DispatchCancelGuard {
	fn drop(&mut self) {
		self.token.cancel();
	}
}

static ACTION_TIMED_OUT_SCHEMA: RivetErrorSchema = RivetErrorSchema {
	group: "actor",
	code: "action_timed_out",
	default_message: "Action timed out",
	meta_type: None,
	_macro_marker: MacroMarker { _private: () },
};

static CALLBACK_TIMED_OUT_SCHEMA: RivetErrorSchema = RivetErrorSchema {
	group: "actor",
	code: "callback_timed_out",
	default_message: "Lifecycle callback timed out",
	meta_type: None,
	_macro_marker: MacroMarker { _private: () },
};

pub(crate) async fn run_adapter_loop(
	bindings: Arc<CallbackBindings>,
	config: Arc<AdapterConfig>,
	start: ActorStart,
) -> Result<()> {
	let ActorStart {
		ctx: core_ctx,
		input,
		is_new,
		snapshot,
		hibernated,
		mut events,
		startup_ready,
	} = start;

	let ctx = ActorContext::new(core_ctx.clone());
	ctx.reset_runtime_shared_state();
	let abort = CancellationToken::new();
	ctx.attach_napi_abort_token(abort.clone());
	let (registered_task_tx, mut registered_task_rx) = unbounded_channel();
	ctx.attach_task_sender(registered_task_tx);

	let dirty = Arc::new(AtomicBool::new(false));
	core_ctx.on_request_save(Box::new({
		let dirty = Arc::clone(&dirty);
		move |_opts| {
			dirty.store(true, Ordering::Release);
		}
	}));

	let mut tasks = JoinSet::new();
	let run_handler = match run_preamble(
		&bindings,
		config.as_ref(),
		&ctx,
		input.as_deref(),
		is_new,
		snapshot,
		hibernated,
	)
	.await
	{
		Ok(run_handler) => {
			if let Some(reply) = startup_ready {
				let _ = reply.send(Ok(()));
			}
			run_handler
		}
		Err(error) => {
			if let Some(reply) = startup_ready {
				let startup_error = anyhow::Error::new(RivetTransportError::extract(&error));
				let _ = reply.send(Err(startup_error));
			}
			return Err(error);
		}
	};

	run_event_loop(
		&bindings,
		config.as_ref(),
		&ctx,
		&abort,
		&mut tasks,
		&mut registered_task_rx,
		&dirty,
		&mut events,
	)
	.await;

	stop_run_handler(&run_handler).await;
	abort.cancel();
	drain_tasks(&mut tasks, &mut registered_task_rx).await;
	Ok(())
}

async fn run_event_loop(
	bindings: &Arc<CallbackBindings>,
	config: &AdapterConfig,
	ctx: &ActorContext,
	abort: &CancellationToken,
	tasks: &mut JoinSet<()>,
	registered_task_rx: &mut UnboundedReceiver<RegisteredTask>,
	dirty: &Arc<AtomicBool>,
	events: &mut ActorEvents,
) {
	loop {
		pump_registered_tasks(tasks, registered_task_rx);

		tokio::select! {
			// Reap completed background tasks as they finish. A tokio JoinSet
			// retains each finished task's allocation until it is joined, so
			// without this the set grows for the entire actor lifetime and
			// shows up as native (non-V8) RSS growth.
			Some(result) = tasks.join_next(), if !tasks.is_empty() => {
				if let Err(error) = result {
					if !error.is_cancelled() {
						tracing::error!(?error, "napi background task failed to join");
					}
				}
			}
			event = events.recv() => {
				let Some(event) = event else {
					break;
				};
				dispatch_event(
					event,
					bindings,
					config,
					ctx,
					abort,
					tasks,
					registered_task_rx,
					dirty,
				)
				.await;
				if ctx.has_end_reason() {
					break;
				}
			}
		}
	}
}

async fn run_preamble(
	bindings: &CallbackBindings,
	config: &AdapterConfig,
	ctx: &ActorContext,
	input: Option<&[u8]>,
	is_new: bool,
	snapshot: Option<Vec<u8>>,
	hibernated: Vec<(rivetkit_core::ConnHandle, Vec<u8>)>,
) -> Result<RunHandlerSlot> {
	let snapshot = normalize_startup_snapshot(bindings.create_state.is_some(), snapshot);

	// Run database migrations before any user lifecycle hook so `c.db` is
	// usable from createState, onCreate, and createVars.
	if let Some(callback) = &bindings.on_migrate {
		with_timeout(
			"onMigrate",
			config.on_migrate_timeout,
			call_on_migrate(callback, ctx, is_new),
		)
		.await?;
	}

	if is_new {
		if let Some(callback) = &bindings.create_state {
			let bytes = with_timeout(
				"createState",
				config.create_state_timeout,
				call_create_state(callback, ctx, input),
			)
			.await?;
			ctx.set_state_initial(bytes)?;
		}
		if let Some(callback) = &bindings.on_create {
			with_timeout(
				"onCreate",
				config.on_create_timeout,
				call_on_create(callback, ctx, input),
			)
			.await?;
		}
		ctx.mark_has_initialized_and_flush().await?;
	} else if let Some(snapshot) = snapshot {
		ctx.set_state_initial(snapshot)?;
	} else if let Some(callback) = &bindings.create_state {
		let bytes = with_timeout(
			"createState",
			config.create_state_timeout,
			call_create_state(callback, ctx, input),
		)
		.await?;
		ctx.set_state_initial(bytes)?;
		ctx.mark_has_initialized_and_flush().await?;
	} else {
		return Err(NapiInvalidState {
			state: "actor wake snapshot".to_owned(),
			reason: "wake path did not include a persisted snapshot".to_owned(),
		}
		.build());
	}

	if !is_new {
		for (conn, bytes) in hibernated {
			ctx.restore_hibernatable_conn(conn, bytes)?;
		}
	}

	if let Some(callback) = &bindings.create_vars {
		with_timeout(
			"createVars",
			config.create_vars_timeout,
			call_create_vars(callback, ctx),
		)
		.await?;
	}

	if let Some(callback) = &bindings.on_wake {
		with_timeout(
			"onWake",
			config.on_wake_timeout,
			call_on_wake(callback, ctx),
		)
		.await?;
	}

	if let Some(callback) = &bindings.on_before_actor_start {
		with_timeout(
			"onBeforeActorStart",
			config.on_before_actor_start_timeout,
			call_on_before_actor_start(callback, ctx),
		)
		.await?;
	}

	let run_handler = configure_run_handler(bindings, ctx);
	if bindings.run.is_some() {
		tokio::task::yield_now().await;
	}

	Ok(run_handler)
}

fn normalize_startup_snapshot(
	has_create_state: bool,
	snapshot: Option<Vec<u8>>,
) -> Option<Vec<u8>> {
	// Empty state with createState means a previous process persisted
	// initialization before the runtime produced initial state.
	match snapshot {
		Some(bytes) if bytes.is_empty() && has_create_state => None,
		other => other,
	}
}

fn configure_run_handler(bindings: &CallbackBindings, ctx: &ActorContext) -> RunHandlerSlot {
	let run_handler = Arc::new(Mutex::new(None));
	let Some(callback) = bindings.run.as_ref().cloned() else {
		return run_handler;
	};

	let restart_slot = Arc::clone(&run_handler);
	let restart_ctx = ctx.clone();
	let restart_callback = callback.clone();

	ctx.attach_run_restart(move |run_wake| {
		let mut guard = restart_slot.lock();
		if let Some(handle) = guard.take() {
			handle.abort();
		}
		*guard = Some(spawn_run_handler(
			restart_callback.clone(),
			restart_ctx.clone(),
			run_wake,
		));
		Ok(())
	});

	{
		let mut guard = run_handler.lock();
		*guard = Some(spawn_run_handler(callback, ctx.clone(), None));
	}

	run_handler
}

#[tracing::instrument(
	skip_all,
	fields(actor_id = %ctx.inner().actor_id()),
)]
pub(crate) async fn dispatch_event(
	event: ActorEvent,
	bindings: &Arc<CallbackBindings>,
	config: &AdapterConfig,
	ctx: &ActorContext,
	abort: &CancellationToken,
	tasks: &mut JoinSet<()>,
	_registered_task_rx: &mut UnboundedReceiver<RegisteredTask>,
	dirty: &Arc<AtomicBool>,
) {
	let _ = dirty;

	match event {
		ActorEvent::Action {
			name,
			args,
			conn,
			scheduled_fire,
			reply,
		} => {
			let invocation_telemetry = reply.invocation_telemetry();
			tracing::info!(
				actor_id = %ctx.inner().actor_id(),
				action_name = %name,
				args_len = args.len(),
				has_conn = conn.is_some(),
				"napi: dispatching ActorEvent::Action to JS"
			);
			let Some(callback) = bindings.actions.get(&name).cloned() else {
				tracing::warn!(
					actor_id = %ctx.inner().actor_id(),
					action_name = %name,
					"napi: no action callback registered",
				);
				reply.send(Err(action_not_found(name)));
				return;
			};
			let on_before_action_response = bindings.on_before_action_response.clone();
			let timeout = config.action_timeout;
			let ctx = ctx.clone();

			spawn_reply(tasks, abort.clone(), reply, async move {
				tracing::info!(action_name = %name, "napi: invoking action JS callback");
				let output = with_dispatch_cancel_token(|cancel_token| {
					with_structured_timeout(
						"actor",
						"action_timed_out",
						"Action timed out",
						None,
						timeout,
						call_action(
							&callback,
							&ctx,
							invocation_telemetry,
							conn,
							name.clone(),
							args.clone(),
							scheduled_fire.clone(),
							Some(cancel_token),
						),
					)
				})
				.await?;
				tracing::info!(
					action_name = %name,
					output_len = output.len(),
					"napi: action JS callback returned"
				);

				if let Some(callback) = on_before_action_response {
					with_structured_timeout(
						"actor",
						"action_timed_out",
						"Action timed out",
						None,
						timeout,
						call_on_before_action_response(&callback, &ctx, name, args, output),
					)
					.await
				} else {
					Ok(output)
				}
			});
		}
		ActorEvent::HttpRequest { request, reply } => {
			let Some(callback) = bindings.on_request.clone() else {
				reply.send(Err(missing_callback("onRequest")));
				return;
			};
			let ctx = ctx.clone();
			let timeout = config.on_request_timeout;
			spawn_reply(tasks, abort.clone(), reply, async move {
				with_structured_timeout(
					"actor",
					"action_timed_out",
					"Action timed out",
					None,
					timeout,
					call_http_request(&callback, &ctx, request),
				)
				.await
			});
		}
		ActorEvent::QueueSend {
			name,
			body,
			conn,
			request,
			wait,
			timeout_ms,
			reply,
		} => {
			let Some(callback) = bindings.on_queue_send.clone() else {
				reply.send(Err(missing_callback("onQueueSend")));
				return;
			};
			let ctx = ctx.clone();
			let timeout = config.on_request_timeout;
			spawn_reply(tasks, abort.clone(), reply, async move {
				with_dispatch_cancel_token(|cancel_token| {
					with_structured_timeout(
						"actor",
						"action_timed_out",
						"Action timed out",
						None,
						timeout,
						async move {
							let result = call_queue_send(
								"onQueueSend",
								&callback,
								QueueSendPayload {
									ctx: ctx.inner().clone(),
									conn,
									request,
									name,
									body,
									wait,
									timeout_ms,
									cancel_token: Some(cancel_token),
								},
							)
							.await?;
							let status = match result.status.as_str() {
								"completed" => QueueSendStatus::Completed,
								"timedOut" => QueueSendStatus::TimedOut,
								other => {
									return Err(NapiInvalidState {
										state: "queue send status".to_owned(),
										reason: format!("invalid status `{other}`"),
									}
									.build());
								}
							};
							Ok(QueueSendResult {
								status,
								response: result.response.map(|buffer| buffer.to_vec()),
							})
						},
					)
				})
				.await
			});
		}
		ActorEvent::WebSocketOpen {
			conn,
			ws,
			request,
			reply,
		} => {
			let Some(callback) = bindings.on_websocket.clone() else {
				reply.send(Ok(()));
				return;
			};
			let ctx = ctx.clone();
			spawn_reply(tasks, abort.clone(), reply, async move {
				call_on_websocket(&callback, &ctx, conn, ws, request).await
			});
		}
		ActorEvent::ConnectionPreflight {
			conn,
			params,
			request,
			reply,
		} => {
			let on_before_connect = bindings.on_before_connect.clone();
			let create_conn_state = bindings.create_conn_state.clone();
			let timeout = config.on_before_connect_timeout;
			let create_conn_state_timeout = config.create_conn_state_timeout;
			let ctx = ctx.clone();

			spawn_reply(tasks, abort.clone(), reply, async move {
				if let Some(callback) = on_before_connect {
					with_timeout(
						"onBeforeConnect",
						timeout,
						call_on_before_connect(&callback, &ctx, params.clone(), request.clone()),
					)
					.await?;
				}

				if let Some(callback) = create_conn_state {
					let state = with_timeout(
						"createConnState",
						create_conn_state_timeout,
						call_create_conn_state(
							&callback,
							&ctx,
							conn.clone(),
							params.clone(),
							request.clone(),
						),
					)
					.await?;
					ctx.set_conn_state_initial(&conn, state)?;
				}

				Ok(())
			});
		}
		ActorEvent::ConnectionOpen {
			conn,
			request,
			reply,
		} => {
			let on_connect = bindings.on_connect.clone();
			let connect_timeout = config.on_connect_timeout;
			let ctx = ctx.clone();

			spawn_reply(tasks, abort.clone(), reply, async move {
				if let Some(callback) = on_connect {
					with_timeout(
						"onConnect",
						connect_timeout,
						call_on_connect(&callback, &ctx, conn, request),
					)
					.await?;
				}

				Ok(())
			});
		}
		ActorEvent::ConnectionClosed { conn } => {
			let Some(callback) = bindings.on_disconnect_final.clone() else {
				return;
			};
			let ctx = ctx.clone();
			let actor_id = ctx.inner().actor_id().to_owned();
			let timeout = config.on_connect_timeout;
			spawn_task(tasks, abort.clone(), actor_id, async move {
				with_timeout(
					"onDisconnect",
					timeout,
					call_on_disconnect_final(&callback, &ctx, conn),
				)
				.await
			});
		}
		ActorEvent::SubscribeRequest {
			conn,
			event_name,
			reply,
		} => {
			let Some(callback) = bindings.on_before_subscribe.clone() else {
				reply.send(Ok(()));
				return;
			};
			let ctx = ctx.clone();
			let timeout = config.on_before_connect_timeout;
			spawn_reply(tasks, abort.clone(), reply, async move {
				with_timeout(
					"onBeforeSubscribe",
					timeout,
					call_on_before_subscribe(&callback, &ctx, conn, event_name),
				)
				.await
			});
		}
		ActorEvent::SerializeState { reason, reply } => {
			reply.send(maybe_serialize(bindings.as_ref(), ctx, dirty.as_ref(), reason).await);
		}
		ActorEvent::RunGracefulCleanup { reason, reply } => {
			let callback = match reason {
				rivetkit_core::actor::ShutdownKind::Sleep => bindings.on_sleep.clone(),
				rivetkit_core::actor::ShutdownKind::Destroy => bindings.on_destroy.clone(),
			};
			let ctx = ctx.clone();
			let shutdown_deadline = ctx.inner().shutdown_deadline_token();
			tasks.spawn(async move {
				let work = async {
					let result: Result<()> = async {
						if let Some(callback) = callback {
							match reason {
								rivetkit_core::actor::ShutdownKind::Sleep => {
									call_on_sleep(&callback, &ctx).await
								}
								rivetkit_core::actor::ShutdownKind::Destroy => {
									call_on_destroy(&callback, &ctx).await
								}
							}?;
						}
						Ok(())
					}
					.await;
					if let Err(error) = result {
						tracing::error!(
							actor_id = %ctx.inner().actor_id(),
							?error,
							"graceful cleanup callback failed",
						);
					}
				};
				tokio::select! {
					_ = work => {}
					_ = shutdown_deadline.cancelled() => {
						tracing::warn!(
							actor_id = %ctx.inner().actor_id(),
							reason = ?reason,
							"graceful cleanup aborted by shutdown grace deadline",
						);
					}
				}
				reply.send(Ok(()));
			});
		}
		ActorEvent::DisconnectConn { conn_id, reply } => {
			let callback = bindings.on_disconnect_final.clone();
			let ctx = ctx.clone();
			tasks.spawn(async move {
				let result: Result<()> = async {
					let conn = { ctx.inner().conns().find(|conn| conn.id() == conn_id) };
					if let Some(conn) = conn {
						if let Some(callback) = callback {
							call_on_disconnect_final(&callback, &ctx, conn.clone()).await?;
						}
						ctx.inner().disconnect_conn(conn_id).await?;
					}
					Ok(())
				}
				.await;
				if let Err(error) = result {
					tracing::error!(
						actor_id = %ctx.inner().actor_id(),
						?error,
						"disconnect cleanup callback failed",
					);
				}
				reply.send(Ok(()));
			});
		}
		ActorEvent::WorkflowHistoryRequested { reply } => {
			let Some(callback) = bindings.get_workflow_history.clone() else {
				// Dropped means unsupported; `Ok(None)` means supported without history.
				drop(reply);
				return;
			};
			let ctx = ctx.clone();
			spawn_reply(tasks, abort.clone(), reply, async move {
				call_workflow_history(&callback, &ctx).await
			});
		}
		ActorEvent::WorkflowReplayRequested { entry_id, reply } => {
			let Some(callback) = bindings.replay_workflow.clone() else {
				drop(reply);
				return;
			};
			let ctx = ctx.clone();
			spawn_reply(tasks, abort.clone(), reply, async move {
				call_workflow_replay(&callback, &ctx, entry_id).await
			});
		}
		ActorEvent::RunWake {
			wake_at,
			wake_revision,
			reply,
		} => {
			let result = if bindings.run.is_none() || ctx.inner().run_handler_active() {
				Ok(())
			} else {
				ctx.restart_run_handler_for_wake(wake_at, wake_revision)
			};
			reply.send(result);
		}
	}
}

async fn maybe_serialize(
	bindings: &CallbackBindings,
	ctx: &ActorContext,
	dirty: &AtomicBool,
	reason: SerializeStateReason,
) -> Result<Vec<StateDelta>> {
	// The adapter dirty bit is consumed only by persistence-bound serialization.
	// Inspector snapshots feed the live overlay and must not steal a pending save.
	maybe_serialize_with(
		bindings,
		ctx,
		dirty,
		reason,
		|bindings, ctx, reason| async move { call_serialize_state(bindings, ctx, reason).await },
	)
	.await
}

async fn maybe_serialize_with<'a, F, Fut>(
	bindings: &'a CallbackBindings,
	ctx: &'a ActorContext,
	dirty: &AtomicBool,
	reason: SerializeStateReason,
	serialize: F,
) -> Result<Vec<StateDelta>>
where
	F: FnOnce(&'a CallbackBindings, &'a ActorContext, &'static str) -> Fut,
	Fut: std::future::Future<Output = Result<Vec<StateDelta>>> + 'a,
{
	if reason != SerializeStateReason::Inspector && !dirty.swap(false, Ordering::AcqRel) {
		return Ok(Vec::new());
	}

	serialize(bindings, ctx, serialize_state_reason_name(reason)).await
}

async fn call_serialize_state(
	bindings: &CallbackBindings,
	ctx: &ActorContext,
	reason: &'static str,
) -> Result<Vec<StateDelta>> {
	let callback = bindings
		.serialize_state
		.as_ref()
		.ok_or_else(|| missing_callback("serializeState"))?;
	let payload = call_state_delta_payload(
		"serializeState",
		callback,
		SerializeStatePayload {
			ctx: ctx.inner().clone(),
			reason: reason.to_owned(),
		},
	)
	.await?;
	Ok(state_deltas_from_payload(payload))
}

fn serialize_state_reason_name(reason: SerializeStateReason) -> &'static str {
	match reason {
		SerializeStateReason::Save => "save",
		SerializeStateReason::Inspector => "inspector",
	}
}

#[allow(dead_code)]
pub(crate) fn spawn_reply<T, F>(
	tasks: &mut JoinSet<()>,
	abort: CancellationToken,
	reply: Reply<T>,
	work: F,
) where
	T: Send + 'static,
	F: std::future::Future<Output = Result<T>> + Send + 'static,
{
	tasks.spawn(async move {
		tokio::select! {
			_ = abort.cancelled() => {
				reply.send(Err(actor_shutting_down()));
			}
			result = work => {
				reply.send(result);
			}
		}
	});
}

fn spawn_task<F>(tasks: &mut JoinSet<()>, abort: CancellationToken, actor_id: String, work: F)
where
	F: std::future::Future<Output = Result<()>> + Send + 'static,
{
	tasks.spawn(async move {
		tokio::select! {
			_ = abort.cancelled() => {}
			result = work => {
				if let Err(error) = result {
					tracing::error!(actor_id, ?error, "napi background callback failed");
				}
			}
		}
	});
}

pub(crate) async fn drain_tasks(
	tasks: &mut JoinSet<()>,
	registered_task_rx: &mut UnboundedReceiver<RegisteredTask>,
) {
	loop {
		pump_registered_tasks(tasks, registered_task_rx);

		if tasks.is_empty() {
			break;
		}

		if let Some(result) = tasks.join_next().await {
			if let Err(error) = result {
				tracing::error!(?error, "napi background task failed to join");
			}
		}
	}
}

fn pump_registered_tasks(
	tasks: &mut JoinSet<()>,
	registered_task_rx: &mut UnboundedReceiver<RegisteredTask>,
) {
	while let Ok(task) = registered_task_rx.try_recv() {
		tasks.spawn(task);
	}
}

async fn stop_run_handler(run_handler: &RunHandlerSlot) {
	let handle = {
		let mut guard = run_handler.lock();
		guard.take()
	};

	if let Some(handle) = handle {
		handle.abort();
		let _ = handle.await;
	}
}

async fn with_timeout<T, F>(callback_name: &str, duration: Duration, future: F) -> Result<T>
where
	F: std::future::Future<Output = Result<T>>,
{
	with_structured_timeout(
		"actor",
		"callback_timed_out",
		format!(
			"callback `{callback_name}` timed out after {} ms",
			duration.as_millis()
		),
		callback_timeout_metadata(callback_name, duration),
		duration,
		future,
	)
	.await
}

async fn with_structured_timeout<T, F>(
	group: &'static str,
	code: &'static str,
	message: impl Into<String>,
	meta: Option<Box<serde_json::value::RawValue>>,
	duration: Duration,
	future: F,
) -> Result<T>
where
	F: std::future::Future<Output = Result<T>>,
{
	let message = message.into();
	let kind = structured_timeout_kind(group, code, &message);
	tokio::time::timeout(duration, future)
		.await
		.map_err(|_| structured_timeout_error(kind, message, meta))?
}

fn structured_timeout_kind(
	group: &'static str,
	code: &'static str,
	message: &str,
) -> RivetErrorKind {
	match (group, code) {
		("actor", "action_timed_out") => RivetErrorKind::Static(&ACTION_TIMED_OUT_SCHEMA),
		("actor", "callback_timed_out") => RivetErrorKind::Static(&CALLBACK_TIMED_OUT_SCHEMA),
		_ => RivetErrorKind::Dynamic {
			group: group.to_owned(),
			code: code.to_owned(),
			default_message: message.to_owned(),
		},
	}
}

fn structured_timeout_error(
	kind: RivetErrorKind,
	message: impl Into<String>,
	meta: Option<Box<serde_json::value::RawValue>>,
) -> anyhow::Error {
	anyhow::Error::new(RivetTransportError {
		kind,
		meta,
		message: Some(message.into()),
		actor: None,
	})
}

fn callback_timeout_metadata(
	callback_name: &str,
	duration: Duration,
) -> Option<Box<serde_json::value::RawValue>> {
	let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
	let metadata = serde_json::json!({
		"callback_name": callback_name,
		"duration_ms": duration_ms,
	});

	serde_json::value::to_raw_value(&metadata).ok()
}

fn spawn_run_handler(
	callback: crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: ActorContext,
	run_wake: Option<(i64, u64)>,
) -> JoinHandle<()> {
	let run_handler_active = RunHandlerActiveGuard::new(ctx.inner().clone());
	tokio::spawn(async move {
		let result = call_run(&callback, &ctx).await;
		match result {
			Ok(callback_accepted) => {
				let core_ctx = ctx.inner().clone();
				if let Err(error) = restore_suppressed_run_wake(
					run_handler_active,
					callback_accepted,
					run_wake,
					move |wake_at, wake_revision| async move {
						core_ctx
							.restore_run_wake_at_if_unchanged(wake_at, wake_revision)
							.await
							.map(|_| ())
					},
				)
				.await
				{
					tracing::error!(
						actor_id = %ctx.inner().actor_id(),
						?run_wake,
						?error,
						"failed to restore run wake suppressed by Inspector replay"
					);
				}
				if !callback_accepted {
					return;
				}
				tracing::debug!(
					actor_id = %ctx.inner().actor_id(),
					"napi run handler exited cleanly"
				);
			}
			Err(error) => {
				drop(run_handler_active);
				let extracted = RivetTransportError::extract(&error);
				if extracted.group() == "actor" && extracted.code() == "aborted" {
					// The run handler was unwound because the actor is going to
					// sleep. This is the expected exit, not a failure.
					tracing::debug!(
						actor_id = %ctx.inner().actor_id(),
						"napi run handler aborted for sleep"
					);
				} else {
					tracing::error!(
						actor_id = %ctx.inner().actor_id(),
						?error,
						"napi run handler failed"
					);
				}
			}
		}
	})
}

async fn call_create_state(
	callback: &crate::actor_factory::CallbackTsfn<CreateStatePayload>,
	ctx: &ActorContext,
	input: Option<&[u8]>,
) -> Result<Vec<u8>> {
	call_buffer(
		"createState",
		callback,
		CreateStatePayload {
			ctx: ctx.inner().clone(),
			input: input.map(|input| input.to_vec()),
		},
	)
	.await
}

async fn call_on_create(
	callback: &crate::actor_factory::CallbackTsfn<CreateStatePayload>,
	ctx: &ActorContext,
	input: Option<&[u8]>,
) -> Result<()> {
	call_void(
		"onCreate",
		callback,
		CreateStatePayload {
			ctx: ctx.inner().clone(),
			input: input.map(|input| input.to_vec()),
		},
	)
	.await
}

async fn call_create_vars(
	callback: &crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: &ActorContext,
) -> Result<()> {
	call_void(
		"createVars",
		callback,
		LifecyclePayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_on_migrate(
	callback: &crate::actor_factory::CallbackTsfn<MigratePayload>,
	ctx: &ActorContext,
	is_new: bool,
) -> Result<()> {
	call_void(
		"onMigrate",
		callback,
		MigratePayload {
			ctx: ctx.inner().clone(),
			is_new,
		},
	)
	.await
}

async fn call_on_wake(
	callback: &crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: &ActorContext,
) -> Result<()> {
	call_void(
		"onWake",
		callback,
		LifecyclePayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_on_before_actor_start(
	callback: &crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: &ActorContext,
) -> Result<()> {
	call_void(
		"onBeforeActorStart",
		callback,
		LifecyclePayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_on_sleep(
	callback: &crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: &ActorContext,
) -> Result<()> {
	call_void(
		"onSleep",
		callback,
		LifecyclePayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_on_destroy(
	callback: &crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: &ActorContext,
) -> Result<()> {
	call_void(
		"onDestroy",
		callback,
		LifecyclePayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_run(
	callback: &crate::actor_factory::CallbackTsfn<LifecyclePayload>,
	ctx: &ActorContext,
) -> Result<bool> {
	crate::actor_factory::call_bool(
		"run",
		callback,
		LifecyclePayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_action(
	callback: &crate::actor_factory::CallbackTsfn<ActionPayload>,
	ctx: &ActorContext,
	telemetry: Option<rivetkit_core::ActorInvocationTelemetry>,
	conn: Option<rivetkit_core::ConnHandle>,
	name: String,
	args: Vec<u8>,
	scheduled_fire: Option<rivetkit_core::actor::schedule::ScheduledFireInfo>,
	cancel_token: Option<CancellationToken>,
) -> Result<Vec<u8>> {
	let callback_name = format!("actions.{name}");
	call_buffer(
		callback_name.as_str(),
		callback,
		ActionPayload {
			ctx: ctx.inner().clone(),
			telemetry,
			conn,
			name,
			args,
			scheduled_fire,
			cancel_token,
		},
	)
	.await
}

async fn call_on_before_action_response(
	callback: &crate::actor_factory::CallbackTsfn<BeforeActionResponsePayload>,
	ctx: &ActorContext,
	name: String,
	args: Vec<u8>,
	output: Vec<u8>,
) -> Result<Vec<u8>> {
	call_buffer(
		"onBeforeActionResponse",
		callback,
		BeforeActionResponsePayload {
			ctx: ctx.inner().clone(),
			name,
			args,
			output,
		},
	)
	.await
}

async fn call_http_request(
	callback: &crate::actor_factory::CallbackTsfn<HttpRequestPayload>,
	ctx: &ActorContext,
	request: rivetkit_core::Request,
) -> Result<rivetkit_core::ActorHttpResponse> {
	let request_cancel_token = request.cancellation_token();
	call_request(
		"onRequest",
		callback,
		HttpRequestPayload {
			ctx: ctx.inner().clone(),
			request,
			cancel_token: Some(request_cancel_token),
			response_stream: None,
		},
	)
	.await
}

async fn with_dispatch_cancel_token<T, F, Fut>(work: F) -> Result<T>
where
	F: FnOnce(CancellationToken) -> Fut,
	Fut: std::future::Future<Output = Result<T>>,
{
	let guard = DispatchCancelGuard::new();
	work(guard.token()).await
}

async fn call_on_websocket(
	callback: &crate::actor_factory::CallbackTsfn<WebSocketPayload>,
	ctx: &ActorContext,
	conn: rivetkit_core::ConnHandle,
	ws: rivetkit_core::WebSocket,
	request: Option<rivetkit_core::Request>,
) -> Result<()> {
	call_void(
		"onWebSocket",
		callback,
		WebSocketPayload {
			ctx: ctx.inner().clone(),
			conn,
			ws,
			request,
		},
	)
	.await
}

async fn call_on_before_subscribe(
	callback: &crate::actor_factory::CallbackTsfn<BeforeSubscribePayload>,
	ctx: &ActorContext,
	conn: rivetkit_core::ConnHandle,
	event_name: String,
) -> Result<()> {
	call_void(
		"onBeforeSubscribe",
		callback,
		BeforeSubscribePayload {
			ctx: ctx.inner().clone(),
			conn,
			event_name,
		},
	)
	.await
}

async fn call_workflow_history(
	callback: &crate::actor_factory::CallbackTsfn<WorkflowHistoryPayload>,
	ctx: &ActorContext,
) -> Result<Option<Vec<u8>>> {
	call_optional_buffer(
		"getWorkflowHistory",
		callback,
		WorkflowHistoryPayload {
			ctx: ctx.inner().clone(),
		},
	)
	.await
}

async fn call_workflow_replay(
	callback: &crate::actor_factory::CallbackTsfn<WorkflowReplayPayload>,
	ctx: &ActorContext,
	entry_id: Option<String>,
) -> Result<Option<Vec<u8>>> {
	call_optional_buffer(
		"replayWorkflow",
		callback,
		WorkflowReplayPayload {
			ctx: ctx.inner().clone(),
			entry_id,
		},
	)
	.await
}

async fn call_on_before_connect(
	callback: &crate::actor_factory::CallbackTsfn<BeforeConnectPayload>,
	ctx: &ActorContext,
	params: Vec<u8>,
	request: Option<rivetkit_core::Request>,
) -> Result<()> {
	call_void(
		"onBeforeConnect",
		callback,
		BeforeConnectPayload {
			ctx: ctx.inner().clone(),
			params,
			request,
		},
	)
	.await
}

async fn call_create_conn_state(
	callback: &crate::actor_factory::CallbackTsfn<CreateConnStatePayload>,
	ctx: &ActorContext,
	conn: rivetkit_core::ConnHandle,
	params: Vec<u8>,
	request: Option<rivetkit_core::Request>,
) -> Result<Vec<u8>> {
	call_buffer(
		"createConnState",
		callback,
		CreateConnStatePayload {
			ctx: ctx.inner().clone(),
			conn,
			params,
			request,
		},
	)
	.await
}

async fn call_on_connect(
	callback: &crate::actor_factory::CallbackTsfn<ConnectionPayload>,
	ctx: &ActorContext,
	conn: rivetkit_core::ConnHandle,
	request: Option<rivetkit_core::Request>,
) -> Result<()> {
	call_void(
		"onConnect",
		callback,
		ConnectionPayload {
			ctx: ctx.inner().clone(),
			conn,
			request,
		},
	)
	.await
}

async fn call_on_disconnect_final(
	callback: &crate::actor_factory::CallbackTsfn<ConnectionPayload>,
	ctx: &ActorContext,
	conn: rivetkit_core::ConnHandle,
) -> Result<()> {
	ctx.inner()
		.with_disconnect_callback(|| async {
			call_void(
				"onDisconnect",
				callback,
				ConnectionPayload {
					ctx: ctx.inner().clone(),
					conn,
					request: None,
				},
			)
			.await
		})
		.await
}

fn action_not_found(name: String) -> anyhow::Error {
	rivetkit_core::error::action_not_found(name)
}

fn actor_shutting_down() -> anyhow::Error {
	ActorLifecycle::Stopping.build()
}

fn missing_callback(name: &str) -> anyhow::Error {
	NapiInvalidState {
		state: format!("callback {name}"),
		reason: "not configured".to_owned(),
	}
	.build()
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../tests/napi_actor_events.rs"]
mod tests;
