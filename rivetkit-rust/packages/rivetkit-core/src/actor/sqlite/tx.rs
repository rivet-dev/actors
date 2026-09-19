use std::{
	collections::{BTreeMap, VecDeque},
	error::Error,
	fmt,
	future::Future,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use anyhow::{Context, Result};
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use tokio::sync::oneshot;
use tokio::sync::{
	Mutex as AsyncMutex, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, OwnedSemaphorePermit, RwLock,
	Semaphore, TryAcquireError,
};
use tokio_util::sync::CancellationToken;

#[cfg(not(target_arch = "wasm32"))]
use crate::runtime::RuntimeSpawner;
use crate::telemetry::SqliteOperation;

#[cfg(feature = "sqlite-local")]
use super::profiling::{FINGERPRINT_FORMAT_VERSION, TransactionProfile};
use super::{BindParam, ExecuteResult, QueryResult, SqliteDb, report_sqlite_worker_fatal};

pub const DEFAULT_TRANSACTION_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(not(feature = "sqlite-local"))]
const MAX_TRANSACTION_NAME_BYTES: usize = 128;
pub const TRANSACTION_COORDINATOR_QUEUE_CAPACITY: usize = 128;
pub(super) const TRANSACTION_TERMINAL_CAPACITY: usize = 1024;
#[cfg(feature = "sqlite-local")]
static UNNAMED_TRANSACTION_WARNINGS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct SqliteTransaction {
	db: SqliteDb,
	key: String,
}

impl SqliteTransaction {
	pub fn key(&self) -> &str {
		&self.key
	}

	pub async fn exec(&self, sql: impl Into<String>) -> Result<QueryResult> {
		self.db.transaction_exec(&self.key, sql.into()).await
	}

	pub async fn execute(
		&self,
		sql: impl Into<String>,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		self.db
			.transaction_execute(&self.key, sql.into(), params)
			.await
	}

	pub async fn commit(&self) -> Result<()> {
		self.db.finish_transaction(&self.key, true).await
	}

	pub async fn rollback(&self) -> Result<()> {
		self.db.finish_transaction(&self.key, false).await
	}

	pub async fn expire(&self) -> Result<()> {
		self.db.expire_transaction(&self.key).await
	}
}

pub(super) struct TransactionCoordinator {
	pub(super) gate: Arc<RwLock<()>>,
	pub(super) admission: Arc<Semaphore>,
	pub(super) state: AsyncMutex<TransactionCoordinatorState>,
	epoch: AtomicU64,
	waiters: AtomicU64,
}

pub(super) struct TransactionCoordinatorState {
	pub(super) active: Option<ActiveTransaction>,
	pub(super) terminal: BTreeMap<String, TransactionTerminalState>,
	pub(super) terminal_order: VecDeque<String>,
	pub(super) poisoned: BTreeMap<String, Duration>,
	pub(super) last_expired_timeout: Option<Duration>,
	pub(super) closed: bool,
}

pub(super) struct ActiveTransaction {
	key: String,
	timeout: Duration,
	pub(super) expiring: bool,
	connection_lost: bool,
	remote_session: Option<u64>,
	gate_guard: Option<OwnedRwLockWriteGuard<()>>,
	operation: Arc<AsyncMutex<()>>,
	timeout_task: Option<CancellationToken>,
	#[cfg(feature = "sqlite-local")]
	profile: Option<TransactionProfile>,
}

#[derive(Clone, Copy)]
pub(super) enum TransactionTerminalState {
	Committed,
	RolledBack,
	Expired(Duration),
	ConnectionLost,
}

pub(super) struct RegularOperationGuard {
	_gate: OwnedRwLockReadGuard<()>,
	_permit: OwnedSemaphorePermit,
}

struct CoordinatorWaitGuard<'a> {
	db: &'a SqliteDb,
}

impl<'a> CoordinatorWaitGuard<'a> {
	#[cfg(feature = "sqlite-local")]
	fn new(db: &'a SqliteDb) -> Option<Self> {
		if !db.profiling.config.enabled {
			return None;
		}
		let metrics = db.vfs_metrics.as_ref()?;
		let _depth = db
			.transaction_coordinator
			.waiters
			.fetch_add(1, Ordering::AcqRel)
			.saturating_add(1);
		metrics.set_coordinator_queue_depth(_depth);
		Some(Self { db })
	}

	#[cfg(not(feature = "sqlite-local"))]
	fn new(_db: &'a SqliteDb) -> Option<Self> {
		None
	}
}

impl Drop for CoordinatorWaitGuard<'_> {
	fn drop(&mut self) {
		let _depth = self
			.db
			.transaction_coordinator
			.waiters
			.fetch_sub(1, Ordering::AcqRel)
			.saturating_sub(1);
		#[cfg(feature = "sqlite-local")]
		if let Some(metrics) = &self.db.vfs_metrics {
			metrics.set_coordinator_queue_depth(_depth);
		}
	}
}

impl Default for TransactionCoordinator {
	fn default() -> Self {
		Self {
			gate: Arc::new(RwLock::new(())),
			admission: Arc::new(Semaphore::new(TRANSACTION_COORDINATOR_QUEUE_CAPACITY)),
			state: AsyncMutex::new(TransactionCoordinatorState {
				active: None,
				terminal: BTreeMap::new(),
				terminal_order: VecDeque::new(),
				poisoned: BTreeMap::new(),
				last_expired_timeout: None,
				closed: false,
			}),
			epoch: AtomicU64::new(0),
			waiters: AtomicU64::new(0),
		}
	}
}

