mod fault;
mod vfs_support;

pub(super) use vfs_support::{
	DirectDepotTransport, DirectMirrorTransport, DirectStorage, DirectStorageStats,
	sqlite_error_response, storage_dirty_page,
};

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex as SyncMutex;
use rivet_envoy_protocol as protocol;
use tempfile::TempDir;
use tokio::runtime::Builder;
use tokio::sync::{Notify, OnceCell};

use crate::optimization_flags::{
	DEFAULT_PAGER_CACHE_SIZE_KIB, DEFAULT_STARTUP_PRELOAD_MAX_BYTES,
	DEFAULT_VFS_PAGE_CACHE_CAPACITY_PAGES, DEFAULT_VFS_PROTECTED_CACHE_PAGES,
	DEFAULT_VFS_STAGING_CACHE_TTL_MS, SqliteOptimizationFlags, SqliteReadAheadMode,
	SqliteVfsPageCacheMode,
};
use crate::query::{BindParam, ColumnValue};
use crate::vfs::SqliteVfsMetrics;
use crate::worker::SqliteWorkerFatalError;

use super::*;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn vfs_config_wires_optimization_flags() {
	let flags = SqliteOptimizationFlags {
		read_ahead_mode: SqliteReadAheadMode::Off,
		read_ahead: false,
		adaptive_read_ahead: false,
		recent_page_hints: true,
		cache_hit_predictor_training: true,
		preload_hint_flush: true,
		startup_preload_max_bytes: DEFAULT_STARTUP_PRELOAD_MAX_BYTES / 2,
		startup_preload_first_pages: false,
		startup_preload_first_page_count: 7,
		preload_hints_on_open: false,
		preload_hint_hot_pages: false,
		preload_hint_early_pages: false,
		preload_hint_scan_ranges: true,
		dedup_get_pages_meta: true,
		cache_get_pages_validation: true,
		range_reads: true,
		batch_chunk_reads: true,
		decoded_ltx_cache: true,
		vfs_page_cache_mode: SqliteVfsPageCacheMode::Startup,
		vfs_page_cache_capacity_pages: DEFAULT_VFS_PAGE_CACHE_CAPACITY_PAGES / 2,
		vfs_protected_cache_pages: DEFAULT_VFS_PROTECTED_CACHE_PAGES / 2,
		vfs_staging_cache_ttl_ms: DEFAULT_VFS_STAGING_CACHE_TTL_MS / 2,
		pager_cache_size_kib: DEFAULT_PAGER_CACHE_SIZE_KIB,
	};

	let config = VfsConfig::from_optimization_flags(flags);
	assert_eq!(config.page_cache_mode, SqliteVfsPageCacheMode::Startup);
	assert_eq!(
		config.cache_capacity_pages,
		DEFAULT_VFS_PAGE_CACHE_CAPACITY_PAGES / 2
	);
	assert_eq!(config.protected_cache_pages, 0);
	assert_eq!(
		config.staging_cache_ttl_ms,
		DEFAULT_VFS_STAGING_CACHE_TTL_MS / 2
	);
	assert_eq!(config.prefetch_depth, 16);
	assert!(!config.adaptive_read_ahead);
	assert_eq!(
		config.startup_preload_max_bytes,
		DEFAULT_STARTUP_PRELOAD_MAX_BYTES / 2
	);
	assert!(!config.startup_preload_first_pages);
	assert_eq!(config.startup_preload_first_page_count, 7);
	assert!(!config.preload_hints_on_open);
	assert!(!config.preload_hint_early_pages);
	assert!(config.retain_read_cache);
	assert_eq!(config.recent_hint_page_budget, 0);
	assert!(config.recent_hint_range_budget > 0);
}

#[derive(Default)]
struct RecordingInitialPagesTransport {
	requested_pgnos: SyncMutex<Vec<u32>>,
}

#[async_trait]
impl SqliteTransport for RecordingInitialPagesTransport {
	async fn get_pages(
		&self,
		request: protocol::SqliteGetPagesRequest,
	) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
		*self.requested_pgnos.lock() = request.pgnos.clone();
		Ok(protocol::SqliteGetPagesResponse::SqliteGetPagesOk(
			protocol::SqliteGetPagesOk {
				pages: request
					.pgnos
					.into_iter()
					.map(|pgno| protocol::SqliteFetchedPage {
						pgno,
						bytes: Some(vec![pgno as u8; DEFAULT_PAGE_SIZE]),
					})
					.collect(),
				head_txid: Some(0),
			},
		))
	}

	async fn commit(
		&self,
		_request: protocol::SqliteCommitRequest,
	) -> anyhow::Result<protocol::SqliteCommitResponse> {
		anyhow::bail!("initial-page preload test does not commit")
	}
}

struct CappedAtomicCommitTransport {
	inner: DirectDepotTransport,
	max_dirty_bytes: usize,
	rejected: AtomicBool,
	max_seen_dirty_bytes: AtomicUsize,
	max_seen_dirty_pages: AtomicUsize,
}

impl CappedAtomicCommitTransport {
	fn new(engine: Arc<DirectStorage>, max_dirty_bytes: usize) -> Self {
		Self {
			inner: DirectDepotTransport::new(engine),
			max_dirty_bytes,
			rejected: AtomicBool::new(false),
			max_seen_dirty_bytes: AtomicUsize::new(0),
			max_seen_dirty_pages: AtomicUsize::new(0),
		}
	}

	fn rejected(&self) -> bool {
		self.rejected.load(Ordering::SeqCst)
	}

	fn max_seen_dirty_bytes(&self) -> usize {
		self.max_seen_dirty_bytes.load(Ordering::SeqCst)
	}

	fn max_seen_dirty_pages(&self) -> usize {
		self.max_seen_dirty_pages.load(Ordering::SeqCst)
	}
}

#[async_trait]
impl SqliteTransport for CappedAtomicCommitTransport {
	async fn get_pages(
		&self,
		request: protocol::SqliteGetPagesRequest,
	) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
		self.inner.get_pages(request).await
	}

	async fn commit(
		&self,
		request: protocol::SqliteCommitRequest,
	) -> anyhow::Result<protocol::SqliteCommitResponse> {
		let dirty_bytes = request
			.dirty_pages
			.iter()
			.map(|page| page.bytes.len())
			.sum::<usize>();
		self.max_seen_dirty_bytes
			.fetch_max(dirty_bytes, Ordering::SeqCst);
		self.max_seen_dirty_pages
			.fetch_max(request.dirty_pages.len(), Ordering::SeqCst);

		if dirty_bytes > self.max_dirty_bytes {
			self.rejected.store(true, Ordering::SeqCst);
			return Ok(protocol::SqliteCommitResponse::SqliteErrorResponse(
				protocol::SqliteErrorResponse {
					group: "sqlite".to_string(),
					code: "transaction_too_large".to_string(),
					message: format!(
						"simulated atomic commit cap exceeded: dirty_bytes={dirty_bytes} max_dirty_bytes={}",
						self.max_dirty_bytes
					),
				},
			));
		}

		self.inner.commit(request).await
	}
}

struct MissingDbTransport;

#[async_trait]
impl SqliteTransport for MissingDbTransport {
	async fn get_pages(
		&self,
		_request: protocol::SqliteGetPagesRequest,
	) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
		Ok(protocol::SqliteGetPagesResponse::SqliteErrorResponse(
			protocol::SqliteErrorResponse {
				group: "depot".to_string(),
				code: "database_not_found".to_string(),
				message: "sqlite database was not found in this bucket branch".to_string(),
			},
		))
	}

	async fn commit(
		&self,
		_request: protocol::SqliteCommitRequest,
	) -> anyhow::Result<protocol::SqliteCommitResponse> {
		anyhow::bail!("missing-db transport test does not commit")
	}
}

struct DelayCapturedGetPagesTransport {
	inner: Arc<DirectDepotTransport>,
	delay_pgno: u32,
	enabled: AtomicBool,
	delayed: AtomicBool,
	captured: Arc<Notify>,
	release: Arc<Notify>,
}

impl DelayCapturedGetPagesTransport {
	fn new(inner: Arc<DirectDepotTransport>, delay_pgno: u32) -> Self {
		Self {
			inner,
			delay_pgno,
			enabled: AtomicBool::new(false),
			delayed: AtomicBool::new(false),
			captured: Arc::new(Notify::new()),
			release: Arc::new(Notify::new()),
		}
	}

	fn enable(&self) {
		self.enabled.store(true, Ordering::SeqCst);
	}

	async fn wait_captured(&self) {
		self.captured.notified().await;
	}

	fn release(&self) {
		self.release.notify_one();
	}
}

#[async_trait]
impl SqliteTransport for DelayCapturedGetPagesTransport {
	async fn get_pages(
		&self,
		request: protocol::SqliteGetPagesRequest,
	) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
		let should_delay = self.enabled.load(Ordering::SeqCst)
			&& (self.delay_pgno == 0 || request.pgnos.contains(&self.delay_pgno))
			&& !self.delayed.swap(true, Ordering::SeqCst);
		if !should_delay {
			return self.inner.get_pages(request).await;
		}

		// Capture real Depot bytes first, then release them after another writer advances the head.
		let response = self.inner.get_pages(request).await?;
		self.captured.notify_one();
		self.release.notified().await;
		Ok(response)
	}

	async fn commit(
		&self,
		request: protocol::SqliteCommitRequest,
	) -> anyhow::Result<protocol::SqliteCommitResponse> {
		self.inner.commit(request).await
	}
}

#[test]
fn startup_initial_pages_do_not_require_preload_hints_on_open() {
	let runtime = direct_runtime();
	let transport = Arc::new(RecordingInitialPagesTransport::default());
	let config = VfsConfig {
		startup_preload_first_pages: true,
		startup_preload_first_page_count: 4,
		startup_preload_max_bytes: DEFAULT_PAGE_SIZE * 4,
		preload_hints_on_open: false,
		page_cache_mode: SqliteVfsPageCacheMode::Startup,
		..VfsConfig::default()
	};

	let pages = runtime
		.block_on(fetch_initial_pages_for_registration(
			transport.clone(),
			"startup-preload-actor",
			0,
			&config,
		))
		.expect("initial pages should load");

	let loaded_pgnos = pages
		.pages
		.iter()
		.map(|(pgno, _)| *pgno)
		.collect::<Vec<_>>();
	assert_eq!(*transport.requested_pgnos.lock(), vec![1, 2, 3, 4]);
	assert_eq!(loaded_pgnos, vec![1, 2, 3, 4]);
}

#[test]
fn vfs_staging_cache_retains_only_speculative_pages() {
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		staging_cache_ttl_ms: DEFAULT_VFS_STAGING_CACHE_TTL_MS,
		..VfsConfig::default()
	};
	let mut state = VfsState::new(&config, true);

	state.cache_page(
		&config,
		PageCacheInsertKind::Target,
		2,
		vec![2; DEFAULT_PAGE_SIZE],
	);
	assert!(state.cached_page(&config, 2).is_none());

	state.cache_page(
		&config,
		PageCacheInsertKind::Prefetch,
		3,
		vec![3; DEFAULT_PAGE_SIZE],
	);
	state.cache_page(
		&config,
		PageCacheInsertKind::Startup,
		4,
		vec![4; DEFAULT_PAGE_SIZE],
	);
	assert!(state.cached_page(&config, 3).is_some());
	assert!(state.cached_page(&config, 4).is_some());
	assert!(
		state
			.protected_page_cache
			.read_sync(&3, |_, _| ())
			.is_none()
	);

	state.evict_target_read_pages(&[1, 3, 4]);
	assert!(state.cached_page(&config, 1).is_none());
	assert!(state.cached_page(&config, 3).is_none());
	assert!(state.cached_page(&config, 4).is_none());
}

#[test]
fn disabled_profiling_skips_prefetch_tracking() {
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		staging_cache_ttl_ms: DEFAULT_VFS_STAGING_CACHE_TTL_MS,
		..VfsConfig::default()
	};
	let mut state = VfsState::new(&config, false);

	state.cache_page(
		&config,
		PageCacheInsertKind::Prefetch,
		2,
		vec![2; DEFAULT_PAGE_SIZE],
	);

	assert!(state.prefetched_pages.is_empty());
	assert_eq!(
		state.cached_page(&config, 2),
		Some((vec![2; DEFAULT_PAGE_SIZE], false))
	);
}

#[test]
fn vfs_staging_cache_ttl_zero_disables_speculative_retention() {
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		staging_cache_ttl_ms: 0,
		..VfsConfig::default()
	};
	let mut state = VfsState::new(&config, true);

	state.cache_page(
		&config,
		PageCacheInsertKind::Prefetch,
		2,
		vec![2; DEFAULT_PAGE_SIZE],
	);
	state.cache_page(
		&config,
		PageCacheInsertKind::Startup,
		3,
		vec![3; DEFAULT_PAGE_SIZE],
	);
	assert!(state.cached_page(&config, 1).is_some());
	assert!(state.cached_page(&config, 2).is_none());
	assert!(state.cached_page(&config, 3).is_none());

	state.evict_target_read_pages(&[1]);
	assert!(state.cached_page(&config, 1).is_none());
}

#[test]
fn evicted_empty_page_one_can_be_synthesized_before_first_commit() {
	let runtime = direct_runtime();
	let config = VfsConfig::default();
	let ctx = VfsContext::new(
		next_test_name("missing-db-actor"),
		None,
		runtime.handle().clone(),
		Arc::new(MissingDbTransport),
		config.clone(),
		unsafe { std::mem::zeroed() },
		InitialPages::default(),
		None,
	)
	.expect("vfs context should build");

	ctx.state.read().evict_target_read_pages(&[1]);
	assert!(ctx.state.read().cached_page(&config, 1).is_none());

	let resolved = ctx
		.resolve_pages(&[1], true)
		.expect("missing empty database should synthesize page 1");
	assert_eq!(resolved.get(&1), Some(&Some(empty_db_page())));
	assert!(ctx.state.read().cached_page(&config, 1).is_none());
}

fn next_test_name(prefix: &str) -> String {
	let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
	format!("{prefix}-{id}")
}

struct DirectEngineHarness {
	actor_id: String,
	db_dir: TempDir,
	storage: OnceCell<Arc<DirectStorage>>,
}

impl DirectEngineHarness {
	fn new() -> Self {
		Self {
			actor_id: next_test_name("sqlite-direct-actor"),
			db_dir: tempfile::tempdir().expect("temp dir should build"),
			storage: OnceCell::new(),
		}
	}

	async fn open_engine(&self) -> Arc<DirectStorage> {
		// RocksDB enforces one open handle per path, so initialization must be atomic.
		let storage = self
			.storage
			.get_or_init(|| async {
				let driver = universaldb::driver::RocksDbDatabaseDriver::new(
					self.db_dir.path().to_path_buf(),
				)
				.await
				.expect("rocksdb driver should build");
				let db = universaldb::Database::new(Arc::new(driver));

				Arc::new(DirectStorage::new(db))
			})
			.await;
		Arc::clone(storage)
	}

	fn open_db_on_engine(
		&self,
		runtime: &tokio::runtime::Runtime,
		engine: Arc<DirectStorage>,
		actor_id: &str,
		config: VfsConfig,
	) -> NativeDatabase {
		let transport = Arc::new(DirectDepotTransport::new(engine));
		self.open_db_with_transport(runtime, transport, actor_id, config)
	}

	fn open_db_with_transport(
		&self,
		runtime: &tokio::runtime::Runtime,
		transport: SqliteTransportHandle,
		actor_id: &str,
		config: VfsConfig,
	) -> NativeDatabase {
		let initial_main_page = runtime
			.block_on(fetch_initial_main_page_for_registration(
				transport.clone(),
				actor_id,
			))
			.expect("initial main page preload should succeed");
		let vfs = SqliteVfs::register_with_transport_and_initial_page(
			&next_test_name("sqlite-direct-vfs"),
			transport,
			actor_id.to_string(),
			runtime.handle().clone(),
			config,
			initial_main_page,
			None,
		)
		.expect("v2 vfs should register");

		open_database(vfs, actor_id).expect("sqlite database should open")
	}

	fn open_db(&self, runtime: &tokio::runtime::Runtime) -> NativeDatabase {
		let engine = runtime.block_on(self.open_engine());
		self.open_db_on_engine(runtime, engine, &self.actor_id, VfsConfig::default())
	}

	fn open_context(&self, runtime: &tokio::runtime::Runtime) -> VfsContext {
		let engine = runtime.block_on(self.open_engine());
		VfsContext::new(
			self.actor_id.clone(),
			None,
			runtime.handle().clone(),
			Arc::new(DirectDepotTransport::new(engine)),
			VfsConfig::default(),
			unsafe { std::mem::zeroed() },
			InitialPages::default(),
			None,
		)
		.expect("vfs context should build")
	}
}

fn direct_vfs_ctx(db: &NativeDatabase) -> &VfsContext {
	db._vfs.ctx()
}

fn open_worker_handle(
	runtime: &tokio::runtime::Runtime,
	harness: &DirectEngineHarness,
) -> crate::database::NativeDatabaseHandle {
	open_worker_handle_with_metrics(runtime, harness, None)
}

fn open_worker_handle_with_metrics(
	runtime: &tokio::runtime::Runtime,
	harness: &DirectEngineHarness,
	metrics: Option<Arc<dyn SqliteVfsMetrics>>,
) -> crate::database::NativeDatabaseHandle {
	open_worker_handle_with_vfs(runtime, harness, metrics).1
}

fn open_worker_handle_with_vfs(
	runtime: &tokio::runtime::Runtime,
	harness: &DirectEngineHarness,
	metrics: Option<Arc<dyn SqliteVfsMetrics>>,
) -> (Arc<SqliteVfs>, crate::database::NativeDatabaseHandle) {
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let config = VfsConfig::default();
	let initial_pages = runtime
		.block_on(fetch_initial_pages_for_registration(
			transport.clone(),
			&harness.actor_id,
			0,
			&config,
		))
		.expect("initial pages preload should succeed");
	let vfs = Arc::new(
		SqliteVfs::register_with_transport_and_initial_pages(
			&next_test_name("sqlite-worker-vfs"),
			transport,
			harness.actor_id.clone(),
			runtime.handle().clone(),
			config,
			initial_pages,
			None,
		)
		.expect("worker vfs should register"),
	);
	let db = crate::database::NativeDatabaseHandle::new_with_metrics(
		vfs.clone(),
		harness.actor_id.clone(),
		metrics,
	)
	.expect("worker handle should start");
	(vfs, db)
}

#[derive(Default)]
struct WorkerTestMetrics {
	queue_depth: AtomicU64,
	overloads: AtomicU64,
	command_durations: AtomicU64,
	command_errors: AtomicU64,
	close_durations: AtomicU64,
	crashes: AtomicU64,
	unclean_closes: AtomicU64,
}

impl SqliteVfsMetrics for WorkerTestMetrics {
	fn profiling_enabled(&self) -> bool {
		false
	}

	fn max_profiled_get_pages_requests(&self) -> usize {
		0
	}

	fn observe_operation_profile(&self, _profile: &SqliteOperationMetric) -> bool {
		false
	}

	fn observe_transaction_profile(&self, _profile: &SqliteTransactionMetric) -> bool {
		false
	}

	fn emit_operation_diagnostic_event(
		&self,
		_actor_id: &str,
		_generation: Option<u64>,
		_profile: &SqliteOperationMetric,
	) {
	}

	fn emit_transaction_diagnostic_event(
		&self,
		_actor_id: &str,
		_generation: Option<u64>,
		_profile: &SqliteTransactionMetric,
	) {
	}

	fn record_fingerprint_catalog(
		&self,
		_operation_type: &'static str,
		_fingerprint: &str,
		_identity: &str,
		_format_version: u8,
	) {
	}

	fn record_resolve_pages(&self, _requested_pages: u64) {}

	fn record_resolve_cache_hits(&self, _pages: u64) {}

	fn record_resolve_cache_misses(&self, _pages: u64) {}

	fn record_get_pages_request(&self, _pages: u64, _prefetch_pages: u64, _page_size: u64) {}

	fn observe_get_pages_duration(&self, _duration_ns: u64) {}

	fn observe_open_phase(
		&self,
		_phase: SqliteOpenPhase,
		_outcome: &'static str,
		_duration_ns: u64,
	) {
	}

	fn record_startup_preload_pages(&self, _kind: &'static str, _pages: u64) {}

	fn record_commit(&self) {}

	fn observe_commit_phases(
		&self,
		_request_build_ns: u64,
		_serialize_ns: u64,
		_transport_ns: u64,
		_state_update_ns: u64,
		_total_ns: u64,
	) {
	}

	fn set_worker_queue_depth(&self, depth: u64) {
		self.queue_depth.store(depth, Ordering::Release);
	}

	fn set_worker_active(&self, _active: bool) {}

	fn set_worker_inflight(&self, _active: bool) {}

	fn set_coordinator_queue_depth(&self, _depth: u64) {}

	fn record_worker_queue_overload(&self) {
		self.overloads.fetch_add(1, Ordering::AcqRel);
	}

	fn observe_worker_command_duration(
		&self,
		_operation: &'static str,
		_in_tx: bool,
		_stmt_kind: &'static str,
		_duration_ns: u64,
	) {
		self.command_durations.fetch_add(1, Ordering::AcqRel);
	}

	fn observe_transaction_round_trips(
		&self,
		_get_pages_round_trips: u64,
		_commit_round_trips: u64,
	) {
	}

	fn record_worker_command_error(&self, _operation: &'static str, _code: &'static str) {
		self.command_errors.fetch_add(1, Ordering::AcqRel);
	}

	fn observe_worker_close_duration(&self, _duration_ns: u64) {
		self.close_durations.fetch_add(1, Ordering::AcqRel);
	}

	fn record_worker_close_timeout(&self) {}

	fn record_worker_crash(&self) {
		self.crashes.fetch_add(1, Ordering::AcqRel);
	}

	fn record_worker_unclean_close(&self) {
		self.unclean_closes.fetch_add(1, Ordering::AcqRel);
	}
}

async fn wait_worker_queue_depth(metrics: &WorkerTestMetrics, depth: u64) {
	tokio::time::timeout(Duration::from_secs(5), async {
		loop {
			if metrics.queue_depth.load(Ordering::Acquire) >= depth {
				return;
			}
			tokio::task::yield_now().await;
		}
	})
	.await
	.expect("worker queue should reach expected depth");
}

async fn wait_worker_closing(db: &crate::database::NativeDatabaseHandle) {
	tokio::time::timeout(Duration::from_secs(5), async {
		loop {
			if db.is_closing_for_test() {
				return;
			}
			tokio::task::yield_now().await;
		}
	})
	.await
	.expect("worker should enter closing state");
}

async fn wait_worker_unclean_close(metrics: &WorkerTestMetrics) {
	tokio::time::timeout(Duration::from_secs(5), async {
		loop {
			if metrics.unclean_closes.load(Ordering::Acquire) >= 1 {
				return;
			}
			tokio::task::yield_now().await;
		}
	})
	.await
	.expect("worker should record unclean close");
}

#[test]
fn worker_preserves_connection_affine_state() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = open_worker_handle(&runtime, &harness);

	runtime.block_on(async {
		db.execute(
			"CREATE TEMP TABLE temp_items(id INTEGER PRIMARY KEY, label TEXT);".to_owned(),
			None,
		)
		.await
		.expect("temp table should be created");
		db.execute(
			"INSERT INTO temp_items(label) VALUES (?);".to_owned(),
			Some(vec![BindParam::Text("alpha".to_owned())]),
		)
		.await
		.expect("insert should succeed");

		let result = db
			.execute(
				"SELECT last_insert_rowid(), label FROM temp_items;".to_owned(),
				None,
			)
			.await
			.expect("connection-affine query should succeed");
		assert_eq!(
			result.rows,
			vec![vec![
				ColumnValue::Integer(1),
				ColumnValue::Text("alpha".to_owned()),
			]]
		);

		db.close().await.expect("worker should close");
	});
}

#[test]
fn worker_executes_commands_in_send_order() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = open_worker_handle(&runtime, &harness);

	runtime.block_on(async {
		db.exec("CREATE TABLE items(id INTEGER PRIMARY KEY, label TEXT);".to_owned())
			.await
			.expect("table should be created");
		let first = db.execute(
			"INSERT INTO items(label) VALUES ('first');".to_owned(),
			None,
		);
		let second = db.execute(
			"INSERT INTO items(label) VALUES ('second');".to_owned(),
			None,
		);
		let (first, second) = tokio::join!(first, second);
		first.expect("first insert should succeed");
		second.expect("second insert should succeed");

		let result = db
			.query("SELECT label FROM items ORDER BY id;".to_owned(), None)
			.await
			.expect("query should succeed");
		assert_eq!(
			result.rows,
			vec![
				vec![ColumnValue::Text("first".to_owned())],
				vec![ColumnValue::Text("second".to_owned())],
			]
		);

		db.close().await.expect("worker should close");
	});
}

#[test]
fn worker_close_rejects_new_work() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = open_worker_handle(&runtime, &harness);

	runtime.block_on(async {
		db.close().await.expect("worker should close");
		let error = db
			.query("SELECT 1;".to_owned(), None)
			.await
			.expect_err("closed worker should reject work");
		assert!(
			error.to_string().contains("sqlite worker is closing")
				|| error.to_string().contains("sqlite worker is closed"),
			"unexpected error: {error}"
		);
	});
}

