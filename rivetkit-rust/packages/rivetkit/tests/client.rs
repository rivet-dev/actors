use std::{
	collections::HashMap,
	future::Future,
	io::Cursor,
	net::SocketAddr,
	pin::Pin,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};

use anyhow::{Result, bail};
use axum::{
	Json, Router,
	body::Bytes,
	extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
	extract::{Path, State},
	http::{HeaderMap, StatusCode, header},
	response::IntoResponse,
	routing::{any, post, put},
};
use futures::StreamExt;
use rivet_envoy_client::{
	config::{
		BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
		WebSocketSender,
	},
	context::{SharedContext, WsTxMessage},
	handle::EnvoyHandle,
	protocol,
};
use rivetkit::{
	Action, Actor, Ctx, Event, Handles, TypedClientExt, action,
	client::{Client, ClientConfig},
};
use rivetkit_client_protocol as wire;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::time::timeout;
use tokio::{net::TcpListener, sync::mpsc};
use vbare::OwnedVersionedData;

struct CallerActor;

impl Actor for CallerActor {
	type State = ();
	type Input = ();
	type Actions = (SiblingPing,);
	type Events = (SiblingNotice,);
	type Queue = ();
	type ConnParams = ();
	type ConnState = ();
	type Action = action::Raw;
}

impl Handles<SiblingPing> for CallerActor {
	type Future = Pin<Box<dyn Future<Output = Result<SiblingPong>> + Send>>;

	fn handle(self: Arc<Self>, _ctx: Ctx<Self>, _action: SiblingPing) -> Self::Future {
		Box::pin(async {
			Ok(SiblingPong {
				reply: "unused-local-handler".to_owned(),
			})
		})
	}
}

#[derive(Clone)]
struct TestState {
	saw_sibling_action: Arc<AtomicBool>,
}

#[tokio::test]
async fn actor_ctx_client_calls_sibling_action() {
	let state = TestState {
		saw_sibling_action: Arc::new(AtomicBool::new(false)),
	};
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/action/{action}", post(sibling_action))
		.with_state(state.clone());
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let core_ctx = rivetkit_core::testing::ActorContextHarness::with_client_endpoint(endpoint(
		addr,
	))
	.context("caller-1", "caller", Vec::new(), "local");
	core_ctx.configure_envoy(test_envoy_handle(endpoint(addr)), Some(1));
	let ctx = Ctx::<CallerActor>::new(core_ctx);

	let output = call_sibling(ctx).await.unwrap();

	assert_eq!(
		output,
		(
			SiblingPong {
				reply: "pong".to_owned()
			},
			SiblingPong {
				reply: "pong".to_owned()
			},
			SiblingPong {
				reply: "pong".to_owned()
			}
		)
	);
	assert!(state.saw_sibling_action.load(Ordering::SeqCst));

	server.abort();
}

async fn call_sibling(ctx: Ctx<CallerActor>) -> Result<(SiblingPong, SiblingPong, SiblingPong)> {
	let sibling = ctx
		.client()?
		.get_or_create_typed_default::<CallerActor>("sibling", ["sibling-key"])?;

	let typed_send = sibling
		.send(SiblingPing {
			from: "from-caller".to_owned(),
		})
		.await?;
	let tier_two_call = sibling
		.call(SiblingPing {
			from: "from-caller".to_owned(),
		})
		.await?;
	let dynamic_action = serde_json::from_value(
		sibling
			.inner()
			.action("ping", vec![json!("from-caller")])
			.await?,
	)?;

	Ok((typed_send, tier_two_call, dynamic_action))
}

#[derive(Debug, Serialize, Deserialize)]
struct SiblingPing {
	from: String,
}

impl Action for SiblingPing {
	type Output = SiblingPong;