impl SqliteDb {
	pub(super) fn try_transaction_admission(&self) -> Result<OwnedSemaphorePermit> {
		match Arc::clone(&self.transaction_coordinator.admission).try_acquire_owned() {
			Ok(permit) => Ok(permit),
			Err(TryAcquireError::NoPermits) => Err(transaction_queue_full_error()),
			Err(TryAcquireError::Closed) => Err(transaction_coordinator_closed_error()),
		}
	}

	pub(super) async fn begin_regular_operation(&self) -> Result<RegularOperationGuard> {
		let epoch = self.transaction_coordinator.epoch.load(Ordering::Acquire);
		let permit = self.try_transaction_admission()?;
		let wait = CoordinatorWaitGuard::new(self);
		let gate = Arc::clone(&self.transaction_coordinator.gate)
			.read_owned()
			.await;
		drop(wait);
		let state = self.transaction_coordinator.state.lock().await;
		if state.closed {
			return Err(transaction_coordinator_closed_error());
		}
		if self.transaction_coordinator.epoch.load(Ordering::Acquire) != epoch {
			return Err(transaction_expired_error(
				state
					.last_expired_timeout
					.unwrap_or(DEFAULT_TRANSACTION_TIMEOUT),
			));
		}
		drop(state);
		Ok(RegularOperationGuard {
			_gate: gate,
			_permit: permit,
		})
	}

	pub async fn begin_transaction(&self, timeout: Option<Duration>) -> Result<SqliteTransaction> {
		self.begin_named_transaction(None, timeout).await
	}

	pub async fn begin_named_transaction(
		&self,
		name: Option<&str>,
		timeout: Option<Duration>,
	) -> Result<SqliteTransaction> {
		#[cfg(feature = "sqlite-local")]
		let max_name_bytes = self.profiling.config.max_transaction_name_bytes;
		#[cfg(not(feature = "sqlite-local"))]
		let max_name_bytes = MAX_TRANSACTION_NAME_BYTES;
		if let Some(name) = name {
			if name.is_empty() {
				return Err(transaction_invalid_argument_error(
					"transaction name must not be empty",
				));
			}
			if name.len() > max_name_bytes {
				return Err(transaction_invalid_argument_error(
					"transaction name exceeds the configured byte limit",
				));
			}
		}
		self.begin_transaction_with_key_and_name(
			uuid::Uuid::new_v4().to_string(),
			name.map(ToOwned::to_owned),
			timeout,
		)
		.await
	}

	pub async fn begin_transaction_with_key(
		&self,
		key: impl Into<String>,
		timeout: Option<Duration>,
	) -> Result<SqliteTransaction> {
		self.begin_transaction_with_key_and_name(key, None, timeout)
			.await
	}

	async fn begin_transaction_with_key_and_name(
		&self,
		key: impl Into<String>,
		name: Option<String>,
		timeout: Option<Duration>,
	) -> Result<SqliteTransaction> {
		#[cfg(feature = "sqlite-local")]
		let started_at = (self.profiling.config.enabled
			&& self.backend() == super::SqliteBackend::LocalNative)
			.then(crate::time::Instant::now);
		let key = key.into();
		let timeout = timeout.unwrap_or(DEFAULT_TRANSACTION_TIMEOUT);
		if key.is_empty() {
			return Err(transaction_invalid_argument_error(
				"transaction key must not be empty",
			));
		}
		if timeout.is_zero() {
			return Err(transaction_invalid_argument_error(
				"transaction timeout must be greater than zero",
			));
		}

		let db = self.clone();
		let task = run_detached_transaction_task(
			async move {
				db.begin_transaction_profiled_inner(
					key,
					timeout,
					name,
					#[cfg(feature = "sqlite-local")]
					started_at,
				)
				.await
			},
			"sqlite transaction begin task failed",
		);
		self.traced(SqliteOperation::TransactionBegin, task).await
	}

	#[cfg(test)]
	pub(super) async fn begin_transaction_inner(
		&self,
		key: String,
		timeout: Duration,
	) -> Result<SqliteTransaction> {
		self.begin_transaction_profiled_inner(
			key,
			timeout,
			None,
			#[cfg(feature = "sqlite-local")]
			(self.profiling.config.enabled && self.backend() == super::SqliteBackend::LocalNative)
				.then(crate::time::Instant::now),
		)
		.await
	}

