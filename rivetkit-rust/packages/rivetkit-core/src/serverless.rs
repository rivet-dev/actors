use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use http::StatusCode;
use rivet_envoy_client::config::{ActorName as EnvoyActorName, EnvoyConfig};
use rivet_envoy_client::envoy::start_envoy as start_envoy_client;
use rivet_envoy_client::handle::EnvoyHandle;
use rivet_envoy_client::protocol;
use rivetkit_shared_types::serverless_metadata::{
	ActorName, ServerlessMetadataEnvoy, ServerlessMetadataEnvoyKind, ServerlessMetadataPayload,
};
use serde::Serialize;
use serde_json::json;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::actor::factory::ActorFactory;
#[cfg(feature = "native-runtime")]
use crate::engine_process::{EngineProcessManager, EngineResolverConfig};
use crate::registry::{
	CoreEnvoyHandle, CoreEnvoyStatus, RegistryCallbacks, RegistryDispatcher, ServeConfig,
	should_manage_engine,
};
use crate::runtime::RuntimeSpawner;
use crate::time::{sleep, timeout};

const DEFAULT_BASE_PATH: &str = "/api/rivet";
const SSE_PING_INTERVAL: Duration = Duration::from_secs(1);
const SSE_PING_FRAME: &[u8] = b"event: ping\ndata:\n\n";
const SSE_STOPPING_FRAME: &[u8] = b"event: stopping\ndata:\n\n";
/// Bound on `handle.shutdown_and_wait` inside teardown paths. If envoy cannot
/// reach the engine (reconnect loop stuck), we fall back to immediate `Stop`
/// rather than hanging indefinitely. Must stay below the outer TS grace ceiling.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct CoreServerlessRuntime {
	settings: Arc<ServerlessSettings>,
	dispatcher: Arc<RegistryDispatcher>,
	envoy: Arc<TokioMutex<Option<EnvoyHandle>>>,
	#[cfg(feature = "native-runtime")]
	_engine_process: Arc<TokioMutex<Option<EngineProcessManager>>>,
	shutting_down: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
struct ServerlessSettings {
	version: u32,
	configured_endpoint: String,
	configured_namespace: String,
	base_path: String,
	package_version: String,
	client_endpoint: Option<String>,
	client_namespace: Option<String>,
	client_token: Option<String>,
	validate_endpoint: bool,
	max_start_payload_bytes: usize,
	cache_envoy: bool,
}

#[derive(Debug)]
pub struct ServerlessRequest {
	pub method: String,
	pub url: String,
	pub headers: HashMap<String, String>,
	pub body: Vec<u8>,
	pub cancel_token: CancellationToken,
}

#[derive(Debug)]
pub struct ServerlessResponse {
	pub status: u16,
	pub headers: HashMap<String, String>,
	pub body: mpsc::UnboundedReceiver<Result<Vec<u8>, ServerlessStreamError>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerlessStreamError {
	pub group: String,
	pub code: String,
	pub message: String,
}

#[derive(Debug)]
struct StartHeaders {
	endpoint: String,
	token: Option<String>,
	pool_name: String,
	namespace: String,
}

#[derive(Debug, Serialize)]
struct ServerlessErrorBody<'a> {
	group: &'a str,
	code: &'a str,
	message: String,
	metadata: serde_json::Value,
}

#[derive(rivet_error::RivetError, Serialize)]
#[error("request", "invalid", "Invalid request.", "Invalid request: {reason}")]
struct InvalidRequest {
	reason: String,
}

#[derive(rivet_error::RivetError, Serialize)]
#[error(
	"config",
	"endpoint_mismatch",
	"Endpoint mismatch.",
	"Endpoint mismatch: expected \"{expected}\", received \"{received}\""
)]
struct EndpointMismatch {
	expected: String,
	received: String,
}

#[derive(rivet_error::RivetError, Serialize)]
#[error(
	"config",
	"namespace_mismatch",
	"Namespace mismatch.",
	"Namespace mismatch: expected \"{expected}\", received \"{received}\""
)]
struct NamespaceMismatch {
	expected: String,
	received: String,
}

