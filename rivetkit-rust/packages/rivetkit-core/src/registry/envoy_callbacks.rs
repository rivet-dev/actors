use tracing::Instrument;

use super::*;
use crate::error::ActorRuntime;
use crate::runtime::RuntimeSpawner;

impl EnvoyCallbacks for RegistryCallbacks {
	fn on_actor_start(
		&self,
		handle: EnvoyHandle,
		actor_id: String,
		generation: u32,
		config: protocol::ActorConfig,
		_preloaded_kv: Option<protocol::PreloadedKv>,
	) -> EnvoyBoxFuture<anyhow::Result<()>> {
		let dispatcher = self.dispatcher.clone();
		let actor_name = config.name.clone();
		let key = actor_key_from_protocol(config.key.clone());
		let input = config.input.clone();
		let factory = dispatcher.factories.get(&actor_name).cloned();

		Box::pin(async move {
			let factory = factory.ok_or_else(|| {
				ActorRuntime::NotRegistered {
					actor_name: actor_name.clone(),
				}
				.build()
			})?;
			let ctx = dispatcher.build_actor_context(
				handle,
				&actor_id,
				generation,
				&actor_name,
				key,
				factory.as_ref(),
			)?;

			dispatcher
				.start_actor(StartActorRequest {
					actor_id: actor_id.clone(),
					generation,
					actor_name,
					input,
					ctx,
				})
				.await?;

			Ok(())
		})
	}

	fn on_actor_stop_with_completion(
		&self,
		_handle: EnvoyHandle,
		actor_id: String,
		_generation: u32,
		reason: protocol::StopActorReason,
		stop_handle: ActorStopHandle,
	) -> EnvoyBoxFuture<anyhow::Result<()>> {
		let dispatcher = self.dispatcher.clone();
		Box::pin(async move {
			RuntimeSpawner::spawn(
				async move {
					if let Err(error) = dispatcher.stop_actor(&actor_id, reason, stop_handle).await
					{
						tracing::error!(
							?error,
							"actor stop failed after asynchronous completion handoff",
						);
					}
				}
				.in_current_span(),
			);
			Ok(())
		})
	}

	fn on_shutdown(&self) {}

	fn fetch(
		&self,
		_handle: EnvoyHandle,
		actor_id: String,
		_gateway_id: protocol::GatewayId,
		_request_id: protocol::RequestId,
		request: HttpRequest,
	) -> EnvoyBoxFuture<anyhow::Result<HttpResponse>> {
		tracing::info!(
			method = %request.method,
			path = %request.path,
			"envoy callback: fetch request"
		);
		let dispatcher = self.dispatcher.clone();
		Box::pin(async move { dispatcher.handle_fetch(&actor_id, request).await })
	}

	fn websocket(
		&self,
		_handle: EnvoyHandle,
		actor_id: String,
		_gateway_id: protocol::GatewayId,
		_request_id: protocol::RequestId,
		_request: HttpRequest,
		_path: String,
		_headers: HashMap<String, String>,
		_is_hibernatable: bool,
		is_restoring_hibernatable: bool,
		sender: WebSocketSender,
	) -> EnvoyBoxFuture<anyhow::Result<WebSocketHandler>> {
		tracing::info!(
			path = %_path,
			is_hibernatable = _is_hibernatable,
			is_restoring_hibernatable,
			"envoy callback: websocket request"
		);
		let dispatcher = self.dispatcher.clone();
		Box::pin(async move {
			dispatcher
				.handle_websocket(
					&actor_id,
					&_request,
					&_path,
					&_headers,
					&_gateway_id,
					&_request_id,
					_is_hibernatable,
					is_restoring_hibernatable,
					sender,
				)
				.await
		})
	}

	fn can_hibernate(
		&self,
		actor_id: &str,
		_gateway_id: &protocol::GatewayId,
		_request_id: &protocol::RequestId,
		request: &HttpRequest,
	) -> EnvoyBoxFuture<anyhow::Result<bool>> {
		let can_hibernate = self.dispatcher.can_hibernate(actor_id, request);
		Box::pin(async move { Ok(can_hibernate) })
	}
}