	async fn begin_transaction_profiled_inner(
		&self,
		key: String,
		timeout: Duration,
		_name: Option<String>,
		#[cfg(feature = "sqlite-local")] started_at: Option<crate::time::Instant>,
	) -> Result<SqliteTransaction> {
		#[cfg(feature = "sqlite-local")]
		let transaction_wait_started_at = started_at.map(|_| crate::time::Instant::now());
		let epoch = self.transaction_coordinator.epoch.load(Ordering::Acquire);
		let permit = self.try_transaction_admission()?;
		let wait = CoordinatorWaitGuard::new(self);
		let gate_guard = Arc::clone(&self.transaction_coordinator.gate)
			.write_owned()
			.await;
		drop(wait);
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = transaction_wait_started_at.map(|started| started.elapsed());
		{
			let state = self.transaction_coordinator.state.lock().await;
			if state.closed {
				return Err(transaction_coordinator_closed_error());
			}
			if self.transaction_coordinator.epoch.load(Ordering::Acquire) != epoch {
				return Err(transaction_expired_error(
					state
						.last_expired_timeout
						.unwrap_or(DEFAULT_TRANSACTION_TIMEOUT),
				));
			}
			if let Some(error) = transaction_known_state_error(&state, &key) {
				return Err(error);
			}
		}

		#[cfg(feature = "sqlite-local")]
		let begin_started_at = started_at.map(|_| crate::time::Instant::now());
		#[cfg(feature = "sqlite-local")]
		let (begin_result, begin_profile) = if begin_started_at.is_some() {
			let profiled = self
				.execute_backend_profiled("BEGIN".to_owned(), None)
				.await;
			(
				profiled.result.map(|result| (result, None)),
				profiled.profile,
			)
		} else {
			(
				self.execute_backend_in_session("BEGIN".to_owned(), None, None)
					.await,
				None,
			)
		};
		#[cfg(feature = "sqlite-local")]
		let begin_duration = begin_started_at.map(|started| started.elapsed());
		#[cfg(not(feature = "sqlite-local"))]
		let begin_result = self
			.execute_backend_in_session("BEGIN".to_owned(), None, None)
			.await;
		let (_, remote_session) = begin_result.map_err(map_transaction_connection_error)?;
		// A successful BEGIN response and the coordinator state update are two
		// separate async events. If the socket disconnected in that narrow gap,
		// pegboard-envoy has already dropped the connection-owned database handle
		// and rolled the transaction back. Never publish a handle for that stale
		// transaction.
		if let Some(session) = remote_session
			&& self.handle()?.connection_session() != Some(session)
		{
			return Err(transaction_connection_lost_error());
		}
		let operation = Arc::new(AsyncMutex::new(()));
		{
			let mut state = self.transaction_coordinator.state.lock().await;
			if state.closed {
				drop(state);
				if let Err(error) = self
					.execute_backend_in_session("ROLLBACK".to_owned(), None, remote_session)
					.await
				{
					tracing::error!(%error, "sqlite rollback after begin raced shutdown failed");
				}
				return Err(transaction_coordinator_closed_error());
			}
			state.active = Some(ActiveTransaction {
				key: key.clone(),
				timeout,
				expiring: false,
				connection_lost: false,
				remote_session,
				gate_guard: Some(gate_guard),
				operation,
				timeout_task: None,
				#[cfg(feature = "sqlite-local")]
				profile: started_at.map(|started_at| {
					let mut profile = TransactionProfile::new(
						_name,
						started_at,
						self.profiling.config.max_statements_per_transaction_trace,
					);
					profile.transaction_wait_ns = transaction_wait
						.unwrap_or_default()
						.as_nanos()
						.try_into()
						.unwrap_or(u64::MAX);
					let default_profile = depot_client::vfs::SqliteOperationProfile::default();
					profile.record_control(
						begin_profile.as_ref().unwrap_or(&default_profile),
						begin_duration
							.unwrap_or_default()
							.as_nanos()
							.try_into()
							.unwrap_or(u64::MAX),
						false,
					);
					profile
				}),
			});
		}
		drop(permit);

		let timeout_task = CancellationToken::new();
		let mut state = self.transaction_coordinator.state.lock().await;
		if let Some(active) = state.active.as_mut().filter(|active| active.key == key) {
			active.timeout_task = Some(timeout_task.clone());
		} else {
			timeout_task.cancel();
		}
		drop(state);
		spawn_transaction_timeout(
			self.clone(),
			key.clone(),
			timeout,
			timeout_task,
			remote_session,
		);

		Ok(SqliteTransaction {
			db: self.clone(),
			key,
		})
	}

	async fn transaction_operation(&self, key: &str) -> Result<(Arc<AsyncMutex<()>>, Option<u64>)> {
		let state = self.transaction_coordinator.state.lock().await;
		if state.closed {
			return Err(transaction_coordinator_closed_error());
		}
		if let Some(active) = state.active.as_ref().filter(|active| active.key == key) {
			if active.expiring {
				if active.connection_lost {
					return Err(transaction_connection_lost_error());
				}
				return Err(transaction_expired_owner_error(key, active.timeout));
			}
			return Ok((Arc::clone(&active.operation), active.remote_session));
		}
		if let Some(error) = transaction_known_state_error(&state, key) {
			return Err(error);
		}
		Err(transaction_unknown_error(key))
	}

	async fn ensure_active_transaction(&self, key: &str) -> Result<()> {
		let state = self.transaction_coordinator.state.lock().await;
		if state.closed {
			return Err(transaction_coordinator_closed_error());
		}
		if let Some(active) = state.active.as_ref().filter(|active| active.key == key) {
			if active.expiring {
				if active.connection_lost {
					return Err(transaction_connection_lost_error());
				}
				return Err(transaction_expired_owner_error(key, active.timeout));
			}
			return Ok(());
		}
		if let Some(error) = transaction_known_state_error(&state, key) {
			return Err(error);
		}
		Err(transaction_unknown_error(key))
	}

	async fn transaction_exec(&self, key: &str, sql: String) -> Result<QueryResult> {
		let db = self.clone();
		let key = key.to_owned();
		let task = run_detached_transaction_task(
			async move { db.transaction_exec_inner(&key, sql).await },
			"sqlite transaction exec task failed",
		);
		self.traced(SqliteOperation::TransactionExec, task).await
	}

