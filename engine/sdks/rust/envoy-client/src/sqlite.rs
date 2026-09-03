use rivet_envoy_protocol as protocol;
use tokio::sync::oneshot;

use crate::connection::{WsSendResult, ws_send, ws_send_for_session};
use crate::envoy::EnvoyContext;
use crate::kv::KV_EXPIRE_MS;
use crate::metrics::METRICS;
use crate::utils::{
	EnvoyShutdownError, RemoteSqliteConnectionSessionLostError,
	RemoteSqliteIndeterminateResultError,
};

#[derive(Clone)]
pub enum SqliteRequest {
	GetPages(protocol::SqliteGetPagesRequest),
	Commit(protocol::SqliteCommitRequest),
}

impl SqliteRequest {
	pub fn kind(&self) -> &'static str {
		match self {
			SqliteRequest::GetPages(_) => "get_pages",
			SqliteRequest::Commit(_) => "commit",
		}
	}
}

pub enum SqliteResponse {
	GetPages(protocol::SqliteGetPagesResponse),
	Commit(protocol::SqliteCommitResponse),
}

#[derive(Clone, Debug)]
pub enum RemoteSqliteRequest {
	Exec(protocol::SqliteExecRequest),
	Execute(protocol::SqliteExecuteRequest),
	ExecuteBatch(protocol::SqliteExecuteBatchRequest),
}

#[derive(Debug)]
pub enum RemoteSqliteResponse {
	Exec(protocol::SqliteExecResponse),
	Execute(protocol::SqliteExecuteResponse),
	ExecuteBatch(protocol::SqliteExecuteBatchResponse),
}

#[derive(Debug)]
pub struct RemoteSqliteResponseEnvelope {
	pub response: RemoteSqliteResponse,
	pub session: u64,
}

impl RemoteSqliteRequest {
	fn operation(&self) -> &'static str {
		match self {
			RemoteSqliteRequest::Exec(_) => "exec",
			RemoteSqliteRequest::Execute(_) => "execute",
			RemoteSqliteRequest::ExecuteBatch(_) => "execute_batch",
		}
	}

	pub fn kind(&self) -> &'static str {
		match self {
			RemoteSqliteRequest::Exec(_) => "remote_exec",
			RemoteSqliteRequest::Execute(_) => "remote_execute",
			RemoteSqliteRequest::ExecuteBatch(_) => "remote_execute_batch",
		}
	}
}

pub struct SqliteRequestEntry {
	pub request: SqliteRequest,
	pub response_tx: oneshot::Sender<anyhow::Result<SqliteResponse>>,
	pub sent: bool,
	pub timestamp: crate::time::Instant,
}

pub struct RemoteSqliteRequestEntry {
	pub request: RemoteSqliteRequest,
	pub response_tx: oneshot::Sender<anyhow::Result<RemoteSqliteResponseEnvelope>>,
	pub expected_session: Option<u64>,
	pub sent_session: Option<u64>,
	pub sent: bool,
	pub timestamp: crate::time::Instant,
}

pub async fn handle_sqlite_request(
	ctx: &mut EnvoyContext,
	request: SqliteRequest,
	response_tx: oneshot::Sender<anyhow::Result<SqliteResponse>>,
) {
	let request_id = ctx.next_sqlite_request_id;
	ctx.next_sqlite_request_id += 1;

	let entry = SqliteRequestEntry {
		request,
		response_tx,
		sent: false,
		timestamp: crate::time::Instant::now(),
	};

	ctx.sqlite_requests.insert(request_id, entry);
	METRICS.sqlite_requests_inflight.inc();

	let ws_available = {
		let guard = ctx.shared.ws_tx.lock().await;
		guard.is_some()
	};

	if ws_available {
		send_single_sqlite_request(ctx, request_id).await;
	}
}

