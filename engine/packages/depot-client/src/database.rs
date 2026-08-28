use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rivet_envoy_protocol as protocol;
use tokio::runtime::Handle;

use crate::{
	query::{BindParam, ExecResult, ExecuteResult, QueryResult},
	vfs::{
		NativeVfsHandle, SqliteOpenPhase, SqliteTransportHandle, SqliteVfs, SqliteVfsMetrics,
		SqliteVfsMetricsSnapshot, VfsConfig, VfsPreloadHintSnapshot,
		fetch_initial_pages_for_registration,
	},
	worker::{SqliteWorkerFatalError, SqliteWorkerHandle, SqliteWorkerResult},
};

#[derive(Clone)]
pub struct NativeDatabaseHandle {
	vfs: NativeVfsHandle,
	worker: SqliteWorkerHandle,
}

pub fn vfs_name_for_actor_database(actor_id: &str, generation: u64) -> String {
	format!("envoy-sqlite-{actor_id}-g{generation}")
}

struct GenerationFencedTransport {
	inner: SqliteTransportHandle,
	generation: u64,
}

#[async_trait]
impl crate::vfs::SqliteTransport for GenerationFencedTransport {
	async fn get_pages(
		&self,
		mut request: protocol::SqliteGetPagesRequest,
	) -> Result<protocol::SqliteGetPagesResponse> {
		request.expected_generation.get_or_insert(self.generation);
		self.inner.get_pages(request).await
	}

	async fn commit(
		&self,
		mut request: protocol::SqliteCommitRequest,
	) -> Result<protocol::SqliteCommitResponse> {
		request.expected_generation.get_or_insert(self.generation);
		self.inner.commit(request).await
	}
}

pub async fn open_database_from_transport(
	transport: SqliteTransportHandle,
	actor_id: String,
	generation: u64,
	rt_handle: Handle,
	metrics: Option<Arc<dyn SqliteVfsMetrics>>,
) -> Result<NativeDatabaseHandle> {
	let open_timer = SqliteOpenTimer::new(&metrics);
	let vfs_name = vfs_name_for_actor_database(&actor_id, generation);
	let config = VfsConfig::default();
	let transport: SqliteTransportHandle = Arc::new(GenerationFencedTransport {
		inner: transport,
		generation,
	});
	let preload_start = Instant::now();
	let preload_result =
		fetch_initial_pages_for_registration(transport.clone(), &actor_id, generation, &config)
			.await
			.map_err(|err| anyhow!("failed to preload sqlite pages: {err}"));
	let initial_pages = observe_open_phase_result(
		&metrics,
		SqliteOpenPhase::InitialPreload,
		preload_start,
		preload_result,
	)?;
	record_startup_preload_pages(
		&metrics,
		u64::from(initial_pages.requested_page_count),
		initial_pages.pages.len() as u64,
	);

	let vfs_register_start = Instant::now();
	let vfs_result = SqliteVfs::register_with_transport_and_initial_pages(
		&vfs_name,
		transport,
		actor_id.clone(),
		rt_handle,
		config,
		initial_pages,
		metrics.clone(),
	)
	.map(Arc::new)
	.map_err(|err| anyhow!("failed to register sqlite VFS: {err}"));
	let vfs = observe_open_phase_result(
		&metrics,
		SqliteOpenPhase::VfsRegister,
		vfs_register_start,
		vfs_result,
	)?;

	let worker_ready_start = Instant::now();
	let worker_ready_result: Result<NativeDatabaseHandle> = async {
		let native_db = NativeDatabaseHandle::new_with_metrics(vfs, actor_id, metrics.clone())?;
		native_db.initialize().await?;
		Ok(native_db)
	}
	.await;
	let native_db = observe_open_phase_result(
		&metrics,
		SqliteOpenPhase::WorkerReady,
		worker_ready_start,
		worker_ready_result,
	)?;
	open_timer.finish_success();
	Ok(native_db)
}

/// Records the total SQLite open metric when the open attempt leaves scope.
///
/// Individual open phases record their own durations. This guard owns the total
/// duration so early returns cannot forget the total error metric.
struct SqliteOpenTimer {
	metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	started_at: Instant,
	finished: bool,
}

impl SqliteOpenTimer {
	fn new(metrics: &Option<Arc<dyn SqliteVfsMetrics>>) -> Self {
		Self {
			metrics: metrics.clone(),
			started_at: Instant::now(),
			finished: false,
		}
	}

	fn finish_success(mut self) {
		observe_open_phase(
			&self.metrics,
			SqliteOpenPhase::Total,
			"success",
			self.started_at,
		);
		self.finished = true;
	}
}

impl Drop for SqliteOpenTimer {
	fn drop(&mut self) {
		if self.finished {
			return;
		}

		observe_open_phase(
			&self.metrics,
			SqliteOpenPhase::Total,
			"error",
			self.started_at,
		);
	}
}

