use super::*;

use std::collections::HashMap;
use std::sync::Arc;

use rivet_envoy_client::config::{
	BoxFuture as EnvoyBoxFuture, EnvoyCallbacks as TestEnvoyCallbacks,
	EnvoyConfig as TestEnvoyConfig, HttpRequest as TestHttpRequest,
	HttpResponse as TestHttpResponse, WebSocketHandler as TestWebSocketHandler,
	WebSocketSender as TestWebSocketSender,
};
use rivet_envoy_client::context::{
	SharedContext as TestSharedContext, WsTxMessage as TestWsTxMessage,
};
use rivet_envoy_client::envoy::ToEnvoyMessage as TestToEnvoyMessage;
use rivet_envoy_client::handle::EnvoyHandle as TestEnvoyHandle;
use rivet_envoy_client::protocol;
use rivet_envoy_client::sqlite::{
	RemoteSqliteRequest as TestRemoteSqliteRequest,
	RemoteSqliteResponse as TestRemoteSqliteResponse,
	RemoteSqliteResponseEnvelope as TestRemoteSqliteResponseEnvelope,
};
use rusqlite::types::{Value as TestSqliteValue, ValueRef as TestSqliteValueRef};
use tokio::sync::mpsc;

struct TestIdleEnvoyCallbacks;

impl TestEnvoyCallbacks for TestIdleEnvoyCallbacks {
	fn on_actor_start(
		&self,
		_handle: TestEnvoyHandle,
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
		_handle: TestEnvoyHandle,
		_actor_id: String,
		_gateway_id: protocol::GatewayId,
		_request_id: protocol::RequestId,
		_request: TestHttpRequest,
	) -> EnvoyBoxFuture<anyhow::Result<TestHttpResponse>> {
		Box::pin(async { anyhow::bail!("fetch should not run in context helper") })
	}

	fn websocket(
		&self,
		_handle: TestEnvoyHandle,
		_actor_id: String,
		_gateway_id: protocol::GatewayId,
		_request_id: protocol::RequestId,
		_request: TestHttpRequest,
		_path: String,
		_headers: HashMap<String, String>,
		_is_hibernatable: bool,
		_is_restoring_hibernatable: bool,
		_sender: TestWebSocketSender,
	) -> EnvoyBoxFuture<anyhow::Result<TestWebSocketHandler>> {
		Box::pin(async { anyhow::bail!("websocket should not run in context helper") })
	}

	fn can_hibernate(
		&self,
		_actor_id: &str,
		_gateway_id: &protocol::GatewayId,
		_request_id: &protocol::RequestId,
		_request: &TestHttpRequest,
	) -> EnvoyBoxFuture<anyhow::Result<bool>> {
		Box::pin(async { Ok(false) })
	}
}

pub(crate) fn new_with_kv(
	actor_id: impl Into<String>,
	name: impl Into<String>,
	key: ActorKey,
	region: impl Into<String>,
	kv: crate::kv::Kv,
) -> ActorContext {
	let (sql_handle, sql_rx) = test_envoy_handle();
	spawn_test_remote_sqlite(sql_rx);
	build_ctx_with_remote_sqlite(
		actor_id,
		name,
		key,
		region,
		kv,
		sql_handle,
		ActorConfig::default(),
	)
}

/// Like [`new_with_kv`], but the remote sqlite executor stalls every request
/// matching the gate until the test releases it. Used to observe in-flight
/// state writes deterministically.
pub(crate) fn new_with_kv_and_write_gate(
	actor_id: impl Into<String>,
	name: impl Into<String>,
	key: ActorKey,
	region: impl Into<String>,
	kv: crate::kv::Kv,
	gate: TestSqliteWriteGate,
) -> ActorContext {
	let (sql_handle, sql_rx) = test_envoy_handle();
	spawn_test_remote_sqlite_on(
		open_test_sqlite_connection_with_schema(),
		sql_rx,
		Some(gate),
	);
	build_ctx_with_remote_sqlite(
		actor_id,
		name,
		key,
		region,
		kv,
		sql_handle,
		ActorConfig::default(),
	)
}

fn build_ctx_with_remote_sqlite(
	actor_id: impl Into<String>,
	name: impl Into<String>,
	key: ActorKey,
	region: impl Into<String>,
	kv: crate::kv::Kv,
	sql_handle: TestEnvoyHandle,
	config: ActorConfig,
) -> ActorContext {
	let actor_id = actor_id.into();
	let ctx = ActorContext::build(
		actor_id.clone(),
		name.into(),
		key,
		region.into(),
		Some(1),
		sql_handle.get_envoy_key().to_owned(),
		config,
		kv,
		SqliteDb::new_with_remote_sqlite(sql_handle.clone(), actor_id, None, Some(1), false, true)
			.expect("test remote sqlite should be configured"),
	);
	ctx.configure_envoy(sql_handle, Some(1));
	ctx
}

