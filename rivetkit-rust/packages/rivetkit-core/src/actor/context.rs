use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as AnyhowContext, Result};
use futures::future::BoxFuture;
use parking_lot::{Mutex, RwLock};
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_client::tunnel::HibernatingWebSocketMetadata;
use rivet_error::ActorSpecifier;
use scc::HashMap as SccHashMap;
use tokio::runtime::Handle;
use tokio::sync::{Mutex as AsyncMutex, Notify, OnceCell, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::ActorConfig;
use crate::actor::connection::{
	ConnHandle, ConnHandles, HibernatableConnectionMetadata, PendingHibernationChanges,
	hibernatable_id_from_slice,
};
use crate::actor::diagnostics::ActorDiagnostics;
use crate::actor::lifecycle_hooks::Reply;
use crate::actor::messages::{ActorEvent, Request, StateDelta};
use crate::actor::metrics::ActorMetrics;
use crate::actor::preload::PreloadedKv;
use crate::actor::queue::{QueueInspectorUpdateCallback, QueueMetadata, QueueWaitActivityCallback};
use crate::actor::schedule::{InternalKeepAwakeCallback, LocalAlarmCallback};
use crate::actor::sleep::{CanSleep, SleepState};
use crate::actor::state::{PendingSave, PersistedActor, RequestSaveOpts};
use crate::actor::task::LifecycleEvent;
use crate::actor::task_types::UserTaskKind;
use crate::actor::work_registry::{ActorWorkKind, CountGuard, RegionGuard};
use crate::error::{ActorLifecycle as ActorLifecycleError, ActorRuntime};
use crate::inspector::{Inspector, InspectorSnapshot};
use crate::kv::Kv;
use crate::sqlite::SqliteDb;
use crate::types::{ActorKey, ConnId, ListOpts, format_actor_key};

/// Shared actor runtime context.
///
/// This public surface is the foreign-runtime contract for `rivetkit-core`.
/// Native Rust, NAPI-backed TypeScript, and future V8 runtimes should be able
/// to drive actor behavior through `ActorFactory` plus the methods exposed here
/// and on the returned runtime objects like `Kv`, `SqliteDb`, schedule APIs,
/// queue APIs, `ConnHandle`, and `WebSocket`.
#[derive(Clone)]
pub struct ActorContext(pub(crate) Arc<ActorContextInner>);

pub(crate) struct ActorContextInner {
	pub(super) kv: Kv,
	sql: SqliteDb,
	// Forced-sync: actor state snapshots are exposed through synchronous
	// accessors and are never held across `.await`.
	pub(super) current_state: RwLock<Vec<u8>>,
	pub(super) persisted: RwLock<PersistedActor>,
	pub(super) last_pushed_alarm: RwLock<Option<i64>>,
	pub(super) state_save_interval: Duration,
	pub(super) state_dirty: AtomicBool,
	pub(super) state_revision: AtomicU64,
	pub(super) save_request_revision: AtomicU64,
	pub(super) save_completed_revision: AtomicU64,
	pub(super) save_completion: Notify,
	pub(super) save_requested: AtomicBool,
	pub(super) save_requested_immediate: AtomicBool,
	// Forced-sync: debounce bookkeeping is updated from sync save-request paths.
	pub(super) save_requested_within_deadline: Mutex<Option<crate::time::Instant>>,
	pub(super) last_save_at: Mutex<Option<crate::time::Instant>>,
	pub(super) pending_save: Mutex<Option<PendingSave>>,
	pub(super) tracked_persist: Mutex<Option<JoinHandle<()>>>,
	pub(super) save_guard: AsyncMutex<()>,
	pub(super) in_flight_state_writes: AtomicUsize,
	pub(super) state_write_completion: Notify,
	pub(super) on_state_change_in_flight: AtomicUsize,
	pub(super) on_state_change_idle: Notify,
	// Forced-sync: hooks are registered and cloned from synchronous runtime
	// wiring slots before use.
	pub(super) request_save_hooks: RwLock<Vec<Arc<dyn Fn(RequestSaveOpts) + Send + Sync>>>,
	// Forced-sync: schedule runtime handles and callbacks are synchronous
	// wiring slots cloned before actor/envoy I/O.
	pub(super) schedule_generation: Mutex<Option<u32>>,
	pub(super) schedule_envoy_handle: Mutex<Option<EnvoyHandle>>,
	pub(super) client_endpoint: OnceLock<String>,
	pub(super) client_token: OnceLock<String>,
	pub(super) client_namespace: OnceLock<String>,
	pub(super) client_pool_name: OnceLock<String>,
	pub(super) schedule_internal_keep_awake: Mutex<Option<InternalKeepAwakeCallback>>,
	pub(super) schedule_local_alarm_callback: Mutex<Option<LocalAlarmCallback>>,
	// Forced-sync: the local alarm timer is aborted from sync paths.
	pub(super) schedule_local_alarm_task: Mutex<Option<JoinHandle<()>>>,
	// Forced-sync: receivers are pushed/taken from sync paths and awaited after
	// being moved out of the lock.
	pub(super) schedule_pending_alarm_writes: Mutex<Vec<oneshot::Receiver<()>>>,
	pub(super) schedule_local_alarm_epoch: AtomicU64,
	pub(super) schedule_alarm_dispatch_enabled: AtomicBool,
	pub(super) schedule_dirty_since_push: AtomicBool,
	#[cfg(test)]
	pub(super) schedule_driver_alarm_cancel_count: AtomicUsize,
	// Forced-sync: queue config is read from sync public methods before blocking
	// on async queue work.
	pub(super) queue_config: Mutex<ActorConfig>,
	pub(super) queue_abort_signal: Mutex<CancellationToken>,
	pub(super) queue_initialize: OnceCell<()>,
	// Forced-sync: startup installs preload before any queue method awaits init.
	pub(super) queue_preloaded_kv: Mutex<Option<PreloadedKv>>,
	pub(super) queue_preloaded_message_entries: Mutex<Option<Vec<(Vec<u8>, Vec<u8>)>>>,
	pub(super) queue_metadata: AsyncMutex<QueueMetadata>,
	pub(super) queue_receive_lock: AsyncMutex<()>,
	pub(super) queue_completion_waiters: SccHashMap<u64, oneshot::Sender<Option<Vec<u8>>>>,
	pub(super) queue_notify: Notify,
	pub(super) active_queue_wait_count: AtomicU32,
	// Forced-sync: callbacks are registered and cloned from synchronous hooks.
	pub(super) queue_wait_activity_callback: Mutex<Option<QueueWaitActivityCallback>>,
	pub(super) queue_inspector_update_callback: Mutex<Option<QueueInspectorUpdateCallback>>,
	// Forced-sync: connection operations expose sync accessors or clone handles
	// before awaiting; connection_disconnect_state serializes disconnect
	// bookkeeping with pending hibernation snapshots.
	pub(super) connection_config: RwLock<ActorConfig>,
	pub(super) connections: RwLock<BTreeMap<ConnId, ConnHandle>>,
	pub(super) pending_hibernation_updates: RwLock<BTreeSet<ConnId>>,
	pub(super) pending_hibernation_removals: RwLock<BTreeSet<ConnId>>,
	pub(super) connection_disconnect_state: Mutex<()>,
	pub(super) sleep: SleepState,
	activity: ActivityState,
	sleep_requested: AtomicBool,
	destroy_requested: AtomicBool,
	destroy_completed: AtomicBool,
	destroy_completion_notify: Notify,
	abort_signal: Mutex<CancellationToken>,
	shutdown_deadline: CancellationToken,
	// Forced-sync: runtime wiring slots are configured through synchronous
	// lifecycle setup and cloned before sending events.
	inspector: RwLock<Option<Inspector>>,
	inspector_attach_count: RwLock<Option<Arc<AtomicU32>>>,
	inspector_overlay_tx: RwLock<Option<broadcast::Sender<Arc<Vec<u8>>>>>,
	actor_events: RwLock<Option<mpsc::UnboundedSender<ActorEvent>>>,
	pub(super) lifecycle_events: RwLock<Option<mpsc::UnboundedSender<LifecycleEvent>>>,
	hibernated_connection_liveness_override: RwLock<Option<BTreeSet<(Vec<u8>, Vec<u8>)>>>,
	pub(super) metrics: ActorMetrics,
	diagnostics: ActorDiagnostics,
	actor_id: String,
	name: String,
	key: ActorKey,
	region: String,
}

#[derive(Debug, Default)]
pub(crate) struct ActivityState {
	dirty: AtomicBool,
}

impl ActivityState {
	fn mark_dirty(&self) -> bool {
		!self.dirty.swap(true, Ordering::AcqRel)
	}

	fn take_dirty(&self) -> bool {
		self.dirty.swap(false, Ordering::AcqRel)
	}
}

impl ActorContext {
	pub fn new(
		actor_id: impl Into<String>,
		name: impl Into<String>,
		key: ActorKey,
		region: impl Into<String>,
	) -> Self {
		Self::build(
			actor_id.into(),
			name.into(),
			key,
			region.into(),
			None,
			String::new(),
			ActorConfig::default(),
			Kv::default(),
			SqliteDb::default(),
		)
	}

	pub fn new_with_kv(
		actor_id: impl Into<String>,
		name: impl Into<String>,
		key: ActorKey,
		region: impl Into<String>,
		kv: Kv,
	) -> Self {
		Self::build(
			actor_id.into(),
			name.into(),
			key,
			region.into(),
			None,
			String::new(),
			ActorConfig::default(),
			kv,
			SqliteDb::default(),
		)
	}

	#[cfg(test)]
	pub(crate) fn new_for_state_tests(kv: Kv, config: ActorConfig) -> Self {
		Self::build(
			"state-test".to_owned(),
			"state-test".to_owned(),
			Vec::new(),
			"local".to_owned(),
			None,
			String::new(),
			config,
			kv,
			SqliteDb::default(),
		)
	}

	pub(crate) fn build(
		actor_id: String,
		name: String,
		key: ActorKey,
		region: String,
		_generation: Option<u32>,
		_envoy_key: String,
		config: ActorConfig,
		kv: Kv,
		sql: SqliteDb,
	) -> Self {
		#[cfg(feature = "sqlite-local")]
		let mut sql = sql;
		#[cfg(feature = "sqlite-local")]
		sql.set_profiling_config(config.sqlite_profiling.clone());
		let metrics =
			ActorMetrics::new_with_sqlite_profiling(name.clone(), config.sqlite_profiling.clone());
		#[cfg(feature = "sqlite-local")]
		sql.set_vfs_metrics(Arc::new(metrics.clone()));
		let diagnostics = ActorDiagnostics::new(actor_id.clone());
		let state_save_interval = config.state_save_interval;
		let abort_signal = CancellationToken::new();
		let shutdown_deadline = CancellationToken::new();
		let sleep = SleepState::new(config.clone());
		let ctx = Self(Arc::new(ActorContextInner {
			kv,
			sql,
			current_state: RwLock::new(Vec::new()),
			persisted: RwLock::new(PersistedActor::default()),
			last_pushed_alarm: RwLock::new(None),
			state_save_interval,
			state_dirty: AtomicBool::new(false),
			state_revision: AtomicU64::new(0),
			save_request_revision: AtomicU64::new(0),
			save_completed_revision: AtomicU64::new(0),
			save_completion: Notify::new(),
			save_requested: AtomicBool::new(false),
			save_requested_immediate: AtomicBool::new(false),
			save_requested_within_deadline: Mutex::new(None),
			last_save_at: Mutex::new(None),
			pending_save: Mutex::new(None),
			tracked_persist: Mutex::new(None),
			save_guard: AsyncMutex::new(()),
			in_flight_state_writes: AtomicUsize::new(0),
			state_write_completion: Notify::new(),
			on_state_change_in_flight: AtomicUsize::new(0),
			on_state_change_idle: Notify::new(),
			request_save_hooks: RwLock::new(Vec::new()),
			schedule_generation: Mutex::new(None),
			schedule_envoy_handle: Mutex::new(None),
			client_endpoint: OnceLock::new(),
			client_token: OnceLock::new(),
			client_namespace: OnceLock::new(),
			client_pool_name: OnceLock::new(),
			schedule_internal_keep_awake: Mutex::new(None),
			schedule_local_alarm_callback: Mutex::new(None),
			schedule_local_alarm_task: Mutex::new(None),
			schedule_pending_alarm_writes: Mutex::new(Vec::new()),
			schedule_local_alarm_epoch: AtomicU64::new(0),
			schedule_alarm_dispatch_enabled: AtomicBool::new(true),
			// A fresh actor context has no in-process record of a successful
			// envoy alarm push yet, so the first sync must always push.
			schedule_dirty_since_push: AtomicBool::new(true),
			#[cfg(test)]
			schedule_driver_alarm_cancel_count: AtomicUsize::new(0),
			queue_config: Mutex::new(config.clone()),
			queue_abort_signal: Mutex::new(abort_signal.clone()),
			queue_initialize: OnceCell::new(),
			queue_preloaded_kv: Mutex::new(None),
			queue_preloaded_message_entries: Mutex::new(None),
			queue_metadata: AsyncMutex::new(QueueMetadata::default()),
			queue_receive_lock: AsyncMutex::new(()),
			queue_completion_waiters: SccHashMap::new(),
			queue_notify: Notify::new(),
			active_queue_wait_count: AtomicU32::new(0),
			queue_wait_activity_callback: Mutex::new(None),
			queue_inspector_update_callback: Mutex::new(None),
			connection_config: RwLock::new(config),
			connections: RwLock::new(BTreeMap::new()),
			pending_hibernation_updates: RwLock::new(BTreeSet::new()),
			pending_hibernation_removals: RwLock::new(BTreeSet::new()),
			connection_disconnect_state: Mutex::new(()),
			sleep,
			activity: ActivityState::default(),
			sleep_requested: AtomicBool::new(false),
			destroy_requested: AtomicBool::new(false),
			destroy_completed: AtomicBool::new(false),
			destroy_completion_notify: Notify::new(),
			abort_signal: Mutex::new(abort_signal),
			shutdown_deadline,
			inspector: RwLock::new(None),
			inspector_attach_count: RwLock::new(None),
			inspector_overlay_tx: RwLock::new(None),
			actor_events: RwLock::new(None),
			lifecycle_events: RwLock::new(None),
			hibernated_connection_liveness_override: RwLock::new(None),
			metrics,
			diagnostics,
			actor_id,
			name,
			key,
			region,
		}));
		ctx.configure_sleep_hooks();
		ctx
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub async fn kv_batch_get(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
		self.0.kv.batch_get(keys).await
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub async fn kv_batch_put(&self, entries: &[(&[u8], &[u8])]) -> Result<()> {
		self.0.kv.batch_put(entries).await
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub async fn kv_batch_delete(&self, keys: &[&[u8]]) -> Result<()> {
		self.0.kv.batch_delete(keys).await
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub async fn kv_delete_range(&self, start: &[u8], end: &[u8]) -> Result<()> {
		self.0.kv.delete_range(start, end).await
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub async fn kv_list_prefix(
		&self,
		prefix: &[u8],
		opts: ListOpts,
	) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
		self.0.kv.list_prefix(prefix, opts).await
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub async fn kv_list_range(
		&self,
		start: &[u8],
		end: &[u8],
		opts: ListOpts,
	) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
		self.0.kv.list_range(start, end, opts).await
	}

	#[deprecated(
		note = "Actor KV is deprecated. Use embedded SQLite (`sql()`) or actor state instead."
	)]
	pub fn kv(&self) -> &Kv {
		&self.0.kv
	}

	/// Internal accessor for the actor KV store. Core uses KV as the backing
	/// store for the inspector token and actor startup persistence. Unlike the
	/// public `kv()` accessor, this is not deprecated because the storage
	/// machinery itself is not going away, only the public actor-facing API.
	pub(crate) fn kv_internal(&self) -> &Kv {
		&self.0.kv
	}

	pub fn sql(&self) -> &SqliteDb {
		&self.0.sql
	}

	pub async fn db_exec(&self, sql: &str) -> Result<Vec<u8>> {
		self.0.sql.exec_rows_cbor(sql).await
	}

	pub async fn db_query(&self, sql: &str, params: Option<&[u8]>) -> Result<Vec<u8>> {
		self.0.sql.query_rows_cbor(sql, params).await
	}

	pub async fn db_execute(&self, sql: &str, params: Option<&[u8]>) -> Result<Vec<u8>> {
		self.0.sql.execute_rows_cbor(sql, params).await
	}

	pub async fn db_run(&self, sql: &str, params: Option<&[u8]>) -> Result<()> {
		self.0.sql.run_cbor(sql, params).await?;
		Ok(())
	}

	pub fn set_alarm(&self, timestamp_ms: Option<i64>) -> Result<()> {
		self.set_schedule_alarm(timestamp_ms)
	}

	/// Resync persisted alarms with the runtime's alarm transport.
	///
	/// Foreign-runtime adapters should call this during startup after loading
	/// any persisted schedule state and before accepting user callbacks that rely
	/// on future alarms being armed.
	pub fn init_alarms(&self) {
		self.sync_future_alarm_logged();
	}

	pub fn queue(&self) -> &Self {
		self
	}

	pub fn sleep(&self) -> Result<()> {
		// `started` is cleared when the lifecycle state machine transitions
		// into SleepGrace / DestroyGrace, so `started=false` covers both
		// "never started" and "already shutting down". Distinguish with the
		// request flags for an accurate diagnostic.
		if !self.0.sleep.lifecycle_started.load(Ordering::SeqCst) {
			let already_stopping = self.0.sleep_requested.load(Ordering::SeqCst)
				|| self.0.destroy_requested.load(Ordering::SeqCst);
			return if already_stopping {
				Err(ActorLifecycleError::Stopping.build()).context("actor is already shutting down")
			} else {
				Err(ActorLifecycleError::Starting.build())
					.context("cannot request sleep before actor startup completes")
			};
		}
		if self.0.sleep_requested.swap(true, Ordering::SeqCst) {
			return Ok(());
		}
		self.cancel_sleep_timer();
		if Handle::try_current().is_ok() {
			let ctx = self.clone();
			let tracked = self.track_shutdown_task(async move {
				ctx.record_user_task_started(UserTaskKind::SleepFinalize);
				let started_at = Instant::now();
				ctx.request_sleep_from_envoy();
				ctx.record_user_task_finished(UserTaskKind::SleepFinalize, started_at.elapsed());
			});
			if tracked {
				return Ok(());
			}
		}

		self.request_sleep_from_envoy();
		Ok(())
	}

	pub fn destroy(&self) -> Result<()> {
		// See `sleep` for why the request flags disambiguate `started=false`.
		// destroy() is allowed after sleep() has been requested because
		// destroy is a stronger signal that escalates an in-flight sleep.
		if !self.0.sleep.lifecycle_started.load(Ordering::SeqCst)
			&& !self.0.sleep_requested.load(Ordering::SeqCst)
			&& !self.0.destroy_requested.load(Ordering::SeqCst)
		{
			return Err(ActorLifecycleError::Starting.build())
				.context("cannot request destroy before actor startup completes");
		}
		if self.0.destroy_requested.swap(true, Ordering::SeqCst) {
			return Err(ActorLifecycleError::Stopping.build())
				.context("destroy already requested for this generation");
		}
		// Reuse the shared teardown sequence used by the registry shutdown path
		// so future changes to `mark_destroy_requested` cannot drift.
		// `destroy_requested` is already true from the swap above. The redundant
		// `store(true)` inside is harmless.
		#[cfg(not(feature = "wasm-runtime"))]
		self.mark_destroy_requested();
		#[cfg(feature = "wasm-runtime")]
		self.mark_destroy_requested_without_spawn();

		let ctx = self.clone();
		if Handle::try_current().is_ok() {
			let tracked = self.track_shutdown_task(async move {
				ctx.record_user_task_started(UserTaskKind::DestroyRequest);
				let started_at = Instant::now();
				ctx.request_destroy_from_envoy();
				ctx.record_user_task_finished(UserTaskKind::DestroyRequest, started_at.elapsed());
			});
			if tracked {
				return Ok(());
			}
		}

		self.request_destroy_from_envoy();
		Ok(())
	}

	pub fn mark_destroy_requested(&self) {
		self.cancel_sleep_timer();
		self.flush_on_shutdown();
		self.0.destroy_requested.store(true, Ordering::SeqCst);
		self.0.destroy_completed.store(false, Ordering::SeqCst);
	}

	#[cfg(feature = "wasm-runtime")]
	fn mark_destroy_requested_without_spawn(&self) {
		self.cancel_sleep_timer();
		self.0.destroy_requested.store(true, Ordering::SeqCst);
		self.0.destroy_completed.store(false, Ordering::SeqCst);
	}

	#[doc(hidden)]
	pub fn cancel_actor_abort_signal(&self) {
		self.0.abort_signal.lock().cancel();
	}

	pub(crate) fn reset_abort_signal_for_start(&self) {
		let mut abort_signal = self.0.abort_signal.lock();
		if !abort_signal.is_cancelled() {
			return;
		}

		// Sleep or destroy cancels the generation abort signal to break actor
		// scoped waits. A restarted actor needs a fresh signal so the next
		// generation can wait normally.
		let next_signal = CancellationToken::new();
		*abort_signal = next_signal.clone();
		*self.0.queue_abort_signal.lock() = next_signal;
	}

	#[doc(hidden)]
	pub fn actor_abort_signal(&self) -> CancellationToken {
		self.0.abort_signal.lock().clone()
	}

	#[doc(hidden)]
	pub fn actor_aborted(&self) -> bool {
		self.0.abort_signal.lock().is_cancelled()
	}

	/// Fires when the shutdown grace deadline has elapsed and core is forcing
	/// cleanup. Foreign-runtime adapters should abort any in-flight shutdown
	/// work (for example `onSleep` / `onDestroy`) when this token is cancelled
	/// so resources like SQLite are not torn down mid-operation.
	#[doc(hidden)]
	pub fn shutdown_deadline_token(&self) -> CancellationToken {
		self.0.shutdown_deadline.clone()
	}

	#[doc(hidden)]
	pub fn cancel_shutdown_deadline(&self) {
		self.0.shutdown_deadline.cancel();
	}

	/// Deprecated no-op. Use `keep_awake` to hold the actor awake for the
	/// duration of a future, or `wait_until` to keep work alive across the
	/// sleep grace period. Retained only for NAPI bridge compatibility.
	#[deprecated(note = "no-op: use `keep_awake` or `wait_until` instead")]
	pub fn set_prevent_sleep(&self, _enabled: bool) {}

	#[deprecated(note = "no-op: always returns false")]
	pub fn prevent_sleep(&self) -> bool {
		false
	}

	#[cfg(not(feature = "wasm-runtime"))]
	pub fn wait_until(&self, future: impl Future<Output = ()> + Send + 'static) {
		self.spawn_work(ActorWorkKind::WaitUntil, future);
	}

	#[cfg(not(feature = "wasm-runtime"))]
	pub fn register_task(&self, future: impl Future<Output = ()> + Send + 'static) {
		self.spawn_work(ActorWorkKind::RegisteredTask, future);
	}

	#[cfg(feature = "wasm-runtime")]
	pub fn wait_until(&self, future: impl Future<Output = ()> + 'static) {
		self.spawn_work(ActorWorkKind::WaitUntil, future);
	}

	#[cfg(feature = "wasm-runtime")]
	pub fn register_task(&self, future: impl Future<Output = ()> + 'static) {
		self.spawn_work(ActorWorkKind::RegisteredTask, future);
	}

	pub async fn keep_awake<F>(&self, future: F) -> F::Output
	where
		F: Future,
	{
		self.track_work(ActorWorkKind::KeepAwake, future).await
	}

	pub fn keep_awake_region(&self) -> KeepAwakeRegion {
		KeepAwakeRegion {
			region: Some(self.begin_work_region(ActorWorkKind::KeepAwake)),
		}
	}

	pub async fn internal_keep_awake<F>(&self, future: F) -> F::Output
	where
		F: Future,
	{
		self.track_work(ActorWorkKind::InternalKeepAwake, future)
			.await
	}

	pub fn keep_awake_count(&self) -> usize {
		self.sleep_keep_awake_count()
	}

	pub fn internal_keep_awake_count(&self) -> usize {
		self.sleep_internal_keep_awake_count()
	}

	pub async fn track_work<F>(&self, kind: ActorWorkKind, future: F) -> F::Output
	where
		F: Future,
	{
		let _region = self.begin_work_region(kind);
		future.await
	}

	#[cfg(not(feature = "wasm-runtime"))]
	pub fn spawn_work<F>(&self, kind: ActorWorkKind, future: F)
	where
		F: Future<Output = ()> + Send + 'static,
	{
		self.spawn_work_inner(kind, future);
	}

	#[cfg(feature = "wasm-runtime")]
	pub fn spawn_work<F>(&self, kind: ActorWorkKind, future: F)
	where
		F: Future<Output = ()> + 'static,
	{
		self.spawn_work_inner(kind, future);
	}

	pub fn begin_work_region(&self, kind: ActorWorkKind) -> ActorWorkRegion {
		ActorWorkRegion {
			guard: Some(ActorWorkGuard::new(self.clone(), kind)),
		}
	}

	pub fn actor_id(&self) -> &str {
		&self.0.actor_id
	}

	pub fn name(&self) -> &str {
		&self.0.name
	}

	pub fn key(&self) -> &ActorKey {
		&self.0.key
	}

	pub(crate) fn actor_specifier(&self) -> Option<ActorSpecifier> {
		Some(
			ActorSpecifier::new(self.actor_id().to_owned(), self.sleep_generation()? as u64)
				.with_key(format_actor_key(self.key())),
		)
	}

	pub(crate) fn attach_actor_to_error(&self, error: anyhow::Error) -> anyhow::Error {
		match self.actor_specifier() {
			Some(actor) => error.context(actor),
			None => error,
		}
	}

	pub fn region(&self) -> &str {
		&self.0.region
	}

	pub fn has_state(&self) -> bool {
		self.0.connection_config.read().has_state
	}

	#[doc(hidden)]
	pub fn record_startup_create_state(&self, duration: Duration) {
		self.0.metrics.observe_create_state(duration);
	}

	#[doc(hidden)]
	pub fn record_startup_create_vars(&self, duration: Duration) {
		self.0.metrics.observe_create_vars(duration);
	}

	pub fn broadcast(&self, name: &str, args: &[u8]) {
		for connection in self.conns() {
			if connection.is_subscribed(name) {
				connection.send(name, args);
			}
		}
	}

	/// Returns a lock-backed iterator over live connections.
	///
	/// Do not hold the returned iterator across `.await`. It keeps a read lock
	/// on the connection map until dropped, which blocks connection writers.
	#[must_use]
	pub fn conns(&self) -> ConnHandles<'_> {
		self.iter_connections()
	}

	pub fn client_endpoint(&self) -> Option<&str> {
		self.0.client_endpoint.get().map(String::as_str)
	}

	pub fn client_token(&self) -> Option<&str> {
		self.0.client_token.get().map(String::as_str)
	}

	pub fn client_namespace(&self) -> Option<&str> {
		self.0.client_namespace.get().map(String::as_str)
	}

	pub fn client_pool_name(&self) -> Option<&str> {
		self.0.client_pool_name.get().map(String::as_str)
	}

	pub fn ack_hibernatable_websocket_message(
		&self,
		gateway_id: &[u8],
		request_id: &[u8],
		server_message_index: u16,
	) -> Result<()> {
		let gateway_id = hibernatable_id_from_slice("gateway_id", gateway_id)?;
		let request_id = hibernatable_id_from_slice("request_id", request_id)?;
		let envoy_handle = self.sleep_envoy_handle().ok_or_else(|| {
			ActorRuntime::NotConfigured {
				component: "hibernatable websocket ack".to_owned(),
			}
			.build()
		})?;
		envoy_handle.send_hibernatable_ws_message_ack(gateway_id, request_id, server_message_index);
		Ok(())
	}

	pub(crate) fn load_persisted_actor(&self, persisted: PersistedActor) {
		self.load_persisted(persisted);
	}

	pub(crate) fn persisted_actor(&self) -> PersistedActor {
		self.persisted()
	}

	/// Dispatches any scheduled actions whose deadline has already passed.
	///
	/// Foreign-runtime adapters should call this after startup callbacks complete
	/// so overdue scheduled work enters the normal actor event loop.
	pub async fn drain_overdue_scheduled_events(&self) -> Result<()> {
		for event in self.due_scheduled_events(now_timestamp_ms()) {
			self.dispatch_scheduled_action(
				&event.event_id,
				event.action,
				event.args.unwrap_or_default(),
			)
			.await;
		}

		self.sync_alarm_logged();
		Ok(())
	}

	pub(crate) fn metrics(&self) -> &ActorMetrics {
		&self.0.metrics
	}

	pub(crate) fn record_user_task_started(&self, kind: UserTaskKind) {
		self.0.metrics.begin_user_task(kind);
	}

	pub(crate) fn record_user_task_finished(&self, kind: UserTaskKind, duration: Duration) {
		self.0.metrics.end_user_task(kind, duration);
	}

	pub(crate) fn record_shutdown_wait(
		&self,
		reason: crate::actor::task_types::ShutdownKind,
		duration: Duration,
	) {
		self.0.metrics.observe_shutdown_wait(reason, duration);
	}

	pub(crate) fn record_shutdown_timeout(&self, reason: crate::actor::task_types::ShutdownKind) {
		self.0.metrics.inc_shutdown_timeout(reason);
	}

	pub(crate) fn record_direct_subsystem_shutdown_warning(
		&self,
		subsystem: &str,
		operation: &str,
	) {
		self.0
			.metrics
			.inc_direct_subsystem_shutdown_warning(subsystem, operation);
	}

	pub(crate) fn warn_work_sent_to_stopping_instance(&self, operation: &'static str) {
		if let Some(suppression) = self.0.diagnostics.record("work_sent_to_stopping_instance") {
			tracing::warn!(
				actor_id = %suppression.actor_id,
				operation,
				per_actor_suppressed = suppression.per_actor_suppressed,
				global_suppressed = suppression.global_suppressed,
				"work sent to stopping actor instance"
			);
		}
	}

	pub(crate) fn warn_self_call_risk(&self, operation: &'static str) {
		if let Some(suppression) = self.0.diagnostics.record("self_call_risk") {
			tracing::warn!(
				actor_id = %suppression.actor_id,
				operation,
				per_actor_suppressed = suppression.per_actor_suppressed,
				global_suppressed = suppression.global_suppressed,
				"actor dispatch may be parked behind the current instance"
			);
		}
	}

	#[cfg(test)]
	pub(crate) fn add_conn(&self, conn: ConnHandle) {
		self.insert_existing(conn);
		self.record_connections_updated();
		self.reset_sleep_timer();
	}

	pub(crate) fn remove_conn(&self, conn_id: &str) -> Option<ConnHandle> {
		let removed = self.remove_existing(conn_id);
		if removed.is_some() {
			self.record_connections_updated();
			self.reset_sleep_timer();
		}
		removed
	}

	pub(crate) fn configure_connection_runtime(&self, config: ActorConfig) {
		self.configure_sleep_state(config.clone());
		self.configure_connection_storage(config);
	}

	pub(crate) fn configure_queue_preload(&self, preloaded_kv: Option<PreloadedKv>) {
		self.configure_preload(preloaded_kv);
	}

	pub(crate) fn configure_actor_events(&self, sender: Option<mpsc::UnboundedSender<ActorEvent>>) {
		*self.0.actor_events.write() = sender;
	}

	pub(crate) fn try_send_actor_event(
		&self,
		event: ActorEvent,
		operation: &'static str,
	) -> Result<()> {
		let sender = self.0.actor_events.read().clone().ok_or_else(|| {
			ActorRuntime::NotConfigured {
				component: "actor event inbox".to_owned(),
			}
			.build()
		})?;
		tracing::debug!(
			actor_id = %self.actor_id(),
			operation,
			event = event.kind(),
			"actor event enqueued"
		);
		sender.send(event).map_err(|_| {
			ActorRuntime::NotConfigured {
				component: "actor event inbox".to_owned(),
			}
			.build()
		})
	}

	#[doc(hidden)]
	pub fn configure_envoy(&self, envoy_handle: EnvoyHandle, generation: Option<u32>) {
		let _ = self
			.0
			.client_endpoint
			.set(envoy_handle.endpoint().to_owned());
		if let Some(token) = envoy_handle.token() {
			let _ = self.0.client_token.set(token.to_owned());
		}
		let _ = self
			.0
			.client_namespace
			.set(envoy_handle.namespace().to_owned());
		let _ = self
			.0
			.client_pool_name
			.set(envoy_handle.pool_name().to_owned());
		self.configure_sleep_envoy(envoy_handle.clone(), generation);
		self.configure_schedule_envoy(envoy_handle, generation);
	}

	pub(crate) async fn connect_conn<F>(
		&self,
		params: Vec<u8>,
		is_hibernatable: bool,
		hibernation: Option<HibernatableConnectionMetadata>,
		request: Option<Request>,
		create_state: F,
	) -> Result<ConnHandle>
	where
		F: Future<Output = Result<Vec<u8>>> + Send,
	{
		let conn = self
			.connect_with_state(params, is_hibernatable, hibernation, request, create_state)
			.await?;
		self.record_connections_updated();
		self.reset_sleep_timer();
		Ok(conn)
	}

	pub(crate) async fn connect_conn_with_prepare<F, P>(
		&self,
		params: Vec<u8>,
		is_hibernatable: bool,
		hibernation: Option<HibernatableConnectionMetadata>,
		request: Option<Request>,
		create_state: F,
		prepare_connection: P,
	) -> Result<ConnHandle>
	where
		F: Future<Output = Result<Vec<u8>>> + Send,
		P: FnOnce(&ConnHandle) -> Result<()>,
	{
		let conn = self
			.connect_with_state_and_prepare(
				params,
				is_hibernatable,
				hibernation,
				request,
				create_state,
				prepare_connection,
			)
			.await?;
		self.record_connections_updated();
		self.reset_sleep_timer();
		Ok(conn)
	}

	pub async fn connect_conn_with_request<F>(
		&self,
		params: Vec<u8>,
		request: Option<Request>,
		create_state: F,
	) -> Result<ConnHandle>
	where
		F: Future<Output = Result<Vec<u8>>> + Send,
	{
		self.connect_conn(params, false, None, request, create_state)
			.await
	}

	pub(crate) fn reconnect_hibernatable_conn(
		&self,
		gateway_id: &[u8],
		request_id: &[u8],
	) -> Result<ConnHandle> {
		self.reconnect_hibernatable(gateway_id, request_id)
	}

	pub async fn disconnect_conn(&self, id: ConnId) -> Result<()> {
		self.disconnect_transport_only(|conn| conn.id() == id).await
	}

	pub async fn disconnect_conns<F>(&self, predicate: F) -> Result<()>
	where
		F: FnMut(&ConnHandle) -> bool,
	{
		self.disconnect_transport_only(predicate).await
	}

	pub(crate) fn request_hibernation_transport_save(&self, conn_id: &str) {
		self.queue_hibernation_update(conn_id.to_owned());
		self.request_save(RequestSaveOpts::default());
	}

	pub(crate) fn request_hibernation_transport_removal(&self, conn_id: impl Into<String>) {
		self.queue_hibernation_removal_inner(conn_id.into());
		self.request_save(RequestSaveOpts::default());
	}

	pub fn queue_hibernation_removal(&self, conn_id: impl Into<String>) {
		self.request_hibernation_transport_removal(conn_id);
	}

	pub fn has_pending_hibernation_changes(&self) -> bool {
		self.has_pending_hibernation_changes_inner()
	}

	pub fn take_pending_hibernation_changes(&self) -> Vec<ConnId> {
		self.pending_hibernation_removals()
	}

	pub fn dirty_hibernatable_conns(&self) -> Vec<ConnHandle> {
		self.dirty_hibernatable_conns_inner()
	}

	pub(crate) fn hibernated_connection_is_live(
		&self,
		gateway_id: &[u8],
		request_id: &[u8],
	) -> Result<bool> {
		let gateway_id = hibernatable_id_from_slice("gateway_id", gateway_id)?;
		let request_id = hibernatable_id_from_slice("request_id", request_id)?;

		if let Some(override_pairs) = self
			.0
			.hibernated_connection_liveness_override
			.read()
			.as_ref()
		{
			return Ok(override_pairs.contains(&(gateway_id.to_vec(), request_id.to_vec())));
		}

		let Some(envoy_handle) = self.sleep_envoy_handle() else {
			return Ok(false);
		};
		let is_live = envoy_handle.hibernatable_connection_is_live(
			self.actor_id(),
			self.sleep_generation(),
			gateway_id,
			request_id,
		);
		Ok(is_live)
	}

	#[cfg(test)]
	pub(crate) fn set_hibernated_connection_liveness_override<I>(&self, pairs: I)
	where
		I: IntoIterator<Item = (Vec<u8>, Vec<u8>)>,
	{
		*self.0.hibernated_connection_liveness_override.write() = Some(pairs.into_iter().collect());
	}

	fn prepare_state_deltas(
		&self,
		deltas: Vec<StateDelta>,
	) -> Result<(Vec<StateDelta>, PendingHibernationChanges)> {
		fn finish_with_error(
			ctx: &ActorContext,
			pending: PendingHibernationChanges,
			error: anyhow::Error,
		) -> Result<(Vec<StateDelta>, PendingHibernationChanges)> {
			ctx.restore_pending_hibernation_changes(pending);
			Err(error)
		}

		let mut next_deltas = Vec::new();
		let mut explicit_updates = std::collections::BTreeMap::new();
		let mut explicit_removals = std::collections::BTreeSet::new();

		for delta in deltas {
			match delta {
				StateDelta::ConnHibernation { conn, bytes } => {
					if let Some(handle) = self.connection(&conn) {
						handle.set_state_initial(bytes.clone());
					}
					explicit_updates.insert(conn, bytes);
				}
				StateDelta::ConnHibernationRemoved(conn) => {
					explicit_removals.insert(conn);
				}
				other => next_deltas.push(other),
			}
		}

		let pending = self.take_pending_hibernation_changes_inner();
		let mut removal_ids = pending.removed.clone();
		removal_ids.extend(explicit_removals.iter().cloned());

		let explicit_update_ids: std::collections::BTreeSet<_> =
			explicit_updates.keys().cloned().collect();

		for (conn, bytes) in explicit_updates {
			if removal_ids.contains(&conn) {
				continue;
			}
			let encoded = match self.encode_hibernation_delta(&conn, bytes) {
				Ok(encoded) => encoded,
				Err(error) => {
					return finish_with_error(self, pending, error);
				}
			};
			next_deltas.push(StateDelta::ConnHibernation {
				conn,
				bytes: encoded,
			});
		}

		for conn in &pending.updated {
			if removal_ids.contains(conn)
				|| explicit_removals.contains(conn)
				|| explicit_update_ids.contains(conn)
			{
				continue;
			}
			let Some(handle) = self.connection(conn) else {
				continue;
			};
			if !handle.is_hibernatable() || handle.hibernation().is_none() {
				continue;
			}
			let encoded = match self.encode_hibernation_delta(conn, handle.state()) {
				Ok(encoded) => encoded,
				Err(error) => {
					return finish_with_error(self, pending, error);
				}
			};
			next_deltas.push(StateDelta::ConnHibernation {
				conn: conn.clone(),
				bytes: encoded,
			});
		}

		for conn in removal_ids {
			next_deltas.push(StateDelta::ConnHibernationRemoved(conn));
		}

		Ok((next_deltas, pending))
	}

	#[cfg(test)]
	pub(crate) async fn restore_hibernatable_connections(&self) -> Result<Vec<ConnHandle>> {
		self.restore_hibernatable_connections_with_preload(None)
			.await
	}

	pub(crate) async fn restore_hibernatable_connections_with_preload(
		&self,
		preloaded_kv: Option<&PreloadedKv>,
	) -> Result<Vec<ConnHandle>> {
		let restored = self.restore_persisted(preloaded_kv).await?;
		if !restored.is_empty() {
			if let Some(envoy_handle) = self.sleep_envoy_handle() {
				let meta_entries: Vec<_> = restored
					.iter()
					.filter_map(|conn| {
						let hibernation = conn.hibernation()?;
						Some(HibernatingWebSocketMetadata {
							gateway_id: hibernation.gateway_id,
							request_id: hibernation.request_id,
							envoy_message_index: hibernation.client_message_index,
							rivet_message_index: hibernation.server_message_index,
							path: hibernation.request_path,
							headers: hibernation.request_headers.into_iter().collect(),
						})
					})
					.collect();
				envoy_handle.restore_hibernating_requests(self.actor_id().to_owned(), meta_entries);
			}
			self.record_connections_updated();
			self.reset_sleep_timer();
		}
		Ok(restored)
	}

	pub(crate) fn configure_inspector(&self, inspector: Option<Inspector>) {
		*self.0.inspector.write() = inspector;
	}

	pub(crate) fn inspector(&self) -> Option<Inspector> {
		self.0.inspector.read().clone()
	}

	pub fn inspector_snapshot(&self) -> InspectorSnapshot {
		self.inspector()
			.map(|inspector| inspector.snapshot())
			.unwrap_or_default()
	}

	pub(crate) fn configure_inspector_runtime(
		&self,
		attach_count: Arc<AtomicU32>,
		overlay_tx: broadcast::Sender<Arc<Vec<u8>>>,
	) {
		*self.0.inspector_attach_count.write() = Some(attach_count);
		*self.0.inspector_overlay_tx.write() = Some(overlay_tx);
	}

	pub(crate) fn inspector_attach(&self) -> Option<InspectorAttachGuard> {
		InspectorAttachGuard::new(self.clone())
	}

	#[cfg(test)]
	pub(crate) fn inspector_attach_count(&self) -> u32 {
		self.inspector_attach_count_arc()
			.map(|attach_count| attach_count.load(Ordering::SeqCst))
			.unwrap_or(0)
	}

	pub(crate) fn subscribe_inspector(&self) -> Option<broadcast::Receiver<Arc<Vec<u8>>>> {
		self.0
			.inspector_overlay_tx
			.read()
			.clone()
			.map(|overlay_tx| overlay_tx.subscribe())
	}

	pub(crate) fn downgrade(&self) -> Weak<ActorContextInner> {
		Arc::downgrade(&self.0)
	}

	pub(crate) fn from_weak(weak: &Weak<ActorContextInner>) -> Option<Self> {
		weak.upgrade().map(Self)
	}

	#[doc(hidden)]
	pub fn set_started(&self, started: bool) {
		self.set_lifecycle_started(started);
		self.reset_sleep_timer();
	}

	#[doc(hidden)]
	pub fn started(&self) -> bool {
		self.lifecycle_started()
	}

	pub(crate) fn destroy_requested(&self) -> bool {
		self.0.destroy_requested.load(Ordering::SeqCst)
	}

	pub fn is_destroy_requested(&self) -> bool {
		self.destroy_requested()
	}

	pub(crate) async fn wait_for_destroy_completion(&self) {
		if self.0.destroy_completed.load(Ordering::SeqCst) {
			return;
		}

		loop {
			let notified = self.0.destroy_completion_notify.notified();
			if self.0.destroy_completed.load(Ordering::SeqCst) {
				return;
			}
			notified.await;
			if self.0.destroy_completed.load(Ordering::SeqCst) {
				return;
			}
		}
	}

	pub async fn wait_for_destroy_completion_public(&self) {
		self.wait_for_destroy_completion().await;
	}

	pub(crate) fn mark_destroy_completed(&self) {
		self.0.destroy_completed.store(true, Ordering::SeqCst);
		self.0.destroy_completion_notify.notify_waiters();
	}

	pub(crate) async fn can_sleep(&self) -> CanSleep {
		self.can_arm_sleep_timer().await
	}

	pub(crate) fn pending_disconnect_count(&self) -> usize {
		self.0.sleep.work.disconnect_callback.load()
	}

	pub async fn with_disconnect_callback<F, Fut, T>(&self, run: F) -> T
	where
		F: FnOnce() -> Fut,
		Fut: Future<Output = T>,
	{
		self.track_work(ActorWorkKind::DisconnectCallback, run())
			.await
	}

	pub(crate) fn configure_lifecycle_events(
		&self,
		sender: Option<mpsc::UnboundedSender<LifecycleEvent>>,
	) {
		*self.0.lifecycle_events.write() = sender;
	}

	pub(crate) fn notify_inspector_serialize_requested(&self) {
		self.try_send_lifecycle_event(
			LifecycleEvent::InspectorSerializeRequested,
			"inspector_serialize_requested",
		);
	}

	pub(crate) fn notify_activity_dirty(&self) -> bool {
		if self.0.lifecycle_events.read().is_none() {
			return false;
		}
		if self.0.activity.mark_dirty() {
			self.sleep_activity_notify().notify_one();
		}
		true
	}

	pub(crate) fn acknowledge_activity_dirty(&self) -> bool {
		self.0.activity.take_dirty()
	}

	/// Notify the ActorTask that a `can_sleep` input has changed so the sleep
	/// deadline gets re-evaluated. Falls back to the detached compat timer
	/// when the actor has no wired `ActorTask` (test-only contexts).
	pub(crate) fn reset_sleep_timer(&self) {
		if self.notify_activity_dirty() {
			return;
		}

		#[cfg(feature = "wasm-runtime")]
		return;

		#[cfg(not(feature = "wasm-runtime"))]
		self.reset_sleep_timer_state();
	}

	fn notify_inspector_attachments_changed(&self) {
		self.try_send_lifecycle_event(
			LifecycleEvent::InspectorAttachmentsChanged,
			"inspector_attachments_changed",
		);
	}

	pub(crate) fn configure_sleep(&self, config: ActorConfig) {
		self.configure_sleep_state(config.clone());
		self.configure_queue(config);
		self.reset_sleep_timer();
	}

	pub(crate) fn sleep_config(&self) -> ActorConfig {
		self.sleep_state_config()
	}

	pub(crate) fn sleep_requested(&self) -> bool {
		self.0.sleep_requested.load(Ordering::SeqCst)
	}

	pub(crate) fn clear_sleep_requested(&self) {
		self.0.sleep_requested.store(false, Ordering::SeqCst);
	}

	pub(crate) async fn internal_keep_awake_task(
		&self,
		future: BoxFuture<'static, Result<()>>,
	) -> Result<()> {
		self.internal_keep_awake(future).await
	}

	pub fn websocket_callback_region(&self) -> WebSocketCallbackRegion {
		WebSocketCallbackRegion {
			region: Some(self.begin_work_region(ActorWorkKind::WebSocketCallback)),
		}
	}

	pub(crate) async fn with_websocket_callback<F, Fut, T>(&self, run: F) -> T
	where
		F: FnOnce() -> Fut,
		Fut: Future<Output = T>,
	{
		let _guard = self.websocket_callback_region();
		run().await
	}

	fn idle_work_region(&self, kind: ActorWorkKind) -> Option<RegionGuard> {
		if !kind.policy().blocks_idle_sleep {
			return None;
		}
		let region = match kind {
			ActorWorkKind::Action => self.internal_keep_awake_region(),
			ActorWorkKind::KeepAwake => self.keep_awake_region_state(),
			ActorWorkKind::InternalKeepAwake => self.internal_keep_awake_region(),
			ActorWorkKind::WaitUntil => return None,
			ActorWorkKind::RegisteredTask => return None,
			ActorWorkKind::WebSocketCallback => self.websocket_callback_region_state(),
			ActorWorkKind::DisconnectCallback => self.disconnect_callback_region_state(),
		};
		Some(region.with_log_fields(kind.label(), Some(self.actor_id().to_owned())))
	}

	fn shutdown_work_region(&self) -> CountGuard {
		let counter = self.0.sleep.work.shutdown_counter.clone();
		counter.increment();
		CountGuard::from_incremented(counter)
	}

	fn configure_sleep_hooks(&self) {
		let keep_awake_ctx = self.downgrade();
		self.0
			.sleep
			.work
			.keep_awake
			.register_change_callback(Arc::new(move || {
				if let Some(ctx) = ActorContext::from_weak(&keep_awake_ctx) {
					ctx.0
						.metrics
						.set_keep_awake_active(ctx.sleep_keep_awake_count());
				}
			}));

		let internal_keep_awake_metric_ctx = self.downgrade();
		self.0
			.sleep
			.work
			.internal_keep_awake
			.register_change_callback(Arc::new(move || {
				if let Some(ctx) = ActorContext::from_weak(&internal_keep_awake_metric_ctx) {
					ctx.0
						.metrics
						.set_internal_keep_awake_active(ctx.sleep_internal_keep_awake_count());
				}
			}));

		let shutdown_tasks_ctx = self.downgrade();
		self.0
			.sleep
			.work
			.shutdown_counter
			.register_change_callback(Arc::new(move || {
				if let Some(ctx) = ActorContext::from_weak(&shutdown_tasks_ctx) {
					ctx.0
						.metrics
						.set_shutdown_tasks_active(ctx.shutdown_task_count());
				}
			}));

		let internal_keep_awake_ctx = self.downgrade();
		self.set_internal_keep_awake(Some(Arc::new(move |future| {
			let ctx = ActorContext::from_weak(&internal_keep_awake_ctx);
			Box::pin(async move {
				let Some(ctx) = ctx else {
					return Err(ActorRuntime::NotConfigured {
						component: "actor context".to_owned(),
					}
					.build());
				};
				ctx.internal_keep_awake_task(future).await
			})
		})));

		let queue_ctx = self.downgrade();
		self.set_wait_activity_callback(Some(Arc::new(move || {
			if let Some(ctx) = ActorContext::from_weak(&queue_ctx) {
				ctx.reset_sleep_timer();
			}
		})));

		let queue_ctx = self.downgrade();
		self.set_inspector_update_callback(Some(Arc::new(move |queue_size| {
			if let Some(ctx) = ActorContext::from_weak(&queue_ctx) {
				ctx.record_queue_updated(queue_size);
			}
		})));
	}

	pub(crate) fn record_state_updated(&self) {
		if let Some(inspector) = self.inspector() {
			inspector.record_state_updated();
		}
	}

	pub(crate) fn record_connections_updated(&self) {
		let Some(inspector) = self.inspector() else {
			return;
		};
		let active_connections = self.active_connection_count();
		inspector.record_connections_updated(active_connections);
	}

	fn record_queue_updated(&self, queue_size: u32) {
		if let Some(inspector) = self.inspector() {
			inspector.record_queue_updated(queue_size);
		}
	}

	pub(crate) async fn save_state_with_revision(
		&self,
		deltas: Vec<StateDelta>,
		save_request_revision: u64,
	) -> Result<()> {
		let (deltas, pending_hibernation_changes) = match self.prepare_state_deltas(deltas) {
			Ok(prepared) => prepared,
			Err(error) => return Err(error),
		};
		if let Err(error) = self.apply_state_deltas(deltas, save_request_revision).await {
			self.restore_pending_hibernation_changes(pending_hibernation_changes);
			return Err(error);
		}
		self.record_state_updated();
		Ok(())
	}

	async fn dispatch_scheduled_action(&self, event_id: &str, action: String, args: Vec<u8>) {
		self.cancel_scheduled_event(event_id);
		let ctx = self.clone();
		let event_id = event_id.to_owned();
		let internal_keep_awake_region = self.begin_work_region(ActorWorkKind::InternalKeepAwake);

		self.track_shutdown_task(async move {
			let _internal_keep_awake_region = internal_keep_awake_region;
			ctx.record_user_task_started(UserTaskKind::ScheduledAction);
			let started_at = Instant::now();
			let action_name = action.clone();
			let (reply_tx, reply_rx) = oneshot::channel();

			match ctx.try_send_actor_event(
				ActorEvent::Action {
					name: action.clone(),
					args,
					conn: None,
					reply: Reply::from(reply_tx),
				},
				"scheduled_action",
			) {
				Ok(()) => match reply_rx.await {
					Ok(Ok(_)) => {}
					Ok(Err(error)) => {
						tracing::error!(
							?error,
							event_id,
							action_name,
							"scheduled event execution failed"
						);
					}
					Err(error) => {
						tracing::error!(
							?error,
							event_id,
							action_name,
							"scheduled event reply dropped"
						);
					}
				},
				Err(error) => {
					tracing::error!(
						?error,
						event_id,
						action_name,
						"failed to enqueue scheduled event"
					);
				}
			}

			ctx.record_user_task_finished(UserTaskKind::ScheduledAction, started_at.elapsed());
		});
	}

	fn inspector_attach_count_arc(&self) -> Option<Arc<AtomicU32>> {
		self.0.inspector_attach_count.read().clone()
	}

	fn try_send_lifecycle_event(&self, event: LifecycleEvent, operation: &'static str) {
		let Some(sender) = self.0.lifecycle_events.read().clone() else {
			return;
		};

		if sender.send(event).is_err() {
			tracing::warn!(operation, "failed to enqueue actor lifecycle event");
		}
	}
}

fn now_timestamp_ms() -> i64 {
	let duration = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default();
	i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

struct ActorWorkGuard {
	ctx: ActorContext,
	kind: ActorWorkKind,
	started_at: Option<Instant>,
	idle_region: Option<RegionGuard>,
	shutdown_region: Option<CountGuard>,
}

#[must_use]
pub struct ActorWorkRegion {
	guard: Option<ActorWorkGuard>,
}

impl ActorWorkGuard {
	fn new(ctx: ActorContext, kind: ActorWorkKind) -> Self {
		let policy = kind.policy();
		let idle_region = ctx.idle_work_region(kind);
		let shutdown_region = if policy.drains_shutdown_grace {
			Some(ctx.shutdown_work_region())
		} else {
			None
		};
		let started_at = if let Some(user_task_kind) = policy.user_task_kind {
			ctx.record_user_task_started(user_task_kind);
			Some(Instant::now())
		} else {
			None
		};
		ctx.reset_sleep_timer();
		Self {
			ctx,
			kind,
			started_at,
			idle_region,
			shutdown_region,
		}
	}
}

impl Drop for ActorWorkGuard {
	fn drop(&mut self) {
		if let Some(started_at) = self.started_at.take()
			&& let Some(user_task_kind) = self.kind.policy().user_task_kind
		{
			self.ctx
				.record_user_task_finished(user_task_kind, started_at.elapsed());
		}
		self.idle_region.take();
		self.shutdown_region.take();
		self.ctx.reset_sleep_timer();
	}
}

impl Drop for ActorWorkRegion {
	fn drop(&mut self) {
		self.guard.take();
	}
}

#[must_use]
#[derive(Debug)]
pub(crate) struct InspectorAttachGuard {
	ctx: ActorContext,
}

impl InspectorAttachGuard {
	fn new(ctx: ActorContext) -> Option<Self> {
		let attach_count = ctx.inspector_attach_count_arc()?;
		let previous = attach_count.fetch_add(1, Ordering::SeqCst);
		let current = previous.saturating_add(1);
		tracing::debug!(
			actor_id = %ctx.actor_id(),
			previous_count = previous,
			current_count = current,
			"inspector attached"
		);
		if previous == 0 {
			ctx.notify_inspector_attachments_changed();
		}
		Some(Self { ctx })
	}
}

impl Drop for InspectorAttachGuard {
	fn drop(&mut self) {
		let Some(attach_count) = self.ctx.inspector_attach_count_arc() else {
			return;
		};
		let Ok(previous) =
			attach_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
				current.checked_sub(1)
			})
		else {
			return;
		};
		let current = previous.saturating_sub(1);
		tracing::debug!(
			actor_id = %self.ctx.actor_id(),
			previous_count = previous,
			current_count = current,
			"inspector detached"
		);
		if previous == 1 {
			self.ctx.notify_inspector_attachments_changed();
		}
	}
}

pub struct WebSocketCallbackRegion {
	region: Option<ActorWorkRegion>,
}

pub struct KeepAwakeRegion {
	region: Option<ActorWorkRegion>,
}

impl Drop for WebSocketCallbackRegion {
	fn drop(&mut self) {
		self.region.take();
	}
}

impl Drop for KeepAwakeRegion {
	fn drop(&mut self) {
		// Take the region explicitly to mirror WebSocketCallbackRegion.
		self.region.take();
	}
}

impl std::fmt::Debug for ActorContext {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ActorContext")
			.field("actor_id", &self.0.actor_id)
			.field("name", &self.0.name)
			.field("key", &self.0.key)
			.field("region", &self.0.region)
			.finish()
	}
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/context.rs"]
pub(crate) mod tests;