pub async fn handle_remote_sqlite_request(
	ctx: &mut EnvoyContext,
	request: RemoteSqliteRequest,
	expected_session: Option<u64>,
	response_tx: oneshot::Sender<anyhow::Result<RemoteSqliteResponseEnvelope>>,
) {
	let request_id = ctx.next_remote_sqlite_request_id;
	ctx.next_remote_sqlite_request_id += 1;

	let entry = RemoteSqliteRequestEntry {
		request,
		response_tx,
		expected_session,
		sent_session: None,
		sent: false,
		timestamp: crate::time::Instant::now(),
	};

	ctx.remote_sqlite_requests.insert(request_id, entry);
	METRICS.remote_sqlite_requests_inflight.inc();

	send_single_remote_sqlite_request(ctx, request_id).await;
}

pub async fn handle_sqlite_get_pages_response(
	ctx: &mut EnvoyContext,
	response: protocol::ToEnvoySqliteGetPagesResponse,
) {
	handle_sqlite_response(
		ctx,
		response.request_id,
		SqliteResponse::GetPages(response.data),
		"sqlite_get_pages",
	);
}

pub async fn handle_sqlite_commit_response(
	ctx: &mut EnvoyContext,
	response: protocol::ToEnvoySqliteCommitResponse,
) {
	handle_sqlite_response(
		ctx,
		response.request_id,
		SqliteResponse::Commit(response.data),
		"sqlite_commit",
	);
}

pub async fn handle_remote_sqlite_exec_response(
	ctx: &mut EnvoyContext,
	response: protocol::ToEnvoySqliteExecResponse,
) {
	handle_remote_sqlite_response(
		ctx,
		response.request_id,
		RemoteSqliteResponse::Exec(response.data),
		"remote_sqlite_exec",
	);
}

pub async fn handle_remote_sqlite_execute_response(
	ctx: &mut EnvoyContext,
	response: protocol::ToEnvoySqliteExecuteResponse,
) {
	handle_remote_sqlite_response(
		ctx,
		response.request_id,
		RemoteSqliteResponse::Execute(response.data),
		"remote_sqlite_execute",
	);
}

pub async fn handle_remote_sqlite_execute_batch_response(
	ctx: &mut EnvoyContext,
	response: protocol::ToEnvoySqliteExecuteBatchResponse,
) {
	handle_remote_sqlite_response(
		ctx,
		response.request_id,
		RemoteSqliteResponse::ExecuteBatch(response.data),
		"remote_sqlite_execute_batch",
	);
}

fn handle_sqlite_response(
	ctx: &mut EnvoyContext,
	request_id: u32,
	response: SqliteResponse,
	op: &str,
) {
	let request = ctx.sqlite_requests.remove(&request_id);

	if let Some(request) = request {
		METRICS.sqlite_requests_inflight.dec();
		let _ = request.response_tx.send(Ok(response));
	} else {
		tracing::error!(
			request_id,
			op,
			"received sqlite response for unknown request id"
		);
	}
}

fn handle_remote_sqlite_response(
	ctx: &mut EnvoyContext,
	request_id: u32,
	response: RemoteSqliteResponse,
	op: &str,
) {
	let request = ctx.remote_sqlite_requests.remove(&request_id);

	if let Some(request) = request {
		METRICS.remote_sqlite_requests_inflight.dec();
		let session = request
			.sent_session
			.expect("a remote sqlite response must belong to a sent request");
		let _ = request
			.response_tx
			.send(Ok(RemoteSqliteResponseEnvelope { response, session }));
	} else {
		tracing::error!(
			request_id,
			op,
			"received remote sqlite response for unknown request id"
		);
	}
}

