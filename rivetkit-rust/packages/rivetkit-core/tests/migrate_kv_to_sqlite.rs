use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::Result;
use parking_lot::Mutex;
use rivet_envoy_client::config::{
	BoxFuture as EnvoyBoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse,
	WebSocketHandler, WebSocketSender,
};
use rivet_envoy_client::context::{SharedContext, WsTxMessage};
use rivet_envoy_client::envoy::ToEnvoyMessage;
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_client::protocol;
use rivet_envoy_client::sqlite::{
	RemoteSqliteRequest, RemoteSqliteResponse, RemoteSqliteResponseEnvelope,
};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, params_from_iter};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::actor::connection::{PersistedConnection, encode_persisted_connection};
use crate::actor::context::ActorContext;
use crate::actor::internal_storage::schema as internal_schema;
use crate::actor::internal_storage::{self, InternalActorSnapshot};
use crate::actor::keys::{
	INSPECTOR_TOKEN_KEY, KV_PREFIX, LAST_PUSHED_ALARM_KEY, PERSIST_DATA_KEY, QUEUE_METADATA_KEY,
	make_connection_key, make_prefixed_key, make_queue_message_key, make_traces_key,
	make_workflow_key,
};
use crate::actor::kv::Kv;
use crate::actor::messages::{StateDelta, WorkflowKvWrite};
use crate::actor::queue::{
	PersistedQueueMessage, QueueMetadata, encode_queue_message, encode_queue_metadata,
};
use crate::actor::state::{
	PersistedActor, PersistedScheduleEvent, encode_last_pushed_alarm, encode_persisted_actor,
};
use crate::sqlite::{ColumnValue, SqliteDb};
use crate::types::{ActorKeySegment, ListOpts};

use super::{LEGACY_SCAN_PAGE_LIMIT, LegacyPrefixScan, import_core_state_if_needed};

struct IdleEnvoyCallbacks;

impl EnvoyCallbacks for IdleEnvoyCallbacks {
	fn on_actor_start(
		&self,
		_handle: EnvoyHandle,
		_actor_id: String,
		_generation: u32,
		_config: protocol::ActorConfig,
		_preloaded_kv: Option<protocol::PreloadedKv>,
	) -> EnvoyBoxFuture<anyhow::Result<()>> {
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
	) -> EnvoyBoxFuture<anyhow::Result<HttpResponse>> {
		Box::pin(async { unreachable!("migration tests do not fetch") })
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
	) -> EnvoyBoxFuture<anyhow::Result<WebSocketHandler>> {
		Box::pin(async { unreachable!("migration tests do not open websockets") })
	}

	fn can_hibernate(
		&self,
		_actor_id: &str,
		_gateway_id: &protocol::GatewayId,
		_request_id: &protocol::RequestId,
		_request: &HttpRequest,
	) -> EnvoyBoxFuture<anyhow::Result<bool>> {
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
		actors: Default::default(),
		actors_notify: Arc::new(tokio::sync::Notify::new()),
		live_tunnel_requests: Default::default(),
		pending_hibernation_restores: Default::default(),
		ws_tx: Arc::new(AsyncMutex::new(None::<mpsc::UnboundedSender<WsTxMessage>>)),
		connection_session: std::sync::atomic::AtomicU64::new(1),
		next_connection_session: std::sync::atomic::AtomicU64::new(1),
		connection_session_tx: tokio::sync::watch::channel(1).0,
		protocol_metadata: Arc::new(AsyncMutex::new(None)),
		shutting_down: AtomicBool::new(false),
		last_ping_ts: std::sync::atomic::AtomicI64::new(i64::MAX),
		stopped_tx: tokio::sync::watch::channel(true).0,
	});

	(EnvoyHandle::from_shared(shared), envoy_rx)
}

fn sqlite_ctx(kv: Kv) -> (ActorContext, tokio::task::JoinHandle<()>) {
	let (ctx, task, _) = sqlite_ctx_with_harness(kv);
	(ctx, task)
}

#[derive(Clone)]
struct SqliteFault {
	sql_contains: &'static str,
	blob_param: Option<&'static [u8]>,
}

#[derive(Default)]
struct SqliteHarness {
	fault: Mutex<Option<SqliteFault>>,
	indeterminate_commit_after_sql: Mutex<Option<&'static str>>,
	fail_commit_after_apply: AtomicBool,
	max_transaction_statements: AtomicUsize,
	queue_next_id_writes: AtomicUsize,
	request_count: AtomicUsize,
	transaction_count: AtomicUsize,
}

fn sqlite_ctx_with_harness(
	kv: Kv,
) -> (
	ActorContext,
	tokio::task::JoinHandle<()>,
	Arc<SqliteHarness>,
) {
	sqlite_ctx_with_harness_enabled(kv, true)
}

fn sqlite_ctx_with_harness_enabled(
	kv: Kv,
	enabled: bool,
) -> (
	ActorContext,
	tokio::task::JoinHandle<()>,
	Arc<SqliteHarness>,
) {
	let (handle, envoy_rx) = test_envoy_handle();
	let db = SqliteDb::new_with_remote_sqlite(
		handle,
		"actor-import",
		Some("user/1".to_owned()),
		Some(1),
		enabled,
		true,
	)
	.expect("test remote sqlite should be configured");
	let ctx = ActorContext::build(
		"actor-import".to_owned(),
		"actor".to_owned(),
		vec![ActorKeySegment::String("user/1".to_owned())],
		"local".to_owned(),
		Some(1),
		"test-envoy".to_owned(),
		Default::default(),
		kv,
		db,
	);
	let harness = Arc::new(SqliteHarness::default());
	(
		ctx,
		tokio::spawn(run_remote_sqlite(envoy_rx, harness.clone())),
		harness,
	)
}

