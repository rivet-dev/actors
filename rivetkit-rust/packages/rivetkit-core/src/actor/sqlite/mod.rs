use std::collections::HashSet;
use std::future::Future;
use std::io::Cursor;
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result};
use depot_client_types::is_head_fence_mismatch;
pub use depot_client_types::{BindParam, ColumnValue, ExecResult, ExecuteResult, QueryResult};
#[cfg(feature = "sqlite-local")]
use parking_lot::Mutex;
use rivet_envoy_client::protocol;
use rivet_envoy_client::{
	handle::EnvoyHandle,
	utils::{RemoteSqliteConnectionSessionLostError, RemoteSqliteIndeterminateResultError},
};
use rivet_error::{ActorSpecifier, RivetError};
use serde::Serialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
#[cfg(feature = "sqlite-local")]
use tokio::sync::Mutex as AsyncMutex;
#[cfg(feature = "sqlite-local")]
use tokio::task::JoinHandle;
use tracing::Instrument as _;

use crate::telemetry::SqliteOperation;

#[cfg(feature = "sqlite-local")]
mod envoy_sqlite_transport;
#[cfg(feature = "sqlite-local")]
mod profiling;
mod tx;

pub use tx::{
	DEFAULT_TRANSACTION_TIMEOUT, SqliteTransaction, TRANSACTION_COORDINATOR_QUEUE_CAPACITY,
	TransactionConnectionLostError, TransactionCoordinatorClosedError, TransactionExpiredError,
	TransactionInvalidArgumentError, TransactionQueueFullError, TransactionTerminalError,
	TransactionUnknownError,
};
#[cfg(test)]
use tx::{
	TRANSACTION_TERMINAL_CAPACITY, TransactionCoordinatorState, TransactionTerminalState,
	insert_terminal_state,
};
use tx::{TransactionCoordinator, run_detached_transaction_task};

#[cfg(feature = "sqlite-local")]
use crate::error::ActorLifecycle;
use crate::error::SqliteRuntimeError;
#[cfg(all(not(target_arch = "wasm32"), feature = "sqlite-local"))]
use crate::runtime::RuntimeSpawner;

#[cfg(feature = "sqlite-local")]
use depot_client::{
	database::{NativeDatabaseHandle, open_database_from_transport},
	vfs::{SqliteVfsMetrics, SqliteVfsMetricsSnapshot},
	worker::{
		SQLITE_WORKER_QUEUE_CAPACITY, SqliteWorkerCloseTimeoutError, SqliteWorkerClosingError,
		SqliteWorkerDeadError, SqliteWorkerFatalError, SqliteWorkerOverloadedError,
		SqliteWorkerResult,
	},
};
#[cfg(feature = "sqlite-local")]
use envoy_sqlite_transport::EnvoySqliteTransport;

#[cfg(not(feature = "sqlite-local"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SqliteVfsMetricsSnapshot {
	pub request_build_ns: u64,
	pub serialize_ns: u64,
	pub transport_ns: u64,
	pub state_update_ns: u64,
	pub total_ns: u64,
	pub commit_count: u64,
}