pub async fn send_single_sqlite_request(ctx: &mut EnvoyContext, request_id: u32) {
	let request = ctx.sqlite_requests.get_mut(&request_id);
	let Some(request) = request else { return };
	if request.sent {
		return;
	}

	let message =
		match request.request.clone() {
			SqliteRequest::GetPages(data) => protocol::ToRivet::ToRivetSqliteGetPagesRequest(
				protocol::ToRivetSqliteGetPagesRequest { request_id, data },
			),
			SqliteRequest::Commit(data) => protocol::ToRivet::ToRivetSqliteCommitRequest(
				protocol::ToRivetSqliteCommitRequest { request_id, data },
			),
		};

	ws_send(&ctx.shared, message).await;

	if let Some(request) = ctx.sqlite_requests.get_mut(&request_id) {
		request.sent = true;
		request.timestamp = crate::time::Instant::now();
	}
}

pub async fn send_single_remote_sqlite_request(ctx: &mut EnvoyContext, request_id: u32) {
	let request = ctx.remote_sqlite_requests.get_mut(&request_id);
	let Some(request) = request else { return };
	if request.sent {
		return;
	}

	let expected_session = request.expected_session;
	let message = remote_sqlite_request_to_message(request_id, request.request.clone());

	match ws_send_for_session(&ctx.shared, message, expected_session).await {
		WsSendResult::Sent { session } => {
			if let Some(request) = ctx.remote_sqlite_requests.get_mut(&request_id) {
				request.sent = true;
				request.sent_session = Some(session);
				request.timestamp = crate::time::Instant::now();
			}
		}
		WsSendResult::Unavailable if expected_session.is_none() => {
			// Ordinary requests that have never crossed the socket remain eligible
			// for replay. A transaction-affine request takes the error branch below.
		}
		WsSendResult::Unavailable => {
			fail_remote_sqlite_session_request(ctx, request_id, None);
		}
		WsSendResult::StaleSession { current } => {
			fail_remote_sqlite_session_request(ctx, request_id, current);
		}
	}
}

fn fail_remote_sqlite_session_request(
	ctx: &mut EnvoyContext,
	request_id: u32,
	current: Option<u64>,
) {
	let Some(request) = ctx.remote_sqlite_requests.remove(&request_id) else {
		return;
	};
	METRICS.remote_sqlite_requests_inflight.dec();
	let expected = request
		.expected_session
		.expect("only session-affine requests fail session admission");
	let _ = request.response_tx.send(Err(anyhow::anyhow!(
		RemoteSqliteConnectionSessionLostError { expected, current }
	)));
}

pub fn remote_sqlite_request_to_message(
	request_id: u32,
	request: RemoteSqliteRequest,
) -> protocol::ToRivet {
	match request {
		RemoteSqliteRequest::Exec(data) => {
			protocol::ToRivet::ToRivetSqliteExecRequest(protocol::ToRivetSqliteExecRequest {
				request_id,
				data,
			})
		}
		RemoteSqliteRequest::Execute(data) => {
			protocol::ToRivet::ToRivetSqliteExecuteRequest(protocol::ToRivetSqliteExecuteRequest {
				request_id,
				data,
			})
		}
		RemoteSqliteRequest::ExecuteBatch(data) => {
			protocol::ToRivet::ToRivetSqliteExecuteBatchRequest(
				protocol::ToRivetSqliteExecuteBatchRequest { request_id, data },
			)
		}
	}
}

pub async fn process_unsent_sqlite_requests(ctx: &mut EnvoyContext) {
	let ws_available = {
		let guard = ctx.shared.ws_tx.lock().await;
		guard.is_some()
	};

	if !ws_available {
		return;
	}

	let unsent: Vec<u32> = ctx
		.sqlite_requests
		.iter()
		.filter(|(_, req)| !req.sent)
		.map(|(id, _)| *id)
		.collect();

	for request_id in unsent {
		send_single_sqlite_request(ctx, request_id).await;
	}
}

pub async fn process_unsent_remote_sqlite_requests(ctx: &mut EnvoyContext) {
	let ws_available = {
		let guard = ctx.shared.ws_tx.lock().await;
		guard.is_some()
	};

	if !ws_available {
		return;
	}

	let unsent: Vec<u32> = ctx
		.remote_sqlite_requests
		.iter()
		.filter(|(_, req)| !req.sent)
		.map(|(id, _)| *id)
		.collect();

	for request_id in unsent {
		send_single_remote_sqlite_request(ctx, request_id).await;
	}
}

