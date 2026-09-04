//! Pegboard Envoy test client.
//!
//! This helper uses the Rust Envoy client and therefore exercises `/envoys/connect`.

use anyhow::{Context, Result, bail};
use rivet_envoy_protocol as ep;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, mpsc};

pub use super::test_runner::{
	Actor, ActorConfig, ActorEvent, ActorLifecycleEvent, ActorStartResult, ActorStopResult,
	CountingCrashActor, CrashNTimesThenSucceedActor, CrashOnStartActor, CustomActor,
	CustomActorBuilder, DelayedStartActor, EchoActor, KvRequest, NotifyOnStartActor,
	SleepImmediatelyActor, StopImmediatelyActor, TestActor, TimeoutActor, VerifyInputActor,
};
pub use rivet_envoy_protocol::PROTOCOL_VERSION;
pub use rivet_test_envoy::{BoxFuture, EnvoyHandle, HttpRequest, HttpResponse, WebSocketHandler};

type ActorFactory = Arc<dyn Fn(ActorConfig) -> Box<dyn TestActor> + Send + Sync>;

pub type TestEnvoy = Envoy;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvoyConnectionEvent {
	Connected,
	Disconnected,
}

pub struct EnvoyConnectionEventWaiter {
	rx: broadcast::Receiver<EnvoyConnectionEvent>,
	expected: EnvoyConnectionEvent,
	timeout: std::time::Duration,
}

impl EnvoyConnectionEventWaiter {
	pub fn assert_no_event(&mut self) {
		match self.rx.try_recv() {
			Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {}
			Ok(event) => panic!("unexpected Envoy connection event before fault: {event:?}"),
			Err(tokio::sync::broadcast::error::TryRecvError::Lagged(count)) => {
				panic!("missed {count} Envoy connection events before fault")
			}
			Err(err) => panic!("Envoy connection event channel closed: {err}"),
		}
	}

	pub async fn wait(mut self) {
		tokio::time::timeout(self.timeout, async {
			loop {
				match self.rx.recv().await {
					Ok(event) if event == self.expected => break,
					Ok(_) => {}
					Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
					Err(err) => panic!("Envoy connection event channel closed: {err}"),
				}
			}
		})
		.await
		.expect("timed out waiting for Envoy connection event");
	}
}

#[derive(Clone)]
pub struct EnvoyConfig {
	endpoint: String,
	token: String,
	namespace: String,
	pool_name: String,
	version: u32,
	metadata: Option<serde_json::Value>,
}

impl EnvoyConfig {
	pub fn builder() -> EnvoyConfigBuilder {
		EnvoyConfigBuilder::default()
	}
}

#[derive(Default)]
pub struct EnvoyConfigBuilder {
	endpoint: Option<String>,
	token: Option<String>,
	namespace: Option<String>,
	pool_name: Option<String>,
	version: Option<u32>,
	metadata: Option<serde_json::Value>,
}

impl EnvoyConfigBuilder {
	pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
		self.endpoint = Some(endpoint.into());
		self
	}

	pub fn token(mut self, token: impl Into<String>) -> Self {
		self.token = Some(token.into());
		self
	}

	pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
		self.namespace = Some(namespace.into());
		self
	}

	pub fn pool_name(mut self, pool_name: impl Into<String>) -> Self {
		self.pool_name = Some(pool_name.into());
		self
	}

	pub fn version(mut self, version: u32) -> Self {
		self.version = Some(version);
		self
	}

	pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
		self.metadata = Some(metadata);
		self
	}

	pub fn build(self) -> Result<EnvoyConfig> {
		Ok(EnvoyConfig {
			endpoint: self.endpoint.context("endpoint is required")?,
			token: self.token.unwrap_or_else(|| "dev".to_string()),
			namespace: self.namespace.context("namespace is required")?,
			pool_name: self.pool_name.unwrap_or_else(|| "test-envoy".to_string()),
			version: self.version.unwrap_or(1),
			metadata: self.metadata,
		})
	}
}

pub struct EnvoyBuilder {
	config: EnvoyConfig,
	actor_factories: HashMap<String, ActorFactory>,
}

impl EnvoyBuilder {
	pub fn new(config: EnvoyConfig) -> Self {
		Self {
			config,
			actor_factories: HashMap::new(),
		}
	}

