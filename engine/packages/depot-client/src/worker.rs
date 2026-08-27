use std::{
	error::Error,
	fmt,
	sync::{
		Arc,
		atomic::{AtomicU8, Ordering},
	},
	thread::JoinHandle,
	time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use libsqlite3_sys::{SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE, sqlite3_get_autocommit};
use parking_lot::Mutex;
use tokio::sync::{Notify, oneshot};

use crate::{
	query::{
		BindParam, ExecuteResult, QueryResult, exec_statements, exec_statements_profiled,
		execute_single_statement, execute_single_statement_profiled,
	},
	vfs::{
		NativeConnection, NativeVfsHandle, SqliteOperationProfile, SqliteRoundTripCounts,
		SqliteVfsMetrics, configure_connection_for_database, open_connection,
		verify_batch_atomic_writes,
	},
};

// Keep the first worker version intentionally fixed-size. A full queue maps to
// actor.overloaded so callers get explicit backpressure instead of hidden work.
pub const SQLITE_WORKER_QUEUE_CAPACITY: usize = 128;
const SQLITE_WORKER_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
// Transactions slower than this are logged at warn so operators can spot SQLite
// work that is stalling on engine round trips. Everything below still logs at
// debug and records its round trip count to the histogram.
const SQLITE_SLOW_TRANSACTION_THRESHOLD: Duration = Duration::from_millis(2500);

const STATE_RUNNING: u8 = 0;
const STATE_CLOSING: u8 = 1;
const STATE_CLOSED: u8 = 2;
const STATE_DEAD: u8 = 3;

#[derive(Clone)]
pub struct SqliteWorkerHandle {
	inner: Arc<SqliteWorkerInner>,
	sql_tx: Sender<SqliteCommand>,
	close_tx: Sender<CloseRequest>,
}

struct SqliteWorkerInner {
	metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	state: AtomicU8,
	closed: Notify,
	join: Mutex<Option<JoinHandle<()>>>,
	ready: Mutex<Option<oneshot::Receiver<Result<()>>>>,
}

enum SqliteCommand {
	Execute {
		sql: String,
		params: Option<Vec<BindParam>>,
		enqueued_at: Option<Instant>,
		reply: oneshot::Sender<Result<SqliteWorkerResult<ExecuteResult>>>,
	},
	Exec {
		sql: String,
		enqueued_at: Option<Instant>,
		reply: oneshot::Sender<Result<SqliteWorkerResult<QueryResult>>>,
	},
	#[cfg(test)]
	Pause {
		entered: oneshot::Sender<()>,
		resume: oneshot::Receiver<()>,
	},
	#[cfg(test)]
	Panic,
}

#[derive(Debug)]
pub struct SqliteWorkerResult<T> {
	pub result: Result<T>,
	pub profile: SqliteOperationProfile,
}

struct CloseRequest;

struct WorkerInflightGuard<'a>(Option<&'a dyn SqliteVfsMetrics>);

impl<'a> WorkerInflightGuard<'a> {
	fn new(metrics: Option<&'a dyn SqliteVfsMetrics>) -> Self {
		if let Some(metrics) = metrics {
			metrics.set_worker_inflight(true);
		}
		Self(metrics)
	}
}

impl Drop for WorkerInflightGuard<'_> {
	fn drop(&mut self) {
		if let Some(metrics) = self.0 {
			metrics.set_worker_inflight(false);
		}
	}
}

/// Tracks an in-progress SQLite transaction so its wall-clock duration and the
/// network round trips it issued can be reported once it commits or rolls back.
///
/// A transaction begins on the first command run while none is open and ends
/// when the connection returns to autocommit mode (an autocommit statement
/// finishes, or an explicit `COMMIT`/`ROLLBACK` runs).
struct TransactionTracker {
	started_at: Instant,
	start_round_trips: SqliteRoundTripCounts,
	statement_count: u64,
}

struct WorkerContext {
	sql_rx: Receiver<SqliteCommand>,
	close_rx: Receiver<CloseRequest>,
	inner: Arc<SqliteWorkerInner>,
	vfs: NativeVfsHandle,
	file_name: String,
	ready_tx: Option<oneshot::Sender<Result<()>>>,
}

