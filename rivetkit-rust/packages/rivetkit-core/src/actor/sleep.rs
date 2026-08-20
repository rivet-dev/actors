use parking_lot::Mutex;
use rivet_envoy_client::async_counter::AsyncCounter;
use rivet_envoy_client::handle::EnvoyHandle;
use std::future::Future;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize as TestAtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(not(feature = "wasm-runtime"))]
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::actor::config::ActorConfig;
use crate::actor::context::ActorContext;
#[cfg(not(feature = "wasm-runtime"))]
use crate::actor::context::ActorWorkRegion;
use crate::actor::task_types::ShutdownKind;
#[cfg(not(feature = "wasm-runtime"))]
use crate::actor::work_registry::ActorWorkPolicy;
#[cfg(feature = "wasm-runtime")]
use crate::actor::work_registry::LocalShutdownTask;
use crate::actor::work_registry::{ActorWorkKind, CountGuard, RegionGuard, WorkRegistry};
#[cfg(feature = "wasm-runtime")]
use crate::runtime::RuntimeSpawner;
#[cfg(test)]
use crate::time::sleep_until;
use crate::time::{Instant, sleep};
#[cfg(test)]
use crate::types::ActorKey;
#[cfg(feature = "wasm-runtime")]
use futures::channel::oneshot as futures_oneshot;
#[cfg(feature = "wasm-runtime")]
use futures::future::{AbortHandle, Abortable};

/// Per-actor sleep state.
///
/// `ActorContext::reset_sleep_timer()` is invoked on every mutation that changes
/// a sleep predicate input. Production actors wake the owning `ActorTask` via a
/// single `Notify`; contexts not wired to an `ActorTask` use the detached
/// compatibility timer below.
pub(crate) struct SleepState {
	// Forced-sync: sleep controller config/runtime handles are synchronous
	// wiring slots cloned before actor I/O.
	pub(super) config: Mutex<ActorConfig>,
	pub(super) envoy_handle: Mutex<Option<EnvoyHandle>>,
	pub(super) generation: Mutex<Option<u32>>,
	pub(super) http_request_counter: Mutex<Option<Arc<AsyncCounter>>>,
	// Forced-sync: written once by whichever caller wins the destroy-request
	// swap, then consumed when the stop intent is sent to the envoy.
	pub(super) destroy_error: Mutex<Option<String>>,
	#[cfg(test)]
	sleep_request_count: TestAtomicUsize,
	#[cfg(test)]
	destroy_request_count: TestAtomicUsize,
	pub(super) lifecycle_started: AtomicBool,
	pub(super) run_handler_active_count: AtomicUsize,
	// Forced-sync: the compatibility sleep timer is aborted from sync paths.
	pub(super) sleep_timer: Mutex<Option<JoinHandle<()>>>,
	pub(super) work: WorkRegistry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanSleep {
	Yes,
	NotReady,
	NoSleep,
	ActiveHttpRequests,
	ActiveKeepAwake,
	ActiveInternalKeepAwake,
	ActiveRunHandler,
	ActiveDisconnectCallbacks,
	ActiveConnections,
	ActiveWebSocketCallbacks,
}

impl SleepState {
	pub fn new(config: ActorConfig) -> Self {
		Self {
			config: Mutex::new(config),
			envoy_handle: Mutex::new(None),
			generation: Mutex::new(None),
			http_request_counter: Mutex::new(None),
			destroy_error: Mutex::new(None),
			#[cfg(test)]
			sleep_request_count: TestAtomicUsize::new(0),
			#[cfg(test)]
			destroy_request_count: TestAtomicUsize::new(0),
			lifecycle_started: AtomicBool::new(false),
			run_handler_active_count: AtomicUsize::new(0),
			sleep_timer: Mutex::new(None),
			work: WorkRegistry::new(),
		}
	}
}

impl Default for SleepState {
	fn default() -> Self {
		Self::new(ActorConfig::default())
	}
}

impl std::fmt::Debug for SleepState {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SleepState")
			.field(
				"lifecycle_started",
				&self.lifecycle_started.load(Ordering::SeqCst),
			)
			.field(
				"run_handler_active_count",
				&self.run_handler_active_count.load(Ordering::SeqCst),
			)
			.field("keep_awake_count", &self.work.keep_awake.load())
			.field(
				"internal_keep_awake_count",
				&self.work.internal_keep_awake.load(),
			)
			.field(
				"websocket_callback_count",
				&self.work.websocket_callback.load(),
			)
			.field(
				"disconnect_callback_count",
				&self.work.disconnect_callback.load(),
			)
			.finish()
	}
}