#[test]
fn worker_queue_full_returns_actor_overloaded() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let metrics = Arc::new(WorkerTestMetrics::default());
	let db = open_worker_handle_with_metrics(
		&runtime,
		&harness,
		Some(metrics.clone() as Arc<dyn SqliteVfsMetrics>),
	);

	runtime.block_on(async {
		let resume = db.pause_for_test().await;
		let mut pending = Vec::new();
		for _ in 0..crate::worker::SQLITE_WORKER_QUEUE_CAPACITY {
			let db = db.clone();
			pending.push(tokio::spawn(async move {
				db.query("SELECT 1;".to_owned(), None).await
			}));
		}
		wait_worker_queue_depth(&metrics, crate::worker::SQLITE_WORKER_QUEUE_CAPACITY as u64).await;

		let error = db
			.query("SELECT 2;".to_owned(), None)
			.await
			.expect_err("full worker queue should reject new work");
		assert!(error.to_string().contains("actor.overloaded"));
		assert_eq!(metrics.overloads.load(Ordering::Acquire), 1);

		let _ = resume.send(());
		for task in pending {
			task.await
				.expect("queued query task should join")
				.expect("queued query should run after pause");
		}
		db.close().await.expect("worker should close");
	});
}

#[test]
fn worker_close_bypasses_full_queue_and_fails_queued_work() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let metrics = Arc::new(WorkerTestMetrics::default());
	let db = open_worker_handle_with_metrics(
		&runtime,
		&harness,
		Some(metrics.clone() as Arc<dyn SqliteVfsMetrics>),
	);

	runtime.block_on(async {
		db.exec("CREATE TABLE items(id INTEGER PRIMARY KEY, label TEXT);".to_owned())
			.await
			.expect("table should be created");
		let resume = db.pause_for_test().await;
		let mut pending = Vec::new();
		for idx in 0..crate::worker::SQLITE_WORKER_QUEUE_CAPACITY {
			let db = db.clone();
			pending.push(tokio::spawn(async move {
				db.execute(
					format!("INSERT INTO items(label) VALUES ('queued-{idx}');"),
					None,
				)
				.await
			}));
		}
		wait_worker_queue_depth(&metrics, crate::worker::SQLITE_WORKER_QUEUE_CAPACITY as u64).await;

		let close_db = db.clone();
		let close_task = tokio::spawn(async move { close_db.close().await });
		wait_worker_closing(&db).await;
		let error = db
			.query("SELECT 1;".to_owned(), None)
			.await
			.expect_err("closing worker should reject new work");
		assert!(error.to_string().contains("sqlite worker is closing"));

		let _ = resume.send(());
		close_task
			.await
			.expect("close task should join")
			.expect("worker should close");
		for task in pending {
			let error = task
				.await
				.expect("queued insert task should join")
				.expect_err("queued insert should fail after close starts");
			assert!(error.to_string().contains("sqlite worker is closing"));
		}
	});
}

#[test]
fn worker_close_is_idempotent_across_clones() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = open_worker_handle(&runtime, &harness);
	let clone = db.clone();

	runtime.block_on(async {
		let (first, second) = tokio::join!(db.close(), clone.close());
		first.expect("first close should succeed");
		second.expect("second close should succeed");
	});
}

#[test]
fn worker_drop_without_close_records_unclean_close() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let metrics = Arc::new(WorkerTestMetrics::default());
	let db = open_worker_handle_with_metrics(
		&runtime,
		&harness,
		Some(metrics.clone() as Arc<dyn SqliteVfsMetrics>),
	);

	runtime.block_on(async {
		db.query("SELECT 1;".to_owned(), None)
			.await
			.expect("worker should accept work before drop");
		drop(db);
		wait_worker_unclean_close(&metrics).await;
	});
}

#[test]
fn worker_panic_marks_worker_dead() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let metrics = Arc::new(WorkerTestMetrics::default());
	let db = open_worker_handle_with_metrics(
		&runtime,
		&harness,
		Some(metrics.clone() as Arc<dyn SqliteVfsMetrics>),
	);

	runtime.block_on(async {
		db.query("SELECT 1;".to_owned(), None)
			.await
			.expect("worker should accept work before panic");
		db.panic_worker_for_test().await;

		let error = db
			.query("SELECT 1;".to_owned(), None)
			.await
			.expect_err("dead worker should reject work");
		assert!(error.to_string().contains("sqlite worker is closed"));
	});

	assert_eq!(metrics.crashes.load(Ordering::Acquire), 1);
}

#[test]
fn worker_records_basic_metrics() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let metrics = Arc::new(WorkerTestMetrics::default());
	let db = open_worker_handle_with_metrics(
		&runtime,
		&harness,
		Some(metrics.clone() as Arc<dyn SqliteVfsMetrics>),
	);

	runtime.block_on(async {
		db.query("SELECT 1;".to_owned(), None)
			.await
			.expect("query should succeed");
		db.close().await.expect("worker should close");
	});

	assert!(metrics.command_durations.load(Ordering::Acquire) >= 1);
	assert!(metrics.close_durations.load(Ordering::Acquire) >= 1);
	assert_eq!(metrics.command_errors.load(Ordering::Acquire), 0);
}

fn sqlite_query_i64(db: *mut sqlite3, sql: &str) -> std::result::Result<i64, String> {
	let c_sql = CString::new(sql).map_err(|err| err.to_string())?;
	let mut stmt = ptr::null_mut();
	let rc = unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut()) };
	if rc != SQLITE_OK {
		return Err(format!(
			"`{sql}` prepare failed with code {rc}: {}",
			sqlite_error_message(db)
		));
	}
	if stmt.is_null() {
		return Err(format!("`{sql}` returned no statement"));
	}

	let result = match unsafe { sqlite3_step(stmt) } {
		SQLITE_ROW => Ok(unsafe { sqlite3_column_int64(stmt, 0) }),
		step_rc => Err(format!(
			"`{sql}` step failed with code {step_rc}: {}",
			sqlite_error_message(db)
		)),
	};

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

fn sqlite_query_text(db: *mut sqlite3, sql: &str) -> std::result::Result<String, String> {
	let c_sql = CString::new(sql).map_err(|err| err.to_string())?;
	let mut stmt = ptr::null_mut();
	let rc = unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut()) };
	if rc != SQLITE_OK {
		return Err(format!(
			"`{sql}` prepare failed with code {rc}: {}",
			sqlite_error_message(db)
		));
	}
	if stmt.is_null() {
		return Err(format!("`{sql}` returned no statement"));
	}

	let result = match unsafe { sqlite3_step(stmt) } {
		SQLITE_ROW => {
			let text_ptr = unsafe { sqlite3_column_text(stmt, 0) };
			if text_ptr.is_null() {
				Ok(String::new())
			} else {
				Ok(unsafe { CStr::from_ptr(text_ptr.cast()) }
					.to_string_lossy()
					.into_owned())
			}
		}
		step_rc => Err(format!(
			"`{sql}` step failed with code {step_rc}: {}",
			sqlite_error_message(db)
		)),
	};

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

fn sqlite_file_control(db: *mut sqlite3, op: c_int) -> std::result::Result<c_int, String> {
	let main = CString::new("main").map_err(|err| err.to_string())?;
	let rc = unsafe { sqlite3_file_control(db, main.as_ptr(), op, ptr::null_mut()) };
	if rc != SQLITE_OK {
		return Err(format!(
			"sqlite3_file_control op {op} failed with code {rc}: {}",
			sqlite_error_message(db)
		));
	}

	Ok(rc)
}

fn direct_runtime() -> tokio::runtime::Runtime {
	Builder::new_multi_thread()
		.worker_threads(2)
		.enable_all()
		.build()
		.expect("runtime should build")
}

#[test]
fn predictor_prefers_stride_after_repeated_reads() {
	let mut predictor = PrefetchPredictor::default();
	for pgno in [5, 8, 11, 14] {
		predictor.record(pgno);
	}

	assert_eq!(predictor.multi_predict(14, 3, 30), vec![17, 20, 23]);
}

#[test]
fn classify_recognizes_btree_and_overflow_pages() {
	let page_size = DEFAULT_PAGE_SIZE;
	for type_byte in [0x02u8, 0x05, 0x0a, 0x0d] {
		let mut page = vec![0u8; page_size];
		page[0] = type_byte;
		assert_eq!(classify(2, &page), PageClass::Btree);
	}

	// An overflow page leads with a 4-byte next-page pointer, not a page type.
	let mut overflow = vec![0u8; page_size];
	overflow[0..4].copy_from_slice(&7u32.to_be_bytes());
	assert_eq!(classify(2, &overflow), PageClass::Overflow);

	// Page 1 carries the b-tree type byte after the 100-byte database header.
	let mut header_page = vec![0u8; page_size];
	header_page[100] = 0x0d;
	assert_eq!(classify(1, &header_page), PageClass::Btree);
}

#[test]
fn interleaved_overflow_reads_hide_leaf_forward_scan() {
	// A single read-ahead tracker fed the merged leaf/overflow access stream of
	// a scan over overflowing rows never sees a monotonic stride, so it never
	// escalates to forward-scan read-ahead. This documents the behavior the
	// per-class trackers fix.
	let config = VfsConfig::from_optimization_flags(SqliteOptimizationFlags::default());
	let mut tracker = AdaptiveReadAhead::default();
	let mut mode = ReadAheadMode::Bounded;
	for pgno in [2, 20, 3, 21, 4, 22, 5, 23, 6, 24] {
		mode = tracker.record_and_plan(&[pgno], &config).mode;
	}
	assert_eq!(mode, ReadAheadMode::Bounded);
}

#[test]
fn separated_leaf_reads_escalate_forward_scan() {
	// With overflow pages routed to a separate tracker, the leaf tracker sees a
	// clean monotonic stride and escalates to forward-scan read-ahead.
	let config = VfsConfig::from_optimization_flags(SqliteOptimizationFlags::default());
	let mut tracker = AdaptiveReadAhead::default();
	let mut mode = ReadAheadMode::Bounded;
	for pgno in [2, 3, 4, 5, 6] {
		mode = tracker.record_and_plan(&[pgno], &config).mode;
	}
	assert_eq!(mode, ReadAheadMode::ForwardScan);
}

#[test]
fn classified_predictor_keeps_per_class_strides_independent() {
	// Interleaving leaf and overflow accesses must not destroy either stream's
	// stride signal: each class predicts its own forward run.
	let mut predictor = ClassifiedPredictor::default();
	for (leaf, overflow) in [(2, 20), (3, 21), (4, 22), (5, 23)] {
		predictor.record(PageClass::Btree, leaf);
		predictor.record(PageClass::Overflow, overflow);
	}

	assert_eq!(
		predictor.multi_predict(PageClass::Btree, 5, 3, 100),
		vec![6, 7, 8]
	);
	assert_eq!(
		predictor.multi_predict(PageClass::Overflow, 23, 3, 100),
		vec![24, 25, 26]
	);
}

#[test]
fn direct_engine_open_engine_is_concurrency_safe() {
	let runtime = direct_runtime();
	let handle = runtime.handle().clone();
	let harness = Arc::new(DirectEngineHarness::new());
	let barrier = Arc::new(Barrier::new(8));

	thread::scope(|scope| {
		let mut workers = Vec::new();
		for _ in 0..8 {
			let handle = handle.clone();
			let harness = Arc::clone(&harness);
			let barrier = Arc::clone(&barrier);
			workers.push(scope.spawn(move || {
				barrier.wait();
				handle.block_on(harness.open_engine())
			}));
		}

		let first = workers
			.pop()
			.expect("at least one worker should exist")
			.join()
			.expect("worker should open engine");
		for worker in workers {
			let storage = worker.join().expect("worker should open engine");
			assert!(
				Arc::ptr_eq(&first, &storage),
				"all concurrent callers should share one direct storage",
			);
		}
	});
}

#[test]
fn initial_page_fetch_inside_block_in_place_can_use_runtime_handle() {
	let runtime = direct_runtime();
	runtime.block_on(async {
		let harness = Arc::new(DirectEngineHarness::new());
		let engine = harness.open_engine().await;
		let actor_id = harness.actor_id.clone();
		let handle = tokio::runtime::Handle::current();

		let initial_main_page = tokio::task::spawn(async move {
			tokio::task::block_in_place(|| {
				handle.block_on(fetch_initial_main_page_for_registration(
					Arc::new(DirectDepotTransport::new(engine)),
					&actor_id,
				))
			})
		})
		.await
		.expect("runtime worker task should finish")
		.expect("initial main page preload should succeed");

		assert!(initial_main_page.is_none());
	});
}

#[test]
fn direct_engine_supports_create_insert_select_and_user_version() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	assert_eq!(
		sqlite_file_control(db.as_ptr(), SQLITE_FCNTL_BEGIN_ATOMIC_WRITE)
			.expect("batch atomic begin should succeed"),
		SQLITE_OK
	);
	assert_eq!(
		sqlite_file_control(db.as_ptr(), SQLITE_FCNTL_COMMIT_ATOMIC_WRITE)
			.expect("batch atomic commit should succeed"),
		SQLITE_OK
	);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_step_statement(
		db.as_ptr(),
		"INSERT INTO items (id, value) VALUES (1, 'alpha');",
	)
	.expect("insert should succeed");
	sqlite_exec(db.as_ptr(), "PRAGMA user_version = 42;")
		.expect("user_version pragma should succeed");

	assert_eq!(
		sqlite_query_text(db.as_ptr(), "SELECT value FROM items WHERE id = 1;")
			.expect("select should succeed"),
		"alpha"
	);
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM items;").expect("count should succeed"),
		1
	);
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "PRAGMA user_version;")
			.expect("user_version read should succeed"),
		42
	);
}

#[test]
fn direct_engine_handles_large_rows_and_multi_page_growth() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE blobs (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create table should succeed");

	for _ in 0..48 {
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO blobs (payload) VALUES (randomblob(3500));",
		)
		.expect("seed insert should succeed");
	}
	sqlite_step_statement(
		db.as_ptr(),
		"INSERT INTO blobs (payload) VALUES (randomblob(9000));",
	)
	.expect("large row insert should succeed");

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM blobs;").expect("count should succeed"),
		49
	);
	assert!(
		sqlite_query_i64(db.as_ptr(), "PRAGMA page_count;").expect("page_count should succeed")
			> 20
	);
	assert!(
		sqlite_query_i64(db.as_ptr(), "SELECT max(length(payload)) FROM blobs;")
			.expect("max payload length should succeed")
			>= 9000
	);
}

#[test]
fn batch_atomic_commit_does_not_depend_on_aux_journal_open() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	ctx.fail_next_aux_open("InjectedAuxOpenError: journal open should not be needed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE batch_atomic_no_journal (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("batch atomic create should not need rollback journal open");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO batch_atomic_no_journal (id, value) VALUES (1, 'ok');",
	)
	.expect("batch atomic insert should not need rollback journal open");

	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity_check should succeed"),
		"ok"
	);
}

#[test]
fn batch_atomic_oversized_commit_fails_closed_and_reopens_clean() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let actor_id = harness.actor_id.clone();
	let transport = Arc::new(CappedAtomicCommitTransport::new(
		Arc::clone(&engine),
		256 * 1024,
	));
	let db = harness.open_db_with_transport(
		&runtime,
		transport.clone(),
		&actor_id,
		VfsConfig::default(),
	);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE atomic_cap (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO atomic_cap (id, payload) VALUES (1, zeroblob(1024));",
	)
	.expect("seed insert should succeed");
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA integrity_check;")
			.expect("pre-cap integrity_check should succeed"),
		"ok"
	);

	sqlite_exec(db.as_ptr(), "BEGIN IMMEDIATE;").expect("large transaction should begin");
	for id in 2..=130 {
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO atomic_cap (id, payload) VALUES ({id}, randomblob(4096));"),
		)
		.expect("large transaction insert should succeed");
	}
	let commit_err = sqlite_exec(db.as_ptr(), "COMMIT;").expect_err("oversized commit should fail");
	assert!(
		commit_err.contains("disk I/O error") || commit_err.contains("transaction_too_large"),
		"unexpected commit error: {commit_err}",
	);
	assert!(
		transport.rejected(),
		"test did not exceed the simulated atomic commit cap; max_seen_dirty_bytes={} max_seen_dirty_pages={}",
		transport.max_seen_dirty_bytes(),
		transport.max_seen_dirty_pages(),
	);
	drop(db);

	let reopened = harness.open_db_on_engine(&runtime, engine, &actor_id, VfsConfig::default());
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM atomic_cap;")
			.expect("count after capped commit should succeed"),
		1
	);
	assert_eq!(
		sqlite_query_text(reopened.as_ptr(), "PRAGMA quick_check;")
			.expect("quick_check after capped commit should succeed"),
		"ok"
	);
	assert_eq!(
		sqlite_query_text(reopened.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity_check after capped commit should succeed"),
		"ok"
	);
}

#[test]
fn volatile_rollback_journal_without_batch_atomic_can_make_failed_commit_durable() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(Arc::clone(&engine)));
	let hooks = transport.direct_hooks();
	let actor_id = harness.actor_id.clone();
	let mut config = VfsConfig::default();
	config.assert_batch_atomic = false;
	config.advertise_batch_atomic = false;
	let db = harness.open_db_with_transport(&runtime, transport, &actor_id, config);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE rollback_journal_loss (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO rollback_journal_loss (id, value) VALUES (1, 'before');",
	)
	.expect("seed insert should succeed");

	hooks.fail_next_commit_after_apply("InjectedTransportError: main db sync response lost");
	sqlite_exec(db.as_ptr(), "BEGIN IMMEDIATE;").expect("transaction should begin");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO rollback_journal_loss (id, value) VALUES (2, 'failed-commit');",
	)
	.expect("transaction insert should succeed");
	let commit_err =
		sqlite_exec(db.as_ptr(), "COMMIT;").expect_err("commit response loss should fail");
	assert!(
		commit_err.contains("disk I/O error"),
		"unexpected commit error: {commit_err}"
	);

	let leaked = ManuallyDrop::new(db);
	let _fatal_after_failed_commit = direct_vfs_ctx(&leaked).clone_fatal_error();

	let reopened = harness.open_db_on_engine(&runtime, engine, &actor_id, VfsConfig::default());
	assert_eq!(
		sqlite_query_text(reopened.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity_check should succeed after simulated crash"),
		"ok"
	);
	assert_eq!(
		sqlite_query_i64(
			reopened.as_ptr(),
			"SELECT COUNT(*) FROM rollback_journal_loss;"
		)
		.expect("count after simulated crash should succeed"),
		2,
		"without batch atomic, a lost main-db sync response can make a failed commit durable because the rollback journal was process-local",
	);
}

#[test]
fn direct_engine_persists_data_across_close_and_reopen() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();

	{
		let db = harness.open_db(&runtime);
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE events (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
		)
		.expect("create table should succeed");
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO events (id, value) VALUES (1, 'persisted');",
		)
		.expect("insert should succeed");
		sqlite_exec(db.as_ptr(), "PRAGMA user_version = 7;")
			.expect("user_version write should succeed");
	}

	let reopened = harness.open_db(&runtime);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM events;")
			.expect("count after reopen should succeed"),
		1
	);
	assert_eq!(
		sqlite_query_text(reopened.as_ptr(), "SELECT value FROM events WHERE id = 1;")
			.expect("value after reopen should succeed"),
		"persisted"
	);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "PRAGMA user_version;")
			.expect("user_version after reopen should succeed"),
		7
	);
}

#[test]
fn strict_direct_reopen_ignores_poisoned_mirror_and_reads_depot() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let page_count;

	{
		let db = harness.open_db(&runtime);
		sqlite_exec(db.as_ptr(), "PRAGMA user_version = 2718;")
			.expect("user_version write should succeed");
		page_count = sqlite_query_i64(db.as_ptr(), "PRAGMA page_count;")
			.expect("page count should succeed") as u32;
	}

	let engine = runtime.block_on(harness.open_engine());
	runtime.block_on(engine.poison_mirror_page(&harness.actor_id, 1, vec![0xdb; 4096], page_count));
	runtime.block_on(engine.evict_actor_db(&harness.actor_id));

	let before = engine.stats();
	let reopened = harness.open_db(&runtime);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "PRAGMA user_version;")
			.expect("strict reopen should read depot state"),
		2718
	);
	let after = engine.stats();
	assert!(after.depot_get_pages > before.depot_get_pages);
	assert_eq!(after.mirror_reads, before.mirror_reads);
	assert_eq!(after.mirror_seeds, before.mirror_seeds);
}

#[test]
fn direct_depot_transport_rejects_mirror_fallback() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());

	runtime
		.block_on(engine.apply_commit(
			&harness.actor_id,
			vec![storage_dirty_page(protocol::SqliteDirtyPage {
				pgno: 1,
				bytes: empty_db_page(),
			})],
			1,
		))
		.expect("non-strict mirror seed should succeed");

	runtime.block_on(engine.evict_actor_db(&harness.actor_id));
	let before = engine.stats();
	let err = runtime
		.block_on(engine.get_pages(&harness.actor_id, &[1]))
		.expect_err("depot transport should not read from the mirror");
	assert!(!err.to_string().is_empty());
	let after = engine.stats();
	assert_eq!(after.mirror_reads, before.mirror_reads);
}

#[test]
fn direct_mirror_transport_reopens_from_mirror_pages() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectMirrorTransport::new(engine.clone()));
	let hooks = transport.direct_hooks();

	{
		let vfs = SqliteVfs::register_with_transport(
			&next_test_name("sqlite-mirror-vfs"),
			transport.clone(),
			harness.actor_id.clone(),
			runtime.handle().clone(),
			VfsConfig::default(),
			None,
		)
		.expect("mirror vfs should register");
		let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
		sqlite_exec(db.as_ptr(), "PRAGMA user_version = 314;")
			.expect("user_version write should succeed");
	}

	let initial_main_page = runtime
		.block_on(fetch_initial_main_page_for_registration(
			transport.clone(),
			&harness.actor_id,
		))
		.expect("mirror preload should succeed");
	let vfs = SqliteVfs::register_with_transport_and_initial_page(
		&next_test_name("sqlite-mirror-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		initial_main_page,
		None,
	)
	.expect("mirror vfs should register");
	let reopened = open_database(vfs, &harness.actor_id).expect("sqlite database should reopen");
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "PRAGMA user_version;")
			.expect("mirror user_version read should succeed"),
		314
	);
	assert!(
		!hooks.commit_requests().is_empty(),
		"mirror transport should record commit requests",
	);
	let stats = engine.stats();
	assert_eq!(stats.depot_get_pages, 0);
	assert!(stats.mirror_reads > 0);
	assert!(stats.mirror_seeds > 0);
}

#[test]
fn direct_engine_handles_aux_files_and_truncate_then_regrow() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(db.as_ptr(), "PRAGMA temp_store = FILE;")
		.expect("temp_store pragma should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE blobs (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create table should succeed");

	for _ in 0..32 {
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO blobs (payload) VALUES (randomblob(8192));",
		)
		.expect("growth insert should succeed");
	}
	let grown_pages = sqlite_query_i64(db.as_ptr(), "PRAGMA page_count;")
		.expect("grown page_count should succeed");
	assert!(grown_pages > 40);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TEMP TABLE scratch AS SELECT id FROM blobs ORDER BY id DESC;",
	)
	.expect("temp table should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM scratch;")
			.expect("temp table count should succeed"),
		32
	);

	sqlite_exec(db.as_ptr(), "DELETE FROM blobs;").expect("delete should succeed");
	sqlite_exec(db.as_ptr(), "VACUUM;").expect("vacuum should succeed");
	let shrunk_pages = sqlite_query_i64(db.as_ptr(), "PRAGMA page_count;")
		.expect("shrunk page_count should succeed");
	assert!(shrunk_pages < grown_pages);

	for _ in 0..8 {
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO blobs (payload) VALUES (randomblob(8192));",
		)
		.expect("regrow insert should succeed");
	}
	let regrown_pages = sqlite_query_i64(db.as_ptr(), "PRAGMA page_count;")
		.expect("regrown page_count should succeed");
	assert!(regrown_pages > shrunk_pages);
}

#[test]
fn direct_engine_accepts_actual_nul_text_when_bound_with_explicit_length() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE nul_texts (payload TEXT PRIMARY KEY, marker INTEGER NOT NULL);",
	)
	.expect("create table should succeed");

	let payload = b"actual\0nul-text";
	sqlite_insert_text_with_int_value(
		db.as_ptr(),
		"INSERT INTO nul_texts (payload, marker) VALUES (?, ?);",
		payload,
		7,
	)
	.expect("explicit-length text bind should preserve embedded nul");

	assert_eq!(
		sqlite_query_i64_bind_text(
			db.as_ptr(),
			"SELECT marker FROM nul_texts WHERE payload = ?;",
			payload,
		)
		.expect("lookup by embedded-nul text should succeed"),
		7
	);
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "SELECT hex(payload) FROM nul_texts;")
			.expect("hex query should succeed"),
		"61637475616C006E756C2D74657874"
	);
}

#[test]
fn direct_engine_handles_boundary_primary_keys() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE boundary_keys (key_text TEXT PRIMARY KEY, value INTEGER NOT NULL);",
	)
	.expect("create table should succeed");

	let mut keys = vec![
		Vec::from("".as_bytes()),
		Vec::from(" ".as_bytes()),
		Vec::from("slash/key".as_bytes()),
		Vec::from("comma,key".as_bytes()),
		Vec::from("percent%key".as_bytes()),
		Vec::from("CaseKey".as_bytes()),
		Vec::from("casekey".as_bytes()),
		vec![b'k'; 2048],
	];
	for i in 0..256 {
		keys.push(format!("seq-{i:04}").into_bytes());
	}

	for (index, key) in keys.iter().enumerate() {
		sqlite_insert_text_with_int_value(
			db.as_ptr(),
			"INSERT INTO boundary_keys (key_text, value) VALUES (?, ?);",
			key,
			index as i64,
		)
		.expect("boundary key insert should succeed");
	}

	for (index, key) in keys.iter().enumerate() {
		assert_eq!(
			sqlite_query_i64_bind_text(
				db.as_ptr(),
				"SELECT value FROM boundary_keys WHERE key_text = ?;",
				key,
			)
			.expect("boundary key lookup should succeed"),
			index as i64
		);
	}

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM boundary_keys;")
			.expect("count should succeed"),
		keys.len() as i64
	);
}

