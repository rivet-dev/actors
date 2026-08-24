use super::*;

mod moved_tests {
	use std::collections::HashMap;
	#[cfg(not(feature = "native-runtime"))]
	use std::path::PathBuf;

	use tokio_util::sync::CancellationToken;

	use super::{
		CoreServerlessRuntime, ServerlessRequest, endpoints_match, handles_listener_request,
		normalize_endpoint_url, parse_start_headers,
	};
	use crate::registry::{EngineSpawnMode, ServeConfig};

	#[test]
	fn normalizes_loopback_addresses() {
		assert_eq!(
			normalize_endpoint_url("http://127.0.0.1:6420/").as_deref(),
			Some("http://localhost:6420/")
		);
		assert!(endpoints_match(
			"http://0.0.0.0:6420/api/",
			"http://localhost:6420/api"
		));
	}

	#[test]
	fn listener_only_reserves_framework_routes_below_base_path() {
		assert!(handles_listener_request(
			"/api/rivet",
			"http://internal/api/rivet/metadata"
		));
		assert!(!handles_listener_request(
			"/api/rivet",
			"http://internal/health"
		));
		assert!(!handles_listener_request(
			"/api/rivet",
			"http://internal/api/rivet/application"
		));
	}

	#[test]
	fn normalizes_rivet_regional_hosts() {
		assert!(endpoints_match(
			"https://api-us-west-1.rivet.dev",
			"https://api.rivet.dev/"
		));
		assert!(endpoints_match(
			"https://api-lax.staging.rivet.dev",
			"https://api.staging.rivet.dev/"
		));
		assert!(!endpoints_match(
			"https://api-us-west-1.example.com",
			"https://api.example.com"
		));
	}

	#[test]
	fn invalid_urls_fall_back_to_string_comparison() {
		assert!(endpoints_match("not a url", "not a url"));
		assert!(!endpoints_match("not a url", "also not a url"));
	}

	#[test]
	fn matches_combined_duplicate_endpoint_headers() {
		assert!(endpoints_match(
			"http://127.0.0.1:6420, http://127.0.0.1:8080",
			"http://localhost:8080/"
		));
		assert!(!endpoints_match(
			"http://127.0.0.1:6420, http://127.0.0.1:8080",
			"http://localhost:9000/"
		));
	}

	#[tokio::test]
	async fn handles_basic_routes() {
		let runtime = test_runtime().await;

		// A fresh runtime has no envoy yet, and the health endpoint treats
		// "no envoy connected" as healthy so container hosts do not recycle a
		// runtime that has simply not received its first /start request. The
		// unhealthy 503 path is covered by
		// `health_reports_engine_ping_stale_when_envoy_ping_missing`.
		let health = runtime
			.handle_request(test_request("GET", "/api/rivet/health"))
			.await;
		assert_eq!(health.status, 200);
		let health_body = read_body(health).await;
		assert_eq!(health_body["status"], "ok");
		assert_eq!(health_body["runtime"], "rivetkit");
		assert_eq!(health_body["version"], "test-version");

		let metadata = runtime
			.handle_request(test_request("GET", "/api/rivet/metadata"))
			.await;
		assert_eq!(metadata.status, 200);
		let metadata_body = read_body(metadata).await;
		assert_eq!(metadata_body["runtime"], "rivetkit");
		assert_eq!(metadata_body["version"], "test-version");
		assert_eq!(
			metadata_body["envoy"]["kind"]["serverless"],
			serde_json::json!({})
		);
		assert_eq!(metadata_body["clientEndpoint"], "http://client.example");
		assert_eq!(metadata_body["clientNamespace"], "default");
		assert_eq!(metadata_body["clientToken"], "client-token");

		let root = runtime
			.handle_request(test_request("GET", "/api/rivet"))
			.await;
		assert_eq!(root.status, 200);
		let root_body = read_text(root).await;
		assert_eq!(
			root_body,
			"This is a RivetKit server.\n\nLearn more at https://rivet.dev"
		);
	}