impl ActorContext {
	#[cfg(test)]
	pub(crate) fn new_for_sleep_tests(actor_id: impl Into<String>) -> Self {
		Self::new(actor_id, "sleep-test", ActorKey::default(), "local")
	}

	pub(crate) fn configure_sleep_state(&self, config: ActorConfig) {
		*self.0.sleep.config.lock() = config;
	}

	pub(crate) fn configure_sleep_envoy(&self, envoy_handle: EnvoyHandle, generation: Option<u32>) {
		*self.0.sleep.envoy_handle.lock() = Some(envoy_handle);
		*self.0.sleep.generation.lock() = generation;
		*self.0.sleep.http_request_counter.lock() =
			self.lookup_http_request_counter(self.actor_id());
	}

	pub(crate) fn sleep_envoy_handle(&self) -> Option<EnvoyHandle> {
		self.0.sleep.envoy_handle.lock().clone()
	}

	pub(crate) fn sleep_generation(&self) -> Option<u32> {
		*self.0.sleep.generation.lock()
	}

	pub(crate) fn request_sleep_from_envoy(&self) {
		#[cfg(test)]
		self.0
			.sleep
			.sleep_request_count
			.fetch_add(1, Ordering::SeqCst);
		let envoy_handle = self.0.sleep.envoy_handle.lock().clone();
		let generation = *self.0.sleep.generation.lock();
		if let Some(envoy_handle) = envoy_handle {
			envoy_handle.sleep_actor(self.actor_id().to_owned(), generation);
		}
	}

	pub(crate) fn request_destroy_from_envoy(&self) {
		#[cfg(test)]
		self.0
			.sleep
			.destroy_request_count
			.fetch_add(1, Ordering::SeqCst);
		let envoy_handle = self.0.sleep.envoy_handle.lock().clone();
		let generation = *self.0.sleep.generation.lock();
		let error = self.0.sleep.destroy_error.lock().take();
		if let Some(envoy_handle) = envoy_handle {
			envoy_handle.stop_actor(self.actor_id().to_owned(), generation, error);
		}
	}

	pub(crate) fn set_lifecycle_started(&self, started: bool) {
		let previous = self
			.0
			.sleep
			.lifecycle_started
			.swap(started, Ordering::SeqCst);
		if previous != started {
			self.reset_sleep_timer();
		}
	}

	pub(crate) fn lifecycle_started(&self) -> bool {
		self.0.sleep.lifecycle_started.load(Ordering::SeqCst)
	}

	#[doc(hidden)]
	pub fn begin_run_handler(&self) {
		let previous = self
			.0
			.sleep
			.run_handler_active_count
			.fetch_add(1, Ordering::SeqCst);
		if previous == 0 {
			self.reset_sleep_timer();
		}
	}

	#[doc(hidden)]
	pub fn end_run_handler(&self) {
		match self.0.sleep.run_handler_active_count.fetch_update(
			Ordering::SeqCst,
			Ordering::SeqCst,
			|count| count.checked_sub(1),
		) {
			Ok(1) => self.reset_sleep_timer(),
			Ok(_) => {}
			Err(_) => {
				tracing::warn!(
					actor_id = %self.actor_id(),
					"run handler active counter underflow"
				);
			}
		}
	}

	#[doc(hidden)]
	pub fn run_handler_active(&self) -> bool {
		self.0.sleep.run_handler_active_count.load(Ordering::SeqCst) > 0
	}

	#[cfg(test)]
	pub(crate) fn sleep_request_count(&self) -> usize {
		self.0.sleep.sleep_request_count.load(Ordering::SeqCst)
	}