impl SqliteWorkerHandle {
	pub fn start(
		vfs: NativeVfsHandle,
		file_name: String,
		metrics: Option<Arc<dyn SqliteVfsMetrics>>,
	) -> Result<Self> {
		let (sql_tx, sql_rx) = crossbeam_channel::bounded(SQLITE_WORKER_QUEUE_CAPACITY);
		let (close_tx, close_rx) = crossbeam_channel::bounded(1);
		let (ready_tx, ready_rx) = oneshot::channel();
		let inner = Arc::new(SqliteWorkerInner {
			metrics,
			state: AtomicU8::new(STATE_RUNNING),
			closed: Notify::new(),
			join: Mutex::new(None),
			ready: Mutex::new(Some(ready_rx)),
		});

		let thread_inner = Arc::clone(&inner);
		let join = std::thread::Builder::new()
			.name(format!("sqlite-worker-{file_name}"))
			.spawn(move || {
				let ctx = WorkerContext {
					sql_rx,
					close_rx,
					inner: Arc::clone(&thread_inner),
					vfs,
					file_name,
					ready_tx: Some(ready_tx),
				};
				if let Err(panic) =
					std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker_main(ctx)))
				{
					thread_inner.state.store(STATE_DEAD, Ordering::Release);
					if let Some(metrics) = &thread_inner.metrics {
						metrics.record_worker_crash();
					}
					thread_inner.closed.notify_waiters();
					tracing::error!(message = panic_message(&panic), "sqlite worker panicked");
				}
			})
			.context("spawn sqlite worker thread")?;
		*inner.join.lock() = Some(join);

		Ok(Self {
			inner,
			sql_tx,
			close_tx,
		})
	}

	pub async fn wait_ready(&self) -> Result<()> {
		let ready = self.inner.ready.lock().take();
		let Some(ready) = ready else {
			return Ok(());
		};
		ready
			.await
			.map_err(|_| sqlite_worker_dead_error())?
			.map_err(|err| anyhow!("failed to initialize sqlite worker: {err}"))
	}

	pub async fn exec(&self, sql: String) -> Result<QueryResult> {
		self.exec_inner(sql, None).await?.result
	}

	pub async fn exec_profiled(&self, sql: String) -> Result<SqliteWorkerResult<QueryResult>> {
		self.exec_inner(sql, Some(Instant::now())).await
	}

	async fn exec_inner(
		&self,
		sql: String,
		enqueued_at: Option<Instant>,
	) -> Result<SqliteWorkerResult<QueryResult>> {
		let (reply, result) = oneshot::channel();
		self.enqueue(SqliteCommand::Exec {
			sql,
			enqueued_at,
			reply,
		})?;
		result.await.map_err(|_| sqlite_worker_dead_error())?
	}

	pub async fn execute(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		self.execute_inner(sql, params, None).await?.result
	}

	pub async fn execute_profiled(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
	) -> Result<SqliteWorkerResult<ExecuteResult>> {
		self.execute_inner(sql, params, Some(Instant::now())).await
	}

	async fn execute_inner(
		&self,
		sql: String,
		params: Option<Vec<BindParam>>,
		enqueued_at: Option<Instant>,
	) -> Result<SqliteWorkerResult<ExecuteResult>> {
		let (reply, result) = oneshot::channel();
		self.enqueue(SqliteCommand::Execute {
			sql,
			params,
			enqueued_at,
			reply,
		})?;
		result.await.map_err(|_| sqlite_worker_dead_error())?
	}

	pub async fn close(&self) -> Result<()> {
		let start = Instant::now();
		if self.inner.mark_closing() {
			// Close is a control path, not SQL work, so it must bypass the bounded
			// SQL queue even when the actor has filled all command slots.
			let _ = self.close_tx.try_send(CloseRequest);
		}

		let wait_closed = async {
			loop {
				let closed = self.inner.closed.notified();
				match self.inner.state.load(Ordering::Acquire) {
					STATE_CLOSED => return Ok(()),
					STATE_DEAD => return Err(sqlite_worker_dead_error()),
					STATE_RUNNING | STATE_CLOSING => closed.await,
					other => return Err(anyhow!("unknown sqlite worker state {other}")),
				}
			}
		};

		match tokio::time::timeout(SQLITE_WORKER_CLOSE_TIMEOUT, wait_closed).await {
			Ok(result) => result?,
			Err(_) => {
				if let Some(metrics) = &self.inner.metrics {
					metrics.record_worker_close_timeout();
				}
				// The worker thread still owns the SQLite connection and VFS handle.
				// Reporting timeout here must not drop or unregister the VFS while
				// SQLite may still be inside a synchronous VFS callback.
				self.join_worker_in_background(start);
				return Err(SqliteWorkerCloseTimeoutError.into());
			}
		}
		if let Some(metrics) = &self.inner.metrics {
			metrics.observe_worker_close_duration(start.elapsed().as_nanos() as u64);
		}

		self.join_worker().await
	}

	pub async fn wait_for_failure(&self) -> bool {
		loop {
			let closed = self.inner.closed.notified();
			match self.inner.state.load(Ordering::Acquire) {
				STATE_DEAD => return true,
				STATE_CLOSED => return false,
				STATE_RUNNING | STATE_CLOSING => closed.await,
				_ => return true,
			}
		}
	}

	fn enqueue(&self, command: SqliteCommand) -> Result<()> {
		match self.inner.state.load(Ordering::Acquire) {
			STATE_RUNNING => {}
			STATE_CLOSING | STATE_CLOSED => return Err(sqlite_closing_error()),
			STATE_DEAD => return Err(sqlite_worker_dead_error()),
			other => return Err(anyhow!("unknown sqlite worker state {other}")),
		}

		match self.sql_tx.try_send(command) {
			Ok(()) => {
				self.inner.record_queue_depth(self.sql_tx.len() as u64);
				Ok(())
			}
			Err(TrySendError::Full(_)) => {
				if let Some(metrics) = &self.inner.metrics {
					metrics.record_worker_queue_overload();
				}
				// SQL backpressure is actor backpressure. Waiting for capacity here
				// would let a single actor build hidden native SQLite backlog.
				Err(SqliteWorkerOverloadedError.into())
			}
			Err(TrySendError::Disconnected(_)) => Err(sqlite_worker_dead_error()),
		}
	}

	#[cfg(test)]
	pub(crate) async fn pause_for_test(&self) -> oneshot::Sender<()> {
		let (entered_tx, entered_rx) = oneshot::channel();
		let (resume_tx, resume_rx) = oneshot::channel();
		self.enqueue(SqliteCommand::Pause {
			entered: entered_tx,
			resume: resume_rx,
		})
		.expect("test pause should enqueue");
		entered_rx.await.expect("test pause should start");
		resume_tx
	}

	#[cfg(test)]
	pub(crate) fn is_closing_for_test(&self) -> bool {
		matches!(
			self.inner.state.load(Ordering::Acquire),
			STATE_CLOSING | STATE_CLOSED | STATE_DEAD
		)
	}

	#[cfg(test)]
	pub(crate) async fn panic_for_test(&self) {
		self.enqueue(SqliteCommand::Panic)
			.expect("test panic should enqueue");
		assert!(self.wait_for_failure().await);
	}

	async fn join_worker(&self) -> Result<()> {
		let join = self.inner.join.lock().take();
		let Some(join) = join else {
			return Ok(());
		};
		tokio::task::spawn_blocking(move || {
			join.join()
				.map_err(|panic| anyhow!("sqlite worker panicked: {}", panic_message(&panic)))
		})
		.await
		.context("join sqlite worker join task")?
	}

	fn join_worker_in_background(&self, start: Instant) {
		let join = self.inner.join.lock().take();
		let Some(join) = join else {
			return;
		};

		let metrics = self.inner.metrics.clone();
		let _ = tokio::task::spawn_blocking(move || {
			let result = join.join();
			let duration_ns = start.elapsed().as_nanos() as u64;

			if let Some(metrics) = &metrics {
				metrics.observe_worker_close_duration(duration_ns);
			}

			match result {
				Ok(()) => {
					tracing::warn!(duration_ns, "sqlite worker finished after close timeout");
				}
				Err(panic) => {
					tracing::error!(
						duration_ns,
						message = panic_message(&panic),
						"sqlite worker finished after close timeout with panic",
					);
				}
			}
		});
	}
}