	async fn transaction_exec_inner(&self, key: &str, sql: String) -> Result<QueryResult> {
		#[cfg(feature = "sqlite-local")]
		let started_at = (self.profiling.config.enabled
			&& self.backend() == super::SqliteBackend::LocalNative)
			.then(crate::time::Instant::now);
		let (operation, remote_session) = self.transaction_operation(key).await?;
		let _operation = operation.lock().await;
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = started_at.map(|started| started.elapsed());
		self.ensure_active_transaction(key).await?;
		#[cfg(feature = "sqlite-local")]
		if let Some(started_at) = started_at {
			let sql_for_profile = sql.clone();
			let profiled = self.exec_backend_profiled(sql).await;
			let result = profiled.result;
			let profile = profiled.profile;
			if let Some(observation) = self.observe_statement_profile(
				&sql_for_profile,
				started_at,
				transaction_wait.unwrap_or_default(),
				profile,
				if result.is_ok() { "success" } else { "error" },
				"explicit",
			) {
				self.record_transaction_statement(key, &observation).await;
			}
			return match result {
				Ok(result) => Ok(result),
				Err(error) => Err(self.handle_transaction_backend_error(key, error).await),
			};
		}
		match self.exec_backend_in_session(sql, remote_session).await {
			Ok((result, _)) => Ok(result),
			Err(error) => Err(self.handle_transaction_backend_error(key, error).await),
		}
	}

	async fn transaction_execute(
		&self,
		key: &str,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		let db = self.clone();
		let key = key.to_owned();
		let task = run_detached_transaction_task(
			async move { db.transaction_execute_inner(&key, sql, params).await },
			"sqlite transaction execute task failed",
		);
		self.traced(SqliteOperation::TransactionExecute, task).await
	}

	async fn transaction_execute_inner(
		&self,
		key: &str,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		#[cfg(feature = "sqlite-local")]
		let started_at = (self.profiling.config.enabled
			&& self.backend() == super::SqliteBackend::LocalNative)
			.then(crate::time::Instant::now);
		let (operation, remote_session) = self.transaction_operation(key).await?;
		let _operation = operation.lock().await;
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = started_at.map(|started| started.elapsed());
		self.ensure_active_transaction(key).await?;
		#[cfg(feature = "sqlite-local")]
		if let Some(started_at) = started_at {
			let sql_for_profile = sql.clone();
			let profiled = self.execute_backend_profiled(sql, params).await;
			let result = profiled.result;
			let profile = profiled.profile;
			if let Some(observation) = self.observe_statement_profile(
				&sql_for_profile,
				started_at,
				transaction_wait.unwrap_or_default(),
				profile,
				if result.is_ok() { "success" } else { "error" },
				"explicit",
			) {
				self.record_transaction_statement(key, &observation).await;
			}
			return match result {
				Ok(result) => Ok(result),
				Err(error) => Err(self.handle_transaction_backend_error(key, error).await),
			};
		}
		match self
			.execute_backend_in_session(sql, params, remote_session)
			.await
		{
			Ok((result, _)) => Ok(result),
			Err(error) => Err(self.handle_transaction_backend_error(key, error).await),
		}
	}

	#[cfg(feature = "sqlite-local")]
	async fn record_transaction_statement(
		&self,
		key: &str,
		observation: &super::profiling::StatementObservation,
	) {
		let mut state = self.transaction_coordinator.state.lock().await;
		if let Some(active) = state.active.as_mut().filter(|active| active.key == key) {
			if let Some(profile) = &mut active.profile {
				profile.record_statement(observation);
			}
		}
	}

	async fn finish_transaction(&self, key: &str, commit: bool) -> Result<()> {
		let db = self.clone();
		let key = key.to_owned();
		let operation = if commit {
			SqliteOperation::TransactionCommit
		} else {
			SqliteOperation::TransactionRollback
		};
		let task = run_detached_transaction_task(
			async move { db.finish_transaction_inner(&key, commit).await },
			"sqlite transaction finish task failed",
		);
		self.traced(operation, task).await
	}

	async fn finish_transaction_inner(&self, key: &str, commit: bool) -> Result<()> {
		let (operation, remote_session) = self.transaction_operation(key).await?;
		let _operation = operation.lock().await;
		self.ensure_active_transaction(key).await?;

		let statement = if commit { "COMMIT" } else { "ROLLBACK" };
		#[cfg(feature = "sqlite-local")]
		let control_started_at = (self.profiling.config.enabled
			&& self.backend() == super::SqliteBackend::LocalNative)
			.then(crate::time::Instant::now);
		#[cfg(feature = "sqlite-local")]
		let (result, control_profile) = if control_started_at.is_some() {
			let profiled = self
				.execute_backend_profiled(statement.to_owned(), None)
				.await;
			(
				profiled.result.map(|result| (result, None)),
				profiled.profile,
			)
		} else {
			(
				self.execute_backend_in_session(statement.to_owned(), None, remote_session)
					.await,
				None,
			)
		};
		#[cfg(not(feature = "sqlite-local"))]
		let result = self
			.execute_backend_in_session(statement.to_owned(), None, remote_session)
			.await;
		#[cfg(feature = "sqlite-local")]
		if let Some(control_started_at) = control_started_at {
			self.record_transaction_control(
				key,
				control_profile.as_ref(),
				control_started_at.elapsed(),
				commit,
			)
			.await;
		}
		if let Err(primary) = result {
			if is_remote_connection_error(&primary) {
				self.release_transaction(key, TransactionTerminalState::ConnectionLost, false)
					.await;
				return Err(self.attach_actor(map_transaction_connection_error(primary)));
			}
			if commit {
				let rollback_result = self
					.execute_backend_in_session("ROLLBACK".to_owned(), None, remote_session)
					.await;
				let rollback_failed = rollback_result
					.as_ref()
					.is_err_and(|error| !is_no_active_transaction_error(error));
				self.release_transaction(
					key,
					TransactionTerminalState::RolledBack,
					rollback_failed,
				)
				.await;
				if let Err(rollback) = rollback_result
					&& rollback_failed
				{
					tracing::error!(%rollback, "sqlite rollback after failed commit failed");
				}
				return Err(self.attach_actor(primary));
			}
			if is_no_active_transaction_error(&primary) {
				self.release_transaction(key, TransactionTerminalState::RolledBack, false)
					.await;
				return Ok(());
			}

			self.release_transaction(key, TransactionTerminalState::RolledBack, true)
				.await;
			return Err(self.attach_actor(primary));
		}

		self.release_transaction(
			key,
			if commit {
				TransactionTerminalState::Committed
			} else {
				TransactionTerminalState::RolledBack
			},
			false,
		)
		.await;
		Ok(())
	}