async fn run_remote_sqlite(
	mut envoy_rx: mpsc::UnboundedReceiver<ToEnvoyMessage>,
	harness: Arc<SqliteHarness>,
) {
	let conn = Connection::open_in_memory().expect("sqlite harness should open");
	let mut transaction_statements = None::<usize>;
	while let Some(message) = envoy_rx.recv().await {
		let ToEnvoyMessage::RemoteSqliteRequest {
			request,
			expected_session: _,
			response_tx,
		} = message
		else {
			continue;
		};
		let request = match request {
			RemoteSqliteRequest::Execute(request) => request,
			RemoteSqliteRequest::ExecuteBatch(request) => {
				let response = execute_remote_batch(&conn, &harness, request);
				response_tx
					.send(Ok(RemoteSqliteResponseEnvelope {
						response: RemoteSqliteResponse::ExecuteBatch(response),
						session: 1,
					}))
					.expect("remote sqlite response receiver should still be alive");
				continue;
			}
			RemoteSqliteRequest::Exec(_) => {
				panic!("migration test only expects remote execute requests")
			}
		};
		harness.request_count.fetch_add(1, Ordering::Relaxed);
		let fail_commit_after_apply = request.sql == "COMMIT"
			&& harness
				.fail_commit_after_apply
				.swap(false, Ordering::SeqCst);
		if request.sql == "BEGIN" || request.sql == "BEGIN IMMEDIATE" {
			harness.transaction_count.fetch_add(1, Ordering::Relaxed);
			transaction_statements = Some(0);
		} else if request.sql == "COMMIT" || request.sql == "ROLLBACK" {
			if let Some(statement_count) = transaction_statements.take() {
				harness
					.max_transaction_statements
					.fetch_max(statement_count, Ordering::SeqCst);
			}
		} else if let Some(statement_count) = transaction_statements.as_mut() {
			*statement_count += 1;
		}
		if request.sql.contains("queue_next_id") {
			harness.queue_next_id_writes.fetch_add(1, Ordering::SeqCst);
		}
		let should_fail = harness.fault.lock().as_ref().is_some_and(|fault| {
			request.sql.contains(fault.sql_contains)
				&& fault.blob_param.is_none_or(|expected| {
					request.params.as_ref().is_some_and(|params| {
						params.iter().any(|param| {
							matches!(param, protocol::SqliteBindParam::SqliteValueBlob(value) if value.value == expected)
						})
					})
				})
		});
		if should_fail {
			harness.fault.lock().take();
			response_tx
				.send(Ok(RemoteSqliteResponseEnvelope {
					response: RemoteSqliteResponse::Execute(
						protocol::SqliteExecuteResponse::SqliteErrorResponse(
							protocol::SqliteErrorResponse {
								group: "test".to_owned(),
								code: "injected_failure".to_owned(),
								message: "injected migration fault".to_owned(),
							},
						),
					),
					session: 1,
				}))
				.expect("remote sqlite response receiver should still be alive");
			continue;
		}
		let response =
			execute_remote_sql(&conn, &request.sql, request.params).unwrap_or_else(|error| {
				protocol::SqliteExecuteResponse::SqliteErrorResponse(
					protocol::SqliteErrorResponse {
						group: "core".to_owned(),
						code: "internal_error".to_owned(),
						message: error.to_string(),
					},
				)
			});
		if matches!(
			response,
			protocol::SqliteExecuteResponse::SqliteExecuteOk(_)
		) {
			let should_arm_indeterminate_commit = transaction_statements.is_some()
				&& harness
					.indeterminate_commit_after_sql
					.lock()
					.is_some_and(|sql| request.sql.contains(sql));
			if should_arm_indeterminate_commit {
				harness.indeterminate_commit_after_sql.lock().take();
				harness
					.fail_commit_after_apply
					.store(true, Ordering::SeqCst);
			}
		}
		if fail_commit_after_apply
			&& matches!(
				response,
				protocol::SqliteExecuteResponse::SqliteExecuteOk(_)
			) {
			response_tx
				.send(Ok(RemoteSqliteResponseEnvelope {
					response: RemoteSqliteResponse::Execute(
						protocol::SqliteExecuteResponse::SqliteErrorResponse(
							protocol::SqliteErrorResponse {
								group: "test".to_owned(),
								code: "indeterminate_result".to_owned(),
								message: "injected lost commit response".to_owned(),
							},
						),
					),
					session: 1,
				}))
				.expect("remote sqlite response receiver should still be alive");
			continue;
		}
		response_tx
			.send(Ok(RemoteSqliteResponseEnvelope {
				response: RemoteSqliteResponse::Execute(response),
				session: 1,
			}))
			.expect("remote sqlite response receiver should still be alive");
	}
}

fn execute_remote_batch(
	conn: &Connection,
	harness: &SqliteHarness,
	request: protocol::SqliteExecuteBatchRequest,
) -> protocol::SqliteExecuteBatchResponse {
	harness.request_count.fetch_add(1, Ordering::Relaxed);
	harness.transaction_count.fetch_add(1, Ordering::Relaxed);
	harness
		.max_transaction_statements
		.fetch_max(request.statements.len(), Ordering::SeqCst);
	for statement in &request.statements {
		if statement.sql.contains("queue_next_id") {
			harness.queue_next_id_writes.fetch_add(1, Ordering::SeqCst);
		}
	}

	let fail_after_apply = harness
		.fail_commit_after_apply
		.swap(false, Ordering::SeqCst)
		|| harness
			.indeterminate_commit_after_sql
			.lock()
			.as_ref()
			.is_some_and(|sql| {
				request
					.statements
					.iter()
					.any(|statement| statement.sql.contains(sql))
			});
	if fail_after_apply {
		harness.indeterminate_commit_after_sql.lock().take();
	}

	if let Err(error) = conn.execute_batch("BEGIN") {
		return batch_sqlite_error("core", "internal_error", error.to_string());
	}
	let mut results = Vec::with_capacity(request.statements.len());
	for statement in request.statements {
		let should_fail = harness.fault.lock().as_ref().is_some_and(|fault| {
			statement.sql.contains(fault.sql_contains)
				&& fault.blob_param.is_none_or(|expected| {
					statement.params.as_ref().is_some_and(|params| {
						params.iter().any(|param| {
							matches!(param, protocol::SqliteBindParam::SqliteValueBlob(value) if value.value == expected)
						})
					})
				})
		});
		if should_fail {
			harness.fault.lock().take();
			let _ = conn.execute_batch("ROLLBACK");
			return batch_sqlite_error("test", "injected_failure", "injected migration fault");
		}

		match execute_remote_sql(conn, &statement.sql, statement.params) {
			Ok(protocol::SqliteExecuteResponse::SqliteExecuteOk(ok)) => results.push(ok.result),
			Ok(protocol::SqliteExecuteResponse::SqliteErrorResponse(error)) => {
				let _ = conn.execute_batch("ROLLBACK");
				return protocol::SqliteExecuteBatchResponse::SqliteErrorResponse(error);
			}
			Err(error) => {
				let _ = conn.execute_batch("ROLLBACK");
				return batch_sqlite_error("core", "internal_error", error.to_string());
			}
		}
	}
	if let Err(error) = conn.execute_batch("COMMIT") {
		let _ = conn.execute_batch("ROLLBACK");
		return batch_sqlite_error("core", "internal_error", error.to_string());
	}
	if fail_after_apply {
		return batch_sqlite_error(
			"test",
			"indeterminate_result",
			"injected lost commit response",
		);
	}
	protocol::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(protocol::SqliteExecuteBatchOk {
		results,
	})
}

fn batch_sqlite_error(
	group: &str,
	code: &str,
	message: impl Into<String>,
) -> protocol::SqliteExecuteBatchResponse {
	protocol::SqliteExecuteBatchResponse::SqliteErrorResponse(protocol::SqliteErrorResponse {
		group: group.to_owned(),
		code: code.to_owned(),
		message: message.into(),
	})
}

fn execute_remote_sql(
	conn: &Connection,
	sql: &str,
	params: Option<Vec<protocol::SqliteBindParam>>,
) -> rusqlite::Result<protocol::SqliteExecuteResponse> {
	let params = params
		.unwrap_or_default()
		.into_iter()
		.map(sqlite_value_from_protocol)
		.collect::<Vec<_>>();
	let mut statement = conn.prepare(sql)?;
	let column_count = statement.column_count();
	let columns = (0..column_count)
		.map(|index| statement.column_name(index).unwrap_or("").to_owned())
		.collect::<Vec<_>>();

	if column_count == 0 {
		let changes = statement.execute(params_from_iter(params.iter()))?;
		return Ok(sqlite_execute_response(protocol::SqliteExecuteResult {
			columns,
			rows: Vec::new(),
			changes: changes.try_into().unwrap_or(i64::MAX),
			last_insert_row_id: Some(conn.last_insert_rowid()),
		}));
	}

	let mut rows = statement.query(params_from_iter(params.iter()))?;
	let mut out_rows = Vec::new();
	while let Some(row) = rows.next()? {
		let mut out_row = Vec::with_capacity(column_count);
		for index in 0..column_count {
			out_row.push(sqlite_column_value_from_ref(row.get_ref(index)?));
		}
		out_rows.push(out_row);
	}

	Ok(sqlite_execute_response(protocol::SqliteExecuteResult {
		columns,
		rows: out_rows,
		changes: conn.changes().try_into().unwrap_or(i64::MAX),
		last_insert_row_id: Some(conn.last_insert_rowid()),
	}))
}