impl SqliteWorkerInner {
	fn mark_closing(&self) -> bool {
		self.state
			.compare_exchange(
				STATE_RUNNING,
				STATE_CLOSING,
				Ordering::AcqRel,
				Ordering::Acquire,
			)
			.is_ok()
	}

	fn record_queue_depth(&self, depth: u64) {
		if let Some(metrics) = &self.metrics {
			metrics.set_worker_queue_depth(depth);
		}
	}
}

impl Drop for SqliteWorkerInner {
	fn drop(&mut self) {
		if self.state.load(Ordering::Acquire) == STATE_RUNNING {
			if let Some(metrics) = &self.metrics {
				metrics.record_worker_unclean_close();
			}
			tracing::error!("sqlite worker handle dropped without clean close");
		}
	}
}

fn worker_main(mut ctx: WorkerContext) {
	let connection = open_worker_connection(&ctx);
	let mut db = match connection {
		Ok(db) => {
			if let Some(ready_tx) = ctx.ready_tx.take() {
				let _ = ready_tx.send(Ok(()));
			}
			db
		}
		Err(err) => {
			if let Some(ready_tx) = ctx.ready_tx.take() {
				let _ = ready_tx.send(Err(err));
			}
			fail_queued_sql(&ctx.sql_rx);
			ctx.inner.state.store(STATE_DEAD, Ordering::Release);
			ctx.inner.closed.notify_waiters();
			return;
		}
	};

	let mut transaction: Option<TransactionTracker> = None;
	loop {
		if ctx.close_rx.try_recv().is_ok()
			|| ctx.inner.state.load(Ordering::Acquire) == STATE_CLOSING
		{
			// The worker checks close before dispatching queued SQL so shutdown
			// cannot be delayed by commands already sitting behind the active call.
			fail_queued_sql(&ctx.sql_rx);
			break;
		}

		crossbeam_channel::select_biased! {
			recv(ctx.close_rx) -> close => {
				fail_queued_sql(&ctx.sql_rx);
				if close.is_err() {
					if let Some(metrics) = &ctx.inner.metrics {
						metrics.record_worker_unclean_close();
					}
					tracing::error!("sqlite worker close channel dropped without clean close");
				}
				break;
			}
			recv(ctx.sql_rx) -> command => {
				let Ok(command) = command else {
					if let Some(metrics) = &ctx.inner.metrics {
						metrics.record_worker_unclean_close();
					}
					// A dropped SQL sender without a close request means the owning
					// runtime lost the handle without running SQLite shutdown.
					tracing::error!("sqlite worker command channel dropped without clean close");
					break;
				};
				ctx.inner.record_queue_depth(ctx.sql_rx.len() as u64);
				if ctx.inner.state.load(Ordering::Acquire) == STATE_CLOSING {
					fail_command(command);
					fail_queued_sql(&ctx.sql_rx);
					break;
				}
				let _inflight = WorkerInflightGuard::new(ctx.inner.metrics.as_deref());
				run_command(
					&mut db,
					command,
					ctx.inner.metrics.as_deref(),
					&ctx.file_name,
					&mut transaction,
				);
			}
		}
	}

	drop(db);
	ctx.inner.state.store(STATE_CLOSED, Ordering::Release);
	ctx.inner.closed.notify_waiters();
}