	pub(crate) async fn can_arm_sleep_timer(&self) -> CanSleep {
		let config = self.sleep_state_config();
		if !self.0.sleep.lifecycle_started.load(Ordering::SeqCst) {
			return CanSleep::NotReady;
		}
		if config.no_sleep {
			return CanSleep::NoSleep;
		}
		if self.active_http_request_count() > 0 {
			return CanSleep::ActiveHttpRequests;
		}
		if self.sleep_keep_awake_count() > 0 {
			return CanSleep::ActiveKeepAwake;
		}
		if self.sleep_internal_keep_awake_count() > 0 {
			return CanSleep::ActiveInternalKeepAwake;
		}
		// Queue receives are sleep-compatible: sleep aborts the wait via the
		// actor abort token, then the next generation restarts the run loop.
		if self.run_handler_active() && self.active_queue_wait_count() == 0 {
			return CanSleep::ActiveRunHandler;
		}
		if self.pending_disconnect_count() > 0 {
			return CanSleep::ActiveDisconnectCallbacks;
		}
		if !self.conns().is_empty() {
			return CanSleep::ActiveConnections;
		}
		if self.websocket_callback_count() > 0 {
			return CanSleep::ActiveWebSocketCallbacks;
		}

		CanSleep::Yes
	}

	pub(crate) fn can_finalize_shutdown(&self, reason: ShutdownKind) -> bool {
		self.0.sleep.work.core_dispatched_hooks.load() == 0
			// Sleep is a reversible pause, so let the actor run handler finish
			// naturally during grace. Destroy is terminal and may cut through it.
			&& (matches!(reason, ShutdownKind::Destroy) || !self.run_handler_active())
			&& self.shutdown_task_count() == 0
			&& self.sleep_keep_awake_count() == 0
			&& self.sleep_internal_keep_awake_count() == 0
			&& self.active_http_request_count() == 0
			&& self.websocket_callback_count() == 0
			&& self.pending_disconnect_count() == 0
	}

	/// Spawn the fallback sleep timer used by `ActorContext`s that are not
	/// bound to an `ActorTask`.
	///
	/// This path only engages when `configure_lifecycle_events` has not been
	/// wired, which in practice means test contexts. Production actors built
	/// through the registry always have an `ActorTask` and never spawn this
	/// detached timer.
	pub(crate) fn reset_sleep_timer_state(&self) {
		self.cancel_sleep_timer();

		#[cfg(not(feature = "wasm-runtime"))]
		let Ok(runtime) = Handle::try_current() else {
			tracing::debug!(
				actor_id = %self.actor_id(),
				"sleep activity reset skipped without tokio runtime"
			);
			return;
		};

		tracing::debug!(
			actor_id = %self.actor_id(),
			sleep_timeout_ms = self.0.sleep.config.lock().sleep_timeout.as_millis() as u64,
			"sleep activity reset"
		);

		let ctx = self.clone();
		let task_body = async move {
			let can_sleep = ctx.can_sleep().await;
			if can_sleep != CanSleep::Yes {
				tracing::debug!(
					actor_id = %ctx.actor_id(),
					reason = ?can_sleep,
					"sleep idle timer skipped"
				);
				return;
			}

			let timeout = ctx.sleep_config().sleep_timeout;
			sleep(timeout).await;

			let can_sleep = ctx.can_sleep().await;
			if can_sleep == CanSleep::Yes {
				tracing::debug!(
					actor_id = %ctx.actor_id(),
					sleep_timeout_ms = timeout.as_millis() as u64,
					"sleep idle timer elapsed"
				);
				if let Err(err) = ctx.sleep() {
					tracing::debug!(
						actor_id = %ctx.actor_id(),
						?err,
						"sleep idle timer request suppressed"
					);
				}
			} else {
				tracing::warn!(
					actor_id = %ctx.actor_id(),
					reason = ?can_sleep,
					"sleep idle timer elapsed but actor stayed awake"
				);
			}
		};

		#[cfg(not(feature = "wasm-runtime"))]
		let task = runtime.spawn(task_body);

		#[cfg(feature = "wasm-runtime")]
		let task = RuntimeSpawner::spawn(task_body);

		*self.0.sleep.sleep_timer.lock() = Some(task);
	}

	pub(crate) fn cancel_sleep_timer(&self) {
		let timer = self.0.sleep.sleep_timer.lock().take();
		if let Some(timer) = timer {
			timer.abort();
		}
	}

	pub(crate) async fn wait_for_internal_keep_awake_idle(&self, deadline: Instant) -> bool {
		self.0
			.sleep
			.work
			.internal_keep_awake
			.wait_zero(deadline)
			.await
	}

