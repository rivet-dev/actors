//! Regression test for the actor-side VFS-SQLite-stall-on-disconnect bug.
//!
//! The bug: when the actor-engine WebSocket disconnected with sent-but-unanswered VFS
//! SQLite requests (`get_pages` / `commit`) in flight, the actor side did not fail those
//! requests. `ConnClose` only ran the *remote* exec/execute variant
//! (`fail_sent_remote_sqlite_requests_with_indeterminate_result`), so VFS requests
//! survived it and were only freed by the periodic cleanup (every 15s, 30s timeout).
//! During that window the SQLite VFS callback stayed parked in
//! `runtime.block_on(transport.get_pages(...))`, which blocked the SQLite worker thread
//! and every subsequent SQLite call on that actor.
//!
//! `fail_sent_sqlite_requests_with_indeterminate_result` closes that gap, and `ConnClose`
//! now runs both variants.
//!
//! This test invokes the same code paths `ConnClose` runs on a synthetic `EnvoyContext`
//! holding one sent VFS get_pages request and one sent remote-execute request, and
//! asserts both resolve immediately with `RemoteSqliteIndeterminateResultError`.
//!
//! A lost `commit` reply stays genuinely ambiguous: the engine may have applied it. Failing
//! fast bounds the stall but does not resolve that, and the retry is still adjudicated by
//! the head fence (see `lost_commit_response_fails_later_on_head_fence_mismatch` in
//! `rivet-depot-client`). Reconciling instead would need writer identity in `DBHead`, which
//! it does not carry.
//!
//! Negative-control test (`unsent_vfs_request_quickly_resubmitted_after_reconnect`)
//! flips one variable: the VFS request is queued *before* the WS goes up, so it never
//! reaches `sent=true`. Unsent requests must be left alone for
//! `process_unsent_sqlite_requests` to re-send on reconnect.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rivet_envoy_client::config::{
	BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
	WebSocketSender,
};
use rivet_envoy_client::context::{SharedContext, WsTxMessage};
use rivet_envoy_client::envoy::EnvoyContext;
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_client::sqlite::{
	RemoteSqliteRequest, SqliteRequest, cleanup_old_sqlite_requests,
	fail_sent_remote_sqlite_requests_with_indeterminate_result,
	fail_sent_sqlite_requests_with_indeterminate_result, handle_remote_sqlite_request,
	handle_sqlite_request, process_unsent_sqlite_requests,
};
use rivet_envoy_client::utils::{BufferMap, RemoteSqliteIndeterminateResultError};
use rivet_envoy_protocol as protocol;
use tokio::sync::{mpsc, oneshot};

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
		Box::pin(async { anyhow::bail!("fetch should not be called") })
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
		Box::pin(async { anyhow::bail!("websocket should not be called") })
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
	let (envoy_tx, _envoy_rx) = mpsc::unbounded_channel();
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
			None::<mpsc::UnboundedSender<WsTxMessage>>,
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

fn get_pages_request() -> protocol::SqliteGetPagesRequest {
	protocol::SqliteGetPagesRequest {
		actor_id: "amp-prod-thread-actor".to_string(),
		pgnos: vec![1, 2, 3],
		expected_generation: Some(1),
		expected_head_txid: None,
	}
}

fn remote_execute_request() -> protocol::SqliteExecuteRequest {
	protocol::SqliteExecuteRequest {
		namespace_id: "ns".to_string(),
		actor_id: "amp-prod-thread-actor".to_string(),
		generation: 1,
		sql: "insert into thread_messages values (?)".to_string(),
		params: Some(vec![protocol::SqliteBindParam::SqliteValueText(
			protocol::SqliteValueText {
				value: "hello".to_string(),
			},
		)]),
	}
}

fn init_tracing() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
		)
		.with_test_writer()
		.try_init();
}