fn test_envoy_handle() -> (TestEnvoyHandle, mpsc::UnboundedReceiver<TestToEnvoyMessage>) {
	let (envoy_tx, envoy_rx) = mpsc::unbounded_channel();
	let shared = Arc::new(TestSharedContext {
		config: TestEnvoyConfig {
			version: 1,
			endpoint: "http://127.0.0.1:1".to_string(),
			token: None,
			namespace: "test".to_string(),
			pool_name: "test".to_string(),
			prepopulate_actor_names: HashMap::new(),
			metadata: None,
			not_global: true,
			debug_latency_ms: None,
			callbacks: Arc::new(TestIdleEnvoyCallbacks),
		},
		envoy_key: "test-envoy".to_string(),
		envoy_tx,
		actors: Default::default(),
		actors_notify: Arc::new(tokio::sync::Notify::new()),
		live_tunnel_requests: Default::default(),
		pending_hibernation_restores: Default::default(),
		ws_tx: Arc::new(tokio::sync::Mutex::new(
			None::<mpsc::UnboundedSender<TestWsTxMessage>>,
		)),
		http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
		connection_session: std::sync::atomic::AtomicU64::new(1),
		next_connection_session: std::sync::atomic::AtomicU64::new(1),
		connection_session_tx: tokio::sync::watch::channel(1).0,
		protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
		shutting_down: std::sync::atomic::AtomicBool::new(false),
		last_ping_ts: std::sync::atomic::AtomicI64::new(i64::MAX),
		stopped_tx: tokio::sync::watch::channel(true).0,
	});

	(TestEnvoyHandle::from_shared(shared), envoy_rx)
}

/// Stalls remote sqlite requests whose SQL starts with `sql_prefix`: each
/// matching request first reports on `entered_tx`, then waits for one permit
/// on `release` before executing.
pub(crate) struct TestSqliteWriteGate {
	pub(crate) sql_prefix: &'static str,
	pub(crate) entered_tx: mpsc::UnboundedSender<()>,
	pub(crate) release: Arc<tokio::sync::Semaphore>,
}

fn open_test_sqlite_connection_with_schema() -> rusqlite::Connection {
	let conn = rusqlite::Connection::open_in_memory().expect("test sqlite connection should open");
	crate::actor::internal_storage::schema::initialize_test_schema(&conn)
		.expect("test sqlite internal schema should initialize");
	conn
}

fn spawn_test_remote_sqlite(rx: mpsc::UnboundedReceiver<TestToEnvoyMessage>) {
	spawn_test_remote_sqlite_on(open_test_sqlite_connection_with_schema(), rx, None);
}

fn spawn_test_remote_sqlite_on(
	conn: rusqlite::Connection,
	mut rx: mpsc::UnboundedReceiver<TestToEnvoyMessage>,
	write_gate: Option<TestSqliteWriteGate>,
) {
	std::thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("test sqlite runtime should build");
		runtime.block_on(async move {
			while let Some(message) = rx.recv().await {
				let TestToEnvoyMessage::RemoteSqliteRequest {
					request,
					expected_session: _,
					response_tx,
				} = message
				else {
					continue;
				};
				let response = match request {
					TestRemoteSqliteRequest::Execute(request) => {
						if let Some(gate) = write_gate
							.as_ref()
							.filter(|gate| request.sql.starts_with(gate.sql_prefix))
						{
							let _ = gate.entered_tx.send(());
							gate.release
								.acquire()
								.await
								.expect("write gate semaphore should stay open")
								.forget();
						}
						TestRemoteSqliteResponse::Execute(execute_test_sqlite(&conn, request))
					}
					TestRemoteSqliteRequest::ExecuteBatch(request) => {
						if let Some(gate) = write_gate.as_ref().filter(|gate| {
							request
								.statements
								.iter()
								.any(|statement| statement.sql.starts_with(gate.sql_prefix))
						}) {
							let _ = gate.entered_tx.send(());
							gate.release
								.acquire()
								.await
								.expect("write gate semaphore should stay open")
								.forget();
						}
						TestRemoteSqliteResponse::ExecuteBatch(execute_test_sqlite_batch(
							&conn, request,
						))
					}
					TestRemoteSqliteRequest::Exec(_) => continue,
				};
				let _ = response_tx.send(Ok(TestRemoteSqliteResponseEnvelope {
					response,
					session: 1,
				}));
			}
		});
	});
}

fn execute_test_sqlite(
	conn: &rusqlite::Connection,
	request: protocol::SqliteExecuteRequest,
) -> protocol::SqliteExecuteResponse {
	match execute_test_sqlite_inner(conn, request) {
		Ok(result) => {
			protocol::SqliteExecuteResponse::SqliteExecuteOk(protocol::SqliteExecuteOk { result })
		}
		Err(error) => {
			protocol::SqliteExecuteResponse::SqliteErrorResponse(protocol::SqliteErrorResponse {
				group: "sqlite".to_owned(),
				code: "internal_error".to_owned(),
				message: error.to_string(),
			})
		}
	}
}