	pub fn with_actor_behavior<F>(mut self, actor_name: &str, factory: F) -> Self
	where
		F: Fn(ActorConfig) -> Box<dyn TestActor> + Send + Sync + 'static,
	{
		self.actor_factories
			.insert(actor_name.to_string(), Arc::new(factory));
		self
	}

	pub fn build(self) -> Result<Envoy> {
		let (lifecycle_tx, _) = broadcast::channel(100);
		Ok(Envoy {
			config: self.config,
			inner: Arc::new(EnvoyInner {
				actor_factories: self.actor_factories,
				actors: tokio::sync::Mutex::new(HashMap::new()),
				lifecycle_tx,
				connection_tx: broadcast::channel(100).0,
			}),
			handle: tokio::sync::Mutex::new(None),
			envoy_key: uuid::Uuid::new_v4().to_string(),
		})
	}
}

struct EnvoyInner {
	actor_factories: HashMap<String, ActorFactory>,
	actors: tokio::sync::Mutex<HashMap<String, Box<dyn TestActor>>>,
	lifecycle_tx: broadcast::Sender<ActorLifecycleEvent>,
	connection_tx: broadcast::Sender<EnvoyConnectionEvent>,
}

pub struct Envoy {
	config: EnvoyConfig,
	inner: Arc<EnvoyInner>,
	handle: tokio::sync::Mutex<Option<EnvoyHandle>>,
	pub envoy_key: String,
}

impl Envoy {
	pub async fn start(&self) -> Result<()> {
		let callbacks = Arc::new(TestEnvoyCallbacks {
			inner: self.inner.clone(),
		});
		let config = rivet_test_envoy::EnvoyConfig {
			version: self.config.version,
			endpoint: self.config.endpoint.clone(),
			token: Some(self.config.token.clone()),
			namespace: self.config.namespace.clone(),
			pool_name: self.config.pool_name.clone(),
			prepopulate_actor_names: self
				.inner
				.actor_factories
				.keys()
				.map(|name| {
					(
						name.clone(),
						rivet_test_envoy::ActorName {
							metadata: serde_json::Value::Object(serde_json::Map::new()),
						},
					)
				})
				.collect(),
			metadata: self.config.metadata.clone(),
			not_global: true,
			debug_latency_ms: None,
			callbacks,
		};

		let handle = rivet_test_envoy::start_envoy_sync(config);
		handle.started().await?;
		*self.handle.lock().await = Some(handle);
		Ok(())
	}

	pub async fn wait_ready(&self) {
		if let Some(handle) = self.handle.lock().await.as_ref() {
			let _ = handle.started().await;
		}
	}

	pub async fn has_actor(&self, actor_id: &str) -> bool {
		self.inner.actors.lock().await.contains_key(actor_id)
	}

	pub async fn get_actor_ids(&self) -> Vec<String> {
		self.inner.actors.lock().await.keys().cloned().collect()
	}

	pub fn pool_name(&self) -> &str {
		&self.config.pool_name
	}

	pub fn subscribe_lifecycle_events(&self) -> broadcast::Receiver<ActorLifecycleEvent> {
		self.inner.lifecycle_tx.subscribe()
	}

	pub fn subscribe_connection_events(&self) -> broadcast::Receiver<EnvoyConnectionEvent> {
		self.inner.connection_tx.subscribe()
	}

	pub fn wait_for_next_connection_event(
		&self,
		expected: EnvoyConnectionEvent,
	) -> EnvoyConnectionEventWaiter {
		EnvoyConnectionEventWaiter {
			rx: self.subscribe_connection_events(),
			expected,
			timeout: std::time::Duration::from_secs(20),
		}
	}

	pub async fn is_ping_healthy(&self) -> Option<bool> {
		self.handle
			.lock()
			.await
			.as_ref()
			.map(|handle| handle.is_ping_healthy())
	}

	pub async fn current_envoy_key(&self) -> Result<String> {
		Ok(self
			.handle
			.lock()
			.await
			.as_ref()
			.context("envoy is not running")?
			.get_envoy_key()
			.to_string())
	}

	pub async fn sqlite_get_pages(
		&self,
		request: ep::SqliteGetPagesRequest,
	) -> Result<ep::SqliteGetPagesResponse> {
		let handle = self
			.handle
			.lock()
			.await
			.as_ref()
			.context("envoy is not started")?
			.clone();
		handle.sqlite_get_pages(request).await
	}

	pub async fn sqlite_commit(
		&self,
		request: ep::SqliteCommitRequest,
	) -> Result<ep::SqliteCommitResponse> {
		let handle = self
			.handle
			.lock()
			.await
			.as_ref()
			.context("envoy is not started")?
			.clone();
		handle.sqlite_commit(request).await
	}