	#[tokio::test]
	async fn start_requires_serverless_headers() {
		let runtime = test_runtime().await;
		let response = runtime
			.handle_request(test_request("POST", "/api/rivet/start"))
			.await;
		assert_eq!(response.status, 400);
		let body = read_body(response).await;
		assert_eq!(body["group"], "request");
		assert_eq!(body["code"], "invalid");
	}

	#[test]
	fn start_headers_do_not_require_token() {
		let headers = HashMap::from([
			(
				"x-rivet-endpoint".to_owned(),
				"http://127.0.0.1:6420".to_owned(),
			),
			("x-rivet-pool-name".to_owned(), "default".to_owned()),
			("x-rivet-namespace-name".to_owned(), "default".to_owned()),
		]);

		let parsed = parse_start_headers(&headers).expect("headers should parse");

		assert_eq!(parsed.token, None);
	}

	#[test]
	fn start_headers_only_use_x_rivet_token() {
		let headers = HashMap::from([
			(
				"x-rivet-endpoint".to_owned(),
				"http://127.0.0.1:6420".to_owned(),
			),
			("authorization".to_owned(), "Bearer fallback".to_owned()),
			("x-rivet-pool-name".to_owned(), "default".to_owned()),
			("x-rivet-namespace-name".to_owned(), "default".to_owned()),
		]);

		let parsed = parse_start_headers(&headers).expect("headers should parse");
		assert_eq!(parsed.token, None);

		let mut headers = headers;
		headers.insert("x-rivet-token".to_owned(), "dev".to_owned());
		let parsed = parse_start_headers(&headers).expect("headers should parse");
		assert_eq!(parsed.token.as_deref(), Some("dev"));
	}

	#[cfg(not(feature = "native-runtime"))]
	#[tokio::test]
	async fn engine_process_spawn_requires_native_runtime() {
		let mut config = test_config();
		config.engine_binary_path = Some(PathBuf::from("rivet-engine"));
		config.engine_spawn = EngineSpawnMode::Always;

		let error = match CoreServerlessRuntime::new(HashMap::new(), config).await {
			Ok(_) => panic!("engine process spawning should fail without native runtime"),
			Err(error) => error,
		};

		assert!(
			error
				.to_string()
				.contains("engine process spawning requires the `native-runtime` feature")
		);
	}

	mod health_envoy {
		use std::collections::HashMap;
		use std::sync::atomic::Ordering;
		use std::sync::{Arc, Mutex as EnvoySharedMutex, atomic::AtomicBool, atomic::AtomicI64};

		use rivet_envoy_client::config::{
			BoxFuture, EnvoyCallbacks, EnvoyConfig, HttpRequest, HttpResponse, WebSocketHandler,
			WebSocketSender,
		};
		use rivet_envoy_client::context::{SharedContext, WsTxMessage};
		use rivet_envoy_client::handle::EnvoyHandle;
		use rivet_envoy_client::protocol;
		use tokio::sync::mpsc;

		use super::{read_body, test_request, test_runtime};

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
				Box::pin(async { anyhow::bail!("fetch should not run in health tests") })
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
				Box::pin(async { anyhow::bail!("websocket should not run in health tests") })
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