fn execute_test_sqlite_batch(
	conn: &rusqlite::Connection,
	request: protocol::SqliteExecuteBatchRequest,
) -> protocol::SqliteExecuteBatchResponse {
	let result = (|| {
		conn.execute_batch("BEGIN")?;
		let mut results = Vec::with_capacity(request.statements.len());
		for statement in request.statements {
			let result = execute_test_sqlite_inner(
				conn,
				protocol::SqliteExecuteRequest {
					namespace_id: request.namespace_id.clone(),
					actor_id: request.actor_id.clone(),
					generation: request.generation,
					sql: statement.sql,
					params: statement.params,
				},
			);
			match result {
				Ok(result) => results.push(result),
				Err(error) => {
					let _ = conn.execute_batch("ROLLBACK");
					return Err(error);
				}
			}
		}
		conn.execute_batch("COMMIT")?;
		Ok(results)
	})();
	match result {
		Ok(results) => protocol::SqliteExecuteBatchResponse::SqliteExecuteBatchOk(
			protocol::SqliteExecuteBatchOk { results },
		),
		Err(error) => protocol::SqliteExecuteBatchResponse::SqliteErrorResponse(
			protocol::SqliteErrorResponse {
				group: "sqlite".to_owned(),
				code: "internal_error".to_owned(),
				message: error.to_string(),
			},
		),
	}
}

fn execute_test_sqlite_inner(
	conn: &rusqlite::Connection,
	request: protocol::SqliteExecuteRequest,
) -> rusqlite::Result<protocol::SqliteExecuteResult> {
	let params = request
		.params
		.unwrap_or_default()
		.into_iter()
		.map(test_sqlite_param_value)
		.collect::<Vec<_>>();
	let mut statement = conn.prepare(&request.sql)?;
	let columns = statement
		.column_names()
		.into_iter()
		.map(ToOwned::to_owned)
		.collect::<Vec<_>>();
	let mut result_rows = Vec::new();
	if statement.column_count() == 0 {
		statement.execute(rusqlite::params_from_iter(params.iter()))?;
	} else {
		let column_count = statement.column_count();
		let mut rows = statement.query(rusqlite::params_from_iter(params.iter()))?;
		while let Some(row) = rows.next()? {
			let mut values = Vec::with_capacity(column_count);
			for index in 0..column_count {
				values.push(test_sqlite_column_value(row.get_ref(index)?));
			}
			result_rows.push(values);
		}
	}

	Ok(protocol::SqliteExecuteResult {
		columns,
		rows: result_rows,
		changes: conn.changes() as i64,
		last_insert_row_id: Some(conn.last_insert_rowid()),
	})
}

fn test_sqlite_param_value(param: protocol::SqliteBindParam) -> TestSqliteValue {
	match param {
		protocol::SqliteBindParam::SqliteValueNull => TestSqliteValue::Null,
		protocol::SqliteBindParam::SqliteValueInteger(value) => {
			TestSqliteValue::Integer(value.value)
		}
		protocol::SqliteBindParam::SqliteValueFloat(value) => {
			TestSqliteValue::Real(f64::from_bits(u64::from_be_bytes(value.value)))
		}
		protocol::SqliteBindParam::SqliteValueText(value) => TestSqliteValue::Text(value.value),
		protocol::SqliteBindParam::SqliteValueBlob(value) => TestSqliteValue::Blob(value.value),
	}
}

fn test_sqlite_column_value(value: TestSqliteValueRef<'_>) -> protocol::SqliteColumnValue {
	match value {
		TestSqliteValueRef::Null => protocol::SqliteColumnValue::SqliteValueNull,
		TestSqliteValueRef::Integer(value) => {
			protocol::SqliteColumnValue::SqliteValueInteger(protocol::SqliteValueInteger { value })
		}
		TestSqliteValueRef::Real(value) => {
			protocol::SqliteColumnValue::SqliteValueFloat(protocol::SqliteValueFloat {
				value: value.to_bits().to_be_bytes(),
			})
		}
		TestSqliteValueRef::Text(value) => {
			protocol::SqliteColumnValue::SqliteValueText(protocol::SqliteValueText {
				value: String::from_utf8_lossy(value).into_owned(),
			})
		}
		TestSqliteValueRef::Blob(value) => {
			protocol::SqliteColumnValue::SqliteValueBlob(protocol::SqliteValueBlob {
				value: value.to_vec(),
			})
		}
	}
}