	#[cfg(test)]
	pub(crate) async fn wait_for_sleep_idle_window(&self, deadline: Instant) -> bool {
		loop {
			let activity = self.sleep_activity_notify();
			let activity_notified = activity.notified();
			tokio::pin!(activity_notified);
			activity_notified.as_mut().enable();
			let idle = self.0.sleep.work.idle_notify.notified();
			tokio::pin!(idle);
			idle.as_mut().enable();

			if self.can_finalize_shutdown(ShutdownKind::Sleep) {
				return true;
			}

			tokio::select! {
				_ = &mut activity_notified => {}
				_ = &mut idle => {}
				_ = sleep_until(deadline) => return false,
			}
		}
	}

	#[cfg(test)]
	pub(crate) async fn wait_for_shutdown_tasks(&self, deadline: Instant) -> bool {
		loop {
			let activity = self.sleep_activity_notify();
			let notified = activity.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();

			let shutdown_count = self.shutdown_task_count();
			let websocket_count = self.websocket_callback_count();
			if shutdown_count == 0 && websocket_count == 0 {
				return true;
			}

			tokio::select! {
				drained = self.0.sleep.work.shutdown_counter.wait_zero(deadline), if shutdown_count > 0 => {
					if !drained {
						return false;
					}
				}
				drained = self.0.sleep.work.websocket_callback.wait_zero(deadline), if websocket_count > 0 => {
					if !drained {
						return false;
					}
				}
				_ = &mut notified => {}
				_ = sleep_until(deadline) => return false,
			}
		}
	}

	pub async fn wait_for_tracked_shutdown_work(&self) -> bool {
		let shutdown_deadline = self.shutdown_deadline_token();
		tokio::select! {
			_ = self.wait_for_tracked_shutdown_work_drained() => true,
			_ = shutdown_deadline.cancelled() => false,
		}
	}

	pub async fn wait_for_tracked_shutdown_work_unbounded(&self) {
		self.wait_for_tracked_shutdown_work_drained().await;
	}

	async fn wait_for_tracked_shutdown_work_drained(&self) {
		loop {
			let shutdown_count = self.shutdown_task_count();
			let websocket_count = self.websocket_callback_count();
			if shutdown_count == 0 && websocket_count == 0 {
				return;
			}

			tokio::select! {
				_ = self.0.sleep.work.shutdown_counter.wait_zero_unbounded(), if shutdown_count > 0 => {}
				_ = self.0.sleep.work.websocket_callback.wait_zero_unbounded(), if websocket_count > 0 => {}
			}
		}
	}

	pub(crate) async fn wait_for_http_requests_drained(&self, deadline: Instant) -> bool {
		let Some(counter) = self.http_request_counter() else {
			return true;
		};
		counter.wait_zero(deadline).await
	}

	pub(crate) async fn wait_for_http_requests_idle(&self) {
		loop {
			let idle = self.0.sleep.work.idle_notify.notified();
			tokio::pin!(idle);
			idle.as_mut().enable();

			if self.active_http_request_count() == 0 {
				return;
			}

			idle.await;
		}
	}

	pub(crate) fn keep_awake_region_state(&self) -> RegionGuard {
		self.0.sleep.work.keep_awake_guard()
	}

	pub(crate) fn sleep_keep_awake_count(&self) -> usize {
		self.0.sleep.work.keep_awake.load()
	}

	pub(crate) fn internal_keep_awake_region(&self) -> RegionGuard {
		self.0.sleep.work.internal_keep_awake_guard()
	}

	pub(crate) fn sleep_internal_keep_awake_count(&self) -> usize {
		self.0.sleep.work.internal_keep_awake.load()
	}

	fn active_queue_wait_count(&self) -> usize {
		self.0.active_queue_wait_count.load(Ordering::SeqCst) as usize
	}

	pub(crate) fn websocket_callback_region_state(&self) -> RegionGuard {
		self.0.sleep.work.websocket_callback_guard()
	}

	pub(crate) fn websocket_callback_count(&self) -> usize {
		self.0.sleep.work.websocket_callback.load()
	}

	pub(crate) fn disconnect_callback_region_state(&self) -> RegionGuard {
		self.0.sleep.work.disconnect_callback_guard()
	}

