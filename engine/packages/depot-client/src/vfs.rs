//! Custom SQLite VFS backed by KV operations over the KV channel.
//!
//! This crate owns the KV-backed SQLite behavior used by `rivetkit-napi`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use depot_client_types::is_head_fence_mismatch;
use libsqlite3_sys::*;
use moka::sync::Cache;
use parking_lot::{Mutex, RwLock};
use rivet_envoy_protocol as protocol;
use scc::HashMap as SccHashMap;
use tokio::runtime::Handle;

use crate::optimization_flags::{
	SqliteOptimizationFlags, SqliteVfsPageCacheMode, sqlite_optimization_flags,
};
use crate::sqlite_page::{PageClass, classify};

const DEFAULT_PREFETCH_DEPTH: usize = 64;
const LEGACY_PREFETCH_DEPTH: usize = 16;
const DEFAULT_MAX_PREFETCH_BYTES: usize = 256 * 1024;
const DEFAULT_ADAPTIVE_PREFETCH_DEPTH: usize = 256;
const DEFAULT_ADAPTIVE_MAX_PREFETCH_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_PAGES_PER_STAGE: usize = 4_000;
const DEFAULT_RECENT_HINT_PAGE_BUDGET: usize = 128;
const DEFAULT_RECENT_HINT_RANGE_BUDGET: usize = 16;
const DEFAULT_PAGE_SIZE: usize = 4096;
const NATIVE_DATABASE_DROP_FLUSH_TIMEOUT: Duration = Duration::from_millis(250);
const MIN_RECENT_SCAN_RANGE_PAGES: u32 = 8;
const FORWARD_SCAN_SCORE_THRESHOLD: i32 = 6;
const FORWARD_SCAN_SCORE_MAX: i32 = 12;
const FORWARD_SCAN_GAP_TOLERANCE: u32 = 8;
const MAX_PATHNAME: c_int = 64;
const TEMP_AUX_PATH_PREFIX: &str = "__sqlite_temp__";
const SQLITE_HEADER_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const EMPTY_DB_PAGE_HEADER_PREFIX: [u8; 108] = [
	83, 81, 76, 105, 116, 101, 32, 102, 111, 114, 109, 97, 116, 32, 51, 0, 16, 0, 1, 1, 0, 64, 32,
	32, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 46, 138, 17, 13, 0, 0, 0, 0, 16, 0, 0,
];

static NEXT_TEMP_AUX_ID: AtomicU64 = AtomicU64::new(1);

unsafe extern "C" {
	fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
}

fn empty_db_page() -> Vec<u8> {
	let mut page = vec![0u8; DEFAULT_PAGE_SIZE];
	page[..EMPTY_DB_PAGE_HEADER_PREFIX.len()].copy_from_slice(&EMPTY_DB_PAGE_HEADER_PREFIX);
	page
}

fn sqlite_header_page_size(page: &[u8]) -> Option<usize> {
	if page.len() < 100 || &page[..SQLITE_HEADER_MAGIC.len()] != SQLITE_HEADER_MAGIC {
		return None;
	}

	let raw = u16::from_be_bytes([page[16], page[17]]);
	let page_size = if raw == 1 { 65_536 } else { usize::from(raw) };

	if (512..=65_536).contains(&page_size) && page_size.is_power_of_two() {
		Some(page_size)
	} else {
		None
	}
}

fn sqlite_header_db_size_pages(page: &[u8]) -> Option<u32> {
	if page.len() < 100 || &page[..SQLITE_HEADER_MAGIC.len()] != SQLITE_HEADER_MAGIC {
		return None;
	}

	let db_size_pages = u32::from_be_bytes([page[28], page[29], page[30], page[31]]);
	Some(db_size_pages.max(1))
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
	if let Some(message) = payload.downcast_ref::<&str>() {
		message.to_string()
	} else if let Some(message) = payload.downcast_ref::<String>() {
		message.clone()
	} else {
		"unknown panic".to_string()
	}
}

macro_rules! vfs_catch_unwind {
	($err_val:expr, $body:expr) => {
		match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)) {
			Ok(result) => result,
			Err(panic) => {
				tracing::error!(message = panic_message(&panic), "sqlite callback panicked");
				$err_val
			}
		}
	};
}

#[async_trait]
pub trait SqliteTransport: Send + Sync {
	async fn get_pages(
		&self,
		request: protocol::SqliteGetPagesRequest,
	) -> Result<protocol::SqliteGetPagesResponse>;

	async fn commit(
		&self,
		request: protocol::SqliteCommitRequest,
	) -> Result<protocol::SqliteCommitResponse>;
}

pub type SqliteTransportHandle = Arc<dyn SqliteTransport>;
fn sqlite_now_ms() -> Result<i64> {
	use std::time::{SystemTime, UNIX_EPOCH};

	Ok(SystemTime::now()
		.duration_since(UNIX_EPOCH)?
		.as_millis()
		.try_into()?)
}

#[derive(Debug, Clone)]
pub struct VfsConfig {
	pub cache_capacity_pages: u64,
	pub protected_cache_pages: usize,
	pub page_cache_mode: SqliteVfsPageCacheMode,
	pub staging_cache_ttl_ms: u64,
	pub prefetch_depth: usize,
	pub adaptive_prefetch_depth: usize,
	pub max_prefetch_bytes: usize,
	pub adaptive_max_prefetch_bytes: usize,
	pub max_pages_per_stage: usize,
	pub startup_preload_max_bytes: usize,
	pub startup_preload_first_pages: bool,
	pub startup_preload_first_page_count: u32,
	pub preload_hints_on_open: bool,
	pub preload_hint_early_pages: bool,
	pub recent_hint_page_budget: usize,
	pub recent_hint_range_budget: usize,
	pub cache_hit_predictor_training: bool,
	pub recent_page_hints: bool,
	pub adaptive_read_ahead: bool,
	pub retain_read_cache: bool,
	#[cfg(test)]
	pub assert_batch_atomic: bool,
	#[cfg(test)]
	pub advertise_batch_atomic: bool,
}

impl Default for VfsConfig {
	fn default() -> Self {
		Self::from_optimization_flags(*sqlite_optimization_flags())
	}
}