#[test]
fn direct_engine_keeps_shadow_checksum_transaction_consistent() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY, value INTEGER NOT NULL);",
	)
	.expect("create items should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE shadow_checksums (name TEXT PRIMARY KEY, total INTEGER NOT NULL);",
	)
	.expect("create shadow table should succeed");

	let expected_total = (1..=128).sum::<i64>();
	sqlite_exec(db.as_ptr(), "BEGIN").expect("begin should succeed");
	for i in 1..=128 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!("INSERT INTO items (id, value) VALUES ({i}, {i});"),
		)
		.expect("item insert should succeed");
	}
	sqlite_step_statement(
		db.as_ptr(),
		&format!("INSERT INTO shadow_checksums (name, total) VALUES ('items', {expected_total});"),
	)
	.expect("shadow insert should succeed");
	sqlite_exec(db.as_ptr(), "COMMIT").expect("commit should succeed");

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT SUM(value) FROM items;")
			.expect("item sum should succeed"),
		expected_total
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT total FROM shadow_checksums WHERE name = 'items';",
		)
		.expect("shadow total should succeed"),
		expected_total
	);
}

#[test]
fn direct_engine_mixed_row_model_preserves_invariants() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (item_key TEXT PRIMARY KEY, value TEXT NOT NULL, version INTEGER NOT NULL);",
	)
	.expect("create table should succeed");

	for i in 0..64 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO items (item_key, value, version) VALUES ('item-{i:02}', 'insert-{i}', 1);"
			),
		)
		.expect("seed insert should succeed");
	}

	for i in 0..32 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"UPDATE items SET value = 'update-{i}', version = version + 1 WHERE item_key = 'item-{i:02}';"
			),
		)
		.expect("update should succeed");
	}

	for i in 16..24 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!("DELETE FROM items WHERE item_key = 'item-{i:02}';"),
		)
		.expect("delete should succeed");
	}

	for i in 0..16 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
					"INSERT INTO items (item_key, value, version) VALUES ('item-{i:02}', 'upsert-{i}', 99)
					ON CONFLICT(item_key) DO UPDATE SET value = excluded.value, version = excluded.version;"
				),
		)
		.expect("upsert should succeed");
	}

	for i in 0..1000 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"UPDATE items SET version = version + 1, value = 'hot-{i}' WHERE item_key = 'item-00';"
			),
		)
		.expect("hot-row update should succeed");
	}

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM items;").expect("count should succeed"),
		56
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT version FROM items WHERE item_key = 'item-00';"
		)
		.expect("hot-row version should succeed"),
		1099
	);
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA quick_check;").expect("quick_check should succeed"),
		"ok"
	);
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity_check should succeed"),
		"ok"
	);
}

#[test]
fn direct_engine_runs_deterministic_nasty_script() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE nasty_edge (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create nasty_edge should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE nasty_counter (id INTEGER PRIMARY KEY, value INTEGER NOT NULL);",
	)
	.expect("create nasty_counter should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE nasty_rows (n INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create nasty_rows should succeed");

	for i in 0..256 {
		let size = 1 + ((131072 - 1) * i / 255);
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT OR REPLACE INTO nasty_edge (id, payload) VALUES (1, randomblob({size}));"
			),
		)
		.expect("grow-row write should succeed");
	}
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT length(payload) FROM nasty_edge WHERE id = 1;"
		)
		.expect("grown row length should succeed"),
		131072
	);

	sqlite_step_statement(
		db.as_ptr(),
		"INSERT INTO nasty_counter (id, value) VALUES (1, 0);",
	)
	.expect("seed counter should succeed");
	sqlite_exec(db.as_ptr(), "BEGIN").expect("counter begin should succeed");
	for _ in 0..10_000 {
		sqlite_step_statement(
			db.as_ptr(),
			"UPDATE nasty_counter SET value = value + 1 WHERE id = 1;",
		)
		.expect("counter update should succeed");
	}
	sqlite_exec(db.as_ptr(), "COMMIT").expect("counter commit should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT value FROM nasty_counter WHERE id = 1;")
			.expect("counter read should succeed"),
		10_000
	);

	sqlite_exec(
		db.as_ptr(),
		"CREATE INDEX idx_nasty_rows_payload ON nasty_rows(payload);",
	)
	.expect("create index should succeed");
	sqlite_exec(db.as_ptr(), "BEGIN").expect("bulk begin should succeed");
	for i in 0..10_000 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!("INSERT INTO nasty_rows (n, payload) VALUES ({i}, randomblob(64));"),
		)
		.expect("bulk insert should succeed");
	}
	sqlite_exec(db.as_ptr(), "DELETE FROM nasty_rows WHERE n % 2 = 0;")
		.expect("bulk delete should succeed");
	sqlite_exec(db.as_ptr(), "COMMIT").expect("bulk commit should succeed");
	sqlite_exec(db.as_ptr(), "DROP INDEX idx_nasty_rows_payload;")
		.expect("drop index should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM nasty_rows;")
			.expect("remaining row count should succeed"),
		5000
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_nasty_rows_payload';",
		)
		.expect("index absence should succeed"),
		0
	);

	sqlite_exec(db.as_ptr(), "BEGIN").expect("rollback begin should succeed");
	for i in 0..1000 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO nasty_rows (n, payload) VALUES ({}, randomblob(8));",
				20_000 + i
			),
		)
		.expect("rollback insert should succeed");
	}
	sqlite_exec(db.as_ptr(), "ROLLBACK").expect("rollback should succeed");
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM nasty_rows WHERE n >= 20000;"
		)
		.expect("rollback count should succeed"),
		0
	);
}

#[test]
fn direct_engine_handles_page_boundary_payloads_and_text_roundtrip() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE payload_matrix (
				id INTEGER PRIMARY KEY,
				blob_payload BLOB NOT NULL,
				unicode_text TEXT NOT NULL,
				escaped_text TEXT NOT NULL
			);",
	)
	.expect("create table should succeed");

	let sizes = [
		1, 4095, 4096, 4097, 8191, 8192, 8193, 32768, 65535, 65536, 98304, 131072,
	];
	for (index, size) in sizes.iter().enumerate() {
		let id = index + 1;
		let unicode_text = format!("snowman-{id}-こんにちは-ß-🧪");
		let escaped_text = format!("escaped\\\\0-row-{id}");
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO payload_matrix (id, blob_payload, unicode_text, escaped_text)
					VALUES ({id}, zeroblob({size}), '{}', '{}');",
				unicode_text.replace('\'', "''"),
				escaped_text.replace('\'', "''"),
			),
		)
		.expect("boundary payload insert should succeed");
	}

	for (index, size) in sizes.iter().enumerate() {
		let id = index + 1;
		assert_eq!(
			sqlite_query_i64(
				db.as_ptr(),
				&format!("SELECT length(blob_payload) FROM payload_matrix WHERE id = {id};"),
			)
			.expect("blob length query should succeed"),
			*size as i64
		);
	}

	assert_eq!(
		sqlite_query_text(
			db.as_ptr(),
			"SELECT unicode_text FROM payload_matrix WHERE id = 4;",
		)
		.expect("unicode text should roundtrip"),
		"snowman-4-こんにちは-ß-🧪"
	);
	assert_eq!(
		sqlite_query_text(
			db.as_ptr(),
			"SELECT escaped_text FROM payload_matrix WHERE id = 4;",
		)
		.expect("escaped nul text should roundtrip"),
		"escaped\\\\0-row-4"
	);
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA quick_check;").expect("quick_check should succeed"),
		"ok"
	);
}

#[test]
fn direct_engine_enforces_constraints_savepoints_and_relational_invariants() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(db.as_ptr(), "PRAGMA foreign_keys = ON;")
		.expect("foreign_keys pragma should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE users (
				id INTEGER PRIMARY KEY,
				email TEXT NOT NULL UNIQUE,
				name TEXT NOT NULL
			);",
	)
	.expect("create users should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE inventory (
				sku TEXT PRIMARY KEY,
				stock INTEGER NOT NULL CHECK(stock >= 0)
			);",
	)
	.expect("create inventory should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE orders (
				id INTEGER PRIMARY KEY,
				user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
				total_cents INTEGER NOT NULL CHECK(total_cents >= 0),
				status TEXT NOT NULL
			);",
	)
	.expect("create orders should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE order_items (
				order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
				sku TEXT NOT NULL REFERENCES inventory(sku),
				qty INTEGER NOT NULL CHECK(qty > 0),
				price_cents INTEGER NOT NULL CHECK(price_cents >= 0),
				PRIMARY KEY(order_id, sku)
			) WITHOUT ROWID;",
	)
	.expect("create order_items should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE payments (
				id INTEGER PRIMARY KEY,
				order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
				amount_cents INTEGER NOT NULL CHECK(amount_cents >= 0),
				status TEXT NOT NULL
			);",
	)
	.expect("create payments should succeed");

	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO users (id, email, name) VALUES (1, 'alice@example.com', 'Alice');",
	)
	.expect("seed user should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO inventory (sku, stock) VALUES ('sku-red', 10), ('sku-blue', 8);",
	)
	.expect("seed inventory should succeed");

	assert!(
		sqlite_exec(
			db.as_ptr(),
			"INSERT INTO users (id, email, name) VALUES (2, 'alice@example.com', 'Again');",
		)
		.expect_err("unique violation should fail")
		.contains("UNIQUE constraint failed")
	);
	assert!(
		sqlite_exec(
			db.as_ptr(),
			"INSERT INTO users (id, email, name) VALUES (3, 'null@example.com', NULL);",
		)
		.expect_err("not-null violation should fail")
		.contains("NOT NULL constraint failed")
	);
	assert!(
		sqlite_exec(
			db.as_ptr(),
			"UPDATE inventory SET stock = -1 WHERE sku = 'sku-red';"
		)
		.expect_err("check violation should fail")
		.contains("CHECK constraint failed")
	);
	assert!(
		sqlite_exec(
			db.as_ptr(),
			"INSERT INTO orders (id, user_id, total_cents, status) VALUES (9, 999, 100, 'pending');",
		)
		.expect_err("foreign-key violation should fail")
		.contains("FOREIGN KEY constraint failed")
	);

	sqlite_exec(db.as_ptr(), "BEGIN").expect("begin should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO orders (id, user_id, total_cents, status) VALUES (1, 1, 700, 'paid');",
	)
	.expect("order insert should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO order_items (order_id, sku, qty, price_cents)
			VALUES (1, 'sku-red', 2, 150), (1, 'sku-blue', 1, 400);",
	)
	.expect("order items insert should succeed");
	sqlite_exec(
		db.as_ptr(),
		"UPDATE inventory
			SET stock = stock - CASE sku
				WHEN 'sku-red' THEN 2
				WHEN 'sku-blue' THEN 1
				ELSE 0
			END
			WHERE sku IN ('sku-red', 'sku-blue');",
	)
	.expect("inventory update should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO payments (id, order_id, amount_cents, status) VALUES (1, 1, 700, 'captured');",
	)
	.expect("payment insert should succeed");
	sqlite_exec(db.as_ptr(), "COMMIT").expect("commit should succeed");

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT total_cents FROM orders WHERE id = 1;",)
			.expect("order total should succeed"),
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT SUM(qty * price_cents) FROM order_items WHERE order_id = 1;",
		)
		.expect("item sum should succeed")
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT SUM(amount_cents) FROM payments WHERE order_id = 1 AND status = 'captured';",
		)
		.expect("captured payment sum should succeed"),
		700
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT SUM(stock) FROM inventory WHERE sku IN ('sku-red', 'sku-blue');",
		)
		.expect("inventory sum should succeed"),
		15
	);

	sqlite_exec(db.as_ptr(), "SAVEPOINT sp_order_rollback;").expect("savepoint should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO orders (id, user_id, total_cents, status) VALUES (2, 1, 123, 'draft');",
	)
	.expect("draft order insert should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO order_items (order_id, sku, qty, price_cents) VALUES (2, 'sku-red', 1, 123);",
	)
	.expect("draft item insert should succeed");
	sqlite_exec(db.as_ptr(), "ROLLBACK TO sp_order_rollback;")
		.expect("rollback to savepoint should succeed");
	sqlite_exec(db.as_ptr(), "RELEASE sp_order_rollback;")
		.expect("release savepoint should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM orders WHERE id = 2;")
			.expect("rolled-back order should be absent"),
		0
	);

	sqlite_exec(db.as_ptr(), "SAVEPOINT sp_order_release;")
		.expect("release savepoint should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO orders (id, user_id, total_cents, status) VALUES (3, 1, 50, 'pending');",
	)
	.expect("released order insert should succeed");
	sqlite_exec(db.as_ptr(), "RELEASE sp_order_release;")
		.expect("release savepoint should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM orders WHERE id = 3;")
			.expect("released order should exist"),
		1
	);

	sqlite_exec(db.as_ptr(), "BEGIN").expect("rollback begin should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO payments (id, order_id, amount_cents, status) VALUES (2, 3, 50, 'captured');",
	)
	.expect("rollback payment insert should succeed");
	sqlite_exec(db.as_ptr(), "ROLLBACK").expect("rollback should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM payments WHERE id = 2;")
			.expect("rolled-back payment should be absent"),
		0
	);

	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO users (id, email, name) VALUES (7, 'idempotent@example.com', 'Replay')
			ON CONFLICT(id) DO NOTHING;",
	)
	.expect("idempotent insert should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO users (id, email, name) VALUES (7, 'idempotent@example.com', 'Replay')
			ON CONFLICT(id) DO NOTHING;",
	)
	.expect("idempotent replay should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM users WHERE id = 7;")
			.expect("idempotent user count should succeed"),
		1
	);

	sqlite_exec(db.as_ptr(), "DELETE FROM orders WHERE id = 1;")
		.expect("cascade delete should succeed");
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM order_items WHERE order_id = 1;"
		)
		.expect("cascaded order items should be removed"),
		0
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM payments WHERE order_id = 1;"
		)
		.expect("cascaded payments should be removed"),
		0
	);
}

#[test]
fn direct_engine_handles_schema_churn_index_parity_and_pragmas() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA journal_mode = DELETE;")
			.expect("journal_mode pragma should succeed"),
		"delete"
	);
	sqlite_exec(db.as_ptr(), "PRAGMA synchronous = NORMAL;")
		.expect("synchronous pragma should succeed");
	sqlite_exec(db.as_ptr(), "PRAGMA cache_size = -2000;")
		.expect("cache_size pragma should succeed");
	sqlite_exec(db.as_ptr(), "PRAGMA foreign_keys = ON;")
		.expect("foreign_keys pragma should succeed");
	sqlite_exec(db.as_ptr(), "PRAGMA auto_vacuum = NONE;")
		.expect("auto_vacuum pragma should succeed");
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "PRAGMA cache_size;")
			.expect("cache_size read should succeed"),
		-2000
	);
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "PRAGMA foreign_keys;")
			.expect("foreign_keys read should succeed"),
		1
	);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (
				id INTEGER PRIMARY KEY,
				bucket INTEGER NOT NULL,
				key_text TEXT NOT NULL,
				value INTEGER NOT NULL
			);",
	)
	.expect("create items should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE item_ops (
				seq INTEGER PRIMARY KEY AUTOINCREMENT,
				kind TEXT NOT NULL,
				item_id INTEGER NOT NULL
			);",
	)
	.expect("create item_ops should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE ledger (
				account_id TEXT NOT NULL,
				entry_id INTEGER NOT NULL,
				amount INTEGER NOT NULL,
				PRIMARY KEY(account_id, entry_id)
			) WITHOUT ROWID;",
	)
	.expect("create ledger should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE VIEW active_items AS
			SELECT id, bucket, key_text, value FROM items WHERE value >= 0;",
	)
	.expect("create view should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TRIGGER items_ai AFTER INSERT ON items
			BEGIN
				INSERT INTO item_ops (kind, item_id) VALUES ('insert', NEW.id);
			END;",
	)
	.expect("create insert trigger should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TRIGGER items_ad AFTER DELETE ON items
			BEGIN
				INSERT INTO item_ops (kind, item_id) VALUES ('delete', OLD.id);
			END;",
	)
	.expect("create delete trigger should succeed");
	sqlite_exec(
		db.as_ptr(),
		"ALTER TABLE items ADD COLUMN tag TEXT NOT NULL DEFAULT 'base';",
	)
	.expect("alter table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE INDEX idx_items_bucket_key ON items(bucket, key_text);",
	)
	.expect("create compound index should succeed");

	sqlite_exec(db.as_ptr(), "BEGIN").expect("begin should succeed");
	for i in 0..128 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO items (id, bucket, key_text, value, tag)
					VALUES ({i}, {}, 'k-{i:03}', {}, 'tag-{}');",
				i % 8,
				i * 2,
				i % 5,
			),
		)
		.expect("items insert should succeed");
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO ledger (account_id, entry_id, amount) VALUES ('acct-{}', {i}, {});",
				i % 4,
				(i as i64) - 32,
			),
		)
		.expect("ledger insert should succeed");
	}
	sqlite_exec(db.as_ptr(), "COMMIT").expect("commit should succeed");
	sqlite_exec(db.as_ptr(), "DELETE FROM items WHERE id % 9 = 0;").expect("delete should succeed");

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM active_items;")
			.expect("view query should succeed"),
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM items WHERE value >= 0;")
			.expect("table query should succeed")
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT
					SUM(CASE WHEN kind = 'insert' THEN 1 ELSE 0 END) -
					SUM(CASE WHEN kind = 'delete' THEN 1 ELSE 0 END)
				FROM item_ops;",
		)
		.expect("op log delta should succeed"),
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM items;")
			.expect("live item count should succeed")
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM items WHERE bucket = 3 AND key_text >= 'k-040';",
		)
		.expect("indexed scan should succeed"),
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM items NOT INDEXED WHERE bucket = 3 AND key_text >= 'k-040';",
		)
		.expect("not-indexed scan should succeed")
	);
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM ledger WHERE account_id = 'acct-2';"
		)
		.expect("without-rowid query should succeed"),
		32
	);

	sqlite_exec(db.as_ptr(), "DROP INDEX idx_items_bucket_key;")
		.expect("drop index should succeed");
	assert_eq!(
		sqlite_query_i64(
			db.as_ptr(),
			"SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_items_bucket_key';",
		)
		.expect("dropped index should be absent"),
		0
	);
}

#[test]
fn direct_engine_handles_prepared_statement_churn() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE prepared_items (
				id INTEGER PRIMARY KEY,
				value TEXT NOT NULL,
				counter INTEGER NOT NULL
			);",
	)
	.expect("create table should succeed");

	for i in 1..=128 {
		let sql = format!(
			"INSERT INTO prepared_items (id, value, counter) VALUES ({i}, 'seed-{i}', 0); -- unique-sql-{i}"
		);
		let stmt = sqlite_prepare_statement(db.as_ptr(), &sql)
			.expect("unique prepared statement should prepare");
		sqlite_step_prepared(db.as_ptr(), stmt, &sql)
			.expect("unique prepared statement should execute");
		unsafe {
			sqlite3_finalize(stmt);
		}
	}

	let update_sql = "UPDATE prepared_items SET counter = counter + ?, value = ? WHERE id = ?;";
	let stmt = sqlite_prepare_statement(db.as_ptr(), update_sql)
		.expect("reused prepared statement should prepare");
	for i in 0..4000 {
		sqlite_reset_prepared(stmt, update_sql).expect("statement reset should succeed");
		sqlite_clear_bindings(stmt, update_sql).expect("binding clear should succeed");
		sqlite_bind_i64(db.as_ptr(), stmt, 1, 1, update_sql)
			.expect("increment bind should succeed");
		let value = format!("value-{i}");
		sqlite_bind_text_bytes(db.as_ptr(), stmt, 2, value.as_bytes(), update_sql)
			.expect("text bind should succeed");
		sqlite_bind_i64(db.as_ptr(), stmt, 3, (i % 128 + 1) as i64, update_sql)
			.expect("id bind should succeed");
		sqlite_step_prepared(db.as_ptr(), stmt, update_sql)
			.expect("reused prepared statement should execute");
	}
	unsafe {
		sqlite3_finalize(stmt);
	}

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM prepared_items;")
			.expect("row count should succeed"),
		128
	);
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT SUM(counter) FROM prepared_items;")
			.expect("counter sum should succeed"),
		4000
	);
	assert_eq!(
		sqlite_query_text(
			db.as_ptr(),
			"SELECT value FROM prepared_items WHERE id = 1;",
		)
		.expect("final prepared value should succeed"),
		"value-3968"
	);
}

#[test]
fn direct_engine_preserves_transaction_balance_and_fragmentation_invariants() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE accounts (
				id INTEGER PRIMARY KEY,
				balance INTEGER NOT NULL
			);",
	)
	.expect("create accounts should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE transfer_log (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				from_id INTEGER NOT NULL,
				to_id INTEGER NOT NULL,
				amount INTEGER NOT NULL
			);",
	)
	.expect("create transfer_log should succeed");
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE frag (
				id INTEGER PRIMARY KEY,
				payload BLOB NOT NULL
			);",
	)
	.expect("create frag should succeed");

	for id in 1..=8 {
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO accounts (id, balance) VALUES ({id}, 1000);"),
		)
		.expect("seed account should succeed");
	}

	sqlite_exec(db.as_ptr(), "BEGIN").expect("transfer begin should succeed");
	for step in 0..500 {
		let from_id = step % 8 + 1;
		let to_id = (step * 5 + 3) % 8 + 1;
		let amount = step % 17 + 1;
		sqlite_exec(
			db.as_ptr(),
			&format!("UPDATE accounts SET balance = balance - {amount} WHERE id = {from_id};"),
		)
		.expect("debit should succeed");
		sqlite_exec(
			db.as_ptr(),
			&format!("UPDATE accounts SET balance = balance + {amount} WHERE id = {to_id};"),
		)
		.expect("credit should succeed");
		sqlite_exec(
			db.as_ptr(),
			&format!(
				"INSERT INTO transfer_log (from_id, to_id, amount) VALUES ({from_id}, {to_id}, {amount});"
			),
		)
		.expect("transfer log insert should succeed");
	}
	sqlite_exec(db.as_ptr(), "COMMIT").expect("transfer commit should succeed");

	sqlite_exec(db.as_ptr(), "BEGIN").expect("rollback begin should succeed");
	sqlite_exec(
		db.as_ptr(),
		"UPDATE accounts SET balance = balance - 777 WHERE id = 1;",
	)
	.expect("rollback debit should succeed");
	sqlite_exec(
		db.as_ptr(),
		"UPDATE accounts SET balance = balance + 777 WHERE id = 2;",
	)
	.expect("rollback credit should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO transfer_log (from_id, to_id, amount) VALUES (1, 2, 777);",
	)
	.expect("rollback transfer log insert should succeed");
	sqlite_exec(db.as_ptr(), "ROLLBACK").expect("rollback should succeed");

	for id in 0..512 {
		let size = ((id * 541) % 16384) + 1;
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO frag (id, payload) VALUES ({id}, randomblob({size}));"),
		)
		.expect("fragmentation insert should succeed");
	}
	for id in (0..512).filter(|id| (id * 17 + 11) % 5 == 0) {
		sqlite_exec(db.as_ptr(), &format!("DELETE FROM frag WHERE id = {id};"))
			.expect("fragmentation delete should succeed");
	}
	for id in (0..512).filter(|id| id % 3 == 0) {
		let shrink_size = ((id * 13) % 256) + 1;
		sqlite_exec(
			db.as_ptr(),
			&format!("UPDATE frag SET payload = randomblob({shrink_size}) WHERE id = {id};"),
		)
		.expect("fragmentation shrink should succeed");
	}
	for id in (0..512).filter(|id| id % 7 == 0) {
		let grow_size = 16384 + ((id * 97) % 8192);
		sqlite_exec(
			db.as_ptr(),
			&format!("UPDATE frag SET payload = randomblob({grow_size}) WHERE id = {id};"),
		)
		.expect("fragmentation grow should succeed");
	}
	sqlite_exec(db.as_ptr(), "VACUUM;").expect("vacuum should succeed");

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT SUM(balance) FROM accounts;")
			.expect("balance sum should succeed"),
		8000
	);
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM transfer_log;")
			.expect("transfer log count should succeed"),
		500
	);
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM frag;")
			.expect("frag count should succeed"),
		410
	);
	assert!(
		sqlite_query_i64(db.as_ptr(), "SELECT SUM(length(payload)) FROM frag;")
			.expect("frag payload sum should succeed")
			> 0
	);
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity_check should succeed"),
		"ok"
	);
}

#[test]
fn direct_engine_batch_atomic_probe_runs_on_open() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	assert!(
		db._vfs.commit_atomic_count() > 0,
		"open_database should run the sqlite batch-atomic probe",
	);
}

#[test]
fn direct_engine_marks_vfs_dead_after_transport_errors() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	{
		let mut state = ctx.state.write();
		state.write_buffer.dirty.insert(1, empty_db_page());
		state.db_size_pages = 1;
	}

	hooks.fail_next_commit("InjectedTransportError: commit transport dropped");
	let err = ctx
		.flush_dirty_pages()
		.expect_err("failing transport commit should surface as a VFS error");
	assert!(
		matches!(err, CommitBufferError::Other(ref message) if message.contains("InjectedTransportError")),
		"VFS should surface transport failure as an IO error: {err:?}",
	);
	assert!(
		!ctx.is_dead(),
		"transport error should not poison the v2 VFS"
	);
	assert!(
		ctx.clone_fatal_error().is_none(),
		"transport error should not be stored as fatal"
	);
	assert_eq!(
		db.take_last_kv_error().as_deref(),
		Some("InjectedTransportError: commit transport dropped"),
	);
	ctx.flush_dirty_pages()
		.expect("retry after unapplied transport failure should succeed");
}