fn open_worker_connection(ctx: &WorkerContext) -> Result<NativeConnection> {
	let connection = open_connection(
		ctx.vfs.clone(),
		&ctx.file_name,
		SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
	)
	.map_err(anyhow::Error::msg)?;
	configure_connection_for_database(connection.as_ptr(), &ctx.vfs, &ctx.file_name)
		.map_err(anyhow::Error::msg)?;
	verify_batch_atomic_writes(connection.as_ptr(), &ctx.vfs, &ctx.file_name)
		.map_err(anyhow::Error::msg)?;
	Ok(connection)
}

fn run_command(
	db: &mut NativeConnection,
	command: SqliteCommand,
	metrics: Option<&dyn SqliteVfsMetrics>,
	file_name: &str,
	transaction: &mut Option<TransactionTracker>,
) {
	let start = Instant::now();
	match command {
		SqliteCommand::Execute {
			sql,
			params,
			enqueued_at,
			reply,
		} => {
			if reply.is_closed() {
				return;
			}
			begin_transaction_if_needed(db, transaction);
			// Read the transaction state before running so the label reflects the
			// transaction the statement executed against, not the state it leaves
			// behind (a BEGIN flips autocommit off, a COMMIT flips it back on).
			let in_tx = command_in_tx(db);
			let stmt_kind = classify_statement(&sql);
			let worker_result = if let Some(enqueued_at) = enqueued_at {
				let worker_wait_ns = enqueued_at.elapsed().as_nanos() as u64;
				let operation_profile = db.begin_operation_profile();
				let execution_start = Instant::now();
				let result =
					execute_single_statement_profiled(db.as_ptr(), &sql, params.as_deref());
				let execution_ns = execution_start.elapsed().as_nanos() as u64;
				let mut profile = operation_profile.finish();
				profile.worker_wait_ns = worker_wait_ns;
				profile.sqlite_execution_ns = execution_ns.saturating_sub(profile.storage_ns);
				if let Ok((_, query_profile)) = &result {
					profile.bind_count = query_profile.bind_count;
					profile.bind_logical_bytes = query_profile.bind_logical_bytes;
					profile.result_rows = query_profile.result_rows;
					profile.result_columns = query_profile.result_columns;
					profile.result_logical_bytes = query_profile.result_logical_bytes;
				}
				record_command_metrics(
					metrics,
					"execute",
					in_tx,
					stmt_kind,
					&result,
					start.elapsed(),
				);
				SqliteWorkerResult {
					result: result.map(|(value, _)| value),
					profile,
				}
			} else {
				let result = execute_single_statement(db.as_ptr(), &sql, params.as_deref());
				record_command_metrics(
					metrics,
					"execute",
					in_tx,
					stmt_kind,
					&result,
					start.elapsed(),
				);
				SqliteWorkerResult {
					result,
					profile: SqliteOperationProfile::default(),
				}
			};
			finalize_transaction_if_complete(db, metrics, file_name, transaction);
			let _ = reply.send(Ok(worker_result));
		}
		SqliteCommand::Exec {
			sql,
			enqueued_at,
			reply,
		} => {
			if reply.is_closed() {
				return;
			}
			begin_transaction_if_needed(db, transaction);
			let in_tx = command_in_tx(db);
			let stmt_kind = classify_statement(&sql);
			let worker_result = if let Some(enqueued_at) = enqueued_at {
				let worker_wait_ns = enqueued_at.elapsed().as_nanos() as u64;
				let operation_profile = db.begin_operation_profile();
				let execution_start = Instant::now();
				let result = exec_statements_profiled(db.as_ptr(), &sql);
				let execution_ns = execution_start.elapsed().as_nanos() as u64;
				let mut profile = operation_profile.finish();
				profile.worker_wait_ns = worker_wait_ns;
				profile.sqlite_execution_ns = execution_ns.saturating_sub(profile.storage_ns);
				if let Ok((_, query_profile)) = &result {
					profile.bind_count = query_profile.bind_count;
					profile.bind_logical_bytes = query_profile.bind_logical_bytes;
					profile.result_rows = query_profile.result_rows;
					profile.result_columns = query_profile.result_columns;
					profile.result_logical_bytes = query_profile.result_logical_bytes;
				}
				record_command_metrics(metrics, "exec", in_tx, stmt_kind, &result, start.elapsed());
				SqliteWorkerResult {
					result: result.map(|(value, _)| value),
					profile,
				}
			} else {
				let result = exec_statements(db.as_ptr(), &sql);
				record_command_metrics(metrics, "exec", in_tx, stmt_kind, &result, start.elapsed());
				SqliteWorkerResult {
					result,
					profile: SqliteOperationProfile::default(),
				}
			};
			finalize_transaction_if_complete(db, metrics, file_name, transaction);
			let _ = reply.send(Ok(worker_result));
		}
		#[cfg(test)]
		SqliteCommand::Pause { entered, resume } => {
			let _ = entered.send(());
			let _ = resume.blocking_recv();
		}
		#[cfg(test)]
		SqliteCommand::Panic => {
			panic!("test sqlite worker panic");
		}
	}
}