impl VfsConfig {
	pub fn from_optimization_flags(flags: SqliteOptimizationFlags) -> Self {
		let caches_pages = flags.vfs_page_cache_mode.caches_any_pages();
		Self {
			cache_capacity_pages: if caches_pages {
				flags.vfs_page_cache_capacity_pages
			} else {
				0
			},
			protected_cache_pages: 0,
			page_cache_mode: flags.vfs_page_cache_mode,
			staging_cache_ttl_ms: if caches_pages {
				flags.vfs_staging_cache_ttl_ms
			} else {
				0
			},
			prefetch_depth: if flags.read_ahead {
				DEFAULT_PREFETCH_DEPTH
			} else {
				LEGACY_PREFETCH_DEPTH
			},
			adaptive_prefetch_depth: DEFAULT_ADAPTIVE_PREFETCH_DEPTH,
			max_prefetch_bytes: DEFAULT_MAX_PREFETCH_BYTES,
			adaptive_max_prefetch_bytes: DEFAULT_ADAPTIVE_MAX_PREFETCH_BYTES,
			max_pages_per_stage: DEFAULT_MAX_PAGES_PER_STAGE,
			startup_preload_max_bytes: flags.startup_preload_max_bytes,
			startup_preload_first_pages: flags.startup_preload_first_pages,
			startup_preload_first_page_count: flags.startup_preload_first_page_count,
			preload_hints_on_open: flags.preload_hints_on_open,
			preload_hint_early_pages: flags.preload_hint_early_pages,
			recent_hint_page_budget: if flags.recent_page_hints && flags.preload_hint_hot_pages {
				DEFAULT_RECENT_HINT_PAGE_BUDGET
			} else {
				0
			},
			recent_hint_range_budget: if flags.recent_page_hints && flags.preload_hint_scan_ranges {
				DEFAULT_RECENT_HINT_RANGE_BUDGET
			} else {
				0
			},
			cache_hit_predictor_training: flags.cache_hit_predictor_training,
			recent_page_hints: flags.recent_page_hints,
			adaptive_read_ahead: flags.adaptive_read_ahead,
			retain_read_cache: flags.vfs_page_cache_mode.caches_any_pages(),
			#[cfg(test)]
			assert_batch_atomic: true,
			#[cfg(test)]
			advertise_batch_atomic: true,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsPreloadHintRange {
	pub start_pgno: u32,
	pub page_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VfsPreloadHintSnapshot {
	pub pgnos: Vec<u32>,
	pub ranges: Vec<VfsPreloadHintRange>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InitialPages {
	pub pages: Vec<(u32, Vec<u8>)>,
	pub head_txid: Option<u64>,
	pub requested_page_count: u32,
}

impl From<Vec<(u32, Vec<u8>)>> for InitialPages {
	fn from(pages: Vec<(u32, Vec<u8>)>) -> Self {
		let requested_page_count = pages.len().try_into().unwrap_or(u32::MAX);
		Self {
			pages,
			head_txid: None,
			requested_page_count,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitPath {
	Fast,
	Slow,
}

#[derive(Debug, Clone)]
pub struct BufferedCommitRequest {
	pub actor_id: String,
	pub new_db_size_pages: u32,
	pub dirty_pages: Vec<protocol::SqliteDirtyPage>,
	pub expected_head_txid: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct BufferedCommitOutcome {
	pub path: CommitPath,
	pub db_size_pages: u32,
	pub head_txid: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitBufferError {
	FenceMismatch(String),
	Other(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteVfsMetricsSnapshot {
	pub request_build_ns: u64,
	pub serialize_ns: u64,
	pub transport_ns: u64,
	pub state_update_ns: u64,
	pub total_ns: u64,
	pub commit_count: u64,
	pub page_cache_entries: u64,
	pub page_cache_weighted_size: u64,
	pub page_cache_capacity_pages: u64,
	pub write_buffer_dirty_pages: u64,
	pub db_size_pages: u64,
}

pub const MAX_PROFILED_GET_PAGES_REQUESTS: usize = 16;
pub const MAX_PROFILED_TRANSACTION_STATEMENTS: usize = 32;

/// Bounded details for one physical `get_pages` call made while a native SQLite
/// command is executing.
#[derive(Debug, Clone, Default)]
pub struct SqliteGetPagesProfile {
	pub ordinal: u64,
	pub duration_ns: u64,
	pub demand_requested: u64,
	pub prefetch_requested: u64,
	pub response_present: u64,
	pub response_missing: u64,
	pub overflow_expansion_extra: u64,
	pub response_bytes: u64,
	pub success: bool,
}

/// Fixed-size, connection-local counters collected by VFS callbacks for one
/// native SQLite command. The worker installs and removes this context around
/// each command, so VFS activity cannot be attributed to a neighboring query.
#[derive(Debug, Clone, Default)]
pub struct SqliteOperationProfile {
	pub worker_wait_ns: u64,
	pub sqlite_execution_ns: u64,
	pub storage_ns: u64,
	pub bind_count: u64,
	pub bind_logical_bytes: u64,
	pub result_rows: u64,
	pub result_columns: u64,
	pub result_logical_bytes: u64,
	pub sqlite_requested_pages: u64,
	pub cache_hit_pages: u64,
	pub cache_miss_pages: u64,
	pub depot_demand_requested_pages: u64,
	pub vfs_prefetch_requested_pages: u64,
	pub response_present_pages: u64,
	pub response_missing_pages: u64,
	pub overflow_expansion_extra_pages: u64,
	pub btree_pages: u64,
	pub non_btree_pages: u64,
	pub prefetch_consumed_pages: u64,
	pub prefetch_unused_pages: u64,
	pub dirty_pages: u64,
	pub storage_response_bytes: u64,
	pub dirty_bytes: u64,
	pub get_pages_round_trips: u64,
	pub commit_round_trips: u64,
	pub get_pages_requests: [Option<SqliteGetPagesProfile>; MAX_PROFILED_GET_PAGES_REQUESTS],
	pub omitted_get_pages_requests: u64,
}

#[derive(Debug, Clone)]
pub struct SqliteOperationMetric {
	pub operation_type: &'static str,
	pub fingerprint: String,
	pub fingerprint_source: &'static str,
	pub transaction_mode: &'static str,
	pub storage_transport: &'static str,
	pub outcome: &'static str,
	pub sql_bytes: u64,
	pub total_ns: u64,
	pub transaction_wait_ns: u64,
	pub profile: SqliteOperationProfile,
}

#[derive(Debug, Clone)]
pub struct SqliteTransactionMetric {
	pub fingerprint: String,
	pub fingerprint_source: &'static str,
	pub shape_fingerprint: String,
	pub statement_fingerprint_hashes: [Option<[u8; 16]>; MAX_PROFILED_TRANSACTION_STATEMENTS],
	pub omitted_statement_fingerprints: u64,
	pub storage_transport: &'static str,
	pub outcome: &'static str,
	pub total_ns: u64,
	pub transaction_wait_ns: u64,
	pub worker_wait_ns: u64,
	pub storage_ns: u64,
	pub local_work_ns: u64,
	pub application_time_ns: u64,
	pub commit_ns: u64,
	pub get_pages_round_trips: u64,
	pub statement_count: u64,
	pub dirty_pages: u64,
	pub dirty_bytes: u64,
}

impl SqliteOperationProfile {
	fn push_get_pages(&mut self, request: SqliteGetPagesProfile, request_limit: usize) {
		self.storage_ns = self.storage_ns.saturating_add(request.duration_ns);
		self.get_pages_round_trips = self.get_pages_round_trips.saturating_add(1);
		self.depot_demand_requested_pages = self
			.depot_demand_requested_pages
			.saturating_add(request.demand_requested);
		self.vfs_prefetch_requested_pages = self
			.vfs_prefetch_requested_pages
			.saturating_add(request.prefetch_requested);
		self.response_present_pages = self
			.response_present_pages
			.saturating_add(request.response_present);
		self.response_missing_pages = self
			.response_missing_pages
			.saturating_add(request.response_missing);
		self.overflow_expansion_extra_pages = self
			.overflow_expansion_extra_pages
			.saturating_add(request.overflow_expansion_extra);
		self.storage_response_bytes = self
			.storage_response_bytes
			.saturating_add(request.response_bytes);
		let index = self.get_pages_round_trips.saturating_sub(1) as usize;
		if index < request_limit
			&& let Some(slot) = self.get_pages_requests.get_mut(index)
		{
			*slot = Some(request);
		} else {
			self.omitted_get_pages_requests = self.omitted_get_pages_requests.saturating_add(1);
		}
	}
}

pub(crate) struct SqliteOperationProfileGuard<'a> {
	ctx: &'a VfsContext,
	active: bool,
}

impl SqliteOperationProfileGuard<'_> {
	pub(crate) fn finish(mut self) -> SqliteOperationProfile {
		self.active = false;
		self.ctx.finish_operation_profile()
	}
}

impl Drop for SqliteOperationProfileGuard<'_> {
	fn drop(&mut self) {
		if self.active {
			let _ = self.ctx.finish_operation_profile();
		}
	}
}

/// Cumulative count of network round trips the VFS has issued to the engine.
///
/// `get_pages` counts `SqliteGetPagesRequest` fetches and `commit` counts
/// `SqliteCommitRequest` commits. Diffing two snapshots gives the round trips
/// performed by the work that ran between them, such as a SQLite transaction.
#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteRoundTripCounts {
	pub get_pages: u64,
	pub commit: u64,
}

impl SqliteRoundTripCounts {
	pub fn total(&self) -> u64 {
		self.get_pages.saturating_add(self.commit)
	}

	pub fn since(&self, earlier: SqliteRoundTripCounts) -> SqliteRoundTripCounts {
		SqliteRoundTripCounts {
			get_pages: self.get_pages.saturating_sub(earlier.get_pages),
			commit: self.commit.saturating_sub(earlier.commit),
		}
	}
}

#[derive(Debug, Clone, Copy)]
pub enum SqliteOpenPhase {
	InitialPreload,
	VfsRegister,
	WorkerReady,
	Total,
}

impl SqliteOpenPhase {
	pub fn as_label(self) -> &'static str {
		match self {
			SqliteOpenPhase::InitialPreload => "initial_preload",
			SqliteOpenPhase::VfsRegister => "vfs_register",
			SqliteOpenPhase::WorkerReady => "worker_ready",
			SqliteOpenPhase::Total => "total",
		}
	}
}

pub trait SqliteVfsMetrics: Send + Sync {
	fn profiling_enabled(&self) -> bool;

	fn max_profiled_get_pages_requests(&self) -> usize;

	/// Records an operation and returns whether its original fingerprint was
	/// admitted instead of being routed to the shared `other` series.
	fn observe_operation_profile(&self, profile: &SqliteOperationMetric) -> bool;

	/// Records a transaction and returns whether its original fingerprint was
	/// admitted instead of being routed to the shared `other` series.
	fn observe_transaction_profile(&self, profile: &SqliteTransactionMetric) -> bool;

	fn emit_operation_diagnostic_event(
		&self,
		actor_id: &str,
		generation: Option<u64>,
		profile: &SqliteOperationMetric,
	);

	fn emit_transaction_diagnostic_event(
		&self,
		actor_id: &str,
		generation: Option<u64>,
		profile: &SqliteTransactionMetric,
	);

	fn record_fingerprint_catalog(
		&self,
		operation_type: &'static str,
		fingerprint: &str,
		identity: &str,
		format_version: u8,
	);

	fn record_resolve_pages(&self, requested_pages: u64);

	fn record_resolve_cache_hits(&self, pages: u64);

	fn record_resolve_cache_misses(&self, pages: u64);

	fn record_get_pages_request(&self, pages: u64, prefetch_pages: u64, page_size: u64);

	fn observe_get_pages_duration(&self, duration_ns: u64);

	fn observe_open_phase(&self, phase: SqliteOpenPhase, outcome: &'static str, duration_ns: u64);

	fn record_startup_preload_pages(&self, kind: &'static str, pages: u64);

	fn record_commit(&self);

	fn observe_commit_phases(
		&self,
		request_build_ns: u64,
		serialize_ns: u64,
		transport_ns: u64,
		state_update_ns: u64,
		total_ns: u64,
	);

	fn set_worker_queue_depth(&self, depth: u64);

	fn set_worker_active(&self, active: bool);

	fn set_worker_inflight(&self, active: bool);

	fn set_coordinator_queue_depth(&self, depth: u64);

	fn record_worker_queue_overload(&self);

	fn observe_worker_command_duration(
		&self,
		operation: &'static str,
		in_tx: bool,
		stmt_kind: &'static str,
		duration_ns: u64,
	);

	fn observe_transaction_round_trips(&self, get_pages_round_trips: u64, commit_round_trips: u64);

	fn record_worker_command_error(&self, operation: &'static str, code: &'static str);

	fn observe_worker_close_duration(&self, duration_ns: u64);

	fn record_worker_close_timeout(&self);

	fn record_worker_crash(&self);

	fn record_worker_unclean_close(&self);
}

#[derive(Debug, Clone, Copy, Default)]
struct CommitTransportMetrics {
	serialize_ns: u64,
	transport_ns: u64,
}

enum CommitWait<T> {
	Completed(T),
	TimedOut,
}

pub struct VfsContext {
	actor_id: String,
	generation: Option<u64>,
	runtime: Handle,
	transport: SqliteTransportHandle,
	config: VfsConfig,
	state: RwLock<VfsState>,
	aux_files: RwLock<BTreeMap<String, Arc<AuxFileState>>>,
	last_error: Mutex<Option<String>>,
	transient_commit_error: Mutex<Option<String>>,
	fatal_error: RwLock<Option<String>>,
	#[cfg(test)]
	fail_next_aux_open: Mutex<Option<String>>,
	#[cfg(test)]
	fail_next_aux_delete: Mutex<Option<String>>,
	commit_atomic_count: AtomicU64,
	io_methods: Box<sqlite3_io_methods>,
	// Performance counters
	pub resolve_pages_total: AtomicU64,
	pub resolve_pages_cache_hits: AtomicU64,
	pub resolve_pages_fetches: AtomicU64,
	pub pages_fetched_total: AtomicU64,
	pub prefetch_pages_total: AtomicU64,
	pub commit_total: AtomicU64,
	pub commit_request_build_ns: AtomicU64,
	pub commit_serialize_ns: AtomicU64,
	pub commit_transport_ns: AtomicU64,
	pub commit_state_update_ns: AtomicU64,
	pub commit_duration_ns_total: AtomicU64,
	// SQLite invokes VFS callbacks synchronously, so this profiling context cannot
	// use an async mutex. The native worker releases each guard before returning
	// to async code.
	operation_profile_active: AtomicBool,
	operation_profile: Mutex<Option<SqliteOperationProfile>>,
	operation_prefetch_unused_start: AtomicU64,
	metrics: Option<Arc<dyn SqliteVfsMetrics>>,
}

#[derive(Debug, Clone)]
struct VfsState {
	db_size_pages: u32,
	head_txid: Option<u64>,
	page_size: usize,
	page_cache: Cache<u32, Vec<u8>>,
	committed_page_cache: Cache<u32, Vec<u8>>,
	protected_page_cache: Arc<SccHashMap<u32, Vec<u8>>>,
	profiling_enabled: bool,
	prefetched_pages: Arc<scc::HashSet<u32>>,
	prefetch_unused_total: Arc<AtomicU64>,
	write_buffer: WriteBuffer,
	predictor: ClassifiedPredictor,
	read_ahead: ClassifiedReadAhead,
	recent_pages: RecentPageTracker,
	dead: bool,
}

#[derive(Debug, Clone, Default)]
struct WriteBuffer {
	in_atomic_write: bool,
	saved_db_size: u32,
	dirty: BTreeMap<u32, Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
struct PrefetchPredictor {
	last_pgno: Option<u32>,
	last_delta: Option<i64>,
	stride_run_len: usize,
	// Inspired by mvSQLite's Markov + stride predictor design (Apache-2.0).
	transitions: HashMap<i64, HashMap<i64, u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadAheadMode {
	Bounded,
	ForwardScan,
}

#[derive(Debug, Clone, Copy)]
struct ReadAheadPlan {
	mode: ReadAheadMode,
	depth: usize,
	max_bytes: usize,
	seed_pgno: Option<u32>,
}

#[derive(Debug, Clone, Default)]
struct AdaptiveReadAhead {
	last_pgno: Option<u32>,
	scan_tip_pgno: Option<u32>,
	score: i32,
}

/// Per-class Markov/stride predictors. Keeping B-tree and overflow accesses on
/// separate predictors prevents the interleaved leaf/overflow read pattern from
/// destroying each stream's stride signal.
#[derive(Debug, Clone, Default)]
struct ClassifiedPredictor {
	btree: PrefetchPredictor,
	overflow: PrefetchPredictor,
}

impl ClassifiedPredictor {
	fn record(&mut self, class: PageClass, pgno: u32) {
		self.for_class(class).record(pgno);
	}

	fn multi_predict(
		&self,
		class: PageClass,
		from_pgno: u32,
		depth: usize,
		db_size_pages: u32,
	) -> Vec<u32> {
		match class {
			PageClass::Btree => self.btree.multi_predict(from_pgno, depth, db_size_pages),
			PageClass::Overflow => self.overflow.multi_predict(from_pgno, depth, db_size_pages),
		}
	}

	fn for_class(&mut self, class: PageClass) -> &mut PrefetchPredictor {
		match class {
			PageClass::Btree => &mut self.btree,
			PageClass::Overflow => &mut self.overflow,
		}
	}
}

/// Per-class adaptive read-ahead trackers. The B-tree tracker drives leaf-scan
/// forward read-ahead while the overflow tracker handles overflow-chain scans,
/// so neither resets the other's forward-scan score.
#[derive(Debug, Clone, Default)]
struct ClassifiedReadAhead {
	btree: AdaptiveReadAhead,
	overflow: AdaptiveReadAhead,
}

impl ClassifiedReadAhead {
	fn for_class(&mut self, class: PageClass) -> &mut AdaptiveReadAhead {
		match class {
			PageClass::Btree => &mut self.btree,
			PageClass::Overflow => &mut self.overflow,
		}
	}
}

/// A read-ahead plan paired with the page class of the prefetch seed, so the
/// caller knows which per-class predictor to extrapolate from.
#[derive(Debug, Clone, Copy)]
struct ClassifiedReadAheadPlan {
	plan: ReadAheadPlan,
	seed_class: PageClass,
}

#[derive(Debug, Clone)]
struct RecentPageTracker {
	page_budget: usize,
	range_budget: usize,
	hot_pages: HashMap<u32, RecentPageAccess>,
	ranges: VecDeque<VfsPreloadHintRange>,
	active_scan_start: Option<u32>,
	active_scan_end: u32,
	last_pgno: Option<u32>,
	access_seq: u64,
}

#[derive(Debug, Clone, Copy)]
struct RecentPageAccess {
	count: u32,
	last_access_seq: u64,
}

#[derive(Debug)]
enum GetPagesError {
	FenceMismatch(String),
	Other(String),
}

#[repr(C)]
struct VfsFile {
	base: sqlite3_file,
	ctx: *const VfsContext,
	aux: *mut AuxFileHandle,
}

#[derive(Default)]
struct AuxFileState {
	bytes: Mutex<Vec<u8>>,
}

struct AuxFileHandle {
	path: String,
	state: Arc<AuxFileState>,
	delete_on_close: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageCacheInsertKind {
	Target,
	Prefetch,
	Startup,
}

unsafe impl Send for VfsContext {}
unsafe impl Sync for VfsContext {}

pub struct SqliteVfs {
	_registration: SqliteVfsRegistration,
	_name: CString,
	ctx: Box<VfsContext>,
}

unsafe impl Send for SqliteVfs {}
unsafe impl Sync for SqliteVfs {}

struct SqliteVfsRegistration {
	vfs_ptr: *mut sqlite3_vfs,
}

pub struct NativeDatabase {
	db: *mut sqlite3,
	_vfs: NativeVfsHandle,
}

unsafe impl Send for NativeDatabase {}

pub type NativeVfsHandle = Arc<SqliteVfs>;
pub type NativeConnection = NativeDatabase;

impl PrefetchPredictor {
	fn record(&mut self, pgno: u32) {
		if let Some(last_pgno) = self.last_pgno {
			let delta = pgno as i64 - last_pgno as i64;
			if let Some(last_delta) = self.last_delta {
				self.transitions
					.entry(last_delta)
					.or_default()
					.entry(delta)
					.and_modify(|count| *count += 1)
					.or_insert(1);
				if delta == last_delta {
					self.stride_run_len += 1;
				} else {
					self.stride_run_len = 1;
				}
			} else {
				self.stride_run_len = 1;
			}
			self.last_delta = Some(delta);
		}
		self.last_pgno = Some(pgno);
	}

	fn multi_predict(&self, from_pgno: u32, depth: usize, db_size_pages: u32) -> Vec<u32> {
		if depth == 0 || db_size_pages == 0 {
			return Vec::new();
		}

		let mut seen = HashSet::new();
		let mut predicted = Vec::with_capacity(depth);

		if let Some(delta) = self.last_delta {
			if self.stride_run_len >= 2 && delta > 0 {
				let mut current = from_pgno as i64;
				for _ in 0..depth {
					current += delta;
					if !(1..=db_size_pages as i64).contains(&current) {
						break;
					}
					let pgno = current as u32;
					if seen.insert(pgno) {
						predicted.push(pgno);
					}
				}
				if predicted.len() >= depth {
					return predicted;
				}
			}

			let mut current_delta = delta;
			let mut current_pgno = from_pgno as i64;
			for _ in predicted.len()..depth {
				let Some(next_delta) = self
					.transitions
					.get(&current_delta)
					.and_then(|counts| counts.iter().max_by_key(|(_, count)| *count))
					.map(|(delta, _)| *delta)
				else {
					break;
				};

				current_pgno += next_delta;
				if !(1..=db_size_pages as i64).contains(&current_pgno) {
					break;
				}
				let pgno = current_pgno as u32;
				if seen.insert(pgno) {
					predicted.push(pgno);
				}
				current_delta = next_delta;
			}
		}

		predicted
	}
}

impl AdaptiveReadAhead {
	fn record_and_plan(&mut self, pgnos: &[u32], config: &VfsConfig) -> ReadAheadPlan {
		let mut scan_seed_pgno = None;
		for pgno in pgnos.iter().copied() {
			if self.record(pgno) {
				scan_seed_pgno = Some(pgno);
			}
		}

		if config.adaptive_read_ahead
			&& self.score >= FORWARD_SCAN_SCORE_THRESHOLD
			&& scan_seed_pgno.is_some()
		{
			let depth = if self.score >= FORWARD_SCAN_SCORE_THRESHOLD + 4 {
				config.adaptive_prefetch_depth
			} else {
				config
					.adaptive_prefetch_depth
					.min(config.prefetch_depth.saturating_mul(2))
			};
			ReadAheadPlan {
				mode: ReadAheadMode::ForwardScan,
				depth,
				max_bytes: config.adaptive_max_prefetch_bytes,
				seed_pgno: scan_seed_pgno,
			}
		} else {
			ReadAheadPlan {
				mode: ReadAheadMode::Bounded,
				depth: config.prefetch_depth,
				max_bytes: config.max_prefetch_bytes,
				seed_pgno: pgnos.last().copied(),
			}
		}
	}

	fn record(&mut self, pgno: u32) -> bool {
		let forward_from_last = self
			.last_pgno
			.and_then(|last_pgno| pgno.checked_sub(last_pgno))
			.is_some_and(|delta| (1..=FORWARD_SCAN_GAP_TOLERANCE).contains(&delta));
		let forward_from_scan_tip = self
			.scan_tip_pgno
			.and_then(|tip_pgno| pgno.checked_sub(tip_pgno))
			.is_some_and(|delta| (1..=FORWARD_SCAN_GAP_TOLERANCE).contains(&delta));
		let repeated = self.last_pgno == Some(pgno);

		let forward_scan_page = forward_from_last || forward_from_scan_tip;
		if forward_scan_page {
			self.score = (self.score + 2).min(FORWARD_SCAN_SCORE_MAX);
			self.scan_tip_pgno = Some(pgno);
		} else if !repeated {
			if self.score >= FORWARD_SCAN_SCORE_THRESHOLD && self.scan_tip_pgno.is_some() {
				self.score = (self.score - 1).max(0);
			} else {
				self.score = (self.score - 4).max(0);
				self.scan_tip_pgno = Some(pgno);
			}
		}

		self.last_pgno = Some(pgno);
		forward_scan_page
	}
}

impl VfsPreloadHintRange {
	fn new(start_pgno: u32, end_pgno: u32) -> Self {
		Self {
			start_pgno,
			page_count: end_pgno.saturating_sub(start_pgno).saturating_add(1),
		}
	}

	fn end_pgno(&self) -> u32 {
		self.start_pgno
			.saturating_add(self.page_count)
			.saturating_sub(1)
	}

	fn contains(&self, pgno: u32) -> bool {
		(self.start_pgno..=self.end_pgno()).contains(&pgno)
	}
}

impl RecentPageTracker {
	fn new(page_budget: usize, range_budget: usize) -> Self {
		Self {
			page_budget,
			range_budget,
			hot_pages: HashMap::new(),
			ranges: VecDeque::new(),
			active_scan_start: None,
			active_scan_end: 0,
			last_pgno: None,
			access_seq: 0,
		}
	}

	fn record_pages(&mut self, pgnos: impl IntoIterator<Item = u32>) {
		for pgno in pgnos {
			self.record_page(pgno);
		}
	}

	fn record_page(&mut self, pgno: u32) {
		self.access_seq = self.access_seq.saturating_add(1);
		self.record_hot_page(pgno);
		self.record_scan_page(pgno);
	}

	fn record_hot_page(&mut self, pgno: u32) {
		if self.page_budget == 0 {
			return;
		}

		if let Some(access) = self.hot_pages.get_mut(&pgno) {
			access.count = access.count.saturating_add(1);
			access.last_access_seq = self.access_seq;
			return;
		}

		if self.hot_pages.len() >= self.page_budget {
			if let Some(evict_pgno) = self
				.hot_pages
				.iter()
				.min_by_key(|(_, access)| (access.count, access.last_access_seq))
				.map(|(pgno, _)| *pgno)
			{
				self.hot_pages.remove(&evict_pgno);
			}
		}

		self.hot_pages.insert(
			pgno,
			RecentPageAccess {
				count: 1,
				last_access_seq: self.access_seq,
			},
		);
	}

	fn record_scan_page(&mut self, pgno: u32) {
		match self.last_pgno {
			Some(last_pgno) if pgno == last_pgno.saturating_add(1) => {
				if self.active_scan_start.is_none() {
					self.active_scan_start = Some(last_pgno);
				}
				self.active_scan_end = pgno;
			}
			Some(last_pgno) if pgno == last_pgno => {}
			Some(_) | None => {
				self.finish_active_scan();
				self.active_scan_start = None;
				self.active_scan_end = 0;
			}
		}
		self.last_pgno = Some(pgno);
	}

	fn finish_active_scan(&mut self) {
		let Some(start_pgno) = self.active_scan_start else {
			return;
		};
		if self.active_scan_end < start_pgno {
			return;
		}
		let page_count = self.active_scan_end - start_pgno + 1;
		if page_count < MIN_RECENT_SCAN_RANGE_PAGES {
			return;
		}
		self.push_range(VfsPreloadHintRange::new(start_pgno, self.active_scan_end));
	}

	fn push_range(&mut self, range: VfsPreloadHintRange) {
		if self.range_budget == 0 || range.page_count == 0 {
			return;
		}
		push_coalesced_range(&mut self.ranges, range);
		while self.ranges.len() > self.range_budget {
			self.ranges.pop_front();
		}
	}

	fn snapshot(&self) -> VfsPreloadHintSnapshot {
		let mut ranges = self.ranges.clone();
		if let Some(start_pgno) = self.active_scan_start {
			if self.active_scan_end >= start_pgno {
				let page_count = self.active_scan_end - start_pgno + 1;
				if page_count >= MIN_RECENT_SCAN_RANGE_PAGES {
					push_coalesced_range(
						&mut ranges,
						VfsPreloadHintRange::new(start_pgno, self.active_scan_end),
					);
				}
			}
		}
		while ranges.len() > self.range_budget {
			ranges.pop_front();
		}

		let mut scored_pages = self
			.hot_pages
			.iter()
			.filter(|(pgno, _)| !ranges.iter().any(|range| range.contains(**pgno)))
			.map(|(pgno, access)| (*pgno, *access))
			.collect::<Vec<_>>();
		scored_pages.sort_by(|(left_pgno, left), (right_pgno, right)| {
			right
				.count
				.cmp(&left.count)
				.then_with(|| right.last_access_seq.cmp(&left.last_access_seq))
				.then_with(|| left_pgno.cmp(right_pgno))
		});

		let mut pgnos = scored_pages
			.into_iter()
			.take(self.page_budget)
			.map(|(pgno, _)| pgno)
			.collect::<Vec<_>>();
		pgnos.sort_unstable();

		VfsPreloadHintSnapshot {
			pgnos,
			ranges: ranges.into_iter().collect(),
		}
	}
}

fn push_coalesced_range(ranges: &mut VecDeque<VfsPreloadHintRange>, range: VfsPreloadHintRange) {
	let mut start_pgno = range.start_pgno;
	let mut end_pgno = range.end_pgno();
	let mut retained = VecDeque::new();
	while let Some(existing) = ranges.pop_front() {
		let existing_end = existing.end_pgno();
		if existing.start_pgno <= end_pgno.saturating_add(1)
			&& start_pgno <= existing_end.saturating_add(1)
		{
			start_pgno = start_pgno.min(existing.start_pgno);
			end_pgno = end_pgno.max(existing_end);
		} else {
			retained.push_back(existing);
		}
	}
	retained.push_back(VfsPreloadHintRange::new(start_pgno, end_pgno));
	*ranges = retained;
}

impl VfsState {
	fn new(config: &VfsConfig, profiling_enabled: bool) -> Self {
		let page_cache = build_page_cache(config);
		let committed_page_cache = build_page_cache(config);
		let mut state = Self {
			db_size_pages: 1,
			head_txid: None,
			page_size: DEFAULT_PAGE_SIZE,
			page_cache,
			committed_page_cache,
			protected_page_cache: Arc::new(SccHashMap::new()),
			profiling_enabled,
			prefetched_pages: Arc::new(scc::HashSet::new()),
			prefetch_unused_total: Arc::new(AtomicU64::new(0)),
			write_buffer: WriteBuffer::default(),
			predictor: ClassifiedPredictor::default(),
			read_ahead: ClassifiedReadAhead::default(),
			recent_pages: RecentPageTracker::new(
				config.recent_hint_page_budget,
				config.recent_hint_range_budget,
			),
			dead: false,
		};
		state.cache_page(config, PageCacheInsertKind::Startup, 1, empty_db_page());
		state
	}

	fn cache_page(
		&mut self,
		config: &VfsConfig,
		kind: PageCacheInsertKind,
		pgno: u32,
		bytes: Vec<u8>,
	) {
		if !should_cache_page(config, kind, pgno) {
			return;
		}
		cache_page(
			config,
			&self.page_cache,
			&self.protected_page_cache,
			&self.prefetched_pages,
			&self.prefetch_unused_total,
			self.profiling_enabled,
			kind,
			pgno,
			bytes,
		);
	}

	fn cached_page(&self, config: &VfsConfig, pgno: u32) -> Option<(Vec<u8>, bool)> {
		if !can_read_cached_page(config, pgno) {
			return None;
		}
		let bytes = self
			.committed_page_cache
			.get(&pgno)
			.or_else(|| {
				self.protected_page_cache
					.read_sync(&pgno, |_, bytes| bytes.clone())
			})
			.or_else(|| self.page_cache.get(&pgno));
		bytes.map(|bytes| {
			let was_prefetched =
				self.profiling_enabled && self.prefetched_pages.remove_sync(&pgno).is_some();
			(bytes, was_prefetched)
		})
	}

	/// Record target page accesses into the per-class predictor and read-ahead
	/// trackers, returning the read-ahead plan for the class of the last target
	/// page (the prefetch seed). `train_predictor` mirrors the existing
	/// predictor-training gate on the cache-hit path.
	///
	/// Pages are classified from `resolved`, the already-materialized bytes for
	/// the non-missing targets, so this adds no extra page copies. A page whose
	/// bytes are not present (a first-touch miss) defaults to
	/// [`PageClass::Btree`]: the only pages read before their bytes are cached
	/// are first-touch leaf misses during a scan, since overflow pages arrive
	/// prefetched (server-side expansion) and are already cached.
	fn record_targets(
		&mut self,
		config: &VfsConfig,
		target_pgnos: &[u32],
		resolved: &HashMap<u32, Option<Vec<u8>>>,
		train_predictor: bool,
	) -> ClassifiedReadAheadPlan {
		let classes: Vec<PageClass> = target_pgnos
			.iter()
			.map(|&pgno| match resolved.get(&pgno) {
				Some(Some(bytes)) => classify(pgno, bytes),
				_ => PageClass::Btree,
			})
			.collect();
		let mut btree_pgnos = Vec::new();
		let mut overflow_pgnos = Vec::new();
		for (&pgno, &class) in target_pgnos.iter().zip(classes.iter()) {
			match class {
				PageClass::Btree => btree_pgnos.push(pgno),
				PageClass::Overflow => overflow_pgnos.push(pgno),
			}
		}

		if train_predictor {
			for pgno in &btree_pgnos {
				self.predictor.record(PageClass::Btree, *pgno);
			}
			for pgno in &overflow_pgnos {
				self.predictor.record(PageClass::Overflow, *pgno);
			}
		}

		let btree_plan = self
			.read_ahead
			.for_class(PageClass::Btree)
			.record_and_plan(&btree_pgnos, config);
		let overflow_plan = self
			.read_ahead
			.for_class(PageClass::Overflow)
			.record_and_plan(&overflow_pgnos, config);

		let seed_class = classes.last().copied().unwrap_or(PageClass::Btree);
		let plan = match seed_class {
			PageClass::Btree => btree_plan,
			PageClass::Overflow => overflow_plan,
		};
		ClassifiedReadAheadPlan { plan, seed_class }
	}

	fn has_readable_page(&self, config: &VfsConfig, pgno: u32) -> bool {
		if self.write_buffer.dirty.contains_key(&pgno) {
			return true;
		}
		if !can_read_cached_page(config, pgno) {
			return false;
		}
		self.committed_page_cache.contains_key(&pgno)
			|| self
				.protected_page_cache
				.read_sync(&pgno, |_, _| true)
				.unwrap_or(false)
			|| self.page_cache.contains_key(&pgno)
	}

	fn cache_committed_page(&mut self, config: &VfsConfig, pgno: u32, bytes: Vec<u8>) {
		if config.staging_cache_ttl_ms == 0 || !config.page_cache_mode.caches_any_pages() {
			return;
		}
		self.committed_page_cache.insert(pgno, bytes);
	}

	fn evict_target_read_pages(&self, pgnos: &[u32]) {
		for pgno in pgnos.iter().copied() {
			self.page_cache.invalidate(&pgno);
			self.protected_page_cache.remove_sync(&pgno);
		}
	}

	fn seed_page(
		&mut self,
		config: &VfsConfig,
		kind: PageCacheInsertKind,
		pgno: u32,
		page: Vec<u8>,
	) {
		if pgno == 1 {
			self.seed_main_page(config, kind, page);
		} else {
			self.cache_page(config, kind, pgno, page);
		}
	}

	fn seed_main_page(&mut self, config: &VfsConfig, kind: PageCacheInsertKind, page: Vec<u8>) {
		if let Some(page_size) = sqlite_header_page_size(&page) {
			self.page_size = page_size;
		}
		if let Some(db_size_pages) = sqlite_header_db_size_pages(&page) {
			self.db_size_pages = db_size_pages;
		}
		self.cache_page(config, kind, 1, page);
	}

	fn invalidate_page_cache(&mut self) {
		if self.profiling_enabled {
			self.prefetch_unused_total
				.fetch_add(self.prefetched_pages.len() as u64, Ordering::Relaxed);
			self.prefetched_pages.clear_sync();
		}
		self.page_cache.invalidate_all();
		self.committed_page_cache.invalidate_all();
		self.protected_page_cache.clear_sync();
	}
}

fn build_page_cache(config: &VfsConfig) -> Cache<u32, Vec<u8>> {
	let mut page_cache_builder = Cache::builder().max_capacity(config.cache_capacity_pages);
	if config.staging_cache_ttl_ms > 0 {
		page_cache_builder =
			page_cache_builder.time_to_live(Duration::from_millis(config.staging_cache_ttl_ms));
	}
	page_cache_builder.build()
}

fn cache_page(
	config: &VfsConfig,
	page_cache: &Cache<u32, Vec<u8>>,
	_protected_page_cache: &SccHashMap<u32, Vec<u8>>,
	prefetched_pages: &scc::HashSet<u32>,
	prefetch_unused_total: &AtomicU64,
	profiling_enabled: bool,
	kind: PageCacheInsertKind,
	pgno: u32,
	bytes: Vec<u8>,
) {
	if !should_cache_page(config, kind, pgno) {
		return;
	}
	if !profiling_enabled {
		page_cache.insert(pgno, bytes);
		return;
	}
	if kind == PageCacheInsertKind::Prefetch {
		let capacity = config.cache_capacity_pages.max(1) as usize;
		if prefetched_pages.len() >= capacity {
			prefetch_unused_total.fetch_add(prefetched_pages.len() as u64, Ordering::Relaxed);
			prefetched_pages.clear_sync();
		}
		let _ = prefetched_pages.insert_sync(pgno);
	} else if prefetched_pages.remove_sync(&pgno).is_some() {
		prefetch_unused_total.fetch_add(1, Ordering::Relaxed);
	}
	page_cache.insert(pgno, bytes);
}

fn should_cache_page(config: &VfsConfig, kind: PageCacheInsertKind, pgno: u32) -> bool {
	match kind {
		PageCacheInsertKind::Target => false,
		PageCacheInsertKind::Prefetch => {
			config.staging_cache_ttl_ms > 0 && config.page_cache_mode.caches_prefetched_pages()
		}
		PageCacheInsertKind::Startup => {
			pgno == 1
				|| (config.staging_cache_ttl_ms > 0
					&& config.page_cache_mode.caches_startup_preloaded_pages())
		}
	}
}

fn can_read_cached_page(config: &VfsConfig, pgno: u32) -> bool {
	pgno == 1 || config.page_cache_mode.caches_any_pages()
}

impl VfsContext {
	fn new(
		actor_id: String,
		generation: Option<u64>,
		runtime: Handle,
		transport: SqliteTransportHandle,
		config: VfsConfig,
		io_methods: sqlite3_io_methods,
		initial_pages: impl Into<InitialPages>,
		metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	) -> std::result::Result<Self, String> {
		let profiling_enabled = metrics
			.as_ref()
			.is_some_and(|metrics| metrics.profiling_enabled());
		let mut state = VfsState::new(&config, profiling_enabled);
		let initial_pages = initial_pages.into();
		state.head_txid = initial_pages.head_txid;
		for (pgno, page) in initial_pages.pages {
			state.seed_page(&config, PageCacheInsertKind::Startup, pgno, page);
		}

		Ok(Self {
			actor_id,
			generation,
			runtime,
			transport,
			config: config.clone(),
			state: RwLock::new(state),
			aux_files: RwLock::new(BTreeMap::new()),
			last_error: Mutex::new(None),
			transient_commit_error: Mutex::new(None),
			fatal_error: RwLock::new(None),
			#[cfg(test)]
			fail_next_aux_open: Mutex::new(None),
			#[cfg(test)]
			fail_next_aux_delete: Mutex::new(None),
			commit_atomic_count: AtomicU64::new(0),
			io_methods: Box::new(io_methods),
			resolve_pages_total: AtomicU64::new(0),
			resolve_pages_cache_hits: AtomicU64::new(0),
			resolve_pages_fetches: AtomicU64::new(0),
			pages_fetched_total: AtomicU64::new(0),
			prefetch_pages_total: AtomicU64::new(0),
			commit_total: AtomicU64::new(0),
			commit_request_build_ns: AtomicU64::new(0),
			commit_serialize_ns: AtomicU64::new(0),
			commit_transport_ns: AtomicU64::new(0),
			commit_state_update_ns: AtomicU64::new(0),
			commit_duration_ns_total: AtomicU64::new(0),
			operation_profile_active: AtomicBool::new(false),
			operation_profile: Mutex::new(None),
			operation_prefetch_unused_start: AtomicU64::new(0),
			metrics,
		})
	}

	fn begin_operation_profile(&self) {
		if self
			.metrics
			.as_ref()
			.is_some_and(|metrics| metrics.profiling_enabled())
		{
			let unused = self
				.state
				.read()
				.prefetch_unused_total
				.load(Ordering::Relaxed);
			self.operation_prefetch_unused_start
				.store(unused, Ordering::Relaxed);
			*self.operation_profile.lock() = Some(SqliteOperationProfile::default());
			self.operation_profile_active.store(true, Ordering::Release);
		}
	}

	fn finish_operation_profile(&self) -> SqliteOperationProfile {
		if !self.operation_profile_active.swap(false, Ordering::AcqRel) {
			return SqliteOperationProfile::default();
		}
		let mut profile = self.operation_profile.lock().take().unwrap_or_default();
		let unused = self
			.state
			.read()
			.prefetch_unused_total
			.load(Ordering::Relaxed)
			.saturating_sub(self.operation_prefetch_unused_start.load(Ordering::Relaxed));
		profile.prefetch_unused_pages = unused;
		profile
	}

	fn update_operation_profile(&self, update: impl FnOnce(&mut SqliteOperationProfile)) {
		if !self.operation_profile_active.load(Ordering::Relaxed) {
			return;
		}
		if let Some(profile) = self.operation_profile.lock().as_mut() {
			update(profile);
		}
	}

	fn profile_request_limit(&self) -> usize {
		self.metrics
			.as_ref()
			.map_or(0, |metrics| metrics.max_profiled_get_pages_requests())
			.min(MAX_PROFILED_GET_PAGES_REQUESTS)
	}

	fn clear_last_error(&self) {
		*self.last_error.lock() = None;
	}

	fn set_last_error(&self, message: String) {
		*self.last_error.lock() = Some(message);
	}

	fn clone_last_error(&self) -> Option<String> {
		self.last_error.lock().clone()
	}

	fn clone_fatal_error(&self) -> Option<String> {
		self.fatal_error.read().clone()
	}

	fn defer_transient_commit_error(&self, message: String) {
		*self.transient_commit_error.lock() = Some(message);
	}

	fn take_transient_commit_error(&self) -> Option<String> {
		self.transient_commit_error.lock().take()
	}

	pub(crate) fn take_last_error(&self) -> Option<String> {
		self.last_error.lock().take()
	}

	fn add_commit_phase_metrics(
		&self,
		request_build_ns: u64,
		transport_metrics: CommitTransportMetrics,
		state_update_ns: u64,
		total_ns: u64,
	) {
		self.update_operation_profile(|profile| {
			profile.storage_ns = profile
				.storage_ns
				.saturating_add(transport_metrics.transport_ns);
			profile.commit_round_trips = profile.commit_round_trips.saturating_add(1);
		});
		if let Some(metrics) = &self.metrics {
			metrics.observe_commit_phases(
				request_build_ns,
				transport_metrics.serialize_ns,
				transport_metrics.transport_ns,
				state_update_ns,
				total_ns,
			);
		}
		self.commit_request_build_ns
			.fetch_add(request_build_ns, Ordering::Relaxed);
		self.commit_serialize_ns
			.fetch_add(transport_metrics.serialize_ns, Ordering::Relaxed);
		self.commit_transport_ns
			.fetch_add(transport_metrics.transport_ns, Ordering::Relaxed);
		self.commit_state_update_ns
			.fetch_add(state_update_ns, Ordering::Relaxed);
		self.commit_duration_ns_total
			.fetch_add(total_ns, Ordering::Relaxed);
	}

	fn sqlite_vfs_metrics(&self) -> SqliteVfsMetricsSnapshot {
		let state = self.state.read();

		SqliteVfsMetricsSnapshot {
			request_build_ns: self.commit_request_build_ns.load(Ordering::Relaxed),
			serialize_ns: self.commit_serialize_ns.load(Ordering::Relaxed),
			transport_ns: self.commit_transport_ns.load(Ordering::Relaxed),
			state_update_ns: self.commit_state_update_ns.load(Ordering::Relaxed),
			total_ns: self.commit_duration_ns_total.load(Ordering::Relaxed),
			commit_count: self.commit_total.load(Ordering::Relaxed),
			page_cache_entries: state
				.page_cache
				.entry_count()
				.saturating_add(state.committed_page_cache.entry_count())
				.saturating_add(state.protected_page_cache.len() as u64),
			page_cache_weighted_size: state
				.page_cache
				.weighted_size()
				.saturating_add(state.protected_page_cache.len() as u64),
			page_cache_capacity_pages: self.config.cache_capacity_pages,
			write_buffer_dirty_pages: state.write_buffer.dirty.len() as u64,
			db_size_pages: state.db_size_pages as u64,
		}
	}

	fn block_on_buffered_commit(
		&self,
		request: BufferedCommitRequest,
		timeout: Option<Duration>,
	) -> std::result::Result<
		CommitWait<(BufferedCommitOutcome, CommitTransportMetrics)>,
		CommitBufferError,
	> {
		let commit = commit_buffered_pages(&*self.transport, request);
		let result = if let Some(timeout) = timeout {
			match self
				.runtime
				.block_on(async { tokio::time::timeout(timeout, commit).await })
			{
				Ok(result) => result,
				Err(_) => return Ok(CommitWait::TimedOut),
			}
		} else {
			self.runtime.block_on(commit)
		};

		result.map(CommitWait::Completed)
	}

	fn round_trip_counts(&self) -> SqliteRoundTripCounts {
		SqliteRoundTripCounts {
			get_pages: self.resolve_pages_fetches.load(Ordering::Relaxed),
			commit: self.commit_total.load(Ordering::Relaxed),
		}
	}

	fn page_size(&self) -> usize {
		self.state.read().page_size.max(DEFAULT_PAGE_SIZE)
	}

	fn open_aux_file(&self, path: &str) -> Arc<AuxFileState> {
		let mut aux_files = self.aux_files.write();
		aux_files
			.entry(path.to_string())
			.or_insert_with(|| Arc::new(AuxFileState::default()))
			.clone()
	}

	fn aux_file_exists(&self, path: &str) -> bool {
		self.aux_files.read().contains_key(path)
	}

	fn delete_aux_file(&self, path: &str) {
		self.aux_files.write().remove(path);
	}

	#[cfg(test)]
	fn fail_next_aux_open(&self, message: impl Into<String>) {
		*self.fail_next_aux_open.lock() = Some(message.into());
	}

	#[cfg(test)]
	fn take_aux_open_error(&self) -> Option<String> {
		self.fail_next_aux_open.lock().take()
	}

	#[cfg(test)]
	fn fail_next_aux_delete(&self, message: impl Into<String>) {
		*self.fail_next_aux_delete.lock() = Some(message.into());
	}

	#[cfg(test)]
	fn take_aux_delete_error(&self) -> Option<String> {
		self.fail_next_aux_delete.lock().take()
	}

	fn is_dead(&self) -> bool {
		self.state.read().dead
	}

	fn mark_fatal(&self, message: String) {
		self.set_last_error(message.clone());
		self.state.write().dead = true;
		let mut fatal_error = self.fatal_error.write();
		if fatal_error.is_none() {
			*fatal_error = Some(message);
		}
	}

	pub(crate) fn snapshot_preload_hints(&self) -> VfsPreloadHintSnapshot {
		if !self.config.recent_page_hints {
			return VfsPreloadHintSnapshot::default();
		}
		let state = self.state.read();
		let mut snapshot = state.recent_pages.snapshot();
		if self.config.preload_hint_early_pages {
			let mut existing_pgnos = snapshot.pgnos.iter().copied().collect::<HashSet<_>>();
			let early_page_count = self
				.config
				.startup_preload_first_page_count
				.min(state.db_size_pages);
			for pgno in 1..=early_page_count {
				if !snapshot.ranges.iter().any(|range| range.contains(pgno))
					&& existing_pgnos.insert(pgno)
				{
					snapshot.pgnos.push(pgno);
				}
			}
			snapshot.pgnos.sort_unstable();
		}
		snapshot
	}

	fn resolve_pages(
		&self,
		target_pgnos: &[u32],
		prefetch: bool,
	) -> std::result::Result<HashMap<u32, Option<Vec<u8>>>, GetPagesError> {
		use std::sync::atomic::Ordering::Relaxed;
		self.resolve_pages_total.fetch_add(1, Relaxed);
		self.update_operation_profile(|profile| {
			profile.sqlite_requested_pages = profile
				.sqlite_requested_pages
				.saturating_add(target_pgnos.len() as u64);
		});
		if let Some(metrics) = &self.metrics {
			metrics.record_resolve_pages(target_pgnos.len() as u64);
		}

		let mut resolved = HashMap::new();
		let mut missing = Vec::new();
		let mut seen = HashSet::new();
		let mut prefetch_consumed = 0_u64;

		{
			let state = self.state.read();
			if state.dead {
				return Err(GetPagesError::Other(
					"sqlite actor lost its fence".to_string(),
				));
			}

			for pgno in target_pgnos.iter().copied() {
				if !seen.insert(pgno) {
					continue;
				}
				if let Some(bytes) = state.write_buffer.dirty.get(&pgno) {
					resolved.insert(pgno, Some(bytes.clone()));
					continue;
				}
				if let Some((bytes, was_prefetched)) = state.cached_page(&self.config, pgno) {
					prefetch_consumed = prefetch_consumed.saturating_add(u64::from(was_prefetched));
					resolved.insert(pgno, Some(bytes));
					continue;
				}
				missing.push(pgno);
			}
		}
		if prefetch_consumed > 0 {
			self.update_operation_profile(|profile| {
				profile.prefetch_consumed_pages = profile
					.prefetch_consumed_pages
					.saturating_add(prefetch_consumed);
			});
		}

		if missing.is_empty() {
			self.resolve_pages_cache_hits
				.fetch_add(target_pgnos.len() as u64, Relaxed);
			let mut state = self.state.write();
			state.record_targets(
				&self.config,
				target_pgnos,
				&resolved,
				self.config.cache_hit_predictor_training,
			);
			if self.config.recent_page_hints {
				state
					.recent_pages
					.record_pages(target_pgnos.iter().copied());
			}
			if let Some(metrics) = &self.metrics {
				metrics.record_resolve_cache_hits(target_pgnos.len() as u64);
			}
			self.update_operation_profile(|profile| {
				profile.cache_hit_pages = profile
					.cache_hit_pages
					.saturating_add(target_pgnos.len() as u64);
			});
			return Ok(resolved);
		}
		self.resolve_pages_cache_hits
			.fetch_add((seen.len() - missing.len()) as u64, Relaxed);
		if let Some(metrics) = &self.metrics {
			metrics.record_resolve_cache_hits((seen.len() - missing.len()) as u64);
			metrics.record_resolve_cache_misses(missing.len() as u64);
		}
		self.update_operation_profile(|profile| {
			profile.cache_hit_pages = profile
				.cache_hit_pages
				.saturating_add((seen.len() - missing.len()) as u64);
			profile.cache_miss_pages = profile
				.cache_miss_pages
				.saturating_add(missing.len() as u64);
		});

		let (
			to_fetch,
			page_size,
			read_ahead_mode,
			read_ahead_depth,
			read_ahead_max_bytes,
			seed_pgno,
			prediction_budget,
			predicted_pgnos,
			skipped_cached_predicted_pages,
			db_size_pages,
			expected_head_txid,
		) = {
			let mut state = self.state.write();
			let ClassifiedReadAheadPlan {
				plan: read_ahead_plan,
				seed_class,
			} = state.record_targets(&self.config, target_pgnos, &resolved, true);
			if self.config.recent_page_hints {
				state
					.recent_pages
					.record_pages(target_pgnos.iter().copied());
			}

			let mut to_fetch = missing.clone();
			let seed_pgno = read_ahead_plan.seed_pgno;
			let mut prediction_budget = 0;
			let mut predicted_pgnos = Vec::new();
			let mut skipped_cached_predicted_pages = 0;
			if prefetch {
				let page_budget = (read_ahead_plan.max_bytes / state.page_size.max(1)).max(1);
				prediction_budget = page_budget.saturating_sub(to_fetch.len());
				let seed = seed_pgno.unwrap_or_default();
				predicted_pgnos = state.predictor.multi_predict(
					seed_class,
					seed,
					prediction_budget.min(read_ahead_plan.depth),
					state.db_size_pages.max(seed),
				);
				for predicted in predicted_pgnos.iter().copied() {
					if resolved.contains_key(&predicted) || to_fetch.contains(&predicted) {
						continue;
					}
					if state.has_readable_page(&self.config, predicted) {
						skipped_cached_predicted_pages += 1;
						continue;
					}
					to_fetch.push(predicted);
				}
			}
			(
				to_fetch,
				state.page_size.max(1),
				read_ahead_plan.mode,
				read_ahead_plan.depth,
				read_ahead_plan.max_bytes,
				seed_pgno,
				prediction_budget,
				predicted_pgnos,
				skipped_cached_predicted_pages,
				state.db_size_pages,
				state.head_txid,
			)
		};

		{
			let prefetch_count = to_fetch.len() - missing.len();
			self.resolve_pages_fetches.fetch_add(1, Relaxed);
			self.pages_fetched_total
				.fetch_add(to_fetch.len() as u64, Relaxed);
			self.prefetch_pages_total
				.fetch_add(prefetch_count as u64, Relaxed);
			if let Some(metrics) = &self.metrics {
				metrics.record_get_pages_request(
					to_fetch.len() as u64,
					prefetch_count as u64,
					page_size as u64,
				);
			}
			tracing::info!(
				actor_id = %self.actor_id,
				generation = ?self.generation,
				requested_pages = ?target_pgnos,
				missing_pages = ?missing,
				read_ahead_mode = ?read_ahead_mode,
				read_ahead_depth,
				read_ahead_max_bytes,
				prediction_budget,
				predicted_pages = ?predicted_pgnos,
				skipped_cached_predicted_pages,
				prefetch_pages = prefetch_count,
				total_fetch_pages = to_fetch.len(),
				total_fetch_bytes = to_fetch.len().saturating_mul(page_size),
				seed_pgno,
				db_size_pages,
				expected_head_txid,
				"vfs get_pages fetch"
			);
		}

		let get_pages_start = Instant::now();
		// Transport rejection, including envoy shutdown while a VFS callback is
		// active, becomes GetPagesError here. The SQLite callback maps that to
		// SQLITE_IOERR_* because VFS has no richer async transport error channel.
		let response =
			self.runtime
				.block_on(self.transport.get_pages(protocol::SqliteGetPagesRequest {
					actor_id: self.actor_id.clone(),
					pgnos: to_fetch.clone(),
					expected_generation: None,
					expected_head_txid,
				}));
		let get_pages_duration_ns = get_pages_start.elapsed().as_nanos() as u64;
		if let Some(metrics) = &self.metrics {
			metrics.observe_get_pages_duration(get_pages_duration_ns);
		}
		let prefetch_count = to_fetch.len().saturating_sub(missing.len()) as u64;
		let profile_active = self.operation_profile_active.load(Ordering::Relaxed);
		let ordinal = if profile_active {
			self.operation_profile
				.lock()
				.as_ref()
				.map_or(1, |profile| profile.get_pages_round_trips.saturating_add(1))
		} else {
			1
		};
		let response = match response {
			Ok(response) => response,
			Err(err) => {
				self.update_operation_profile(|profile| {
					profile.push_get_pages(
						SqliteGetPagesProfile {
							ordinal,
							duration_ns: get_pages_duration_ns,
							demand_requested: missing.len() as u64,
							prefetch_requested: prefetch_count,
							..Default::default()
						},
						self.profile_request_limit(),
					);
				});
				return Err(GetPagesError::Other(err.to_string()));
			}
		};

		match response {
			protocol::SqliteGetPagesResponse::SqliteGetPagesOk(ok) => {
				if profile_active {
					let requested = to_fetch.iter().copied().collect::<HashSet<_>>();
					let mut request_profile = SqliteGetPagesProfile {
						ordinal,
						duration_ns: get_pages_duration_ns,
						demand_requested: missing.len() as u64,
						prefetch_requested: prefetch_count,
						success: true,
						..Default::default()
					};
					let mut btree_pages = 0_u64;
					let mut non_btree_pages = 0_u64;
					for fetched in &ok.pages {
						if !requested.contains(&fetched.pgno) {
							request_profile.overflow_expansion_extra =
								request_profile.overflow_expansion_extra.saturating_add(1);
						}
						if let Some(bytes) = &fetched.bytes {
							request_profile.response_present =
								request_profile.response_present.saturating_add(1);
							request_profile.response_bytes = request_profile
								.response_bytes
								.saturating_add(bytes.len() as u64);
							match classify(fetched.pgno, bytes) {
								PageClass::Btree => btree_pages = btree_pages.saturating_add(1),
								PageClass::Overflow => {
									non_btree_pages = non_btree_pages.saturating_add(1)
								}
							}
						} else {
							request_profile.response_missing =
								request_profile.response_missing.saturating_add(1);
						}
					}
					self.update_operation_profile(|profile| {
						profile.btree_pages = profile.btree_pages.saturating_add(btree_pages);
						profile.non_btree_pages =
							profile.non_btree_pages.saturating_add(non_btree_pages);
						profile.push_get_pages(request_profile, self.profile_request_limit());
					});
				}
				let response_head_txid = ok.head_txid;
				if let Some(head_txid) = response_head_txid {
					self.state.write().head_txid = Some(head_txid);
				}
				let missing_pages = missing.iter().copied().collect::<HashSet<_>>();
				let (page_cache, protected_page_cache, prefetched_pages, prefetch_unused_total) = {
					let state = self.state.read();
					(
						state.page_cache.clone(),
						state.protected_page_cache.clone(),
						state.prefetched_pages.clone(),
						state.prefetch_unused_total.clone(),
					)
				};
				#[cfg(debug_assertions)]
				let mut returned_pgnos = HashSet::new();
				#[cfg(debug_assertions)]
				let mut returned_missing_pages = Vec::new();
				#[cfg(debug_assertions)]
				let mut returned_missing_in_range_pages = Vec::new();
				for fetched in ok.pages {
					#[cfg(debug_assertions)]
					{
						returned_pgnos.insert(fetched.pgno);
						if fetched.bytes.is_none() {
							returned_missing_pages.push(fetched.pgno);
							if fetched.pgno <= db_size_pages {
								returned_missing_in_range_pages.push(fetched.pgno);
							}
						}
					}
					let bytes = if fetched.bytes.is_none()
						&& self.commit_total.load(Relaxed) == 0
						&& missing_pages.contains(&fetched.pgno)
						&& fetched.pgno == 1
					{
						self.state.write().head_txid = Some(0);
						Some(empty_db_page())
					} else {
						fetched.bytes
					};
					if let Some(bytes) = &bytes {
						let kind = if missing_pages.contains(&fetched.pgno) {
							PageCacheInsertKind::Target
						} else {
							PageCacheInsertKind::Prefetch
						};
						cache_page(
							&self.config,
							&page_cache,
							&protected_page_cache,
							&prefetched_pages,
							&prefetch_unused_total,
							profile_active,
							kind,
							fetched.pgno,
							bytes.clone(),
						);
					}
					resolved.insert(fetched.pgno, bytes);
				}
				#[cfg(debug_assertions)]
				{
					let absent_response_pages = to_fetch
						.iter()
						.copied()
						.filter(|pgno| !returned_pgnos.contains(pgno))
						.collect::<Vec<_>>();
					let absent_in_range_pages = absent_response_pages
						.iter()
						.copied()
						.filter(|pgno| *pgno <= db_size_pages)
						.collect::<Vec<_>>();
					if !returned_missing_in_range_pages.is_empty()
						|| !absent_in_range_pages.is_empty()
					{
						tracing::warn!(
							actor_id = %self.actor_id,
							requested_pages = ?target_pgnos,
							missing_pages = ?missing,
							fetch_pages = ?to_fetch,
							db_size_pages,
							returned_missing_pages = ?returned_missing_pages,
							returned_missing_in_range_pages = ?returned_missing_in_range_pages,
							absent_response_pages = ?absent_response_pages,
							absent_in_range_pages = ?absent_in_range_pages,
							"sqlite get_pages returned missing pages within declared db size"
						);
					}
				}
				for pgno in missing {
					resolved.entry(pgno).or_insert(None);
				}
				Ok(resolved)
			}
			protocol::SqliteGetPagesResponse::SqliteErrorResponse(error) => {
				self.update_operation_profile(|profile| {
					profile.push_get_pages(
						SqliteGetPagesProfile {
							ordinal,
							duration_ns: get_pages_duration_ns,
							demand_requested: missing.len() as u64,
							prefetch_requested: prefetch_count,
							..Default::default()
						},
						self.profile_request_limit(),
					);
				});
				if self.commit_total.load(Relaxed) == 0
					&& missing.contains(&1)
					&& is_initial_main_page_missing(&error.message)
				{
					for pgno in missing {
						let bytes = if pgno == 1 {
							Some(empty_db_page())
						} else {
							None
						};
						resolved.entry(pgno).or_insert(bytes);
					}
					return Ok(resolved);
				}
				if is_head_fence_mismatch_response(&error) {
					return Err(GetPagesError::FenceMismatch(error.message));
				}
				Err(GetPagesError::Other(error.message))
			}
		}
	}

	fn flush_dirty_pages(
		&self,
	) -> std::result::Result<Option<BufferedCommitOutcome>, CommitBufferError> {
		match self.flush_dirty_pages_with_timeout(None)? {
			CommitWait::Completed(outcome) => Ok(outcome),
			CommitWait::TimedOut => Err(CommitBufferError::Other(
				"sqlite commit timed out".to_string(),
			)),
		}
	}

	fn flush_dirty_pages_with_timeout(
		&self,
		timeout: Option<Duration>,
	) -> std::result::Result<CommitWait<Option<BufferedCommitOutcome>>, CommitBufferError> {
		let total_start = Instant::now();
		let request_build_start = Instant::now();
		let request = {
			let state = self.state.read();
			if state.dead {
				return Err(CommitBufferError::Other(
					"sqlite actor lost its fence".to_string(),
				));
			}
			if state.write_buffer.in_atomic_write || state.write_buffer.dirty.is_empty() {
				return Ok(CommitWait::Completed(None));
			}

			BufferedCommitRequest {
				actor_id: self.actor_id.clone(),
				new_db_size_pages: state.db_size_pages,
				expected_head_txid: state.head_txid,
				dirty_pages: state
					.write_buffer
					.dirty
					.iter()
					.map(|(pgno, bytes)| protocol::SqliteDirtyPage {
						pgno: *pgno,
						bytes: bytes.clone(),
					})
					.collect(),
			}
		};
		let request_build_ns = request_build_start.elapsed().as_nanos() as u64;
		self.update_operation_profile(|profile| {
			profile.dirty_pages = profile
				.dirty_pages
				.saturating_add(request.dirty_pages.len() as u64);
			profile.dirty_bytes = profile.dirty_bytes.saturating_add(
				request
					.dirty_pages
					.iter()
					.map(|page| page.bytes.len() as u64)
					.sum::<u64>(),
			);
		});

		let (outcome, transport_metrics) =
			// Transport rejection, including envoy shutdown while a VFS callback is
			// active, becomes CommitBufferError here. xSync and xClose then surface
			// it to SQLite as SQLITE_IOERR_*.
			match self.block_on_buffered_commit(request.clone(), timeout) {
				Ok(CommitWait::Completed(outcome)) => outcome,
				Ok(CommitWait::TimedOut) => return Ok(CommitWait::TimedOut),
				Err(err) => {
					tracing::error!(
						actor_id = %self.actor_id,
						new_db_size_pages = request.new_db_size_pages,
						dirty_pages = request.dirty_pages.len(),
						?err,
						"sqlite flush commit failed"
					);
					handle_non_finalize_commit_error(self, &err);
					return Err(err);
				}
			};
		self.commit_total
			.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		if let Some(metrics) = &self.metrics {
			metrics.record_commit();
		}
		tracing::info!(
			dirty_pages = request.dirty_pages.len(),
			path = ?outcome.path,
			requested_db_size_pages = request.new_db_size_pages,
			db_size_pages = outcome.db_size_pages,
			request_build_ns,
			serialize_ns = transport_metrics.serialize_ns,
			transport_ns = transport_metrics.transport_ns,
			"vfs commit complete (flush)"
		);
		#[cfg(debug_assertions)]
		{
			if outcome.db_size_pages != request.new_db_size_pages {
				tracing::warn!(
					actor_id = %self.actor_id,
					dirty_pages = request.dirty_pages.len(),
					path = ?outcome.path,
					requested_db_size_pages = request.new_db_size_pages,
					outcome_db_size_pages = outcome.db_size_pages,
					"sqlite flush commit returned db size different from request"
				);
			}
		}
		let state_update_start = Instant::now();
		let mut state = self.state.write();
		state.db_size_pages = outcome.db_size_pages;
		state.head_txid = outcome
			.head_txid
			.or_else(|| state.head_txid.map(|head_txid| head_txid.saturating_add(1)));
		for dirty_page in &request.dirty_pages {
			state.cache_committed_page(&self.config, dirty_page.pgno, dirty_page.bytes.clone());
		}
		state.write_buffer.dirty.clear();
		let state_update_ns = state_update_start.elapsed().as_nanos() as u64;
		self.add_commit_phase_metrics(
			request_build_ns,
			transport_metrics,
			state_update_ns,
			total_start.elapsed().as_nanos() as u64,
		);
		Ok(CommitWait::Completed(Some(outcome)))
	}

	fn commit_atomic_write(&self) -> std::result::Result<(), CommitBufferError> {
		match self.commit_atomic_write_with_timeout(None)? {
			CommitWait::Completed(()) => Ok(()),
			CommitWait::TimedOut => Err(CommitBufferError::Other(
				"sqlite commit timed out".to_string(),
			)),
		}
	}

	fn commit_atomic_write_with_timeout(
		&self,
		timeout: Option<Duration>,
	) -> std::result::Result<CommitWait<()>, CommitBufferError> {
		let total_start = Instant::now();
		let request_build_start = Instant::now();
		let request = {
			let mut state = self.state.write();
			if state.dead {
				return Err(CommitBufferError::Other(
					"sqlite actor lost its fence".to_string(),
				));
			}
			if !state.write_buffer.in_atomic_write {
				return Ok(CommitWait::Completed(()));
			}
			if state.write_buffer.dirty.is_empty() {
				state.write_buffer.in_atomic_write = false;
				return Ok(CommitWait::Completed(()));
			}

			BufferedCommitRequest {
				actor_id: self.actor_id.clone(),
				new_db_size_pages: state.db_size_pages,
				expected_head_txid: state.head_txid,
				dirty_pages: state
					.write_buffer
					.dirty
					.iter()
					.map(|(pgno, bytes)| protocol::SqliteDirtyPage {
						pgno: *pgno,
						bytes: bytes.clone(),
					})
					.collect(),
			}
		};
		let request_build_ns = request_build_start.elapsed().as_nanos() as u64;
		self.update_operation_profile(|profile| {
			profile.dirty_pages = profile
				.dirty_pages
				.saturating_add(request.dirty_pages.len() as u64);
			profile.dirty_bytes = profile.dirty_bytes.saturating_add(
				request
					.dirty_pages
					.iter()
					.map(|page| page.bytes.len() as u64)
					.sum::<u64>(),
			);
		});

		let (outcome, transport_metrics) =
			match self.block_on_buffered_commit(request.clone(), timeout) {
				Ok(CommitWait::Completed(outcome)) => outcome,
				Ok(CommitWait::TimedOut) => return Ok(CommitWait::TimedOut),
				Err(err) => {
					tracing::error!(
						actor_id = %self.actor_id,
						new_db_size_pages = request.new_db_size_pages,
						dirty_pages = request.dirty_pages.len(),
						?err,
						"sqlite atomic commit failed"
					);
					handle_non_finalize_commit_error(self, &err);
					return Err(err);
				}
			};
		self.commit_total
			.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		if let Some(metrics) = &self.metrics {
			metrics.record_commit();
		}
		tracing::debug!(
			dirty_pages = request.dirty_pages.len(),
			path = ?outcome.path,
			requested_db_size_pages = request.new_db_size_pages,
			db_size_pages = outcome.db_size_pages,
			request_build_ns,
			serialize_ns = transport_metrics.serialize_ns,
			transport_ns = transport_metrics.transport_ns,
			"vfs commit complete (atomic)"
		);
		#[cfg(debug_assertions)]
		{
			if outcome.db_size_pages != request.new_db_size_pages {
				tracing::warn!(
					actor_id = %self.actor_id,
					dirty_pages = request.dirty_pages.len(),
					path = ?outcome.path,
					requested_db_size_pages = request.new_db_size_pages,
					outcome_db_size_pages = outcome.db_size_pages,
					"sqlite atomic commit returned db size different from request"
				);
			}
		}
		self.clear_last_error();
		let state_update_start = Instant::now();
		let mut state = self.state.write();
		state.db_size_pages = outcome.db_size_pages;
		state.head_txid = outcome
			.head_txid
			.or_else(|| state.head_txid.map(|head_txid| head_txid.saturating_add(1)));
		for dirty_page in &request.dirty_pages {
			state.cache_committed_page(&self.config, dirty_page.pgno, dirty_page.bytes.clone());
		}
		state.write_buffer.dirty.clear();
		state.write_buffer.in_atomic_write = false;
		let state_update_ns = state_update_start.elapsed().as_nanos() as u64;
		self.add_commit_phase_metrics(
			request_build_ns,
			transport_metrics,
			state_update_ns,
			total_start.elapsed().as_nanos() as u64,
		);
		Ok(CommitWait::Completed(()))
	}

	fn truncate_main_file(&self, size: sqlite3_int64) {
		let page_size = self.page_size() as i64;
		let truncated_pages = ((size + page_size - 1) / page_size) as u32;
		let mut state = self.state.write();
		state.db_size_pages = truncated_pages;
		state
			.write_buffer
			.dirty
			.retain(|pgno, _| *pgno <= truncated_pages);
		state.invalidate_page_cache();
	}
}

impl Drop for VfsContext {
	fn drop(&mut self) {
		let state = self.state.read();
		let page_cache_entries = state
			.page_cache
			.entry_count()
			.saturating_add(state.committed_page_cache.entry_count())
			.saturating_add(state.protected_page_cache.len() as u64);
		let page_cache_weighted_size = state
			.page_cache
			.weighted_size()
			.saturating_add(state.protected_page_cache.len() as u64);
		tracing::debug!(
			actor_id = %self.actor_id,
			generation = ?self.generation,
			resolve_pages_calls = self.resolve_pages_total.load(Ordering::Relaxed),
			resolve_pages_cache_hits = self.resolve_pages_cache_hits.load(Ordering::Relaxed),
			get_pages_round_trips = self.resolve_pages_fetches.load(Ordering::Relaxed),
			pages_fetched_total = self.pages_fetched_total.load(Ordering::Relaxed),
			prefetch_pages_total = self.prefetch_pages_total.load(Ordering::Relaxed),
			commit_count = self.commit_total.load(Ordering::Relaxed),
			db_size_pages = state.db_size_pages,
			head_txid = state.head_txid,
			page_cache_entries,
			page_cache_weighted_size,
			page_cache_capacity_pages = self.config.cache_capacity_pages,
			"sqlite vfs close summary"
		);
	}
}

fn cleanup_batch_atomic_probe(db: *mut sqlite3) {
	if let Err(err) = sqlite_exec(db, "DROP TABLE IF EXISTS __rivet_batch_probe;") {
		tracing::warn!(%err, "failed to clean up sqlite batch atomic probe table");
	}
}

fn assert_batch_atomic_probe(db: *mut sqlite3, vfs: &SqliteVfs) -> std::result::Result<(), String> {
	let commit_atomic_before = vfs.commit_atomic_count();
	let probe_sql = "\
		BEGIN IMMEDIATE;\
		CREATE TABLE IF NOT EXISTS __rivet_batch_probe(x INTEGER);\
		INSERT INTO __rivet_batch_probe VALUES(1);\
		DELETE FROM __rivet_batch_probe;\
		DROP TABLE IF EXISTS __rivet_batch_probe;\
		COMMIT;\
	";

	if let Err(err) = sqlite_exec(db, probe_sql) {
		let last_error = vfs.clone_last_error();
		tracing::error!(
			%err,
			last_error = ?last_error,
			commit_atomic_before,
			"sqlite batch atomic probe failed"
		);
		cleanup_batch_atomic_probe(db);
		if let Some(last_error) = last_error {
			return Err(format!(
				"batch atomic probe failed: {err}; vfs last_error: {last_error}"
			));
		}
		return Err(format!("batch atomic probe failed: {err}"));
	}

	let commit_atomic_after = vfs.commit_atomic_count();
	if commit_atomic_after == commit_atomic_before {
		tracing::error!(
			commit_atomic_before,
			commit_atomic_after,
			last_error = ?vfs.clone_last_error(),
			"batch atomic writes not active for sqlite, SQLITE_ENABLE_BATCH_ATOMIC_WRITE may be missing"
		);
		cleanup_batch_atomic_probe(db);
		return Err(
			"batch atomic writes not active for sqlite, SQLITE_ENABLE_BATCH_ATOMIC_WRITE may be missing"
				.to_string(),
		);
	}

	Ok(())
}

fn handle_non_finalize_commit_error(ctx: &VfsContext, err: &CommitBufferError) {
	match err {
		CommitBufferError::FenceMismatch(message) => ctx.mark_fatal(message.clone()),
		CommitBufferError::Other(message) => ctx.set_last_error(message.clone()),
	}
}

fn handle_finalize_fence_error(ctx: &VfsContext, err: &CommitBufferError) {
	if let CommitBufferError::FenceMismatch(reason) = err {
		ctx.mark_fatal(reason.clone());
	}
}

#[cfg(test)]
pub(crate) async fn fetch_initial_main_page_for_registration(
	transport: SqliteTransportHandle,
	actor_id: &str,
) -> std::result::Result<Option<Vec<u8>>, String> {
	fetch_initial_pages(transport, actor_id.to_string(), 0, 1)
		.await
		.map(|pages| {
			pages
				.pages
				.into_iter()
				.find(|(pgno, _)| *pgno == 1)
				.map(|(_, bytes)| bytes)
		})
}

pub(crate) async fn fetch_initial_pages_for_registration(
	transport: SqliteTransportHandle,
	actor_id: &str,
	generation: u64,
	config: &VfsConfig,
) -> std::result::Result<InitialPages, String> {
	if !config.startup_preload_first_pages
		|| !config.page_cache_mode.caches_startup_preloaded_pages()
		|| config.startup_preload_max_bytes < DEFAULT_PAGE_SIZE
	{
		return fetch_initial_pages(transport, actor_id.to_string(), generation, 1).await;
	}

	let page_count_from_bytes = config.startup_preload_max_bytes / DEFAULT_PAGE_SIZE;
	let page_count = config
		.startup_preload_first_page_count
		.min(page_count_from_bytes as u32)
		.max(1);
	fetch_initial_pages(transport, actor_id.to_string(), generation, page_count).await
}

async fn fetch_initial_pages(
	transport: SqliteTransportHandle,
	actor_id: String,
	generation: u64,
	page_count: u32,
) -> std::result::Result<InitialPages, String> {
	tracing::info!(
		actor_id = %actor_id,
		generation,
		page_count,
		"sqlite initial page preload request"
	);

	let request_actor_id = actor_id.clone();
	let response = transport
		.get_pages(protocol::SqliteGetPagesRequest {
			actor_id: request_actor_id,
			pgnos: (1..=page_count).collect(),
			expected_generation: None,
			expected_head_txid: None,
		})
		.await;

	match response {
		Ok(protocol::SqliteGetPagesResponse::SqliteGetPagesOk(ok)) => {
			let head_txid = ok.head_txid;
			let pages: Vec<_> = ok
				.pages
				.into_iter()
				.filter_map(|page| page.bytes.map(|bytes| (page.pgno, bytes)))
				.collect();
			tracing::info!(
				actor_id = %actor_id,
				generation,
				page_count,
				loaded_pages = pages.len(),
				head_txid,
				"sqlite initial page preload result"
			);
			Ok(InitialPages {
				pages,
				head_txid,
				requested_page_count: page_count,
			})
		}
		Ok(protocol::SqliteGetPagesResponse::SqliteErrorResponse(error)) => {
			if !is_initial_main_page_missing(&error.message) {
				return Err(format!(
					"sqlite initial page fetch failed: {}",
					error.message
				));
			}
			tracing::info!(
				actor_id = %actor_id,
				generation,
				page_count,
				error = %error.message,
				"sqlite initial page fetch did not find persisted data"
			);
			Ok(InitialPages {
				pages: Vec::new(),
				head_txid: Some(0),
				requested_page_count: page_count,
			})
		}
		Err(err) => Err(format!("sqlite initial page fetch failed: {err}")),
	}
}

fn is_initial_main_page_missing(message: &str) -> bool {
	message.contains("sqlite database was not found in this bucket branch")
		|| message.contains("sqlite meta missing for get_pages")
		|| message == "actor does not exist"
}

fn next_temp_aux_path() -> String {
	format!(
		"{TEMP_AUX_PATH_PREFIX}-{}",
		NEXT_TEMP_AUX_ID.fetch_add(1, Ordering::Relaxed)
	)
}

unsafe fn get_aux_state(file: &VfsFile) -> Option<&AuxFileHandle> {
	(!file.aux.is_null()).then(|| &*file.aux)
}

async fn commit_buffered_pages(
	transport: &dyn SqliteTransport,
	request: BufferedCommitRequest,
) -> std::result::Result<(BufferedCommitOutcome, CommitTransportMetrics), CommitBufferError> {
	let mut metrics = CommitTransportMetrics::default();
	let serialize_start = Instant::now();
	let commit_request = protocol::SqliteCommitRequest {
		actor_id: request.actor_id.clone(),
		dirty_pages: request.dirty_pages.clone(),
		db_size_pages: request.new_db_size_pages,
		now_ms: sqlite_now_ms().map_err(|err| CommitBufferError::Other(err.to_string()))?,
		expected_generation: None,
		expected_head_txid: request.expected_head_txid,
	};
	metrics.serialize_ns += serialize_start.elapsed().as_nanos() as u64;
	let transport_start = Instant::now();
	match transport
		.commit(commit_request)
		.await
		.map_err(|err| CommitBufferError::Other(err.to_string()))?
	{
		protocol::SqliteCommitResponse::SqliteCommitOk(ok) => {
			metrics.transport_ns += transport_start.elapsed().as_nanos() as u64;
			Ok((
				BufferedCommitOutcome {
					path: CommitPath::Fast,
					db_size_pages: request.new_db_size_pages,
					head_txid: ok.head_txid,
				},
				metrics,
			))
		}
		protocol::SqliteCommitResponse::SqliteErrorResponse(error) => {
			if is_head_fence_mismatch_response(&error) {
				Err(CommitBufferError::FenceMismatch(error.message))
			} else {
				Err(CommitBufferError::Other(error.message))
			}
		}
	}
}

fn is_head_fence_mismatch_response(error: &protocol::SqliteErrorResponse) -> bool {
	is_head_fence_mismatch(&error.group, &error.code)
}

unsafe fn get_file(p: *mut sqlite3_file) -> &'static mut VfsFile {
	&mut *(p as *mut VfsFile)
}

unsafe fn get_vfs_ctx(p: *mut sqlite3_vfs) -> &'static VfsContext {
	&*((*p).pAppData as *const VfsContext)
}

fn sqlite_error_message(db: *mut sqlite3) -> String {
	unsafe {
		if db.is_null() {
			"unknown sqlite error".to_string()
		} else {
			CStr::from_ptr(sqlite3_errmsg(db))
				.to_string_lossy()
				.into_owned()
		}
	}
}

fn sqlite_exec(db: *mut sqlite3, sql: &str) -> std::result::Result<(), String> {
	let c_sql = CString::new(sql).map_err(|err| err.to_string())?;
	let rc = unsafe { sqlite3_exec(db, c_sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) };
	if rc != SQLITE_OK {
		return Err(format!(
			"`{sql}` failed with code {rc}: {}",
			sqlite_error_message(db)
		));
	}
	Ok(())
}

#[cfg(test)]
fn sqlite_step_statement(db: *mut sqlite3, sql: &str) -> std::result::Result<(), String> {
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
		return Ok(());
	}

	let result = loop {
		let step_rc = unsafe { sqlite3_step(stmt) };
		if step_rc == SQLITE_DONE {
			break Ok(());
		}
		if step_rc != SQLITE_ROW {
			break Err(format!(
				"`{sql}` step failed with code {step_rc}: {}",
				sqlite_error_message(db)
			));
		}
	};

	unsafe {
		sqlite3_finalize(stmt);
	}

	result
}

#[cfg(test)]
fn sqlite_prepare_statement(
	db: *mut sqlite3,
	sql: &str,
) -> std::result::Result<*mut sqlite3_stmt, String> {
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

	Ok(stmt)
}

#[cfg(test)]
fn sqlite_bind_text_bytes(
	db: *mut sqlite3,
	stmt: *mut sqlite3_stmt,
	index: c_int,
	bytes: &[u8],
	sql: &str,
) -> std::result::Result<(), String> {
	let rc = unsafe {
		sqlite3_bind_text(
			stmt,
			index,
			bytes.as_ptr().cast(),
			bytes.len() as c_int,
			None,
		)
	};
	if rc != SQLITE_OK {
		return Err(format!(
			"`{sql}` bind text index {index} failed with code {rc}: {}",
			sqlite_error_message(db)
		));
	}

	Ok(())
}

#[cfg(test)]
fn sqlite_bind_i64(
	db: *mut sqlite3,
	stmt: *mut sqlite3_stmt,
	index: c_int,
	value: i64,
	sql: &str,
) -> std::result::Result<(), String> {
	let rc = unsafe { sqlite3_bind_int64(stmt, index, value) };
	if rc != SQLITE_OK {
		return Err(format!(
			"`{sql}` bind int index {index} failed with code {rc}: {}",
			sqlite_error_message(db)
		));
	}

	Ok(())
}

#[cfg(test)]
fn sqlite_step_prepared(
	db: *mut sqlite3,
	stmt: *mut sqlite3_stmt,
	sql: &str,
) -> std::result::Result<(), String> {
	let step_rc = unsafe { sqlite3_step(stmt) };
	if step_rc != SQLITE_DONE {
		return Err(format!(
			"`{sql}` step failed with code {step_rc}: {}",
			sqlite_error_message(db)
		));
	}

	Ok(())
}

#[cfg(test)]
fn sqlite_reset_prepared(stmt: *mut sqlite3_stmt, sql: &str) -> std::result::Result<(), String> {
	let rc = unsafe { sqlite3_reset(stmt) };
	if rc != SQLITE_OK {
		return Err(format!("`{sql}` reset failed with code {rc}"));
	}

	Ok(())
}

#[cfg(test)]
fn sqlite_clear_bindings(stmt: *mut sqlite3_stmt, sql: &str) -> std::result::Result<(), String> {
	let rc = unsafe { sqlite3_clear_bindings(stmt) };
	if rc != SQLITE_OK {
		return Err(format!("`{sql}` clear bindings failed with code {rc}"));
	}

	Ok(())
}

#[cfg(test)]
fn sqlite_insert_text_with_int_value(
	db: *mut sqlite3,
	sql: &str,
	text_bytes: &[u8],
	int_value: i64,
) -> std::result::Result<(), String> {
	let stmt = sqlite_prepare_statement(db, sql)?;
	let result = (|| {
		sqlite_bind_text_bytes(db, stmt, 1, text_bytes, sql)?;
		sqlite_bind_i64(db, stmt, 2, int_value, sql)?;
		sqlite_step_prepared(db, stmt, sql)
	})();
	unsafe {
		sqlite3_finalize(stmt);
	}
	result
}

#[cfg(test)]
fn sqlite_query_i64_bind_text(
	db: *mut sqlite3,
	sql: &str,
	text_bytes: &[u8],
) -> std::result::Result<i64, String> {
	let stmt = sqlite_prepare_statement(db, sql)?;
	let result = (|| {
		sqlite_bind_text_bytes(db, stmt, 1, text_bytes, sql)?;
		match unsafe { sqlite3_step(stmt) } {
			SQLITE_ROW => Ok(unsafe { sqlite3_column_int64(stmt, 0) }),
			step_rc => Err(format!(
				"`{sql}` step failed with code {step_rc}: {}",
				sqlite_error_message(db)
			)),
		}
	})();
	unsafe {
		sqlite3_finalize(stmt);
	}
	result
}

fn page_span(offset: i64, length: usize, page_size: usize) -> std::result::Result<Vec<u32>, ()> {
	if offset < 0 {
		return Err(());
	}
	if length == 0 {
		return Ok(Vec::new());
	}

	let start = offset as usize / page_size + 1;
	let end = (offset as usize + length - 1) / page_size + 1;
	Ok((start as u32..=end as u32).collect())
}

unsafe extern "C" fn io_close(p_file: *mut sqlite3_file) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR, {
		if p_file.is_null() {
			return SQLITE_OK;
		}
		let file = get_file(p_file);
		let result = if !file.aux.is_null() {
			let aux = Box::from_raw(file.aux);
			if aux.delete_on_close {
				let ctx = &*file.ctx;
				ctx.delete_aux_file(&aux.path);
			}
			file.aux = ptr::null_mut();
			Ok(())
		} else {
			let ctx = &*file.ctx;
			let should_flush = {
				let state = ctx.state.read();
				state.write_buffer.in_atomic_write || !state.write_buffer.dirty.is_empty()
			};
			if should_flush {
				if ctx.state.read().write_buffer.in_atomic_write {
					ctx.commit_atomic_write().map(|_| ())
				} else {
					ctx.flush_dirty_pages().map(|_| ())
				}
			} else {
				Ok(())
			}
		};
		file.base.pMethods = ptr::null();
		match result {
			Ok(()) => SQLITE_OK,
			Err(err) => {
				let ctx = &*file.ctx;
				handle_finalize_fence_error(ctx, &err);
				SQLITE_IOERR
			}
		}
	})
}

unsafe extern "C" fn io_read(
	p_file: *mut sqlite3_file,
	p_buf: *mut c_void,
	i_amt: c_int,
	i_offset: sqlite3_int64,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_READ, {
		if i_amt <= 0 {
			return SQLITE_OK;
		}

		let file = get_file(p_file);
		if let Some(aux) = get_aux_state(file) {
			if i_offset < 0 {
				return SQLITE_IOERR_READ;
			}

			let offset = i_offset as usize;
			let requested = i_amt as usize;
			let buf = slice::from_raw_parts_mut(p_buf.cast::<u8>(), requested);
			buf.fill(0);

			let bytes = aux.state.bytes.lock();
			if offset >= bytes.len() {
				return SQLITE_IOERR_SHORT_READ;
			}

			let copy_len = requested.min(bytes.len() - offset);
			buf[..copy_len].copy_from_slice(&bytes[offset..offset + copy_len]);
			return if copy_len < requested {
				SQLITE_IOERR_SHORT_READ
			} else {
				SQLITE_OK
			};
		}

		let ctx = &*file.ctx;
		if ctx.is_dead() {
			return SQLITE_IOERR_READ;
		}

		let buf = slice::from_raw_parts_mut(p_buf.cast::<u8>(), i_amt as usize);
		let requested_pages = match page_span(i_offset, i_amt as usize, ctx.page_size()) {
			Ok(pages) => pages,
			Err(_) => return SQLITE_IOERR_READ,
		};
		let page_size = ctx.page_size();
		let (file_size, db_size_pages) = {
			let state = ctx.state.read();
			(
				state.db_size_pages as usize * state.page_size,
				state.db_size_pages,
			)
		};

		let resolved = match ctx.resolve_pages(&requested_pages, true) {
			Ok(pages) => pages,
			Err(GetPagesError::FenceMismatch(message)) => {
				tracing::error!(
					actor_id = %ctx.actor_id,
					requested_pages = ?requested_pages,
					error = %message,
					"sqlite xRead hit fatal sqlite error"
				);
				ctx.mark_fatal(message);
				return SQLITE_IOERR_READ;
			}
			Err(GetPagesError::Other(message)) => {
				tracing::error!(
					actor_id = %ctx.actor_id,
					requested_pages = ?requested_pages,
					error = %message,
					"sqlite xRead failed to resolve pages"
				);
				ctx.set_last_error(message);
				return SQLITE_IOERR_READ;
			}
		};
		ctx.clear_last_error();

		#[cfg(debug_assertions)]
		{
			let missing_in_range_pages = requested_pages
				.iter()
				.copied()
				.filter(|pgno| *pgno <= db_size_pages)
				.filter(|pgno| !matches!(resolved.get(pgno), Some(Some(_))))
				.collect::<Vec<_>>();
			if !missing_in_range_pages.is_empty() {
				tracing::warn!(
					actor_id = %ctx.actor_id,
					offset = i_offset,
					amount = i_amt,
					page_size,
					db_size_pages,
					file_size,
					requested_pages = ?requested_pages,
					missing_in_range_pages = ?missing_in_range_pages,
					"sqlite xRead would zero-fill pages within declared db size"
				);
			}
		}

		buf.fill(0);
		for pgno in requested_pages.iter().copied() {
			let Some(Some(bytes)) = resolved.get(&pgno) else {
				continue;
			};
			let page_start = (pgno as usize - 1) * page_size;
			let copy_start = page_start.max(i_offset as usize);
			let copy_end = (page_start + page_size).min(i_offset as usize + i_amt as usize);
			if copy_start >= copy_end {
				continue;
			}
			let page_offset = copy_start - page_start;
			let dest_offset = copy_start - i_offset as usize;
			let copy_len = copy_end - copy_start;
			buf[dest_offset..dest_offset + copy_len]
				.copy_from_slice(&bytes[page_offset..page_offset + copy_len]);
		}
		if !ctx.config.retain_read_cache {
			ctx.state.read().evict_target_read_pages(&requested_pages);
		}

		if i_offset as usize + i_amt as usize > file_size {
			return SQLITE_IOERR_SHORT_READ;
		}

		SQLITE_OK
	})
}

unsafe extern "C" fn io_write(
	p_file: *mut sqlite3_file,
	p_buf: *const c_void,
	i_amt: c_int,
	i_offset: sqlite3_int64,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_WRITE, {
		if i_amt <= 0 {
			return SQLITE_OK;
		}

		let file = get_file(p_file);
		if let Some(aux) = get_aux_state(file) {
			if i_offset < 0 {
				return SQLITE_IOERR_WRITE;
			}

			let offset = i_offset as usize;
			let source = slice::from_raw_parts(p_buf.cast::<u8>(), i_amt as usize);
			let mut bytes = aux.state.bytes.lock();
			let end = offset + source.len();
			if bytes.len() < end {
				bytes.resize(end, 0);
			}
			bytes[offset..end].copy_from_slice(source);
			return SQLITE_OK;
		}

		let ctx = &*file.ctx;
		if ctx.is_dead() {
			return SQLITE_IOERR_WRITE;
		}

		let page_size = ctx.page_size();
		let source = slice::from_raw_parts(p_buf.cast::<u8>(), i_amt as usize);
		let target_pages = match page_span(i_offset, i_amt as usize, page_size) {
			Ok(pages) => pages,
			Err(_) => return SQLITE_IOERR_WRITE,
		};

		// Fast path: for full-page aligned writes we don't need the existing
		// page data because we're overwriting every byte. Skip resolve_pages
		// to eliminate a round trip to the engine per page. Also, for pages
		// beyond db_size_pages (new allocations), there's nothing to fetch.
		let offset = i_offset as usize;
		let amt = i_amt as usize;
		let is_aligned_full_page = offset % page_size == 0 && amt % page_size == 0;

		let (resolved, existing_db_size_pages) = if is_aligned_full_page {
			(HashMap::new(), None)
		} else {
			let (db_size_pages, pages_to_resolve): (u32, Vec<u32>) = {
				let state = ctx.state.read();
				let known_max = state.db_size_pages;
				(
					known_max,
					target_pages
						.iter()
						.copied()
						.filter(|pgno| *pgno <= known_max)
						.collect(),
				)
			};

			let mut resolved = if pages_to_resolve.is_empty() {
				HashMap::new()
			} else {
				match ctx.resolve_pages(&pages_to_resolve, false) {
					Ok(pages) => pages,
					Err(GetPagesError::FenceMismatch(message)) => {
						ctx.mark_fatal(message);
						return SQLITE_IOERR_WRITE;
					}
					Err(GetPagesError::Other(message)) => {
						ctx.set_last_error(message);
						return SQLITE_IOERR_WRITE;
					}
				}
			};
			for pgno in &target_pages {
				if *pgno > db_size_pages {
					resolved.entry(*pgno).or_insert(None);
				}
			}
			(resolved, Some(db_size_pages))
		};
		#[cfg(debug_assertions)]
		{
			if let Some(db_size_pages) = existing_db_size_pages {
				let missing_existing_pages = target_pages
					.iter()
					.copied()
					.filter(|pgno| *pgno <= db_size_pages)
					.filter(|pgno| !matches!(resolved.get(pgno), Some(Some(_))))
					.collect::<Vec<_>>();
				if !missing_existing_pages.is_empty() {
					tracing::warn!(
						actor_id = %ctx.actor_id,
						offset = i_offset,
						amount = i_amt,
						page_size,
						db_size_pages,
						target_pages = ?target_pages,
						missing_existing_pages = ?missing_existing_pages,
						"sqlite xWrite partial update would synthesize existing pages from zeros"
					);
				}
			}
		}

		let mut dirty_pages = BTreeMap::new();
		for pgno in target_pages {
			let page_start = (pgno as usize - 1) * page_size;
			let patch_start = page_start.max(offset);
			let patch_end = (page_start + page_size).min(offset + amt);
			let Some(copy_len) = patch_end.checked_sub(patch_start) else {
				continue;
			};
			if copy_len == 0 {
				continue;
			}

			let mut page = if is_aligned_full_page {
				vec![0; page_size]
			} else {
				resolved
					.get(&pgno)
					.and_then(|bytes| bytes.clone())
					.unwrap_or_else(|| vec![0; page_size])
			};
			if page.len() < page_size {
				page.resize(page_size, 0);
			}

			let page_offset = patch_start - page_start;
			let source_offset = patch_start - offset;
			page[page_offset..page_offset + copy_len]
				.copy_from_slice(&source[source_offset..source_offset + copy_len]);
			dirty_pages.insert(pgno, page);
		}

		let mut state = ctx.state.write();
		for (pgno, bytes) in dirty_pages {
			state.write_buffer.dirty.insert(pgno, bytes);
		}
		let end_page = ((offset + amt) + page_size - 1) / page_size;
		state.db_size_pages = state.db_size_pages.max(end_page as u32);
		ctx.clear_last_error();
		SQLITE_OK
	})
}

unsafe extern "C" fn io_truncate(p_file: *mut sqlite3_file, size: sqlite3_int64) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_TRUNCATE, {
		if size < 0 {
			return SQLITE_IOERR_TRUNCATE;
		}
		let file = get_file(p_file);
		if let Some(aux) = get_aux_state(file) {
			aux.state.bytes.lock().truncate(size as usize);
			return SQLITE_OK;
		}
		let ctx = &*file.ctx;
		ctx.truncate_main_file(size);
		SQLITE_OK
	})
}

/// xSync returns once `ctx.flush_dirty_pages()` resolves. Durability of those
/// bytes is delegated to depot's `sqlite_commit` reply. If pegboard-envoy ever
/// pre-acks before the FDB tx commit, xSync's durability contract is broken.
unsafe extern "C" fn io_sync(p_file: *mut sqlite3_file, _flags: c_int) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_FSYNC, {
		let file = get_file(p_file);
		if get_aux_state(file).is_some() {
			return SQLITE_OK;
		}
		let ctx = &*file.ctx;
		if let Some(message) = ctx.take_transient_commit_error() {
			ctx.set_last_error(message);
			return SQLITE_IOERR_FSYNC;
		}
		match ctx.flush_dirty_pages() {
			Ok(_) => SQLITE_OK,
			Err(err) => {
				tracing::error!(
					actor_id = %ctx.actor_id,
					last_error = ?ctx.clone_last_error(),
					?err,
					"sqlite sync failed"
				);
				handle_finalize_fence_error(ctx, &err);
				SQLITE_IOERR_FSYNC
			}
		}
	})
}

unsafe extern "C" fn io_file_size(p_file: *mut sqlite3_file, p_size: *mut sqlite3_int64) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_FSTAT, {
		let file = get_file(p_file);
		if let Some(aux) = get_aux_state(file) {
			*p_size = aux.state.bytes.lock().len() as sqlite3_int64;
			return SQLITE_OK;
		}
		let ctx = &*file.ctx;
		let state = ctx.state.read();
		*p_size = (state.db_size_pages as usize * state.page_size) as sqlite3_int64;
		SQLITE_OK
	})
}