#[derive(rivet_error::RivetError, Serialize)]
#[error(
	"message",
	"incoming_too_long",
	"Incoming message too long.",
	"Incoming message too long. Exceeded limit of {limit} bytes."
)]
struct IncomingMessageTooLong {
	limit: usize,
}

#[derive(rivet_error::RivetError, Serialize)]
#[error(
	"registry",
	"shut_down",
	"Registry is shut down.",
	"Registry is shut down; no new requests can be accepted."
)]
struct RuntimeShutDown;

impl CoreServerlessRuntime {
	pub(crate) async fn new(
		factories: HashMap<String, Arc<ActorFactory>>,
		config: ServeConfig,
	) -> Result<Self> {
		#[cfg(feature = "native-runtime")]
		let engine_process = if should_manage_engine(&config.endpoint, config.engine_spawn)? {
			Some(
				EngineProcessManager::start_or_reuse(EngineResolverConfig::from_parts(
					&config.endpoint,
					config.engine_binary_path.clone(),
					config.engine_host.clone(),
					config.engine_port,
					config.engine_auto_download,
				))
				.await?,
			)
		} else {
			None
		};
		#[cfg(not(feature = "native-runtime"))]
		if should_manage_engine(&config.endpoint, config.engine_spawn)? {
			anyhow::bail!("engine process spawning requires the `native-runtime` feature");
		}

		let dispatcher = Arc::new(RegistryDispatcher::new(
			factories,
			config.handle_inspector_http_in_runtime,
		));
		let base_path = normalize_base_path(config.serverless_base_path.as_deref());
		crate::metrics_endpoint::record_rivetkit_info(
			config.serverless_package_version.clone(),
			config.version,
			"serverless",
			config.pool_name.clone(),
		);

		Ok(Self {
			settings: Arc::new(ServerlessSettings {
				version: config.version,
				configured_endpoint: config.endpoint,
				configured_namespace: config.namespace,
				base_path,
				package_version: config.serverless_package_version,
				client_endpoint: config.serverless_client_endpoint,
				client_namespace: config.serverless_client_namespace,
				client_token: config.serverless_client_token,
				validate_endpoint: config.serverless_validate_endpoint,
				max_start_payload_bytes: config.serverless_max_start_payload_bytes,
				cache_envoy: config.serverless_cache_envoy,
			}),
			dispatcher,
			envoy: Arc::new(TokioMutex::new(None)),
			#[cfg(feature = "native-runtime")]
			_engine_process: Arc::new(TokioMutex::new(engine_process)),
			shutting_down: Arc::new(AtomicBool::new(false)),
		})
	}

	/// Tear down the cached envoy handle. Idempotent.
	///
	/// Sets `shutting_down` so concurrent `ensure_envoy` callers short-circuit
	/// instead of starting a fresh envoy after teardown, and waits (with a
	/// bounded timeout) for `envoy_loop` to exit. If the drain exceeds the
	/// timeout (e.g. engine unreachable), falls back to an immediate `Stop`.
	pub async fn shutdown(&self) {
		self.shutting_down.store(true, Ordering::Release);
		let handle = { self.envoy.lock().await.take() };
		let Some(handle) = handle else { return };
		match timeout(SHUTDOWN_DRAIN_TIMEOUT, handle.shutdown_and_wait(false)).await {
			Ok(()) => {}
			Err(_) => {
				tracing::warn!(
					"serverless runtime envoy drain exceeded timeout; forcing immediate stop"
				);
				handle.shutdown(true);
				handle.wait_stopped().await;
			}
		}
	}

	pub async fn active_envoy_actor_count(&self) -> Option<usize> {
		self.active_envoy_status()
			.await
			.map(|status| status.active_actor_count)
	}

