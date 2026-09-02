//! Test-only actor context fixtures backed by a real in-memory SQLite database.
//!
//! Enable the `test-support` feature in downstream crates. The fixture keeps
//! the production invariant that every [`ActorContext`] has a configured
//! SQLite backend; it does not reintroduce an unavailable or in-memory runtime
//! backend.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use rivet_envoy_client::config::{
	BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
	WebSocketSender,
};
use rivet_envoy_client::context::{SharedContext, WsTxMessage};
use rivet_envoy_client::envoy::ToEnvoyMessage;
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_client::protocol;
use rivet_envoy_client::sqlite::{
	RemoteSqliteRequest, RemoteSqliteResponse, RemoteSqliteResponseEnvelope,
};
use rusqlite::types::{Value, ValueRef};
use tokio::sync::mpsc;

use crate::ActorConfig;
use crate::actor::context::ActorContext;
use crate::actor::kv::LegacyActorKv;
use crate::sqlite::SqliteDb;
use crate::types::ActorKey;

/// Reusable in-memory SQLite store for constructing fully configured contexts.
/// Contexts created from the same harness observe the same database.
#[derive(Clone)]
pub struct ActorContextHarness {
	handle: EnvoyHandle,
}

impl Default for ActorContextHarness {
	fn default() -> Self {
		Self::new()
	}
}

impl ActorContextHarness {
	pub fn new() -> Self {
		Self::with_client_endpoint("http://127.0.0.1:1")
	}

	/// Constructs a harness whose actor clients use the provided engine endpoint.
	pub fn with_client_endpoint(endpoint: impl Into<String>) -> Self {
		let (handle, receiver) = test_envoy_handle(endpoint.into());
		spawn_remote_sqlite(receiver);
		Self { handle }
	}

	pub fn context(
		&self,
		actor_id: impl Into<String>,
		name: impl Into<String>,
		key: ActorKey,
		region: impl Into<String>,
	) -> ActorContext {
		self.context_with_config(actor_id, name, key, region, ActorConfig::default())
	}

	pub fn context_with_config(
		&self,
		actor_id: impl Into<String>,
		name: impl Into<String>,
		key: ActorKey,
		region: impl Into<String>,
		config: ActorConfig,
	) -> ActorContext {
		let actor_id = actor_id.into();
		let generation = Some(1);
		let sql = SqliteDb::new_with_remote_sqlite(
			self.handle.clone(),
			actor_id.clone(),
			None,
			generation.map(u64::from),
			false,
			true,
		)
		.expect("test remote sqlite should be configured");
		let ctx = ActorContext::build(
			actor_id.clone(),
			name.into(),
			key,
			region.into(),
			generation,
			self.handle.get_envoy_key().to_owned(),
			config,
			LegacyActorKv::new(self.handle.clone(), actor_id),
			sql,
		);
		ctx.configure_envoy(self.handle.clone(), generation);
		ctx
	}
}

/// Constructs a context with its own in-memory SQLite store.
pub fn actor_context(
	actor_id: impl Into<String>,
	name: impl Into<String>,
	key: ActorKey,
	region: impl Into<String>,
) -> ActorContext {
	ActorContextHarness::new().context(actor_id, name, key, region)
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
	) -> BoxFuture<Result<()>> {
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
	) -> BoxFuture<Result<HttpResponse>> {
		Box::pin(async { anyhow::bail!("fetch should not run in actor context fixture") })
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
	) -> BoxFuture<Result<WebSocketHandler>> {
		Box::pin(async { anyhow::bail!("websocket should not run in actor context fixture") })
	}

	fn can_hibernate(
		&self,
		_actor_id: &str,
		_gateway_id: &protocol::GatewayId,
		_request_id: &protocol::RequestId,
		_request: &HttpRequest,
	) -> BoxFuture<Result<bool>> {
		Box::pin(async { Ok(false) })
	}
}