// Lock callbacks are intentional no-ops. Pegboard guarantees a single actor
// process per actor_id, the database is opened with `locking_mode=EXCLUSIVE`,
// and only one SQLite connection runs against it, so SQLite's internal lock
// state machine is single-party and has nothing to coordinate with. Flipping
// to a non-EXCLUSIVE locking mode or a multi-connection setup would require
// implementing a real lock state ladder here.
unsafe extern "C" fn io_lock(_p_file: *mut sqlite3_file, _level: c_int) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_LOCK, SQLITE_OK)
}

unsafe extern "C" fn io_unlock(_p_file: *mut sqlite3_file, _level: c_int) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_UNLOCK, SQLITE_OK)
}

unsafe extern "C" fn io_check_reserved_lock(
	_p_file: *mut sqlite3_file,
	p_res_out: *mut c_int,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR, {
		*p_res_out = 0;
		SQLITE_OK
	})
}

unsafe extern "C" fn io_file_control(
	p_file: *mut sqlite3_file,
	op: c_int,
	_p_arg: *mut c_void,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR, {
		let file = get_file(p_file);
		if get_aux_state(file).is_some() {
			return SQLITE_NOTFOUND;
		}
		let ctx = &*file.ctx;

		match op {
			SQLITE_FCNTL_BEGIN_ATOMIC_WRITE => {
				let mut state = ctx.state.write();
				state.write_buffer.in_atomic_write = true;
				state.write_buffer.saved_db_size = state.db_size_pages;
				state.write_buffer.dirty.clear();
				SQLITE_OK
			}
			SQLITE_FCNTL_COMMIT_ATOMIC_WRITE => match ctx.commit_atomic_write() {
				Ok(()) => {
					ctx.commit_atomic_count.fetch_add(1, Ordering::Relaxed);
					SQLITE_OK
				}
				Err(err) => {
					tracing::error!(
						actor_id = %ctx.actor_id,
						last_error = ?ctx.clone_last_error(),
						?err,
						"sqlite atomic write file control failed"
					);
					if let CommitBufferError::Other(message) = &err {
						ctx.defer_transient_commit_error(message.clone());
					}
					handle_finalize_fence_error(ctx, &err);
					SQLITE_IOERR
				}
			},
			SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE => {
				let mut state = ctx.state.write();
				state.write_buffer.dirty.clear();
				state.write_buffer.in_atomic_write = false;
				state.db_size_pages = state.write_buffer.saved_db_size;
				SQLITE_OK
			}
			_ => SQLITE_NOTFOUND,
		}
	})
}