	pub async fn active_envoy_status(&self) -> Option<CoreEnvoyStatus> {
		self.envoy
			.lock()
			.await
			.as_ref()
			.map(|handle| CoreEnvoyHandle::new(handle.clone()).status())
	}

	pub async fn active_envoy_actor_stop_threshold_ms(&self) -> Option<i64> {
		let handle = self.envoy.lock().await.as_ref().cloned()?;
		CoreEnvoyHandle::new(handle).actor_stop_threshold_ms().await
	}

	/// Listener-side body cap; reuses the `/start` payload limit.
	pub fn max_request_body_bytes(&self) -> usize {
		self.settings.max_start_payload_bytes
	}

	/// Returns whether the native listener should reserve this URL for
	/// RivetKit's framework routes instead of forwarding it to an application
	/// fallback.
	pub fn handles_listener_request(&self, url: &str) -> bool {
		handles_listener_request(&self.settings.base_path, url)
	}

	/// Canonical 413 response built through the `RivetError` system.
	pub fn incoming_too_long_response(&self) -> ServerlessResponse {
		let error = IncomingMessageTooLong {
			limit: self.settings.max_start_payload_bytes,
		}
		.build();
		error_response(error)
	}

	/// Canonical 400 response for malformed requests.
	pub fn invalid_request_response(&self, reason: impl Into<String>) -> ServerlessResponse {
		let error = InvalidRequest {
			reason: reason.into(),
		}
		.build();
		error_response(error)
	}

	pub async fn handle_request(&self, req: ServerlessRequest) -> ServerlessResponse {
		let cors = cors_headers(&req);
		match self.handle_request_inner(req).await {
			Ok(mut response) => {
				apply_cors(&mut response.headers, cors);
				response
			}
			Err(error) => {
				let mut response = error_response(error);
				apply_cors(&mut response.headers, cors);
				response
			}
		}
	}

	async fn handle_request_inner(&self, req: ServerlessRequest) -> Result<ServerlessResponse> {
		let path = route_path(&self.settings.base_path, &req.url)?;
		match (req.method.as_str(), path.as_str()) {
			("GET", "") | ("GET", "/") => Ok(text_response(
				StatusCode::OK,
				"text/plain; charset=utf-8",
				"This is a RivetKit server.\n\nLearn more at https://rivet.dev",
			)),
			("GET", "/health") => {
				// Healthy if no envoy is connected yet or if the envoy has received a
				// recent engine ping. Unhealthy only when an envoy exists but has not
				// received a recent ping. 503 is the conventional "recycle me" signal
				// for container hosts running behind an HTTP health probe.
				let runtime_healthy = {
					let guard = self.envoy.lock().await;
					guard
						.as_ref()
						.map(|handle| handle.is_ping_healthy())
						.unwrap_or(true)
				};
				if runtime_healthy {
					Ok(json_response(
						StatusCode::OK,
						json!({
							"status": "ok",
							"runtime": "rivetkit",
							"version": self.settings.package_version,
						}),
					))
				} else {
					Ok(json_response(
						StatusCode::SERVICE_UNAVAILABLE,
						json!({
							"status": "engine_ping_stale",
							"runtime": "rivetkit",
							"version": self.settings.package_version,
						}),
					))
				}
			}
			("GET", "/metadata") => Ok(self.metadata_response()),
			("GET", "/metrics") => Ok(metrics_response(&req.headers)),
			("GET", "/start") | ("POST", "/start") => self.start_response(req).await,
			("OPTIONS", _) => Ok(bytes_response(
				StatusCode::NO_CONTENT,
				HashMap::new(),
				Vec::new(),
			)),
			_ => Ok(text_response(
				StatusCode::NOT_FOUND,
				"text/plain; charset=utf-8",
				"Not Found (RivetKit)",
			)),
		}
	}