	#[cfg(not(feature = "wasm-runtime"))]
	pub(crate) fn spawn_work_inner<F>(&self, kind: ActorWorkKind, fut: F) -> bool
	where
		F: Future<Output = ()> + Send + 'static,
	{
		if Handle::try_current().is_err() {
			tracing::warn!(
				kind = kind.label(),
				"actor work spawned without tokio runtime"
			);
			return false;
		}

		let policy = kind.policy();
		if policy.aborts_at_shutdown_deadline {
			let mut shutdown_tasks = self.0.sleep.work.shutdown_tasks.lock();
			if self.0.sleep.work.teardown_started.load(Ordering::Acquire) {
				tracing::warn!(
					kind = kind.label(),
					"actor work spawned after teardown; aborting immediately"
				);
				return false;
			}
			// Reap completed tasks so the JoinSet stays bounded to in-flight
			// work. Without this the set is only drained at shutdown, so a
			// long-lived actor that keeps spawning work (for example a workflow
			// loop registering keep-awake tasks each tick) accumulates finished
			// task handles for its entire lifetime.
			while shutdown_tasks.try_join_next().is_some() {}
			let region = self.begin_work_region(kind);
			shutdown_tasks.spawn(self.build_spawned_work_task(kind, policy, region, fut));
		} else {
			let mut unabortable_shutdown_tasks =
				self.0.sleep.work.unabortable_shutdown_tasks.lock();
			if self.0.sleep.work.teardown_started.load(Ordering::Acquire) {
				tracing::warn!(
					kind = kind.label(),
					"actor work spawned after teardown; aborting immediately"
				);
				return false;
			}
			while unabortable_shutdown_tasks.try_join_next().is_some() {}
			let region = self.begin_work_region(kind);
			unabortable_shutdown_tasks
				.spawn(self.build_spawned_work_task(kind, policy, region, fut));
		}
		self.reset_sleep_timer();
		true
	}

	#[cfg(not(feature = "wasm-runtime"))]
	fn build_spawned_work_task<F>(
		&self,
		kind: ActorWorkKind,
		policy: ActorWorkPolicy,
		region: ActorWorkRegion,
		fut: F,
	) -> impl Future<Output = ()> + Send + 'static
	where
		F: Future<Output = ()> + Send + 'static,
	{
		let ctx = self.clone();
		async move {
			let _region = region;
			if policy.aborts_at_shutdown_deadline {
				let shutdown_deadline = ctx.shutdown_deadline_token();
				tokio::select! {
					_ = fut => {}
					_ = shutdown_deadline.cancelled() => {
						tracing::warn!(
							actor_id = %ctx.actor_id(),
							kind = kind.label(),
							reason = "shutdown_deadline_elapsed",
							"actor work cancelled by shutdown deadline"
						);
					}
				}
			} else {
				fut.await;
			}
			ctx.reset_sleep_timer();
		}
		.in_current_span()
	}

	#[cfg(feature = "wasm-runtime")]
	pub(crate) fn spawn_work_inner<F>(&self, kind: ActorWorkKind, fut: F) -> bool
	where
		F: Future<Output = ()> + 'static,
	{
		let mut local_shutdown_tasks = self.0.sleep.work.local_shutdown_tasks.lock();
		if self.0.sleep.work.teardown_started.load(Ordering::Acquire) {
			tracing::warn!(
				kind = kind.label(),
				"actor work spawned after teardown; aborting immediately"
			);
			return false;
		}

		// Reap finished tasks so the Vec stays bounded to in-flight work
		// instead of only draining at shutdown.
		local_shutdown_tasks.retain_mut(|task| matches!(task.complete_rx.try_recv(), Ok(None)));

		let policy = kind.policy();
		let region = self.begin_work_region(kind);
		let ctx = self.clone();
		let (complete_tx, complete_rx) = futures_oneshot::channel();
		let (abort_handle, abort_registration) = AbortHandle::new_pair();
		local_shutdown_tasks.push(LocalShutdownTask {
			abort_handle,
			complete_rx,
			aborts_at_shutdown_deadline: policy.aborts_at_shutdown_deadline,
		});
		drop(local_shutdown_tasks);
		let ctx_for_task = ctx.clone();
		wasm_bindgen_futures::spawn_local(
			async move {
				let task = async move {
					let _region = region;
					if policy.aborts_at_shutdown_deadline {
						let shutdown_deadline = ctx_for_task.shutdown_deadline_token();
						tokio::select! {
							_ = fut => {}
							_ = shutdown_deadline.cancelled() => {
								tracing::warn!(
									actor_id = %ctx_for_task.actor_id(),
									kind = kind.label(),
									reason = "shutdown_deadline_elapsed",
									"actor work cancelled by shutdown deadline"
								);
							}
						}
					} else {
						fut.await;
					}
					let _ = complete_tx.send(());
					ctx_for_task.reset_sleep_timer();
				};
				if Abortable::new(task, abort_registration).await.is_err() {
					ctx.reset_sleep_timer();
				}
			}
			.in_current_span(),
		);
		self.reset_sleep_timer();
		true
	}