unsafe extern "C" fn io_sector_size(_p_file: *mut sqlite3_file) -> c_int {
	vfs_catch_unwind!(DEFAULT_PAGE_SIZE as c_int, DEFAULT_PAGE_SIZE as c_int)
}

unsafe extern "C" fn io_device_characteristics(p_file: *mut sqlite3_file) -> c_int {
	vfs_catch_unwind!(0, {
		let file = get_file(p_file);
		if get_aux_state(file).is_some() {
			0
		} else {
			#[cfg(test)]
			if !(*file.ctx).config.advertise_batch_atomic {
				return 0;
			}
			SQLITE_IOCAP_BATCH_ATOMIC
		}
	})
}

unsafe extern "C" fn vfs_open(
	p_vfs: *mut sqlite3_vfs,
	z_name: *const c_char,
	p_file: *mut sqlite3_file,
	flags: c_int,
	p_out_flags: *mut c_int,
) -> c_int {
	vfs_catch_unwind!(SQLITE_CANTOPEN, {
		let ctx = get_vfs_ctx(p_vfs);
		let delete_on_close = (flags & SQLITE_OPEN_DELETEONCLOSE) != 0;
		let path = if z_name.is_null() {
			if delete_on_close {
				next_temp_aux_path()
			} else {
				return SQLITE_CANTOPEN;
			}
		} else {
			match CStr::from_ptr(z_name).to_str() {
				Ok(path) => path.to_string(),
				Err(_) => return SQLITE_CANTOPEN,
			}
		};
		let is_main =
			path == ctx.actor_id && !delete_on_close && (flags & SQLITE_OPEN_MAIN_DB) != 0;

		#[cfg(test)]
		if !is_main {
			if let Some(message) = ctx.take_aux_open_error() {
				ctx.set_last_error(message);
				return SQLITE_CANTOPEN;
			}
		}

		let base = sqlite3_file {
			pMethods: ctx.io_methods.as_ref(),
		};
		let aux = if is_main {
			ptr::null_mut()
		} else {
			Box::into_raw(Box::new(AuxFileHandle {
				path: path.clone(),
				state: ctx.open_aux_file(&path),
				delete_on_close,
			}))
		};
		ptr::write(
			p_file.cast::<VfsFile>(),
			VfsFile {
				base,
				ctx: ctx as *const VfsContext,
				aux,
			},
		);

		if !p_out_flags.is_null() {
			*p_out_flags = flags;
		}

		SQLITE_OK
	})
}