	pub async fn remote_sqlite_execute(
		&self,
		request: ep::SqliteExecuteRequest,
	) -> Result<ep::SqliteExecuteResponse> {
		let handle = self
			.handle
			.lock()
			.await
			.as_ref()
			.context("envoy is not started")?
			.clone();
		handle.remote_sqlite_execute(request).await
	}

	pub async fn shutdown(&self) {
		if let Some(handle) = self.handle.lock().await.take() {
			handle.shutdown_and_wait(false).await;
		}
		self.inner.actors.lock().await.clear();
	}

	pub async fn crash(&self) {
		if let Some(handle) = self.handle.lock().await.take() {
			handle.shutdown_and_wait(true).await;
		}
		self.inner.actors.lock().await.clear();
	}
}

struct TestEnvoyCallbacks {
	inner: Arc<EnvoyInner>,
}

impl TestEnvoyCallbacks {
	fn actor_config(
		handle: EnvoyHandle,
		actor_id: String,
		generation: u32,
		config: ep::ActorConfig,
	) -> ActorConfig {
		let (event_tx, event_rx) = mpsc::unbounded_channel();
		let (_kv_tx, kv_rx) = mpsc::unbounded_channel();
		spawn_event_bridge(handle.clone(), event_rx);
		spawn_kv_bridge(handle, kv_rx);
		ActorConfig {
			actor_id,
			generation,
			name: config.name,
			key: config.key,
			create_ts: config.create_ts,
			input: config.input,
			event_tx,
			kv_request_tx: _kv_tx,
		}
	}
}

impl rivet_test_envoy::EnvoyCallbacks for TestEnvoyCallbacks {
	fn on_connect(&self, _handle: EnvoyHandle) {
		let _ = self
			.inner
			.connection_tx
			.send(EnvoyConnectionEvent::Connected);
	}

	fn on_disconnect(&self, _handle: EnvoyHandle) {
		let _ = self
			.inner
			.connection_tx
			.send(EnvoyConnectionEvent::Disconnected);
	}

	fn on_actor_start(
		&self,
		handle: EnvoyHandle,
		actor_id: String,
		generation: u32,
		config: ep::ActorConfig,
		_preloaded_kv: Option<ep::PreloadedKv>,
	) -> BoxFuture<Result<()>> {
		let inner = self.inner.clone();
		Box::pin(async move {
			let factory = inner
				.actor_factories
				.get(&config.name)
				.cloned()
				.unwrap_or_else(|| Arc::new(|_| Box::new(EchoActor::new())));
			let actor_config =
				Self::actor_config(handle, actor_id.clone(), generation, config.clone());
			let mut actor = factory(actor_config.clone());
			let start_result = actor.on_start(actor_config).await?;

			let _ = inner.lifecycle_tx.send(ActorLifecycleEvent::Started {
				actor_id: actor_id.clone(),
				generation,
			});

			match start_result {
				ActorStartResult::Running => {
					inner.actors.lock().await.insert(actor_id, actor);
					Ok(())
				}
				ActorStartResult::Delay(duration) => {
					tokio::time::sleep(duration).await;
					inner.actors.lock().await.insert(actor_id, actor);
					Ok(())
				}
				ActorStartResult::Timeout => std::future::pending::<Result<()>>().await,
				ActorStartResult::Crash { message, .. } => bail!(message),
			}
		})
	}

	fn on_actor_stop(
		&self,
		_handle: EnvoyHandle,
		actor_id: String,
		generation: u32,
		_reason: ep::StopActorReason,
	) -> BoxFuture<Result<()>> {
		let inner = self.inner.clone();
		Box::pin(async move {
			let actor = inner.actors.lock().await.remove(&actor_id);
			if let Some(mut actor) = actor {
				match actor.on_stop().await? {
					ActorStopResult::Success => {}
					ActorStopResult::Delay(duration) => {
						tokio::time::sleep(duration).await;
					}
					ActorStopResult::Crash { message, .. } => {
						bail!(message);
					}
				}
			}

			let _ = inner.lifecycle_tx.send(ActorLifecycleEvent::Stopped {
				actor_id,
				generation,
			});
			Ok(())
		})
	}

	fn on_shutdown(&self) {}