fn sqlite_value_from_protocol(value: protocol::SqliteBindParam) -> Value {
	match value {
		protocol::SqliteBindParam::SqliteValueNull => Value::Null,
		protocol::SqliteBindParam::SqliteValueInteger(value) => Value::Integer(value.value),
		protocol::SqliteBindParam::SqliteValueFloat(value) => {
			Value::Real(f64::from_bits(u64::from_be_bytes(value.value)))
		}
		protocol::SqliteBindParam::SqliteValueText(value) => Value::Text(value.value),
		protocol::SqliteBindParam::SqliteValueBlob(value) => Value::Blob(value.value),
	}
}

fn sqlite_column_value_from_ref(value: ValueRef<'_>) -> protocol::SqliteColumnValue {
	match value {
		ValueRef::Null => protocol::SqliteColumnValue::SqliteValueNull,
		ValueRef::Integer(value) => {
			protocol::SqliteColumnValue::SqliteValueInteger(protocol::SqliteValueInteger { value })
		}
		ValueRef::Real(value) => {
			protocol::SqliteColumnValue::SqliteValueFloat(protocol::SqliteValueFloat {
				value: value.to_bits().to_be_bytes(),
			})
		}
		ValueRef::Text(value) => {
			protocol::SqliteColumnValue::SqliteValueText(protocol::SqliteValueText {
				value: String::from_utf8_lossy(value).into_owned(),
			})
		}
		ValueRef::Blob(value) => {
			protocol::SqliteColumnValue::SqliteValueBlob(protocol::SqliteValueBlob {
				value: value.to_vec(),
			})
		}
	}
}

fn sqlite_execute_response(
	result: protocol::SqliteExecuteResult,
) -> protocol::SqliteExecuteResponse {
	protocol::SqliteExecuteResponse::SqliteExecuteOk(protocol::SqliteExecuteOk { result })
}