	#[cfg(feature = "sqlite-local")]
	async fn record_transaction_control(
		&self,
		key: &str,
		profile: Option<&depot_client::vfs::SqliteOperationProfile>,
		duration: Duration,
		is_commit: bool,
	) {
		let mut state = self.transaction_coordinator.state.lock().await;
		if let Some(active) = state.active.as_mut().filter(|active| active.key == key) {
			if let Some(transaction_profile) = &mut active.profile {
				let default_profile = depot_client::vfs::SqliteOperationProfile::default();
				transaction_profile.record_control(
					profile.unwrap_or(&default_profile),
					duration.as_nanos().try_into().unwrap_or(u64::MAX),
					is_commit,
				);
			}
		}
	}

	async fn expire_transaction(&self, key: &str) -> Result<()> {
		let db = self.clone();
		let key = key.to_owned();
		run_detached_transaction_task(
			async move { db.expire_transaction_inner(&key).await },
			"sqlite transaction expiry task failed",
		)
		.await
	}

	async fn expire_transaction_inner(&self, key: &str) -> Result<()> {
		let (operation, timeout, remote_session) = {
			let mut state = self.transaction_coordinator.state.lock().await;
			if state.closed {
				return Err(transaction_coordinator_closed_error());
			}
			if let Some(active) = state.active.as_mut().filter(|active| active.key == key) {
				active.expiring = true;
				(
					Arc::clone(&active.operation),
					active.timeout,
					active.remote_session,
				)
			} else if let Some(error) = transaction_known_state_error(&state, key) {
				return Err(error);
			} else {
				return Err(transaction_unknown_error(key));
			}
		};
		let _operation = operation.lock().await;
		{
			let state = self.transaction_coordinator.state.lock().await;
			if state.closed {
				return Err(transaction_coordinator_closed_error());
			}
			if !state
				.active
				.as_ref()
				.is_some_and(|active| active.key == key)
			{
				if let Some(error) = transaction_known_state_error(&state, key) {
					return Err(error);
				}
				return Err(transaction_unknown_error(key));
			}
		}
		// The deadline only wins after any operation that already owned the
		// transaction settles. A commit or rollback that acquired the operation
		// lock first is allowed to finish without poisoning parked work.
		self.transaction_coordinator
			.epoch
			.fetch_add(1, Ordering::AcqRel);
		let rollback = self
			.execute_backend_in_session("ROLLBACK".to_owned(), None, remote_session)
			.await;
		let rollback_failed = rollback.as_ref().is_err_and(|error| {
			!is_no_active_transaction_error(error) && !is_remote_connection_error(error)
		});
		// Advance again before releasing the exclusive gate. Calls submitted both
		// before and during expiry cleanup must observe a changed epoch and fail;
		// only calls submitted after cleanup may proceed.
		self.transaction_coordinator
			.epoch
			.fetch_add(1, Ordering::AcqRel);
		self.release_transaction(
			key,
			TransactionTerminalState::Expired(timeout),
			rollback_failed,
		)
		.await;
		if let Err(error) = rollback
			&& rollback_failed
		{
			return Err(self.attach_actor(error));
		}
		Ok(())
	}

	async fn connection_lost_transaction(&self, key: &str, expected_session: u64) -> Result<()> {
		let (operation, still_expected) = {
			let mut state = self.transaction_coordinator.state.lock().await;
			let Some(active) = state.active.as_mut().filter(|active| active.key == key) else {
				return Ok(());
			};
			let still_expected = active.remote_session == Some(expected_session);
			if still_expected {
				active.expiring = true;
				active.connection_lost = true;
			}
			(Arc::clone(&active.operation), still_expected)
		};
		if !still_expected {
			return Ok(());
		}
		let _operation = operation.lock().await;

		// The remote database handle is scoped to the server-side WebSocket
		// connection. Disconnect closes it and SQLite rolls back its open
		// transaction. Sending ROLLBACK on a replacement connection would target a
		// different database session, so cleanup here is intentionally local: make
		// the handle terminal and release actor/socket work queued behind it.
		self.release_transaction(key, TransactionTerminalState::ConnectionLost, false)
			.await;
		Ok(())
	}

	async fn handle_transaction_backend_error(
		&self,
		key: &str,
		error: anyhow::Error,
	) -> anyhow::Error {
		if is_remote_connection_error(&error) {
			self.release_transaction(key, TransactionTerminalState::ConnectionLost, false)
				.await;
		}
		self.attach_actor(map_transaction_connection_error(error))
	}