unsafe extern "C" fn vfs_delete(
	p_vfs: *mut sqlite3_vfs,
	z_name: *const c_char,
	_sync_dir: c_int,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_DELETE, {
		if z_name.is_null() {
			return SQLITE_OK;
		}

		let ctx = get_vfs_ctx(p_vfs);
		let path = match CStr::from_ptr(z_name).to_str() {
			Ok(path) => path,
			Err(_) => return SQLITE_OK,
		};
		if path == ctx.actor_id {
			// Main database deletion is unsupported because xDelete cannot remove persisted depot state.
			ctx.set_last_error("main database deletion is unsupported".to_string());
			return SQLITE_IOERR_DELETE;
		} else {
			#[cfg(test)]
			if let Some(message) = ctx.take_aux_delete_error() {
				ctx.set_last_error(message);
				return SQLITE_IOERR_DELETE;
			}
			ctx.delete_aux_file(path);
		}
		SQLITE_OK
	})
}

unsafe extern "C" fn vfs_access(
	p_vfs: *mut sqlite3_vfs,
	z_name: *const c_char,
	_flags: c_int,
	p_res_out: *mut c_int,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR_ACCESS, {
		if z_name.is_null() {
			*p_res_out = 0;
			return SQLITE_OK;
		}

		let ctx = get_vfs_ctx(p_vfs);
		let path = match CStr::from_ptr(z_name).to_str() {
			Ok(path) => path,
			Err(_) => {
				*p_res_out = 0;
				return SQLITE_OK;
			}
		};

		*p_res_out = if path == ctx.actor_id || ctx.aux_file_exists(path) {
			1
		} else {
			0
		};
		SQLITE_OK
	})
}