#[test]
fn flush_dirty_pages_marks_vfs_dead_after_transport_error() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	{
		let mut state = ctx.state.write();
		state.write_buffer.dirty.insert(1, vec![0x7a; 4096]);
		state.db_size_pages = 1;
	}

	hooks.fail_next_commit("InjectedTransportError: flush transport dropped");
	let err = ctx
		.flush_dirty_pages()
		.expect_err("transport failure should bubble out of flush_dirty_pages");

	assert!(
		matches!(err, CommitBufferError::Other(ref message) if message.contains("InjectedTransportError")),
		"flush failure should surface as a transport error: {err:?}",
	);
	assert!(
		!ctx.is_dead(),
		"flush transport failure should not poison the VFS"
	);
	assert!(
		ctx.clone_fatal_error().is_none(),
		"flush transport failure should not be stored as fatal"
	);
	assert_eq!(
		db.take_last_kv_error().as_deref(),
		Some("InjectedTransportError: flush transport dropped"),
	);
	ctx.flush_dirty_pages()
		.expect("retry after unapplied transport failure should succeed");
	assert!(
		ctx.state.read().write_buffer.dirty.is_empty(),
		"successful retry should clear dirty pages",
	);
}

#[test]
fn commit_atomic_write_marks_vfs_dead_after_transport_error() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	{
		let mut state = ctx.state.write();
		state.write_buffer.in_atomic_write = true;
		state.write_buffer.saved_db_size = state.db_size_pages;
		state.write_buffer.dirty.insert(1, vec![0x5c; 4096]);
		state.db_size_pages = 1;
	}

	hooks.fail_next_commit("InjectedTransportError: atomic transport dropped");
	let err = ctx
		.commit_atomic_write()
		.expect_err("transport failure should bubble out of commit_atomic_write");

	assert!(
		matches!(err, CommitBufferError::Other(ref message) if message.contains("InjectedTransportError")),
		"atomic-write failure should surface as a transport error: {err:?}",
	);
	assert!(
		!ctx.is_dead(),
		"commit_atomic_write transport failure should not poison the VFS",
	);
	assert!(
		ctx.clone_fatal_error().is_none(),
		"commit_atomic_write transport failure should not be stored as fatal",
	);
	assert_eq!(
		db.take_last_kv_error().as_deref(),
		Some("InjectedTransportError: atomic transport dropped"),
	);
	ctx.commit_atomic_write()
		.expect("retry after unapplied atomic transport failure should succeed");
	assert!(
		!ctx.state.read().write_buffer.in_atomic_write,
		"successful retry should leave atomic-write mode",
	);
}

#[test]
fn lost_commit_response_fails_later_on_head_fence_mismatch() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	{
		let mut state = ctx.state.write();
		state.write_buffer.dirty.insert(1, empty_db_page());
		state.db_size_pages = 1;
	}

	hooks.fail_next_commit_after_apply("InjectedTransportError: commit response lost");
	let err = ctx
		.flush_dirty_pages()
		.expect_err("lost commit response should surface as a transport error");
	assert!(
		matches!(err, CommitBufferError::Other(ref message) if message.contains("commit response lost")),
		"lost response should be ambiguous before the next fence check: {err:?}",
	);
	assert!(
		!ctx.is_dead(),
		"ambiguous lost response should not immediately poison the VFS"
	);
	assert!(ctx.clone_fatal_error().is_none());

	let err = ctx
		.flush_dirty_pages()
		.expect_err("retry after applied lost response should hit the stale head fence");
	assert!(
		matches!(err, CommitBufferError::FenceMismatch(ref message) if message.contains("head fence mismatch")),
		"retry should confirm the stale fence: {err:?}",
	);
	assert!(ctx.is_dead());
	assert!(
		ctx.clone_fatal_error()
			.is_some_and(|message| message.contains("head fence mismatch")),
		"confirmed fence mismatch should be stored as fatal"
	);
}

#[test]
fn unapplied_commit_transport_failure_can_retry_from_same_head() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	{
		let mut state = ctx.state.write();
		state.write_buffer.dirty.insert(1, empty_db_page());
		state.db_size_pages = 1;
	}

	hooks.fail_next_commit("InjectedTransportError: commit dropped before apply");
	let err = ctx
		.flush_dirty_pages()
		.expect_err("pre-apply transport error should surface");
	assert!(
		matches!(err, CommitBufferError::Other(ref message) if message.contains("before apply")),
		"pre-apply failure should be generic transport error: {err:?}",
	);
	assert!(!ctx.is_dead());
	assert!(ctx.clone_fatal_error().is_none());

	ctx.flush_dirty_pages()
		.expect("retry from same head should succeed");
	let state = ctx.state.read();
	assert!(state.write_buffer.dirty.is_empty());
	assert!(state.head_txid.is_some());
}

#[test]
fn commit_atomic_write_clears_last_error_on_success() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	// An empty dirty buffer short-circuits before the success branch runs.
	{
		let mut state = ctx.state.write();
		state.write_buffer.in_atomic_write = true;
		state.write_buffer.saved_db_size = state.db_size_pages;
		state.write_buffer.dirty.insert(1, vec![0xa3; 4096]);
		state.db_size_pages = 1;
	}

	ctx.commit_atomic_write()
		.expect("commit_atomic_write should succeed against the direct engine");

	assert!(
		!ctx.is_dead(),
		"successful commit_atomic_write must not poison the VFS",
	);
	let last_err = db.take_last_kv_error();
	assert!(
		last_err.is_none(),
		"successful commit_atomic_write must leave last_kv_error unset; got {last_err:?}",
	);
}

#[test]
fn concurrent_reader_during_commit_atomic_observes_consistent_snapshot() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let ctx = VfsContext::new(
		harness.actor_id.clone(),
		None,
		runtime.handle().clone(),
		transport,
		VfsConfig::default(),
		unsafe { std::mem::zeroed() },
		InitialPages::default(),
		None,
	)
	.expect("vfs context should build");

	let before_page_1 = vec![0x11; 4096];
	let before_page_2 = vec![0x22; 4096];
	let after_page_1 = vec![0xaa; 4096];
	let after_page_2 = vec![0xbb; 4096];
	{
		let mut state = ctx.state.write();
		state.db_size_pages = 2;
		state.page_cache.insert(1, before_page_1.clone());
		state.page_cache.insert(2, before_page_2.clone());
		state.write_buffer.in_atomic_write = true;
		state.write_buffer.saved_db_size = state.db_size_pages;
		state.write_buffer.dirty.insert(1, after_page_1.clone());
		state.write_buffer.dirty.insert(2, after_page_2.clone());
	}

	let pause = hooks.pause_next_commit();
	let (read_rc, observed) = thread::scope(|scope| {
		let writer = scope.spawn(|| {
			ctx.commit_atomic_write()
				.expect("commit_atomic_write should finish after the pause is released");
		});
		pause.wait_until_reached();

		let reader = scope.spawn(|| {
			let mut file = VfsFile {
				base: unsafe { std::mem::zeroed() },
				ctx: &ctx,
				aux: ptr::null_mut(),
			};
			let mut buf = vec![0; 8192];
			let rc = unsafe {
				io_read(
					(&mut file as *mut VfsFile).cast::<sqlite3_file>(),
					buf.as_mut_ptr().cast(),
					buf.len() as c_int,
					0,
				)
			};
			(rc, buf)
		});
		let read = reader.join().expect("reader thread should not panic");
		pause.resume();
		writer.join().expect("writer thread should not panic");
		read
	});

	assert_eq!(read_rc, SQLITE_OK);
	let before = [before_page_1, before_page_2].concat();
	let after = [after_page_1, after_page_2].concat();
	assert!(
		observed == before || observed == after,
		"concurrent xRead during commit_atomic_write saw a torn page snapshot",
	);
	let resolved = ctx
		.resolve_pages(&[1, 2], false)
		.expect("post-commit pages should resolve");
	assert_eq!(
		resolved.get(&1).and_then(Option::as_deref),
		Some(&after[..4096]),
	);
	assert_eq!(
		resolved.get(&2).and_then(Option::as_deref),
		Some(&after[4096..]),
	);
}

#[test]
fn vfs_registration_is_removed_after_registration_panic() {
	let vfs_name = next_test_name("panic-leak-vfs");
	let c_vfs_name = CString::new(vfs_name).expect("vfs name should not contain NULs");

	let panic_result = std::panic::catch_unwind(|| {
		let mut vfs: sqlite3_vfs = unsafe { std::mem::zeroed() };
		vfs.iVersion = 1;
		vfs.szOsFile = std::mem::size_of::<VfsFile>() as c_int;
		vfs.mxPathname = MAX_PATHNAME;
		vfs.zName = c_vfs_name.as_ptr();

		let _registration = SqliteVfsRegistration::register(vfs).expect("vfs should register");
		let registered = unsafe { sqlite3_vfs_find(c_vfs_name.as_ptr()) };
		assert!(!registered.is_null(), "registered vfs should be findable");

		panic!("simulate panic after sqlite3_vfs_register");
	});

	assert!(panic_result.is_err(), "test panic should be captured");
	let registered = unsafe { sqlite3_vfs_find(c_vfs_name.as_ptr()) };
	assert!(
		registered.is_null(),
		"panicked registration should be unregistered during unwind",
	);
}

#[test]
fn vfs_delete_main_db_resets_in_memory_state() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let vfs_name = next_test_name("sqlite-direct-vfs");
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let vfs = SqliteVfs::register_with_transport(
		&vfs_name,
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE doomed (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO doomed (id, value) VALUES (1, 'gone');",
	)
	.expect("insert should succeed");

	assert!(
		ctx.state.read().db_size_pages > 0,
		"db should have pages before delete",
	);

	let c_vfs_name = CString::new(vfs_name).expect("vfs name should not contain NULs");
	let c_actor_path =
		CString::new(harness.actor_id.as_str()).expect("actor id should not contain NULs");
	let rc = unsafe {
		let p_vfs = sqlite3_vfs_find(c_vfs_name.as_ptr());
		assert!(!p_vfs.is_null(), "registered vfs should be findable");
		let x_delete = (*p_vfs).xDelete.expect("vfs must define xDelete");
		x_delete(p_vfs, c_actor_path.as_ptr(), 0)
	};

	assert_eq!(rc, SQLITE_IOERR_DELETE);
	assert_eq!(
		db.take_last_kv_error().as_deref(),
		Some("main database deletion is unsupported"),
	);
}

#[test]
fn direct_engine_handles_multithreaded_statement_churn() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	// Forced-sync: this test shares one SQLite handle across std::thread workers.
	let db = Arc::new(SyncMutex::new(harness.open_db(&runtime)));

	{
		let db = db.lock();
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT NOT NULL);",
		)
		.expect("create table should succeed");
	}

	let mut workers = Vec::new();
	for worker_id in 0..4 {
		let db = Arc::clone(&db);
		workers.push(thread::spawn(move || {
			for idx in 0..40 {
				let db = db.lock();
				sqlite_step_statement(
					db.as_ptr(),
					&format!("INSERT INTO items (value) VALUES ('worker-{worker_id}-row-{idx}');"),
				)
				.expect("threaded insert should succeed");
			}
		}));
	}
	for worker in workers {
		worker.join().expect("worker thread should finish");
	}

	let db = db.lock();
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM items;")
			.expect("threaded row count should succeed"),
		160
	);
}

#[test]
fn direct_engine_isolates_two_actors_on_one_shared_engine() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let actor_a = next_test_name("sqlite-actor-a");
	let actor_b = next_test_name("sqlite-actor-b");
	let db_a = harness.open_db_on_engine(
		&runtime,
		Arc::clone(&engine),
		&actor_a,
		VfsConfig::default(),
	);
	let db_b = harness.open_db_on_engine(&runtime, engine, &actor_b, VfsConfig::default());

	sqlite_exec(
		db_a.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("actor A create table should succeed");
	sqlite_exec(
		db_b.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("actor B create table should succeed");
	sqlite_step_statement(
		db_a.as_ptr(),
		"INSERT INTO items (id, value) VALUES (1, 'alpha');",
	)
	.expect("actor A insert should succeed");
	sqlite_step_statement(
		db_b.as_ptr(),
		"INSERT INTO items (id, value) VALUES (1, 'beta');",
	)
	.expect("actor B insert should succeed");

	assert_eq!(
		sqlite_query_text(db_a.as_ptr(), "SELECT value FROM items WHERE id = 1;")
			.expect("actor A select should succeed"),
		"alpha"
	);
	assert_eq!(
		sqlite_query_text(db_b.as_ptr(), "SELECT value FROM items WHERE id = 1;")
			.expect("actor B select should succeed"),
		"beta"
	);
}

#[test]
fn direct_engine_hot_row_updates_survive_reopen() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();

	{
		let db = harness.open_db(&runtime);
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE counters (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
		)
		.expect("create table should succeed");
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO counters (id, value) VALUES (1, 'v-0');",
		)
		.expect("seed row should succeed");
		for i in 1..=150 {
			sqlite_step_statement(
				db.as_ptr(),
				&format!("UPDATE counters SET value = 'v-{i}' WHERE id = 1;"),
			)
			.expect("hot-row update should succeed");
		}
	}

	let reopened = harness.open_db(&runtime);
	assert_eq!(
		sqlite_query_text(
			reopened.as_ptr(),
			"SELECT value FROM counters WHERE id = 1;"
		)
		.expect("final value should survive reopen"),
		"v-150"
	);
}

#[test]
fn direct_engine_repeated_close_reopen_cycles_preserve_state() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let actor_id = &harness.actor_id;

	for cycle in 0..20 {
		let db =
			harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE IF NOT EXISTS reopen_cycles (
					id INTEGER PRIMARY KEY,
					cycle INTEGER NOT NULL,
					value INTEGER NOT NULL
				);",
		)
		.expect("create table should succeed");

		let start = cycle * 25;
		for id in start..start + 25 {
			sqlite_exec(
				db.as_ptr(),
				&format!(
					"INSERT INTO reopen_cycles (id, cycle, value) VALUES ({id}, {cycle}, {})
						ON CONFLICT(id) DO UPDATE SET cycle = excluded.cycle, value = excluded.value;",
					id * 3
				),
			)
			.expect("insert across reopen cycle should succeed");
		}

		assert_eq!(
			sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM reopen_cycles;")
				.expect("count during reopen cycle should succeed"),
			((cycle + 1) * 25) as i64
		);
	}

	let reopened =
		harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM reopen_cycles;")
			.expect("final reopen count should succeed"),
		500
	);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT SUM(value) FROM reopen_cycles;")
			.expect("final reopen sum should succeed"),
		(0..500).map(|id| (id * 3) as i64).sum::<i64>()
	);
	assert_eq!(
		sqlite_query_text(reopened.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity_check after reopen loop should succeed"),
		"ok"
	);
}

#[test]
fn direct_engine_preserves_mixed_workload_across_sleep_wake() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();

	{
		let db = harness.open_db(&runtime);
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL, status TEXT NOT NULL);",
		)
		.expect("create table should succeed");
		for id in 1..=50 {
			sqlite_step_statement(
				db.as_ptr(),
				&format!(
					"INSERT INTO items (id, value, status) VALUES ({id}, 'item-{id}', 'new');"
				),
			)
			.expect("seed insert should succeed");
		}
		for id in 1..=20 {
			sqlite_step_statement(
				db.as_ptr(),
				&format!(
					"UPDATE items SET status = 'updated', value = 'item-{id}-updated' WHERE id = {id};"
				),
			)
			.expect("update should succeed");
		}
		for id in 41..=50 {
			sqlite_step_statement(db.as_ptr(), &format!("DELETE FROM items WHERE id = {id};"))
				.expect("delete should succeed");
		}
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO items (id, value, status) VALUES (1000, 'disconnect-write', 'new');",
		)
		.expect("disconnect-style write before close should succeed");
	}

	let reopened = harness.open_db(&runtime);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM items;")
			.expect("row count after reopen should succeed"),
		41
	);
	assert_eq!(
		sqlite_query_i64(
			reopened.as_ptr(),
			"SELECT COUNT(*) FROM items WHERE status = 'updated';",
		)
		.expect("updated row count should succeed"),
		20
	);
	assert_eq!(
		sqlite_query_text(
			reopened.as_ptr(),
			"SELECT value FROM items WHERE id = 1000;",
		)
		.expect("disconnect write should survive reopen"),
		"disconnect-write"
	);
}

#[test]
fn direct_engine_reopens_cleanly_after_failed_migration() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();

	{
		let db = harness.open_db(&runtime);
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
		)
		.expect("create table should succeed");
		sqlite_exec(db.as_ptr(), "ALTER TABLE items ADD COLUMN;")
			.expect_err("broken migration should fail");
	}

	let reopened = harness.open_db(&runtime);
	sqlite_step_statement(
		reopened.as_ptr(),
		"INSERT INTO items (id, value) VALUES (1, 'still-alive');",
	)
	.expect("reopened database should still accept writes after migration failure");
	assert_eq!(
		sqlite_query_text(reopened.as_ptr(), "SELECT value FROM items WHERE id = 1;")
			.expect("select after reopen should succeed"),
		"still-alive"
	);
}

#[test]
fn direct_engine_fresh_reopen_recovers_after_poisoned_handle() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine.clone()));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE stable_rows (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO stable_rows (id, value) VALUES (1, 'committed-before-failure');",
	)
	.expect("seed write should succeed");

	hooks.fail_next_commit("InjectedTransportError: reopen recovery transport dropped");
	let err = sqlite_exec(
		db.as_ptr(),
		"INSERT INTO stable_rows (id, value) VALUES (2, 'should-not-commit');",
	)
	.expect_err("failing transport commit should surface as an IO error");
	assert!(
		err.contains("I/O") || err.contains("disk I/O"),
		"sqlite should surface transport failure as an IO error: {err}",
	);
	assert!(
		!direct_vfs_ctx(&db).is_dead(),
		"transport error should not poison the live VFS",
	);

	drop(db);

	let reopened = harness.open_db_on_engine(
		&runtime,
		engine.clone(),
		&harness.actor_id,
		VfsConfig::default(),
	);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM stable_rows;")
			.expect("reopened count should succeed"),
		1
	);
	assert_eq!(
		sqlite_query_text(
			reopened.as_ptr(),
			"SELECT value FROM stable_rows WHERE id = 1;"
		)
		.expect("committed row should survive reopen"),
		"committed-before-failure"
	);
	assert_eq!(
		sqlite_query_i64(
			reopened.as_ptr(),
			"SELECT COUNT(*) FROM stable_rows WHERE id = 2;",
		)
		.expect("failed row should stay absent"),
		0
	);

	sqlite_exec(
		reopened.as_ptr(),
		"INSERT INTO stable_rows (id, value) VALUES (3, 'after-reopen');",
	)
	.expect("fresh reopen should accept new writes");
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM stable_rows;")
			.expect("final count should succeed"),
		2
	);
}

#[test]
fn direct_engine_crash_with_dirty_buffer_recovers_last_commit() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine.clone()));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("v2 vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("sqlite database should open");

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE crash_rows (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO crash_rows (id, value) VALUES (1, 'durable-before-crash');",
	)
	.expect("seed write should succeed");

	let ctx = direct_vfs_ctx(&db);
	{
		let mut state = ctx.state.write();
		state.write_buffer.in_atomic_write = true;
		state.write_buffer.saved_db_size = state.db_size_pages;
		state.write_buffer.dirty.insert(1, empty_db_page());
		state.db_size_pages = 1;
	}
	hooks.fail_next_commit("InjectedTransportError: crash before dirty buffer commit ack");
	drop(db);

	let reopened = harness.open_db_on_engine(
		&runtime,
		engine.clone(),
		&harness.actor_id,
		VfsConfig::default(),
	);
	assert_eq!(
		sqlite_query_i64(reopened.as_ptr(), "SELECT COUNT(*) FROM crash_rows;")
			.expect("reopened database should keep the last successful commit"),
		1
	);
	assert_eq!(
		sqlite_query_text(
			reopened.as_ptr(),
			"SELECT value FROM crash_rows WHERE id = 1;"
		)
		.expect("committed row should survive dirty-buffer crash"),
		"durable-before-crash"
	);
}

#[test]
fn direct_engine_aux_open_failure_surfaces_without_poisoning_main_db() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO items (id, value) VALUES (1, 'still-works');",
	)
	.expect("seed write should succeed");

	ctx.fail_next_aux_open("InjectedAuxOpenError: attached db open failed");
	let err = sqlite_exec(db.as_ptr(), "ATTACH 'scratch-aux.db' AS scratch;")
		.expect_err("attach should surface aux open failure");
	assert!(
		err.contains("open") || err.contains("I/O") || err.contains("disk I/O"),
		"sqlite should surface aux open failure: {err}",
	);
	assert_eq!(
		db.take_last_kv_error().as_deref(),
		Some("InjectedAuxOpenError: attached db open failed"),
	);
	assert!(
		!ctx.is_dead(),
		"aux open failure should not poison the main db handle",
	);
	assert_eq!(
		sqlite_query_text(db.as_ptr(), "SELECT value FROM items WHERE id = 1;")
			.expect("main db should remain queryable"),
		"still-works"
	);
}

#[test]
fn vfs_delete_surfaces_aux_delete_failure() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	ctx.open_aux_file("actor-journal");
	ctx.fail_next_aux_delete("InjectedAuxDeleteError: delete failed");
	let path = CString::new("actor-journal").expect("cstring should build");

	let rc = unsafe { vfs_delete(db._vfs.vfs_ptr(), path.as_ptr(), 0) };
	assert_eq!(rc, SQLITE_IOERR_DELETE);
	assert_eq!(
		db.take_last_kv_error().as_deref(),
		Some("InjectedAuxDeleteError: delete failed"),
	);
	assert!(
		ctx.aux_file_exists("actor-journal"),
		"failed delete should leave aux state intact",
	);
}

#[test]
fn direct_engine_commits_trigger_workflow_compaction_wake() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let db = harness.open_db_on_engine(
		&runtime,
		Arc::clone(&engine),
		&harness.actor_id,
		VfsConfig::default(),
	);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	for id in 1..=40 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!("INSERT INTO items (id, value) VALUES ({id}, 'row-{id}');"),
		)
		.expect("seed insert should succeed");
	}

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM items;")
			.expect("final row count should succeed"),
		40
	);
	let signals = engine.compaction_signals();
	assert!(
		!signals.is_empty(),
		"VFS commits should wake workflow compaction once hot lag is actionable",
	);
	assert!(
		signals.iter().any(|signal| signal.observed_head_txid >= 32),
		"workflow wake should observe the actionable hot-lag txid: {signals:?}",
	);
}

#[test]
fn native_database_drop_times_out_pending_commit() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let vfs = SqliteVfs::register_with_transport(
		&next_test_name("sqlite-direct-vfs"),
		transport,
		harness.actor_id.clone(),
		runtime.handle().clone(),
		VfsConfig::default(),
		None,
	)
	.expect("vfs should register");
	let db = open_database(vfs, &harness.actor_id).expect("db should open");
	let commit_count_before_drop = hooks.commit_requests().len();
	{
		let ctx = db._vfs.ctx();
		let mut state = ctx.state.write();
		state.db_size_pages = 1;
		state.write_buffer.dirty.insert(1, empty_db_page());
	}
	hooks.hang_next_commit();

	let (finished_tx, finished_rx) = mpsc::channel();
	let drop_thread = thread::spawn(move || {
		drop(db);
		finished_tx
			.send(())
			.expect("drop completion should be reported");
	});

	finished_rx
		.recv_timeout(Duration::from_secs(2))
		.expect("drop should not block forever on a pending commit");
	drop_thread.join().expect("drop thread should finish");
	assert_eq!(hooks.commit_requests().len(), commit_count_before_drop + 1);
}

#[test]
fn open_database_supports_empty_db_schema_setup() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("schema setup should succeed");
}

#[test]
fn open_database_supports_insert_after_pragma_migration() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"ALTER TABLE items ADD COLUMN status TEXT NOT NULL DEFAULT 'active';",
	)
	.expect("alter table should succeed");
	sqlite_exec(db.as_ptr(), "PRAGMA user_version = 2;").expect("pragma should succeed");
	sqlite_step_statement(
		db.as_ptr(),
		"INSERT INTO items (name) VALUES ('test-item');",
	)
	.expect("insert after pragma migration should succeed");
}

#[test]
fn open_database_supports_explicit_status_insert_after_pragma_migration() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(
		db.as_ptr(),
		"ALTER TABLE items ADD COLUMN status TEXT NOT NULL DEFAULT 'active';",
	)
	.expect("alter table should succeed");
	sqlite_exec(db.as_ptr(), "PRAGMA user_version = 2;").expect("pragma should succeed");
	sqlite_step_statement(
		db.as_ptr(),
		"INSERT INTO items (name, status) VALUES ('done-item', 'completed');",
	)
	.expect("explicit status insert should succeed");
}

#[test]
fn open_database_supports_hot_row_update_churn() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);

	sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE test_data (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT NOT NULL, payload TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL);",
		)
		.expect("create table should succeed");
	for i in 0..10 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO test_data (value, payload, created_at) VALUES ('init-{i}', '', 1);"
			),
		)
		.expect("seed insert should succeed");
	}
	for i in 0..240 {
		let row_id = i % 10 + 1;
		sqlite_step_statement(
			db.as_ptr(),
			&format!("UPDATE test_data SET value = 'v-{i}' WHERE id = {row_id};"),
		)
		.expect("hot-row update should succeed");
	}
}