pub fn cleanup_old_sqlite_requests(ctx: &mut EnvoyContext) {
	let now = crate::time::Instant::now();
	let mut to_delete = Vec::new();

	for (request_id, request) in &ctx.sqlite_requests {
		if now.duration_since(request.timestamp).as_millis() > KV_EXPIRE_MS as u128 {
			to_delete.push(*request_id);
		}
	}

	for request_id in to_delete {
		if let Some(request) = ctx.sqlite_requests.remove(&request_id) {
			let kind = request.request.kind();
			let was_sent = if request.sent { "true" } else { "false" };
			METRICS
				.sqlite_request_expired_total
				.with_label_values(&[kind, was_sent])
				.inc();
			METRICS.sqlite_requests_inflight.dec();
			tracing::warn!(
				request_id,
				kind,
				was_sent = request.sent,
				age_ms = now.duration_since(request.timestamp).as_millis() as u64,
				"sqlite request expired by cleanup; if was_sent=true this indicates an abandoned in-flight request"
			);
			let _ = request
				.response_tx
				.send(Err(anyhow::anyhow!("sqlite request timed out")));
		}
	}
}

pub fn cleanup_old_remote_sqlite_requests(ctx: &mut EnvoyContext) {
	let now = crate::time::Instant::now();
	let mut to_delete = Vec::new();

	for (request_id, request) in &ctx.remote_sqlite_requests {
		if now.duration_since(request.timestamp).as_millis() > KV_EXPIRE_MS as u128 {
			to_delete.push(*request_id);
		}
	}

	for request_id in to_delete {
		if let Some(request) = ctx.remote_sqlite_requests.remove(&request_id) {
			let kind = request.request.kind();
			let was_sent = if request.sent { "true" } else { "false" };
			METRICS
				.sqlite_request_expired_total
				.with_label_values(&[kind, was_sent])
				.inc();
			METRICS.remote_sqlite_requests_inflight.dec();
			tracing::warn!(
				request_id,
				kind,
				was_sent = request.sent,
				age_ms = now.duration_since(request.timestamp).as_millis() as u64,
				"remote sqlite request expired by cleanup; if was_sent=true this indicates an abandoned in-flight request"
			);
			let _ = request
				.response_tx
				.send(Err(anyhow::anyhow!("remote sqlite request timed out")));
		}
	}
}

pub fn fail_sqlite_requests_with_shutdown(ctx: &mut EnvoyContext) {
	for (_id, request) in ctx.sqlite_requests.drain() {
		METRICS.sqlite_requests_inflight.dec();
		let _ = request
			.response_tx
			.send(Err(anyhow::anyhow!(EnvoyShutdownError)));
	}
}

pub fn fail_remote_sqlite_requests_with_shutdown(ctx: &mut EnvoyContext) {
	for (_id, request) in ctx.remote_sqlite_requests.drain() {
		METRICS.remote_sqlite_requests_inflight.dec();
		let _ = request
			.response_tx
			.send(Err(anyhow::anyhow!(EnvoyShutdownError)));
	}
}