/// Opens a transaction tracker before running a command when none is active.
///
/// The first command after autocommit resumes starts a new transaction, whether
/// it is an autocommit statement or an explicit `BEGIN`. The round trip baseline
/// is captured here so the delta covers the whole transaction.
fn begin_transaction_if_needed(
	db: &mut NativeConnection,
	transaction: &mut Option<TransactionTracker>,
) {
	match transaction {
		Some(tracker) => tracker.statement_count = tracker.statement_count.saturating_add(1),
		None => {
			*transaction = Some(TransactionTracker {
				started_at: Instant::now(),
				start_round_trips: db.round_trip_counts(),
				statement_count: 1,
			});
		}
	}
}

/// Reports a transaction once the connection returns to autocommit mode.
///
/// Always records the round trip count to the histogram and logs at debug.
/// Transactions slower than `SQLITE_SLOW_TRANSACTION_THRESHOLD` also log at warn.
fn finalize_transaction_if_complete(
	db: &mut NativeConnection,
	metrics: Option<&dyn SqliteVfsMetrics>,
	file_name: &str,
	transaction: &mut Option<TransactionTracker>,
) {
	// Autocommit off means an explicit transaction is still open across the next
	// command, so the transaction has not finished yet.
	if command_in_tx(db) {
		return;
	}
	let Some(tracker) = transaction.take() else {
		return;
	};

	let duration = tracker.started_at.elapsed();
	let round_trips = db.round_trip_counts().since(tracker.start_round_trips);

	if let Some(metrics) = metrics {
		metrics.observe_transaction_round_trips(round_trips.get_pages, round_trips.commit);
	}

	if duration >= SQLITE_SLOW_TRANSACTION_THRESHOLD {
		tracing::warn!(
			database = %file_name,
			duration_ms = duration.as_millis() as u64,
			statement_count = tracker.statement_count,
			get_pages_round_trips = round_trips.get_pages,
			commit_round_trips = round_trips.commit,
			total_round_trips = round_trips.total(),
			"slow sqlite transaction"
		);
	} else {
		tracing::debug!(
			database = %file_name,
			duration_ms = duration.as_millis() as u64,
			statement_count = tracker.statement_count,
			get_pages_round_trips = round_trips.get_pages,
			commit_round_trips = round_trips.commit,
			total_round_trips = round_trips.total(),
			"sqlite transaction complete"
		);
	}
}