unsafe extern "C" fn vfs_full_pathname(
	_p_vfs: *mut sqlite3_vfs,
	z_name: *const c_char,
	n_out: c_int,
	z_out: *mut c_char,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR, {
		if z_name.is_null() || z_out.is_null() || n_out <= 0 {
			return SQLITE_IOERR;
		}

		let name = CStr::from_ptr(z_name);
		let bytes = name.to_bytes_with_nul();
		if bytes.len() >= n_out as usize {
			return SQLITE_IOERR;
		}

		ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), z_out, bytes.len());
		SQLITE_OK
	})
}

unsafe extern "C" fn vfs_randomness(
	_p_vfs: *mut sqlite3_vfs,
	n_byte: c_int,
	z_out: *mut c_char,
) -> c_int {
	vfs_catch_unwind!(0, {
		let buf = slice::from_raw_parts_mut(z_out.cast::<u8>(), n_byte as usize);
		match getrandom::getrandom(buf) {
			Ok(()) => n_byte,
			Err(_) => 0,
		}
	})
}

unsafe extern "C" fn vfs_sleep(_p_vfs: *mut sqlite3_vfs, microseconds: c_int) -> c_int {
	vfs_catch_unwind!(0, {
		std::thread::sleep(std::time::Duration::from_micros(microseconds as u64));
		microseconds
	})
}