#[derive(Clone)]
pub struct SqliteRuntimeConfig {
	pub handle: EnvoyHandle,
	pub actor_id: String,
	pub generation: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct SqliteBatchStatement {
	pub sql: String,
	pub params: Option<Vec<BindParam>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteBackend {
	LocalNative,
	RemoteEnvoy,
}

#[cfg(feature = "sqlite-local")]
struct ProfiledBackendResult<T> {
	result: Result<T>,
	profile: Option<depot_client::vfs::SqliteOperationProfile>,
}

impl SqliteDb {
	#[cfg(feature = "sqlite-local")]
	async fn exec_backend_profiled(&self, sql: String) -> ProfiledBackendResult<QueryResult> {
		match self.backend {
			SqliteBackend::LocalNative => match self.local_exec_profiled(sql).await {
				Ok(result) => ProfiledBackendResult {
					result: result.result,
					profile: Some(result.profile),
				},
				Err(error) => ProfiledBackendResult {
					result: Err(error),
					profile: None,
				},
			},
			SqliteBackend::RemoteEnvoy => ProfiledBackendResult {
				result: self.remote_exec(sql).await,
				profile: None,
			},
		}
	}

	#[cfg(feature = "sqlite-local")]
	async fn execute_backend_profiled(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> ProfiledBackendResult<ExecuteResult> {
		match self.backend {
			SqliteBackend::LocalNative => match self.local_execute_profiled(sql, params).await {
				Ok(result) => ProfiledBackendResult {
					result: result.result,
					profile: Some(result.profile),
				},
				Err(error) => ProfiledBackendResult {
					result: Err(error),
					profile: None,
				},
			},
			SqliteBackend::RemoteEnvoy => ProfiledBackendResult {
				result: self.remote_execute(sql, params).await,
				profile: None,
			},
		}
	}

	async fn exec_backend(&self, sql: String) -> Result<QueryResult> {
		match self.backend {
			SqliteBackend::LocalNative => self.local_exec(sql).await,
			SqliteBackend::RemoteEnvoy => self.remote_exec(sql).await,
		}
	}

	async fn exec_backend_in_session(
		&self,
		sql: String,
		expected_session: Option<u64>,
	) -> Result<(QueryResult, Option<u64>)> {
		match self.backend {
			SqliteBackend::LocalNative => self.local_exec(sql).await.map(|result| (result, None)),
			SqliteBackend::RemoteEnvoy => self
				.remote_exec_with_session(sql, expected_session)
				.await
				.map(|(result, session)| (result, Some(session))),
		}
	}

	async fn query_backend(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<QueryResult> {
		match self.backend {
			SqliteBackend::LocalNative => self
				.local_execute(sql, params)
				.await
				.map(ExecuteResult::into_query_result),
			SqliteBackend::RemoteEnvoy => self
				.remote_execute(sql, params)
				.await
				.map(ExecuteResult::into_query_result),
		}
	}

	async fn run_backend(&self, sql: String, params: Option<Vec<BindParam>>) -> Result<ExecResult> {
		match self.backend {
			SqliteBackend::LocalNative => self
				.local_execute(sql, params)
				.await
				.map(ExecuteResult::into_exec_result),
			SqliteBackend::RemoteEnvoy => self
				.remote_execute(sql, params)
				.await
				.map(ExecuteResult::into_exec_result),
		}
	}

	async fn execute_backend(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		match self.backend {
			SqliteBackend::LocalNative => self.local_execute(sql, params).await,
			SqliteBackend::RemoteEnvoy => self.remote_execute(sql, params).await,
		}
	}

	async fn execute_backend_in_session(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
		expected_session: Option<u64>,
	) -> Result<(ExecuteResult, Option<u64>)> {
		match self.backend {
			SqliteBackend::LocalNative => self
				.local_execute(sql, params)
				.await
				.map(|result| (result, None)),
			SqliteBackend::RemoteEnvoy => self
				.remote_execute_with_session(sql, params, expected_session)
				.await
				.map(|(result, session)| (result, Some(session))),
		}
	}
}

#[derive(Clone)]
pub struct SqliteDb {
	handle: Option<EnvoyHandle>,
	actor_id: Option<String>,
	actor_key: Option<String>,
	generation: Option<u64>,
	backend: SqliteBackend,
	/// Mirrors the user's actor-config `db({...})` declaration. The envoy
	/// always sets up sqlite storage under the hood, so handle/actor_id are
	/// not a reliable signal for whether the user opted in; this flag is.
	enabled: bool,
	invocation_telemetry: Option<crate::ActorInvocationTelemetry>,
	#[cfg(feature = "sqlite-local")]
	// Forced-sync: native SQLite handles are used inside spawn_blocking and
	// synchronous diagnostic accessors.
	db: Arc<Mutex<Option<NativeDatabaseHandle>>>,
	#[cfg(feature = "sqlite-local")]
	open_lock: Arc<AsyncMutex<()>>,
	#[cfg(feature = "sqlite-local")]
	worker_failure_task: Arc<Mutex<Option<JoinHandle<()>>>>,
	worker_fatal_reported: Arc<AtomicBool>,
	transaction_coordinator: Arc<TransactionCoordinator>,
	#[cfg(feature = "sqlite-local")]
	vfs_metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	#[cfg(feature = "sqlite-local")]
	profiling: Arc<profiling::SqliteProfilingState>,
}

impl Default for SqliteDb {
	fn default() -> Self {
		Self {
			handle: None,
			actor_id: None,
			actor_key: None,
			generation: None,
			backend: if cfg!(feature = "sqlite-local") {
				SqliteBackend::LocalNative
			} else {
				SqliteBackend::RemoteEnvoy
			},
			enabled: false,
			invocation_telemetry: None,
			#[cfg(feature = "sqlite-local")]
			db: Default::default(),
			#[cfg(feature = "sqlite-local")]
			open_lock: Default::default(),
			#[cfg(feature = "sqlite-local")]
			worker_failure_task: Default::default(),
			worker_fatal_reported: Default::default(),
			transaction_coordinator: Default::default(),
			#[cfg(feature = "sqlite-local")]
			vfs_metrics: None,
			#[cfg(feature = "sqlite-local")]
			profiling: Default::default(),
		}
	}
}

impl SqliteDb {
	pub fn new(handle: EnvoyHandle, actor_id: impl Into<String>, enabled: bool) -> Result<Self> {
		Self::new_with_remote_sqlite(handle, actor_id, None, None, enabled, false)
	}

	pub fn new_with_remote_sqlite(
		handle: EnvoyHandle,
		actor_id: impl Into<String>,
		actor_key: Option<String>,
		generation: Option<u64>,
		enabled: bool,
		remote_sqlite: bool,
	) -> Result<Self> {
		Ok(Self {
			handle: Some(handle),
			actor_id: Some(actor_id.into()),
			actor_key,
			generation,
			backend: select_sqlite_backend(remote_sqlite)?,
			enabled,
			invocation_telemetry: None,
			#[cfg(feature = "sqlite-local")]
			db: Default::default(),
			#[cfg(feature = "sqlite-local")]
			open_lock: Default::default(),
			#[cfg(feature = "sqlite-local")]
			worker_failure_task: Default::default(),
			worker_fatal_reported: Default::default(),
			transaction_coordinator: Default::default(),
			#[cfg(feature = "sqlite-local")]
			vfs_metrics: None,
			#[cfg(feature = "sqlite-local")]
			profiling: Default::default(),
		})
	}

	#[cfg(feature = "sqlite-local")]
	pub(crate) fn set_vfs_metrics(&mut self, metrics: Arc<dyn SqliteVfsMetrics>) {
		self.vfs_metrics = Some(metrics);
	}

	#[cfg(feature = "sqlite-local")]
	pub(crate) fn set_profiling_config(&mut self, config: crate::SqliteProfilingConfig) {
		self.profiling = Arc::new(profiling::SqliteProfilingState::new(config));
	}

	#[cfg(all(test, feature = "sqlite-local"))]
	pub(crate) fn from_native_database_for_test(
		actor_id: impl Into<String>,
		generation: u64,
		native_db: NativeDatabaseHandle,
		metrics: Arc<dyn SqliteVfsMetrics>,
		profiling: crate::SqliteProfilingConfig,
	) -> Self {
		Self {
			actor_id: Some(actor_id.into()),
			generation: Some(generation),
			enabled: true,
			db: Arc::new(Mutex::new(Some(native_db))),
			vfs_metrics: Some(metrics),
			profiling: Arc::new(profiling::SqliteProfilingState::new(profiling)),
			..Self::default()
		}
	}

	pub fn is_enabled(&self) -> bool {
		self.enabled
	}

	pub fn backend(&self) -> SqliteBackend {
		self.backend
	}

	#[doc(hidden)]
	pub fn with_invocation_telemetry(
		mut self,
		telemetry: Option<crate::ActorInvocationTelemetry>,
	) -> Self {
		self.invocation_telemetry = telemetry;
		self
	}

	/// Runs one SQLite operation inside a `rivet.sqlite.*` span when the
	/// current invocation is traced.
	pub(super) async fn traced<T>(
		&self,
		operation: SqliteOperation,
		future: impl Future<Output = Result<T>>,
	) -> Result<T> {
		let Some(mut span) = self
			.invocation_telemetry
			.as_ref()
			.and_then(|telemetry| telemetry.start_sqlite(operation))
		else {
			return future.await;
		};
		let result = future.instrument(span.span()).await;
		span.finish(result.as_ref().err());
		result
	}

	pub async fn get_pages(
		&self,
		request: protocol::SqliteGetPagesRequest,
	) -> Result<protocol::SqliteGetPagesResponse> {
		self.handle()?.sqlite_get_pages(request).await
	}

	pub async fn commit(
		&self,
		request: protocol::SqliteCommitRequest,
	) -> Result<protocol::SqliteCommitResponse> {
		self.handle()?.sqlite_commit(request).await
	}

	pub async fn open(&self) -> Result<()> {
		match self.backend {
			SqliteBackend::LocalNative => {
				#[cfg(feature = "sqlite-local")]
				{
					let _open_guard = self.open_lock.lock().await;
					if self.db.lock().is_some() {
						return Ok(());
					}

					let config = self.runtime_config()?;
					let vfs_metrics = self.vfs_metrics.clone();
					let rt_handle = tokio::runtime::Handle::try_current()
						.context("open sqlite database requires a tokio runtime")?;
					self.worker_fatal_reported.store(false, Ordering::Release);

					let native_db = self.map_local_worker_result(
						open_database_from_transport(
							Arc::new(EnvoySqliteTransport::new(config.handle.clone())),
							config.actor_id.clone(),
							config
								.generation
								.ok_or_else(|| sqlite_not_configured("generation"))?,
							rt_handle,
							vfs_metrics,
						)
						.await,
					)?;
					self.start_worker_failure_monitor(native_db.clone(), config);
					*self.db.lock() = Some(native_db);
					if let Some(metrics) = self.vfs_metrics.as_ref() {
						metrics.set_worker_active(true);
					}
					Ok(())
				}

				#[cfg(not(feature = "sqlite-local"))]
				{
					Err(SqliteRuntimeError::Unavailable.build())
				}
			}
			SqliteBackend::RemoteEnvoy => {
				self.remote_config()?;
				Ok(())
			}
		}
	}

	#[cfg(feature = "sqlite-local")]
	async fn local_exec(&self, sql: String) -> Result<QueryResult> {
		self.open().await?;
		self.map_local_worker_result(self.native_db_handle()?.exec(sql).await)
	}

	#[cfg(feature = "sqlite-local")]
	async fn local_exec_profiled(&self, sql: String) -> Result<SqliteWorkerResult<QueryResult>> {
		self.open().await?;
		self.map_local_worker_result(self.native_db_handle()?.exec_profiled(sql).await)
	}

	#[cfg(not(feature = "sqlite-local"))]
	async fn local_exec(&self, _sql: String) -> Result<QueryResult> {
		Err(SqliteRuntimeError::Unavailable.build())
	}

	#[cfg(feature = "sqlite-local")]
	async fn local_execute(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		self.open().await?;
		self.map_local_worker_result(self.native_db_handle()?.execute(sql, params).await)
	}

	#[cfg(feature = "sqlite-local")]
	async fn local_execute_profiled(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<SqliteWorkerResult<ExecuteResult>> {
		self.open().await?;
		self.map_local_worker_result(self.native_db_handle()?.execute_profiled(sql, params).await)
	}

	#[cfg(not(feature = "sqlite-local"))]
	async fn local_execute(
		&self,
		_sql: String,
		_params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		Err(SqliteRuntimeError::Unavailable.build())
	}

	#[cfg(feature = "sqlite-local")]
	fn observe_statement_profile(
		&self,
		sql: &str,
		started_at: crate::time::Instant,
		transaction_wait: std::time::Duration,
		profile: Option<depot_client::vfs::SqliteOperationProfile>,
		outcome: &'static str,
		transaction_mode: &'static str,
	) -> Option<profiling::StatementObservation> {
		if self.backend != SqliteBackend::LocalNative {
			return None;
		}
		let fingerprint = self.profiling.statement_fingerprint(sql)?;
		let observation = profiling::StatementObservation {
			fingerprint,
			total_ns: duration_ns(started_at.elapsed()),
			transaction_wait_ns: duration_ns(transaction_wait),
			profile: profile.unwrap_or_default(),
		};
		if let Some(metrics) = &self.vfs_metrics {
			let metric = depot_client::vfs::SqliteOperationMetric {
				operation_type: "statement",
				fingerprint: observation.fingerprint.display.clone(),
				fingerprint_source: "query",
				transaction_mode,
				storage_transport: "proxy",
				outcome,
				sql_bytes: sql.len().try_into().unwrap_or(u64::MAX),
				total_ns: observation.total_ns,
				transaction_wait_ns: observation.transaction_wait_ns,
				profile: observation.profile.clone(),
			};
			if metrics.observe_operation_profile(&metric)
				&& self.profiling.mark_cataloged(&metric.fingerprint)
			{
				metrics.record_fingerprint_catalog(
					"statement",
					&metric.fingerprint,
					sql,
					profiling::FINGERPRINT_FORMAT_VERSION,
				);
			}
			metrics.emit_operation_diagnostic_event(
				self.actor_id.as_deref().unwrap_or("unknown"),
				self.generation,
				&metric,
			);
		}
		Some(observation)
	}

	pub async fn exec(&self, sql: impl Into<String>) -> Result<QueryResult> {
		let sql = sql.into();
		self.traced(SqliteOperation::Exec, self.exec_untraced(sql)).await
	}

	async fn exec_untraced(&self, sql: String) -> Result<QueryResult> {
		let sql_for_log = sql.clone();
		#[cfg(feature = "sqlite-local")]
		let started_at = self
			.profiling
			.config
			.enabled
			.then(crate::time::Instant::now);
		#[cfg(feature = "sqlite-local")]
		let transaction_wait_started_at = started_at.map(|_| crate::time::Instant::now());
		let guard = self.begin_regular_operation().await;
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = transaction_wait_started_at.map(|started| started.elapsed());
		#[cfg(feature = "sqlite-local")]
		let (result, profile) = match guard {
			Ok(_guard) if started_at.is_some() => {
				let profiled = self.exec_backend_profiled(sql).await;
				(profiled.result, profiled.profile)
			}
			Ok(_guard) => (self.exec_backend(sql).await, None),
			Err(error) => (Err(error), None),
		};
		#[cfg(not(feature = "sqlite-local"))]
		let result = match guard {
			Ok(_guard) => self.exec_backend(sql).await,
			Err(error) => Err(error),
		};
		#[cfg(feature = "sqlite-local")]
		if let Some(started_at) = started_at {
			self.observe_statement_profile(
				&sql_for_log,
				started_at,
				transaction_wait.unwrap_or_default(),
				profile,
				if result.is_ok() { "success" } else { "error" },
				"autocommit",
			);
		}
		match result {
			Ok(result) => Ok(result),
			Err(error) => {
				let error = self.attach_actor(error);
				self.log_operation_error("exec", &sql_for_log, 0, &error);
				Err(error)
			}
		}
	}

	pub async fn query(
		&self,
		sql: impl Into<String>,
		params: Option<Vec<BindParam>>,
	) -> Result<QueryResult> {
		let sql = sql.into();
		self.traced(SqliteOperation::Query, self.query_untraced(sql, params)).await
	}

	async fn query_untraced(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<QueryResult> {
		let sql_for_log = sql.clone();
		let binding_count = bind_param_count(&params);
		#[cfg(feature = "sqlite-local")]
		let started_at = self
			.profiling
			.config
			.enabled
			.then(crate::time::Instant::now);
		#[cfg(feature = "sqlite-local")]
		let transaction_wait_started_at = started_at.map(|_| crate::time::Instant::now());
		let guard = self.begin_regular_operation().await;
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = transaction_wait_started_at.map(|started| started.elapsed());
		#[cfg(feature = "sqlite-local")]
		let (result, profile) = match guard {
			Ok(_guard) if started_at.is_some() => {
				let profiled = self.execute_backend_profiled(sql, params).await;
				(
					profiled.result.map(|result| QueryResult {
						columns: result.columns,
						rows: result.rows,
					}),
					profiled.profile,
				)
			}
			Ok(_guard) => (self.query_backend(sql, params).await, None),
			Err(error) => (Err(error), None),
		};
		#[cfg(not(feature = "sqlite-local"))]
		let result = match guard {
			Ok(_guard) => self.query_backend(sql, params).await,
			Err(error) => Err(error),
		};
		#[cfg(feature = "sqlite-local")]
		if let Some(started_at) = started_at {
			self.observe_statement_profile(
				&sql_for_log,
				started_at,
				transaction_wait.unwrap_or_default(),
				profile,
				if result.is_ok() { "success" } else { "error" },
				"autocommit",
			);
		}
		match result {
			Ok(result) => Ok(result),
			Err(error) => {
				let error = self.attach_actor(error);
				self.log_operation_error("query", &sql_for_log, binding_count, &error);
				Err(error)
			}
		}
	}

	pub async fn run(
		&self,
		sql: impl Into<String>,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecResult> {
		let sql = sql.into();
		self.traced(SqliteOperation::Run, self.run_untraced(sql, params)).await
	}

	async fn run_untraced(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecResult> {
		let sql_for_log = sql.clone();
		let binding_count = bind_param_count(&params);
		#[cfg(feature = "sqlite-local")]
		let started_at = self
			.profiling
			.config
			.enabled
			.then(crate::time::Instant::now);
		#[cfg(feature = "sqlite-local")]
		let transaction_wait_started_at = started_at.map(|_| crate::time::Instant::now());
		let guard = self.begin_regular_operation().await;
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = transaction_wait_started_at.map(|started| started.elapsed());
		#[cfg(feature = "sqlite-local")]
		let (result, profile) = match guard {
			Ok(_guard) if started_at.is_some() => {
				let profiled = self.execute_backend_profiled(sql, params).await;
				(
					profiled.result.map(|result| ExecResult {
						changes: result.changes,
					}),
					profiled.profile,
				)
			}
			Ok(_guard) => (self.run_backend(sql, params).await, None),
			Err(error) => (Err(error), None),
		};
		#[cfg(not(feature = "sqlite-local"))]
		let result = match guard {
			Ok(_guard) => self.run_backend(sql, params).await,
			Err(error) => Err(error),
		};
		#[cfg(feature = "sqlite-local")]
		if let Some(started_at) = started_at {
			self.observe_statement_profile(
				&sql_for_log,
				started_at,
				transaction_wait.unwrap_or_default(),
				profile,
				if result.is_ok() { "success" } else { "error" },
				"autocommit",
			);
		}
		match result {
			Ok(result) => Ok(result),
			Err(error) => {
				let error = self.attach_actor(error);
				self.log_operation_error("run", &sql_for_log, binding_count, &error);
				Err(error)
			}
		}
	}

	pub async fn execute(
		&self,
		sql: impl Into<String>,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		let sql = sql.into();
		self.traced(SqliteOperation::Execute, self.execute_untraced(sql, params))
			.await
	}

	async fn execute_untraced(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		let sql_for_log = sql.clone();
		let binding_count = bind_param_count(&params);
		#[cfg(feature = "sqlite-local")]
		let started_at = self
			.profiling
			.config
			.enabled
			.then(crate::time::Instant::now);
		#[cfg(feature = "sqlite-local")]
		let transaction_wait_started_at = started_at.map(|_| crate::time::Instant::now());
		let guard = self.begin_regular_operation().await;
		#[cfg(feature = "sqlite-local")]
		let transaction_wait = transaction_wait_started_at.map(|started| started.elapsed());
		#[cfg(feature = "sqlite-local")]
		let (result, profile) = match guard {
			Ok(_guard) if started_at.is_some() => {
				let profiled = self.execute_backend_profiled(sql, params).await;
				(profiled.result, profiled.profile)
			}
			Ok(_guard) => (self.execute_backend(sql, params).await, None),
			Err(error) => (Err(error), None),
		};
		#[cfg(not(feature = "sqlite-local"))]
		let result = match guard {
			Ok(_guard) => self.execute_backend(sql, params).await,
			Err(error) => Err(error),
		};
		#[cfg(feature = "sqlite-local")]
		if let Some(started_at) = started_at {
			self.observe_statement_profile(
				&sql_for_log,
				started_at,
				transaction_wait.unwrap_or_default(),
				profile,
				if result.is_ok() { "success" } else { "error" },
				"autocommit",
			);
		}
		match result {
			Ok(result) => Ok(result),
			Err(error) => {
				let error = self.attach_actor(error);
				self.log_operation_error("execute", &sql_for_log, binding_count, &error);
				Err(error)
			}
		}
	}

	pub async fn execute_batch(
		&self,
		statements: Vec<SqliteBatchStatement>,
	) -> Result<Vec<ExecuteResult>> {
		self.traced(SqliteOperation::ExecuteBatch, self.execute_batch_untraced(statements))
			.await
	}

	async fn execute_batch_untraced(
		&self,
		statements: Vec<SqliteBatchStatement>,
	) -> Result<Vec<ExecuteResult>> {
		let statement_count = statements.len();
		let binding_count = statements
			.iter()
			.map(|statement| bind_param_count(&statement.params))
			.sum();
		let result = if self.backend == SqliteBackend::RemoteEnvoy {
			match self.begin_regular_operation().await {
				Ok(_guard) => self.remote_execute_batch(statements).await,
				Err(error) => Err(error),
			}
		} else {
			async {
				let transaction = self
					.clone()
					.with_invocation_telemetry(None)
					.begin_transaction(None)
					.await?;
				let mut results = Vec::with_capacity(statements.len());
				for statement in statements {
					match transaction.execute(statement.sql, statement.params).await {
						Ok(result) => results.push(result),
						Err(error) => {
							return match transaction.rollback().await {
								Ok(()) => Err(error.context("execute sqlite batch statement")),
								Err(rollback_error) => {
									Err(error.context("execute sqlite batch statement").context(
										rollback_error.context("rollback sqlite batch transaction"),
									))
								}
							};
						}
					}
				}
				transaction
					.commit()
					.await
					.context("commit sqlite batch transaction")?;
				Ok(results)
			}
			.await
		};

		match result {
			Ok(results) => Ok(results),
			Err(error) => {
				let error = self.attach_actor(error);
				self.log_operation_error_with_count(
					"execute_batch",
					"<batch>",
					binding_count,
					statement_count,
					&error,
				);
				Err(error)
			}
		}
	}

	pub async fn close(&self) -> Result<()> {
		let db = self.clone();
		run_detached_transaction_task(
			async move {
				let _gate = db.shutdown_transaction_coordinator().await;
				db.close_backend().await
			},
			"sqlite close task failed",
		)
		.await
	}

	async fn close_backend(&self) -> Result<()> {
		match self.backend {
			SqliteBackend::LocalNative => {
				#[cfg(feature = "sqlite-local")]
				{
					let native_db = self.db.lock().take();
					if let Some(native_db) = native_db {
						let result = self.map_local_worker_result(native_db.close().await);
						self.abort_worker_failure_monitor();
						if let Some(metrics) = self.vfs_metrics.as_ref() {
							metrics.set_worker_active(false);
						}
						result?;
					}
				}
				Ok(())
			}
			SqliteBackend::RemoteEnvoy => Ok(()),
		}
	}

	pub(crate) async fn cleanup_for_shutdown(&self, reusable: bool) -> Result<()> {
		// A remote database has no actor-local resources to release. Keep its
		// coordinator usable across a sleep/wake cycle on the same ActorTask.
		if reusable && self.backend == SqliteBackend::RemoteEnvoy {
			return Ok(());
		}
		self.close().await
	}

	#[cfg(test)]
	pub(crate) fn fresh_remote_for_test(&self) -> Self {
		Self::new_with_remote_sqlite(
			self.handle
				.clone()
				.expect("remote sqlite test database should have an envoy handle"),
			self.actor_id
				.clone()
				.expect("remote sqlite test database should have an actor id"),
			self.actor_key.clone(),
			self.generation,
			self.enabled,
			true,
		)
		.expect("remote sqlite test database should be configured")
	}

	pub fn take_last_kv_error(&self) -> Option<String> {
		if self.backend != SqliteBackend::LocalNative {
			return None;
		}

		#[cfg(feature = "sqlite-local")]
		{
			return self
				.db
				.lock()
				.as_ref()
				.and_then(NativeDatabaseHandle::take_last_kv_error);
		}

		#[cfg(not(feature = "sqlite-local"))]
		None
	}

	#[cfg(feature = "sqlite-local")]
	fn native_db_handle(&self) -> Result<NativeDatabaseHandle> {
		self.db
			.lock()
			.as_ref()
			.cloned()
			.ok_or_else(|| SqliteRuntimeError::Closed.build())
	}

	#[cfg(feature = "sqlite-local")]
	fn map_local_worker_result<T>(&self, result: Result<T>) -> Result<T> {
		match result {
			Ok(value) => Ok(value),
			Err(error) => {
				if is_fatal_worker_error(&error) {
					self.report_worker_fatal(&error);
				}
				Err(map_local_worker_error(error))
			}
		}
	}

	#[cfg(feature = "sqlite-local")]
	fn report_worker_fatal(&self, error: &anyhow::Error) {
		let Ok(config) = self.runtime_config() else {
			return;
		};
		report_sqlite_worker_fatal(
			&self.worker_fatal_reported,
			config,
			sqlite_worker_fatal_message(error),
		);
	}

	#[cfg(feature = "sqlite-local")]
	fn start_worker_failure_monitor(
		&self,
		native_db: NativeDatabaseHandle,
		config: SqliteRuntimeConfig,
	) {
		self.abort_worker_failure_monitor();
		let reported = Arc::clone(&self.worker_fatal_reported);
		let task = RuntimeSpawner::spawn(async move {
			if native_db.wait_for_worker_failure().await {
				report_sqlite_worker_fatal(
					&reported,
					config,
					"sqlite worker thread stopped unexpectedly".to_string(),
				);
			}
		});
		*self.worker_failure_task.lock() = Some(task);
	}

	#[cfg(feature = "sqlite-local")]
	fn abort_worker_failure_monitor(&self) {
		if let Some(task) = self.worker_failure_task.lock().take() {
			task.abort();
		}
	}

	pub fn metrics(&self) -> Option<SqliteVfsMetricsSnapshot> {
		#[cfg(feature = "sqlite-local")]
		{
			self.db
				.lock()
				.as_ref()
				.map(NativeDatabaseHandle::sqlite_vfs_metrics)
		}

		#[cfg(not(feature = "sqlite-local"))]
		{
			None
		}
	}

	pub fn runtime_config(&self) -> Result<SqliteRuntimeConfig> {
		Ok(SqliteRuntimeConfig {
			handle: self.handle()?,
			actor_id: self
				.actor_id
				.clone()
				.ok_or_else(|| sqlite_not_configured("actor id"))?,
			generation: self.generation,
		})
	}

	fn log_operation_error(
		&self,
		operation: &'static str,
		sql: &str,
		binding_count: usize,
		error: &anyhow::Error,
	) {
		self.log_operation_error_with_count(operation, sql, binding_count, 1, error);
	}

	fn log_operation_error_with_count(
		&self,
		operation: &'static str,
		sql: &str,
		binding_count: usize,
		statement_count: usize,
		error: &anyhow::Error,
	) {
		let structured = RivetError::extract(error);
		let error_chain = error.chain().map(ToString::to_string).collect::<Vec<_>>();
		tracing::error!(
			actor_id = self.actor_id.as_deref().unwrap_or("<unknown>"),
			generation = ?self.generation,
			backend = ?self.backend,
			operation,
			sql,
			binding_count,
			statement_count,
			group = structured.group(),
			code = structured.code(),
			error_message = %structured.message(),
			metadata = ?structured.metadata(),
			error_chain = ?error_chain,
			"sqlite operation failed"
		);
	}

	fn actor_specifier(&self) -> Option<ActorSpecifier> {
		let mut specifier = ActorSpecifier::new(self.actor_id.as_ref()?.clone(), self.generation?);
		if let Some(key) = self.actor_key.as_ref() {
			specifier = specifier.with_key(key.clone());
		}
		Some(specifier)
	}

	fn attach_actor(&self, error: anyhow::Error) -> anyhow::Error {
		match self.actor_specifier() {
			Some(actor) => error.context(actor),
			None => error,
		}
	}

	fn remote_config(&self) -> Result<RemoteSqliteConfig> {
		let config = self.runtime_config()?;
		let generation = config
			.generation
			.ok_or_else(|| sqlite_not_configured("generation"))?;
		Ok(RemoteSqliteConfig {
			namespace_id: config.handle.namespace().to_owned(),
			handle: config.handle,
			actor_id: config.actor_id,
			generation,
		})
	}

	async fn remote_exec(&self, sql: String) -> Result<QueryResult> {
		let config = self.remote_config()?;
		let response = config
			.handle
			.remote_sqlite_exec(protocol::SqliteExecRequest {
				namespace_id: config.namespace_id,
				actor_id: config.actor_id,
				generation: config.generation,
				sql,
			})
			.await
			.map_err(remote_request_error)?;

		match response {
			protocol::SqliteExecResponse::SqliteExecOk(ok) => {
				Ok(query_result_from_protocol(ok.result))
			}
			protocol::SqliteExecResponse::SqliteErrorResponse(error) => {
				Err(self.remote_sqlite_error_response(error))
			}
		}
	}

	async fn remote_exec_with_session(
		&self,
		sql: String,
		expected_session: Option<u64>,
	) -> Result<(QueryResult, u64)> {
		let config = self.remote_config()?;
		let (response, session) = config
			.handle
			.remote_sqlite_exec_with_session(
				protocol::SqliteExecRequest {
					namespace_id: config.namespace_id,
					actor_id: config.actor_id,
					generation: config.generation,
					sql,
				},
				expected_session,
			)
			.await?;

		match response {
			protocol::SqliteExecResponse::SqliteExecOk(ok) => {
				Ok((query_result_from_protocol(ok.result), session))
			}
			protocol::SqliteExecResponse::SqliteErrorResponse(error) => {
				Err(self.remote_sqlite_error_response(error))
			}
		}
	}

	async fn remote_execute(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		let config = self.remote_config()?;
		let response = config
			.handle
			.remote_sqlite_execute(protocol::SqliteExecuteRequest {
				namespace_id: config.namespace_id,
				actor_id: config.actor_id,
				generation: config.generation,
				sql,
				params: params.map(protocol_bind_params),
			})
			.await
			.map_err(remote_request_error)?;

		match response {
			protocol::SqliteExecuteResponse::SqliteExecuteOk(ok) => {
				Ok(execute_result_from_protocol(ok.result))
			}
			protocol::SqliteExecuteResponse::SqliteErrorResponse(error) => {
				Err(self.remote_sqlite_error_response(error))
			}
		}
	}

	async fn remote_execute_batch(
		&self,
		statements: Vec<SqliteBatchStatement>,
	) -> Result<Vec<ExecuteResult>> {
		let config = self.remote_config()?;
		let response = config
			.handle
			.remote_sqlite_execute_batch(protocol::SqliteExecuteBatchRequest {
				namespace_id: config.namespace_id,
				actor_id: config.actor_id,
				generation: config.generation,
				statements: statements
					.into_iter()
					.map(|statement| protocol::SqliteBatchStatement {
						sql: statement.sql,
						params: statement.params.map(protocol_bind_params),
					})
					.collect(),
			})
			.await
			.map_err(remote_request_error)?;

		match response {
			protocol::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(ok) => Ok(ok
				.results
				.into_iter()
				.map(execute_result_from_protocol)
				.collect()),
			protocol::SqliteExecuteBatchResponse::SqliteErrorResponse(error) => {
				Err(self.remote_sqlite_error_response(error))
			}
		}
	}

	async fn remote_execute_with_session(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
		expected_session: Option<u64>,
	) -> Result<(ExecuteResult, u64)> {
		let config = self.remote_config()?;
		let (response, session) = config
			.handle
			.remote_sqlite_execute_with_session(
				protocol::SqliteExecuteRequest {
					namespace_id: config.namespace_id,
					actor_id: config.actor_id,
					generation: config.generation,
					sql,
					params: params.map(protocol_bind_params),
				},
				expected_session,
			)
			.await?;

		match response {
			protocol::SqliteExecuteResponse::SqliteExecuteOk(ok) => {
				Ok((execute_result_from_protocol(ok.result), session))
			}
			protocol::SqliteExecuteResponse::SqliteErrorResponse(error) => {
				Err(self.remote_sqlite_error_response(error))
			}
		}
	}

	pub(crate) async fn query_rows_cbor(
		&self,
		sql: &str,
		params: Option<&[u8]>,
	) -> Result<Vec<u8>> {
		let bind_params = bind_params_from_cbor(sql, params)?;
		let result = self.query(sql.to_owned(), bind_params).await?;
		encode_json_as_cbor(&query_result_to_json_rows(&result))
	}

	pub(crate) async fn exec_rows_cbor(&self, sql: &str) -> Result<Vec<u8>> {
		let result = self.exec(sql.to_owned()).await?;
		encode_json_as_cbor(&query_result_to_json_rows(&result))
	}

	pub(crate) async fn run_cbor(&self, sql: &str, params: Option<&[u8]>) -> Result<ExecResult> {
		let bind_params = bind_params_from_cbor(sql, params)?;
		self.run(sql.to_owned(), bind_params).await
	}

	pub(crate) async fn execute_rows_cbor(
		&self,
		sql: &str,
		params: Option<&[u8]>,
	) -> Result<Vec<u8>> {
		let bind_params = bind_params_from_cbor(sql, params)?;
		let result = self.execute(sql.to_owned(), bind_params).await?;
		encode_json_as_cbor(&query_result_to_json_rows(&QueryResult {
			columns: result.columns,
			rows: result.rows,
		}))
	}

	fn handle(&self) -> Result<EnvoyHandle> {
		self.handle
			.clone()
			.ok_or_else(|| sqlite_not_configured("handle"))
	}

	fn remote_sqlite_error_response(&self, error: protocol::SqliteErrorResponse) -> anyhow::Error {
		if is_head_fence_mismatch_response(&error) {
			if let Ok(config) = self.runtime_config() {
				report_sqlite_worker_fatal(
					&self.worker_fatal_reported,
					config,
					format!("remote sqlite fatal storage error: {}", error.message),
				);
			}
			return SqliteRuntimeError::Closed.build();
		}

		remote_sqlite_error_response(error.message)
	}
}

fn report_sqlite_worker_fatal(reported: &AtomicBool, config: SqliteRuntimeConfig, message: String) {
	if reported.swap(true, Ordering::AcqRel) {
		return;
	}
	// A dead worker means SQLite's sole native connection is no longer a valid
	// actor subsystem. Core reports that through envoy lifecycle instead of
	// letting the actor continue to serve requests with a broken database.
	config.handle.stop_actor(
		config.actor_id,
		config
			.generation
			.and_then(|generation| generation.try_into().ok()),
		Some(message),
	);
}

struct RemoteSqliteConfig {
	handle: EnvoyHandle,
	namespace_id: String,
	actor_id: String,
	generation: u64,
}

fn select_sqlite_backend(remote_sqlite: bool) -> Result<SqliteBackend> {
	if remote_sqlite {
		return Ok(SqliteBackend::RemoteEnvoy);
	}

	#[cfg(feature = "sqlite-local")]
	{
		Ok(SqliteBackend::LocalNative)
	}

	#[cfg(not(feature = "sqlite-local"))]
	{
		Err(SqliteRuntimeError::Unavailable.build())
	}
}

fn bind_param_count(params: &Option<Vec<BindParam>>) -> usize {
	params.as_ref().map_or(0, Vec::len)
}

#[cfg(feature = "sqlite-local")]
fn duration_ns(duration: std::time::Duration) -> u64 {
	duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

#[cfg(feature = "sqlite-local")]
fn is_fatal_worker_error(error: &anyhow::Error) -> bool {
	error.downcast_ref::<SqliteWorkerFatalError>().is_some()
		|| error.downcast_ref::<SqliteWorkerDeadError>().is_some()
		|| error
			.downcast_ref::<SqliteWorkerCloseTimeoutError>()
			.is_some()
}

#[cfg(feature = "sqlite-local")]
fn sqlite_worker_fatal_message(error: &anyhow::Error) -> String {
	if let Some(error) = error.downcast_ref::<SqliteWorkerFatalError>() {
		return format!("sqlite fatal storage error: {}", error.message());
	}

	format!("sqlite worker failed: {error}")
}

#[cfg(feature = "sqlite-local")]
fn map_local_worker_error(error: anyhow::Error) -> anyhow::Error {
	if error
		.downcast_ref::<SqliteWorkerOverloadedError>()
		.is_some()
	{
		return ActorLifecycle::Overloaded {
			channel: "sqlite_worker".to_string(),
			capacity: SQLITE_WORKER_QUEUE_CAPACITY,
			operation: "execute sqlite command".to_string(),
		}
		.build();
	}

	if error.downcast_ref::<SqliteWorkerClosingError>().is_some()
		|| error.downcast_ref::<SqliteWorkerDeadError>().is_some()
		|| error.downcast_ref::<SqliteWorkerFatalError>().is_some()
	{
		return SqliteRuntimeError::Closed.build();
	}

	error
}

fn protocol_bind_params(params: Vec<BindParam>) -> Vec<protocol::SqliteBindParam> {
	params.into_iter().map(protocol_bind_param).collect()
}

fn protocol_bind_param(param: BindParam) -> protocol::SqliteBindParam {
	match param {
		BindParam::Null => protocol::SqliteBindParam::SqliteValueNull,
		BindParam::Integer(value) => {
			protocol::SqliteBindParam::SqliteValueInteger(protocol::SqliteValueInteger { value })
		}
		BindParam::Float(value) => {
			protocol::SqliteBindParam::SqliteValueFloat(protocol::SqliteValueFloat {
				value: value.to_bits().to_be_bytes(),
			})
		}
		BindParam::Text(value) => {
			protocol::SqliteBindParam::SqliteValueText(protocol::SqliteValueText { value })
		}
		BindParam::Blob(value) => {
			protocol::SqliteBindParam::SqliteValueBlob(protocol::SqliteValueBlob { value })
		}
	}
}

fn query_result_from_protocol(result: protocol::SqliteQueryResult) -> QueryResult {
	QueryResult {
		columns: result.columns,
		rows: result
			.rows
			.into_iter()
			.map(|row| row.into_iter().map(column_value_from_protocol).collect())
			.collect(),
	}
}

fn execute_result_from_protocol(result: protocol::SqliteExecuteResult) -> ExecuteResult {
	ExecuteResult {
		columns: result.columns,
		rows: result
			.rows
			.into_iter()
			.map(|row| row.into_iter().map(column_value_from_protocol).collect())
			.collect(),
		changes: result.changes,
		last_insert_row_id: result.last_insert_row_id,
	}
}

fn column_value_from_protocol(value: protocol::SqliteColumnValue) -> ColumnValue {
	match value {
		protocol::SqliteColumnValue::SqliteValueNull => ColumnValue::Null,
		protocol::SqliteColumnValue::SqliteValueInteger(value) => ColumnValue::Integer(value.value),
		protocol::SqliteColumnValue::SqliteValueFloat(value) => {
			ColumnValue::Float(f64::from_bits(u64::from_be_bytes(value.value)))
		}
		protocol::SqliteColumnValue::SqliteValueText(value) => ColumnValue::Text(value.value),
		protocol::SqliteColumnValue::SqliteValueBlob(value) => ColumnValue::Blob(value.value),
	}
}

fn remote_request_error(error: anyhow::Error) -> anyhow::Error {
	if let Some(indeterminate) = error.downcast_ref::<RemoteSqliteIndeterminateResultError>() {
		return SqliteRuntimeError::RemoteIndeterminateResult {
			operation: indeterminate.operation.to_owned(),
		}
		.build();
	}

	if let Some(compatibility) =
		error.downcast_ref::<protocol::versioned::ProtocolCompatibilityError>()
	{
		if compatibility.feature
			== protocol::versioned::ProtocolCompatibilityFeature::RemoteSqliteExecution
		{
			return SqliteRuntimeError::RemoteUnavailable {
				reason: compatibility.to_string(),
			}
			.build();
		}
	}

	error
}

fn remote_sqlite_error_response(message: String) -> anyhow::Error {
	if message.contains("unavailable") || message.contains("unsupported") {
		return SqliteRuntimeError::RemoteUnavailable { reason: message }.build();
	}

	SqliteRuntimeError::RemoteExecutionFailed { message }.build()
}

fn is_head_fence_mismatch_response(error: &protocol::SqliteErrorResponse) -> bool {
	is_head_fence_mismatch(&error.group, &error.code)
}
impl std::fmt::Debug for SqliteDb {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SqliteDb")
			.field("configured", &self.handle.is_some())
			.field("actor_id", &self.actor_id)
			.finish()
	}
}

fn bind_params_from_cbor(sql: &str, params: Option<&[u8]>) -> Result<Option<Vec<BindParam>>> {
	let Some(params) = params else {
		return Ok(None);
	};
	if params.is_empty() {
		return Ok(None);
	}

	let value = ciborium::from_reader::<JsonValue, _>(Cursor::new(params))
		.context("decode sqlite bind params as cbor json")?;
	match value {
		JsonValue::Array(values) => values
			.iter()
			.map(json_to_bind_param)
			.collect::<Result<Vec<_>>>()
			.map(Some),
		JsonValue::Object(properties) => {
			let ordered_names = extract_named_sqlite_parameters(sql);
			if ordered_names.is_empty() {
				return properties
					.values()
					.map(json_to_bind_param)
					.collect::<Result<Vec<_>>>()
					.map(Some);
			}

			ordered_names
				.iter()
				.map(|name| {
					get_named_sqlite_binding(&properties, name)
						.ok_or_else(|| {
							SqliteRuntimeError::InvalidBindParameter {
								name: name.clone(),
								reason: "missing parameter".to_owned(),
							}
							.build()
						})
						.and_then(json_to_bind_param)
				})
				.collect::<Result<Vec<_>>>()
				.map(Some)
		}
		JsonValue::Null => Ok(None),
		other => Err(SqliteRuntimeError::InvalidBindParameter {
			name: "params".to_owned(),
			reason: format!("expected array or object, got {}", json_type_name(&other)),
		}
		.build()),
	}
}

fn json_to_bind_param(value: &JsonValue) -> Result<BindParam> {
	match value {
		JsonValue::Null => Ok(BindParam::Null),
		JsonValue::Bool(value) => Ok(BindParam::Integer(i64::from(*value))),
		JsonValue::Number(value) => {
			if let Some(value) = value.as_i64() {
				return Ok(BindParam::Integer(value));
			}
			if let Some(value) = value.as_u64() {
				let value = i64::try_from(value)
					.context("sqlite integer bind parameter exceeds i64 range")?;
				return Ok(BindParam::Integer(value));
			}
			value.as_f64().map(BindParam::Float).ok_or_else(|| {
				SqliteRuntimeError::InvalidBindParameter {
					name: "number".to_owned(),
					reason: "unsupported numeric value".to_owned(),
				}
				.build()
			})
		}
		JsonValue::String(value) => Ok(BindParam::Text(value.clone())),
		other => Err(SqliteRuntimeError::InvalidBindParameter {
			name: "value".to_owned(),
			reason: format!("unsupported type {}", json_type_name(other)),
		}
		.build()),
	}
}

fn sqlite_not_configured(component: &str) -> anyhow::Error {
	SqliteRuntimeError::NotConfigured {
		component: component.to_owned(),
	}
	.build()
}

fn extract_named_sqlite_parameters(sql: &str) -> Vec<String> {
	let mut ordered_names = Vec::new();
	let mut seen = HashSet::new();
	let bytes = sql.as_bytes();
	let mut idx = 0;

	while idx < bytes.len() {
		let byte = bytes[idx];
		if !matches!(byte, b':' | b'@' | b'$') {
			idx += 1;
			continue;
		}

		let start = idx;
		idx += 1;
		if idx >= bytes.len() || !is_sqlite_param_start(bytes[idx]) {
			continue;
		}
		idx += 1;
		while idx < bytes.len() && is_sqlite_param_continue(bytes[idx]) {
			idx += 1;
		}

		let name = &sql[start..idx];
		if seen.insert(name.to_owned()) {
			ordered_names.push(name.to_owned());
		}
	}

	ordered_names
}

fn is_sqlite_param_start(byte: u8) -> bool {
	byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_sqlite_param_continue(byte: u8) -> bool {
	byte == b'_' || byte.is_ascii_alphanumeric()
}

fn get_named_sqlite_binding<'a>(
	bindings: &'a JsonMap<String, JsonValue>,
	name: &str,
) -> Option<&'a JsonValue> {
	if let Some(value) = bindings.get(name) {
		return Some(value);
	}

	let bare_name = name.get(1..)?;
	if let Some(value) = bindings.get(bare_name) {
		return Some(value);
	}

	for prefix in [":", "@", "$"] {
		let candidate = format!("{prefix}{bare_name}");
		if let Some(value) = bindings.get(&candidate) {
			return Some(value);
		}
	}

	None
}

fn query_result_to_json_rows(result: &QueryResult) -> JsonValue {
	JsonValue::Array(
		result
			.rows
			.iter()
			.map(|row| {
				let mut object = JsonMap::new();
				for (index, column) in result.columns.iter().enumerate() {
					let value = row
						.get(index)
						.map(column_value_to_json)
						.unwrap_or(JsonValue::Null);
					object.insert(column.clone(), value);
				}
				JsonValue::Object(object)
			})
			.collect(),
	)
}

fn column_value_to_json(value: &ColumnValue) -> JsonValue {
	match value {
		ColumnValue::Null => JsonValue::Null,
		ColumnValue::Integer(value) => JsonValue::from(*value),
		ColumnValue::Float(value) => JsonValue::from(*value),
		ColumnValue::Text(value) => JsonValue::String(value.clone()),
		ColumnValue::Blob(value) => {
			JsonValue::Array(value.iter().map(|byte| JsonValue::from(*byte)).collect())
		}
	}
}

fn encode_json_as_cbor(value: &impl Serialize) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded).context("encode sqlite rows as cbor")?;
	Ok(encoded)
}

fn json_type_name(value: &JsonValue) -> &'static str {
	match value {
		JsonValue::Null => "null",
		JsonValue::Bool(_) => "boolean",
		JsonValue::Number(_) => "number",
		JsonValue::String(_) => "string",
		JsonValue::Array(_) => "array",
		JsonValue::Object(_) => "object",
	}
}

#[cfg(test)]
#[path = "../../../tests/sqlite.rs"]
mod tests;