	fn fetch(
		&self,
		_handle: EnvoyHandle,
		actor_id: String,
		_gateway_id: ep::GatewayId,
		_request_id: ep::RequestId,
		request: HttpRequest,
	) -> BoxFuture<Result<HttpResponse>> {
		Box::pin(async move {
			let mut request_body = request.body.unwrap_or_default();
			if let Some(mut body_stream) = request.body_stream {
				while let Some(chunk) = body_stream.recv().await {
					request_body.extend(chunk);
				}
			}

			let (status, body) = match request.path.as_str() {
				"/ping" => (
					200,
					serde_json::to_vec(&serde_json::json!({
						"actorId": actor_id,
						"status": "ok",
						"timestamp": rivet_util::timestamp::now(),
					}))?,
				),
				"/echo" => (
					201,
					serde_json::to_vec(&serde_json::json!({
						"actorId": actor_id,
						"method": request.method,
						"path": request.path,
						"testHeader": request.headers.get("x-test-header").cloned(),
						"body": String::from_utf8_lossy(&request_body),
						"bodyLen": request_body.len(),
					}))?,
				),
				"/actor-error" => return Err(anyhow::anyhow!("intentional actor fetch error")),
				_ => (404, b"not found".to_vec()),
			};

			let mut headers = HashMap::new();
			headers.insert("content-length".to_string(), body.len().to_string());
			headers.insert("x-envoy-test".to_string(), "ok".to_string());
			Ok(HttpResponse {
				status,
				headers,
				body: Some(body),
				body_stream: None,
			})
		})
	}

	fn websocket(
		&self,
		_handle: EnvoyHandle,
		_actor_id: String,
		_gateway_id: ep::GatewayId,
		_request_id: ep::RequestId,
		_request: HttpRequest,
		_path: String,
		_headers: HashMap<String, String>,
		_is_hibernatable: bool,
		_is_restoring_hibernatable: bool,
		_sender: rivet_test_envoy::WebSocketSender,
	) -> BoxFuture<Result<WebSocketHandler>> {
		Box::pin(async {
			Ok(WebSocketHandler {
				on_message: Box::new(|msg| {
					let text = String::from_utf8_lossy(&msg.data);
					if text == "close-from-actor" {
						msg.sender
							.close(Some(4001), Some("actor.requested_close".to_string()));
					} else {
						msg.sender.send_text(&format!("Echo: {}", text));
					}
					Box::pin(async {})
				}),
				on_close: Box::new(|_, _| Box::pin(async {})),
				on_open: None,
			})
		})
	}

	fn can_hibernate(
		&self,
		_actor_id: &str,
		_gateway_id: &ep::GatewayId,
		_request_id: &ep::RequestId,
		_request: &HttpRequest,
	) -> BoxFuture<Result<bool>> {
		Box::pin(async { Ok(false) })
	}
}

fn spawn_event_bridge(handle: EnvoyHandle, mut event_rx: mpsc::UnboundedReceiver<ActorEvent>) {
	tokio::spawn(async move {
		while let Some(event) = event_rx.recv().await {
			match event.event {
				rivet_runner_protocol::mk2::Event::EventActorIntent(intent) => {
					match intent.intent {
						rivet_runner_protocol::mk2::ActorIntent::ActorIntentSleep => {
							handle.sleep_actor(event.actor_id, Some(event.generation), None);
						}
						rivet_runner_protocol::mk2::ActorIntent::ActorIntentStop => {
							handle.stop_actor(event.actor_id, Some(event.generation));
						}
					}
				}
				rivet_runner_protocol::mk2::Event::EventActorSetAlarm(alarm) => {
					handle.set_alarm(event.actor_id, alarm.alarm_ts, Some(event.generation));
				}
				rivet_runner_protocol::mk2::Event::EventActorStateUpdate(_) => {}
			}
		}
	});
}