	async fn start_response(&self, req: ServerlessRequest) -> Result<ServerlessResponse> {
		let headers = parse_start_headers(&req.headers)?;
		self.validate_start_headers(&headers)?;
		crate::metrics_endpoint::record_rivetkit_info(
			self.settings.package_version.clone(),
			self.settings.version,
			"serverless",
			headers.pool_name.clone(),
		);
		if req.body.len() > self.settings.max_start_payload_bytes {
			return Err(IncomingMessageTooLong {
				limit: self.settings.max_start_payload_bytes,
			}
			.build());
		}

		let handle = self.ensure_envoy(&headers).await?;
		let payload = req.body;
		let actor_start = handle.decode_serverless_actor_start(&payload)?;
		let cancel_token = req.cancel_token;
		let cache_envoy = self.settings.cache_envoy;
		let (tx, rx) = mpsc::unbounded_channel();
		let _ = tx.send(Ok(SSE_PING_FRAME.to_vec()));

		RuntimeSpawner::spawn(async move {
			let shutdown_handle = handle.clone();
			let result = tokio::select! {
				_ = cancel_token.cancelled() => {
					if !cache_envoy {
						shutdown_handle.shutdown_and_wait(false).await;
					}
					return;
				}
				result = handle.start_serverless_actor(&payload) => result,
			};
			if let Err(error) = result {
				let error = stream_error(error);
				let _ = tx.send(Err(error));
				if !cache_envoy {
					handle.shutdown_and_wait(false).await;
				}
				return;
			}

			loop {
				tokio::select! {
					_ = cancel_token.cancelled() => {
						break;
					}
					_ = handle.wait_actor_registered_then_stopped(&actor_start.actor_id, actor_start.generation) => {
						let _ = tx.send(Ok(SSE_STOPPING_FRAME.to_vec()));
						break;
					}
					_ = sleep(SSE_PING_INTERVAL) => {
						if tx.send(Ok(SSE_PING_FRAME.to_vec())).is_err() {
							break;
						}
					}
				}
			}

			if !cache_envoy {
				handle.shutdown_and_wait(false).await;
			}
		});

		Ok(ServerlessResponse {
			status: StatusCode::OK.as_u16(),
			headers: HashMap::from([
				("content-type".to_owned(), "text/event-stream".to_owned()),
				("cache-control".to_owned(), "no-cache".to_owned()),
				("connection".to_owned(), "keep-alive".to_owned()),
			]),
			body: rx,
		})
	}

	fn metadata_response(&self) -> ServerlessResponse {
		let actor_names = self
			.dispatcher
			.build_actor_metadata_map()
			.into_iter()
			.map(|(name, metadata)| {
				(
					name,
					ActorName {
						metadata: Some(metadata),
					},
				)
			})
			.collect::<HashMap<_, _>>();

		let payload = ServerlessMetadataPayload {
			runtime: "rivetkit".to_owned(),
			version: self.settings.package_version.clone(),
			envoy_protocol_version: Some(protocol::PROTOCOL_VERSION),
			actor_names,
			envoy: Some(ServerlessMetadataEnvoy {
				kind: Some(ServerlessMetadataEnvoyKind::Serverless {}),
				version: Some(self.settings.version),
			}),
			runner: None,
			client_endpoint: self.settings.client_endpoint.clone(),
			client_namespace: self.settings.client_namespace.clone(),
			client_token: self.settings.client_token.clone(),
		};

		let response = serde_json::to_value(payload).unwrap_or_else(|_| json!({}));

		json_response(StatusCode::OK, response)
	}

	fn validate_start_headers(&self, headers: &StartHeaders) -> Result<()> {
		if self.settings.validate_endpoint {
			if !endpoints_match(&headers.endpoint, &self.settings.configured_endpoint) {
				tracing::warn!(
					configured_endpoint = %self.settings.configured_endpoint,
					received_endpoint = %headers.endpoint,
					"serverless start rejected: endpoint mismatch",
				);
				return Err(EndpointMismatch {
					expected: self.settings.configured_endpoint.clone(),
					received: headers.endpoint.clone(),
				}
				.build());
			}

			if headers.namespace != self.settings.configured_namespace {
				tracing::warn!(
					configured_namespace = %self.settings.configured_namespace,
					received_namespace = %headers.namespace,
					"serverless start rejected: namespace mismatch",
				);
				return Err(NamespaceMismatch {
					expected: self.settings.configured_namespace.clone(),
					received: headers.namespace.clone(),
				}
				.build());
			}
		}

		Ok(())
	}