#[test]
fn open_database_supports_cross_thread_exec_sequence() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	// Forced-sync: this test moves one SQLite handle between std::thread workers.
	let db = Arc::new(SyncMutex::new(harness.open_db(&runtime)));

	{
		let db = db.clone();
		thread::spawn(move || {
			let db = db.lock();
			sqlite_exec(
				db.as_ptr(),
				"CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);",
			)
			.expect("create table should succeed");
			sqlite_exec(
				db.as_ptr(),
				"ALTER TABLE items ADD COLUMN status TEXT NOT NULL DEFAULT 'active';",
			)
			.expect("alter table should succeed");
			sqlite_exec(db.as_ptr(), "PRAGMA user_version = 2;").expect("pragma should succeed");
		})
		.join()
		.expect("migration thread should finish");
	}

	thread::spawn(move || {
		let db = db.lock();
		sqlite_step_statement(
			db.as_ptr(),
			"INSERT INTO items (name) VALUES ('test-item');",
		)
		.expect("cross-thread insert should succeed");
	})
	.join()
	.expect("insert thread should finish");
}

#[test]
fn aux_files_are_shared_by_path_until_deleted() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let ctx = harness.open_context(&runtime);

	let first = ctx.open_aux_file("actor-journal");
	first.bytes.lock().extend_from_slice(&[1, 2, 3, 4]);
	let second = ctx.open_aux_file("actor-journal");
	assert_eq!(*second.bytes.lock(), vec![1, 2, 3, 4]);
	assert!(ctx.aux_file_exists("actor-journal"));

	ctx.delete_aux_file("actor-journal");
	assert!(!ctx.aux_file_exists("actor-journal"));
	assert!(ctx.open_aux_file("actor-journal").bytes.lock().is_empty());
}

#[test]
fn concurrent_aux_file_opens_share_single_state() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let ctx = Arc::new(harness.open_context(&runtime));
	let barrier = Arc::new(Barrier::new(2));

	let first = {
		let ctx = ctx.clone();
		let barrier = barrier.clone();
		thread::spawn(move || {
			barrier.wait();
			ctx.open_aux_file("actor-journal")
		})
	};
	let second = {
		let ctx = ctx.clone();
		let barrier = barrier.clone();
		thread::spawn(move || {
			barrier.wait();
			ctx.open_aux_file("actor-journal")
		})
	};

	let first = first.join().expect("first open should complete");
	let second = second.join().expect("second open should complete");
	assert!(Arc::ptr_eq(&first, &second));
	assert_eq!(ctx.aux_files.read().len(), 1);
}

#[test]
fn truncate_main_file_discards_pages_beyond_eof() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let ctx = harness.open_context(&runtime);
	{
		let mut state = ctx.state.write();
		state.write_buffer.dirty.insert(3, vec![3; 4096]);
		state.write_buffer.dirty.insert(4, vec![4; 4096]);
	}

	ctx.truncate_main_file(2 * 4096);

	let state = ctx.state.read();
	assert_eq!(state.db_size_pages, 2);
	assert!(!state.write_buffer.dirty.contains_key(&3));
	assert!(!state.write_buffer.dirty.contains_key(&4));
	assert!(state.page_cache.get(&4).is_none());
}

#[test]
fn resolve_pages_surfaces_read_path_error_response() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let ctx = VfsContext::new(
		harness.actor_id.clone(),
		None,
		runtime.handle().clone(),
		transport,
		VfsConfig::default(),
		unsafe { std::mem::zeroed() },
		InitialPages::default(),
		None,
	)
	.expect("vfs context should build");
	hooks.fail_next_get_pages("InjectedGetPagesError: read path dropped");

	let err = ctx
		.resolve_pages(&[2], false)
		.expect_err("read-path error response should surface");
	assert!(matches!(
		err,
		GetPagesError::Other(ref message)
			if message.contains("InjectedGetPagesError")
	));
}

#[test]
fn dead_vfs_rejects_cache_only_reads_before_serving_page_cache() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let config = VfsConfig::default();
	let ctx = VfsContext::new(
		harness.actor_id.clone(),
		None,
		runtime.handle().clone(),
		transport,
		config.clone(),
		unsafe { std::mem::zeroed() },
		InitialPages::default(),
		None,
	)
	.expect("vfs context should build");
	{
		let mut state = ctx.state.write();
		state.db_size_pages = 2;
		state.cache_page(&config, PageCacheInsertKind::Target, 2, vec![0x42; 4096]);
	}

	ctx.mark_fatal("generation fence lost".to_string());
	let err = ctx
		.resolve_pages(&[2], false)
		.expect_err("dead VFS should not serve cached pages");
	assert!(matches!(
		err,
		GetPagesError::Other(ref message) if message.contains("lost its fence")
	));
	assert!(
		hooks.get_pages_requests().is_empty(),
		"dead cache-only read should fail before calling Depot"
	);
}

#[test]
fn head_fence_mismatch_on_xread_poisoned_vfs_fails_later_operations_closed() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	runtime
		.block_on(async {
			engine
				.actor_db(harness.actor_id.clone())
				.await
				.commit(
					vec![depot::types::DirtyPage {
						pgno: 2,
						bytes: vec![0x55; 4096],
					}],
					2,
					sqlite_now_ms().expect("now ms should build"),
				)
				.await
		})
		.expect("seed commit should advance Depot head");

	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();
	let config = VfsConfig::default();
	let ctx = VfsContext::new(
		harness.actor_id.clone(),
		None,
		runtime.handle().clone(),
		transport,
		config.clone(),
		unsafe { std::mem::zeroed() },
		InitialPages::default(),
		None,
	)
	.expect("vfs context should build");
	{
		let mut state = ctx.state.write();
		state.db_size_pages = 2;
		state.head_txid = Some(0);
		state.cache_page(&config, PageCacheInsertKind::Target, 1, empty_db_page());
	}

	let mut file = VfsFile {
		base: unsafe { std::mem::zeroed() },
		ctx: &ctx,
		aux: ptr::null_mut(),
	};
	let mut buf = vec![0; 4096];
	let rc = unsafe {
		io_read(
			(&mut file as *mut VfsFile).cast::<sqlite3_file>(),
			buf.as_mut_ptr().cast(),
			buf.len() as c_int,
			4096,
		)
	};
	assert_eq!(rc, SQLITE_IOERR_READ);
	assert!(ctx.is_dead());
	assert!(
		ctx.clone_fatal_error()
			.is_some_and(|message| message.contains("head fence mismatch")),
		"head fence mismatch should be retained as the fatal reason"
	);
	assert_eq!(hooks.get_pages_requests().len(), 1);

	let rc = unsafe {
		io_read(
			(&mut file as *mut VfsFile).cast::<sqlite3_file>(),
			buf.as_mut_ptr().cast(),
			buf.len() as c_int,
			0,
		)
	};
	assert_eq!(rc, SQLITE_IOERR_READ);
	assert_eq!(
		hooks.get_pages_requests().len(),
		1,
		"dead cache-only read should not call Depot again"
	);

	let source = vec![0x7a; 4096];
	let rc = unsafe {
		io_write(
			(&mut file as *mut VfsFile).cast::<sqlite3_file>(),
			source.as_ptr().cast(),
			source.len() as c_int,
			0,
		)
	};
	assert_eq!(rc, SQLITE_IOERR_WRITE);

	{
		let mut state = ctx.state.write();
		state.write_buffer.dirty.insert(1, empty_db_page());
	}
	let rc = unsafe { io_sync((&mut file as *mut VfsFile).cast::<sqlite3_file>(), 0) };
	assert_eq!(rc, SQLITE_IOERR_FSYNC);
	assert!(
		hooks.commit_requests().is_empty(),
		"dead VFS should fail writes and sync before committing"
	);
}

#[test]
fn resolve_pages_sends_known_head_txid_as_read_fence() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	runtime
		.block_on(engine.apply_commit(
			&harness.actor_id,
			vec![depot::types::DirtyPage {
				pgno: 2,
				bytes: vec![0x44; DEFAULT_PAGE_SIZE],
			}],
			2,
		))
		.expect("mirror seed should succeed");
	let transport = Arc::new(DirectMirrorTransport::new(engine));
	let hooks = transport.direct_hooks();
	let ctx = VfsContext::new(
		harness.actor_id.clone(),
		None,
		runtime.handle().clone(),
		transport,
		VfsConfig::default(),
		unsafe { std::mem::zeroed() },
		InitialPages::default(),
		None,
	)
	.expect("vfs context should build");
	{
		let mut state = ctx.state.write();
		state.db_size_pages = 2;
		state.head_txid = Some(7);
	}

	ctx.resolve_pages(&[2], false)
		.expect("read should fetch from transport");
	let requests = hooks.get_pages_requests();
	let request = requests
		.last()
		.expect("get_pages request should be recorded");
	assert_eq!(request.expected_head_txid, Some(7));
}

#[test]
fn fatal_sqlite_error_reports_first_message_through_worker_boundary() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let (vfs, db) = open_worker_handle_with_vfs(&runtime, &harness, None);
	let ctx = vfs.ctx();
	runtime
		.block_on(db.exec("SELECT 1;".to_string()))
		.expect("worker should initialize before fatal state is injected");

	ctx.mark_fatal("first fatal sqlite error".to_string());
	ctx.mark_fatal("second fatal sqlite error".to_string());

	assert!(ctx.is_dead());
	assert_eq!(
		ctx.clone_fatal_error().as_deref(),
		Some("first fatal sqlite error")
	);

	let err = runtime
		.block_on(db.exec("SELECT 1;".to_string()))
		.expect_err("fatal VFS state should fail before SQL execution");
	let fatal = err
		.downcast_ref::<SqliteWorkerFatalError>()
		.expect("error should be typed as fatal worker error");
	assert_eq!(fatal.message(), "first fatal sqlite error");

	runtime
		.block_on(db.close())
		.expect("worker should still close cleanly after fatal state");
}

#[test]
fn head_fence_ioerr_maps_to_fatal_worker_error_and_future_operations_fail_closed() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let stale = open_worker_handle(&runtime, &harness);
	runtime
		.block_on(stale.exec("SELECT 1;".to_string()))
		.expect("stale worker should initialize before becoming stale");
	let writer = open_worker_handle(&runtime, &harness);

	runtime
		.block_on(writer.exec("CREATE TABLE writer_first (value INTEGER);".to_string()))
		.expect("writer should advance depot head");

	let err = runtime
		.block_on(stale.exec("CREATE TABLE stale_writer (value INTEGER);".to_string()))
		.expect_err("stale writer should hit a fatal head fence mismatch");
	let fatal = err
		.downcast_ref::<SqliteWorkerFatalError>()
		.expect("head fence mismatch should be surfaced as a fatal worker error");
	assert!(
		fatal.message().contains("head fence mismatch"),
		"unexpected fatal message: {}",
		fatal.message()
	);
	assert!(
		stale
			.clone_fatal_error()
			.is_some_and(|message| message.contains("head fence mismatch")),
		"VFS should retain the fatal head fence error"
	);

	let err = runtime
		.block_on(stale.exec("SELECT 1;".to_string()))
		.expect_err("future operations should fail closed from stored fatal state");
	assert!(
		err.downcast_ref::<SqliteWorkerFatalError>().is_some(),
		"future error should remain typed as fatal worker error: {err:#}"
	);

	runtime
		.block_on(stale.close())
		.expect("stale worker should still close cleanly");
	runtime
		.block_on(writer.close())
		.expect("writer worker should close cleanly");
}

#[test]
fn stale_vfs_page_cache_writer_fails_head_fence_and_reopen_is_clean() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		..VfsConfig::default()
	};
	let db_a =
		harness.open_db_on_engine(&runtime, engine.clone(), &harness.actor_id, config.clone());

	sqlite_exec(
		db_a.as_ptr(),
		"CREATE TABLE cached_rows (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
	)
	.expect("create should succeed");
	for id in 1..=64 {
		sqlite_exec(
			db_a.as_ptr(),
			&format!(
				"INSERT INTO cached_rows (id, payload) VALUES ({id}, printf('%04d:%s', {id}, zeroblob(512)));"
			),
		)
		.expect("insert should succeed");
	}

	let cached_before = sqlite_query_text(
		db_a.as_ptr(),
		"SELECT payload FROM cached_rows WHERE id = 32;",
	)
	.expect("stale handle should read target row");
	assert!(cached_before.starts_with("0032:"));

	let db_b =
		harness.open_db_on_engine(&runtime, engine.clone(), &harness.actor_id, config.clone());
	sqlite_exec(
		db_b.as_ptr(),
		"UPDATE cached_rows SET payload = 'writer-b-new-value' WHERE id = 32;",
	)
	.expect("fresh writer should commit");
	assert_eq!(
		sqlite_query_text(
			db_b.as_ptr(),
			"SELECT payload FROM cached_rows WHERE id = 32;"
		)
		.expect("fresh writer should read its update"),
		"writer-b-new-value"
	);

	let stale_update = sqlite_exec(
		db_a.as_ptr(),
		"UPDATE cached_rows SET payload = 'stale-writer-value' WHERE id = 32;",
	);
	assert!(
		stale_update
			.as_ref()
			.expect_err("stale writer must fail the head fence")
			.contains("disk I/O error"),
		"unexpected stale writer error: {stale_update:?}"
	);
	assert!(
		direct_vfs_ctx(&db_a)
			.clone_fatal_error()
			.is_some_and(|message| message.contains("head fence mismatch")),
		"stale VFS should retain the head fence mismatch"
	);

	drop(db_a);
	drop(db_b);

	let db_c = harness.open_db_on_engine(&runtime, engine, &harness.actor_id, config);
	assert_eq!(
		sqlite_query_text(db_c.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity check should run after stale writer failed"),
		"ok"
	);
	assert_eq!(
		sqlite_query_text(
			db_c.as_ptr(),
			"SELECT payload FROM cached_rows WHERE id = 32;"
		)
		.expect("fresh reopen should see writer B value"),
		"writer-b-new-value"
	);
}

#[test]
fn delayed_read_ahead_response_fails_head_fence_and_reopen_is_clean() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		..VfsConfig::default()
	};
	let db_setup =
		harness.open_db_on_engine(&runtime, engine.clone(), &harness.actor_id, config.clone());

	sqlite_exec(
		db_setup.as_ptr(),
		"CREATE TABLE delayed_rows (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
	)
	.expect("create should succeed");
	for id in 1..=96 {
		sqlite_exec(
			db_setup.as_ptr(),
			&format!(
				"INSERT INTO delayed_rows (id, payload) VALUES ({id}, printf('%04d:%s', {id}, hex(zeroblob(2048))));"
			),
		)
		.expect("insert should succeed");
	}
	let page_count = sqlite_query_i64(db_setup.as_ptr(), "PRAGMA page_count;")
		.expect("page count should be readable");
	assert!(
		page_count >= 12,
		"test setup should create enough pages for read-ahead, got {page_count}"
	);
	drop(db_setup);

	let delaying_transport = Arc::new(DelayCapturedGetPagesTransport::new(
		Arc::new(DirectDepotTransport::new(engine.clone())),
		0,
	));
	let db_a = harness.open_db_with_transport(
		&runtime,
		delaying_transport.clone(),
		&harness.actor_id,
		config.clone(),
	);

	let ctx = direct_vfs_ctx(&db_a);
	ctx.resolve_pages(&[2], false)
		.expect("training read page 2 should succeed");
	ctx.resolve_pages(&[3], false)
		.expect("training read page 3 should succeed");
	ctx.resolve_pages(&[4], false)
		.expect("training read page 4 should succeed");

	delaying_transport.enable();
	let vfs_a = db_a._vfs.clone();
	let delayed_read = thread::spawn(move || {
		vfs_a
			.ctx()
			.resolve_pages(&[5], true)
			.expect("delayed read-ahead fetch should eventually succeed")
	});

	runtime
		.block_on(async {
			tokio::time::timeout(Duration::from_secs(5), delaying_transport.wait_captured()).await
		})
		.expect("delayed get_pages response should be captured before writer B commits");

	let db_b =
		harness.open_db_on_engine(&runtime, engine.clone(), &harness.actor_id, config.clone());
	sqlite_exec(
		db_b.as_ptr(),
		"UPDATE delayed_rows SET payload = 'writer-b-new-value' WHERE id = 48;",
	)
	.expect("fresh writer should commit while delayed response is held");

	delaying_transport.release();
	let delayed_pages = delayed_read
		.join()
		.expect("delayed read thread should not panic");
	assert!(
		delayed_pages.contains_key(&5),
		"delayed read should resolve the target page"
	);

	let stale_update = sqlite_exec(
		db_a.as_ptr(),
		"UPDATE delayed_rows SET payload = 'stale-writer-value' WHERE id = 48;",
	);
	assert!(
		stale_update
			.as_ref()
			.expect_err("stale writer must fail after delayed old response")
			.contains("disk I/O error"),
		"unexpected stale writer error: {stale_update:?}"
	);
	assert!(
		direct_vfs_ctx(&db_a)
			.clone_fatal_error()
			.is_some_and(|message| message.contains("head fence mismatch")),
		"stale VFS should retain the head fence mismatch"
	);

	drop(db_a);
	drop(db_b);

	let db_c = harness.open_db_on_engine(&runtime, engine, &harness.actor_id, config);
	assert_eq!(
		sqlite_query_text(db_c.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity check should run after delayed stale writer failed"),
		"ok"
	);
	assert_eq!(
		sqlite_query_text(
			db_c.as_ptr(),
			"SELECT payload FROM delayed_rows WHERE id = 48;"
		)
		.expect("fresh reopen should see writer B value"),
		"writer-b-new-value"
	);
}

#[test]
fn delayed_startup_preload_response_fails_closed_and_reopen_is_clean() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		startup_preload_first_pages: true,
		startup_preload_first_page_count: 8,
		startup_preload_max_bytes: DEFAULT_PAGE_SIZE * 8,
		..VfsConfig::default()
	};
	let db_setup =
		harness.open_db_on_engine(&runtime, engine.clone(), &harness.actor_id, config.clone());

	sqlite_exec(
		db_setup.as_ptr(),
		"CREATE TABLE preload_rows (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
	)
	.expect("create should succeed");
	for id in 1..=96 {
		sqlite_exec(
			db_setup.as_ptr(),
			&format!(
				"INSERT INTO preload_rows (id, payload) VALUES ({id}, printf('%04d:%s', {id}, hex(zeroblob(2048))));"
			),
		)
		.expect("insert should succeed");
	}
	let page_count = sqlite_query_i64(db_setup.as_ptr(), "PRAGMA page_count;")
		.expect("page count should be readable");
	assert!(
		page_count >= 12,
		"test setup should create enough pages for startup preload, got {page_count}"
	);
	drop(db_setup);

	let delaying_transport = Arc::new(DelayCapturedGetPagesTransport::new(
		Arc::new(DirectDepotTransport::new(engine.clone())),
		0,
	));
	delaying_transport.enable();
	let actor_id = harness.actor_id.clone();
	let config_a = config.clone();
	let runtime_handle = runtime.handle().clone();
	let open_transport = delaying_transport.clone();
	let opening = thread::spawn(move || {
		let initial_pages = runtime_handle
			.block_on(fetch_initial_pages_for_registration(
				open_transport.clone(),
				&actor_id,
				0,
				&config_a,
			))
			.expect("startup preload should eventually succeed");
		let vfs = SqliteVfs::register_with_transport_and_initial_pages(
			&next_test_name("sqlite-startup-preload-vfs"),
			open_transport,
			actor_id.clone(),
			runtime_handle,
			config_a,
			initial_pages,
			None,
		)
		.expect("vfs should register after startup preload");
		open_database(vfs, &actor_id)
	});

	runtime
		.block_on(async {
			tokio::time::timeout(Duration::from_secs(5), delaying_transport.wait_captured()).await
		})
		.expect("startup preload response should be captured before writer B commits");

	let db_b =
		harness.open_db_on_engine(&runtime, engine.clone(), &harness.actor_id, config.clone());
	sqlite_exec(
		db_b.as_ptr(),
		"UPDATE preload_rows SET payload = 'writer-b-new-value' WHERE id = 48;",
	)
	.expect("fresh writer should commit while startup preload response is held");

	delaying_transport.release();
	let opened_a = opening
		.join()
		.expect("startup preload open thread should not panic");
	if let Ok(db_a) = opened_a {
		let stale_update = sqlite_exec(
			db_a.as_ptr(),
			"UPDATE preload_rows SET payload = 'stale-writer-value' WHERE id = 48;",
		);
		assert!(
			stale_update
				.as_ref()
				.expect_err("stale writer must fail after delayed startup preload")
				.contains("disk I/O error"),
			"unexpected stale writer error: {stale_update:?}"
		);
		assert!(
			direct_vfs_ctx(&db_a)
				.clone_fatal_error()
				.is_some_and(|message| message.contains("head fence mismatch")),
			"stale VFS should retain the head fence mismatch"
		);
		drop(db_a);
	} else {
		let Err(err) = opened_a else {
			unreachable!("startup preload result was already checked");
		};
		assert!(
			err.contains("disk I/O error"),
			"unexpected startup preload open error: {err}"
		);
	}
	drop(db_b);

	let db_c = harness.open_db_on_engine(&runtime, engine, &harness.actor_id, config);
	assert_eq!(
		sqlite_query_text(db_c.as_ptr(), "PRAGMA integrity_check;")
			.expect("integrity check should run after stale startup preload failed"),
		"ok"
	);
	assert_eq!(
		sqlite_query_text(
			db_c.as_ptr(),
			"SELECT payload FROM preload_rows WHERE id = 48;"
		)
		.expect("fresh reopen should see writer B value"),
		"writer-b-new-value"
	);
}