impl ServeSettings {
	fn from_env() -> Self {
		let engine_host = env::var("RIVET_RUN_ENGINE_HOST").ok();
		let engine_port = env::var("RIVET_RUN_ENGINE_PORT")
			.ok()
			.and_then(|value| value.parse().ok());
		let endpoint = env::var("RIVET_ENDPOINT").unwrap_or_else(|_| {
			default_engine_endpoint(
				engine_host.as_deref().unwrap_or("127.0.0.1"),
				engine_port.unwrap_or(6420),
			)
		});

		Self {
			version: env::var("RIVET_ENVOY_VERSION")
				.ok()
				.and_then(|value| value.parse().ok())
				.unwrap_or(1),
			endpoint,
			token: Some(env::var("RIVET_TOKEN").unwrap_or_else(|_| "dev".to_owned())),
			namespace: env::var("RIVET_NAMESPACE").unwrap_or_else(|_| "default".to_owned()),
			pool_name: env::var("RIVET_POOL_NAME").unwrap_or_else(|_| "rivetkit-rust".to_owned()),
			engine_binary_path: env::var_os("RIVET_ENGINE_BINARY_PATH").map(PathBuf::from),
			start_services: matches!(env::var("RIVET_RUN_SERVICES").as_deref(), Ok("1")),
			services_binary_path: env::var_os("RIVET_SERVICES_BINARY").map(PathBuf::from),
			engine_host,
			engine_port,
			engine_spawn: super::EngineSpawnMode::from_env(),
			engine_auto_download: matches!(
				env::var("RIVETKIT_ENGINE_AUTO_DOWNLOAD").as_deref(),
				Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
			),
			handle_inspector_http_in_runtime: false,
			serverless_base_path: None,
			serverless_package_version: env!("CARGO_PKG_VERSION").to_owned(),
			serverless_client_endpoint: None,
			serverless_client_namespace: None,
			serverless_client_token: None,
			serverless_validate_endpoint: true,
			serverless_max_start_payload_bytes: 1_048_576,
		}
	}
}

fn default_engine_endpoint(host: &str, port: u16) -> String {
	let url_host = if host.contains(':') && !host.starts_with('[') {
		format!("[{host}]")
	} else {
		host.to_owned()
	};
	format!("http://{url_host}:{port}")
}

impl ServeConfig {
	pub fn from_env() -> Self {
		let settings = ServeSettings::from_env();
		Self {
			version: settings.version,
			endpoint: settings.endpoint,
			token: settings.token,
			namespace: settings.namespace,
			pool_name: settings.pool_name,
			engine_binary_path: settings.engine_binary_path,
			start_services: settings.start_services,
			services_binary_path: settings.services_binary_path,
			engine_host: settings.engine_host,
			engine_port: settings.engine_port,
			engine_spawn: settings.engine_spawn,
			engine_auto_download: settings.engine_auto_download,
			handle_inspector_http_in_runtime: settings.handle_inspector_http_in_runtime,
			serverless_base_path: settings.serverless_base_path,
			serverless_package_version: settings.serverless_package_version,
			serverless_client_endpoint: settings.serverless_client_endpoint,
			serverless_client_namespace: settings.serverless_client_namespace,
			serverless_client_token: settings.serverless_client_token,
			serverless_validate_endpoint: settings.serverless_validate_endpoint,
			serverless_max_start_payload_bytes: settings.serverless_max_start_payload_bytes,
			serverless_cache_envoy: true,
			..Default::default()
		}
	}
}

fn actor_key_from_protocol(key: Option<String>) -> ActorKey {
	key.as_deref()
		.map(deserialize_actor_key_from_protocol)
		.unwrap_or_default()
}

fn deserialize_actor_key_from_protocol(key: &str) -> ActorKey {
	const EMPTY_KEY: &str = "/";
	const KEY_SEPARATOR: char = '/';

	if key.is_empty() || key == EMPTY_KEY {
		return Vec::new();
	}

	let mut parts = Vec::new();
	let mut current_part = String::new();
	let mut escaping = false;
	let mut empty_string_marker = false;

	for ch in key.chars() {
		if escaping {
			if ch == '0' {
				empty_string_marker = true;
			} else {
				current_part.push(ch);
			}
			escaping = false;
		} else if ch == '\\' {
			escaping = true;
		} else if ch == KEY_SEPARATOR {
			if empty_string_marker {
				parts.push(String::new());
				empty_string_marker = false;
			} else {
				parts.push(std::mem::take(&mut current_part));
			}
		} else {
			current_part.push(ch);
		}
	}

	if escaping {
		current_part.push('\\');
		parts.push(current_part);
	} else if empty_string_marker {
		parts.push(String::new());
	} else if !current_part.is_empty() || !parts.is_empty() {
		parts.push(current_part);
	}

	parts.into_iter().map(ActorKeySegment::String).collect()
}