#[tokio::test]
async fn marks_empty_legacy_import_done_without_runtime_rows() -> Result<()> {
	let kv = Kv::new_in_memory();
	let trace_key = make_traces_key(b"trace-only");
	kv.put(&trace_key, b"trace-value").await?;

	let (ctx, sqlite_task) = sqlite_ctx(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	import_core_state_if_needed(&ctx).await?;

	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("done".to_owned())
	);
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql()).await?,
		None
	);
	assert_eq!(
		internal_storage::load_queue_metadata(ctx.sql()).await?,
		QueueMetadata {
			next_id: 1,
			size: 0,
		}
	);
	let runtime_rows = ctx
		.sql()
		.query("SELECT COUNT(*) FROM _rivet_runtime", None)
		.await?;
	assert_eq!(
		runtime_rows.rows,
		vec![vec![ColumnValue::Integer(0)]],
		"fresh actors should not create _rivet_runtime rows during empty legacy import",
	);
	let user_rows = ctx
		.sql()
		.query("SELECT key, value FROM _rivet_user_kv", None)
		.await?;
	assert!(
		user_rows.rows.is_empty(),
		"trace records must not import as user KV"
	);

	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn user_kv_batch_get_chunks_queries_and_preserves_input_order() -> Result<()> {
	let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(Kv::new_in_memory());
	internal_schema::ensure_internal_schema(ctx.sql()).await?;

	let entries = (0..=internal_storage::USER_KV_BATCH_GET_MAX_KEYS)
		.map(|index| {
			(
				format!("key-{index:04}").into_bytes(),
				format!("value-{index:04}").into_bytes(),
			)
		})
		.collect::<Vec<_>>();
	let entry_refs = entries
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	internal_storage::user_kv_batch_put(ctx.sql(), &entry_refs).await?;

	let mut keys = entries
		.iter()
		.rev()
		.map(|(key, _)| key.as_slice())
		.collect::<Vec<_>>();
	keys.push(b"missing");
	keys.push(entries[0].0.as_slice());
	let request_count_before = harness.request_count.load(Ordering::SeqCst);
	let values = internal_storage::user_kv_batch_get(ctx.sql(), &keys).await?;
	let request_count = harness.request_count.load(Ordering::SeqCst) - request_count_before;

	let expected = entries
		.iter()
		.rev()
		.map(|(_, value)| Some(value.clone()))
		.chain([None, Some(entries[0].1.clone())])
		.collect::<Vec<_>>();
	assert_eq!(values, expected);
	assert_eq!(
		request_count,
		keys.len()
			.div_ceil(internal_storage::USER_KV_BATCH_GET_MAX_KEYS),
		"batch get should issue one remote query per bounded key chunk",
	);

	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
#[allow(deprecated)]
async fn imports_legacy_kv_snapshot_to_sqlite_once() -> Result<()> {
	let kv = Kv::new_in_memory();
	let actor = PersistedActor {
		input: Some(b"input".to_vec()),
		has_initialized: true,
		state: b"state-v1".to_vec(),
		scheduled_events: vec![PersistedScheduleEvent {
			event_id: "event-1".to_owned(),
			timestamp: 1234,
			action: "tick".to_owned(),
			args: Some(b"args".to_vec()),
		}],
	};
	let connection = PersistedConnection {
		id: "conn-1".to_owned(),
		parameters: b"params".to_vec(),
		state: b"conn-state".to_vec(),
		subscriptions: Vec::new(),
		gateway_id: [1, 2, 3, 4],
		request_id: [5, 6, 7, 8],
		server_message_index: 9,
		client_message_index: 10,
		request_path: "/socket".to_owned(),
		request_headers: HashMap::from([("x-test".to_owned(), "yes".to_owned())]),
	};
	let queue_message = PersistedQueueMessage {
		name: "job".to_owned(),
		body: b"body".to_vec(),
		created_at: 9876,
		failure_count: None,
		available_at: None,
		in_flight: None,
		in_flight_at: None,
	};
	let workflow_key = make_workflow_key(b"wf-key");
	let user_key = make_prefixed_key(b"user-key");
	let trace_key = make_traces_key(b"trace-key");
	let legacy_entries = vec![
		(PERSIST_DATA_KEY.to_vec(), encode_persisted_actor(&actor)?),
		(
			LAST_PUSHED_ALARM_KEY.to_vec(),
			encode_last_pushed_alarm(Some(5678))?,
		),
		(INSPECTOR_TOKEN_KEY.to_vec(), b"inspector-token".to_vec()),
		(
			make_connection_key("conn-1"),
			encode_persisted_connection(&connection)?,
		),
		(
			QUEUE_METADATA_KEY.to_vec(),
			encode_queue_metadata(&QueueMetadata {
				next_id: 41,
				size: 1,
			})?,
		),
		(
			make_queue_message_key(40),
			encode_queue_message(&queue_message)?,
		),
		(workflow_key.clone(), b"workflow-value".to_vec()),
		(user_key.clone(), b"user-value".to_vec()),
		(trace_key.clone(), b"trace-value".to_vec()),
	];
	let legacy_refs = legacy_entries
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	kv.batch_put(&legacy_refs).await?;

	let (ctx, sqlite_task) = sqlite_ctx(kv.clone());
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	import_core_state_if_needed(&ctx).await?;

	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("done".to_owned())
	);
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql()).await?,
		Some(InternalActorSnapshot {
			actor: PersistedActor {
				scheduled_events: Vec::new(),
				..actor.clone()
			},
			last_pushed_alarm: Some(5678),
			run_wake_at: None,
		})
	);
	let schedule_rows = ctx
		.sql()
		.query(
			"SELECT event_id, trigger_at, action, args, kind FROM _rivet_schedule_events",
			None,
		)
		.await?;
	assert_eq!(
		schedule_rows.rows,
		vec![vec![
			ColumnValue::Text("event-1".to_owned()),
			ColumnValue::Integer(1234),
			ColumnValue::Text("tick".to_owned()),
			ColumnValue::Blob(b"args".to_vec()),
			ColumnValue::Integer(0),
		]],
	);
	assert_eq!(
		internal_storage::load_inspector_token(ctx.sql()).await?,
		Some("inspector-token".to_owned())
	);
	assert_eq!(
		internal_storage::load_connections(ctx.sql()).await?,
		vec![connection]
	);
	assert_eq!(
		internal_storage::load_queue_metadata(ctx.sql()).await?,
		QueueMetadata {
			next_id: 41,
			size: 1,
		}
	);
	let queue_rows = internal_storage::load_queue_messages(ctx.sql()).await?;
	assert_eq!(queue_rows.len(), 1);
	assert_eq!(queue_rows[0].id, 40);
	assert_eq!(queue_rows[0].message, queue_message);

	let workflow_rows = ctx
		.sql()
		.query("SELECT key, value FROM _rivet_wf_kv", None)
		.await?;
	assert_eq!(
		workflow_rows.rows,
		vec![vec![
			ColumnValue::Blob(workflow_key),
			ColumnValue::Blob(b"workflow-value".to_vec())
		]]
	);
	assert_eq!(
		internal_storage::user_kv_batch_get(ctx.sql(), &[user_key.as_slice()]).await?,
		vec![Some(b"user-value".to_vec())],
		"legacy user KV keys must import verbatim, keeping the [4] prefix the TS runtime queries with",
	);
	assert_eq!(
		internal_storage::user_kv_batch_get(ctx.sql(), &[b"user-key"]).await?,
		vec![None],
		"stripped keys must not exist; the runtime passes keys through unchanged",
	);
	assert_eq!(
		ctx.kv_batch_get(&[user_key.as_slice()]).await?,
		vec![Some(b"user-value".to_vec())],
		"the runtime kv path used by the TS bridge must read migrated entries",
	);
	let trace_rows = ctx
		.sql()
		.query(
			"SELECT key, value FROM _rivet_user_kv WHERE key = ?",
			Some(vec![crate::sqlite::BindParam::Blob(trace_key.clone())]),
		)
		.await?;
	assert!(trace_rows.rows.is_empty(), "legacy traces must not import");

	let legacy_after_import = kv
		.list_prefix(
			&[],
			ListOpts {
				reverse: false,
				limit: None,
			},
		)
		.await?;
	let mut sorted_legacy_entries = legacy_entries.clone();
	sorted_legacy_entries.sort_by(|left, right| left.0.cmp(&right.0));
	assert_eq!(legacy_after_import, sorted_legacy_entries);

	kv.put(
		PERSIST_DATA_KEY,
		&encode_persisted_actor(&PersistedActor {
			state: b"mutated-after-import".to_vec(),
			..actor
		})?,
	)
	.await?;
	import_core_state_if_needed(&ctx).await?;
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.expect("actor snapshot should remain imported")
			.actor
			.state,
		b"state-v1".to_vec(),
		"done import state must freeze the sqlite snapshot against later legacy kv changes",
	);

	drop(ctx);
	drop(kv);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn imports_subspaces_larger_than_the_backend_page_cap() -> Result<()> {
	let kv = Kv::new_in_memory();
	// Exercise the exact importer page boundary plus one. The in-memory range
	// backend includes the cursor, so this also verifies cursor de-duplication.
	kv.test_set_list_limit_cap(LEGACY_SCAN_PAGE_LIMIT);

	let entry_count: usize = LEGACY_SCAN_PAGE_LIMIT as usize + 1;
	let mut expected_user_entries = Vec::new();
	for index in 0..entry_count {
		let user_key = make_prefixed_key(format!("user-key-{index:04}").as_bytes());
		let value = format!("user-value-{index:04}").into_bytes();
		kv.put(&user_key, &value).await?;
		expected_user_entries.push((user_key, value));
	}
	for index in 0..entry_count {
		let workflow_key = make_workflow_key(format!("wf-key-{index:04}").as_bytes());
		kv.put(&workflow_key, format!("wf-value-{index:04}").as_bytes())
			.await?;
	}
	for index in 0..entry_count {
		kv.put(
			&make_queue_message_key(index as u64 + 1),
			&encode_queue_message(&PersistedQueueMessage {
				name: format!("job-{index:04}"),
				body: b"body".to_vec(),
				created_at: 1000 + index as i64,
				failure_count: None,
				available_at: None,
				in_flight: None,
				in_flight_at: None,
			})?,
		)
		.await?;
	}

	let (ctx, sqlite_task) = sqlite_ctx(kv.clone());
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	import_core_state_if_needed(&ctx).await?;

	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("done".to_owned())
	);
	let user_rows = ctx
		.sql()
		.query("SELECT COUNT(*) FROM _rivet_user_kv", None)
		.await?;
	assert_eq!(
		user_rows.rows,
		vec![vec![ColumnValue::Integer(entry_count as i64)]],
		"every user kv entry must import even when listings are capped below the total",
	);
	let expected_user_refs = expected_user_entries
		.iter()
		.map(|(key, _)| key.as_slice())
		.collect::<Vec<_>>();
	assert_eq!(
		internal_storage::user_kv_batch_get(ctx.sql(), &expected_user_refs).await?,
		expected_user_entries
			.iter()
			.map(|(_, value)| Some(value.clone()))
			.collect::<Vec<_>>(),
	);
	let workflow_rows = ctx
		.sql()
		.query("SELECT COUNT(*) FROM _rivet_wf_kv", None)
		.await?;
	assert_eq!(
		workflow_rows.rows,
		vec![vec![ColumnValue::Integer(entry_count as i64)]],
		"every workflow kv entry must import even when listings are capped below the total",
	);
	let queue_rows = internal_storage::load_queue_messages(ctx.sql()).await?;
	assert_eq!(
		queue_rows.len(),
		entry_count,
		"every queue message must import even when listings are capped below the total",
	);
	assert_eq!(
		internal_storage::load_queue_metadata(ctx.sql())
			.await?
			.next_id,
		entry_count as u64 + 1,
	);

	drop(ctx);
	drop(kv);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn legacy_scan_handles_exact_page_boundary_and_out_of_prefix_sentinel() -> Result<()> {
	for inclusive in [true, false] {
		for entry_count in [255usize, 256, 257] {
			let kv = Kv::new_in_memory();
			kv.test_set_list_limit_cap(7);
			kv.test_set_range_start_inclusive(inclusive);
			for index in 0..entry_count {
				let mut key = make_prefixed_key(format!("boundary-{index:04}").as_bytes());
				if index + 1 == entry_count {
					key.push(0xff);
				}
				kv.put(&key, b"value").await?;
			}
			// Sorts immediately after user KV and must terminate the continuation
			// scan without being returned.
			kv.put(&make_workflow_key(b"sentinel"), b"outside").await?;

			let (ctx, sqlite_task) = sqlite_ctx(kv);
			let mut scan = LegacyPrefixScan::new(&ctx, &KV_PREFIX);
			let mut scanned = Vec::new();
			while let Some(page) = scan.next_page().await? {
				scanned.extend(page);
			}
			assert_eq!(scanned.len(), entry_count);
			assert!(scanned.iter().all(|(key, _)| key.starts_with(&KV_PREFIX)));
			drop(ctx);
			sqlite_task.abort();
		}
	}
	Ok(())
}