pub fn fail_sent_remote_sqlite_requests_with_indeterminate_result(ctx: &mut EnvoyContext) {
	let request_ids: Vec<u32> = ctx
		.remote_sqlite_requests
		.iter()
		.filter(|(_, request)| request.sent)
		.map(|(request_id, _)| *request_id)
		.collect();

	for request_id in request_ids {
		if let Some(request) = ctx.remote_sqlite_requests.remove(&request_id) {
			METRICS.remote_sqlite_requests_inflight.dec();
			let operation = request.request.operation();
			tracing::warn!(
				request_id,
				operation,
				"remote sqlite response lost after websocket disconnect"
			);
			let _ = request.response_tx.send(Err(anyhow::anyhow!(
				RemoteSqliteIndeterminateResultError { operation }
			)));
		}
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::Arc;

	use vbare::OwnedVersionedData;

	use super::*;
	use crate::config::{
		BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
		WebSocketSender,
	};
	use crate::context::{SharedContext, WsTxMessage};
	use crate::handle::EnvoyHandle;
	use crate::utils::{BufferMap, RemoteSqliteIndeterminateResultError};

	struct IdleCallbacks;

	impl EnvoyCallbacks for IdleCallbacks {
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
			Box::pin(async { anyhow::bail!("fetch should not be called in sqlite tests") })
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
			Box::pin(async { anyhow::bail!("websocket should not be called in sqlite tests") })
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

	fn new_envoy_context() -> EnvoyContext {
		let (envoy_tx, _envoy_rx) = tokio::sync::mpsc::unbounded_channel();
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
				callbacks: Arc::new(IdleCallbacks),
			},
			envoy_key: "test-envoy".to_string(),
			envoy_tx,
			actors: Arc::new(std::sync::Mutex::new(HashMap::new())),
			actors_notify: Arc::new(tokio::sync::Notify::new()),
			live_tunnel_requests: Arc::new(std::sync::Mutex::new(HashMap::new())),
			pending_hibernation_restores: Arc::new(std::sync::Mutex::new(HashMap::new())),
			ws_tx: Arc::new(tokio::sync::Mutex::new(
				None::<tokio::sync::mpsc::UnboundedSender<WsTxMessage>>,
			)),
			http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
			connection_session: std::sync::atomic::AtomicU64::new(0),
			next_connection_session: std::sync::atomic::AtomicU64::new(0),
			connection_session_tx: tokio::sync::watch::channel(0).0,
			protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
			shutting_down: std::sync::atomic::AtomicBool::new(false),
			last_ping_ts: std::sync::atomic::AtomicI64::new(0),
			stopped_tx: tokio::sync::watch::channel(true).0,
		});

		EnvoyContext {
			shared,
			shutting_down: false,
			actors: HashMap::new(),
			buffered_actor_messages: HashMap::new(),
			kv_requests: HashMap::new(),
			next_kv_request_id: 0,
			sqlite_requests: HashMap::new(),
			next_sqlite_request_id: 0,
			remote_sqlite_requests: HashMap::new(),
			next_remote_sqlite_request_id: 0,
			request_to_actor: BufferMap::new(),
			http_request_routes: BufferMap::new(),
			http_message_indices: BufferMap::new(),
			http_request_cancellations: HashMap::new(),
			buffered_messages: Vec::new(),
			processed_command_idx: HashMap::new(),
		}
	}

	fn exec_request() -> protocol::SqliteExecRequest {
		protocol::SqliteExecRequest {
			namespace_id: "ns".to_string(),
			actor_id: "actor".to_string(),
			generation: 1,
			sql: "select 1".to_string(),
		}
	}

	fn execute_request() -> protocol::SqliteExecuteRequest {
		protocol::SqliteExecuteRequest {
			namespace_id: "ns".to_string(),
			actor_id: "actor".to_string(),
			generation: 1,
			sql: "select ?".to_string(),
			params: Some(vec![protocol::SqliteBindParam::SqliteValueInteger(
				protocol::SqliteValueInteger { value: 1 },
			)]),
		}
	}

	fn execute_batch_request() -> protocol::SqliteExecuteBatchRequest {
		protocol::SqliteExecuteBatchRequest {
			namespace_id: "ns".to_string(),
			actor_id: "actor".to_string(),
			generation: 1,
			statements: vec![
				protocol::SqliteBatchStatement {
					sql: "insert into t values (?)".to_string(),
					params: Some(vec![protocol::SqliteBindParam::SqliteValueInteger(
						protocol::SqliteValueInteger { value: 1 },
					)]),
				},
				protocol::SqliteBatchStatement {
					sql: "insert into t values (?)".to_string(),
					params: Some(vec![protocol::SqliteBindParam::SqliteValueInteger(
						protocol::SqliteValueInteger { value: 2 },
					)]),
				},
			],
		}
	}

	#[tokio::test]
	async fn remote_sqlite_exec_response_matches_pending_request() {
		let mut ctx = new_envoy_context();
		let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
		let session = crate::connection::install_connection(&ctx.shared, ws_tx).await;
		let (tx, rx) = oneshot::channel();

		handle_remote_sqlite_request(
			&mut ctx,
			RemoteSqliteRequest::Exec(exec_request()),
			None,
			tx,
		)
		.await;
		assert!(matches!(ws_rx.recv().await, Some(WsTxMessage::Send(_))));
		assert!(ctx.remote_sqlite_requests.contains_key(&0));

		handle_remote_sqlite_exec_response(
			&mut ctx,
			protocol::ToEnvoySqliteExecResponse {
				request_id: 0,
				data: protocol::SqliteExecResponse::SqliteExecOk(protocol::SqliteExecOk {
					result: protocol::SqliteQueryResult {
						columns: vec!["one".to_string()],
						rows: vec![vec![protocol::SqliteColumnValue::SqliteValueInteger(
							protocol::SqliteValueInteger { value: 1 },
						)]],
					},
				}),
			},
		)
		.await;

		let response = rx
			.await
			.expect("response sender should complete")
			.expect("response should succeed");
		assert_eq!(response.session, session);
		match response.response {
			RemoteSqliteResponse::Exec(protocol::SqliteExecResponse::SqliteExecOk(ok)) => {
				assert_eq!(ok.result.columns, vec!["one"]);
				assert_eq!(ok.result.rows.len(), 1);
			}
			_ => panic!("unexpected response"),
		}
		assert!(ctx.remote_sqlite_requests.is_empty());
	}

	#[tokio::test]
	async fn remote_sqlite_batch_uses_one_websocket_request() {
		let mut ctx = new_envoy_context();
		let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
		crate::connection::install_connection(&ctx.shared, ws_tx).await;
		let (tx, _rx) = oneshot::channel();

		handle_remote_sqlite_request(
			&mut ctx,
			RemoteSqliteRequest::ExecuteBatch(execute_batch_request()),
			None,
			tx,
		)
		.await;

		assert!(matches!(ws_rx.recv().await, Some(WsTxMessage::Send(_))));
		assert!(
			ws_rx.try_recv().is_err(),
			"a batch must serialize as one WebSocket message"
		);
		assert_eq!(ctx.remote_sqlite_requests.len(), 1);
	}

	#[test]
	fn remote_sqlite_requests_reject_protocol_v3_serialization() {
		let requests = vec![
			RemoteSqliteRequest::Exec(exec_request()),
			RemoteSqliteRequest::Execute(execute_request()),
		];

		for request in requests {
			let message = remote_sqlite_request_to_message(7, request);
			let err = protocol::versioned::ToRivet::wrap_latest(message)
				.serialize(3)
				.expect_err("remote sqlite requests should require protocol v4");
			let compatibility = err
				.downcast_ref::<protocol::versioned::ProtocolCompatibilityError>()
				.expect("error should be a protocol compatibility error");
			assert_eq!(
				compatibility.feature,
				protocol::versioned::ProtocolCompatibilityFeature::RemoteSqliteExecution
			);
			assert_eq!(compatibility.required_version, 4);
			assert_eq!(compatibility.target_version, 3);
		}
	}

	#[tokio::test]
	async fn remote_sqlite_shutdown_cleanup_fails_pending_requests() {
		let mut ctx = new_envoy_context();
		let (tx, rx) = oneshot::channel();

		handle_remote_sqlite_request(
			&mut ctx,
			RemoteSqliteRequest::Execute(execute_request()),
			None,
			tx,
		)
		.await;
		fail_remote_sqlite_requests_with_shutdown(&mut ctx);

		let err = rx
			.await
			.expect("response sender should complete")
			.expect_err("pending request should fail during shutdown");
		assert!(err.downcast_ref::<EnvoyShutdownError>().is_some());
		assert!(ctx.remote_sqlite_requests.is_empty());
	}

	#[tokio::test]
	async fn sent_remote_sqlite_request_fails_indeterminate_on_disconnect() {
		let mut ctx = new_envoy_context();
		let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
		crate::connection::install_connection(&ctx.shared, ws_tx).await;
		let (tx, rx) = oneshot::channel();

		handle_remote_sqlite_request(
			&mut ctx,
			RemoteSqliteRequest::Execute(execute_request()),
			None,
			tx,
		)
		.await;
		assert!(matches!(ws_rx.recv().await, Some(WsTxMessage::Send(_))));
		assert!(
			ctx.remote_sqlite_requests
				.get(&0)
				.expect("request should be pending")
				.sent
		);

		fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx);

		let err = rx
			.await
			.expect("response sender should complete")
			.expect_err("sent write should fail indeterminate on disconnect");
		let indeterminate = err
			.downcast_ref::<RemoteSqliteIndeterminateResultError>()
			.expect("error should describe indeterminate remote sqlite result");
		assert_eq!(indeterminate.operation, "execute");
		assert!(ctx.remote_sqlite_requests.is_empty());
	}

	#[tokio::test]
	async fn unsent_remote_sqlite_request_survives_disconnect_and_sends_on_reconnect() {
		let mut ctx = new_envoy_context();
		let (tx, mut rx) = oneshot::channel();

		handle_remote_sqlite_request(
			&mut ctx,
			RemoteSqliteRequest::Execute(execute_request()),
			None,
			tx,
		)
		.await;
		assert!(
			!ctx.remote_sqlite_requests
				.get(&0)
				.expect("request should be pending")
				.sent
		);

		fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx);
		assert!(matches!(
			rx.try_recv(),
			Err(tokio::sync::oneshot::error::TryRecvError::Empty)
		));
		assert!(ctx.remote_sqlite_requests.contains_key(&0));

		let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
		crate::connection::install_connection(&ctx.shared, ws_tx).await;
		process_unsent_remote_sqlite_requests(&mut ctx).await;

		assert!(matches!(ws_rx.recv().await, Some(WsTxMessage::Send(_))));
		assert!(
			ctx.remote_sqlite_requests
				.get(&0)
				.expect("request should still be pending")
				.sent
		);
	}

	#[tokio::test]
	async fn transaction_request_never_crosses_connection_sessions() {
		let mut ctx = new_envoy_context();
		let (first_tx, _first_rx) = tokio::sync::mpsc::unbounded_channel();
		let first_session = crate::connection::install_connection(&ctx.shared, first_tx).await;
		crate::connection::remove_connection(&ctx.shared).await;
		let (second_tx, mut second_rx) = tokio::sync::mpsc::unbounded_channel();
		let second_session = crate::connection::install_connection(&ctx.shared, second_tx).await;
		assert_ne!(first_session, second_session);

		let (response_tx, response_rx) = oneshot::channel();
		handle_remote_sqlite_request(
			&mut ctx,
			RemoteSqliteRequest::Execute(execute_request()),
			Some(first_session),
			response_tx,
		)
		.await;

		let error = response_rx
			.await
			.expect("response sender should complete")
			.expect_err("stale transaction session must fail before send");
		let lost = error
			.downcast_ref::<RemoteSqliteConnectionSessionLostError>()
			.expect("error should identify the stale session");
		assert_eq!(lost.expected, first_session);
		assert_eq!(lost.current, Some(second_session));
		assert!(second_rx.try_recv().is_err(), "stale request was sent");
		assert!(ctx.remote_sqlite_requests.is_empty());
	}
}