fn test_envoy_handle(endpoint: String) -> (EnvoyHandle, mpsc::UnboundedReceiver<ToEnvoyMessage>) {
	let (envoy_tx, envoy_rx) = mpsc::unbounded_channel();
	let shared = Arc::new(SharedContext {
		config: EnvoyConfig {
			version: 1,
			endpoint,
			token: None,
			namespace: "test".to_owned(),
			pool_name: "test".to_owned(),
			prepopulate_actor_names: HashMap::new(),
			metadata: None,
			not_global: true,
			debug_latency_ms: None,
			callbacks: Arc::new(IdleEnvoyCallbacks),
		},
		envoy_key: "test-envoy".to_owned(),
		envoy_tx,
		actors: Default::default(),
		actors_notify: Arc::new(tokio::sync::Notify::new()),
		live_tunnel_requests: Default::default(),
		pending_hibernation_restores: Default::default(),
		ws_tx: Arc::new(tokio::sync::Mutex::new(
			None::<mpsc::UnboundedSender<WsTxMessage>>,
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
	(EnvoyHandle::from_shared(shared), envoy_rx)
}

fn spawn_remote_sqlite(mut receiver: mpsc::UnboundedReceiver<ToEnvoyMessage>) {
	std::thread::spawn(move || {
		let conn =
			rusqlite::Connection::open_in_memory().expect("test sqlite connection should open");
		crate::actor::internal_storage::schema::initialize_test_schema(&conn)
			.expect("test sqlite schema should initialize");
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("test sqlite runtime should build");
		runtime.block_on(async move {
			while let Some(message) = receiver.recv().await {
				let ToEnvoyMessage::RemoteSqliteRequest {
					request,
					expected_session: _,
					response_tx,
				} = message
				else {
					continue;
				};
				let response = match request {
					RemoteSqliteRequest::Execute(request) => {
						RemoteSqliteResponse::Execute(execute_sqlite(&conn, request))
					}
					RemoteSqliteRequest::ExecuteBatch(request) => {
						RemoteSqliteResponse::ExecuteBatch(execute_sqlite_batch(&conn, request))
					}
					RemoteSqliteRequest::Exec(_) => continue,
				};
				let _ = response_tx.send(Ok(RemoteSqliteResponseEnvelope {
					response,
					session: 1,
				}));
			}
		});
	});
}

fn execute_sqlite_batch(
	conn: &rusqlite::Connection,
	request: protocol::SqliteExecuteBatchRequest,
) -> protocol::SqliteExecuteBatchResponse {
	let result = (|| {
		conn.execute_batch("BEGIN")?;
		let mut results = Vec::with_capacity(request.statements.len());
		for statement in request.statements {
			let result = execute_sqlite_inner(
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

fn execute_sqlite(
	conn: &rusqlite::Connection,
	request: protocol::SqliteExecuteRequest,
) -> protocol::SqliteExecuteResponse {
	match execute_sqlite_inner(conn, request) {
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

fn execute_sqlite_inner(
	conn: &rusqlite::Connection,
	request: protocol::SqliteExecuteRequest,
) -> rusqlite::Result<protocol::SqliteExecuteResult> {
	let params = request
		.params
		.unwrap_or_default()
		.into_iter()
		.map(sqlite_param_value)
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
				values.push(sqlite_column_value(row.get_ref(index)?));
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

fn sqlite_param_value(param: protocol::SqliteBindParam) -> Value {
	match param {
		protocol::SqliteBindParam::SqliteValueNull => Value::Null,
		protocol::SqliteBindParam::SqliteValueInteger(value) => Value::Integer(value.value),
		protocol::SqliteBindParam::SqliteValueFloat(value) => {
			Value::Real(f64::from_bits(u64::from_be_bytes(value.value)))
		}
		protocol::SqliteBindParam::SqliteValueText(value) => Value::Text(value.value),
		protocol::SqliteBindParam::SqliteValueBlob(value) => Value::Blob(value.value),
	}
}

fn sqlite_column_value(value: ValueRef<'_>) -> protocol::SqliteColumnValue {
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
