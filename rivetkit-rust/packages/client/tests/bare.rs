use std::{
	collections::HashMap,
	net::SocketAddr,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	time::Duration,
};

use axum::{
	body::Bytes,
	extract::{
		ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
		Path, State,
	},
	http::{header, HeaderMap, Method as AxumMethod, StatusCode, Uri},
	response::IntoResponse,
	routing::{any, get, post, put},
	Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{
	header::{HeaderMap as ReqwestHeaderMap, HeaderValue},
	Method, Url,
};
use rivetkit_client::{
	Client, ClientConfig, ConnectionStatus, CreateOptions, EncodingKind, GetOptions,
	GetOrCreateOptions, QueueSendStatus, SendAndWaitOpts, SendOpts,
};
use rivetkit_client_protocol as wire;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tokio::{
	net::TcpListener,
	sync::{mpsc, Notify},
	time::timeout,
};
use vbare::OwnedVersionedData;

#[derive(Clone)]
struct TestState {
	saw_bare_action: Arc<AtomicBool>,
	saw_bare_queue: Arc<AtomicBool>,
	saw_raw_fetch: Arc<AtomicBool>,
	saw_raw_websocket: Arc<AtomicBool>,
}

#[derive(Clone)]
struct ConnectionTestState {
	release_init: Arc<Notify>,
}

#[derive(Clone)]
struct OnceEventTestState {
	release_init: Arc<Notify>,
	unsubscribe_seen: Arc<Notify>,
}

#[derive(Clone)]
struct ConfigHeaderTestState {
	saw_actor_lookup: Arc<AtomicBool>,
	saw_action: Arc<AtomicBool>,
	saw_connection_websocket: Arc<AtomicBool>,
	saw_raw_websocket: Arc<AtomicBool>,
}

#[derive(Clone)]
struct MetadataLookupState {
	saw_metadata: Arc<AtomicBool>,
	target_endpoint: String,
}

#[derive(Clone)]
struct DisableMetadataState {
	saw_metadata: Arc<AtomicBool>,
}

#[derive(Deserialize)]
struct ActorRequest {
	name: String,
	key: String,
}

#[derive(Serialize)]
struct Actor {
	actor_id: &'static str,
	name: String,
	key: String,
}

#[derive(Serialize)]
struct ActorResponse {
	actor: Actor,
	created: bool,
}

#[derive(Deserialize)]
struct RunnerSelectorRequest {
	name: String,
	key: String,
	runner_name_selector: String,
}

#[derive(Clone, Default)]
struct RunnerSelectorState {
	seen: Arc<std::sync::Mutex<Vec<String>>>,
}

impl RunnerSelectorState {
	fn take(&self) -> Vec<String> {
		std::mem::take(&mut self.seen.lock().unwrap())
	}
}

async fn capture_runner_selector(
	State(state): State<RunnerSelectorState>,
	Json(request): Json<RunnerSelectorRequest>,
) -> impl IntoResponse {
	state
		.seen
		.lock()
		.unwrap()
		.push(request.runner_name_selector);
	Json(ActorResponse {
		actor: Actor {
			actor_id: "actor-1",
			name: request.name,
			key: request.key,
		},
		created: true,
	})
}

#[tokio::test]
async fn default_bare_action_round_trips_against_test_actor() {
	assert_eq!(EncodingKind::default(), EncodingKind::Bare);

	let state = TestState {
		saw_bare_action: Arc::new(AtomicBool::new(false)),
		saw_bare_queue: Arc::new(AtomicBool::new(false)),
		saw_raw_fetch: Arc::new(AtomicBool::new(false)),
		saw_raw_websocket: Arc::new(AtomicBool::new(false)),
	};
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/action/{action}", post(action))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let counter = client
		.get_or_create(
			"counter",
			vec!["bare-smoke".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();

	let output = counter.action("increment", vec![json!(2)]).await.unwrap();

	assert_eq!(output, json!({ "count": 3 }));
	assert!(state.saw_bare_action.load(Ordering::SeqCst));

	server.abort();
}

#[tokio::test]
async fn default_bare_queue_send_round_trips_against_test_actor() {
	let state = TestState {
		saw_bare_action: Arc::new(AtomicBool::new(false)),
		saw_bare_queue: Arc::new(AtomicBool::new(false)),
		saw_raw_fetch: Arc::new(AtomicBool::new(false)),
		saw_raw_websocket: Arc::new(AtomicBool::new(false)),
	};
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/queue/{queue}", post(queue_send))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let counter = client
		.get_or_create(
			"counter",
			vec!["bare-queue".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();

	counter
		.send("jobs", json!({ "id": 1 }), SendOpts::default())
		.await
		.unwrap();
	let output = counter
		.send_and_wait(
			"jobs",
			json!({ "id": 2 }),
			SendAndWaitOpts {
				timeout: Some(Duration::from_millis(50)),
			},
		)
		.await
		.unwrap();

	assert_eq!(output.status, QueueSendStatus::Completed);
	assert_eq!(output.response, Some(json!({ "accepted": "jobs" })));
	assert!(state.saw_bare_queue.load(Ordering::SeqCst));

	server.abort();
}

#[tokio::test]
async fn raw_fetch_posts_to_actor_request_endpoint() {
	let state = TestState {
		saw_bare_action: Arc::new(AtomicBool::new(false)),
		saw_bare_queue: Arc::new(AtomicBool::new(false)),
		saw_raw_fetch: Arc::new(AtomicBool::new(false)),
		saw_raw_websocket: Arc::new(AtomicBool::new(false)),
	};
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/request/{*path}", any(raw_fetch))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let actor = client
		.get_or_create(
			"counter",
			vec!["raw-fetch".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();
	let mut headers = ReqwestHeaderMap::new();
	headers.insert("x-test-header", HeaderValue::from_static("raw"));

	let response = actor
		.fetch(
			"api/echo?source=rust",
			Method::POST,
			headers,
			Some(Bytes::from_static(b"hello raw")),
		)
		.await
		.unwrap();

	assert_eq!(response.status(), StatusCode::CREATED);
	assert_eq!(response.text().await.unwrap(), "POST:hello raw");
	assert!(state.saw_raw_fetch.load(Ordering::SeqCst));

	server.abort();
}

#[tokio::test]
async fn raw_web_socket_round_trips_against_test_actor() {
	let state = TestState {
		saw_bare_action: Arc::new(AtomicBool::new(false)),
		saw_bare_queue: Arc::new(AtomicBool::new(false)),
		saw_raw_fetch: Arc::new(AtomicBool::new(false)),
		saw_raw_websocket: Arc::new(AtomicBool::new(false)),
	};
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/websocket/{*path}", any(raw_websocket))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let actor = client
		.get_or_create(
			"counter",
			vec!["raw-websocket".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();

	let mut ws = actor
		.web_socket("ws?source=rust", Some(vec!["raw.test".to_owned()]))
		.await
		.unwrap();
	ws.send(tokio_tungstenite::tungstenite::Message::Text(
		"hello".into(),
	))
	.await
	.unwrap();

	let message = ws.next().await.unwrap().unwrap();
	assert_eq!(
		message,
		tokio_tungstenite::tungstenite::Message::Text("raw:hello".into())
	);
	assert!(state.saw_raw_websocket.load(Ordering::SeqCst));

	server.abort();
}

#[tokio::test]
async fn connection_lifecycle_callbacks_fire_and_status_watch_updates() {
	let release_init = Arc::new(Notify::new());
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/connect", any(connection_websocket))
		.with_state(ConnectionTestState {
			release_init: release_init.clone(),
		});

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let actor = client
		.get_or_create(
			"counter",
			vec!["connection-lifecycle".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();
	let conn = actor.connect();

	let mut connected_status_rx = conn.status_receiver();
	let connected_status = tokio::spawn(async move {
		wait_for_status_watch(&mut connected_status_rx, ConnectionStatus::Connected).await;
	});
	let (open_tx, mut open_rx) = mpsc::unbounded_channel();
	let (close_tx, mut close_rx) = mpsc::unbounded_channel();
	let (error_tx, mut error_rx) = mpsc::unbounded_channel();
	let (status_tx, mut status_events) = mpsc::unbounded_channel();

	conn.on_open(move || {
		open_tx.send(()).ok();
	})
	.await;
	conn.on_close(move || {
		close_tx.send(()).ok();
	})
	.await;
	conn.on_error(move |message| {
		error_tx.send(message.to_owned()).ok();
	})
	.await;
	conn.on_status_change(move |status| {
		status_tx.send(status).ok();
	})
	.await;

	release_init.notify_one();

	connected_status.await.unwrap();
	assert_eq!(
		timeout(Duration::from_secs(2), open_rx.recv())
			.await
			.unwrap(),
		Some(())
	);
	assert_eq!(
		timeout(Duration::from_secs(2), error_rx.recv())
			.await
			.unwrap(),
		Some("server-side lifecycle error".to_owned())
	);
	let mut final_status_rx = conn.status_receiver();
	timeout(Duration::from_secs(2), conn.disconnect())
		.await
		.unwrap();
	wait_for_status_watch(&mut final_status_rx, ConnectionStatus::Disconnected).await;
	assert_eq!(conn.conn_status(), ConnectionStatus::Disconnected);
	assert_eq!(
		timeout(Duration::from_secs(2), close_rx.recv())
			.await
			.unwrap(),
		Some(())
	);
	wait_for_status_event(&mut status_events, ConnectionStatus::Connected).await;
	wait_for_status_event(&mut status_events, ConnectionStatus::Disconnected).await;
	server.abort();
}

#[tokio::test]
async fn once_event_callback_fires_once_and_unsubscribes() {
	let release_init = Arc::new(Notify::new());
	let unsubscribe_seen = Arc::new(Notify::new());
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route(
			"/gateway/{actor_id}/connect",
			any(connection_once_event_websocket),
		)
		.with_state(OnceEventTestState {
			release_init: release_init.clone(),
			unsubscribe_seen: unsubscribe_seen.clone(),
		});

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let actor = client
		.get_or_create(
			"counter",
			vec!["once-event".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();
	let conn = actor.connect();
	let (event_tx, mut event_rx) = mpsc::unbounded_channel();

	let _subscription = conn
		.once_event("tick", move |event| {
			event_tx.send(event).ok();
		})
		.await;

	release_init.notify_one();

	let event = timeout(Duration::from_secs(2), event_rx.recv())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(event.name, "tick");
	assert_eq!(event.args, vec![json!(1)]);
	if let Ok(Some(event)) = timeout(Duration::from_millis(200), event_rx.recv()).await {
		panic!("once_event callback fired more than once: {event:?}");
	}
	timeout(Duration::from_secs(2), unsubscribe_seen.notified())
		.await
		.unwrap();

	conn.disconnect().await;
	server.abort();
}

#[tokio::test]
async fn config_headers_are_sent_on_http_and_websocket_paths() {
	let state = ConfigHeaderTestState {
		saw_actor_lookup: Arc::new(AtomicBool::new(false)),
		saw_action: Arc::new(AtomicBool::new(false)),
		saw_connection_websocket: Arc::new(AtomicBool::new(false)),
		saw_raw_websocket: Arc::new(AtomicBool::new(false)),
	};
	let app = Router::new()
		.route("/actors", put(get_or_create_actor_with_config_header))
		.route(
			"/gateway/{actor_id}/action/{action}",
			post(action_with_config_header),
		)
		.route(
			"/gateway/{actor_id}/connect",
			any(connection_websocket_with_config_header),
		)
		.route(
			"/gateway/{actor_id}/websocket/{*path}",
			any(raw_websocket_with_config_header),
		)
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = Client::new(
		ClientConfig::new(endpoint(addr))
			.disable_metadata_lookup(true)
			.header("x-config-header", "from-config"),
	);
	let actor = client
		.get_or_create(
			"counter",
			vec!["config-headers".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();

	let output = actor.action("increment", vec![json!(2)]).await.unwrap();
	assert_eq!(output, json!({ "count": 3 }));

	let conn = actor.connect();
	let mut status_rx = conn.status_receiver();
	wait_for_status_watch(&mut status_rx, ConnectionStatus::Connected).await;
	conn.disconnect().await;

	let mut raw_ws = actor
		.web_socket("ws", Some(vec!["raw.test".to_owned()]))
		.await
		.unwrap();
	raw_ws.close(None).await.unwrap();

	assert!(state.saw_actor_lookup.load(Ordering::SeqCst));
	assert!(state.saw_action.load(Ordering::SeqCst));
	assert!(state.saw_connection_websocket.load(Ordering::SeqCst));
	assert!(state.saw_raw_websocket.load(Ordering::SeqCst));

	server.abort();
}

#[tokio::test]
async fn max_input_size_checks_raw_query_input_before_base64url_encoding() {
	let client = Client::new(
		ClientConfig::new("http://127.0.0.1:6420")
			.disable_metadata_lookup(true)
			.max_input_size(1),
	);
	let actor = client
		.get_or_create(
			"counter",
			vec!["too-large".to_owned()],
			GetOrCreateOptions {
				create_with_input: Some(json!({ "payload": "larger than one byte" })),
				..Default::default()
			},
		)
		.unwrap();

	let error = actor.gateway_url().unwrap_err().to_string();
	assert!(
		error.contains("actor query input exceeds max_input_size"),
		"{error}"
	);
}

#[test]
fn gateway_url_uses_direct_actor_id_target() {
	let client = Client::new(
		ClientConfig::new("http://127.0.0.1:6420/")
			.token("dev token")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get_for_id("counter", "actor/1", GetOptions::default())
		.unwrap();

	assert_eq!(
		actor.gateway_url().unwrap(),
		"http://127.0.0.1:6420/gateway/actor%2F1@dev%20token"
	);
}

#[test]
fn gateway_url_uses_query_backed_get_target() {
	let client = Client::new(
		ClientConfig::new("http://127.0.0.1:6420")
			.namespace("ns")
			.token("dev-token")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get(
			"counter",
			vec!["tenant".to_owned(), "room 1".to_owned()],
			GetOptions::default(),
		)
		.unwrap();

	let url = Url::parse(&actor.gateway_url().unwrap()).unwrap();
	assert_eq!(url.path(), "/gateway/counter");
	let params = query_params(&url);
	assert_eq!(params.get("rvt-namespace").map(String::as_str), Some("ns"));
	assert_eq!(params.get("rvt-method").map(String::as_str), Some("get"));
	assert_eq!(
		params.get("rvt-key").map(String::as_str),
		Some("tenant,room 1")
	);
	assert_eq!(
		params.get("rvt-token").map(String::as_str),
		Some("dev-token")
	);
	assert!(!params.contains_key("rvt-runner"));
	assert!(!params.contains_key("rvt-crash-policy"));
	assert!(!params.contains_key("rvt-input"));
}

#[test]
fn gateway_url_uses_query_backed_get_or_create_target() {
	let client = Client::new(
		ClientConfig::new("http://127.0.0.1:6420")
			.namespace("ns")
			.pool_name("runner-a")
			.token("dev-token")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get_or_create(
			"chat room",
			vec!["tenant".to_owned(), "room 1".to_owned()],
			GetOrCreateOptions {
				create_in_region: Some("ams".to_owned()),
				create_with_input: Some(json!({ "seed": 1 })),
				..Default::default()
			},
		)
		.unwrap();

	let url = Url::parse(&actor.gateway_url().unwrap()).unwrap();
	assert_eq!(url.path(), "/gateway/chat%20room");
	let params = query_params(&url);
	assert_eq!(params.get("rvt-namespace").map(String::as_str), Some("ns"));
	assert_eq!(
		params.get("rvt-method").map(String::as_str),
		Some("getOrCreate")
	);
	assert_eq!(
		params.get("rvt-key").map(String::as_str),
		Some("tenant,room 1")
	);
	assert_eq!(
		params.get("rvt-runner").map(String::as_str),
		Some("runner-a")
	);
	assert_eq!(
		params.get("rvt-crash-policy").map(String::as_str),
		Some("sleep")
	);
	assert_eq!(params.get("rvt-region").map(String::as_str), Some("ams"));
	assert_eq!(
		params.get("rvt-token").map(String::as_str),
		Some("dev-token")
	);
	assert!(params
		.get("rvt-input")
		.is_some_and(|value| !value.is_empty()));
}

#[tokio::test]
async fn create_sends_per_call_pool_name_as_runner_name_selector() {
	let state = RunnerSelectorState::default();
	let app = Router::new()
		.route("/actors", post(capture_runner_selector))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = Client::new(
		ClientConfig::new(endpoint(addr))
			.namespace("ns")
			.pool_name("config-pool")
			.disable_metadata_lookup(true),
	);
	client
		.create(
			"counter",
			vec!["k".to_owned()],
			CreateOptions {
				pool_name: Some("call-pool".to_owned()),
				..Default::default()
			},
		)
		.await
		.unwrap();

	assert_eq!(state.take(), vec!["call-pool".to_owned()]);

	server.abort();
}

#[tokio::test]
async fn create_falls_back_to_config_pool_name() {
	let state = RunnerSelectorState::default();
	let app = Router::new()
		.route("/actors", post(capture_runner_selector))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = Client::new(
		ClientConfig::new(endpoint(addr))
			.namespace("ns")
			.pool_name("config-pool")
			.disable_metadata_lookup(true),
	);
	client
		.create("counter", vec!["k".to_owned()], CreateOptions::default())
		.await
		.unwrap();

	assert_eq!(state.take(), vec!["config-pool".to_owned()]);

	server.abort();
}

#[tokio::test]
async fn get_or_create_sends_per_call_pool_name_as_runner_name_selector() {
	let state = RunnerSelectorState::default();
	let app = Router::new()
		.route("/actors", put(capture_runner_selector))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = Client::new(
		ClientConfig::new(endpoint(addr))
			.namespace("ns")
			.pool_name("config-pool")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get_or_create(
			"counter",
			vec!["k".to_owned()],
			GetOrCreateOptions {
				pool_name: Some("call-pool".to_owned()),
				..Default::default()
			},
		)
		.unwrap();
	// Resolving the handle to an actor ID triggers the PUT /actors request
	// that carries runner_name_selector.
	actor.resolve().await.unwrap();

	assert_eq!(state.take(), vec!["call-pool".to_owned()]);

	server.abort();
}

#[tokio::test]
async fn get_or_create_falls_back_to_config_pool_name() {
	let state = RunnerSelectorState::default();
	let app = Router::new()
		.route("/actors", put(capture_runner_selector))
		.with_state(state.clone());

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = Client::new(
		ClientConfig::new(endpoint(addr))
			.namespace("ns")
			.pool_name("config-pool")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get_or_create("counter", vec!["k".to_owned()], GetOrCreateOptions::default())
		.unwrap();
	// Resolving the handle to an actor ID triggers the PUT /actors request
	// that carries runner_name_selector.
	actor.resolve().await.unwrap();

	assert_eq!(state.take(), vec!["config-pool".to_owned()]);

	server.abort();
}

#[test]
fn gateway_url_get_or_create_uses_per_call_pool_name() {
	let client = Client::new(
		ClientConfig::new("http://127.0.0.1:6420")
			.namespace("ns")
			.pool_name("config-pool")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get_or_create(
			"chat room",
			vec!["tenant".to_owned()],
			GetOrCreateOptions {
				pool_name: Some("call-pool".to_owned()),
				..Default::default()
			},
		)
		.unwrap();

	let url = Url::parse(&actor.gateway_url().unwrap()).unwrap();
	let params = query_params(&url);
	assert_eq!(
		params.get("rvt-runner").map(String::as_str),
		Some("call-pool")
	);
}

#[tokio::test]
async fn metadata_lookup_overrides_endpoint_before_requests() {
	let target_state = TestState {
		saw_bare_action: Arc::new(AtomicBool::new(false)),
		saw_bare_queue: Arc::new(AtomicBool::new(false)),
		saw_raw_fetch: Arc::new(AtomicBool::new(false)),
		saw_raw_websocket: Arc::new(AtomicBool::new(false)),
	};
	let target_app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/action/{action}", post(action))
		.with_state(target_state.clone());
	let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let target_addr = target_listener.local_addr().unwrap();
	let target_server = tokio::spawn(async move {
		axum::serve(target_listener, target_app).await.unwrap();
	});

	let metadata_state = MetadataLookupState {
		saw_metadata: Arc::new(AtomicBool::new(false)),
		target_endpoint: endpoint(target_addr),
	};
	let metadata_seen = metadata_state.saw_metadata.clone();
	let metadata_app = Router::new()
		.route("/metadata", get(metadata_response))
		.with_state(metadata_state);
	let metadata_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let metadata_addr = metadata_listener.local_addr().unwrap();
	let metadata_server = tokio::spawn(async move {
		axum::serve(metadata_listener, metadata_app).await.unwrap();
	});

	let client = Client::new(ClientConfig::new(endpoint(metadata_addr)));
	let actor = client
		.get_or_create(
			"counter",
			vec!["metadata".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();
	let output = actor.action("increment", vec![json!(2)]).await.unwrap();

	assert_eq!(output, json!({ "count": 3 }));
	assert!(metadata_seen.load(Ordering::SeqCst));
	assert!(target_state.saw_bare_action.load(Ordering::SeqCst));

	metadata_server.abort();
	target_server.abort();
}

#[tokio::test]
async fn disable_metadata_lookup_skips_pre_call_metadata_fetch() {
	let saw_metadata = Arc::new(AtomicBool::new(false));
	let app = Router::new()
		.route("/metadata", get(disabled_metadata_response))
		.route("/actors", put(get_or_create_actor))
		.route(
			"/gateway/{actor_id}/action/{action}",
			post(action_for_disable_metadata),
		)
		.with_state(DisableMetadataState {
			saw_metadata: saw_metadata.clone(),
		});

	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = test_client(addr);
	let actor = client
		.get_or_create(
			"counter",
			vec!["metadata-disabled".to_owned()],
			GetOrCreateOptions::default(),
		)
		.unwrap();
	let output = actor.action("increment", vec![json!(2)]).await.unwrap();

	assert_eq!(output, json!({ "count": 3 }));
	assert!(!saw_metadata.load(Ordering::SeqCst));

	server.abort();
}

async fn get_or_create_actor(Json(request): Json<ActorRequest>) -> impl IntoResponse {
	Json(ActorResponse {
		actor: Actor {
			actor_id: "actor-1",
			name: request.name,
			key: request.key,
		},
		created: true,
	})
}

async fn get_or_create_actor_with_config_header(
	State(state): State<ConfigHeaderTestState>,
	headers: HeaderMap,
	Json(request): Json<ActorRequest>,
) -> impl IntoResponse {
	assert_config_header(&headers);
	state.saw_actor_lookup.store(true, Ordering::SeqCst);
	Json(ActorResponse {
		actor: Actor {
			actor_id: "actor-1",
			name: request.name,
			key: request.key,
		},
		created: true,
	})
}

async fn metadata_response(
	State(state): State<MetadataLookupState>,
	headers: HeaderMap,
) -> impl IntoResponse {
	assert_eq!(
		headers
			.get("x-rivet-namespace")
			.and_then(|value| value.to_str().ok()),
		Some("default")
	);
	state.saw_metadata.store(true, Ordering::SeqCst);
	Json(json!({
		"runtime": "rivetkit",
		"version": "test",
		"envoyProtocolVersion": 1,
		"actorNames": {},
		"clientEndpoint": state.target_endpoint,
		"clientNamespace": "metadata-namespace"
	}))
}

async fn disabled_metadata_response(
	State(state): State<DisableMetadataState>,
) -> impl IntoResponse {
	state.saw_metadata.store(true, Ordering::SeqCst);
	StatusCode::INTERNAL_SERVER_ERROR
}

async fn raw_fetch(
	State(state): State<TestState>,
	Path((actor_id, path)): Path<(String, String)>,
	headers: HeaderMap,
	method: AxumMethod,
	uri: Uri,
	body: Bytes,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	assert_eq!(path, "api/echo");
	assert_eq!(method, AxumMethod::POST);
	assert_eq!(
		uri.path_and_query().map(|value| value.as_str()),
		Some("/gateway/actor-1/request/api/echo?source=rust")
	);
	assert_eq!(
		headers
			.get("x-test-header")
			.and_then(|value| value.to_str().ok()),
		Some("raw")
	);
	assert_eq!(
		headers
			.get("x-rivet-target")
			.and_then(|value| value.to_str().ok()),
		Some("actor")
	);
	assert_eq!(
		headers
			.get("x-rivet-actor")
			.and_then(|value| value.to_str().ok()),
		Some("actor-1")
	);
	state.saw_raw_fetch.store(true, Ordering::SeqCst);

	(
		StatusCode::CREATED,
		[(header::CONTENT_TYPE, "text/plain")],
		format!("{}:{}", method, String::from_utf8_lossy(&body)),
	)
}

async fn raw_websocket(
	State(state): State<TestState>,
	Path((actor_id, path)): Path<(String, String)>,
	headers: HeaderMap,
	uri: Uri,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	assert_eq!(path, "ws");
	assert_eq!(
		uri.path_and_query().map(|value| value.as_str()),
		Some("/gateway/actor-1/websocket/ws?source=rust")
	);
	let protocols = headers
		.get(header::SEC_WEBSOCKET_PROTOCOL)
		.and_then(|value| value.to_str().ok())
		.unwrap_or_default()
		.to_owned();
	assert!(protocols.contains("rivet"));
	assert!(protocols.contains("rivet_target.actor"));
	assert!(protocols.contains("rivet_actor.actor-1"));
	assert!(protocols.contains("raw.test"));
	assert!(!protocols.contains("rivet_encoding."));
	state.saw_raw_websocket.store(true, Ordering::SeqCst);

	ws.protocols(["raw.test"]).on_upgrade(raw_websocket_echo)
}

async fn raw_websocket_echo(mut socket: WebSocket) {
	while let Some(Ok(message)) = socket.next().await {
		match message {
			AxumWsMessage::Text(text) => {
				socket
					.send(AxumWsMessage::Text(format!("raw:{text}").into()))
					.await
					.unwrap();
			}
			AxumWsMessage::Binary(bytes) => {
				socket.send(AxumWsMessage::Binary(bytes)).await.unwrap();
			}
			AxumWsMessage::Close(_) => break,
			AxumWsMessage::Ping(_) | AxumWsMessage::Pong(_) => {}
		}
	}
}

async fn action(
	State(state): State<TestState>,
	Path((actor_id, action)): Path<(String, String)>,
	headers: HeaderMap,
	body: Bytes,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	assert_eq!(action, "increment");
	assert_eq!(
		headers
			.get("x-rivet-encoding")
			.and_then(|value| value.to_str().ok()),
		Some("bare")
	);
	state.saw_bare_action.store(true, Ordering::SeqCst);

	let request =
        <wire::versioned::HttpActionRequest as OwnedVersionedData>::deserialize_with_embedded_version(
            &body,
        )
        .unwrap();
	let args: Vec<JsonValue> = serde_cbor::from_slice(&request.args).unwrap();
	assert_eq!(args, vec![json!(2)]);

	let payload = wire::versioned::HttpActionResponse::wrap_latest(wire::HttpActionResponse {
		output: serde_cbor::to_vec(&json!({ "count": 3 })).unwrap(),
	})
	.serialize_with_embedded_version(wire::PROTOCOL_VERSION)
	.unwrap();

	(
		StatusCode::OK,
		[(header::CONTENT_TYPE, "application/octet-stream")],
		payload,
	)
}

async fn action_with_config_header(
	State(state): State<ConfigHeaderTestState>,
	Path((actor_id, action_name)): Path<(String, String)>,
	headers: HeaderMap,
	body: Bytes,
) -> impl IntoResponse {
	assert_config_header(&headers);
	state.saw_action.store(true, Ordering::SeqCst);
	action(
		State(TestState {
			saw_bare_action: Arc::new(AtomicBool::new(false)),
			saw_bare_queue: Arc::new(AtomicBool::new(false)),
			saw_raw_fetch: Arc::new(AtomicBool::new(false)),
			saw_raw_websocket: Arc::new(AtomicBool::new(false)),
		}),
		Path((actor_id, action_name)),
		headers,
		body,
	)
	.await
}

async fn action_for_disable_metadata(
	Path((actor_id, action_name)): Path<(String, String)>,
	headers: HeaderMap,
	body: Bytes,
) -> impl IntoResponse {
	action(
		State(TestState {
			saw_bare_action: Arc::new(AtomicBool::new(false)),
			saw_bare_queue: Arc::new(AtomicBool::new(false)),
			saw_raw_fetch: Arc::new(AtomicBool::new(false)),
			saw_raw_websocket: Arc::new(AtomicBool::new(false)),
		}),
		Path((actor_id, action_name)),
		headers,
		body,
	)
	.await
}

async fn queue_send(
	State(state): State<TestState>,
	Path((actor_id, queue)): Path<(String, String)>,
	headers: HeaderMap,
	body: Bytes,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	assert_eq!(queue, "jobs");
	assert_eq!(
		headers
			.get("x-rivet-encoding")
			.and_then(|value| value.to_str().ok()),
		Some("bare")
	);
	state.saw_bare_queue.store(true, Ordering::SeqCst);

	let request =
        <wire::versioned::HttpQueueSendRequest as OwnedVersionedData>::deserialize_with_embedded_version(
            &body,
        )
        .unwrap();
	assert_eq!(request.name.as_deref(), Some("jobs"));
	let payload: JsonValue = serde_cbor::from_slice(&request.body).unwrap();
	assert!(payload == json!({ "id": 1 }) || payload == json!({ "id": 2 }));
	if payload == json!({ "id": 1 }) {
		assert_eq!(request.wait, Some(false));
		assert_eq!(request.timeout, None);
	} else {
		assert_eq!(request.wait, Some(true));
		assert_eq!(request.timeout, Some(50));
	}

	let payload =
		wire::versioned::HttpQueueSendResponse::wrap_latest(wire::HttpQueueSendResponse {
			status: "completed".to_owned(),
			response: request
				.wait
				.unwrap_or_default()
				.then(|| serde_cbor::to_vec(&json!({ "accepted": "jobs" })).unwrap()),
		})
		.serialize_with_embedded_version(wire::PROTOCOL_VERSION)
		.unwrap();

	(
		StatusCode::OK,
		[(header::CONTENT_TYPE, "application/octet-stream")],
		payload,
	)
}

async fn connection_websocket(
	State(state): State<ConnectionTestState>,
	Path(actor_id): Path<String>,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	ws.protocols(["rivet"])
		.on_upgrade(move |socket| connection_lifecycle(socket, state))
}

async fn connection_once_event_websocket(
	State(state): State<OnceEventTestState>,
	Path(actor_id): Path<String>,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	ws.protocols(["rivet"])
		.on_upgrade(move |socket| connection_once_event(socket, state))
}

async fn connection_websocket_with_config_header(
	State(state): State<ConfigHeaderTestState>,
	Path(actor_id): Path<String>,
	headers: HeaderMap,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	assert_config_header(&headers);
	state.saw_connection_websocket.store(true, Ordering::SeqCst);
	ws.protocols(["rivet"])
		.on_upgrade(config_header_connection_websocket)
}

async fn raw_websocket_with_config_header(
	State(state): State<ConfigHeaderTestState>,
	Path((actor_id, path)): Path<(String, String)>,
	headers: HeaderMap,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	assert_eq!(actor_id, "actor-1");
	assert_eq!(path, "ws");
	assert_config_header(&headers);
	state.saw_raw_websocket.store(true, Ordering::SeqCst);
	ws.protocols(["raw.test"])
		.on_upgrade(|_socket| async move {})
}

async fn config_header_connection_websocket(mut socket: WebSocket) {
	socket
		.send(connection_message(wire::ToClientBody::Init(wire::Init {
			actor_id: "actor-1".to_owned(),
			connection_id: "conn-1".to_owned(),
		})))
		.await
		.unwrap();

	while let Some(Ok(message)) = socket.next().await {
		if matches!(message, AxumWsMessage::Close(_)) {
			break;
		}
	}
}

async fn connection_once_event(mut socket: WebSocket, state: OnceEventTestState) {
	state.release_init.notified().await;

	socket
		.send(connection_message(wire::ToClientBody::Init(wire::Init {
			actor_id: "actor-1".to_owned(),
			connection_id: "conn-1".to_owned(),
		})))
		.await
		.unwrap();

	for value in [1, 2] {
		socket
			.send(connection_message(wire::ToClientBody::Event(wire::Event {
				name: "tick".to_owned(),
				args: serde_cbor::to_vec(&vec![json!(value)]).unwrap(),
			})))
			.await
			.unwrap();
	}

	let mut saw_unsubscribe = false;
	while let Some(Ok(message)) = socket.next().await {
		let AxumWsMessage::Binary(body) = message else {
			continue;
		};
		let msg =
			<wire::versioned::ToServer as OwnedVersionedData>::deserialize_with_embedded_version(
				&body,
			)
			.unwrap();
		if let wire::ToServerBody::SubscriptionRequest(request) = msg.body {
			if request.event_name == "tick" && request.subscribe == false {
				saw_unsubscribe = true;
				state.unsubscribe_seen.notify_one();
				break;
			}
		}
	}

	assert!(saw_unsubscribe);
}

async fn connection_lifecycle(mut socket: WebSocket, state: ConnectionTestState) {
	state.release_init.notified().await;

	socket
		.send(connection_message(wire::ToClientBody::Init(wire::Init {
			actor_id: "actor-1".to_owned(),
			connection_id: "conn-1".to_owned(),
		})))
		.await
		.unwrap();
	socket
		.send(connection_message(wire::ToClientBody::Error(wire::Error {
			group: "actor".to_owned(),
			code: "test".to_owned(),
			message: "server-side lifecycle error".to_owned(),
			metadata: None,
			action_id: None,
			actor: None,
		})))
		.await
		.unwrap();
	while let Some(Ok(message)) = socket.next().await {
		if matches!(message, AxumWsMessage::Close(_)) {
			break;
		}
	}
}

fn connection_message(body: wire::ToClientBody) -> AxumWsMessage {
	let payload = wire::versioned::ToClient::wrap_latest(wire::ToClient { body })
		.serialize_with_embedded_version(wire::PROTOCOL_VERSION)
		.unwrap();
	AxumWsMessage::Binary(payload.into())
}

async fn wait_for_status_watch(
	rx: &mut tokio::sync::watch::Receiver<ConnectionStatus>,
	expected: ConnectionStatus,
) {
	timeout(Duration::from_secs(2), async {
		loop {
			if *rx.borrow_and_update() == expected {
				break;
			}
			rx.changed().await.unwrap();
		}
	})
	.await
	.unwrap();
}

async fn wait_for_status_event(
	rx: &mut mpsc::UnboundedReceiver<ConnectionStatus>,
	expected: ConnectionStatus,
) {
	timeout(Duration::from_secs(2), async {
		while let Some(status) = rx.recv().await {
			if status == expected {
				break;
			}
		}
	})
	.await
	.unwrap();
}

fn endpoint(addr: SocketAddr) -> String {
	format!("http://{addr}")
}

fn query_params(url: &Url) -> HashMap<String, String> {
	url.query_pairs().into_owned().collect()
}

fn test_client(addr: SocketAddr) -> Client {
	Client::new(ClientConfig::new(endpoint(addr)).disable_metadata_lookup(true))
}

fn assert_config_header(headers: &HeaderMap) {
	assert_eq!(
		headers
			.get("x-config-header")
			.and_then(|value| value.to_str().ok()),
		Some("from-config")
	);
}