	#[cfg(not(feature = "wasm-runtime"))]
	pub(crate) fn track_shutdown_task<F>(&self, fut: F) -> bool
	where
		F: Future<Output = ()> + Send + 'static,
	{
		if Handle::try_current().is_err() {
			tracing::warn!("shutdown task spawned without tokio runtime; running fallback");
			return false;
		}

		let mut shutdown_tasks = self.0.sleep.work.shutdown_tasks.lock();
		if self.0.sleep.work.teardown_started.load(Ordering::Acquire) {
			tracing::warn!("shutdown task spawned after teardown; aborting immediately");
			return false;
		}
		let counter = self.0.sleep.work.shutdown_counter.clone();
		counter.increment();
		let guard = CountGuard::from_incremented(counter);
		let ctx = self.clone();
		// The guard must drop before resetting the sleep timer so the lifecycle
		// task observes the post-completion counter state on the wakeup.
		shutdown_tasks.spawn(
			async move {
				{
					let _guard = guard;
					fut.await;
				}
				ctx.reset_sleep_timer();
			}
			.in_current_span(),
		);
		drop(shutdown_tasks);
		self.reset_sleep_timer();
		true
	}

	#[cfg(feature = "wasm-runtime")]
	pub(crate) fn track_shutdown_task<F>(&self, fut: F) -> bool
	where
		F: Future<Output = ()> + 'static,
	{
		let mut local_shutdown_tasks = self.0.sleep.work.local_shutdown_tasks.lock();
		if self.0.sleep.work.teardown_started.load(Ordering::Acquire) {
			tracing::warn!("shutdown task spawned after teardown; aborting immediately");
			return false;
		}
		let counter = self.0.sleep.work.shutdown_counter.clone();
		counter.increment();
		let guard = CountGuard::from_incremented(counter);
		let ctx = self.clone();
		// Wasm tasks cannot be joined through Tokio. Store an explicit
		// completion channel plus abort handle so teardown can either drain or
		// cancel them using the same policy as native shutdown.
		let (complete_tx, complete_rx) = futures_oneshot::channel();
		let (abort_handle, abort_registration) = AbortHandle::new_pair();
		local_shutdown_tasks.push(LocalShutdownTask {
			abort_handle,
			complete_rx,
			aborts_at_shutdown_deadline: true,
		});
		drop(local_shutdown_tasks);
		let ctx_for_task = ctx.clone();
		wasm_bindgen_futures::spawn_local(
			async move {
				let task = async move {
					{
						let _guard = guard;
						fut.await;
					}
					let _ = complete_tx.send(());
					ctx_for_task.reset_sleep_timer();
				};
				if Abortable::new(task, abort_registration).await.is_err() {
					ctx.reset_sleep_timer();
				}
			}
			.in_current_span(),
		);
		self.reset_sleep_timer();
		true
	}

	pub(crate) fn shutdown_task_count(&self) -> usize {
		self.0.sleep.work.shutdown_counter.load()
	}

	pub(crate) fn mark_shutdown_deadline_reached(&self) {
		self.0
			.sleep
			.work
			.shutdown_deadline_reached
			.store(true, Ordering::Release);
	}

	pub(crate) fn begin_core_dispatched_hook(&self) {
		self.0.sleep.work.core_dispatched_hooks.increment();
		self.reset_sleep_timer();
	}

	pub fn mark_core_dispatched_hook_completed(&self) {
		self.0.sleep.work.core_dispatched_hooks.decrement();
		self.reset_sleep_timer();
	}

	pub(crate) fn core_dispatched_hook_count(&self) -> usize {
		self.0.sleep.work.core_dispatched_hooks.load()
	}