#[test]
fn warm_pidx_stale_read_then_rmw_commit_produces_malformed_db() {
	// Reproduction attempt for the Depot warm-PIDX-cache hazard described in
	// `.agent/notes/sqlite-corruption-deep-dive-findings.md` (Tier 1B / 2C):
	//
	// 1. Handle A populates the shared `Db` instance's warm PIDX cache while building real
	//    SQLite schema + rows. The warm cache records `(pgno -> owner_txid)` for sqlite_master
	//    and several root pages.
	// 2. A second, independent `Db` instance over the same UniversalDB performs a real
	//    schema-mutating commit (`CREATE INDEX`). This advances the head and rewrites pgno 1
	//    in FDB, but only updates the second `Db`'s cache; the shared `Db`'s warm PIDX keeps
	//    the pre-mutation `(pgno=1 -> old txid)` row.
	// 3. A fresh VFS handle B opens through the shared `Db`. B starts with
	//    `state.head_txid = None`, so the read fence cannot trigger. The warm PIDX hands
	//    back pgno 1 bytes from the old owner DELTA blob paired with the current head txid.
	// 4. B runs SQLite `CREATE INDEX`, forcing an RMW on pgno 1 (sqlite_master) computed
	//    from the stale bytes. B commits with `expected_head_txid = current head`, the
	//    fence passes, and the stale-derived pgno 1 lands durably.
	// 5. All handles drop; the test reopens with a fresh `Db` (no warm cache) and runs
	//    `PRAGMA integrity_check` plus schema queries to look for corruption signatures.
	use depot::conveyer::Db;
	use depot::types::{CommitOptions, DirtyPage};
	use rivet_pools::__rivet_util::Id;
	use rivet_pools::NodeId;

	struct PinnedDbTransport {
		db: Arc<Db>,
		actor_id: String,
	}

	#[async_trait]
	impl SqliteTransport for PinnedDbTransport {
		async fn get_pages(
			&self,
			request: protocol::SqliteGetPagesRequest,
		) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
			// The pinned `Db` is the only routing target; reject actor-id drift loudly.
			assert_eq!(request.actor_id, self.actor_id, "pinned transport actor id");
			match self
				.db
				.get_pages_with_options(
					request.pgnos.clone(),
					depot::types::GetPagesOptions {
						expected_head_txid: request.expected_head_txid,
						..Default::default()
					},
				)
				.await
			{
				Ok(result) => Ok(protocol::SqliteGetPagesResponse::SqliteGetPagesOk(
					protocol::SqliteGetPagesOk {
						pages: result
							.pages
							.into_iter()
							.map(|page| protocol::SqliteFetchedPage {
								pgno: page.pgno,
								bytes: page.bytes,
							})
							.collect(),
						head_txid: Some(result.head_txid),
					},
				)),
				Err(err) => Ok(protocol::SqliteGetPagesResponse::SqliteErrorResponse(
					sqlite_error_response(&err),
				)),
			}
		}

		async fn commit(
			&self,
			request: protocol::SqliteCommitRequest,
		) -> anyhow::Result<protocol::SqliteCommitResponse> {
			assert_eq!(request.actor_id, self.actor_id, "pinned transport actor id");
			let dirty_pages = request
				.dirty_pages
				.into_iter()
				.map(|page| DirtyPage {
					pgno: page.pgno,
					bytes: page.bytes,
				})
				.collect::<Vec<_>>();
			match self
				.db
				.commit_with_options(
					dirty_pages,
					request.db_size_pages,
					request.now_ms,
					CommitOptions {
						expected_head_txid: request.expected_head_txid,
						..Default::default()
					},
				)
				.await
			{
				Ok(result) => Ok(protocol::SqliteCommitResponse::SqliteCommitOk(
					protocol::SqliteCommitOk {
						head_txid: Some(result.head_txid),
					},
				)),
				Err(err) => Ok(protocol::SqliteCommitResponse::SqliteErrorResponse(
					sqlite_error_response(&err),
				)),
			}
		}
	}

	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let udb = engine.depot_database();

	// Two independent `Db` instances sharing the same actor id and UniversalDB.
	// `db_shared` is the cache-warming `Db` reused by handle A and (later) handle B.
	// `db_writer` is the "second pegboard-envoy WS conn" that commits without touching
	// `db_shared`'s warm PIDX cache.
	let db_shared = Arc::new(Db::new(
		udb.clone(),
		Id::nil(),
		harness.actor_id.clone(),
		NodeId::new(),
	));
	let db_writer = Arc::new(Db::new(
		udb.clone(),
		Id::nil(),
		harness.actor_id.clone(),
		NodeId::new(),
	));

	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		..VfsConfig::default()
	};

	// Step 1: handle A builds a real schema with enough rows to span btree internal pages.
	// A's commits flow through `db_shared`, populating its warm PIDX cache.
	let transport_a: Arc<dyn SqliteTransport + Send + Sync> = Arc::new(PinnedDbTransport {
		db: db_shared.clone(),
		actor_id: harness.actor_id.clone(),
	});
	let db_a = harness.open_db_with_transport(
		&runtime,
		transport_a.clone(),
		&harness.actor_id,
		config.clone(),
	);
	sqlite_exec(
		db_a.as_ptr(),
		"CREATE TABLE t1 (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(db_a.as_ptr(), "CREATE INDEX idx_t1_payload ON t1(payload);")
		.expect("create initial index should succeed");
	sqlite_exec(db_a.as_ptr(), "BEGIN;").expect("begin should succeed");
	for id in 1..=2000 {
		sqlite_exec(
			db_a.as_ptr(),
			&format!(
				"INSERT INTO t1 (id, payload) VALUES ({id}, printf('payload-%05d-%s', {id}, hex(zeroblob(64))));"
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db_a.as_ptr(), "COMMIT;").expect("commit should succeed");

	let page_count_after_a = sqlite_query_i64(db_a.as_ptr(), "PRAGMA page_count;")
		.expect("page_count should be readable");
	assert!(
		page_count_after_a >= 32,
		"setup should produce enough pages to span btree internals, got {page_count_after_a}"
	);
	// Step 1b: reads that exercise sqlite_master + index root + table btree internals so
	// the shared `Db` PIDX cache is warm for the pages production probes would touch.
	let _row_count =
		sqlite_query_i64(db_a.as_ptr(), "SELECT COUNT(*) FROM t1;").expect("count should succeed");
	let _sample = sqlite_query_i64(db_a.as_ptr(), "SELECT id FROM t1 WHERE id = 1;")
		.expect("indexed lookup should succeed");

	// Snapshot the post-A schema so we can detect mutations or rollback after the bug.
	let schema_after_a = sqlite_query_text(
		db_a.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema query should succeed");
	assert!(
		schema_after_a.contains("idx_t1_payload"),
		"baseline schema should include the seed index: {schema_after_a}"
	);
	drop(db_a);

	// Step 1c: directly call `db_shared.get_pages(...)` for the low-numbered pages that
	// future B reads will request. SQLite + the VFS page cache may have suppressed the
	// per-page transport calls during A's operations, leaving the warm PIDX cache empty.
	// We need entries in `db_shared.cache_snapshot.pidx` for the warm-cache hazard.
	let warm_pgnos: Vec<u32> = (1..=10).collect();
	let warm_result =
		runtime
			.block_on(db_shared.get_pages_with_options(
				warm_pgnos.clone(),
				depot::types::GetPagesOptions::default(),
			))
			.expect("warm read through db_shared should succeed");
	tracing::info!(
		?warm_pgnos,
		warm_head_txid = warm_result.head_txid,
		"warm pidx repro: warmed db_shared cache via direct get_pages"
	);

	// Step 2: an independent `Db` instance commits schema mutations on the same actor.
	// This represents the other pegboard-envoy WS conn in production. The commit rewrites
	// pgno 1 (sqlite_master) in FDB and advances the head, but `db_shared`'s warm PIDX
	// cache is not touched.
	let transport_writer: Arc<dyn SqliteTransport + Send + Sync> = Arc::new(PinnedDbTransport {
		db: db_writer.clone(),
		actor_id: harness.actor_id.clone(),
	});
	let db_writer_handle = harness.open_db_with_transport(
		&runtime,
		transport_writer.clone(),
		&harness.actor_id,
		config.clone(),
	);
	sqlite_exec(
		db_writer_handle.as_ptr(),
		"CREATE INDEX idx_t1_payload2 ON t1(payload, id);",
	)
	.expect("writer CREATE INDEX should commit");
	sqlite_exec(db_writer_handle.as_ptr(), "BEGIN;").expect("begin should succeed");
	for id in 1..=200 {
		sqlite_exec(
			db_writer_handle.as_ptr(),
			&format!("UPDATE t1 SET payload = 'writer-rewrite-{id}' WHERE id = {id};"),
		)
		.expect("writer update should succeed");
	}
	sqlite_exec(db_writer_handle.as_ptr(), "COMMIT;").expect("writer commit should succeed");
	let writer_head = runtime
		.block_on(
			db_writer.get_pages_with_options(vec![1], depot::types::GetPagesOptions::default()),
		)
		.expect("writer head fetch should succeed")
		.head_txid;
	drop(db_writer_handle);

	// Diagnostic: inspect the shared `Db` warm PIDX cache before B reads. If pgno 1
	// is present with the OLD owner txid (from A's commits) while `db_writer`'s cache
	// shows pgno 1 with the NEW owner txid (from the writer commit), the warm-cache
	// hazard is set up.
	let shared_cache_pidx = runtime
		.block_on(db_shared.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let writer_cache_pidx = runtime
		.block_on(db_writer.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let shared_pg1 = shared_cache_pidx
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	let writer_pg1 = writer_cache_pidx
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	eprintln!(
		"warm-pidx-repro: db_shared cache rows = {} (pgno 1 owner = {:?}), db_writer cache rows = {} (pgno 1 owner = {:?})",
		shared_cache_pidx.len(),
		shared_pg1,
		writer_cache_pidx.len(),
		writer_pg1,
	);

	// Step 3: open a fresh handle B through the warm `db_shared`. The startup preload
	// requests pgno 1 with `expected_head_txid = None`, hitting the warm PIDX cache and
	// returning old pgno 1 bytes paired with the current head. B then adopts the current
	// head as its own.
	let db_b = harness.open_db_with_transport(
		&runtime,
		transport_a.clone(),
		&harness.actor_id,
		config.clone(),
	);
	let schema_seen_by_b = sqlite_query_text(
		db_b.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema query through B should succeed");
	tracing::info!(
		schema_after_a = %schema_after_a,
		schema_seen_by_b = %schema_seen_by_b,
		writer_head = ?writer_head,
		"warm pidx repro: schema visibility through stale Db"
	);

	// Step 4: RMW on pgno 1 from the stale view. SQLite rewrites sqlite_master with a new
	// schema row, allocates a new root, and commits. With the warm cache hazard the
	// commit's expected_head_txid matches the current head, the fence passes, and the
	// stale-derived pgno 1 lands durably.
	let stale_create = sqlite_exec(
		db_b.as_ptr(),
		"CREATE INDEX idx_t1_payload_stale ON t1(payload, id, id);",
	);
	let mut stale_commit_observed = stale_create.is_ok();
	if let Err(message) = &stale_create {
		tracing::info!(?message, "stale RMW create index failed");
	}
	// Force a freelist mutation via DELETE so the stale RMW may also rewrite the
	// freelist trunk page from stale base bytes.
	let stale_delete = sqlite_exec(db_b.as_ptr(), "DELETE FROM t1 WHERE id <= 50;");
	if stale_delete.is_ok() {
		stale_commit_observed = true;
	}

	drop(db_b);
	drop(db_shared);
	drop(db_writer);
	runtime.block_on(engine.evict_actor_db(&harness.actor_id));

	// Step 5: fresh reopen with no warm cache. Run integrity check + schema scan.
	let db_c = harness.open_db_on_engine(&runtime, engine, &harness.actor_id, config);
	let integrity = sqlite_query_text(db_c.as_ptr(), "PRAGMA integrity_check;");
	let quick_check = sqlite_query_text(db_c.as_ptr(), "PRAGMA quick_check;");
	let schema_after_reopen = sqlite_query_text(
		db_c.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	);
	let row_count = sqlite_query_i64(db_c.as_ptr(), "SELECT COUNT(*) FROM t1;");

	// Exercise the schema and indexes more thoroughly so SQLite has to walk btree
	// internal pages, indexes, and intermediate result sets. The literal runtime
	// "database disk image is malformed" string typically surfaces during page
	// reads through these paths rather than via PRAGMA integrity_check.
	//
	// Ordering matters: passive scans must run BEFORE any REINDEX. REINDEX rebuilds
	// the index from a fresh table scan and writes a new root page, which would mask
	// the malformation for any probe running afterwards. Probes that walk the stale
	// index (page 165) are sequenced first.
	let select_indexed_by_grouped = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM (SELECT payload, COUNT(*) FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00000' AND 'payload-99999' GROUP BY payload);",
	);
	let select_indexed_by_count_star = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00000' AND 'payload-99999';",
	);
	let select_distinct_via_index = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM (SELECT DISTINCT payload FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload >= 'payload-');",
	);
	let update_through_index = sqlite_exec(
		db_c.as_ptr(),
		"UPDATE t1 SET payload = payload || '-tag' WHERE payload BETWEEN 'payload-00000' AND 'payload-99999';",
	);
	let delete_through_index = sqlite_exec(
		db_c.as_ptr(),
		"DELETE FROM t1 WHERE payload IN (SELECT payload FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00500' AND 'payload-00600');",
	);
	let analyze_stale = sqlite_exec(db_c.as_ptr(), "ANALYZE idx_t1_payload_stale;");
	let reindex_stale = sqlite_exec(db_c.as_ptr(), "REINDEX idx_t1_payload_stale;");

	let runtime_probes: [(&str, Result<String, String>); 7] = [
		(
			"select_indexed_by_grouped",
			select_indexed_by_grouped
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"select_indexed_by_count_star",
			select_indexed_by_count_star
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"select_distinct_via_index",
			select_distinct_via_index
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"update_through_index",
			update_through_index
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"delete_through_index",
			delete_through_index
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"analyze",
			analyze_stale
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"reindex_stale",
			reindex_stale
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
	];

	let mut runtime_corrupt_observed: Option<String> = None;
	for (label, result) in &runtime_probes {
		if let Err(message) = result {
			if message.contains("database disk image is malformed")
				&& runtime_corrupt_observed.is_none()
			{
				runtime_corrupt_observed = Some(format!("{label}: {message}"));
			}
		}
	}

	// Escalation: if none of the structured probes surfaced the literal substring,
	// try a writable_schema toggle plus direct keyed lookups at probable leaf
	// boundary payloads to force reads of cells 0 / 15 on page 165.
	let mut escalation_probes: Vec<(String, Result<String, String>)> = Vec::new();
	if runtime_corrupt_observed.is_none() {
		let pragma_writable_schema_on = sqlite_exec(db_c.as_ptr(), "PRAGMA writable_schema = ON;");
		escalation_probes.push((
			"pragma_writable_schema_on".to_string(),
			pragma_writable_schema_on
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		));
		for boundary_id in [125u32, 126, 127, 250, 251, 252, 375, 376, 377] {
			let q = format!(
				"SELECT id FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload = printf('payload-%05d-%s', {boundary_id}, hex(zeroblob(64)));"
			);
			let result = sqlite_query_i64(db_c.as_ptr(), &q);
			escalation_probes.push((
				format!("direct_page165_boundary_{boundary_id}"),
				result
					.as_ref()
					.map(|v| v.to_string())
					.map_err(|e| e.clone()),
			));
		}
		for (label, result) in &escalation_probes {
			if let Err(message) = result {
				if message.contains("database disk image is malformed")
					&& runtime_corrupt_observed.is_none()
				{
					runtime_corrupt_observed = Some(format!("{label}: {message}"));
				}
			}
		}
	}

	tracing::warn!(
		?integrity,
		?quick_check,
		?schema_after_reopen,
		?row_count,
		?select_indexed_by_grouped,
		?select_indexed_by_count_star,
		?select_distinct_via_index,
		?update_through_index,
		?delete_through_index,
		?analyze_stale,
		?reindex_stale,
		?runtime_corrupt_observed,
		stale_commit_observed,
		"warm pidx repro: final reopen state"
	);
	eprintln!(
		"warm-pidx-repro: integrity={integrity:?} quick_check={quick_check:?} \
		 schema_after_reopen={schema_after_reopen:?} row_count={row_count:?} \
		 select_indexed_by_grouped={select_indexed_by_grouped:?} \
		 select_indexed_by_count_star={select_indexed_by_count_star:?} \
		 select_distinct_via_index={select_distinct_via_index:?} \
		 update_through_index={update_through_index:?} \
		 delete_through_index={delete_through_index:?} \
		 analyze_stale={analyze_stale:?} reindex_stale={reindex_stale:?} \
		 runtime_corrupt_observed={runtime_corrupt_observed:?} \
		 stale_commit_observed={stale_commit_observed}"
	);
	for (label, result) in &runtime_probes {
		eprintln!("warm-pidx-repro: probe[{label}] = {result:?}");
	}
	for (label, result) in &escalation_probes {
		eprintln!("warm-pidx-repro: escalation[{label}] = {result:?}");
	}

	// Primary assertion: detect any malformed-DB signature.
	//
	// Any non-"ok" result from `PRAGMA integrity_check` or `PRAGMA quick_check` is a
	// malformed-DB signal; SQLite returns "ok" exactly when both pass. SQLite reports
	// btree structure violations (e.g. "Tree N page X cell Y: 2nd reference to page Z"),
	// orphan rows, dangling pointers, header mismatches, etc. as multi-line text.
	let malformed_keywords = [
		"malformed",
		"corrupt",
		"wrong page type",
		"reference to page",
		"out of order",
		"page is never used",
		"missing from index",
		"row count",
		"unordered",
	];
	let is_malformed_text = |text: &str| -> bool {
		text != "ok"
			&& (malformed_keywords.iter().any(|k| text.contains(k))
				|| text.contains("***")
				|| text.lines().count() > 1)
	};
	let mut malformed = false;
	for probe in [&integrity, &quick_check] {
		match probe {
			Ok(text) => {
				if is_malformed_text(text) {
					malformed = true;
				}
			}
			Err(message) => {
				if malformed_keywords.iter().any(|k| message.contains(k)) {
					malformed = true;
				}
			}
		}
	}
	if let Err(message) = &row_count {
		if malformed_keywords.iter().any(|k| message.contains(k)) {
			malformed = true;
		}
	}
	if let Err(message) = &schema_after_reopen {
		if malformed_keywords.iter().any(|k| message.contains(k)) {
			malformed = true;
		}
	}
	if runtime_corrupt_observed.is_some() {
		malformed = true;
	}

	if malformed {
		panic!(
			"REPRO: warm-pidx-cache hazard produced a malformed DB after reopen. \
			 integrity={integrity:?} quick_check={quick_check:?} \
			 schema_after_reopen={schema_after_reopen:?} row_count={row_count:?} \
			 runtime_corrupt_observed={runtime_corrupt_observed:?}. \
			 Likely landing site: depot-client/src/vfs.rs (VFS adopts response head_txid \
			 unconditionally around line 1396-1399) + depot/src/conveyer/read.rs:413 \
			 (warm PIDX returns stale bytes paired with current head)."
		);
	}

	// No repro on this run. The test stands as a documented attempt and regression guard;
	// see the doc comment at the top for production landing sites. Variations to try next:
	//   - VACUUM through `db_writer` to renumber pages, then warm-stale-read a btree
	//     internal page rather than sqlite_master.
	//   - Larger DB (>= 20k rows) so the freelist trunk page is referenced by the warm
	//     cache and the stale RMW lands on the freelist instead of sqlite_master.
	//   - Force PRAGMA auto_vacuum = FULL during open so page renumbering happens
	//     transparently after the writer's DELETE.
	//   - Replicate the production batch-atomic probe sequence (CREATE TABLE
	//     __rivet_batch_probe / INSERT / DELETE / DROP) exactly to mirror the 30-second
	//     post-Started window.
	// Note: once handle B's open-time read populates state.head_txid from the stale warm
	// cache response, all subsequent get_pages still pass the fence (the response
	// advertised the current head). The dangerous `(stale bytes, current head)` pair is
	// what the test deliberately preserves; if the underlying repro hits it should show up
	// in integrity_check or schema queries after the fresh reopen above.
	tracing::info!(
		"warm pidx repro: no malformed DB observed on this run; see eprintln for details"
	);
}

#[test]
fn warm_pidx_stale_read_then_rmw_commit_natural_repro() {
	// Sibling of `warm_pidx_stale_read_then_rmw_commit_produces_malformed_db` that
	// removes the artificial Step 1c warm-up call (`db_shared.get_pages_with_options`).
	// Production code never calls `get_pages_with_options` directly; the warm PIDX cache
	// is supposed to be populated naturally by commits flowing through `db_shared`
	// (see `apply.rs` `pidx.insert(pgno, result.txid)` for each dirty pgno on commit).
	// This test checks whether the warm-cache hazard reproduces under those natural
	// conditions, i.e. with cache entries only from commit-side population.
	use depot::conveyer::Db;
	use depot::types::{CommitOptions, DirtyPage};
	use rivet_pools::__rivet_util::Id;
	use rivet_pools::NodeId;

	struct PinnedDbTransport {
		db: Arc<Db>,
		actor_id: String,
	}

	#[async_trait]
	impl SqliteTransport for PinnedDbTransport {
		async fn get_pages(
			&self,
			request: protocol::SqliteGetPagesRequest,
		) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
			assert_eq!(request.actor_id, self.actor_id, "pinned transport actor id");
			match self
				.db
				.get_pages_with_options(
					request.pgnos.clone(),
					depot::types::GetPagesOptions {
						expected_head_txid: request.expected_head_txid,
						..Default::default()
					},
				)
				.await
			{
				Ok(result) => Ok(protocol::SqliteGetPagesResponse::SqliteGetPagesOk(
					protocol::SqliteGetPagesOk {
						pages: result
							.pages
							.into_iter()
							.map(|page| protocol::SqliteFetchedPage {
								pgno: page.pgno,
								bytes: page.bytes,
							})
							.collect(),
						head_txid: Some(result.head_txid),
					},
				)),
				Err(err) => Ok(protocol::SqliteGetPagesResponse::SqliteErrorResponse(
					sqlite_error_response(&err),
				)),
			}
		}

		async fn commit(
			&self,
			request: protocol::SqliteCommitRequest,
		) -> anyhow::Result<protocol::SqliteCommitResponse> {
			assert_eq!(request.actor_id, self.actor_id, "pinned transport actor id");
			let dirty_pages = request
				.dirty_pages
				.into_iter()
				.map(|page| DirtyPage {
					pgno: page.pgno,
					bytes: page.bytes,
				})
				.collect::<Vec<_>>();
			match self
				.db
				.commit_with_options(
					dirty_pages,
					request.db_size_pages,
					request.now_ms,
					CommitOptions {
						expected_head_txid: request.expected_head_txid,
						..Default::default()
					},
				)
				.await
			{
				Ok(result) => Ok(protocol::SqliteCommitResponse::SqliteCommitOk(
					protocol::SqliteCommitOk {
						head_txid: Some(result.head_txid),
					},
				)),
				Err(err) => Ok(protocol::SqliteCommitResponse::SqliteErrorResponse(
					sqlite_error_response(&err),
				)),
			}
		}
	}

	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let udb = engine.depot_database();

	let db_shared = Arc::new(Db::new(
		udb.clone(),
		Id::nil(),
		harness.actor_id.clone(),
		NodeId::new(),
	));
	let db_writer = Arc::new(Db::new(
		udb.clone(),
		Id::nil(),
		harness.actor_id.clone(),
		NodeId::new(),
	));

	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		..VfsConfig::default()
	};

	// Step 1: handle A builds a real schema with enough rows to span btree internal pages.
	// A's commits flow through `db_shared`, populating its warm PIDX cache naturally via
	// the commit-side `pidx.insert(pgno, result.txid)` path.
	let transport_a: Arc<dyn SqliteTransport + Send + Sync> = Arc::new(PinnedDbTransport {
		db: db_shared.clone(),
		actor_id: harness.actor_id.clone(),
	});
	let db_a = harness.open_db_with_transport(
		&runtime,
		transport_a.clone(),
		&harness.actor_id,
		config.clone(),
	);
	sqlite_exec(
		db_a.as_ptr(),
		"CREATE TABLE t1 (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(db_a.as_ptr(), "CREATE INDEX idx_t1_payload ON t1(payload);")
		.expect("create initial index should succeed");
	sqlite_exec(db_a.as_ptr(), "BEGIN;").expect("begin should succeed");
	for id in 1..=2000 {
		sqlite_exec(
			db_a.as_ptr(),
			&format!(
				"INSERT INTO t1 (id, payload) VALUES ({id}, printf('payload-%05d-%s', {id}, hex(zeroblob(64))));"
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db_a.as_ptr(), "COMMIT;").expect("commit should succeed");

	let page_count_after_a = sqlite_query_i64(db_a.as_ptr(), "PRAGMA page_count;")
		.expect("page_count should be readable");
	assert!(
		page_count_after_a >= 32,
		"setup should produce enough pages to span btree internals, got {page_count_after_a}"
	);
	let _row_count =
		sqlite_query_i64(db_a.as_ptr(), "SELECT COUNT(*) FROM t1;").expect("count should succeed");
	let _sample = sqlite_query_i64(db_a.as_ptr(), "SELECT id FROM t1 WHERE id = 1;")
		.expect("indexed lookup should succeed");

	let schema_after_a = sqlite_query_text(
		db_a.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema query should succeed");
	assert!(
		schema_after_a.contains("idx_t1_payload"),
		"baseline schema should include the seed index: {schema_after_a}"
	);
	drop(db_a);

	// NOTE: Step 1c removed. No explicit `db_shared.get_pages_with_options(...)` call.
	// We rely entirely on commit-side `pidx.insert(...)` from handle A's commits to
	// populate `db_shared.cache_snapshot.pidx`.

	// Step 2: an independent `Db` instance commits schema mutations on the same actor.
	let transport_writer: Arc<dyn SqliteTransport + Send + Sync> = Arc::new(PinnedDbTransport {
		db: db_writer.clone(),
		actor_id: harness.actor_id.clone(),
	});
	let db_writer_handle = harness.open_db_with_transport(
		&runtime,
		transport_writer.clone(),
		&harness.actor_id,
		config.clone(),
	);
	sqlite_exec(
		db_writer_handle.as_ptr(),
		"CREATE INDEX idx_t1_payload2 ON t1(payload, id);",
	)
	.expect("writer CREATE INDEX should commit");
	sqlite_exec(db_writer_handle.as_ptr(), "BEGIN;").expect("begin should succeed");
	for id in 1..=200 {
		sqlite_exec(
			db_writer_handle.as_ptr(),
			&format!("UPDATE t1 SET payload = 'writer-rewrite-{id}' WHERE id = {id};"),
		)
		.expect("writer update should succeed");
	}
	sqlite_exec(db_writer_handle.as_ptr(), "COMMIT;").expect("writer commit should succeed");
	let writer_head = runtime
		.block_on(
			db_writer.get_pages_with_options(vec![1], depot::types::GetPagesOptions::default()),
		)
		.expect("writer head fetch should succeed")
		.head_txid;
	drop(db_writer_handle);

	// Diagnostic: inspect the shared `Db` warm PIDX cache. Without the explicit warm-up
	// call, this shows whether commits alone populate the cache for the test's workload.
	let shared_cache_pidx = runtime
		.block_on(db_shared.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let writer_cache_pidx = runtime
		.block_on(db_writer.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let shared_pg1 = shared_cache_pidx
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	let writer_pg1 = writer_cache_pidx
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	let shared_pgnos: Vec<u32> = shared_cache_pidx.iter().map(|(p, _)| *p).collect();
	eprintln!(
		"warm-pidx-natural-repro: db_shared cache rows = {} (pgno 1 owner = {:?}), db_writer cache rows = {} (pgno 1 owner = {:?}), db_shared pgnos = {:?}",
		shared_cache_pidx.len(),
		shared_pg1,
		writer_cache_pidx.len(),
		writer_pg1,
		shared_pgnos,
	);

	// Step 3: open a fresh handle B through `db_shared`.
	let db_b = harness.open_db_with_transport(
		&runtime,
		transport_a.clone(),
		&harness.actor_id,
		config.clone(),
	);
	let schema_seen_by_b = sqlite_query_text(
		db_b.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema query through B should succeed");
	tracing::info!(
		schema_after_a = %schema_after_a,
		schema_seen_by_b = %schema_seen_by_b,
		writer_head = ?writer_head,
		"warm pidx natural repro: schema visibility through stale Db"
	);

	// Step 4: RMW on pgno 1 from the (possibly stale) view.
	let stale_create = sqlite_exec(
		db_b.as_ptr(),
		"CREATE INDEX idx_t1_payload_stale ON t1(payload, id, id);",
	);
	let mut stale_commit_observed = stale_create.is_ok();
	if let Err(message) = &stale_create {
		tracing::info!(?message, "stale RMW create index failed");
	}
	let stale_delete = sqlite_exec(db_b.as_ptr(), "DELETE FROM t1 WHERE id <= 50;");
	if stale_delete.is_ok() {
		stale_commit_observed = true;
	}

	drop(db_b);
	drop(db_shared);
	drop(db_writer);
	runtime.block_on(engine.evict_actor_db(&harness.actor_id));

	// Step 5: fresh reopen with no warm cache. Run integrity check + schema scan + the
	// post-reopen probes that surface `database disk image is malformed` at runtime.
	let db_c = harness.open_db_on_engine(&runtime, engine, &harness.actor_id, config);
	let integrity = sqlite_query_text(db_c.as_ptr(), "PRAGMA integrity_check;");
	let quick_check = sqlite_query_text(db_c.as_ptr(), "PRAGMA quick_check;");
	let schema_after_reopen = sqlite_query_text(
		db_c.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	);
	let row_count = sqlite_query_i64(db_c.as_ptr(), "SELECT COUNT(*) FROM t1;");

	// Ordering matters: passive scans before REINDEX (which would mask malformation).
	let select_indexed_by_grouped = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM (SELECT payload, COUNT(*) FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00000' AND 'payload-99999' GROUP BY payload);",
	);
	let select_indexed_by_count_star = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00000' AND 'payload-99999';",
	);
	let select_distinct_via_index = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM (SELECT DISTINCT payload FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload >= 'payload-');",
	);
	let update_through_index = sqlite_exec(
		db_c.as_ptr(),
		"UPDATE t1 SET payload = payload || '-tag' WHERE payload BETWEEN 'payload-00000' AND 'payload-99999';",
	);
	let delete_through_index = sqlite_exec(
		db_c.as_ptr(),
		"DELETE FROM t1 WHERE payload IN (SELECT payload FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00500' AND 'payload-00600');",
	);
	let analyze_stale = sqlite_exec(db_c.as_ptr(), "ANALYZE idx_t1_payload_stale;");
	let reindex_stale = sqlite_exec(db_c.as_ptr(), "REINDEX idx_t1_payload_stale;");

	let runtime_probes: [(&str, Result<String, String>); 7] = [
		(
			"select_indexed_by_grouped",
			select_indexed_by_grouped
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"select_indexed_by_count_star",
			select_indexed_by_count_star
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"select_distinct_via_index",
			select_distinct_via_index
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"update_through_index",
			update_through_index
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"delete_through_index",
			delete_through_index
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"analyze",
			analyze_stale
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"reindex_stale",
			reindex_stale
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
	];

	let mut runtime_corrupt_observed: Option<String> = None;
	for (label, result) in &runtime_probes {
		if let Err(message) = result {
			if message.contains("database disk image is malformed")
				&& runtime_corrupt_observed.is_none()
			{
				runtime_corrupt_observed = Some(format!("{label}: {message}"));
			}
		}
	}

	let mut escalation_probes: Vec<(String, Result<String, String>)> = Vec::new();
	if runtime_corrupt_observed.is_none() {
		let pragma_writable_schema_on = sqlite_exec(db_c.as_ptr(), "PRAGMA writable_schema = ON;");
		escalation_probes.push((
			"pragma_writable_schema_on".to_string(),
			pragma_writable_schema_on
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		));
		for boundary_id in [125u32, 126, 127, 250, 251, 252, 375, 376, 377] {
			let q = format!(
				"SELECT id FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload = printf('payload-%05d-%s', {boundary_id}, hex(zeroblob(64)));"
			);
			let result = sqlite_query_i64(db_c.as_ptr(), &q);
			escalation_probes.push((
				format!("direct_page165_boundary_{boundary_id}"),
				result
					.as_ref()
					.map(|v| v.to_string())
					.map_err(|e| e.clone()),
			));
		}
		for (label, result) in &escalation_probes {
			if let Err(message) = result {
				if message.contains("database disk image is malformed")
					&& runtime_corrupt_observed.is_none()
				{
					runtime_corrupt_observed = Some(format!("{label}: {message}"));
				}
			}
		}
	}

	tracing::warn!(
		?integrity,
		?quick_check,
		?schema_after_reopen,
		?row_count,
		?select_indexed_by_grouped,
		?select_indexed_by_count_star,
		?select_distinct_via_index,
		?update_through_index,
		?delete_through_index,
		?analyze_stale,
		?reindex_stale,
		?runtime_corrupt_observed,
		stale_commit_observed,
		"warm pidx natural repro: final reopen state"
	);
	eprintln!(
		"warm-pidx-natural-repro: integrity={integrity:?} quick_check={quick_check:?} \
		 schema_after_reopen={schema_after_reopen:?} row_count={row_count:?} \
		 select_indexed_by_grouped={select_indexed_by_grouped:?} \
		 select_indexed_by_count_star={select_indexed_by_count_star:?} \
		 select_distinct_via_index={select_distinct_via_index:?} \
		 update_through_index={update_through_index:?} \
		 delete_through_index={delete_through_index:?} \
		 analyze_stale={analyze_stale:?} reindex_stale={reindex_stale:?} \
		 runtime_corrupt_observed={runtime_corrupt_observed:?} \
		 stale_commit_observed={stale_commit_observed}"
	);
	for (label, result) in &runtime_probes {
		eprintln!("warm-pidx-natural-repro: probe[{label}] = {result:?}");
	}
	for (label, result) in &escalation_probes {
		eprintln!("warm-pidx-natural-repro: escalation[{label}] = {result:?}");
	}

	let malformed_keywords = [
		"malformed",
		"corrupt",
		"wrong page type",
		"reference to page",
		"out of order",
		"page is never used",
		"missing from index",
		"row count",
		"unordered",
	];
	let is_malformed_text = |text: &str| -> bool {
		text != "ok"
			&& (malformed_keywords.iter().any(|k| text.contains(k))
				|| text.contains("***")
				|| text.lines().count() > 1)
	};
	let mut malformed = false;
	for probe in [&integrity, &quick_check] {
		match probe {
			Ok(text) => {
				if is_malformed_text(text) {
					malformed = true;
				}
			}
			Err(message) => {
				if malformed_keywords.iter().any(|k| message.contains(k)) {
					malformed = true;
				}
			}
		}
	}
	if let Err(message) = &row_count {
		if malformed_keywords.iter().any(|k| message.contains(k)) {
			malformed = true;
		}
	}
	if let Err(message) = &schema_after_reopen {
		if malformed_keywords.iter().any(|k| message.contains(k)) {
			malformed = true;
		}
	}
	if runtime_corrupt_observed.is_some() {
		malformed = true;
	}

	if malformed {
		panic!(
			"REPRO (natural): warm-pidx-cache hazard produced a malformed DB after reopen \
			 WITHOUT explicit cache warm-up. integrity={integrity:?} quick_check={quick_check:?} \
			 schema_after_reopen={schema_after_reopen:?} row_count={row_count:?} \
			 runtime_corrupt_observed={runtime_corrupt_observed:?}."
		);
	}

	tracing::info!(
		"warm pidx natural repro: no malformed DB observed on this run; see eprintln for details"
	);
}

#[test]
fn warm_pidx_stale_read_then_rmw_commit_via_natural_reopen() {
	// Sibling of `warm_pidx_stale_read_then_rmw_commit_natural_repro` that populates
	// `db_shared.cache_snapshot.pidx` entirely through natural VFS open/SELECT/commit
	// paths. The previous natural test showed `db_shared cache rows = 0` because
	// commits route through `PinnedDbTransport::commit` -> `db_shared.commit_with_options`,
	// which populates the writer-side cache through `apply.rs`, but the `db_shared` cache
	// remained empty because none of A's commits routed back through `db_shared`'s own
	// get_pages path during steady-state SQLite operations (VFS-level caches absorbed
	// them).
	//
	// This variant adds a "handle A_warm" step: an extra VFS handle opened through
	// `db_shared` after A1's commits land. Its startup preload calls
	// `db_shared.get_pages_with_options(vec![1..=N], expected_head_txid=None)` through
	// the natural `fetch_initial_pages_for_registration` path, which populates
	// `db_shared.cache_snapshot.pidx`. A few representative SELECTs follow to give the
	// VFS more cache misses to route through depot. Then A_warm drops, the writer
	// advances FDB head, and handle C opens through the still-warm-but-now-stale
	// `db_shared`.
	//
	// No explicit `db_shared.get_pages_with_options(...)` is called outside the
	// transport. The cache is populated entirely by the production-equivalent startup
	// preload path.
	use depot::conveyer::Db;
	use depot::types::{CommitOptions, DirtyPage};
	use rivet_pools::__rivet_util::Id;
	use rivet_pools::NodeId;

	struct PinnedDbTransport {
		db: Arc<Db>,
		actor_id: String,
	}

	#[async_trait]
	impl SqliteTransport for PinnedDbTransport {
		async fn get_pages(
			&self,
			request: protocol::SqliteGetPagesRequest,
		) -> anyhow::Result<protocol::SqliteGetPagesResponse> {
			assert_eq!(request.actor_id, self.actor_id, "pinned transport actor id");
			match self
				.db
				.get_pages_with_options(
					request.pgnos.clone(),
					depot::types::GetPagesOptions {
						expected_head_txid: request.expected_head_txid,
						..Default::default()
					},
				)
				.await
			{
				Ok(result) => Ok(protocol::SqliteGetPagesResponse::SqliteGetPagesOk(
					protocol::SqliteGetPagesOk {
						pages: result
							.pages
							.into_iter()
							.map(|page| protocol::SqliteFetchedPage {
								pgno: page.pgno,
								bytes: page.bytes,
							})
							.collect(),
						head_txid: Some(result.head_txid),
					},
				)),
				Err(err) => Ok(protocol::SqliteGetPagesResponse::SqliteErrorResponse(
					sqlite_error_response(&err),
				)),
			}
		}

		async fn commit(
			&self,
			request: protocol::SqliteCommitRequest,
		) -> anyhow::Result<protocol::SqliteCommitResponse> {
			assert_eq!(request.actor_id, self.actor_id, "pinned transport actor id");
			let dirty_pages = request
				.dirty_pages
				.into_iter()
				.map(|page| DirtyPage {
					pgno: page.pgno,
					bytes: page.bytes,
				})
				.collect::<Vec<_>>();
			match self
				.db
				.commit_with_options(
					dirty_pages,
					request.db_size_pages,
					request.now_ms,
					CommitOptions {
						expected_head_txid: request.expected_head_txid,
						disable_size_cap: false,
					},
				)
				.await
			{
				Ok(result) => Ok(protocol::SqliteCommitResponse::SqliteCommitOk(
					protocol::SqliteCommitOk {
						head_txid: Some(result.head_txid),
					},
				)),
				Err(err) => Ok(protocol::SqliteCommitResponse::SqliteErrorResponse(
					sqlite_error_response(&err),
				)),
			}
		}
	}

	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let udb = engine.depot_database();

	let db_shared = Arc::new(Db::new(
		udb.clone(),
		Id::nil(),
		harness.actor_id.clone(),
		NodeId::new(),
	));
	let db_writer = Arc::new(Db::new(
		udb.clone(),
		Id::nil(),
		harness.actor_id.clone(),
		NodeId::new(),
	));

	// Pump the preload up so the startup preload requests multiple pages, giving the
	// natural warming step a wider footprint to populate `db_shared.cache_snapshot.pidx`.
	let config = VfsConfig {
		page_cache_mode: SqliteVfsPageCacheMode::All,
		startup_preload_first_pages: true,
		startup_preload_first_page_count: 32,
		startup_preload_max_bytes: 32 * 4096,
		..VfsConfig::default()
	};

	let transport_shared: Arc<dyn SqliteTransport + Send + Sync> = Arc::new(PinnedDbTransport {
		db: db_shared.clone(),
		actor_id: harness.actor_id.clone(),
	});

	// Step 1: handle A1 builds a real schema with enough rows to span btree internals.
	let db_a = harness.open_db_with_transport(
		&runtime,
		transport_shared.clone(),
		&harness.actor_id,
		config.clone(),
	);
	sqlite_exec(
		db_a.as_ptr(),
		"CREATE TABLE t1 (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(db_a.as_ptr(), "CREATE INDEX idx_t1_payload ON t1(payload);")
		.expect("create initial index should succeed");
	sqlite_exec(db_a.as_ptr(), "BEGIN;").expect("begin should succeed");
	for id in 1..=2000 {
		sqlite_exec(
			db_a.as_ptr(),
			&format!(
				"INSERT INTO t1 (id, payload) VALUES ({id}, printf('payload-%05d-%s', {id}, hex(zeroblob(64))));"
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db_a.as_ptr(), "COMMIT;").expect("commit should succeed");

	let page_count_after_a = sqlite_query_i64(db_a.as_ptr(), "PRAGMA page_count;")
		.expect("page_count should be readable");
	assert!(
		page_count_after_a >= 32,
		"setup should produce enough pages to span btree internals, got {page_count_after_a}"
	);
	let schema_after_a = sqlite_query_text(
		db_a.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema query should succeed");
	assert!(
		schema_after_a.contains("idx_t1_payload"),
		"baseline schema should include the seed index: {schema_after_a}"
	);
	drop(db_a);

	// Step 1.5: open A_warm through `db_shared` and let the startup preload populate
	// `db_shared.cache_snapshot.pidx` naturally via `fetch_initial_pages_for_registration`.
	// Also run representative SELECTs to widen the VFS cache miss footprint, sending more
	// `get_pages` requests through `db_shared`.
	let db_a_warm = harness.open_db_with_transport(
		&runtime,
		transport_shared.clone(),
		&harness.actor_id,
		config.clone(),
	);
	let _ = sqlite_query_i64(db_a_warm.as_ptr(), "SELECT COUNT(*) FROM t1;")
		.expect("count through A_warm should succeed");
	let _ = sqlite_query_i64(
		db_a_warm.as_ptr(),
		"SELECT id FROM t1 WHERE payload LIKE 'payload-00500%' LIMIT 1;",
	)
	.expect("indexed lookup through A_warm should succeed");
	let _ = sqlite_query_i64(db_a_warm.as_ptr(), "SELECT id FROM t1 WHERE id = 1;")
		.expect("pk lookup through A_warm should succeed");
	let _ = sqlite_query_text(
		db_a_warm.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema scan through A_warm should succeed");
	drop(db_a_warm);

	// Snapshot db_shared.cache_snapshot.pidx after the natural warming step so we know
	// whether the startup preload alone is enough to populate it.
	let shared_cache_after_warm = runtime
		.block_on(db_shared.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let shared_pg1_after_warm = shared_cache_after_warm
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	let shared_pgnos_after_warm: Vec<u32> =
		shared_cache_after_warm.iter().map(|(p, _)| *p).collect();
	eprintln!(
		"warm-pidx-natural-reopen: after A_warm db_shared cache rows = {} (pgno 1 owner = {:?}), pgnos = {:?}",
		shared_cache_after_warm.len(),
		shared_pg1_after_warm,
		shared_pgnos_after_warm,
	);

	// Step 2: independent `Db` instance commits schema mutations on the same actor.
	let transport_writer: Arc<dyn SqliteTransport + Send + Sync> = Arc::new(PinnedDbTransport {
		db: db_writer.clone(),
		actor_id: harness.actor_id.clone(),
	});
	let db_writer_handle = harness.open_db_with_transport(
		&runtime,
		transport_writer.clone(),
		&harness.actor_id,
		config.clone(),
	);
	sqlite_exec(
		db_writer_handle.as_ptr(),
		"CREATE INDEX idx_t1_payload2 ON t1(payload, id);",
	)
	.expect("writer CREATE INDEX should commit");
	sqlite_exec(db_writer_handle.as_ptr(), "BEGIN;").expect("begin should succeed");
	for id in 1..=200 {
		sqlite_exec(
			db_writer_handle.as_ptr(),
			&format!("UPDATE t1 SET payload = 'writer-rewrite-{id}' WHERE id = {id};"),
		)
		.expect("writer update should succeed");
	}
	sqlite_exec(db_writer_handle.as_ptr(), "COMMIT;").expect("writer commit should succeed");
	let writer_head = runtime
		.block_on(
			db_writer.get_pages_with_options(vec![1], depot::types::GetPagesOptions::default()),
		)
		.expect("writer head fetch should succeed")
		.head_txid;
	drop(db_writer_handle);

	// Diagnostic: inspect the shared `Db` warm PIDX cache. Step 2 must NOT have touched
	// it. If `shared_cache_after_warm` matches what we see now and writer pgno 1 owner is
	// fresher, the hazard is set up.
	let shared_cache_pidx = runtime
		.block_on(db_shared.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let writer_cache_pidx = runtime
		.block_on(db_writer.branch_cache_snapshot_for_test())
		.map(|(_, _, _, pidx)| pidx)
		.unwrap_or_default();
	let shared_pg1 = shared_cache_pidx
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	let writer_pg1 = writer_cache_pidx
		.iter()
		.find(|(pgno, _)| *pgno == 1)
		.copied();
	let shared_pgnos: Vec<u32> = shared_cache_pidx.iter().map(|(p, _)| *p).collect();
	eprintln!(
		"warm-pidx-natural-reopen: db_shared cache rows = {} (pgno 1 owner = {:?}), db_writer cache rows = {} (pgno 1 owner = {:?}), db_shared pgnos = {:?}",
		shared_cache_pidx.len(),
		shared_pg1,
		writer_cache_pidx.len(),
		writer_pg1,
		shared_pgnos,
	);

	// Step 3: open a fresh handle B (the "C" of the spec, but kept as `db_b` for symmetry
	// with the sibling test) through the still-warm `db_shared`. Startup preload sees
	// `db_shared.cache_snapshot.pidx` with stale pgno 1 entries.
	let db_b = harness.open_db_with_transport(
		&runtime,
		transport_shared.clone(),
		&harness.actor_id,
		config.clone(),
	);
	let schema_seen_by_b = sqlite_query_text(
		db_b.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	)
	.expect("schema query through B should succeed");
	tracing::info!(
		schema_after_a = %schema_after_a,
		schema_seen_by_b = %schema_seen_by_b,
		writer_head = ?writer_head,
		"warm pidx natural reopen repro: schema visibility through stale Db"
	);

	// Step 4: RMW on pgno 1 from the (possibly stale) view.
	let stale_create = sqlite_exec(
		db_b.as_ptr(),
		"CREATE INDEX idx_t1_payload_stale ON t1(payload, id, id);",
	);
	let mut stale_commit_observed = stale_create.is_ok();
	if let Err(message) = &stale_create {
		tracing::info!(?message, "stale RMW create index failed");
	}
	let stale_delete = sqlite_exec(db_b.as_ptr(), "DELETE FROM t1 WHERE id <= 50;");
	if stale_delete.is_ok() {
		stale_commit_observed = true;
	}

	drop(db_b);
	drop(db_shared);
	drop(db_writer);
	runtime.block_on(engine.evict_actor_db(&harness.actor_id));

	// Step 5: fresh reopen with no warm cache. Run integrity check + schema scan + the
	// post-reopen probes that surface `database disk image is malformed` at runtime.
	let db_c = harness.open_db_on_engine(&runtime, engine, &harness.actor_id, config);
	let integrity = sqlite_query_text(db_c.as_ptr(), "PRAGMA integrity_check;");
	let quick_check = sqlite_query_text(db_c.as_ptr(), "PRAGMA quick_check;");
	let schema_after_reopen = sqlite_query_text(
		db_c.as_ptr(),
		"SELECT group_concat(name, ',') FROM sqlite_master ORDER BY name;",
	);
	let row_count = sqlite_query_i64(db_c.as_ptr(), "SELECT COUNT(*) FROM t1;");

	let select_indexed_by_grouped = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM (SELECT payload, COUNT(*) FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00000' AND 'payload-99999' GROUP BY payload);",
	);
	let select_indexed_by_count_star = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00000' AND 'payload-99999';",
	);
	let select_distinct_via_index = sqlite_query_i64(
		db_c.as_ptr(),
		"SELECT COUNT(*) FROM (SELECT DISTINCT payload FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload >= 'payload-');",
	);
	let update_through_index = sqlite_exec(
		db_c.as_ptr(),
		"UPDATE t1 SET payload = payload || '-tag' WHERE payload BETWEEN 'payload-00000' AND 'payload-99999';",
	);
	let delete_through_index = sqlite_exec(
		db_c.as_ptr(),
		"DELETE FROM t1 WHERE payload IN (SELECT payload FROM t1 INDEXED BY idx_t1_payload_stale WHERE payload BETWEEN 'payload-00500' AND 'payload-00600');",
	);
	let analyze_stale = sqlite_exec(db_c.as_ptr(), "ANALYZE idx_t1_payload_stale;");
	let reindex_stale = sqlite_exec(db_c.as_ptr(), "REINDEX idx_t1_payload_stale;");

	let runtime_probes: [(&str, Result<String, String>); 7] = [
		(
			"select_indexed_by_grouped",
			select_indexed_by_grouped
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"select_indexed_by_count_star",
			select_indexed_by_count_star
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"select_distinct_via_index",
			select_distinct_via_index
				.as_ref()
				.map(|v| v.to_string())
				.map_err(|e| e.clone()),
		),
		(
			"update_through_index",
			update_through_index
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"delete_through_index",
			delete_through_index
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"analyze",
			analyze_stale
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
		(
			"reindex_stale",
			reindex_stale
				.as_ref()
				.map(|_| String::new())
				.map_err(|e| e.clone()),
		),
	];

	let mut runtime_corrupt_observed: Option<String> = None;
	for (label, result) in &runtime_probes {
		if let Err(message) = result {
			if message.contains("database disk image is malformed")
				&& runtime_corrupt_observed.is_none()
			{
				runtime_corrupt_observed = Some(format!("{label}: {message}"));
			}
		}
	}

	tracing::warn!(
		?integrity,
		?quick_check,
		?schema_after_reopen,
		?row_count,
		?select_indexed_by_grouped,
		?select_indexed_by_count_star,
		?select_distinct_via_index,
		?update_through_index,
		?delete_through_index,
		?analyze_stale,
		?reindex_stale,
		?runtime_corrupt_observed,
		stale_commit_observed,
		"warm pidx natural reopen repro: final reopen state"
	);
	eprintln!(
		"warm-pidx-natural-reopen: integrity={integrity:?} quick_check={quick_check:?} \
		 schema_after_reopen={schema_after_reopen:?} row_count={row_count:?} \
		 select_indexed_by_grouped={select_indexed_by_grouped:?} \
		 select_indexed_by_count_star={select_indexed_by_count_star:?} \
		 select_distinct_via_index={select_distinct_via_index:?} \
		 update_through_index={update_through_index:?} \
		 delete_through_index={delete_through_index:?} \
		 analyze_stale={analyze_stale:?} reindex_stale={reindex_stale:?} \
		 runtime_corrupt_observed={runtime_corrupt_observed:?} \
		 stale_commit_observed={stale_commit_observed}"
	);
	for (label, result) in &runtime_probes {
		eprintln!("warm-pidx-natural-reopen: probe[{label}] = {result:?}");
	}

	let malformed_keywords = [
		"malformed",
		"corrupt",
		"wrong page type",
		"reference to page",
		"out of order",
		"page is never used",
		"missing from index",
		"row count",
		"unordered",
	];
	let is_malformed_text = |text: &str| -> bool {
		text != "ok"
			&& (malformed_keywords.iter().any(|k| text.contains(k))
				|| text.contains("***")
				|| text.lines().count() > 1)
	};
	let mut malformed = false;
	for probe in [&integrity, &quick_check] {
		match probe {
			Ok(text) => {
				if is_malformed_text(text) {
					malformed = true;
				}
			}
			Err(message) => {
				if malformed_keywords.iter().any(|k| message.contains(k)) {
					malformed = true;
				}
			}
		}
	}
	if let Err(message) = &row_count {
		if malformed_keywords.iter().any(|k| message.contains(k)) {
			malformed = true;
		}
	}
	if let Err(message) = &schema_after_reopen {
		if malformed_keywords.iter().any(|k| message.contains(k)) {
			malformed = true;
		}
	}
	if runtime_corrupt_observed.is_some() {
		malformed = true;
	}

	if malformed {
		panic!(
			"REPRO (natural reopen): warm-pidx-cache hazard produced a malformed DB after reopen \
			 with cache populated entirely by VFS startup preload. integrity={integrity:?} \
			 quick_check={quick_check:?} schema_after_reopen={schema_after_reopen:?} \
			 row_count={row_count:?} runtime_corrupt_observed={runtime_corrupt_observed:?}."
		);
	}

	tracing::info!(
		"warm pidx natural reopen repro: no malformed DB observed on this run; see eprintln for details"
	);
}

#[test]
fn commit_buffered_pages_uses_fast_path() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let transport = Arc::new(DirectDepotTransport::new(engine));
	let hooks = transport.direct_hooks();

	let outcome = runtime
		.block_on(commit_buffered_pages(
			&*transport,
			BufferedCommitRequest {
				actor_id: harness.actor_id.clone(),
				new_db_size_pages: 1,
				expected_head_txid: None,
				dirty_pages: vec![protocol::SqliteDirtyPage {
					pgno: 1,
					bytes: empty_db_page(),
				}],
			},
		))
		.expect("fast-path commit should succeed");
	let (outcome, metrics) = outcome;

	assert_eq!(outcome.path, CommitPath::Fast);
	assert_eq!(outcome.db_size_pages, 1);
	assert!(metrics.serialize_ns > 0);
	assert!(metrics.transport_ns > 0);
	assert_eq!(hooks.commit_requests().len(), 1);
}

#[test]
fn vfs_records_commit_phase_durations() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE metrics_test (id INTEGER PRIMARY KEY, value TEXT NOT NULL);",
	)
	.expect("create table should succeed");

	let relaxed = std::sync::atomic::Ordering::Relaxed;
	ctx.commit_request_build_ns.store(0, relaxed);
	ctx.commit_serialize_ns.store(0, relaxed);
	ctx.commit_transport_ns.store(0, relaxed);
	ctx.commit_state_update_ns.store(0, relaxed);
	ctx.commit_duration_ns_total.store(0, relaxed);
	ctx.commit_total.store(0, relaxed);

	sqlite_exec(
		db.as_ptr(),
		"INSERT INTO metrics_test (id, value) VALUES (1, 'hello');",
	)
	.expect("insert should succeed");

	let metrics = db.sqlite_vfs_metrics();
	assert_eq!(metrics.commit_count, 1);
	assert!(metrics.request_build_ns > 0);
	assert!(metrics.serialize_ns > 0);
	assert!(metrics.transport_ns > 0);
	assert!(metrics.state_update_ns > 0);
	assert!(metrics.total_ns >= metrics.request_build_ns);
	assert!(metrics.request_build_ns + metrics.transport_ns + metrics.state_update_ns > 0);
}

#[test]
fn partial_status_index_reduces_storage_and_cold_page_fetches() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let relaxed = std::sync::atomic::Ordering::Relaxed;

	let measure = |actor_id: &str, index_sql: &str| {
		let db =
			harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE history (id INTEGER PRIMARY KEY, result INTEGER NOT NULL, payload BLOB NOT NULL);",
		)
		.expect("create history table");
		sqlite_exec(db.as_ptr(), index_sql).expect("create history index");
		sqlite_exec(
			db.as_ptr(),
			"WITH RECURSIVE seq(id) AS (SELECT 1 UNION ALL SELECT id + 1 FROM seq WHERE id < 4000) INSERT INTO history (id, result, payload) SELECT id, CASE WHEN id = 1 THEN 0 ELSE 1 END, randomblob(256) FROM seq;",
		)
		.expect("populate history table");
		let page_count =
			sqlite_query_i64(db.as_ptr(), "PRAGMA page_count;").expect("read database page count");
		drop(db);

		let reopened =
			harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
		let ctx = direct_vfs_ctx(&reopened);
		ctx.resolve_pages_fetches.store(0, relaxed);
		ctx.pages_fetched_total.store(0, relaxed);
		sqlite_exec(
			reopened.as_ptr(),
			"UPDATE history SET payload = randomblob(256) WHERE result = 0;",
		)
		.expect("update running history row");
		(
			page_count,
			ctx.resolve_pages_fetches.load(relaxed),
			ctx.pages_fetched_total.load(relaxed),
		)
	};

	let full = measure(
		&next_test_name("sqlite-full-status-index"),
		"CREATE INDEX history_result ON history (result);",
	);
	let partial = measure(
		&next_test_name("sqlite-partial-status-index"),
		"CREATE INDEX history_running ON history (result) WHERE result = 0;",
	);

	assert!(
		partial.0 < full.0,
		"partial index should persist fewer pages"
	);
	assert!(partial.1 > 0, "cold update should fetch pages from storage");
	assert!(
		partial.1 <= full.1,
		"partial index should not add cold storage round trips: partial={partial:?}, full={full:?}"
	);
	assert!(
		partial.2 <= full.2,
		"partial index should not fetch more cold pages: partial={partial:?}, full={full:?}"
	);
}

#[test]
fn profile_large_tx_insert_5mb() {
	// 5MB = 1280 rows x 4KB blobs in one transaction
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE bench (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create table should succeed");

	let relaxed = std::sync::atomic::Ordering::Relaxed;
	ctx.resolve_pages_total.store(0, relaxed);
	ctx.resolve_pages_cache_hits.store(0, relaxed);
	ctx.resolve_pages_fetches.store(0, relaxed);
	ctx.pages_fetched_total.store(0, relaxed);
	ctx.prefetch_pages_total.store(0, relaxed);
	ctx.commit_total.store(0, relaxed);

	let start = std::time::Instant::now();
	sqlite_exec(db.as_ptr(), "BEGIN;").expect("begin");
	for i in 0..1280 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO bench (id, payload) VALUES ({}, randomblob(4096));",
				i
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db.as_ptr(), "COMMIT;").expect("commit");
	let elapsed = start.elapsed();

	let resolve_total = ctx.resolve_pages_total.load(relaxed);
	let cache_hits = ctx.resolve_pages_cache_hits.load(relaxed);
	let fetches = ctx.resolve_pages_fetches.load(relaxed);
	let pages_fetched = ctx.pages_fetched_total.load(relaxed);
	let prefetch = ctx.prefetch_pages_total.load(relaxed);
	let commits = ctx.commit_total.load(relaxed);

	eprintln!("=== 5MB INSERT PROFILE (1280 rows x 4KB) ===");
	eprintln!("  wall clock:           {:?}", elapsed);
	eprintln!("  resolve_pages calls:  {}", resolve_total);
	eprintln!("  cache hits (pages):   {}", cache_hits);
	eprintln!("  engine fetches:       {}", fetches);
	eprintln!("  pages fetched total:  {}", pages_fetched);
	eprintln!("  prefetch pages:       {}", prefetch);
	eprintln!("  commits:              {}", commits);
	eprintln!("============================================");

	// In a single transaction, all 1280 row writes are to new pages.
	// Only the single commit at the end should hit the engine.
	assert_eq!(
		fetches, 0,
		"expected 0 engine fetches during 5MB insert transaction"
	);
	assert_eq!(
		commits, 1,
		"expected exactly 1 commit for transactional insert"
	);

	let count =
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM bench;").expect("count should succeed");
	assert_eq!(count, 1280);
}

#[test]
fn profile_hot_row_updates() {
	// 100 updates to the same row - this is the autocommit case
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE counter (id INTEGER PRIMARY KEY, value INTEGER NOT NULL);",
	)
	.expect("create");
	sqlite_exec(db.as_ptr(), "INSERT INTO counter VALUES (1, 0);").expect("insert");

	let relaxed = std::sync::atomic::Ordering::Relaxed;
	ctx.resolve_pages_total.store(0, relaxed);
	ctx.resolve_pages_cache_hits.store(0, relaxed);
	ctx.resolve_pages_fetches.store(0, relaxed);
	ctx.pages_fetched_total.store(0, relaxed);
	ctx.prefetch_pages_total.store(0, relaxed);
	ctx.commit_total.store(0, relaxed);

	let start = std::time::Instant::now();
	for _ in 0..100 {
		sqlite_exec(
			db.as_ptr(),
			"UPDATE counter SET value = value + 1 WHERE id = 1;",
		)
		.expect("update");
	}
	let elapsed = start.elapsed();

	let fetches = ctx.resolve_pages_fetches.load(relaxed);
	let commits = ctx.commit_total.load(relaxed);

	eprintln!("=== 100 HOT ROW UPDATES (autocommit) ===");
	eprintln!("  wall clock:           {:?}", elapsed);
	eprintln!(
		"  resolve_pages calls:  {}",
		ctx.resolve_pages_total.load(relaxed)
	);
	eprintln!(
		"  cache hits (pages):   {}",
		ctx.resolve_pages_cache_hits.load(relaxed)
	);
	eprintln!("  engine fetches:       {}", fetches);
	eprintln!(
		"  pages fetched total:  {}",
		ctx.pages_fetched_total.load(relaxed)
	);
	eprintln!(
		"  prefetch pages:       {}",
		ctx.prefetch_pages_total.load(relaxed)
	);
	eprintln!("  commits:              {}", commits);
	eprintln!("=========================================");

	// Hot row updates: each update modifies the same page. Pages already
	// in write_buffer or cache should not need re-fetching. With the
	// counter's page(s) already warm, subsequent updates should be
	// 100% cache hits (0 fetches). Autocommit means 100 separate commits.
	assert_eq!(
		fetches, 0,
		"expected 0 engine fetches for 100 hot row updates"
	);
	assert_eq!(
		commits, 100,
		"expected 100 commits (autocommit per statement)"
	);
}

#[test]
fn profile_large_tx_insert_1mb_preloaded() {
	// Same as the 1MB test but preload all pages first to see commit-only cost
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let actor_id = &harness.actor_id;

	// First pass: create and populate the table to generate pages
	let db1 = harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
	sqlite_exec(
		db1.as_ptr(),
		"CREATE TABLE bench (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create table should succeed");
	sqlite_exec(db1.as_ptr(), "BEGIN;").expect("begin");
	for i in 0..256 {
		sqlite_step_statement(
			db1.as_ptr(),
			&format!(
				"INSERT INTO bench (id, payload) VALUES ({}, randomblob(4096));",
				i
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db1.as_ptr(), "COMMIT;").expect("commit");
	drop(db1);

	// Second pass: reopen with warm cache (takeover preloads page 1, rest from reads)
	let db2 = harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
	let ctx = direct_vfs_ctx(&db2);

	// Warm the cache by reading everything
	sqlite_exec(db2.as_ptr(), "SELECT COUNT(*) FROM bench;").expect("count");

	// Reset counters
	let relaxed = std::sync::atomic::Ordering::Relaxed;
	ctx.resolve_pages_total.store(0, relaxed);
	ctx.resolve_pages_cache_hits.store(0, relaxed);
	ctx.resolve_pages_fetches.store(0, relaxed);
	ctx.pages_fetched_total.store(0, relaxed);
	ctx.prefetch_pages_total.store(0, relaxed);
	ctx.commit_total.store(0, relaxed);

	let start = std::time::Instant::now();
	sqlite_exec(db2.as_ptr(), "BEGIN;").expect("begin");
	for i in 256..512 {
		sqlite_step_statement(
			db2.as_ptr(),
			&format!(
				"INSERT INTO bench (id, payload) VALUES ({}, randomblob(4096));",
				i
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db2.as_ptr(), "COMMIT;").expect("commit");
	let elapsed = start.elapsed();

	let resolve_total = ctx.resolve_pages_total.load(relaxed);
	let cache_hits = ctx.resolve_pages_cache_hits.load(relaxed);
	let fetches = ctx.resolve_pages_fetches.load(relaxed);
	let pages_fetched = ctx.pages_fetched_total.load(relaxed);
	let prefetch = ctx.prefetch_pages_total.load(relaxed);
	let commits = ctx.commit_total.load(relaxed);

	eprintln!("=== 1MB INSERT PROFILE (WARM CACHE) ===");
	eprintln!("  wall clock:           {:?}", elapsed);
	eprintln!("  resolve_pages calls:  {}", resolve_total);
	eprintln!("  cache hits (pages):   {}", cache_hits);
	eprintln!("  engine fetches:       {}", fetches);
	eprintln!("  pages fetched total:  {}", pages_fetched);
	eprintln!("  prefetch pages:       {}", prefetch);
	eprintln!("  commits:              {}", commits);
	eprintln!("========================================");

	// Second 256-row transaction into the already-populated table.
	// All new pages are beyond db_size_pages, so no engine fetches.
	assert_eq!(
		fetches, 0,
		"expected 0 engine fetches during warm 1MB insert"
	);
	assert_eq!(
		commits, 1,
		"expected exactly 1 commit for transactional insert"
	);

	let count = sqlite_query_i64(db2.as_ptr(), "SELECT COUNT(*) FROM bench;")
		.expect("count should succeed");
	assert_eq!(count, 512);
}

#[test]
fn profile_large_tx_insert_1mb() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE bench (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);",
	)
	.expect("create table should succeed");

	// Reset counters after schema setup
	ctx.resolve_pages_total
		.store(0, std::sync::atomic::Ordering::Relaxed);
	ctx.resolve_pages_cache_hits
		.store(0, std::sync::atomic::Ordering::Relaxed);
	ctx.resolve_pages_fetches
		.store(0, std::sync::atomic::Ordering::Relaxed);
	ctx.pages_fetched_total
		.store(0, std::sync::atomic::Ordering::Relaxed);
	ctx.prefetch_pages_total
		.store(0, std::sync::atomic::Ordering::Relaxed);
	ctx.commit_total
		.store(0, std::sync::atomic::Ordering::Relaxed);

	let start = std::time::Instant::now();

	sqlite_exec(db.as_ptr(), "BEGIN;").expect("begin should succeed");
	for i in 0..256 {
		sqlite_step_statement(
			db.as_ptr(),
			&format!(
				"INSERT INTO bench (id, payload) VALUES ({}, randomblob(4096));",
				i
			),
		)
		.expect("insert should succeed");
	}
	sqlite_exec(db.as_ptr(), "COMMIT;").expect("commit should succeed");

	let elapsed = start.elapsed();
	let relaxed = std::sync::atomic::Ordering::Relaxed;

	let resolve_total = ctx.resolve_pages_total.load(relaxed);
	let cache_hits = ctx.resolve_pages_cache_hits.load(relaxed);
	let fetches = ctx.resolve_pages_fetches.load(relaxed);
	let pages_fetched = ctx.pages_fetched_total.load(relaxed);
	let prefetch = ctx.prefetch_pages_total.load(relaxed);
	let commits = ctx.commit_total.load(relaxed);

	eprintln!("=== 1MB INSERT PROFILE (256 rows x 4KB) ===");
	eprintln!("  wall clock:           {:?}", elapsed);
	eprintln!("  resolve_pages calls:  {}", resolve_total);
	eprintln!("  cache hits (pages):   {}", cache_hits);
	eprintln!("  engine fetches:       {}", fetches);
	eprintln!("  pages fetched total:  {}", pages_fetched);
	eprintln!("  prefetch pages:       {}", prefetch);
	eprintln!("  commits:              {}", commits);
	eprintln!("============================================");

	// Assert expected zero-fetch behavior: in a single transaction,
	// all writes are to new pages, so no engine fetches should happen.
	// Only the single commit at the end should hit the engine.
	assert_eq!(
		fetches, 0,
		"expected 0 engine fetches during 1MB insert transaction"
	);
	assert_eq!(
		commits, 1,
		"expected exactly 1 commit for transactional insert"
	);

	let count =
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM bench;").expect("count should succeed");
	assert_eq!(count, 256);
}

// Regression test for fence mismatch during rapid autocommit inserts.
// Each autocommit INSERT is its own transaction. This test drives many
// sequential commits through the VFS and verifies they all succeed.
#[test]
fn autocommit_inserts_maintain_head_txid_consistency() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let db = harness.open_db(&runtime);
	let ctx = direct_vfs_ctx(&db);

	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL);",
	)
	.expect("create table should succeed");

	let relaxed = std::sync::atomic::Ordering::Relaxed;
	ctx.commit_total.store(0, relaxed);

	// 100 sequential autocommit inserts. If fence mismatch is the bug,
	// this will fail partway through with "commit head_txid X did not
	// match current head_txid X-1".
	for i in 0..100 {
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO t (id, v) VALUES ({i}, {});", i * 2),
		)
		.expect("autocommit insert should not fence-mismatch");
	}

	let commits = ctx.commit_total.load(relaxed);
	// Each autocommit INSERT = 1 commit. CREATE TABLE was 1 more.
	// We reset commit_total after CREATE, so expect 100.
	assert_eq!(commits, 100, "expected exactly 100 commits");

	let count =
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM t;").expect("count should succeed");
	assert_eq!(count, 100);

	// Verify the sum to make sure data is correct and not corrupted
	let sum = sqlite_query_i64(db.as_ptr(), "SELECT SUM(v) FROM t;").expect("sum should succeed");
	assert_eq!(sum, (0..100).map(|i| i * 2).sum::<i64>());
}

// Regression test: 5 actors run 200 autocommits each on the same engine.
// Compaction is triggered via the mpsc channel after each commit, so this
// also exercises the commit-vs-compaction race that caused fence rewinds
// before the tx_get_value_serializable fix.
#[test]
fn stress_concurrent_multi_actor_autocommits() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());

	let mut dbs = Vec::new();
	for i in 0..5 {
		let actor_id = format!("{}-stress-{}", harness.actor_id, i);
		let db =
			harness.open_db_on_engine(&runtime, engine.clone(), &actor_id, VfsConfig::default());
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL);",
		)
		.expect("create");
		dbs.push(db);
	}

	// Interleave 200 autocommit inserts across all 5 actors
	for i in 0..200 {
		for db in &dbs {
			sqlite_exec(
				db.as_ptr(),
				&format!("INSERT INTO t (id, v) VALUES ({i}, {i});"),
			)
			.expect("insert");
		}
	}

	for db in &dbs {
		let count = sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM t;").expect("count");
		assert_eq!(count, 200);
	}
}

// Regression test: two actors run autocommits concurrently on the same
// direct storage. If compaction cross-contaminates actors or races on
// shared state, we'd see fence mismatches.
#[test]
fn concurrent_multi_actor_autocommits() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());

	let actor_a = format!("{}-a", harness.actor_id);
	let actor_b = format!("{}-b", harness.actor_id);

	let db_a = harness.open_db_on_engine(&runtime, engine.clone(), &actor_a, VfsConfig::default());
	let db_b = harness.open_db_on_engine(&runtime, engine.clone(), &actor_b, VfsConfig::default());

	sqlite_exec(
		db_a.as_ptr(),
		"CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL);",
	)
	.expect("create a");
	sqlite_exec(
		db_b.as_ptr(),
		"CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL);",
	)
	.expect("create b");

	// Run 100 autocommits on each actor, interleaved.
	for i in 0..100 {
		sqlite_exec(
			db_a.as_ptr(),
			&format!("INSERT INTO t (id, v) VALUES ({i}, {i});"),
		)
		.expect("insert a");
		sqlite_exec(
			db_b.as_ptr(),
			&format!("INSERT INTO t (id, v) VALUES ({i}, {i});"),
		)
		.expect("insert b");
	}

	let count_a = sqlite_query_i64(db_a.as_ptr(), "SELECT COUNT(*) FROM t;").expect("count a");
	assert_eq!(count_a, 100);
	let count_b = sqlite_query_i64(db_b.as_ptr(), "SELECT COUNT(*) FROM t;").expect("count b");
	assert_eq!(count_b, 100);
}

// Same as above but across a close/reopen cycle to exercise takeover.
#[test]
fn autocommit_survives_close_reopen() {
	let runtime = direct_runtime();
	let harness = DirectEngineHarness::new();
	let engine = runtime.block_on(harness.open_engine());
	let actor_id = &harness.actor_id;

	{
		let db =
			harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
		sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER NOT NULL);",
		)
		.expect("create table");
		for i in 0..50 {
			sqlite_exec(
				db.as_ptr(),
				&format!("INSERT INTO t (id, v) VALUES ({i}, {});", i),
			)
			.expect("insert");
		}
	}

	// Reopen (triggers takeover which bumps generation)
	let db2 = harness.open_db_on_engine(&runtime, engine.clone(), actor_id, VfsConfig::default());
	for i in 50..100 {
		sqlite_exec(
			db2.as_ptr(),
			&format!("INSERT INTO t (id, v) VALUES ({i}, {});", i),
		)
		.expect("insert after reopen");
	}

	let count =
		sqlite_query_i64(db2.as_ptr(), "SELECT COUNT(*) FROM t;").expect("count should succeed");
	assert_eq!(count, 100);
}