	async fn ensure_envoy(&self, headers: &StartHeaders) -> Result<EnvoyHandle> {
		if self.shutting_down.load(Ordering::Acquire) {
			return Err(RuntimeShutDown.build());
		}
		if !self.settings.cache_envoy {
			return self.start_envoy(headers).await;
		}
		let mut guard = self.envoy.lock().await;
		if let Some(handle) = guard.as_ref() {
			// The start request token authenticates the serverless callback. It is not part
			// of envoy identity, and may differ from the token used for the engine connection.
			if !endpoints_match(handle.endpoint(), &headers.endpoint)
				|| handle.namespace() != headers.namespace
				|| handle.pool_name() != headers.pool_name
			{
				anyhow::bail!("serverless start headers do not match active envoy");
			}
			return Ok(handle.clone());
		}

		let handle = self.start_envoy(headers).await?;
		// Re-check under the lock: shutdown may have run while we were awaiting
		// `start_envoy`. If so, tear down the freshly-built envoy rather than
		// installing it into the cache.
		if self.shutting_down.load(Ordering::Acquire) {
			drop(guard);
			match timeout(SHUTDOWN_DRAIN_TIMEOUT, handle.shutdown_and_wait(false)).await {
				Ok(()) => {}
				Err(_) => {
					handle.shutdown(true);
					handle.wait_stopped().await;
				}
			}
			return Err(RuntimeShutDown.build());
		}
		*guard = Some(handle.clone());
		Ok(handle)
	}

	async fn start_envoy(&self, headers: &StartHeaders) -> Result<EnvoyHandle> {
		let callbacks = Arc::new(RegistryCallbacks {
			dispatcher: self.dispatcher.clone(),
		});
		let prepopulate_actor_names = self
			.dispatcher
			.build_actor_metadata_map()
			.into_iter()
			.map(|(name, metadata)| (name, EnvoyActorName { metadata }))
			.collect();
		// not_global: true to avoid caching the handle in the process-wide
		// `GLOBAL_ENVOY` OnceLock. Without this, a shutdown-during-build race
		// (spec §3 step 7) leaves a dead handle cached for the life of the
		// process and any subsequent consumer gets it back.
		Ok(start_envoy_client(EnvoyConfig {
			version: self.settings.version,
			endpoint: headers.endpoint.clone(),
			token: headers.token.clone(),
			namespace: headers.namespace.clone(),
			pool_name: headers.pool_name.clone(),
			prepopulate_actor_names,
			metadata: Some(json!({
				"rivetkit": { "version": self.settings.package_version },
			})),
			not_global: true,
			debug_latency_ms: None,
			callbacks,
		})
		.await)
	}
}

fn route_path(base_path: &str, url: &str) -> Result<String> {
	let parsed = Url::parse(url).with_context(|| format!("parse request URL `{url}`"))?;
	let path = parsed.path();
	if path == base_path {
		return Ok(String::new());
	}
	let prefix = format!("{base_path}/");
	if let Some(rest) = path.strip_prefix(&prefix) {
		return Ok(format!("/{rest}"));
	}
	Ok(path.to_owned())
}

fn handles_listener_request(base_path: &str, url: &str) -> bool {
	let Ok(parsed) = Url::parse(url) else {
		// Let the normal framework handler return its structured invalid URL
		// response rather than passing malformed input to user code.
		return true;
	};
	let request_path = parsed.path();
	if request_path != base_path && !request_path.starts_with(&format!("{base_path}/")) {
		return false;
	}
	let path = route_path(base_path, url).expect("URL was parsed and the same base path is valid");
	matches!(
		path.as_str(),
		"" | "/" | "/health" | "/metadata" | "/metrics" | "/start"
	)
}