fn record_command_metrics<T>(
	metrics: Option<&dyn SqliteVfsMetrics>,
	operation: &'static str,
	in_tx: bool,
	stmt_kind: &'static str,
	result: &Result<T>,
	duration: Duration,
) {
	let Some(metrics) = metrics else {
		return;
	};
	metrics.observe_worker_command_duration(
		operation,
		in_tx,
		stmt_kind,
		duration.as_nanos() as u64,
	);
	if let Err(error) = result {
		metrics.record_worker_command_error(operation, worker_error_code(error));
	}
}

/// Returns whether an explicit transaction is open on the connection.
///
/// `sqlite3_get_autocommit` returns non-zero in autocommit mode (no open
/// transaction) and zero once a `BEGIN` opens one. Implicit per-statement
/// transactions keep autocommit on, so this only reports explicit transactions.
fn command_in_tx(db: &mut NativeConnection) -> bool {
	unsafe { sqlite3_get_autocommit(db.as_ptr()) == 0 }
}

/// Classifies a SQL command by its leading keyword for metric labeling.
///
/// Leading line and block comments are skipped so generated SQL with a header
/// comment still classifies by its first statement. Multi-statement `exec`
/// payloads are labeled by their first statement.
fn classify_statement(sql: &str) -> &'static str {
	let trimmed = strip_leading_sql_noise(sql);
	let keyword: String = trimmed
		.chars()
		.take_while(|c| c.is_ascii_alphabetic())
		.collect();
	match keyword.to_ascii_uppercase().as_str() {
		"SELECT" | "WITH" => "select",
		"INSERT" | "REPLACE" => "insert",
		"UPDATE" => "update",
		"DELETE" => "delete",
		"BEGIN" => "begin",
		"COMMIT" | "END" => "commit",
		"ROLLBACK" => "rollback",
		"SAVEPOINT" => "savepoint",
		"RELEASE" => "release",
		"PRAGMA" => "pragma",
		"CREATE" => "create",
		"DROP" => "drop",
		"ALTER" => "alter",
		"VACUUM" => "vacuum",
		"ANALYZE" => "analyze",
		"ATTACH" => "attach",
		"DETACH" => "detach",
		_ => "other",
	}
}