fn observe_open_phase_result<T, E>(
	metrics: &Option<Arc<dyn SqliteVfsMetrics>>,
	phase: SqliteOpenPhase,
	start: Instant,
	result: std::result::Result<T, E>,
) -> std::result::Result<T, E> {
	let outcome = if result.is_ok() { "success" } else { "error" };
	observe_open_phase(metrics, phase, outcome, start);
	result
}

fn observe_open_phase(
	metrics: &Option<Arc<dyn SqliteVfsMetrics>>,
	phase: SqliteOpenPhase,
	outcome: &'static str,
	start: Instant,
) {
	if let Some(metrics) = metrics {
		metrics.observe_open_phase(phase, outcome, start.elapsed().as_nanos() as u64);
	}
}

fn record_startup_preload_pages(
	metrics: &Option<Arc<dyn SqliteVfsMetrics>>,
	requested_pages: u64,
	loaded_pages: u64,
) {
	if let Some(metrics) = metrics {
		if requested_pages > 0 {
			metrics.record_startup_preload_pages("requested", requested_pages);
		}
		if loaded_pages > 0 {
			metrics.record_startup_preload_pages("loaded", loaded_pages);
		}
	}
}

impl NativeDatabaseHandle {
	pub fn new(vfs: NativeVfsHandle, file_name: String) -> Result<Self> {
		Self::new_with_metrics(vfs, file_name, None)
	}

	pub fn new_with_metrics(
		vfs: NativeVfsHandle,
		file_name: String,
		metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	) -> Result<Self> {
		Ok(Self {
			worker: SqliteWorkerHandle::start(vfs.clone(), file_name, metrics)?,
			vfs,
		})
	}

	pub async fn exec(&self, sql: String) -> Result<QueryResult> {
		self.check_fatal_error()?;
		self.map_worker_result(self.worker.exec(sql).await)
	}

	pub async fn exec_profiled(&self, sql: String) -> Result<SqliteWorkerResult<QueryResult>> {
		self.check_fatal_error()?;
		self.map_worker_result(self.worker.exec_profiled(sql).await)
	}

	pub async fn query(&self, sql: String, params: Option<Vec<BindParam>>) -> Result<QueryResult> {
		self.execute(sql, params).await.map(|result| QueryResult {
			columns: result.columns,
			rows: result.rows,
		})
	}

	pub async fn run(&self, sql: String, params: Option<Vec<BindParam>>) -> Result<ExecResult> {
		self.execute(sql, params).await.map(|result| ExecResult {
			changes: result.changes,
		})
	}

	pub async fn execute(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		self.check_fatal_error()?;
		self.map_worker_result(self.worker.execute(sql, params).await)
	}

	pub async fn execute_profiled(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<SqliteWorkerResult<ExecuteResult>> {
		self.check_fatal_error()?;
		self.map_worker_result(self.worker.execute_profiled(sql, params).await)
	}

	pub async fn close(&self) -> Result<()> {
		match self.worker.close().await {
			Ok(()) => Ok(()),
			Err(error) => Err(self.fatal_error().unwrap_or(error)),
		}
	}

	pub async fn wait_for_worker_failure(&self) -> bool {
		self.worker.wait_for_failure().await
	}

	pub fn take_last_kv_error(&self) -> Option<String> {
		self.vfs.take_last_error()
	}

	pub fn clone_fatal_error(&self) -> Option<String> {
		self.vfs.clone_fatal_error()
	}

	pub fn snapshot_preload_hints(&self) -> VfsPreloadHintSnapshot {
		self.vfs.snapshot_preload_hints()
	}

	pub fn sqlite_vfs_metrics(&self) -> SqliteVfsMetricsSnapshot {
		self.vfs.sqlite_vfs_metrics()
	}

	#[cfg(test)]
	pub(crate) async fn pause_for_test(&self) -> tokio::sync::oneshot::Sender<()> {
		self.worker.pause_for_test().await
	}

	#[cfg(test)]
	pub(crate) fn is_closing_for_test(&self) -> bool {
		self.worker.is_closing_for_test()
	}

	#[cfg(test)]
	pub(crate) async fn panic_worker_for_test(&self) {
		self.worker.panic_for_test().await
	}

	async fn initialize(&self) -> Result<()> {
		self.map_worker_result(self.worker.wait_ready().await)
	}

	fn check_fatal_error(&self) -> Result<()> {
		if let Some(error) = self.fatal_error() {
			return Err(error);
		}

		Ok(())
	}

	fn map_worker_result<T>(&self, result: Result<T>) -> Result<T> {
		match result {
			Ok(value) => {
				self.check_fatal_error()?;
				Ok(value)
			}
			Err(error) => Err(self.fatal_error().unwrap_or(error)),
		}
	}

	fn fatal_error(&self) -> Option<anyhow::Error> {
		self.clone_fatal_error()
			.map(|message| SqliteWorkerFatalError::new(message).into())
	}
}

#[cfg(test)]
mod tests {
	use super::vfs_name_for_actor_database;

	#[test]
	fn vfs_name_includes_actor_and_generation() {
		assert_eq!(
			vfs_name_for_actor_database("actor-123", 42),
			"envoy-sqlite-actor-123-g42"
		);
	}
}