unsafe extern "C" fn vfs_current_time(_p_vfs: *mut sqlite3_vfs, p_time_out: *mut f64) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR, {
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default();
		*p_time_out = 2440587.5 + (now.as_secs_f64() / 86400.0);
		SQLITE_OK
	})
}

unsafe extern "C" fn vfs_get_last_error(
	p_vfs: *mut sqlite3_vfs,
	n_byte: c_int,
	z_err_msg: *mut c_char,
) -> c_int {
	vfs_catch_unwind!(SQLITE_IOERR, {
		if n_byte <= 0 || z_err_msg.is_null() {
			return 0;
		}

		let ctx = get_vfs_ctx(p_vfs);
		let Some(message) = ctx.clone_last_error() else {
			*z_err_msg = 0;
			return 0;
		};

		let bytes = message.as_bytes();
		let max_len = (n_byte as usize).saturating_sub(1);
		let copy_len = bytes.len().min(max_len);
		let dst = z_err_msg.cast::<u8>();
		ptr::copy_nonoverlapping(bytes.as_ptr(), dst, copy_len);
		*dst.add(copy_len) = 0;
		0
	})
}

impl SqliteVfs {
	pub(crate) fn take_last_error(&self) -> Option<String> {
		self.ctx.take_last_error()
	}