#[tokio::test]
async fn legacy_scan_crosses_the_real_engine_listing_cap() -> Result<()> {
	let kv = Kv::new_in_memory();
	const ENGINE_LIST_CAP: usize = 16_384;
	for index in 0..=ENGINE_LIST_CAP {
		kv.put(
			&make_prefixed_key(format!("scale-{index:05}").as_bytes()),
			b"v",
		)
		.await?;
	}
	// Model the real backend cap even though the importer asks for smaller
	// pages; this guards against accidentally returning to an unpaginated scan.
	kv.test_set_list_limit_cap(ENGINE_LIST_CAP as u32);
	let (ctx, sqlite_task) = sqlite_ctx(kv);
	let mut scan = LegacyPrefixScan::new(&ctx, &KV_PREFIX);
	let mut count = 0usize;
	while let Some(page) = scan.next_page().await? {
		count += page.len();
	}
	assert_eq!(count, ENGINE_LIST_CAP + 1);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn interrupted_import_clears_partial_rows_and_reimports() -> Result<()> {
	let kv = Kv::new_in_memory();
	let actor = PersistedActor {
		input: None,
		has_initialized: true,
		state: b"legacy-state".to_vec(),
		scheduled_events: Vec::new(),
	};
	kv.put(PERSIST_DATA_KEY, &encode_persisted_actor(&actor)?)
		.await?;
	let user_key = make_prefixed_key(b"user-key");
	kv.put(&user_key, b"user-value").await?;

	let (ctx, sqlite_task) = sqlite_ctx(kv.clone());
	internal_schema::ensure_internal_schema(ctx.sql()).await?;

	// Simulate a crash mid-import: the importing marker is set and a stale
	// partial row exists that a fresh import would not produce.
	internal_storage::persist_meta_text(ctx.sql(), "kv_import_state", "importing").await?;
	internal_storage::user_kv_batch_put(
		ctx.sql(),
		&[(b"stale-partial-key".as_slice(), b"stale".as_slice())],
	)
	.await?;
	internal_storage::persist_actor_snapshot(
		ctx.sql(),
		&PersistedActor {
			input: None,
			has_initialized: false,
			state: b"stale-partial-state".to_vec(),
			scheduled_events: Vec::new(),
		},
	)
	.await?;

	import_core_state_if_needed(&ctx).await?;

	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("done".to_owned())
	);
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.expect("actor snapshot should import after interrupted import")
			.actor,
		actor,
		"interrupted imports must restart from scratch, replacing partial rows",
	);
	assert_eq!(
		internal_storage::user_kv_batch_get(
			ctx.sql(),
			&[user_key.as_slice(), b"stale-partial-key"],
		)
		.await?,
		vec![Some(b"user-value".to_vec()), None],
		"partial user kv rows must be cleared before the re-import",
	);
	let legacy_after_import = kv
		.list_prefix(
			&[],
			ListOpts {
				reverse: false,
				limit: None,
			},
		)
		.await?;
	assert_eq!(
		legacy_after_import.len(),
		2,
		"the frozen legacy kv must survive the interrupted import retry",
	);

	drop(ctx);
	drop(kv);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn non_empty_first_import_uses_legacy_kv_as_the_only_source_of_truth() -> Result<()> {
	let kv = Kv::new_in_memory();
	let legacy_actor = PersistedActor {
		input: None,
		has_initialized: true,
		state: b"legacy".to_vec(),
		scheduled_events: Vec::new(),
	};
	kv.put(PERSIST_DATA_KEY, &encode_persisted_actor(&legacy_actor)?)
		.await?;
	kv.put(&INSPECTOR_TOKEN_KEY, b"legacy-token").await?;
	let (ctx, sqlite_task) = sqlite_ctx(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	internal_storage::persist_actor_snapshot(
		ctx.sql(),
		&PersistedActor {
			state: b"stale-sqlite".to_vec(),
			..legacy_actor.clone()
		},
	)
	.await?;
	internal_storage::persist_inspector_token(ctx.sql(), "stale-token").await?;

	import_core_state_if_needed(&ctx).await?;
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.expect("legacy actor should import")
			.actor,
		legacy_actor
	);
	assert_eq!(
		internal_storage::load_inspector_token(ctx.sql()).await?,
		Some("legacy-token".to_owned())
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn rejects_legacy_queue_delivery_state_instead_of_dropping_it() -> Result<()> {
	let kv = Kv::new_in_memory();
	let key = make_queue_message_key(7);
	let value = encode_queue_message(&PersistedQueueMessage {
		name: "delayed".to_owned(),
		body: b"body".to_vec(),
		created_at: 100,
		failure_count: Some(1),
		available_at: Some(200),
		in_flight: Some(true),
		in_flight_at: Some(150),
	})?;
	kv.put(&key, &value).await?;
	let (ctx, sqlite_task) = sqlite_ctx(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	let error = import_core_state_if_needed(&ctx)
		.await
		.expect_err("unsupported delivery state must not be silently discarded");
	assert!(error.to_string().contains("cannot preserve"));
	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("importing".to_owned())
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn corrupt_legacy_records_fail_the_import_without_marking_done() -> Result<()> {
	let cases = [
		(
			PERSIST_DATA_KEY.to_vec(),
			b"bad actor".to_vec(),
			"persisted actor",
		),
		(
			LAST_PUSHED_ALARM_KEY.to_vec(),
			b"bad alarm".to_vec(),
			"last pushed alarm",
		),
		(INSPECTOR_TOKEN_KEY.to_vec(), vec![0xff], "utf-8"),
		(
			QUEUE_METADATA_KEY.to_vec(),
			b"bad metadata".to_vec(),
			"queue metadata",
		),
		(
			make_connection_key("bad"),
			b"bad connection".to_vec(),
			"connection",
		),
		(
			vec![5, 1, 2, 0],
			encode_queue_message(&PersistedQueueMessage {
				name: "job".to_owned(),
				body: Vec::new(),
				created_at: 1,
				failure_count: None,
				available_at: None,
				in_flight: None,
				in_flight_at: None,
			})?,
			"queue message key",
		),
		(
			make_queue_message_key(1),
			b"bad message".to_vec(),
			"queue message 1",
		),
	];

	for (key, value, expected) in cases {
		let kv = Kv::new_in_memory();
		kv.put(&key, &value).await?;
		let (ctx, sqlite_task) = sqlite_ctx(kv);
		internal_schema::ensure_internal_schema(ctx.sql()).await?;
		let error = import_core_state_if_needed(&ctx)
			.await
			.expect_err("corrupt legacy records must fail closed");
		assert!(
			error.to_string().contains(expected),
			"expected {expected:?} in {error:#}"
		);
		assert_ne!(
			internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
			Some("done".to_owned())
		);
		drop(ctx);
		sqlite_task.abort();
	}
	Ok(())
}

#[tokio::test]
async fn unknown_or_non_utf8_import_markers_fail_closed() -> Result<()> {
	for value in [b"mystery".as_slice(), b"\xff".as_slice()] {
		let kv = Kv::new_in_memory();
		let (ctx, sqlite_task) = sqlite_ctx(kv);
		internal_schema::ensure_internal_schema(ctx.sql()).await?;
		ctx.sql()
			.execute(
				"INSERT INTO _rivet_meta (key, value) VALUES ('kv_import_state', ?)",
				Some(vec![crate::sqlite::BindParam::Blob(value.to_vec())]),
			)
			.await?;
		assert!(import_core_state_if_needed(&ctx).await.is_err());
		drop(ctx);
		sqlite_task.abort();
	}
	Ok(())
}

async fn populate_fault_matrix_legacy(kv: &Kv) -> Result<()> {
	let actor = PersistedActor {
		input: Some(b"input".to_vec()),
		has_initialized: true,
		state: b"state".to_vec(),
		scheduled_events: Vec::new(),
	};
	let connection = PersistedConnection {
		id: "conn-fault".to_owned(),
		parameters: b"params".to_vec(),
		state: b"conn-state".to_vec(),
		subscriptions: Vec::new(),
		gateway_id: [1, 2, 3, 4],
		request_id: [5, 6, 7, 8],
		server_message_index: 9,
		client_message_index: 10,
		request_path: "/socket".to_owned(),
		request_headers: HashMap::new(),
	};
	let entries = vec![
		(PERSIST_DATA_KEY.to_vec(), encode_persisted_actor(&actor)?),
		(
			make_connection_key(&connection.id),
			encode_persisted_connection(&connection)?,
		),
		(
			make_queue_message_key(1),
			encode_queue_message(&PersistedQueueMessage {
				name: "job".to_owned(),
				body: b"body".to_vec(),
				created_at: 1,
				failure_count: None,
				available_at: None,
				in_flight: None,
				in_flight_at: None,
			})?,
		),
		(make_workflow_key(b"workflow"), b"workflow-value".to_vec()),
		(make_prefixed_key(b"user"), b"user-value".to_vec()),
	];
	let refs = entries
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	kv.batch_put(&refs).await
}

#[tokio::test]
async fn retries_after_faults_in_every_import_phase() -> Result<()> {
	let faults = [
		SqliteFault {
			sql_contains: "INSERT INTO _rivet_meta",
			blob_param: Some(b"importing"),
		},
		SqliteFault {
			sql_contains: "INSERT INTO _rivet_actor_state",
			blob_param: None,
		},
		SqliteFault {
			sql_contains: "INSERT INTO _rivet_conn_state",
			blob_param: None,
		},
		SqliteFault {
			sql_contains: "INSERT OR REPLACE INTO _rivet_queue",
			blob_param: None,
		},
		SqliteFault {
			sql_contains: "INSERT INTO _rivet_wf_kv",
			blob_param: None,
		},
		SqliteFault {
			sql_contains: "INSERT INTO _rivet_user_kv",
			blob_param: None,
		},
		SqliteFault {
			sql_contains: "INSERT INTO _rivet_meta",
			blob_param: Some(b"done"),
		},
	];

	for fault in faults {
		let kv = Kv::new_in_memory();
		populate_fault_matrix_legacy(&kv).await?;
		let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
		internal_schema::ensure_internal_schema(ctx.sql()).await?;
		*harness.fault.lock() = Some(fault.clone());
		assert!(
			import_core_state_if_needed(&ctx).await.is_err(),
			"fault for {} should interrupt the first attempt",
			fault.sql_contains,
		);
		import_core_state_if_needed(&ctx).await?;
		assert_eq!(
			internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
			Some("done".to_owned())
		);
		assert_eq!(
			internal_storage::load_connections(ctx.sql()).await?.len(),
			1
		);
		assert_eq!(
			internal_storage::load_queue_messages(ctx.sql())
				.await?
				.len(),
			1
		);
		assert_eq!(
			internal_storage::user_kv_batch_get(
				ctx.sql(),
				&[make_prefixed_key(b"user").as_slice()]
			)
			.await?,
			vec![Some(b"user-value".to_vec())]
		);
		drop(ctx);
		sqlite_task.abort();
	}
	Ok(())
}

#[tokio::test]
async fn retries_after_commit_applies_but_response_is_lost() -> Result<()> {
	let kv = Kv::new_in_memory();
	populate_fault_matrix_legacy(&kv).await?;
	let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	*harness.indeterminate_commit_after_sql.lock() = Some("INSERT INTO _rivet_conn_state");

	let error = import_core_state_if_needed(&ctx)
		.await
		.expect_err("lost commit response should make the first attempt indeterminate");
	assert!(
		format!("{error:#}").contains("injected lost commit response"),
		"{error:#}"
	);
	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("importing".to_owned())
	);

	import_core_state_if_needed(&ctx).await?;
	assert_eq!(
		internal_storage::load_meta_text(ctx.sql(), "kv_import_state").await?,
		Some("done".to_owned())
	);
	assert_eq!(
		internal_storage::load_connections(ctx.sql()).await?.len(),
		1
	);
	assert_eq!(
		internal_storage::load_queue_messages(ctx.sql())
			.await?
			.len(),
		1
	);
	assert_eq!(
		internal_storage::user_kv_batch_get(ctx.sql(), &[make_prefixed_key(b"user").as_slice()])
			.await?,
		vec![Some(b"user-value".to_vec())]
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn large_interrupted_import_cleanup_is_chunked_and_retryable() -> Result<()> {
	let kv = Kv::new_in_memory();
	let imported_key = make_prefixed_key(b"survivor");
	kv.put(&imported_key, b"legacy").await?;
	let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;

	let stale = (0..1_025usize)
		.map(|index| {
			(
				make_prefixed_key(format!("stale-{index:04}").as_bytes()),
				vec![index as u8; 4 * 1024],
			)
		})
		.collect::<Vec<_>>();
	let stale_refs = stale
		.iter()
		.map(|(key, value)| (key.as_slice(), value.as_slice()))
		.collect::<Vec<_>>();
	internal_storage::user_kv_batch_put(ctx.sql(), &stale_refs).await?;
	internal_storage::persist_meta_text(ctx.sql(), "kv_import_state", "importing").await?;
	harness
		.max_transaction_statements
		.store(0, Ordering::SeqCst);

	import_core_state_if_needed(&ctx).await?;
	assert!(
		harness.max_transaction_statements.load(Ordering::SeqCst)
			<= internal_storage::KV_TX_MAX_ROWS,
		"cleanup must keep every transaction within the row budget"
	);
	let row_count = ctx
		.sql()
		.query("SELECT COUNT(*) FROM _rivet_user_kv", None)
		.await?;
	assert_eq!(row_count.rows, vec![vec![ColumnValue::Integer(1)]]);
	assert_eq!(
		internal_storage::user_kv_batch_get(ctx.sql(), &[imported_key.as_slice()]).await?,
		vec![Some(b"legacy".to_vec())]
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn queue_import_batches_row_boundaries_and_writes_next_id_once() -> Result<()> {
	for row_count in [127usize, 128, 129] {
		let kv = Kv::new_in_memory();
		for index in 0..row_count {
			kv.put(
				&make_queue_message_key(index as u64 + 1),
				&encode_queue_message(&PersistedQueueMessage {
					name: format!("job-{index}"),
					body: vec![index as u8; 32],
					created_at: index as i64,
					failure_count: None,
					available_at: None,
					in_flight: None,
					in_flight_at: None,
				})?,
			)
			.await?;
		}
		let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
		internal_schema::ensure_internal_schema(ctx.sql()).await?;
		harness
			.max_transaction_statements
			.store(0, Ordering::SeqCst);
		harness.queue_next_id_writes.store(0, Ordering::SeqCst);
		import_core_state_if_needed(&ctx).await?;
		assert_eq!(
			internal_storage::load_queue_messages(ctx.sql())
				.await?
				.len(),
			row_count
		);
		assert_eq!(harness.queue_next_id_writes.load(Ordering::SeqCst), 1);
		assert!(
			harness.max_transaction_statements.load(Ordering::SeqCst)
				<= internal_storage::KV_TX_MAX_ROWS
		);
		drop(ctx);
		sqlite_task.abort();
	}
	Ok(())
}

#[tokio::test]
async fn connection_import_bounds_expanded_statements_per_transaction() -> Result<()> {
	for row_count in [63usize, 64, 65, 128, 129] {
		let kv = Kv::new_in_memory();
		for index in 0..row_count {
			let connection = PersistedConnection {
				id: format!("connection-{index:04}"),
				state: vec![index as u8; 32],
				..PersistedConnection::default()
			};
			kv.put(
				&make_connection_key(&connection.id),
				&encode_persisted_connection(&connection)?,
			)
			.await?;
		}

		let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
		internal_schema::ensure_internal_schema(ctx.sql()).await?;
		harness
			.max_transaction_statements
			.store(0, Ordering::SeqCst);
		import_core_state_if_needed(&ctx).await?;
		assert_eq!(
			internal_storage::load_connections(ctx.sql()).await?.len(),
			row_count
		);
		assert!(
			harness.max_transaction_statements.load(Ordering::SeqCst)
				<= internal_storage::KV_TX_MAX_ROWS,
			"{row_count} connection records exceeded the statement budget"
		);
		drop(ctx);
		sqlite_task.abort();
	}
	Ok(())
}

#[tokio::test]
async fn import_preserves_user_tables_when_actor_has_no_declared_database() -> Result<()> {
	let kv = Kv::new_in_memory();
	let key = make_prefixed_key(b"user-key");
	kv.put(&key, b"legacy-value").await?;
	let (ctx, sqlite_task, _) = sqlite_ctx_with_harness_enabled(kv, false);
	assert!(
		!ctx.sql().is_enabled(),
		"fixture must model an actor without db()"
	);
	ctx.sql()
		.execute(
			"CREATE TABLE user_owned (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
			None,
		)
		.await?;
	ctx.sql()
		.execute(
			"INSERT INTO user_owned (id, value) VALUES (1, 'keep')",
			None,
		)
		.await?;
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	import_core_state_if_needed(&ctx).await?;

	let user_rows = ctx
		.sql()
		.query("SELECT id, value FROM user_owned", None)
		.await?;
	assert_eq!(
		user_rows.rows,
		vec![vec![
			ColumnValue::Integer(1),
			ColumnValue::Text("keep".to_owned())
		]]
	);
	assert_eq!(
		internal_storage::user_kv_batch_get(ctx.sql(), &[key.as_slice()]).await?,
		vec![Some(b"legacy-value".to_vec())]
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

async fn load_workflow_value(ctx: &ActorContext, key: &[u8]) -> Result<Option<Vec<u8>>> {
	let result = ctx
		.sql()
		.query(
			"SELECT value FROM _rivet_wf_kv WHERE key = ?",
			Some(vec![crate::sqlite::BindParam::Blob(make_workflow_key(key))]),
		)
		.await?;
	Ok(match result.rows.first().and_then(|row| row.first()) {
		Some(ColumnValue::Blob(value)) => Some(value.clone()),
		Some(ColumnValue::Null) => Some(Vec::new()),
		None => None,
		other => anyhow::bail!("unexpected workflow value: {other:?}"),
	})
}

#[tokio::test]
async fn atomic_workflow_flush_rolls_back_both_sides_and_replays_idempotently() -> Result<()> {
	let kv = Kv::new_in_memory();
	let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	ctx.commit_serialized_state_and_workflow_batch(
		vec![StateDelta::ActorState(b"old-state".to_vec())],
		vec![WorkflowKvWrite {
			key: b"step".to_vec(),
			value: b"old-workflow".to_vec(),
		}],
	)
	.await?;

	*harness.fault.lock() = Some(SqliteFault {
		sql_contains: "INSERT INTO _rivet_wf_kv",
		blob_param: Some(b"new-workflow"),
	});
	assert!(
		ctx.commit_serialized_state_and_workflow_batch(
			vec![StateDelta::ActorState(b"new-state".to_vec())],
			vec![WorkflowKvWrite {
				key: b"step".to_vec(),
				value: b"new-workflow".to_vec(),
			}],
		)
		.await
		.is_err()
	);
	assert_eq!(ctx.state(), b"old-state");
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.expect("seed actor state should remain")
			.actor
			.state,
		b"old-state"
	);
	assert_eq!(
		load_workflow_value(&ctx, b"step").await?,
		Some(b"old-workflow".to_vec())
	);

	let replay_writes = vec![WorkflowKvWrite {
		key: b"step".to_vec(),
		value: b"new-workflow".to_vec(),
	}];
	ctx.commit_serialized_state_and_workflow_batch(
		vec![StateDelta::ActorState(b"new-state".to_vec())],
		replay_writes.clone(),
	)
	.await?;
	ctx.commit_serialized_state_and_workflow_batch(
		vec![StateDelta::ActorState(b"new-state".to_vec())],
		replay_writes,
	)
	.await?;
	assert_eq!(ctx.state(), b"new-state");
	assert_eq!(
		load_workflow_value(&ctx, b"step").await?,
		Some(b"new-workflow".to_vec())
	);
	let count = ctx
		.sql()
		.query("SELECT COUNT(*) FROM _rivet_wf_kv", None)
		.await?;
	assert_eq!(count.rows, vec![vec![ColumnValue::Integer(1)]]);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn atomic_workflow_flush_replays_after_commit_response_is_lost() -> Result<()> {
	let kv = Kv::new_in_memory();
	let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	ctx.commit_serialized_state_and_workflow_batch(
		vec![StateDelta::ActorState(b"old-state".to_vec())],
		vec![WorkflowKvWrite {
			key: b"step".to_vec(),
			value: b"old-workflow".to_vec(),
		}],
	)
	.await?;
	*harness.indeterminate_commit_after_sql.lock() = Some("INSERT INTO _rivet_wf_kv");

	let writes = vec![WorkflowKvWrite {
		key: b"step".to_vec(),
		value: b"committed-workflow".to_vec(),
	}];
	assert!(
		ctx.commit_serialized_state_and_workflow_batch(
			vec![StateDelta::ActorState(b"committed-state".to_vec())],
			writes.clone(),
		)
		.await
		.is_err()
	);
	assert_eq!(
		ctx.state(),
		b"old-state",
		"memory must not claim an unacknowledged commit"
	);
	assert_eq!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.expect("committed actor state should exist")
			.actor
			.state,
		b"committed-state"
	);
	assert_eq!(
		load_workflow_value(&ctx, b"step").await?,
		Some(b"committed-workflow".to_vec())
	);

	ctx.commit_serialized_state_and_workflow_batch(
		vec![StateDelta::ActorState(b"committed-state".to_vec())],
		writes,
	)
	.await?;
	assert_eq!(ctx.state(), b"committed-state");
	assert_eq!(
		load_workflow_value(&ctx, b"step").await?,
		Some(b"committed-workflow".to_vec())
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn atomic_workflow_flush_rejects_whole_units_over_transaction_budget() -> Result<()> {
	let kv = Kv::new_in_memory();
	let (ctx, sqlite_task) = sqlite_ctx(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	let writes = (0..127)
		.map(|index| WorkflowKvWrite {
			key: format!("key-{index}").into_bytes(),
			value: vec![index as u8],
		})
		.collect();
	let error = ctx
		.commit_serialized_state_and_workflow_batch(
			vec![StateDelta::ActorState(b"must-not-persist".to_vec())],
			writes,
		)
		.await
		.expect_err("the atomic unit must not be chunked");
	assert!(format!("{error:#}").contains("exceeds transaction budget"));
	assert!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.is_none()
	);
	assert_eq!(
		ctx.sql()
			.query("SELECT COUNT(*) FROM _rivet_wf_kv", None)
			.await?
			.rows,
		vec![vec![ColumnValue::Integer(0)]]
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn atomic_workflow_only_flush_prefixes_raw_keys_once() -> Result<()> {
	let kv = Kv::new_in_memory();
	let (ctx, sqlite_task) = sqlite_ctx(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	ctx.commit_serialized_state_and_workflow_batch(
		Vec::new(),
		vec![WorkflowKvWrite {
			key: b"raw-key".to_vec(),
			value: b"workflow-only".to_vec(),
		}],
	)
	.await?;

	assert!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.is_none()
	);
	let rows = ctx
		.sql()
		.query("SELECT key, value FROM _rivet_wf_kv", None)
		.await?;
	assert_eq!(
		rows.rows,
		vec![vec![
			ColumnValue::Blob(make_workflow_key(b"raw-key")),
			ColumnValue::Blob(b"workflow-only".to_vec()),
		]]
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

#[tokio::test]
async fn atomic_workflow_flush_rejects_oversized_values_without_partial_state() -> Result<()> {
	let kv = Kv::new_in_memory();
	let (ctx, sqlite_task) = sqlite_ctx(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	let error = ctx
		.commit_serialized_state_and_workflow_batch(
			vec![StateDelta::ActorState(b"must-not-persist".to_vec())],
			vec![WorkflowKvWrite {
				key: b"oversized".to_vec(),
				value: vec![0; 256 * 1024 + 1],
			}],
		)
		.await
		.expect_err("oversized workflow values must fail before sqlite execution");
	assert!(format!("{error:#}").contains("workflow kv value exceeds sqlite storage limit"));
	assert!(
		internal_storage::load_actor_snapshot(ctx.sql())
			.await?
			.is_none()
	);
	assert_eq!(
		ctx.sql()
			.query("SELECT COUNT(*) FROM _rivet_wf_kv", None)
			.await?
			.rows,
		vec![vec![ColumnValue::Integer(0)]]
	);
	drop(ctx);
	sqlite_task.abort();
	Ok(())
}

struct MigrationBenchCase {
	name: &'static str,
	user_rows: usize,
	workflow_rows: usize,
	queue_rows: usize,
	connection_rows: usize,
	value_bytes: usize,
}

#[tokio::test]
#[ignore = "manual large migration benchmark"]
async fn benchmark_large_full_migrations() -> Result<()> {
	let profile =
		std::env::var("RIVETKIT_MIGRATION_BENCH_PROFILE").unwrap_or_else(|_| "quick".to_owned());
	let cases = if profile == "full" {
		vec![
			MigrationBenchCase {
				name: "user-tiny-1k",
				user_rows: 1_000,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 32,
			},
			MigrationBenchCase {
				name: "user-tiny-10k",
				user_rows: 10_000,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 32,
			},
			MigrationBenchCase {
				name: "user-tiny-100k",
				user_rows: 100_000,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 32,
			},
			MigrationBenchCase {
				name: "user-bytes-4mib",
				user_rows: 1_024,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 4 * 1024,
			},
			MigrationBenchCase {
				name: "user-bytes-40mib",
				user_rows: 10_240,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 4 * 1024,
			},
			MigrationBenchCase {
				name: "user-bytes-100mib",
				user_rows: 25_600,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 4 * 1024,
			},
			MigrationBenchCase {
				name: "mixed-40k",
				user_rows: 10_000,
				workflow_rows: 10_000,
				queue_rows: 10_000,
				connection_rows: 10_000,
				value_bytes: 256,
			},
		]
	} else {
		vec![
			MigrationBenchCase {
				name: "user-tiny-1k",
				user_rows: 1_000,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 32,
			},
			MigrationBenchCase {
				name: "user-tiny-10k",
				user_rows: 10_000,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 32,
			},
			MigrationBenchCase {
				name: "user-bytes-4mib",
				user_rows: 1_024,
				workflow_rows: 0,
				queue_rows: 0,
				connection_rows: 0,
				value_bytes: 4 * 1024,
			},
		]
	};

	for case in cases {
		run_migration_bench_case(case).await?;
	}
	Ok(())
}

async fn run_migration_bench_case(case: MigrationBenchCase) -> Result<()> {
	let kv = Kv::new_in_memory();
	let value = vec![0x5a; case.value_bytes];
	let mut input_bytes = 0usize;

	for index in 0..case.user_rows {
		let key = make_prefixed_key(format!("bench-user-{index:08}").as_bytes());
		input_bytes = input_bytes.saturating_add(key.len() + value.len());
		kv.put(&key, &value).await?;
	}
	for index in 0..case.workflow_rows {
		let key = make_workflow_key(format!("bench-workflow-{index:08}").as_bytes());
		input_bytes = input_bytes.saturating_add(key.len() + value.len());
		kv.put(&key, &value).await?;
	}
	for index in 0..case.queue_rows {
		let key = make_queue_message_key(index as u64 + 1);
		let encoded = encode_queue_message(&PersistedQueueMessage {
			name: format!("bench-queue-{index:08}"),
			body: value.clone(),
			created_at: index as i64,
			failure_count: None,
			available_at: None,
			in_flight: None,
			in_flight_at: None,
		})?;
		input_bytes = input_bytes.saturating_add(key.len() + encoded.len());
		kv.put(&key, &encoded).await?;
	}
	for index in 0..case.connection_rows {
		let connection = PersistedConnection {
			id: format!("bench-connection-{index:08}"),
			state: value.clone(),
			..PersistedConnection::default()
		};
		let key = make_connection_key(&connection.id);
		let encoded = encode_persisted_connection(&connection)?;
		input_bytes = input_bytes.saturating_add(key.len() + encoded.len());
		kv.put(&key, &encoded).await?;
	}

	let row_count = case
		.user_rows
		.saturating_add(case.workflow_rows)
		.saturating_add(case.queue_rows)
		.saturating_add(case.connection_rows);
	let (ctx, sqlite_task, harness) = sqlite_ctx_with_harness(kv);
	internal_schema::ensure_internal_schema(ctx.sql()).await?;
	harness.request_count.store(0, Ordering::Relaxed);
	harness.transaction_count.store(0, Ordering::Relaxed);
	harness
		.max_transaction_statements
		.store(0, Ordering::Relaxed);

	let started_at = Instant::now();
	import_core_state_if_needed(&ctx).await?;
	let elapsed = started_at.elapsed();
	let requests = harness.request_count.load(Ordering::Relaxed);
	let transactions = harness.transaction_count.load(Ordering::Relaxed);
	for (table, expected_rows) in [
		("_rivet_user_kv", case.user_rows),
		("_rivet_wf_kv", case.workflow_rows),
		("_rivet_queue", case.queue_rows),
		("_rivet_conns", case.connection_rows),
		("_rivet_conn_state", case.connection_rows),
	] {
		let result = ctx
			.sql()
			.query(&format!("SELECT COUNT(*) FROM {table}"), None)
			.await?;
		assert_eq!(
			result.rows,
			vec![vec![ColumnValue::Integer(expected_rows as i64)]],
			"benchmark case {} did not fully migrate {table}",
			case.name
		);
	}
	println!(
		"migration_bench name={} rows={} input_bytes={} elapsed_ms={:.3} rows_per_second={:.1} mib_per_second={:.2} remote_requests={} transactions={} max_transaction_statements={}",
		case.name,
		row_count,
		input_bytes,
		elapsed.as_secs_f64() * 1_000.0,
		row_count as f64 / elapsed.as_secs_f64(),
		input_bytes as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64(),
		requests,
		transactions,
		harness.max_transaction_statements.load(Ordering::Relaxed),
	);

	drop(ctx);
	sqlite_task.abort();
	Ok(())
}