fn parse_start_headers(headers: &HashMap<String, String>) -> Result<StartHeaders> {
	let pool_name = match optional_header(headers, "x-rivet-pool-name") {
		Some(pool_name) => pool_name,
		None => optional_header(headers, "x-rivet-runner-name").ok_or_else(|| {
			InvalidRequest {
				reason: "x-rivet-pool-name header is required".to_string(),
			}
			.build()
		})?,
	};

	Ok(StartHeaders {
		endpoint: required_header(headers, "x-rivet-endpoint")?,
		token: optional_header(headers, "x-rivet-token"),
		pool_name,
		namespace: required_header(headers, "x-rivet-namespace-name")?,
	})
}

fn required_header(headers: &HashMap<String, String>, name: &str) -> Result<String> {
	headers
		.get(name)
		.filter(|value| !value.is_empty())
		.cloned()
		.ok_or_else(|| {
			InvalidRequest {
				reason: format!("{name} header is required"),
			}
			.build()
		})
}

fn optional_header(headers: &HashMap<String, String>, name: &str) -> Option<String> {
	headers.get(name).filter(|value| !value.is_empty()).cloned()
}

fn cors_headers(req: &ServerlessRequest) -> HashMap<String, String> {
	let origin = req
		.headers
		.get("origin")
		.cloned()
		.unwrap_or_else(|| "*".to_owned());
	let mut headers = HashMap::from([
		("access-control-allow-origin".to_owned(), origin.clone()),
		(
			"access-control-allow-credentials".to_owned(),
			"true".to_owned(),
		),
		("access-control-expose-headers".to_owned(), "*".to_owned()),
	]);
	if origin != "*" {
		headers.insert("vary".to_owned(), "Origin".to_owned());
	}

	if req.method == "OPTIONS" {
		headers.insert(
			"access-control-allow-methods".to_owned(),
			"GET, POST, PUT, DELETE, OPTIONS, PATCH".to_owned(),
		);
		headers.insert(
			"access-control-allow-headers".to_owned(),
			req.headers
				.get("access-control-request-headers")
				.cloned()
				.unwrap_or_else(|| "*".to_owned()),
		);
		headers.insert("access-control-max-age".to_owned(), "86400".to_owned());
	}

	headers
}

fn apply_cors(headers: &mut HashMap<String, String>, cors: HashMap<String, String>) {
	headers.extend(cors);
}

fn normalize_base_path(base_path: Option<&str>) -> String {
	let base_path = base_path
		.filter(|base_path| !base_path.is_empty())
		.unwrap_or(DEFAULT_BASE_PATH);
	let prefixed = if base_path.starts_with('/') {
		base_path.to_owned()
	} else {
		format!("/{base_path}")
	};
	let trimmed = prefixed.trim_end_matches('/');
	if trimmed.is_empty() {
		"/".to_owned()
	} else {
		trimmed.to_owned()
	}
}

fn text_response(status: StatusCode, content_type: &str, body: &str) -> ServerlessResponse {
	bytes_response(
		status,
		HashMap::from([("content-type".to_owned(), content_type.to_owned())]),
		body.as_bytes().to_vec(),
	)
}

fn json_response(status: StatusCode, body: serde_json::Value) -> ServerlessResponse {
	bytes_response(
		status,
		HashMap::from([("content-type".to_owned(), "application/json".to_owned())]),
		serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec()),
	)
}

fn metrics_response(headers: &HashMap<String, String>) -> ServerlessResponse {
	let bearer_token = crate::metrics_endpoint::authorization_bearer_token_map(headers);
	match crate::metrics_endpoint::authorize_metrics_request(bearer_token) {
		Ok(()) => match crate::metrics_endpoint::render_prometheus_metrics() {
			Ok(metrics) => bytes_response(
				StatusCode::OK,
				HashMap::from([("content-type".to_owned(), metrics.content_type)]),
				metrics.body,
			),
			Err(error) => error_response(error),
		},
		Err(crate::metrics_endpoint::MetricsAccessError::NotEnabled) => text_response(
			StatusCode::FORBIDDEN,
			"text/plain; charset=utf-8",
			"metrics not enabled\n",
		),
		Err(crate::metrics_endpoint::MetricsAccessError::Unauthorized) => text_response(
			StatusCode::UNAUTHORIZED,
			"text/plain; charset=utf-8",
			"metrics request requires a valid bearer token\n",
		),
	}
}