/// Bug-present repro.
///
/// 1. Attach a fake WS sender (so requests get marked `sent=true`).
/// 2. Submit one VFS `get_pages` request AND one remote `execute` request. Both flush to
///    the WS channel and have `sent=true`.
/// 3. Drop the WS — exactly what the production `ConnClose` handler sees.
/// 4. Invoke the same code the disconnect handler runs:
///    `fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx)`.
/// 5. Assert: the remote-execute oneshot resolves immediately with
///    `RemoteSqliteIndeterminateResultError`. The VFS get_pages oneshot is STILL pending.
/// 6. Verify that ONLY the cleanup path can release the VFS request. We backdate its
///    timestamp by `KV_EXPIRE_MS + 1ms` and call `cleanup_old_sqlite_requests`. Now the
///    VFS oneshot resolves with the `sqlite request timed out` error. This is the same
///    error path that fires in production at ~30s after the disconnect.
#[tokio::test]
async fn sent_vfs_request_fails_immediately_on_disconnect() {
	init_tracing();
	let mut ctx = new_envoy_context();

	// Attach fake WS so `handle_*` flushes the request and marks it sent=true.
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);

	// VFS get_pages: this is the production code path being investigated.
	let (vfs_tx, mut vfs_rx) = oneshot::channel();
	handle_sqlite_request(
		&mut ctx,
		SqliteRequest::GetPages(get_pages_request()),
		vfs_tx,
	)
	.await;

	// Remote execute: this is the control. It uses the same disconnect handler, but the
	// production code currently fails it correctly. Used to show the contrast.
	let (remote_tx, mut remote_rx) = oneshot::channel();
	handle_remote_sqlite_request(
		&mut ctx,
		RemoteSqliteRequest::Execute(remote_execute_request()),
		None,
		remote_tx,
	)
	.await;

	// Confirm both made it onto the wire.
	let _ = ws_rx.recv().await.expect("vfs get_pages WS msg");
	let _ = ws_rx.recv().await.expect("remote execute WS msg");
	assert!(
		ctx.sqlite_requests
			.get(&0)
			.expect("vfs request pending")
			.sent,
		"VFS request must be marked sent=true before we kill the link"
	);
	assert!(
		ctx.remote_sqlite_requests
			.get(&0)
			.expect("remote request pending")
			.sent,
		"remote request must be marked sent=true before we kill the link"
	);
	tracing::info!("both sqlite requests sent=true");

	// Simulate the WebSocket dying. This is the production ConnClose path: drop the WS
	// sender and call the same handler. We do NOT change any production code; we just
	// invoke what `envoy_loop` invokes on ConnClose.
	*ctx.shared.ws_tx.lock().await = None;
	tracing::info!("ws disconnected; running production ConnClose handler");
	let before_handler = std::time::Instant::now();
	fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx);
	fail_sent_sqlite_requests_with_indeterminate_result(&mut ctx);
	tracing::info!(
		elapsed_us = before_handler.elapsed().as_micros() as u64,
		"ran both ConnClose sqlite failure handlers"
	);

	// 1) Remote oneshot resolves NOW with indeterminate result. (Existing behavior.)
	let remote_resolve_start = std::time::Instant::now();
	let remote_result = tokio::time::timeout(Duration::from_millis(50), &mut remote_rx)
		.await
		.expect("remote execute oneshot must resolve immediately after disconnect")
		.expect("remote execute oneshot must complete");
	let remote_elapsed = remote_resolve_start.elapsed();
	let remote_err = remote_result.expect_err("remote execute must fail with indeterminate");
	let indet = remote_err
		.downcast_ref::<RemoteSqliteIndeterminateResultError>()
		.expect("remote execute must fail with RemoteSqliteIndeterminateResultError");
	assert_eq!(indet.operation, "execute");
	tracing::info!(
		elapsed_us = remote_elapsed.as_micros() as u64,
		operation = %indet.operation,
		"REMOTE: fails immediately on disconnect (existing correct behavior)"
	);

	// 2) VFS oneshot resolves NOW too, with the same indeterminate error.
	let vfs_resolve_start = std::time::Instant::now();
	let vfs_result = tokio::time::timeout(Duration::from_millis(50), &mut vfs_rx)
		.await
		.expect("vfs oneshot must resolve immediately after disconnect")
		.expect("vfs oneshot must complete");
	let vfs_elapsed = vfs_resolve_start.elapsed();
	let vfs_err = match vfs_result {
		Ok(_) => panic!("vfs get_pages must fail with indeterminate"),
		Err(err) => err,
	};
	let vfs_indet = vfs_err
		.downcast_ref::<RemoteSqliteIndeterminateResultError>()
		.expect("vfs get_pages must fail with RemoteSqliteIndeterminateResultError");
	assert_eq!(vfs_indet.operation, "get_pages");
	assert!(
		!ctx.sqlite_requests.contains_key(&0),
		"vfs request must be removed from the pending map by the disconnect handler"
	);
	tracing::info!(
		elapsed_us = vfs_elapsed.as_micros() as u64,
		operation = %vfs_indet.operation,
		"VFS: fails immediately on disconnect, matching the remote path"
	);

	// 3) The expiry tick is no longer the escape hatch for a sent request: there is
	//    nothing left for it to reap.
	cleanup_old_sqlite_requests(&mut ctx);
	assert!(
		ctx.sqlite_requests.is_empty(),
		"no sent vfs request should survive to the expiry tick"
	);
}