fn strip_leading_sql_noise(sql: &str) -> &str {
	let mut rest = sql.trim_start();
	loop {
		if let Some(after) = rest.strip_prefix("--") {
			rest = match after.find('\n') {
				Some(idx) => after[idx + 1..].trim_start(),
				None => "",
			};
		} else if let Some(after) = rest.strip_prefix("/*") {
			rest = match after.find("*/") {
				Some(idx) => after[idx + 2..].trim_start(),
				None => "",
			};
		} else {
			return rest;
		}
	}
}

fn fail_queued_sql(sql_rx: &Receiver<SqliteCommand>) {
	for command in sql_rx.try_iter() {
		fail_command(command);
	}
}

fn fail_command(command: SqliteCommand) {
	// Cancelled requests may sit in the bounded queue until the worker observes
	// them. Once observed during shutdown, they fail instead of running.
	match command {
		SqliteCommand::Execute { reply, .. } => {
			let _ = reply.send(Err(sqlite_closing_error()));
		}
		SqliteCommand::Exec { reply, .. } => {
			let _ = reply.send(Err(sqlite_closing_error()));
		}
		#[cfg(test)]
		SqliteCommand::Pause { resume, .. } => {
			drop(resume);
		}
		#[cfg(test)]
		SqliteCommand::Panic => {}
	}
}

fn sqlite_closing_error() -> anyhow::Error {
	SqliteWorkerClosingError.into()
}

fn sqlite_worker_dead_error() -> anyhow::Error {
	SqliteWorkerDeadError.into()
}

fn worker_error_code(error: &anyhow::Error) -> &'static str {
	if error
		.downcast_ref::<SqliteWorkerOverloadedError>()
		.is_some()
	{
		"overloaded"
	} else if error.downcast_ref::<SqliteWorkerFatalError>().is_some() {
		"fatal"
	} else if error.downcast_ref::<SqliteWorkerClosingError>().is_some() {
		"closing"
	} else if error.downcast_ref::<SqliteWorkerDeadError>().is_some() {
		"dead"
	} else if error
		.downcast_ref::<SqliteWorkerCloseTimeoutError>()
		.is_some()
	{
		"close_timeout"
	} else {
		"sqlite"
	}
}

#[derive(Debug)]
pub struct SqliteWorkerOverloadedError;

impl fmt::Display for SqliteWorkerOverloadedError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("actor.overloaded: sqlite worker command queue is full")
	}
}

impl Error for SqliteWorkerOverloadedError {}

#[derive(Debug)]
pub struct SqliteWorkerClosingError;

impl fmt::Display for SqliteWorkerClosingError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("sqlite worker is closing")
	}
}

impl Error for SqliteWorkerClosingError {}

#[derive(Debug)]
pub struct SqliteWorkerDeadError;

impl fmt::Display for SqliteWorkerDeadError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("sqlite worker is closed")
	}
}

impl Error for SqliteWorkerDeadError {}

#[derive(Debug)]
pub struct SqliteWorkerCloseTimeoutError;

impl fmt::Display for SqliteWorkerCloseTimeoutError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("sqlite worker close timed out")
	}
}

impl Error for SqliteWorkerCloseTimeoutError {}

#[derive(Debug, Clone)]
pub struct SqliteWorkerFatalError {
	message: String,
}

impl SqliteWorkerFatalError {
	pub fn new(message: String) -> Self {
		Self { message }
	}

	pub fn message(&self) -> &str {
		&self.message
	}
}

impl fmt::Display for SqliteWorkerFatalError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "sqlite worker hit fatal storage error: {}", self.message)
	}
}

impl Error for SqliteWorkerFatalError {}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
	if let Some(message) = payload.downcast_ref::<&str>() {
		message.to_string()
	} else if let Some(message) = payload.downcast_ref::<String>() {
		message.clone()
	} else {
		"unknown panic".to_string()
	}
}