	pub(super) async fn release_transaction(
		&self,
		key: &str,
		terminal: TransactionTerminalState,
		close: bool,
	) {
		let mut state = self.transaction_coordinator.state.lock().await;
		if !state
			.active
			.as_ref()
			.is_some_and(|active| active.key == key)
		{
			return;
		}
		let mut active = state.active.take().expect("active transaction was checked");
		if let Some(task) = active.timeout_task.take() {
			task.cancel();
		}
		if let TransactionTerminalState::Expired(timeout) = terminal {
			state.last_expired_timeout = Some(timeout);
		}
		insert_terminal_state(&mut state, key.to_owned(), terminal);
		state.closed |= close;
		if close {
			self.transaction_coordinator.admission.close();
			if let Ok(config) = self.runtime_config() {
				report_sqlite_worker_fatal(
					&self.worker_fatal_reported,
					config,
					"sqlite transaction rollback failed; coordinator closed".to_owned(),
				);
			}
		}
		drop(active.gate_guard.take());
		#[cfg(feature = "sqlite-local")]
		let profile = active.profile;
		drop(state);

		#[cfg(feature = "sqlite-local")]
		if self.backend() == super::SqliteBackend::LocalNative
			&& let Some(profile) = profile
		{
			let (fingerprint, fingerprint_source) = profile.fingerprint();
			if profile.name.is_none() {
				let warning = UNNAMED_TRANSACTION_WARNINGS.fetch_add(1, Ordering::Relaxed);
				if warning == 0 || warning.is_power_of_two() {
					tracing::warn!(
						fingerprint,
						"SQLite transaction is unnamed. Add a static `name` to group transaction, storage, and commit performance across executions. Individual statement profiling remains available."
					);
				}
			}
			if let Some(metrics) = &self.vfs_metrics {
				let total_ns = profile
					.started_at
					.elapsed()
					.as_nanos()
					.try_into()
					.unwrap_or(u64::MAX);
				let application_time_ns = total_ns
					.saturating_sub(profile.transaction_wait_ns)
					.saturating_sub(profile.worker_wait_ns)
					.saturating_sub(profile.storage_ns)
					.saturating_sub(profile.local_work_ns);
				let metric = depot_client::vfs::SqliteTransactionMetric {
					fingerprint,
					fingerprint_source,
					shape_fingerprint: profile.shape_fingerprint(),
					statement_fingerprint_hashes: profile.statement_fingerprint_hashes,
					omitted_statement_fingerprints: profile.omitted_statement_fingerprints,
					storage_transport: "proxy",
					outcome: match terminal {
						TransactionTerminalState::Committed => "success",
						TransactionTerminalState::RolledBack => "rollback",
						TransactionTerminalState::Expired(_) => "expired",
						TransactionTerminalState::ConnectionLost => "connection_lost",
					},
					total_ns,
					transaction_wait_ns: profile.transaction_wait_ns,
					worker_wait_ns: profile.worker_wait_ns,
					storage_ns: profile.storage_ns,
					local_work_ns: profile.local_work_ns,
					application_time_ns,
					commit_ns: profile.commit_ns,
					get_pages_round_trips: profile.get_pages_round_trips,
					statement_count: profile.statement_count,
					dirty_pages: profile.dirty_pages,
					dirty_bytes: profile.dirty_bytes,
				};
				if metrics.observe_transaction_profile(&metric)
					&& self.profiling.mark_cataloged(&metric.fingerprint)
				{
					metrics.record_fingerprint_catalog(
						"transaction",
						&metric.fingerprint,
						profile.name.as_deref().unwrap_or(&metric.fingerprint),
						FINGERPRINT_FORMAT_VERSION,
					);
				}
				metrics.emit_transaction_diagnostic_event(
					self.actor_id.as_deref().unwrap_or("unknown"),
					self.generation,
					&metric,
				);
			}
		}
	}

	pub(super) async fn shutdown_transaction_coordinator(&self) -> OwnedRwLockWriteGuard<()> {
		let active_operation = {
			let mut state = self.transaction_coordinator.state.lock().await;
			state.closed = true;
			state.active.as_ref().map(|active| {
				(
					active.key.clone(),
					Arc::clone(&active.operation),
					active.remote_session,
				)
			})
		};
		self.transaction_coordinator.admission.close();
		if let Some((key, operation, remote_session)) = active_operation {
			let _operation = operation.lock().await;
			let still_active = {
				let state = self.transaction_coordinator.state.lock().await;
				state
					.active
					.as_ref()
					.is_some_and(|active| active.key == key)
			};
			if still_active {
				let rollback = self
					.execute_backend_in_session("ROLLBACK".to_owned(), None, remote_session)
					.await;
				let rollback_failed = rollback.as_ref().is_err_and(|error| {
					!is_no_active_transaction_error(error) && !is_remote_connection_error(error)
				});
				if let Err(error) = &rollback
					&& rollback_failed
				{
					tracing::error!(%error, "sqlite rollback during coordinator shutdown failed");
				}
				self.transaction_coordinator
					.epoch
					.fetch_add(1, Ordering::AcqRel);
				self.release_transaction(
					&key,
					TransactionTerminalState::RolledBack,
					rollback_failed,
				)
				.await;
			}
		}
		Arc::clone(&self.transaction_coordinator.gate)
			.write_owned()
			.await
	}
}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_queue_full",
	"SQLite transaction queue is full.",
	"SQLite transaction coordinator queue is full. Limit is 128 operations."
)]
pub struct TransactionQueueFullError;