	const NAME: &'static str = "ping";
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SiblingPong {
	reply: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SiblingNotice {
	message: String,
	count: u32,
}

impl Event for SiblingNotice {
	const NAME: &'static str = "notice";
}

#[tokio::test]
async fn typed_connection_receives_event() {
	let app = Router::new()
		.route("/actors", put(get_or_create_actor))
		.route("/gateway/{actor_id}/connect", any(typed_event_websocket));
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let server = tokio::spawn(async move {
		axum::serve(listener, app).await.unwrap();
	});

	let client = Client::new(
		ClientConfig::new(endpoint(addr))
			.token("secret")
			.disable_metadata_lookup(true),
	);
	let actor = client
		.get_or_create_typed_default::<CallerActor>("sibling", ["typed-event"])
		.unwrap();
	let conn = actor.connect();
	let (event_tx, mut event_rx) = mpsc::unbounded_channel();

	let _subscription = conn
		.on::<SiblingNotice>(move |event| {
			event_tx.send(event).ok();
		})
		.await;

	let event = timeout(Duration::from_secs(2), event_rx.recv())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(
		event,
		SiblingNotice {
			message: "typed-event".to_owned(),
			count: 7
		}
	);

	server.abort();
	// The mock server closes after one event; keep cleanup bounded so a reconnect
	// attempt cannot park the test after the event assertion has passed.
	let _ = timeout(Duration::from_millis(200), conn.disconnect()).await;
}

async fn get_or_create_actor(Json(body): Json<JsonValue>) -> impl IntoResponse {
	assert_eq!(body.get("name"), Some(&json!("sibling")));

	Json(json!({
		"actor": {
			"actor_id": "sibling-1",
			"name": "sibling",
			"key": body.get("key").and_then(JsonValue::as_str).unwrap_or("[]"),
		},
		"created": false,
	}))
}

async fn sibling_action(
	State(state): State<TestState>,
	Path((actor_id, action)): Path<(String, String)>,
	headers: HeaderMap,
	body: Bytes,
) -> impl IntoResponse {
	assert_eq!(actor_id, "sibling-1@secret");
	assert_eq!(action, "ping");
	assert_eq!(
		headers
			.get("x-rivet-token")
			.and_then(|value| value.to_str().ok()),
		Some("secret")
	);
	state.saw_sibling_action.store(true, Ordering::SeqCst);

	let request =
		<wire::versioned::HttpActionRequest as OwnedVersionedData>::deserialize_with_embedded_version(
			&body,
		)
		.unwrap();
	let args: Vec<JsonValue> =
		ciborium::from_reader(Cursor::new(request.args)).expect("decode action args");
	assert!(args == vec![json!({ "from": "from-caller" })] || args == vec![json!("from-caller")]);

	let payload = wire::versioned::HttpActionResponse::wrap_latest(wire::HttpActionResponse {
		output: cbor(&json!({ "reply": "pong" })),
	})
	.serialize_with_embedded_version(wire::PROTOCOL_VERSION)
	.unwrap();

	(
		StatusCode::OK,
		[(header::CONTENT_TYPE, "application/octet-stream")],
		payload,
	)
}

async fn typed_event_websocket(
	Path(actor_id): Path<String>,
	ws: WebSocketUpgrade,
) -> impl IntoResponse {
	assert_eq!(actor_id, "sibling-1@secret");
	ws.protocols(["rivet"]).on_upgrade(typed_event_connection)
}

async fn typed_event_connection(mut socket: WebSocket) {
	socket
		.send(connection_message(wire::ToClientBody::Init(wire::Init {
			actor_id: "sibling-1".to_owned(),
			connection_id: "conn-1".to_owned(),
		})))
		.await
		.unwrap();

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
			assert_eq!(request.event_name, "notice");
			assert!(request.subscribe);
			socket
				.send(connection_message(wire::ToClientBody::Event(wire::Event {
					name: "notice".to_owned(),
					args: cbor(&vec![SiblingNotice {
						message: "typed-event".to_owned(),
						count: 7,
					}]),
				})))
				.await
				.unwrap();
			break;
		}
	}
}

fn test_envoy_handle(endpoint: String) -> EnvoyHandle {
	let (envoy_tx, _envoy_rx) = mpsc::unbounded_channel();
	let shared = Arc::new(SharedContext {
		config: EnvoyConfig {
			version: 1,
			endpoint,
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
		actors: Arc::new(Mutex::new(HashMap::new())),
		actors_notify: Arc::new(tokio::sync::Notify::new()),
		live_tunnel_requests: Arc::new(Mutex::new(HashMap::new())),
		pending_hibernation_restores: Arc::new(Mutex::new(HashMap::new())),
		ws_tx: Arc::new(tokio::sync::Mutex::new(
			None::<mpsc::UnboundedSender<WsTxMessage>>,
		)),
		http_ws_tx: Arc::new(tokio::sync::Mutex::new(None)),
		connection_session: std::sync::atomic::AtomicU64::new(0),
		next_connection_session: std::sync::atomic::AtomicU64::new(0),
		connection_session_tx: tokio::sync::watch::channel(0).0,
		protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
		shutting_down: AtomicBool::new(false),
		last_ping_ts: std::sync::atomic::AtomicI64::new(0),
		stopped_tx: tokio::sync::watch::channel(true).0,
	});

	EnvoyHandle::from_shared(shared)
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
		Box::pin(async { bail!("fetch should not run in c.client test") })
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
		Box::pin(async { bail!("websocket should not run in c.client test") })
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

fn endpoint(addr: SocketAddr) -> String {
	format!("http://{addr}")
}

fn connection_message(body: wire::ToClientBody) -> AxumWsMessage {
	let payload = wire::versioned::ToClient::wrap_latest(wire::ToClient { body })
		.serialize_with_embedded_version(wire::PROTOCOL_VERSION)
		.unwrap();
	AxumWsMessage::Binary(payload.into())
}

fn cbor<T: Serialize>(value: &T) -> Vec<u8> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded).expect("encode test cbor");
	encoded
}
