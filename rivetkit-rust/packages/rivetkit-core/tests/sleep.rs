mod moved_tests {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};

	use crate::actor::context::ActorContext;
	use crate::actor::work_registry::ActorWorkKind;
	use parking_lot::Mutex as DropMutex;
	use rivet_envoy_client::async_counter::AsyncCounter;
	use std::time::{Duration, Instant};
	use tokio::sync::oneshot;
	use tokio::task::yield_now;

	use tokio::time::advance;
	use tracing::field::{Field, Visit};
	use tracing::{Event, Subscriber};
	use tracing_subscriber::layer::{Context as LayerContext, Layer};
	use tracing_subscriber::prelude::*;
	use tracing_subscriber::registry::Registry;

	#[derive(Default)]
	struct MessageVisitor {
		message: Option<String>,
		actor_id: Option<String>,
		kind: Option<String>,
		reason: Option<String>,
	}

	impl Visit for MessageVisitor {
		fn record_str(&mut self, field: &Field, value: &str) {
			match field.name() {
				"message" => self.message = Some(value.to_owned()),
				"actor_id" => self.actor_id = Some(value.to_owned()),
				"kind" => self.kind = Some(value.to_owned()),
				"reason" => self.reason = Some(value.to_owned()),
				_ => {}
			}
		}

		fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
			let value = format!("{value:?}").trim_matches('"').to_owned();
			match field.name() {
				"message" => self.message = Some(value),
				"actor_id" => self.actor_id = Some(value),
				"kind" => self.kind = Some(value),
				"reason" => self.reason = Some(value),
				_ => {}
			}
		}
	}

	#[derive(Clone)]
	struct ShutdownTaskRefusedLayer {
		count: Arc<AtomicUsize>,
	}

	#[derive(Clone)]
	struct RegisteredTaskDeadlineLayer {
		count: Arc<AtomicUsize>,
	}

	impl<S> Layer<S> for ShutdownTaskRefusedLayer
	where
		S: Subscriber,
	{
		fn on_event(&self, event: &Event<'_>, _ctx: LayerContext<'_, S>) {
			if *event.metadata().level() != tracing::Level::WARN {
				return;
			}

			let mut visitor = MessageVisitor::default();
			event.record(&mut visitor);
			if visitor.message.as_deref()
				== Some("shutdown task spawned after teardown; aborting immediately")
			{
				self.count.fetch_add(1, Ordering::SeqCst);
			}
		}
	}

	impl<S> Layer<S> for RegisteredTaskDeadlineLayer
	where
		S: Subscriber,
	{
		fn on_event(&self, event: &Event<'_>, _ctx: LayerContext<'_, S>) {
			if *event.metadata().level() != tracing::Level::WARN {
				return;
			}

			let mut visitor = MessageVisitor::default();
			event.record(&mut visitor);
			if visitor.message.as_deref() == Some("actor work cancelled by shutdown deadline")
				&& visitor.actor_id.as_deref() == Some("actor-register-task-deadline")
				&& visitor.kind.as_deref() == Some("registered_task")
				&& visitor.reason.as_deref() == Some("shutdown_deadline_elapsed")
			{
				self.count.fetch_add(1, Ordering::SeqCst);
			}
		}
	}

	struct NotifyOnDrop(DropMutex<Option<oneshot::Sender<()>>>);

	impl NotifyOnDrop {
		fn new(sender: oneshot::Sender<()>) -> Self {
			Self(DropMutex::new(Some(sender)))
		}
	}

	impl Drop for NotifyOnDrop {
		fn drop(&mut self) {
			if let Some(sender) = self.0.lock().take() {
				let _ = sender.send(());
			}
		}
	}

	#[tokio::test(start_paused = true)]
	async fn shutdown_task_counter_reaches_zero_after_completion() {
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-complete");
		let (done_tx, done_rx) = oneshot::channel();

		ctx.track_shutdown_task(async move {
			let _ = done_tx.send(());
		});

		done_rx.await.expect("shutdown task should complete");
		yield_now().await;

		assert_eq!(ctx.shutdown_task_count(), 0);
		assert!(
			ctx.0
				.sleep
				.work
				.shutdown_counter
				.wait_zero(Instant::now() + Duration::from_millis(1))
				.await
		);
	}

	#[tokio::test(start_paused = true)]
	async fn registered_tasks_are_reaped_and_do_not_accumulate() {
		let ctx = ActorContext::new_for_sleep_tests("actor-reap");

		// Register many quickly-completing tasks. With no await between spawns
		// none of them have run yet, so they all accumulate in the JoinSet.
		for _ in 0..50 {
			ctx.register_task(async {});
		}

		// Let every spawned task run to completion.
		for _ in 0..5 {
			yield_now().await;
		}

		// They have completed but their handles are still in the set: completed
		// JoinSet entries are not auto-reaped.
		let before = ctx.0.sleep.work.shutdown_tasks.lock().len();
		assert!(
			before >= 40,
			"expected completed task handles to accumulate before the next spawn, got {before}"
		);

		// Spawning one more reaps the completed handles first, so the set stays
		// bounded to in-flight work instead of growing for the actor lifetime.
		ctx.register_task(async {});
		let after = ctx.0.sleep.work.shutdown_tasks.lock().len();
		assert!(
			after <= 2,
			"expected the JoinSet to be reaped down to in-flight work, got {after}"
		);
	}

	#[tokio::test(start_paused = true)]
	async fn shutdown_task_counter_reaches_zero_after_panic() {
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-panic");

		ctx.track_shutdown_task(async move {
			panic!("boom");
		});

		yield_now().await;
		yield_now().await;

		assert_eq!(ctx.shutdown_task_count(), 0);
		assert!(
			ctx.0
				.sleep
				.work
				.shutdown_counter
				.wait_zero(Instant::now() + Duration::from_millis(1))
				.await
		);
	}

	#[tokio::test(start_paused = true)]
	async fn teardown_aborts_tracked_shutdown_tasks() {
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-teardown");
		let (drop_tx, drop_rx) = oneshot::channel();
		let (_never_tx, never_rx) = oneshot::channel::<()>();
		let notify = NotifyOnDrop::new(drop_tx);

		ctx.track_shutdown_task(async move {
			let _notify = notify;
			let _ = never_rx.await;
		});

		assert_eq!(ctx.shutdown_task_count(), 1);

		ctx.teardown_sleep_state().await;
		advance(Duration::from_millis(1)).await;

		drop_rx
			.await
			.expect("teardown should abort the tracked task");
		assert_eq!(ctx.shutdown_task_count(), 0);
	}

	#[tokio::test(start_paused = true)]
	async fn track_shutdown_task_refuses_spawns_after_teardown() {
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-refuse");
		let warning_count = Arc::new(AtomicUsize::new(0));
		let subscriber = Registry::default().with(ShutdownTaskRefusedLayer {
			count: warning_count.clone(),
		});
		let _guard = tracing::subscriber::set_default(subscriber);

		ctx.teardown_sleep_state().await;
		ctx.track_shutdown_task(async move {
			panic!("post-teardown shutdown task should never spawn");
		});
		yield_now().await;

		assert_eq!(ctx.shutdown_task_count(), 0);
		assert_eq!(warning_count.load(Ordering::SeqCst), 1);
	}

	#[tokio::test(start_paused = true)]
	async fn register_task_exits_when_shutdown_deadline_cancels() {
		let ctx = ActorContext::new_for_sleep_tests("actor-register-task-deadline");
		let warning_count = Arc::new(AtomicUsize::new(0));
		let subscriber = Registry::default().with(RegisteredTaskDeadlineLayer {
			count: warning_count.clone(),
		});
		let _guard = tracing::subscriber::set_default(subscriber);

		ctx.register_task(futures::future::pending::<()>());
		assert_eq!(ctx.shutdown_task_count(), 1);

		ctx.cancel_shutdown_deadline();

		assert!(
			ctx.0
				.sleep
				.work
				.shutdown_counter
				.wait_zero(Instant::now() + Duration::from_millis(1))
				.await,
			"registered task should stop waiting after the shutdown deadline"
		);
		assert_eq!(ctx.shutdown_task_count(), 0);
		assert_eq!(warning_count.load(Ordering::SeqCst), 1);
	}

	#[tokio::test(start_paused = true)]
	async fn tracked_shutdown_work_drain_wakes_on_shutdown_counter_zero() {
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-drain-counter");
		ctx.notify_activity_dirty();
		let (release_tx, release_rx) = oneshot::channel();

		ctx.track_shutdown_task(async move {
			let _ = release_rx.await;
		});
		let waiter = tokio::spawn({
			let ctx = ctx.clone();
			async move { ctx.wait_for_tracked_shutdown_work().await }
		});

		yield_now().await;
		assert!(
			!waiter.is_finished(),
			"shutdown drain should wait while the counter is non-zero"
		);

		release_tx
			.send(())
			.expect("release signal should send to tracked shutdown task");
		yield_now().await;
		yield_now().await;

		assert!(
			waiter.is_finished(),
			"shutdown drain should wake from the counter zero notification"
		);
		assert!(waiter.await.expect("shutdown drain waiter should join"));
	}

	#[tokio::test(start_paused = true)]
	async fn tracked_shutdown_work_drain_wakes_on_websocket_callback_zero() {
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-drain-websocket");
		ctx.notify_activity_dirty();
		let guard = ctx.websocket_callback_region();
		let waiter = tokio::spawn({
			let ctx = ctx.clone();
			async move { ctx.wait_for_tracked_shutdown_work().await }
		});

		yield_now().await;
		assert!(
			!waiter.is_finished(),
			"shutdown drain should wait while the websocket callback is active"
		);

		drop(guard);
		yield_now().await;

		assert!(
			waiter.is_finished(),
			"shutdown drain should wake from the websocket zero notification"
		);
		assert!(waiter.await.expect("shutdown drain waiter should join"));
	}

	#[tokio::test(start_paused = true)]
	async fn keep_awake_spawned_work_exits_when_shutdown_deadline_cancels() {
		let ctx = ActorContext::new_for_sleep_tests("actor-keep-awake-deadline");

		ctx.spawn_work(ActorWorkKind::KeepAwake, futures::future::pending::<()>());
		assert_eq!(ctx.shutdown_task_count(), 1);
		assert_eq!(ctx.sleep_keep_awake_count(), 1);

		ctx.cancel_shutdown_deadline();

		assert!(
			ctx.0
				.sleep
				.work
				.shutdown_counter
				.wait_zero(Instant::now() + Duration::from_millis(1))
				.await,
			"keepAwake work should stop waiting after the shutdown deadline"
		);
		assert_eq!(ctx.shutdown_task_count(), 0);
		assert_eq!(ctx.sleep_keep_awake_count(), 0);
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_then_destroy_signal_tasks_do_not_leak_after_teardown() {
		let ctx = ActorContext::new_for_sleep_tests("actor-sleep-destroy");
		ctx.set_started(true);

		ctx.sleep()
			.expect("sleep should succeed after started is set");
		ctx.destroy()
			.expect("destroy should succeed after started is set");

		assert_eq!(
			ctx.shutdown_task_count(),
			2,
			"sleep and destroy bridge work should be tracked before it runs"
		);

		ctx.teardown_sleep_state().await;
		advance(Duration::from_millis(1)).await;

		assert_eq!(ctx.shutdown_task_count(), 0);
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_idle_window_without_work_returns_next_tick() {
		let ctx = ActorContext::new_for_sleep_tests("actor-sleep-idle");

		let waiter = tokio::spawn({
			let ctx = ctx.clone();
			async move {
				ctx.wait_for_sleep_idle_window(Instant::now() + Duration::from_secs(1))
					.await
			}
		});

		yield_now().await;

		assert!(
			waiter.is_finished(),
			"idle wait should not poll in 10ms slices"
		);
		assert!(waiter.await.expect("idle waiter should join"));
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_idle_window_waits_for_http_counter_zero_transition() {
		let ctx = ActorContext::new_for_sleep_tests("actor-http-idle");
		let counter = Arc::new(AsyncCounter::new());
		counter.register_zero_notify(&ctx.0.sleep.work.idle_notify);
		counter.register_change_notify(&ctx.sleep_activity_notify());
		*ctx.0.sleep.http_request_counter.lock() = Some(counter.clone());

		counter.increment();
		let waiter = tokio::spawn({
			let ctx = ctx.clone();
			async move {
				ctx.wait_for_sleep_idle_window(Instant::now() + Duration::from_secs(1))
					.await
			}
		});

		yield_now().await;
		assert!(
			!waiter.is_finished(),
			"http request drain should stay blocked while the counter is non-zero"
		);

		counter.decrement();
		advance(Duration::from_millis(1)).await;
		yield_now().await;
		assert!(waiter.await.expect("http idle waiter should join"));
	}

	#[tokio::test(start_paused = true)]
	async fn http_request_idle_wait_uses_zero_notify() {
		let ctx = ActorContext::new_for_sleep_tests("actor-http-zero-notify");
		let counter = Arc::new(AsyncCounter::new());
		counter.register_zero_notify(&ctx.0.sleep.work.idle_notify);
		*ctx.0.sleep.http_request_counter.lock() = Some(counter.clone());

		counter.increment();
		let waiter = tokio::spawn({
			let ctx = ctx.clone();
			async move {
				ctx.wait_for_http_requests_idle().await;
			}
		});

		yield_now().await;
		assert!(
			!waiter.is_finished(),
			"http request idle wait should block while the counter is non-zero"
		);

		counter.decrement();
		yield_now().await;

		assert!(
			waiter.is_finished(),
			"http request idle wait should wake on the zero notification"
		);
		waiter.await.expect("http idle waiter should join");
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_idle_window_waits_for_websocket_callback_zero_transition() {
		let ctx = ActorContext::new_for_sleep_tests("actor-websocket-idle");
		let guard = ctx.websocket_callback_region();

		let waiter = tokio::spawn({
			let ctx = ctx.clone();
			async move {
				ctx.wait_for_sleep_idle_window(Instant::now() + Duration::from_secs(1))
					.await
			}
		});

		yield_now().await;
		assert!(
			!waiter.is_finished(),
			"websocket callback drain should stay blocked while the counter is non-zero"
		);

		drop(guard);
		advance(Duration::from_millis(1)).await;
		yield_now().await;
		assert!(waiter.await.expect("websocket idle waiter should join"));
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_before_started_errors_with_actor_starting() {
		let ctx = ActorContext::new_for_sleep_tests("actor-sleep-before-started");

		let err = ctx
			.sleep()
			.expect_err("sleep should fail before started is set");
		let rivet_err = rivet_error::RivetError::extract(&err);
		assert_eq!(rivet_err.group(), "actor");
		assert_eq!(rivet_err.code(), "starting");
	}

	#[tokio::test(start_paused = true)]
	async fn destroy_before_started_errors_with_actor_starting() {
		let ctx = ActorContext::new_for_sleep_tests("actor-destroy-before-started");

		let err = ctx
			.destroy()
			.expect_err("destroy should fail before started is set");
		let rivet_err = rivet_error::RivetError::extract(&err);
		assert_eq!(rivet_err.group(), "actor");
		assert_eq!(rivet_err.code(), "starting");
	}

	// Duplicate `ctx.sleep()` calls are deliberately idempotent: `sleep()` returns
	// `Ok(())` when `sleep_requested` was already set for this generation (commit
	// ae09be095abf "fix(rivetkit): make duplicate sleep requests idempotent").
	// Only `destroy()` still errors with `actor.stopping` on a duplicate request;
	// that path is covered by `double_destroy_errors_with_actor_stopping` below.
	#[tokio::test(start_paused = true)]
	async fn double_sleep_is_idempotent_while_started() {
		let ctx = ActorContext::new_for_sleep_tests("actor-double-sleep");
		ctx.set_started(true);

		ctx.sleep()
			.expect("first sleep call should be accepted after startup");
		ctx.sleep()
			.expect("second sleep call should be accepted as an idempotent request");
	}

	#[tokio::test(start_paused = true)]
	async fn double_destroy_errors_with_actor_stopping() {
		let ctx = ActorContext::new_for_sleep_tests("actor-double-destroy");
		ctx.set_started(true);

		ctx.destroy()
			.expect("first destroy call should be accepted after startup");

		let err = ctx
			.destroy()
			.expect_err("second destroy call should fail as already requested");
		let rivet_err = rivet_error::RivetError::extract(&err);
		assert_eq!(rivet_err.group(), "actor");
		assert_eq!(rivet_err.code(), "stopping");
	}

	#[tokio::test(start_paused = true)]
	async fn stop_with_error_before_started_errors_with_actor_starting() {
		let ctx = ActorContext::new_for_sleep_tests("actor-stop-error-before-started");

		let err = ctx
			.stop_with_error("boom")
			.expect_err("stop_with_error should fail before started is set");
		let rivet_err = rivet_error::RivetError::extract(&err);
		assert_eq!(rivet_err.group(), "actor");
		assert_eq!(rivet_err.code(), "starting");
	}

	#[tokio::test(start_paused = true)]
	async fn stop_with_error_after_destroy_errors_with_actor_stopping() {
		let ctx = ActorContext::new_for_sleep_tests("actor-stop-error-after-destroy");
		ctx.set_started(true);

		ctx.destroy()
			.expect("destroy call should be accepted after startup");

		let err = ctx
			.stop_with_error("boom")
			.expect_err("stop_with_error should fail once destroy is already requested");
		let rivet_err = rivet_error::RivetError::extract(&err);
		assert_eq!(rivet_err.group(), "actor");
		assert_eq!(rivet_err.code(), "stopping");
	}

	mod stop_intent {
		use std::collections::HashMap;
		use std::sync::Mutex as EnvoySharedMutex;
		use std::sync::atomic::AtomicBool;
		use std::time::Duration;

		use rivet_envoy_client::config::{
			BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
			WebSocketSender,
		};
		use rivet_envoy_client::context::{SharedContext, WsTxMessage};
		use rivet_envoy_client::envoy::ToEnvoyMessage;
		use rivet_envoy_client::handle::EnvoyHandle;
		use rivet_envoy_client::protocol;
		use tokio::sync::mpsc;

		use std::sync::Arc;

		use crate::actor::context::ActorContext;

		struct IdleEnvoyCallbacks;

		impl EnvoyCallbacks for IdleEnvoyCallbacks {
			fn on_actor_start(
				&self,
				_handle: EnvoyHandle,
				_actor_id: String,
				_generation: u32,
				_config: protocol::ActorConfig,
				_preloaded_kv: Option<protocol::PreloadedKv>,
			) -> BoxFuture<anyhow::Result<()>> {
				Box::pin(async { Ok(()) })
			}

			fn on_shutdown(&self) {}

			fn fetch(
				&self,
				_handle: EnvoyHandle,
				_actor_id: String,
				_gateway_id: protocol::GatewayId,
				_request_id: protocol::RequestId,
				_request: HttpRequest,
			) -> BoxFuture<anyhow::Result<HttpResponse>> {
				Box::pin(async { anyhow::bail!("fetch should not run in sleep tests") })
			}

			fn websocket(
				&self,
				_handle: EnvoyHandle,
				_actor_id: String,
				_gateway_id: protocol::GatewayId,
				_request_id: protocol::RequestId,
				_request: HttpRequest,
				_path: String,
				_headers: HashMap<String, String>,
				_is_hibernatable: bool,
				_is_restoring_hibernatable: bool,
				_sender: WebSocketSender,
			) -> BoxFuture<anyhow::Result<WebSocketHandler>> {
				Box::pin(async { anyhow::bail!("websocket should not run in sleep tests") })
			}

			fn can_hibernate(
				&self,
				_actor_id: &str,
				_gateway_id: &protocol::GatewayId,
				_request_id: &protocol::RequestId,
				_request: &HttpRequest,
			) -> BoxFuture<anyhow::Result<bool>> {
				Box::pin(async { Ok(false) })
			}
		}

		fn test_envoy_handle() -> (EnvoyHandle, mpsc::UnboundedReceiver<ToEnvoyMessage>) {
			let (envoy_tx, envoy_rx) = mpsc::unbounded_channel();
			let shared = Arc::new(SharedContext {
				config: EnvoyConfig {
					version: 1,
					endpoint: "http://127.0.0.1:1".to_string(),
					token: None,
					namespace: "test".to_string(),
					pool_name: "test".to_string(),
					prepopulate_actor_names: HashMap::new(),
					metadata: None,
					not_global: true,
					debug_latency_ms: None,
					callbacks: Arc::new(IdleEnvoyCallbacks),
				},
				envoy_key: "test-envoy".to_string(),
				envoy_tx,
				actors: Arc::new(EnvoySharedMutex::new(HashMap::new())),
				actors_notify: Arc::new(tokio::sync::Notify::new()),
				live_tunnel_requests: Arc::new(EnvoySharedMutex::new(HashMap::new())),
				pending_hibernation_restores: Arc::new(EnvoySharedMutex::new(HashMap::new())),
				ws_tx: Arc::new(tokio::sync::Mutex::new(
					None::<mpsc::UnboundedSender<WsTxMessage>>,
				)),
				http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
				connection_session: std::sync::atomic::AtomicU64::new(0),
				next_connection_session: std::sync::atomic::AtomicU64::new(0),
				connection_session_tx: tokio::sync::watch::channel(0).0,
				protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
				shutting_down: AtomicBool::new(false),
				last_ping_ts: std::sync::atomic::AtomicI64::new(i64::MAX),
				stopped_tx: tokio::sync::watch::channel(true).0,
			});

			(EnvoyHandle::from_shared(shared), envoy_rx)
		}

		async fn recv_stop_intent(
			rx: &mut mpsc::UnboundedReceiver<ToEnvoyMessage>,
			expected_actor_id: &str,
		) -> Option<String> {
			let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
				.await
				.expect("timed out waiting for stop intent")
				.expect("envoy channel closed before stop intent");
			match message {
				ToEnvoyMessage::ActorIntent {
					actor_id,
					generation,
					intent: protocol::ActorIntent::ActorIntentStop,
					error,
				} => {
					assert_eq!(actor_id, expected_actor_id);
					assert_eq!(generation, Some(3));
					error
				}
				_ => panic!("expected stop intent envoy message"),
			}
		}

		#[tokio::test(start_paused = true)]
		async fn destroy_sends_stop_intent_without_error() {
			let ctx = ActorContext::new_for_sleep_tests("actor-destroy-intent");
			let (handle, mut rx) = test_envoy_handle();
			ctx.configure_sleep_envoy(handle, Some(3));
			ctx.set_started(true);

			ctx.destroy().expect("destroy should succeed after startup");

			let error = recv_stop_intent(&mut rx, "actor-destroy-intent").await;
			assert_eq!(error, None);
		}

		#[tokio::test(start_paused = true)]
		async fn stop_with_error_sends_stop_intent_with_message() {
			let ctx = ActorContext::new_for_sleep_tests("actor-stop-error-intent");
			let (handle, mut rx) = test_envoy_handle();
			ctx.configure_sleep_envoy(handle, Some(3));
			ctx.set_started(true);

			ctx.stop_with_error("child exited unexpectedly (exit status: 137)")
				.expect("stop_with_error should succeed after startup");

			let error = recv_stop_intent(&mut rx, "actor-stop-error-intent").await;
			assert_eq!(
				error.as_deref(),
				Some("child exited unexpectedly (exit status: 137)")
			);
		}

		#[tokio::test(start_paused = true)]
		async fn stop_with_error_truncates_long_message() {
			let ctx = ActorContext::new_for_sleep_tests("actor-stop-error-truncated");
			let (handle, mut rx) = test_envoy_handle();
			ctx.configure_sleep_envoy(handle, Some(3));
			ctx.set_started(true);

			ctx.stop_with_error("x".repeat(1024 * 1024))
				.expect("stop_with_error should succeed after startup");

			let error = recv_stop_intent(&mut rx, "actor-stop-error-truncated")
				.await
				.expect("stop intent should carry the truncated message");
			assert!(
				error.len() < 4096,
				"message must be capped: {}",
				error.len()
			);
			assert!(error.ends_with("... (truncated)"));
		}
	}

	// `set_prevent_sleep` is a deprecated no-op kept for NAPI bridge
	// compatibility. The exhaustive `CanSleep` match below is a build-time
	// guard against reintroducing a `PreventSleep` enum variant.
	#[tokio::test(start_paused = true)]
	#[allow(deprecated)]
	async fn set_prevent_sleep_is_a_deprecated_noop() {
		use crate::actor::sleep::CanSleep;

		let ctx = ActorContext::new_for_sleep_tests("actor-prevent-sleep-noop");
		ctx.set_started(true);

		ctx.set_prevent_sleep(true);
		match ctx.can_sleep().await {
			CanSleep::Yes
			| CanSleep::NotReady
			| CanSleep::NoSleep
			| CanSleep::ActiveHttpRequests
			| CanSleep::ActiveKeepAwake
			| CanSleep::ActiveInternalKeepAwake
			| CanSleep::ActiveRunHandler
			| CanSleep::ActiveDisconnectCallbacks
			| CanSleep::ActiveConnections
			| CanSleep::ActiveWebSocketCallbacks => {}
		}

		ctx.set_prevent_sleep(false);
	}

	#[tokio::test(start_paused = true)]
	async fn shutdown_deadline_token_aborts_select_awaiting_task() {
		// Mirrors the NAPI `RunGracefulCleanup` pattern: a task awaits user
		// work and the shutdown_deadline cancellation in a `tokio::select!`.
		// If `cancel_shutdown_deadline()` does not propagate to clones of the
		// token, the spawned task would hang and the test would time out.
		let ctx = ActorContext::new_for_sleep_tests("actor-shutdown-deadline");
		let token = ctx.shutdown_deadline_token();
		assert!(!token.is_cancelled());

		let aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
		let aborted_clone = aborted.clone();
		let task = tokio::spawn(async move {
			tokio::select! {
				_ = token.cancelled() => {
					aborted_clone.store(true, Ordering::SeqCst);
				}
				_ = futures::future::pending::<()>() => {}
			}
		});

		yield_now().await;
		assert!(!aborted.load(Ordering::SeqCst));

		ctx.cancel_shutdown_deadline();
		task.await.expect("select task should join after cancel");
		assert!(
			aborted.load(Ordering::SeqCst),
			"select-awaiting task must observe cancel via the cloned token"
		);
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_after_grace_clears_started_returns_stopping_not_starting() {
		// Simulate the lifecycle state machine clearing `started` when it
		// transitions into SleepGrace. Calls into `sleep()` after that point
		// must surface `Stopping`, not `Starting`.
		let ctx = ActorContext::new_for_sleep_tests("actor-sleep-after-grace");
		ctx.set_started(true);

		ctx.sleep().expect("first sleep call should be accepted");

		// Lifecycle machine clears `started` on transition into SleepGrace.
		ctx.set_started(false);

		let err = ctx.sleep().expect_err("second sleep should fail");
		let rivet_err = rivet_error::RivetError::extract(&err);
		assert_eq!(rivet_err.group(), "actor");
		assert_eq!(
			rivet_err.code(),
			"stopping",
			"started=false during shutdown must surface stopping, not starting"
		);
	}
}