	pub(crate) async fn teardown_sleep_state(&self) {
		let abort_remaining = self
			.0
			.sleep
			.work
			.shutdown_deadline_reached
			.swap(false, Ordering::AcqRel);
		// Normal shutdown drains tracked work. Once the grace deadline fires,
		// teardown switches to cancellation so a stuck waitUntil cannot hold the
		// actor instance forever.
		if abort_remaining {
			self.0
				.sleep
				.work
				.teardown_started
				.store(true, Ordering::Release);
		}

		#[cfg(feature = "wasm-runtime")]
		{
			loop {
				let local_shutdown_tasks = {
					let mut guard = self.0.sleep.work.local_shutdown_tasks.lock();
					let taken = std::mem::take(&mut *guard);
					if taken.is_empty() {
						self.0
							.sleep
							.work
							.teardown_started
							.store(true, Ordering::Release);
						return;
					}
					taken
				};

				if abort_remaining {
					for task in local_shutdown_tasks {
						if task.aborts_at_shutdown_deadline {
							task.abort_handle.abort();
						}
						if task.complete_rx.await.is_err() {
							tracing::debug!("aborted shutdown task during teardown");
						}
					}
					self.0
						.sleep
						.work
						.teardown_started
						.store(true, Ordering::Release);
					return;
				}

				for task in local_shutdown_tasks {
					if task.complete_rx.await.is_err() {
						tracing::debug!("shutdown task completion dropped during teardown");
					}
				}
			}
		}

		#[cfg(not(feature = "wasm-runtime"))]
		loop {
			let mut abortable_shutdown_tasks = {
				let mut guard = self.0.sleep.work.shutdown_tasks.lock();
				let taken = std::mem::take(&mut *guard);
				let mut unabortable_guard = self.0.sleep.work.unabortable_shutdown_tasks.lock();
				let unabortable_taken = std::mem::take(&mut *unabortable_guard);
				if taken.is_empty() && unabortable_taken.is_empty() {
					self.0
						.sleep
						.work
						.teardown_started
						.store(true, Ordering::Release);
					return;
				}
				(taken, unabortable_taken)
			};

			abortable_shutdown_tasks.0.shutdown().await;
			while let Some(result) = abortable_shutdown_tasks.0.join_next().await {
				if let Err(error) = result
					&& !error.is_cancelled()
				{
					tracing::error!(?error, "shutdown task join failed during teardown");
				}
			}
			while let Some(result) = abortable_shutdown_tasks.1.join_next().await {
				if let Err(error) = result
					&& !error.is_cancelled()
				{
					tracing::error!(?error, "shutdown task join failed during teardown");
				}
			}
		}
	}

	pub(crate) fn sleep_state_config(&self) -> ActorConfig {
		self.0.sleep.config.lock().clone()
	}

	pub(crate) fn active_http_request_count(&self) -> usize {
		self.http_request_counter()
			.map(|counter| counter.load())
			.unwrap_or(0)
	}

	pub(crate) fn sleep_activity_notify(&self) -> Arc<Notify> {
		self.0.sleep.work.activity_notify.clone()
	}

	fn http_request_counter(&self) -> Option<Arc<AsyncCounter>> {
		if let Some(counter) = self.0.sleep.http_request_counter.lock().clone() {
			return Some(counter);
		}

		let counter = self.lookup_http_request_counter(self.actor_id())?;
		*self.0.sleep.http_request_counter.lock() = Some(counter.clone());
		Some(counter)
	}

	fn lookup_http_request_counter(&self, actor_id: &str) -> Option<Arc<AsyncCounter>> {
		let envoy_handle = self.0.sleep.envoy_handle.lock().clone();
		let generation = *self.0.sleep.generation.lock();
		let envoy_handle = envoy_handle?;
		let counter = envoy_handle.http_request_counter(actor_id, generation)?;
		counter.register_zero_notify(&self.0.sleep.work.idle_notify);
		// The HTTP counter is owned by envoy-client, so neither increment nor
		// decrement goes through a rivetkit-core guard. Hook every transition
		// into the sleep activity notify so the sleep deadline gets
		// re-evaluated when a request starts or completes.
		let ctx = self.clone();
		counter.register_change_callback(Arc::new(move || {
			ctx.0
				.metrics
				.set_http_requests_active(ctx.active_http_request_count());
			ctx.reset_sleep_timer();
		}));
		self.0.metrics.set_http_requests_active(counter.load());
		Some(counter)
	}
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/sleep.rs"]
mod tests;