impl fmt::Display for TransactionQueueFullError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("sqlite transaction coordinator queue is full")
	}
}

impl Error for TransactionQueueFullError {}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_closed",
	"SQLite transaction coordinator is closed."
)]
pub struct TransactionCoordinatorClosedError;

impl fmt::Display for TransactionCoordinatorClosedError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("sqlite transaction coordinator is closed")
	}
}

impl Error for TransactionCoordinatorClosedError {}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_invalid_argument",
	"Invalid SQLite transaction argument.",
	"{message}"
)]
pub struct TransactionInvalidArgumentError {
	pub message: &'static str,
}

impl fmt::Display for TransactionInvalidArgumentError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.message)
	}
}

impl Error for TransactionInvalidArgumentError {}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_connection_lost",
	"SQLite transaction connection was lost.",
	"The Envoy connection that owned this SQLite transaction disconnected. The transaction was rolled back and cannot be resumed; start a new db.transaction()."
)]
pub struct TransactionConnectionLostError;

impl fmt::Display for TransactionConnectionLostError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(
			"the Envoy connection that owned this SQLite transaction disconnected; the transaction was rolled back and cannot be resumed",
		)
	}
}

impl Error for TransactionConnectionLostError {}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_unknown",
	"Unknown SQLite transaction handle.",
	"Unknown SQLite transaction handle `{key}`."
)]
pub struct TransactionUnknownError {
	pub key: String,
}

impl fmt::Display for TransactionUnknownError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "unknown sqlite transaction handle `{}`", self.key)
	}
}

impl Error for TransactionUnknownError {}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_terminal",
	"SQLite transaction handle is terminal.",
	"SQLite transaction handle `{key}` is already {state}."
)]
pub struct TransactionTerminalError {
	pub key: String,
	pub state: &'static str,
}

impl fmt::Display for TransactionTerminalError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"sqlite transaction handle `{}` is already {}",
			self.key, self.state
		)
	}
}

impl Error for TransactionTerminalError {}

#[derive(rivet_error::RivetError, Debug, Serialize)]
#[error(
	"sqlite",
	"transaction_expired",
	"SQLite transaction expired.",
	"SQLite transaction expired after {timeout_ms} ms and was rolled back; this timeout is a deadlock-safety backstop. Increase the db.transaction() `timeout` option if the transaction legitimately needs longer, and check for a nested transaction or use of the outer `db` instead of `tx` inside the callback."
)]
pub struct TransactionExpiredError {
	pub timeout_ms: u64,
}

impl fmt::Display for TransactionExpiredError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"sqlite transaction expired after {} ms and was rolled back; this timeout is a deadlock-safety backstop. Increase the db.transaction() `timeout` option if the transaction legitimately needs longer, and check for a nested transaction or use of the outer `db` instead of `tx` inside the callback",
			self.timeout_ms
		)
	}
}

impl Error for TransactionExpiredError {}

impl TransactionExpiredError {
	fn owner(_key: &str, timeout: Duration) -> Self {
		Self {
			timeout_ms: duration_millis(timeout),
		}
	}

	fn parked(timeout: Duration) -> Self {
		Self {
			timeout_ms: duration_millis(timeout),
		}
	}
}

fn duration_millis(duration: Duration) -> u64 {
	duration.as_millis().try_into().unwrap_or(u64::MAX)
}

async fn sleep_transaction_timeout(mut timeout: Duration) {
	// Browser timers commonly cap one wait at signed 32-bit milliseconds.
	// Chunking preserves the caller's unbounded product-level timeout on wasm.
	const MAX_TIMER_WAIT: Duration = Duration::from_millis(2_000_000_000);
	while timeout > MAX_TIMER_WAIT {
		crate::time::sleep(MAX_TIMER_WAIT).await;
		timeout = timeout.saturating_sub(MAX_TIMER_WAIT);
	}
	crate::time::sleep(timeout).await;
}

fn spawn_transaction_timeout(
	db: SqliteDb,
	key: String,
	timeout: Duration,
	cancel: CancellationToken,
	remote_session: Option<u64>,
) {
	let task = async move {
		let connection_lost = async {
			let Some(expected) = remote_session else {
				std::future::pending::<()>().await;
				return;
			};
			let Ok(handle) = db.handle() else {
				return;
			};
			let mut sessions = handle.subscribe_connection_session();
			loop {
				if *sessions.borrow_and_update() != expected {
					return;
				}
				if sessions.changed().await.is_err() {
					return;
				}
			}
		};
		tokio::pin!(connection_lost);

		let expired = tokio::select! {
			_ = cancel.cancelled() => return,
			_ = sleep_transaction_timeout(timeout) => true,
			_ = &mut connection_lost => false,
		};
		let result = if expired {
			db.expire_transaction(&key).await
		} else {
			// This covers all disconnect timing windows. During BEGIN there is no
			// active coordinator entry yet and the BEGIN caller receives its own
			// indeterminate/session error. Between statements this branch is the only
			// signal, and it prevents the next statement from autocommitting after a
			// fast reconnect. During a statement or COMMIT, request tracking preserves
			// the operation's indeterminate result while this terminalizes the handle.
			db.connection_lost_transaction(&key, remote_session.expect("checked above"))
				.await
		};
		if let Err(error) = result {
			if error.downcast_ref::<TransactionTerminalError>().is_none() {
				tracing::error!(%error, "sqlite transaction terminal cleanup failed");
			}
		}
	};

	#[cfg(target_arch = "wasm32")]
	wasm_bindgen_futures::spawn_local(task);

	#[cfg(not(target_arch = "wasm32"))]
	RuntimeSpawner::spawn(task);
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) async fn run_detached_transaction_task<F, T>(
	future: F,
	context: &'static str,
) -> Result<T>
where
	F: Future<Output = Result<T>> + Send + 'static,
	T: Send + 'static,
{
	RuntimeSpawner::spawn(future).await.context(context)?
}