// Bench-parity tests. Each mirrors a workload in
// examples/kitchen-sink/src/actors/testing/test-sqlite-bench.ts so
// storage-layer regressions surface here without needing the full stack.

fn open_bench_db(runtime: &tokio::runtime::Runtime) -> NativeDatabase {
	let harness = DirectEngineHarness::new();
	harness.open_db(runtime)
}

#[test]
fn bench_insert_tx_x10000() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER);",
	)
	.unwrap();

	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..10_000 {
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO t (id, v) VALUES ({i}, {i});"),
		)
		.unwrap();
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM t;").unwrap(),
		10_000
	);
}

#[test]
fn bench_large_tx_insert_500kb() {
	large_tx_insert(500 * 1024);
}

#[test]
fn bench_large_tx_insert_10mb() {
	large_tx_insert(10 * 1024 * 1024);
}

#[test]
fn bench_large_tx_insert_50mb() {
	// 50MB exercises the slow-path stage/finalize chunking that has
	// historically hit decode errors under certain transports.
	large_tx_insert(50 * 1024 * 1024);
}

#[test]
fn bench_large_tx_insert_100mb() {
	large_tx_insert(100 * 1024 * 1024);
}

fn large_tx_insert(target_bytes: usize) {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE large_tx (id INTEGER PRIMARY KEY AUTOINCREMENT, payload BLOB NOT NULL);",
	)
	.unwrap();

	let row_size = 4 * 1024;
	let rows = (target_bytes + row_size - 1) / row_size;
	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for _ in 0..rows {
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO large_tx (payload) VALUES (randomblob({row_size}));"),
		)
		.unwrap();
	}
	if let Err(err) = sqlite_exec(db.as_ptr(), "COMMIT") {
		let vfs_err = direct_vfs_ctx(&db).clone_last_error();
		panic!(
			"COMMIT failed for {} MiB: sqlite={}, vfs_last_error={:?}",
			target_bytes / (1024 * 1024),
			err,
			vfs_err,
		);
	}

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM large_tx;").unwrap(),
		rows as i64
	);
}