fn bytes_response(
	status: StatusCode,
	headers: HashMap<String, String>,
	body: Vec<u8>,
) -> ServerlessResponse {
	let (tx, rx) = mpsc::unbounded_channel();
	let _ = tx.send(Ok(body));
	ServerlessResponse {
		status: status.as_u16(),
		headers,
		body: rx,
	}
}

fn error_response(error: anyhow::Error) -> ServerlessResponse {
	let extracted = rivet_error::RivetError::extract(&error);
	let status = serverless_error_status(extracted.group(), extracted.code());
	let body = ServerlessErrorBody {
		group: extracted.group(),
		code: extracted.code(),
		message: extracted.message().to_owned(),
		metadata: extracted.metadata().unwrap_or(serde_json::Value::Null),
	};
	bytes_response(
		status,
		HashMap::from([("content-type".to_owned(), "application/json".to_owned())]),
		serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec()),
	)
}

fn serverless_error_status(group: &str, code: &str) -> StatusCode {
	match (group, code) {
		("auth", "forbidden") => StatusCode::FORBIDDEN,
		("message", "incoming_too_long") => StatusCode::PAYLOAD_TOO_LARGE,
		_ => StatusCode::BAD_REQUEST,
	}
}

fn stream_error(error: anyhow::Error) -> ServerlessStreamError {
	let extracted = rivet_error::RivetError::extract(&error);
	ServerlessStreamError {
		group: extracted.group().to_owned(),
		code: extracted.code().to_owned(),
		message: extracted.message().to_owned(),
	}
}

pub fn normalize_endpoint_url(url: &str) -> Option<String> {
	let parsed = Url::parse(url).ok()?;
	let pathname = if parsed.path() == "/" {
		"/".to_owned()
	} else {
		parsed.path().trim_end_matches('/').to_owned()
	};
	let mut hostname = parsed.host_str()?.to_owned();
	if is_loopback_address(&hostname) {
		hostname = "localhost".to_owned();
	}
	hostname = normalize_regional_hostname(&hostname);
	let host = match parsed.port() {
		Some(port) => format!("{hostname}:{port}"),
		None => hostname,
	};
	Some(format!("{}://{}{}", parsed.scheme(), host, pathname))
}

fn normalized_endpoint_candidates(value: &str) -> Vec<String> {
	value
		.split(',')
		.map(str::trim)
		.filter(|candidate| !candidate.is_empty())
		.map(|candidate| normalize_endpoint_url(candidate).unwrap_or_else(|| candidate.to_owned()))
		.collect()
}

pub fn endpoints_match(a: &str, b: &str) -> bool {
	let a_candidates = normalized_endpoint_candidates(a);
	let b_candidates = normalized_endpoint_candidates(b);
	a_candidates.iter().any(|a_candidate| {
		b_candidates
			.iter()
			.any(|b_candidate| a_candidate == b_candidate)
	})
}

fn normalize_regional_hostname(hostname: &str) -> String {
	if !hostname.ends_with(".rivet.dev") || !hostname.starts_with("api-") {
		return hostname.to_owned();
	}
	let without_prefix = &hostname[4..];
	let Some(first_dot_index) = without_prefix.find('.') else {
		return hostname.to_owned();
	};
	let domain = &without_prefix[first_dot_index + 1..];
	format!("api.{domain}")
}

fn is_loopback_address(hostname: &str) -> bool {
	matches!(hostname, "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]")
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../tests/serverless.rs"]
mod tests;