#[cfg(target_arch = "wasm32")]
pub(super) async fn run_detached_transaction_task<F, T>(
	future: F,
	context: &'static str,
) -> Result<T>
where
	F: Future<Output = Result<T>> + 'static,
	T: 'static,
{
	let (response_tx, response_rx) = oneshot::channel();
	wasm_bindgen_futures::spawn_local(async move {
		let _ = response_tx.send(future.await);
	});
	response_rx.await.context(context)?
}

pub(super) fn insert_terminal_state(
	state: &mut TransactionCoordinatorState,
	key: String,
	terminal: TransactionTerminalState,
) {
	if let TransactionTerminalState::Expired(timeout) = terminal {
		state.poisoned.insert(key, timeout);
		return;
	}
	if state.terminal.insert(key.clone(), terminal).is_none() {
		state.terminal_order.push_back(key);
	}
	while state.terminal_order.len() > TRANSACTION_TERMINAL_CAPACITY {
		if let Some(expired_key) = state.terminal_order.pop_front() {
			state.terminal.remove(&expired_key);
		}
	}
}

fn transaction_known_state_error(
	state: &TransactionCoordinatorState,
	key: &str,
) -> Option<anyhow::Error> {
	if let Some(timeout) = state.poisoned.get(key).copied() {
		return Some(transaction_expired_owner_error(key, timeout));
	}
	state
		.terminal
		.get(key)
		.copied()
		.map(|terminal| transaction_terminal_error(key, terminal))
}

fn is_no_active_transaction_error(error: &anyhow::Error) -> bool {
	// SQLite does not expose a dedicated result code for "cannot commit/rollback
	// - no transaction is active"; both native SQLite and the remote executor
	// surface SQLITE_ERROR plus this stable SQLite-generated text. Keep the
	// classifier isolated here. In particular, ON CONFLICT ROLLBACK can end a
	// transaction before our cleanup call, and treating that cleanup response as
	// fatal would permanently close the coordinator. The cross-runtime actor-db
	// suite exercises that real automatic-rollback path on native and Wasm.
	error.chain().any(|cause| {
		cause
			.to_string()
			.to_ascii_lowercase()
			.contains("no transaction is active")
	})
}

fn transaction_queue_full_error() -> anyhow::Error {
	TransactionQueueFullError
		.build()
		.context(TransactionQueueFullError)
}

fn transaction_coordinator_closed_error() -> anyhow::Error {
	TransactionCoordinatorClosedError
		.build()
		.context(TransactionCoordinatorClosedError)
}

fn transaction_unknown_error(key: &str) -> anyhow::Error {
	TransactionUnknownError {
		key: key.to_owned(),
	}
	.build()
	.context(TransactionUnknownError {
		key: key.to_owned(),
	})
}

fn transaction_expired_error(timeout: Duration) -> anyhow::Error {
	TransactionExpiredError::parked(timeout)
		.build()
		.context(TransactionExpiredError::parked(timeout))
}

fn transaction_expired_owner_error(key: &str, timeout: Duration) -> anyhow::Error {
	TransactionExpiredError::owner(key, timeout)
		.build()
		.context(TransactionExpiredError::owner(key, timeout))
}

fn transaction_terminal_error(key: &str, terminal: TransactionTerminalState) -> anyhow::Error {
	match terminal {
		TransactionTerminalState::Expired(timeout) => transaction_expired_owner_error(key, timeout),
		TransactionTerminalState::Committed => transaction_terminal_state_error(key, "committed"),
		TransactionTerminalState::RolledBack => {
			transaction_terminal_state_error(key, "rolled back")
		}
		TransactionTerminalState::ConnectionLost => transaction_connection_lost_error(),
	}
}

fn transaction_invalid_argument_error(message: &'static str) -> anyhow::Error {
	TransactionInvalidArgumentError { message }
		.build()
		.context(TransactionInvalidArgumentError { message })
}

fn transaction_connection_lost_error() -> anyhow::Error {
	TransactionConnectionLostError
		.build()
		.context(TransactionConnectionLostError)
}

fn is_remote_connection_error(error: &anyhow::Error) -> bool {
	error
		.downcast_ref::<super::RemoteSqliteConnectionSessionLostError>()
		.is_some()
		|| error
			.downcast_ref::<super::RemoteSqliteIndeterminateResultError>()
			.is_some()
}

fn map_transaction_connection_error(error: anyhow::Error) -> anyhow::Error {
	if error
		.downcast_ref::<super::RemoteSqliteConnectionSessionLostError>()
		.is_some()
	{
		return transaction_connection_lost_error();
	}
	super::remote_request_error(error)
}

fn transaction_terminal_state_error(key: &str, state: &'static str) -> anyhow::Error {
	TransactionTerminalError {
		key: key.to_owned(),
		state,
	}
	.build()
	.context(TransactionTerminalError {
		key: key.to_owned(),
		state,
	})
}