#[test]
fn build_applies_actor_config_to_owned_subsystems() {
	let mut config = ActorConfig::default();
	config.max_queue_size = 7;
	config.max_queue_message_size = 11;
	config.create_conn_state_timeout = std::time::Duration::from_millis(123);
	config.connection_liveness_timeout = std::time::Duration::from_millis(456);
	config.sleep_timeout = std::time::Duration::from_millis(789);
	config.no_sleep = true;

	let (sql_handle, sql_rx) = test_envoy_handle();
	spawn_test_remote_sqlite(sql_rx);
	let ctx = build_ctx_with_remote_sqlite(
		"configured-actor".to_owned(),
		"configured".to_owned(),
		Vec::new(),
		"local".to_owned(),
		crate::kv::tests::new_in_memory(),
		sql_handle,
		config.clone(),
	);

	let queue_config = ctx.queue_config_for_tests();
	assert_eq!(queue_config.max_queue_size, config.max_queue_size);
	assert_eq!(
		queue_config.max_queue_message_size,
		config.max_queue_message_size
	);

	let connection_config = ctx.connection_config_for_tests();
	assert_eq!(
		connection_config.create_conn_state_timeout,
		config.create_conn_state_timeout
	);
	assert_eq!(
		connection_config.connection_liveness_timeout,
		config.connection_liveness_timeout
	);

	let sleep_config = ctx.sleep_config();
	assert_eq!(sleep_config.sleep_timeout, config.sleep_timeout);
	assert_eq!(sleep_config.no_sleep, config.no_sleep);
}

#[tokio::test]
async fn inspector_attach_guard_notifies_on_threshold_edges() {
	let ctx = ActorContext::new("inspector-actor", "actor", Vec::new(), "local");
	let attach_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
	let (overlay_tx, _) = tokio::sync::broadcast::channel(4);
	ctx.configure_inspector_runtime(std::sync::Arc::clone(&attach_count), overlay_tx);
	let (lifecycle_tx, mut lifecycle_rx) = tokio::sync::mpsc::unbounded_channel();
	ctx.configure_lifecycle_events(Some(lifecycle_tx));

	let first_guard = ctx
		.inspector_attach()
		.expect("inspector runtime should be configured");
	assert_eq!(ctx.inspector_attach_count(), 1);
	assert!(matches!(
		lifecycle_rx.try_recv(),
		Ok(LifecycleEvent::InspectorAttachmentsChanged)
	));

	let second_guard = ctx
		.inspector_attach()
		.expect("inspector runtime should be configured");
	assert_eq!(ctx.inspector_attach_count(), 2);
	assert!(matches!(
		lifecycle_rx.try_recv(),
		Err(tokio::sync::mpsc::error::TryRecvError::Empty)
	));

	drop(second_guard);
	assert_eq!(ctx.inspector_attach_count(), 1);
	assert!(matches!(
		lifecycle_rx.try_recv(),
		Err(tokio::sync::mpsc::error::TryRecvError::Empty)
	));

	drop(first_guard);
	assert_eq!(ctx.inspector_attach_count(), 0);
	assert!(matches!(
		lifecycle_rx.try_recv(),
		Ok(LifecycleEvent::InspectorAttachmentsChanged)
	));
}

#[tokio::test]
async fn disconnect_callback_guard_blocks_sleep_until_drop() {
	let ctx = ActorContext::new("actor-disconnect", "actor", Vec::new(), "local");
	ctx.set_started(true);

	let (started_tx, started_rx) = tokio::sync::oneshot::channel();
	let (release_tx, release_rx) = tokio::sync::oneshot::channel();
	let task = tokio::spawn({
		let ctx = ctx.clone();
		async move {
			ctx.with_disconnect_callback(|| async move {
				let _ = started_tx.send(());
				let _ = release_rx.await;
			})
			.await;
		}
	});

	started_rx.await.expect("disconnect callback should start");
	assert_eq!(ctx.pending_disconnect_count(), 1);
	assert_eq!(ctx.can_sleep().await, CanSleep::ActiveDisconnectCallbacks);

	release_tx
		.send(())
		.expect("disconnect callback should still be waiting");
	task.await.expect("disconnect callback task should join");

	assert_eq!(ctx.pending_disconnect_count(), 0);
	assert_eq!(ctx.can_sleep().await, CanSleep::Yes);
}