		fn test_envoy_handle() -> (EnvoyHandle, Arc<SharedContext>) {
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
					callbacks: Arc::new(IdleEnvoyCallbacks),
				},
				envoy_key: "test-envoy".to_string(),
				envoy_tx,
				actors: Arc::new(EnvoySharedMutex::new(HashMap::new())),
				actors_notify: Arc::new(tokio::sync::Notify::new()),
				live_tunnel_requests: Arc::new(EnvoySharedMutex::new(HashMap::new())),
				pending_hibernation_restores: Arc::new(EnvoySharedMutex::new(HashMap::new())),
				ws_tx: Arc::new(tokio::sync::Mutex::new(
					None::<mpsc::UnboundedSender<WsTxMessage>>,
				)),
				connection_session: std::sync::atomic::AtomicU64::new(1),
				next_connection_session: std::sync::atomic::AtomicU64::new(1),
				connection_session_tx: tokio::sync::watch::channel(1).0,
				protocol_metadata: Arc::new(tokio::sync::Mutex::new(None)),
				shutting_down: AtomicBool::new(false),
				// Zero means no engine ping has been received yet, which
				// `is_ping_healthy` treats as unhealthy.
				last_ping_ts: AtomicI64::new(0),
				stopped_tx: tokio::sync::watch::channel(true).0,
			});

			(EnvoyHandle::from_shared(shared.clone()), shared)
		}

		fn now_epoch_millis() -> i64 {
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.expect("system clock should be after the epoch")
				.as_millis() as i64
		}

		#[tokio::test]
		async fn health_reports_engine_ping_stale_when_envoy_ping_missing() {
			let runtime = test_runtime().await;
			let (handle, shared) = test_envoy_handle();
			*runtime.envoy.lock().await = Some(handle);

			// An envoy exists but the engine has never completed the ping
			// handshake, so the runtime must report 503 to get recycled.
			let health = runtime
				.handle_request(test_request("GET", "/api/rivet/health"))
				.await;
			assert_eq!(health.status, 503);
			let health_body = read_body(health).await;
			assert_eq!(health_body["status"], "engine_ping_stale");
			assert_eq!(health_body["runtime"], "rivetkit");
			assert_eq!(health_body["version"], "test-version");

			// A recent engine ping flips the same runtime back to healthy.
			shared
				.last_ping_ts
				.store(now_epoch_millis(), Ordering::Release);
			let health = runtime
				.handle_request(test_request("GET", "/api/rivet/health"))
				.await;
			assert_eq!(health.status, 200);
			let health_body = read_body(health).await;
			assert_eq!(health_body["status"], "ok");
		}
	}

	async fn test_runtime() -> CoreServerlessRuntime {
		CoreServerlessRuntime::new(HashMap::new(), test_config())
			.await
			.expect("runtime should build")
	}

	fn test_config() -> ServeConfig {
		ServeConfig {
			version: 1,
			endpoint: "http://127.0.0.1:6420".to_owned(),
			token: Some("dev".to_owned()),
			namespace: "default".to_owned(),
			pool_name: "default".to_owned(),
			engine_binary_path: None,
			engine_host: None,
			engine_port: None,
			engine_spawn: EngineSpawnMode::Never,
			engine_auto_download: false,
			handle_inspector_http_in_runtime: true,
			serverless_base_path: Some("/api/rivet".to_owned()),
			serverless_package_version: "test-version".to_owned(),
			serverless_client_endpoint: Some("http://client.example".to_owned()),
			serverless_client_namespace: Some("default".to_owned()),
			serverless_client_token: Some("client-token".to_owned()),
			serverless_validate_endpoint: true,
			serverless_max_start_payload_bytes: 1_048_576,
			serverless_cache_envoy: true,
		}
	}

	fn test_request(method: &str, path: &str) -> ServerlessRequest {
		ServerlessRequest {
			method: method.to_owned(),
			url: format!("http://localhost{path}"),
			headers: HashMap::new(),
			body: Vec::new(),
			cancel_token: CancellationToken::new(),
		}
	}

	async fn read_body(response: super::ServerlessResponse) -> serde_json::Value {
		let text = read_text(response).await;
		serde_json::from_str(&text).expect("response should be json")
	}

	async fn read_text(mut response: super::ServerlessResponse) -> String {
		let mut body = Vec::new();
		while let Some(chunk) = response.body.recv().await {
			body.extend(chunk.expect("stream should not error"));
		}
		String::from_utf8(body).expect("response should be utf-8")
	}
}