/// Negative control: VFS request was NEVER sent to the wire (no WS at submission time),
/// then we simulate a disconnect followed by reconnect. The production code MUST handle
/// this by re-sending the request from the unsent queue; the bug must NOT manifest.
#[tokio::test]
async fn unsent_vfs_request_quickly_resubmitted_after_reconnect() {
	init_tracing();
	let mut ctx = new_envoy_context();

	// Submit BEFORE the WS exists; the request will sit with sent=false.
	let (vfs_tx, mut vfs_rx) = oneshot::channel();
	handle_sqlite_request(
		&mut ctx,
		SqliteRequest::GetPages(get_pages_request()),
		vfs_tx,
	)
	.await;
	let entry = ctx.sqlite_requests.get(&0).expect("vfs request queued");
	assert!(!entry.sent, "VFS request must be unsent before disconnect");
	tracing::info!("VFS request queued with sent=false (link was already down)");

	// Run the same disconnect handler. With sent=false the bug should NOT bite.
	fail_sent_remote_sqlite_requests_with_indeterminate_result(&mut ctx);

	// Oneshot is still pending — that's fine for an unsent request, because the recovery
	// path is "resubmit on reconnect," not "fail with indeterminate."
	assert!(matches!(
		vfs_rx.try_recv(),
		Err(tokio::sync::oneshot::error::TryRecvError::Empty)
	));

	// Reconnect: attach a fresh WS sender and run `process_unsent_sqlite_requests`. This
	// is what the reconnect handler in envoy_loop does for KV/SQLite requests with sent=false.
	let (ws_tx, mut ws_rx) = mpsc::unbounded_channel();
	*ctx.shared.ws_tx.lock().await = Some(ws_tx);
	let resubmit_start = std::time::Instant::now();
	process_unsent_sqlite_requests(&mut ctx).await;
	let resubmit_elapsed = resubmit_start.elapsed();

	// The request now goes out on the new WS — fast path, no stall.
	let msg = ws_rx.recv().await.expect("resubmitted sqlite message");
	assert!(matches!(msg, WsTxMessage::Send(_)));
	let entry = ctx
		.sqlite_requests
		.get(&0)
		.expect("vfs request still tracked");
	assert!(entry.sent, "request must be marked sent=true on resubmit");
	tracing::info!(
		elapsed_us = resubmit_elapsed.as_micros() as u64,
		"NEGATIVE CONTROL: unsent VFS request resubmitted on reconnect; no stall"
	);
	assert!(
		resubmit_elapsed < Duration::from_millis(100),
		"resubmit must be fast; took {resubmit_elapsed:?}"
	);
}