#[tokio::test(start_paused = true)]
async fn disconnect_callback_completion_resets_sleep_timer() {
	let ctx = ActorContext::new("actor-disconnect-timer", "actor", Vec::new(), "local");
	let mut config = ActorConfig::default();
	config.sleep_timeout = std::time::Duration::from_secs(5);
	ctx.configure_sleep(config);
	ctx.set_started(true);

	let (started_tx, started_rx) = tokio::sync::oneshot::channel();
	let (release_tx, release_rx) = tokio::sync::oneshot::channel();
	let task = tokio::spawn({
		let ctx = ctx.clone();
		async move {
			ctx.with_disconnect_callback(|| async move {
				let _ = started_tx.send(());
				let _ = release_rx.await;
			})
			.await;
		}
	});
	started_rx.await.expect("disconnect callback should start");

	tokio::time::advance(std::time::Duration::from_secs(10)).await;
	tokio::task::yield_now().await;
	assert_eq!(ctx.sleep_request_count(), 0);

	release_tx
		.send(())
		.expect("disconnect callback should still be waiting");
	task.await.expect("disconnect callback task should join");

	tokio::time::advance(std::time::Duration::from_secs(5)).await;
	tokio::task::yield_now().await;
	tokio::task::yield_now().await;
	assert_eq!(ctx.sleep_request_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn active_run_handler_blocks_sleep_until_cleared() {
	let ctx = ActorContext::new("actor-run-active", "actor", Vec::new(), "local");
	let mut config = ActorConfig::default();
	config.sleep_timeout = std::time::Duration::from_secs(5);
	ctx.configure_sleep(config);
	ctx.set_started(true);

	ctx.begin_run_handler();
	assert_eq!(ctx.can_sleep().await, CanSleep::ActiveRunHandler);

	tokio::time::advance(std::time::Duration::from_secs(10)).await;
	tokio::task::yield_now().await;
	assert_eq!(ctx.sleep_request_count(), 0);

	ctx.end_run_handler();
	assert_eq!(ctx.can_sleep().await, CanSleep::Yes);
	tokio::task::yield_now().await;

	tokio::time::advance(std::time::Duration::from_secs(5)).await;
	tokio::task::yield_now().await;
	tokio::task::yield_now().await;
	assert_eq!(ctx.sleep_request_count(), 1);
}

mod moved_tests {
	use std::collections::{BTreeSet, HashMap, HashSet};
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::sync::{Arc, Mutex};
	use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

	use anyhow::anyhow;
	use rivet_envoy_client::config::{
		BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
		WebSocketSender,
	};
	use rivet_envoy_client::context::{SharedActorEntry, SharedContext, WsTxMessage};
	use rivet_envoy_client::handle::EnvoyHandle;
	use rivet_envoy_client::protocol;
	use rivet_envoy_client::tunnel::HibernatingWebSocketMetadata;
	use tokio::sync::mpsc;
	use tokio::time::sleep;

	use super::ActorContext;
	use crate::actor::connection::ConnHandle;
	use crate::actor::messages::ActorEvent;
	use crate::types::ListOpts;
	use crate::{ActorConfig, SqliteDb};

	fn now_timestamp_ms() -> i64 {
		let duration = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap_or_default();
		i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
	}

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
			Box::pin(async { anyhow::bail!("fetch should not be called in context tests") })
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
			Box::pin(async { anyhow::bail!("websocket should not be called in context tests") })
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

	fn build_envoy_handle_with_live_connections(
		actor_id: &str,
		generation: u32,
		live_connections: HashSet<[u8; 8]>,
		pending_restores: Vec<HibernatingWebSocketMetadata>,
	) -> EnvoyHandle {
		let (envoy_tx, _envoy_rx) = mpsc::unbounded_channel();
		let live_tunnel_requests = Arc::new(std::sync::Mutex::new(HashMap::new()));
		{
			let mut requests = live_tunnel_requests
				.lock()
				.expect("live tunnel request registry poisoned");
			for request_key in live_connections {
				requests.insert(request_key, actor_id.to_owned());
			}
		}
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
			actors: Arc::new(std::sync::Mutex::new(HashMap::new())),
			actors_notify: Arc::new(tokio::sync::Notify::new()),
			live_tunnel_requests,
			pending_hibernation_restores: Arc::new(std::sync::Mutex::new(HashMap::from([(
				actor_id.to_owned(),
				pending_restores,
			)]))),
			ws_tx: Arc::new(tokio::sync::Mutex::new(
				None::<mpsc::UnboundedSender<WsTxMessage>>,
			)),
			http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
			connection_session: std::sync::atomic::AtomicU64::new(0),
			next_connection_session: std::sync::atomic::AtomicU64::new(0),
			connection_session_tx: tokio::sync::watch::channel(0).0,
			protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
			shutting_down: std::sync::atomic::AtomicBool::new(false),
			last_ping_ts: std::sync::atomic::AtomicI64::new(i64::MAX),
			stopped_tx: tokio::sync::watch::channel(true).0,
		});
		shared
			.actors
			.lock()
			.expect("shared actor registry poisoned")
			.entry(actor_id.to_owned())
			.or_insert_with(HashMap::new)
			.insert(
				generation,
				SharedActorEntry {
					handle: mpsc::unbounded_channel().0,
					active_http_request_count: Arc::new(
						rivet_envoy_client::async_counter::AsyncCounter::new(),
					),
				},
			);
		EnvoyHandle::from_shared(shared)
	}

	fn build_client_envoy_handle() -> EnvoyHandle {
		let (envoy_tx, _envoy_rx) = mpsc::unbounded_channel();
		let shared = Arc::new(SharedContext {
			config: EnvoyConfig {
				version: 1,
				endpoint: "http://127.0.0.1:7777".to_string(),
				token: Some("secret".to_string()),
				namespace: "test-ns".to_string(),
				pool_name: "test-pool".to_string(),
				prepopulate_actor_names: HashMap::new(),
				metadata: None,
				not_global: true,
				debug_latency_ms: None,
				callbacks: Arc::new(IdleEnvoyCallbacks),
			},
			envoy_key: "test-envoy".to_string(),
			envoy_tx,
			actors: Arc::new(std::sync::Mutex::new(HashMap::new())),
			actors_notify: Arc::new(tokio::sync::Notify::new()),
			live_tunnel_requests: Arc::new(std::sync::Mutex::new(HashMap::new())),
			pending_hibernation_restores: Arc::new(std::sync::Mutex::new(HashMap::new())),
			ws_tx: Arc::new(tokio::sync::Mutex::new(
				None::<mpsc::UnboundedSender<WsTxMessage>>,
			)),
			http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
			connection_session: std::sync::atomic::AtomicU64::new(0),
			next_connection_session: std::sync::atomic::AtomicU64::new(0),
			connection_session_tx: tokio::sync::watch::channel(0).0,
			protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
			shutting_down: std::sync::atomic::AtomicBool::new(false),
			last_ping_ts: std::sync::atomic::AtomicI64::new(i64::MAX),
			stopped_tx: tokio::sync::watch::channel(true).0,
		});
		EnvoyHandle::from_shared(shared)
	}

	#[tokio::test]
	#[allow(deprecated)]
	async fn kv_helpers_delegate_to_kv_wrapper() {
		let ctx = super::new_with_kv(
			"actor-1",
			"actor",
			Vec::new(),
			"local",
			crate::kv::tests::new_in_memory(),
		);

		ctx.kv_batch_put(&[(b"alpha".as_slice(), b"1".as_slice())])
			.await
			.expect("kv batch put should succeed");

		let values = ctx
			.kv_batch_get(&[b"alpha".as_slice()])
			.await
			.expect("kv batch get should succeed");
		assert_eq!(values, vec![Some(b"1".to_vec())]);

		let listed = ctx
			.kv_list_prefix(b"alp", ListOpts::default())
			.await
			.expect("kv list prefix should succeed");
		assert_eq!(listed, vec![(b"alpha".to_vec(), b"1".to_vec())]);

		ctx.kv_batch_delete(&[b"alpha".as_slice()])
			.await
			.expect("kv batch delete should succeed");
		let values = ctx
			.kv_batch_get(&[b"alpha".as_slice()])
			.await
			.expect("kv batch get after delete should succeed");
		assert_eq!(values, vec![None]);
	}

	#[test]
	fn client_accessors_read_config_from_wired_envoy_handle() {
		let handle = build_client_envoy_handle();
		let ctx = ActorContext::build(
			"client-actor".to_owned(),
			"actor".to_owned(),
			Vec::new(),
			"local".to_owned(),
			Some(1),
			handle.get_envoy_key().to_owned(),
			ActorConfig::default(),
			crate::kv::Kv::new(handle.clone(), "client-actor"),
			SqliteDb::new_with_remote_sqlite(
				handle.clone(),
				"client-actor",
				None,
				Some(1),
				false,
				true,
			)
			.expect("test remote sqlite should be configured"),
		);
		ctx.configure_envoy(handle, Some(1));

		assert_eq!(ctx.client_endpoint(), Some("http://127.0.0.1:7777"));
		assert_eq!(ctx.client_token(), Some("secret"));
		assert_eq!(ctx.client_namespace(), Some("test-ns"));
		assert_eq!(ctx.client_pool_name(), Some("test-pool"));
	}

	#[tokio::test]
	async fn connection_helpers_iterate_and_disconnect_without_managed_callback() {
		let ctx = super::new_with_kv(
			"actor-conns",
			"actor",
			Vec::new(),
			"local",
			crate::kv::tests::new_in_memory(),
		);
		let managed_disconnects = Arc::new(Mutex::new(Vec::<String>::new()));
		let transport_disconnects = Arc::new(Mutex::new(Vec::<String>::new()));

		let conn_a = ConnHandle::new("conn-a", vec![1], vec![2], false);
		conn_a.configure_disconnect_handler(Some(Arc::new({
			let managed_disconnects = managed_disconnects.clone();
			move |_reason| {
				let managed_disconnects = managed_disconnects.clone();
				Box::pin(async move {
					managed_disconnects
						.lock()
						.expect("managed disconnect log lock poisoned")
						.push("conn-a".to_owned());
					Ok(())
				})
			}
		})));
		conn_a.configure_transport_disconnect_handler(Some(Arc::new({
			let transport_disconnects = transport_disconnects.clone();
			move |_reason| {
				let transport_disconnects = transport_disconnects.clone();
				Box::pin(async move {
					transport_disconnects
						.lock()
						.expect("transport disconnect log lock poisoned")
						.push("conn-a".to_owned());
					Ok(())
				})
			}
		})));

		let conn_b = ConnHandle::new("conn-b", vec![3], vec![4], false);
		ctx.add_conn(conn_a);
		ctx.add_conn(conn_b);

		assert_eq!(
			ctx.conns()
				.map(|conn| conn.id().to_owned())
				.collect::<Vec<_>>(),
			vec!["conn-a".to_owned(), "conn-b".to_owned()]
		);
		assert_eq!(ctx.conns().len(), 2);

		ctx.disconnect_conn("conn-a".into())
			.await
			.expect("targeted disconnect should succeed");

		assert_eq!(
			transport_disconnects
				.lock()
				.expect("transport disconnect log lock poisoned")
				.as_slice(),
			["conn-a"]
		);
		assert!(
			managed_disconnects
				.lock()
				.expect("managed disconnect log lock poisoned")
				.is_empty()
		);
		assert_eq!(
			ctx.conns()
				.map(|conn| conn.id().to_owned())
				.collect::<Vec<_>>(),
			vec!["conn-b".to_owned()]
		);
	}

	#[tokio::test]
	async fn take_pending_hibernation_changes_snapshots_removals_without_draining_core_state() {
		let ctx = super::new_with_kv(
			"actor-hibernation-pending",
			"actor",
			Vec::new(),
			"local",
			crate::kv::tests::new_in_memory(),
		);

		ctx.request_hibernation_transport_save("conn-updated");
		ctx.request_hibernation_transport_removal("conn-removed");

		assert_eq!(
			ctx.take_pending_hibernation_changes(),
			vec!["conn-removed".to_owned()]
		);

		let pending = ctx.take_pending_hibernation_changes_inner();
		assert_eq!(pending.updated, BTreeSet::from(["conn-updated".to_owned()]));
		assert_eq!(pending.removed, BTreeSet::from(["conn-removed".to_owned()]));
	}

	#[tokio::test]
	async fn hibernated_connection_is_live_checks_specific_live_registry_entry() {
		let ctx = super::new_with_kv(
			"actor-live-conn",
			"actor",
			Vec::new(),
			"local",
			crate::kv::tests::new_in_memory(),
		);
		ctx.configure_envoy(
			build_envoy_handle_with_live_connections(
				"actor-live-conn",
				7,
				HashSet::from([[1, 2, 3, 4, 5, 6, 7, 8]]),
				Vec::new(),
			),
			Some(7),
		);

		assert!(
			ctx.hibernated_connection_is_live(&[1, 2, 3, 4], &[5, 6, 7, 8])
				.expect("matching live connection should be found")
		);
		assert!(
			!ctx.hibernated_connection_is_live(&[1, 2, 3, 4], &[9, 9, 9, 9])
				.expect("missing live connection should return false")
		);
	}

	#[tokio::test]
	async fn hibernated_connection_is_live_checks_pending_restore_registry_entry() {
		let ctx = super::new_with_kv(
			"actor-pending-restore-conn",
			"actor",
			Vec::new(),
			"local",
			crate::kv::tests::new_in_memory(),
		);
		ctx.configure_envoy(
			build_envoy_handle_with_live_connections(
				"actor-pending-restore-conn",
				3,
				HashSet::new(),
				vec![HibernatingWebSocketMetadata {
					gateway_id: [1, 2, 3, 4],
					request_id: [5, 6, 7, 8],
					envoy_message_index: 0,
					rivet_message_index: 0,
					path: "/ws".to_owned(),
					headers: HashMap::new(),
				}],
			),
			Some(3),
		);

		assert!(
			ctx.hibernated_connection_is_live(&[1, 2, 3, 4], &[5, 6, 7, 8])
				.expect("pending restore should count as a live hibernated connection")
		);
		assert!(
			!ctx.hibernated_connection_is_live(&[9, 9, 9, 9], &[5, 6, 7, 8])
				.expect("non-matching pending restore should return false")
		);
	}

	#[tokio::test]
	async fn disconnect_conns_continues_past_per_conn_errors() {
		let ctx = super::new_with_kv(
			"actor-disconnect-conns",
			"actor",
			Vec::new(),
			"local",
			crate::kv::tests::new_in_memory(),
		);

		let conn_a = ConnHandle::new("conn-a", vec![1], vec![2], false);
		conn_a.configure_transport_disconnect_handler(Some(Arc::new(move |_reason| {
			Box::pin(async move { Err(anyhow!("boom-a")) })
		})));

		let conn_b = ConnHandle::new("conn-b", vec![3], vec![4], false);
		let transport_disconnects = Arc::new(Mutex::new(Vec::<String>::new()));
		conn_b.configure_transport_disconnect_handler(Some(Arc::new({
			let transport_disconnects = transport_disconnects.clone();
			move |_reason| {
				let transport_disconnects = transport_disconnects.clone();
				Box::pin(async move {
					transport_disconnects
						.lock()
						.expect("transport disconnect log lock poisoned")
						.push("conn-b".to_owned());
					Ok(())
				})
			}
		})));

		ctx.add_conn(conn_a.clone());
		ctx.add_conn(conn_b);

		let error = ctx
			.disconnect_conns(|conn| conn.id().starts_with("conn-"))
			.await
			.expect_err("bulk disconnect should surface transport failures");
		let error_text = format!("{error:#}");
		assert!(error_text.contains("conn-a"));
		assert!(
			transport_disconnects
				.lock()
				.expect("transport disconnect log lock poisoned")
				.iter()
				.any(|conn_id| conn_id == "conn-b")
		);
		assert!(ctx.conns().any(|conn| conn.id() == "conn-a"));
		assert!(!ctx.conns().any(|conn| conn.id() == "conn-b"));

		let conn_c = ConnHandle::new("conn-c", vec![5], vec![6], false);
		conn_c.configure_transport_disconnect_handler(Some(Arc::new(move |_reason| {
			Box::pin(async move { Err(anyhow!("boom-c")) })
		})));
		ctx.add_conn(conn_c);

		let error = ctx
			.disconnect_conns(|conn| conn.id() == "conn-a" || conn.id() == "conn-c")
			.await
			.expect_err("bulk disconnect should aggregate multiple failures");
		let error_text = format!("{error:#}");
		assert!(error_text.contains("conn-a"));
		assert!(error_text.contains("conn-c"));
		assert!(ctx.conns().any(|conn| conn.id() == "conn-a"));
		assert!(ctx.conns().any(|conn| conn.id() == "conn-c"));
	}

	#[tokio::test(start_paused = true)]
	async fn init_alarms_arms_local_alarm_for_persisted_schedule_state() {
		let ctx = super::new_with_kv(
			"actor-init-alarms",
			"actor",
			Vec::new(),
			"local",
			crate::kv::Kv::new_in_memory(),
		);
		let now_ms = 1_700_000_000_000;
		ctx.set_schedule_time_for_tests(now_ms);
		let fired = Arc::new(AtomicUsize::new(0));
		ctx.set_local_alarm_callback(Some(Arc::new({
			let fired = fired.clone();
			move || {
				let fired = fired.clone();
				Box::pin(async move {
					fired.fetch_add(1, Ordering::SeqCst);
				})
			}
		})));
		ctx.at(now_ms + 20, "tick", &[1])
			.await
			.expect("persist future schedule");
		ctx.cancel_local_alarm_timeouts();
		ctx.init_alarms().await;

		tokio::task::yield_now().await;
		tokio::time::advance(Duration::from_millis(20)).await;
		tokio::task::yield_now().await;

		assert_eq!(fired.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn drain_overdue_scheduled_events_dispatches_actions_via_actor_inbox() {
		let ctx = super::new_with_kv(
			"actor-overdue-events",
			"actor",
			Vec::new(),
			"local",
			crate::kv::Kv::new_in_memory(),
		);
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(events_tx));
		ctx.at(now_timestamp_ms() - 1_000, "tick", &[1, 2, 3])
			.await
			.expect("persist overdue schedule");

		let recv = tokio::spawn(async move {
			match events_rx
				.recv()
				.await
				.expect("scheduled action event should arrive")
			{
				ActorEvent::Action {
					name,
					args,
					conn,
					scheduled_fire,
					reply,
				} => {
					assert_eq!(name, "tick");
					assert_eq!(args, vec![1, 2, 3]);
					assert!(conn.is_none());
					let fire = scheduled_fire.expect("scheduled fire metadata");
					assert_eq!(fire.kind, crate::actor::schedule::ScheduleKind::At);
					assert!(fire.name.is_none());
					reply.send(Ok(Vec::new()));
				}
				event => panic!("unexpected event: {event:?}"),
			}
		});

		ctx.drain_overdue_scheduled_events()
			.await
			.expect("draining overdue scheduled events should succeed");
		recv.await.expect("scheduled action receiver should join");

		assert!(ctx.list_scheduled_events().await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn keep_awake_region_blocks_sleep_idle_until_guard_drops() {
		let ctx = super::new_with_kv(
			"actor-keep-awake",
			"actor",
			Vec::new(),
			"local",
			crate::kv::Kv::new_in_memory(),
		);

		let keep_awake = tokio::spawn({
			let ctx = ctx.clone();
			async move {
				ctx.keep_awake(async {
					sleep(Duration::from_millis(30)).await;
				})
				.await;
			}
		});

		for _ in 0..20 {
			if ctx.keep_awake_count() > 0 {
				break;
			}
			sleep(Duration::from_millis(1)).await;
		}

		assert_eq!(ctx.keep_awake_count(), 1);
		assert!(
			!ctx.wait_for_sleep_idle_window(Instant::now() + Duration::from_millis(5))
				.await
		);
		assert!(
			ctx.wait_for_sleep_idle_window(Instant::now() + Duration::from_millis(100))
				.await
		);

		keep_awake.await.expect("keep_awake task should complete");
		assert_eq!(ctx.keep_awake_count(), 0);
	}

	#[tokio::test(start_paused = true)]
	async fn sleep_requests_envoy_on_next_scheduler_tick_without_wall_clock_delay() {
		let ctx = super::new_with_kv(
			"actor-sleep-request",
			"actor",
			Vec::new(),
			"local",
			crate::kv::Kv::new_in_memory(),
		);

		ctx.set_started(true);

		assert_eq!(ctx.sleep_request_count(), 0);

		ctx.sleep().expect("sleep should be accepted after startup");
		tokio::task::yield_now().await;

		assert_eq!(ctx.sleep_request_count(), 1);
	}
}