fn spawn_kv_bridge(handle: EnvoyHandle, mut kv_rx: mpsc::UnboundedReceiver<KvRequest>) {
	tokio::spawn(async move {
		while let Some(req) = kv_rx.recv().await {
			let result = match req.data {
				rivet_runner_protocol::mk2::KvRequestData::KvGetRequest(body) => handle
					.kv_get(req.actor_id, body.keys.clone())
					.await
					.map(|values| {
						let mut keys = Vec::new();
						let mut out = Vec::new();
						for (key, value) in body.keys.into_iter().zip(values.into_iter()) {
							if let Some(value) = value {
								keys.push(key);
								out.push(value);
							}
						}
						rivet_runner_protocol::mk2::KvResponseData::KvGetResponse(
							rivet_runner_protocol::mk2::KvGetResponse {
								keys,
								values: out,
								metadata: Vec::new(),
							},
						)
					}),
				rivet_runner_protocol::mk2::KvRequestData::KvListRequest(body) => {
					let list_result = match body.query {
						rivet_runner_protocol::mk2::KvListQuery::KvListAllQuery => {
							handle
								.kv_list_all(req.actor_id, body.reverse, body.limit)
								.await
						}
						rivet_runner_protocol::mk2::KvListQuery::KvListRangeQuery(range) => {
							handle
								.kv_list_range(
									req.actor_id,
									range.start,
									range.end,
									range.exclusive,
									body.reverse,
									body.limit,
								)
								.await
						}
						rivet_runner_protocol::mk2::KvListQuery::KvListPrefixQuery(prefix) => {
							handle
								.kv_list_prefix(req.actor_id, prefix.key, body.reverse, body.limit)
								.await
						}
					};
					list_result.map(|entries| {
						let (keys, values): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
						rivet_runner_protocol::mk2::KvResponseData::KvListResponse(
							rivet_runner_protocol::mk2::KvListResponse {
								keys,
								values,
								metadata: Vec::new(),
							},
						)
					})
				}
				rivet_runner_protocol::mk2::KvRequestData::KvPutRequest(body) => handle
					.kv_put(
						req.actor_id,
						body.keys.into_iter().zip(body.values).collect(),
					)
					.await
					.map(|_| rivet_runner_protocol::mk2::KvResponseData::KvPutResponse),
				rivet_runner_protocol::mk2::KvRequestData::KvDeleteRequest(body) => handle
					.kv_delete(req.actor_id, body.keys)
					.await
					.map(|_| rivet_runner_protocol::mk2::KvResponseData::KvDeleteResponse),
				rivet_runner_protocol::mk2::KvRequestData::KvDeleteRangeRequest(body) => handle
					.kv_delete_range(req.actor_id, body.start, body.end)
					.await
					.map(|_| rivet_runner_protocol::mk2::KvResponseData::KvDeleteResponse),
				rivet_runner_protocol::mk2::KvRequestData::KvDropRequest => handle
					.kv_drop(req.actor_id)
					.await
					.map(|_| rivet_runner_protocol::mk2::KvResponseData::KvDropResponse),
			}
			.unwrap_or_else(|err| {
				rivet_runner_protocol::mk2::KvResponseData::KvErrorResponse(
					rivet_runner_protocol::mk2::KvErrorResponse {
						message: err.to_string(),
					},
				)
			});
			let _ = req.response_tx.send(result);
		}
	});
}

pub struct TestEnvoyBuilder {
	namespace: String,
	pool_name: String,
	version: u32,
	endpoint: Option<String>,
	actor_factories: HashMap<String, ActorFactory>,
}

impl TestEnvoyBuilder {
	pub fn new(namespace: &str) -> Self {
		Self {
			namespace: namespace.to_string(),
			pool_name: "test-envoy".to_string(),
			version: 1,
			endpoint: None,
			actor_factories: HashMap::new(),
		}
	}

	pub fn with_pool_name(mut self, name: &str) -> Self {
		self.pool_name = name.to_string();
		self
	}

	pub fn with_version(mut self, version: u32) -> Self {
		self.version = version;
		self
	}

	pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
		self.endpoint = Some(endpoint.into());
		self
	}

	pub fn with_actor_behavior<F>(mut self, actor_name: &str, factory: F) -> Self
	where
		F: Fn(ActorConfig) -> Box<dyn TestActor> + Send + Sync + 'static,
	{
		self.actor_factories
			.insert(actor_name.to_string(), Arc::new(factory));
		self
	}

	pub async fn build(self, dc: &super::TestDatacenter) -> Result<Envoy> {
		let config = EnvoyConfig::builder()
			.endpoint(
				self.endpoint
					.unwrap_or_else(|| format!("http://127.0.0.1:{}", dc.guard_port())),
			)
			.token("dev")
			.namespace(&self.namespace)
			.pool_name(&self.pool_name)
			.version(self.version)
			.build()?;
		let mut builder = EnvoyBuilder::new(config);
		for (name, factory) in self.actor_factories {
			builder = builder.with_actor_behavior(&name, move |config| factory(config));
		}
		builder.build()
	}
}