	fn clone_last_error(&self) -> Option<String> {
		self.ctx.clone_last_error()
	}

	pub fn clone_fatal_error(&self) -> Option<String> {
		self.ctx.clone_fatal_error()
	}

	pub(crate) fn snapshot_preload_hints(&self) -> VfsPreloadHintSnapshot {
		self.ctx.snapshot_preload_hints()
	}

	pub(crate) fn sqlite_vfs_metrics(&self) -> SqliteVfsMetricsSnapshot {
		self.ctx.sqlite_vfs_metrics()
	}

	#[cfg(test)]
	pub(crate) fn register_with_transport(
		name: &str,
		transport: SqliteTransportHandle,
		actor_id: String,
		runtime: Handle,
		config: VfsConfig,
		metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	) -> std::result::Result<Self, String> {
		Self::register_with_transport_and_initial_pages(
			name,
			transport,
			actor_id,
			runtime,
			config,
			InitialPages::default(),
			metrics,
		)
	}

	#[cfg(test)]
	pub(crate) fn register_with_transport_and_initial_page(
		name: &str,
		transport: SqliteTransportHandle,
		actor_id: String,
		runtime: Handle,
		config: VfsConfig,
		initial_main_page: Option<Vec<u8>>,
		metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	) -> std::result::Result<Self, String> {
		let initial_pages = initial_main_page
			.into_iter()
			.map(|page| (1, page))
			.collect();
		Self::register_with_transport_and_initial_pages(
			name,
			transport,
			actor_id,
			runtime,
			config,
			InitialPages {
				pages: initial_pages,
				head_txid: None,
				requested_page_count: 1,
			},
			metrics,
		)
	}

	pub(crate) fn register_with_transport_and_initial_pages(
		name: &str,
		transport: SqliteTransportHandle,
		actor_id: String,
		runtime: Handle,
		config: VfsConfig,
		initial_pages: InitialPages,
		metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	) -> std::result::Result<Self, String> {
		let mut io_methods: sqlite3_io_methods = unsafe { std::mem::zeroed() };
		io_methods.iVersion = 1;
		io_methods.xClose = Some(io_close);
		io_methods.xRead = Some(io_read);
		io_methods.xWrite = Some(io_write);
		io_methods.xTruncate = Some(io_truncate);
		io_methods.xSync = Some(io_sync);
		io_methods.xFileSize = Some(io_file_size);
		io_methods.xLock = Some(io_lock);
		io_methods.xUnlock = Some(io_unlock);
		io_methods.xCheckReservedLock = Some(io_check_reserved_lock);
		io_methods.xFileControl = Some(io_file_control);
		io_methods.xSectorSize = Some(io_sector_size);
		io_methods.xDeviceCharacteristics = Some(io_device_characteristics);

		let generation = name
			.rsplit_once("-g")
			.and_then(|(_, generation)| generation.parse::<u64>().ok());
		let mut ctx = Box::new(VfsContext::new(
			actor_id,
			generation,
			runtime,
			transport,
			config,
			io_methods,
			initial_pages,
			metrics,
		)?);
		let ctx_ptr = (&mut *ctx) as *mut VfsContext;
		let name_cstring = CString::new(name).map_err(|err| err.to_string())?;

		let mut vfs: sqlite3_vfs = unsafe { std::mem::zeroed() };
		vfs.iVersion = 1;
		vfs.szOsFile = std::mem::size_of::<VfsFile>() as c_int;
		vfs.mxPathname = MAX_PATHNAME;
		vfs.zName = name_cstring.as_ptr();
		vfs.pAppData = ctx_ptr.cast::<c_void>();
		vfs.xOpen = Some(vfs_open);
		vfs.xDelete = Some(vfs_delete);
		vfs.xAccess = Some(vfs_access);
		vfs.xFullPathname = Some(vfs_full_pathname);
		vfs.xRandomness = Some(vfs_randomness);
		vfs.xSleep = Some(vfs_sleep);
		vfs.xCurrentTime = Some(vfs_current_time);
		vfs.xGetLastError = Some(vfs_get_last_error);

		let registration = SqliteVfsRegistration::register(vfs)?;

		Ok(Self {
			_registration: registration,
			_name: name_cstring,
			ctx,
		})
	}

	pub fn name_ptr(&self) -> *const c_char {
		self._name.as_ptr()
	}

	#[cfg(test)]
	fn vfs_ptr(&self) -> *mut sqlite3_vfs {
		self._registration.vfs_ptr
	}

	fn ctx(&self) -> &VfsContext {
		&self.ctx
	}

	fn commit_atomic_count(&self) -> u64 {
		self.ctx.commit_atomic_count.load(Ordering::Relaxed)
	}
}

impl SqliteVfsRegistration {
	fn register(vfs: sqlite3_vfs) -> std::result::Result<Self, String> {
		let vfs_ptr = Box::into_raw(Box::new(vfs));
		let rc = unsafe { sqlite3_vfs_register(vfs_ptr, 0) };
		if rc != SQLITE_OK {
			unsafe {
				drop(Box::from_raw(vfs_ptr));
			}
			return Err(format!("sqlite3_vfs_register failed with code {rc}"));
		}

		Ok(Self { vfs_ptr })
	}
}

impl Drop for SqliteVfsRegistration {
	fn drop(&mut self) {
		unsafe {
			sqlite3_vfs_unregister(self.vfs_ptr);
			drop(Box::from_raw(self.vfs_ptr));
		}
	}
}

impl NativeDatabase {
	pub fn as_ptr(&self) -> *mut sqlite3 {
		self.db
	}

	pub fn take_last_kv_error(&self) -> Option<String> {
		self._vfs.take_last_error()
	}

	pub fn sqlite_vfs_metrics(&self) -> SqliteVfsMetricsSnapshot {
		self._vfs.ctx.sqlite_vfs_metrics()
	}

	pub fn round_trip_counts(&self) -> SqliteRoundTripCounts {
		self._vfs.ctx.round_trip_counts()
	}

	pub(crate) fn begin_operation_profile(&self) -> SqliteOperationProfileGuard<'_> {
		self._vfs.ctx.begin_operation_profile();
		SqliteOperationProfileGuard {
			ctx: &self._vfs.ctx,
			active: true,
		}
	}

	pub fn snapshot_preload_hints(&self) -> VfsPreloadHintSnapshot {
		self._vfs.snapshot_preload_hints()
	}
}

impl Drop for NativeDatabase {
	fn drop(&mut self) {
		if !self.db.is_null() {
			let ctx = self._vfs.ctx();
			let should_flush = {
				let state = ctx.state.read();
				state.write_buffer.in_atomic_write || !state.write_buffer.dirty.is_empty()
			};
			if should_flush {
				let result = if ctx.state.read().write_buffer.in_atomic_write {
					ctx.commit_atomic_write_with_timeout(Some(NATIVE_DATABASE_DROP_FLUSH_TIMEOUT))
				} else {
					ctx.flush_dirty_pages_with_timeout(Some(NATIVE_DATABASE_DROP_FLUSH_TIMEOUT))
						.map(|wait| match wait {
							CommitWait::Completed(_) => CommitWait::Completed(()),
							CommitWait::TimedOut => CommitWait::TimedOut,
						})
				};
				match result {
					Ok(CommitWait::Completed(())) => {}
					Ok(CommitWait::TimedOut) => {
						tracing::error!(
							actor_id = %ctx.actor_id,
							timeout_ms = NATIVE_DATABASE_DROP_FLUSH_TIMEOUT.as_millis(),
							"timed out flushing sqlite database before close"
						);
						self.db = ptr::null_mut();
						return;
					}
					Err(err) => {
						handle_non_finalize_commit_error(ctx, &err);
						tracing::warn!(?err, "failed to flush sqlite database before close");
						self.db = ptr::null_mut();
						return;
					}
				}
			}

			let rc = unsafe { sqlite3_close_v2(self.db) };
			if rc != SQLITE_OK {
				tracing::warn!(
					rc,
					error = sqlite_error_message(self.db),
					"failed to close sqlite database"
				);
			}
			self.db = ptr::null_mut();
		}
	}
}

pub fn open_database(
	vfs: SqliteVfs,
	file_name: &str,
) -> std::result::Result<NativeDatabase, String> {
	open_connection(
		Arc::new(vfs),
		file_name,
		SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
	)
	.and_then(|connection| {
		configure_connection_for_database(connection.as_ptr(), &connection._vfs, file_name)?;
		verify_batch_atomic_writes(connection.as_ptr(), &connection._vfs, file_name)?;
		Ok(connection)
	})
}

pub fn open_connection(
	vfs: NativeVfsHandle,
	file_name: &str,
	flags: c_int,
) -> std::result::Result<NativeConnection, String> {
	let c_name = CString::new(file_name).map_err(|err| err.to_string())?;
	let mut db: *mut sqlite3 = ptr::null_mut();

	let rc = unsafe { sqlite3_open_v2(c_name.as_ptr(), &mut db, flags, vfs.name_ptr()) };
	if rc != SQLITE_OK {
		let message = sqlite_error_message(db);
		tracing::error!(
			file_name,
			rc,
			%message,
			last_error = ?vfs.clone_last_error(),
			"failed to open sqlite database with custom VFS"
		);
		if !db.is_null() {
			unsafe {
				sqlite3_close(db);
			}
		}
		return Err(format!("sqlite3_open_v2 failed with code {rc}: {message}"));
	}

	Ok(NativeDatabase { db, _vfs: vfs })
}

pub fn configure_connection_for_database(
	db: *mut sqlite3,
	vfs: &SqliteVfs,
	file_name: &str,
) -> std::result::Result<(), String> {
	// SQLite interprets a negative cache_size as a KiB budget instead of a page count.
	let cache_size_kib = sqlite_optimization_flags().pager_cache_size_kib;
	let cache_size_pragma = format!("PRAGMA cache_size = -{cache_size_kib};");

	let pragmas = [
		"PRAGMA page_size = 4096;",
		"PRAGMA journal_mode = DELETE;",
		"PRAGMA synchronous = NORMAL;",
		"PRAGMA temp_store = MEMORY;",
		"PRAGMA auto_vacuum = NONE;",
		"PRAGMA locking_mode = EXCLUSIVE;",
		cache_size_pragma.as_str(),
	];
	for pragma in &pragmas {
		if let Err(err) = sqlite_exec(db, pragma) {
			tracing::error!(
				file_name,
				pragma,
				%err,
				last_error = ?vfs.clone_last_error(),
				"failed to configure sqlite database"
			);
			return Err(err);
		}
	}

	Ok(())
}

pub fn verify_batch_atomic_writes(
	db: *mut sqlite3,
	vfs: &SqliteVfs,
	file_name: &str,
) -> std::result::Result<(), String> {
	#[cfg(test)]
	let assert_batch_atomic = vfs.ctx.config.assert_batch_atomic;
	#[cfg(not(test))]
	let assert_batch_atomic = true;
	if assert_batch_atomic {
		if let Err(err) = assert_batch_atomic_probe(db, &vfs) {
			tracing::error!(
				file_name,
				%err,
				last_error = ?vfs.clone_last_error(),
				"failed to verify sqlite batch atomic writes"
			);
			return Err(err);
		}
	}

	Ok(())
}

#[cfg(test)]
#[path = "../tests/inline/vfs.rs"]
mod tests;