#[test]
fn bench_churn_insert_delete_10x1000() {
	// Tests freelist reuse / space reclamation under heavy churn.
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE churn (id INTEGER PRIMARY KEY AUTOINCREMENT, payload BLOB NOT NULL);",
	)
	.unwrap();
	for _ in 0..10 {
		sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
		for _ in 0..1000 {
			sqlite_exec(
				db.as_ptr(),
				"INSERT INTO churn (payload) VALUES (randomblob(1024));",
			)
			.unwrap();
		}
		sqlite_exec(db.as_ptr(), "DELETE FROM churn;").unwrap();
		sqlite_exec(db.as_ptr(), "COMMIT").unwrap();
	}
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM churn;").unwrap(),
		0
	);
}

#[test]
fn bench_mixed_oltp_large() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE mixed (id INTEGER PRIMARY KEY, v INTEGER NOT NULL, data BLOB NOT NULL);",
	)
	.unwrap();

	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..500 {
		sqlite_exec(
			db.as_ptr(),
			&format!(
				"INSERT INTO mixed (id, v, data) VALUES ({i}, {}, randomblob(1024));",
				i * 2
			),
		)
		.unwrap();
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..500 {
		sqlite_exec(
			db.as_ptr(),
			&format!(
				"INSERT INTO mixed (id, v, data) VALUES ({}, {}, randomblob(1024));",
				500 + i,
				i * 3
			),
		)
		.unwrap();
		sqlite_exec(
			db.as_ptr(),
			&format!("UPDATE mixed SET v = v + 1 WHERE id = {i};"),
		)
		.unwrap();
		if i % 5 == 0 && i >= 50 {
			sqlite_exec(
				db.as_ptr(),
				&format!("DELETE FROM mixed WHERE id = {};", i - 50),
			)
			.unwrap();
		}
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	let count = sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM mixed;").unwrap();
	assert!(count > 900 && count < 1000);
}

#[test]
fn bench_bulk_update_1000_rows() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE bulk (id INTEGER PRIMARY KEY, v INTEGER);",
	)
	.unwrap();
	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..1000 {
		sqlite_exec(
			db.as_ptr(),
			&format!("INSERT INTO bulk (id, v) VALUES ({i}, {i});"),
		)
		.unwrap();
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..1000 {
		sqlite_exec(
			db.as_ptr(),
			&format!("UPDATE bulk SET v = v + 1 WHERE id = {i};"),
		)
		.unwrap();
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT SUM(v) FROM bulk;").unwrap(),
		(0..1000).map(|i| i + 1).sum::<i64>()
	);
}

#[test]
fn bench_truncate_and_regrow() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE regrow (id INTEGER PRIMARY KEY AUTOINCREMENT, payload BLOB NOT NULL);",
	)
	.unwrap();
	for _ in 0..2 {
		sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
		for _ in 0..500 {
			sqlite_exec(
				db.as_ptr(),
				"INSERT INTO regrow (payload) VALUES (randomblob(1024));",
			)
			.unwrap();
		}
		sqlite_exec(db.as_ptr(), "COMMIT").unwrap();
		sqlite_exec(db.as_ptr(), "DELETE FROM regrow;").unwrap();
	}
	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM regrow;").unwrap(),
		0
	);
}

#[test]
fn bench_many_small_tables() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..50 {
		sqlite_exec(
			db.as_ptr(),
			&format!("CREATE TABLE t_{i} (id INTEGER PRIMARY KEY, v INTEGER);"),
		)
		.unwrap();
		for j in 0..10 {
			sqlite_exec(
				db.as_ptr(),
				&format!("INSERT INTO t_{i} (id, v) VALUES ({j}, {});", i * j),
			)
			.unwrap();
		}
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	let total: i64 = (0..50)
		.map(|i| sqlite_query_i64(db.as_ptr(), &format!("SELECT COUNT(*) FROM t_{i};")).unwrap())
		.sum();
	assert_eq!(total, 500);
}

#[test]
fn bench_index_creation_on_10k_rows() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
			db.as_ptr(),
			"CREATE TABLE idx_test (id INTEGER PRIMARY KEY AUTOINCREMENT, k TEXT NOT NULL, v INTEGER NOT NULL);",
		)
		.unwrap();
	sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
	for i in 0..10_000 {
		sqlite_exec(
			db.as_ptr(),
			&format!(
				"INSERT INTO idx_test (k, v) VALUES ('key-{}-{i}', {i});",
				i % 1000
			),
		)
		.unwrap();
	}
	sqlite_exec(db.as_ptr(), "COMMIT").unwrap();

	sqlite_exec(db.as_ptr(), "CREATE INDEX idx_test_k ON idx_test(k);").unwrap();

	assert_eq!(
		sqlite_query_i64(db.as_ptr(), "SELECT COUNT(*) FROM idx_test;").unwrap(),
		10_000
	);
}

#[test]
fn bench_growing_aggregation() {
	let runtime = direct_runtime();
	let db = open_bench_db(&runtime);
	sqlite_exec(
		db.as_ptr(),
		"CREATE TABLE agg (id INTEGER PRIMARY KEY AUTOINCREMENT, v INTEGER NOT NULL);",
	)
	.unwrap();

	let batches = 20;
	let per_batch = 100;
	for batch in 0..batches {
		sqlite_exec(db.as_ptr(), "BEGIN").unwrap();
		for i in 0..per_batch {
			sqlite_exec(
				db.as_ptr(),
				&format!("INSERT INTO agg (v) VALUES ({});", batch * per_batch + i),
			)
			.unwrap();
		}
		sqlite_exec(db.as_ptr(), "COMMIT").unwrap();
		let expected_sum: i64 = (0..(batch + 1) * per_batch).map(|i| i as i64).sum();
		assert_eq!(
			sqlite_query_i64(db.as_ptr(), "SELECT SUM(v) FROM agg;").unwrap(),
			expected_sum
		);
	}
}
