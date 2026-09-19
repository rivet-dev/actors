use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bytes::Bytes;
use opentelemetry::baggage::BaggageExt as _;
use opentelemetry::propagation::TextMapPropagator as _;
use opentelemetry_http::HeaderInjector;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use reqwest::{
	header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT},
	Method,
};
use rivetkit_client_protocol::telemetry_headers::{
	bounded_ray_id, HEADER_RIVET_RAY_ID, HEADER_TRACEPARENT, HEADER_TRACESTATE, RAY_BAGGAGE_KEY,
};
use serde::{Deserialize, Serialize};
use serde_cbor;
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tokio::sync::OnceCell;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use crate::{
	common::{
		serialize_actor_key, ActorKey, EncodingKind, RawWebSocket, HEADER_RIVET_ACTOR,
		HEADER_RIVET_NAMESPACE, HEADER_RIVET_TARGET, HEADER_RIVET_TOKEN, PATH_CONNECT_WEBSOCKET,
		PATH_WEBSOCKET_PREFIX, USER_AGENT_VALUE, WS_PROTOCOL_ACTOR, WS_PROTOCOL_CONN_ID,
		WS_PROTOCOL_CONN_PARAMS, WS_PROTOCOL_CONN_TOKEN, WS_PROTOCOL_ENCODING,
		WS_PROTOCOL_STANDARD, WS_PROTOCOL_TARGET, WS_PROTOCOL_TOKEN,
	},
	protocol::query::ActorQuery,
};

#[derive(Clone)]
pub struct RemoteManager {
	endpoint: String,
	token: Option<String>,
	namespace: String,
	pool_name: String,
	headers: HashMap<String, String>,
	max_input_size: usize,
	disable_metadata_lookup: bool,
	resolved_config: Arc<OnceCell<ResolvedClientConfig>>,
	client: reqwest::Client,
}

#[derive(Clone)]
struct ResolvedClientConfig {
	endpoint: String,
	token: Option<String>,
	namespace: String,
}

#[derive(Debug, Deserialize)]
struct MetadataResponse {
	#[serde(rename = "clientEndpoint")]
	client_endpoint: Option<String>,
	#[serde(rename = "clientNamespace")]
	client_namespace: Option<String>,
	#[serde(rename = "clientToken")]
	client_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Actor {
	actor_id: String,
	name: String,
	key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActorsListResponse {
	actors: Vec<Actor>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActorsGetOrCreateRequest {
	name: String,
	key: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	input: Option<String>, // base64-encoded CBOR
	runner_name_selector: String,
	crash_policy: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActorsGetOrCreateResponse {
	actor: Actor,
	created: bool,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
	group: Option<String>,
	code: Option<String>,
}

fn is_key_reserved_in_different_datacenter(body: &str) -> bool {
	serde_json::from_str::<ApiErrorBody>(body)
		.ok()
		.map(|err| {
			err.group.as_deref() == Some("actor")
				&& err.code.as_deref() == Some("key_reserved_in_different_datacenter")
		})
		.unwrap_or(false)
}

#[derive(Debug, Serialize, Deserialize)]
struct ActorsCreateRequest {
	name: String,
	key: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	input: Option<String>, // base64-encoded CBOR
	runner_name_selector: String,
	crash_policy: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActorsCreateResponse {
	actor: Actor,
}

/// How a data-plane request should address its actor at the gateway.
#[derive(Debug, Clone)]
pub enum GatewayTarget {
	Direct { actor_id: String },
	Query { query: ActorQuery },
}

impl RemoteManager {
	pub fn new(endpoint: &str, token: Option<String>) -> Self {
		Self {
			endpoint: endpoint.to_string(),
			token,
			namespace: default_namespace(),
			pool_name: default_pool_name(),
			headers: HashMap::new(),
			max_input_size: default_max_input_size(),
			disable_metadata_lookup: false,
			resolved_config: Arc::new(OnceCell::new()),
			client: reqwest::Client::new(),
		}
	}

	pub fn from_config(
		endpoint: String,
		token: Option<String>,
		namespace: Option<String>,
		pool_name: Option<String>,
		headers: Option<HashMap<String, String>>,
		max_input_size: Option<usize>,
		disable_metadata_lookup: bool,
	) -> Self {
		Self {
			endpoint,
			token,
			namespace: namespace.unwrap_or_else(default_namespace),
			pool_name: pool_name.unwrap_or_else(default_pool_name),
			headers: headers.unwrap_or_default(),
			max_input_size: max_input_size.unwrap_or_else(default_max_input_size),
			disable_metadata_lookup,
			resolved_config: Arc::new(OnceCell::new()),
			client: reqwest::Client::new(),
		}
	}

	pub fn endpoint(&self) -> &str {
		&self.endpoint
	}

	pub fn token(&self) -> Option<&str> {
		self.token.as_deref()
	}

	fn base_config(&self) -> ResolvedClientConfig {
		ResolvedClientConfig {
			endpoint: self.endpoint.clone(),
			token: self.token.clone(),
			namespace: self.namespace.clone(),
		}
	}

	async fn resolved_config(&self) -> Result<ResolvedClientConfig> {
		if self.disable_metadata_lookup {
			return Ok(self.base_config());
		}

		self.resolved_config
			.get_or_try_init(|| async { self.lookup_metadata().await })
			.await
			.cloned()
	}

	async fn lookup_metadata(&self) -> Result<ResolvedClientConfig> {
		let base_config = self.base_config();
		let url = combine_url_path(&base_config.endpoint, "/metadata");
		let req = self.apply_common_headers_with(self.client.get(&url), &base_config)?;
		let res = req.send().await?;

		if !res.status().is_success() {
			return Err(anyhow!("failed to fetch metadata: {}", res.status()));
		}

		let metadata: MetadataResponse = res.json().await?;
		let mut resolved = base_config;
		if let Some(endpoint) = metadata.client_endpoint {
			resolved.endpoint = endpoint;
		}
		if let Some(namespace) = metadata.client_namespace {
			resolved.namespace = namespace;
		}
		if let Some(token) = metadata.client_token {
			resolved.token = Some(token);
		}
		Ok(resolved)
	}

	fn apply_common_headers_with(
		&self,
		mut req: reqwest::RequestBuilder,
		config: &ResolvedClientConfig,
	) -> Result<reqwest::RequestBuilder> {
		req = req.header(USER_AGENT, USER_AGENT_VALUE);

		for (key, value) in &self.headers {
			if is_trace_context_header(key) {
				continue;
			}
			let name = HeaderName::from_str(key)
				.with_context(|| format!("invalid configured header name `{key}`"))?;
			let value = HeaderValue::from_str(value)
				.with_context(|| format!("invalid configured header value for `{key}`"))?;
			req = req.header(name, value);
		}

		if let Some(token) = &config.token {
			req = req.header(HEADER_RIVET_TOKEN, token);
		}

		if !config.namespace.is_empty() {
			req = req.header(HEADER_RIVET_NAMESPACE, &config.namespace);
		}

		Ok(req)
	}

	pub async fn get_for_id(&self, name: &str, actor_id: &str) -> Result<Option<String>> {
		let config = self.resolved_config().await?;
		let url = format!(
			"{}/actors?namespace={}&name={}&actor_ids={}",
			config.endpoint,
			urlencoding::encode(&config.namespace),
			urlencoding::encode(name),
			urlencoding::encode(actor_id)
		);

		let req = self.apply_common_headers_with(self.client.get(&url), &config)?;

		let res = req.send().await?;

		if !res.status().is_success() {
			return Err(anyhow!("failed to get actor: {}", res.status()));
		}

		let data: ActorsListResponse = res.json().await?;

		if let Some(actor) = data.actors.first() {
			if actor.name == name {
				Ok(Some(actor.actor_id.clone()))
			} else {
				Ok(None)
			}
		} else {
			Ok(None)
		}
	}

	pub async fn get_with_key(&self, name: &str, key: &ActorKey) -> Result<Option<String>> {
		let config = self.resolved_config().await?;
		// Canonical slash-escaped key format (matches the TS SDK and the
		// runner-side parser) — NOT JSON (see common::serialize_actor_key).
		let key_str = serialize_actor_key(key);
		let url = format!(
			"{}/actors?namespace={}&name={}&key={}",
			config.endpoint,
			urlencoding::encode(&config.namespace),
			urlencoding::encode(name),
			urlencoding::encode(&key_str)
		);

		let req = self.apply_common_headers_with(self.client.get(&url), &config)?;

		let res = req.send().await?;

		if !res.status().is_success() {
			if res.status() == 404 {
				return Ok(None);
			}
			return Err(anyhow!("failed to get actor by key: {}", res.status()));
		}

		let data: ActorsListResponse = res.json().await?;

		if let Some(actor) = data.actors.first() {
			Ok(Some(actor.actor_id.clone()))
		} else {
			Ok(None)
		}
	}

	pub async fn get_or_create_with_key(
		&self,
		name: &str,
		key: &ActorKey,
		input: Option<serde_json::Value>,
		pool_name: Option<String>,
	) -> Result<String> {
		let config = self.resolved_config().await?;
		// Canonical slash-escaped key format (matches the TS SDK and the
		// runner-side parser) — NOT JSON (see common::serialize_actor_key).
		let key_str = serialize_actor_key(key);

		let input_encoded = if let Some(inp) = input {
			let cbor = serde_cbor::to_vec(&inp)?;
			Some(general_purpose::STANDARD.encode(cbor))
		} else {
			None
		};

		let request_body = ActorsGetOrCreateRequest {
			name: name.to_string(),
			key: key_str,
			input: input_encoded,
			runner_name_selector: pool_name.unwrap_or_else(|| self.pool_name.clone()),
			crash_policy: "destroy".to_string(),
		};

		let req = self.apply_common_headers_with(
			self.client
				.put(format!(
					"{}/actors?namespace={}",
					config.endpoint,
					urlencoding::encode(&config.namespace)
				))
				.json(&request_body),
			&config,
		)?;

		let res = req.send().await?;

		let status = res.status();
		if !status.is_success() {
			let body = res.text().await.unwrap_or_default();

			// Retry with a get in case of collision, heals after race.
			if is_key_reserved_in_different_datacenter(&body) {
				tracing::warn!(
					%name,
					"actor key reserved in different datacenter, falling back to get by key"
				);

				return self.get_with_key(name, key).await?.ok_or_else(|| {
					anyhow!(
						"actor key reserved in different datacenter but get by key found no actor"
					)
				});
			}

			return Err(anyhow!("failed to get or create actor ({status}): {body}"));
		}

		let data: ActorsGetOrCreateResponse = res.json().await?;
		Ok(data.actor.actor_id)
	}

	pub async fn create_actor(
		&self,
		name: &str,
		key: &ActorKey,
		input: Option<serde_json::Value>,
		pool_name: Option<String>,
	) -> Result<String> {
		let config = self.resolved_config().await?;
		// Canonical slash-escaped key format (matches the TS SDK and the
		// runner-side parser) — NOT JSON (see common::serialize_actor_key).
		let key_str = serialize_actor_key(key);

		let input_encoded = if let Some(inp) = input {
			let cbor = serde_cbor::to_vec(&inp)?;
			Some(general_purpose::STANDARD.encode(cbor))
		} else {
			None
		};

		let request_body = ActorsCreateRequest {
			name: name.to_string(),
			key: key_str,
			input: input_encoded,
			runner_name_selector: pool_name.unwrap_or_else(|| self.pool_name.clone()),
			crash_policy: "destroy".to_string(),
		};

		let req = self.apply_common_headers_with(
			self.client
				.post(format!(
					"{}/actors?namespace={}",
					config.endpoint,
					urlencoding::encode(&config.namespace)
				))
				.json(&request_body),
			&config,
		)?;

		let res = req.send().await?;

		let status = res.status();
		if !status.is_success() {
			let body = res.text().await.unwrap_or_default();
			return Err(anyhow!("failed to create actor ({status}): {body}"));
		}

		let data: ActorsCreateResponse = res.json().await?;
		Ok(data.actor.actor_id)
	}

	pub async fn resolve_actor_id(&self, query: &ActorQuery) -> Result<String> {
		match query {
			ActorQuery::GetForId { get_for_id } => self
				.get_for_id(&get_for_id.name, &get_for_id.actor_id)
				.await?
				.ok_or_else(|| anyhow!("actor not found")),
			ActorQuery::GetForKey { get_for_key } => self
				.get_with_key(&get_for_key.name, &get_for_key.key)
				.await?
				.ok_or_else(|| anyhow!("actor not found")),
			ActorQuery::GetOrCreateForKey {
				get_or_create_for_key,
			} => {
				self.get_or_create_with_key(
					&get_or_create_for_key.name,
					&get_or_create_for_key.key,
					get_or_create_for_key.input.clone(),
					get_or_create_for_key.pool_name.clone(),
				)
				.await
			}
			ActorQuery::Create { create } => {
				self.create_actor(
					&create.name,
					&create.key,
					create.input.clone(),
					create.pool_name.clone(),
				)
				.await
			}
		}
	}

	/// Resolve a query into a gateway target for a data-plane request. Key-based
	/// queries route to the gateway for resolution; `getForId` and `create`
	/// resolve to a direct actor id.
	pub async fn gateway_target(&self, query: &ActorQuery) -> Result<GatewayTarget> {
		match query {
			ActorQuery::GetForId { get_for_id } => Ok(GatewayTarget::Direct {
				actor_id: get_for_id.actor_id.clone(),
			}),
			ActorQuery::GetForKey { .. } | ActorQuery::GetOrCreateForKey { .. } => {
				Ok(GatewayTarget::Query {
					query: query.clone(),
				})
			}
			ActorQuery::Create { create } => {
				let actor_id = self
					.create_actor(
						&create.name,
						&create.key,
						create.input.clone(),
						create.pool_name.clone(),
					)
					.await?;
				Ok(GatewayTarget::Direct { actor_id })
			}
		}
	}

	pub async fn send_request(
		&self,
		target: &GatewayTarget,
		path: &str,
		method: Method,
		headers: HeaderMap,
		body: Option<Bytes>,
	) -> Result<reqwest::Response> {
		let config = self.resolved_config().await?;
		let url = self.build_gateway_url(&config, target, path)?;

		let mut builder = self.client.request(method, &url);
		// Query targets are resolved by the gateway from the URL, so no actor
		// headers are sent.
		if let GatewayTarget::Direct { actor_id } = target {
			builder = builder
				.header(HEADER_RIVET_TARGET, "actor")
				.header(HEADER_RIVET_ACTOR, actor_id.as_str());
		}

		let mut req = self.apply_common_headers_with(builder, &config)?;

		let mut headers = headers;
		let caller_set_trace_context =
			headers.contains_key(HEADER_TRACEPARENT) || headers.contains_key(HEADER_TRACESTATE);
		for (name, value) in self.telemetry_headers()? {
			let is_trace_context = name == HEADER_TRACEPARENT || name == HEADER_TRACESTATE;
			if is_trace_context && caller_set_trace_context {
				continue;
			}
			headers.entry(name).or_insert(value);
		}
		req = req.headers(headers);

		if let Some(body_data) = body {
			req = req.body(body_data);
		}

		let res = req.send().await?;
		Ok(res)
	}

	/// Headers that carry the caller's trace context and ray ID into the actor,
	/// read from the `tracing` span and baggage current at the call. They
	/// override a configured `x-rivet-ray-id`, which stays the fallback.
	fn telemetry_headers(&self) -> Result<Vec<(HeaderName, HeaderValue)>> {
		let mut headers = HeaderMap::with_capacity(3);
		let context = tracing::Span::current().context();
		TraceContextPropagator::new().inject_context(&context, &mut HeaderInjector(&mut headers));
		if let Some(ray_id) = context.baggage().get(RAY_BAGGAGE_KEY) {
			if let Some(ray_id) = bounded_ray_id(&ray_id.as_str()) {
				headers.insert(
					HeaderName::from_static(HEADER_RIVET_RAY_ID),
					HeaderValue::from_str(ray_id).context("format ray id header")?,
				);
			}
		}
		if headers
			.get(HEADER_TRACESTATE)
			.is_some_and(HeaderValue::is_empty)
		{
			headers.remove(HEADER_TRACESTATE);
		}
		Ok(headers
			.into_iter()
			.filter_map(|(name, value)| name.map(|name| (name, value)))
			.collect())
	}

	pub fn gateway_url(&self, query: &ActorQuery) -> Result<String> {
		let config = self.base_config();
		match query {
			ActorQuery::GetForId { get_for_id } => {
				Ok(self.build_actor_gateway_url_with(&config, &get_for_id.actor_id, ""))
			}
			ActorQuery::GetForKey { .. } | ActorQuery::GetOrCreateForKey { .. } => {
				self.build_query_gateway_url(&config, query, "")
			}
			ActorQuery::Create { .. } => {
				Err(anyhow!("gateway URL does not support create actor queries"))
			}
		}
	}

	fn build_gateway_url(
		&self,
		config: &ResolvedClientConfig,
		target: &GatewayTarget,
		path: &str,
	) -> Result<String> {
		match target {
			GatewayTarget::Direct { actor_id } => {
				Ok(self.build_actor_gateway_url_with(config, actor_id, path))
			}
			GatewayTarget::Query { query } => self.build_query_gateway_url(config, query, path),
		}
	}

	fn build_query_gateway_url(
		&self,
		config: &ResolvedClientConfig,
		query: &ActorQuery,
		path: &str,
	) -> Result<String> {
		match query {
			ActorQuery::GetForKey { get_for_key } => self.build_actor_query_gateway_url(
				config,
				&get_for_key.name,
				"get",
				Some(&get_for_key.key),
				None,
				None,
				None,
				path,
			),
			ActorQuery::GetOrCreateForKey {
				get_or_create_for_key,
			} => self.build_actor_query_gateway_url(
				config,
				&get_or_create_for_key.name,
				"getOrCreate",
				Some(&get_or_create_for_key.key),
				get_or_create_for_key.input.as_ref(),
				get_or_create_for_key.region.as_deref(),
				get_or_create_for_key.pool_name.as_deref(),
				path,
			),
			ActorQuery::GetForId { .. } | ActorQuery::Create { .. } => Err(anyhow!(
				"query gateway URL only supports get and getOrCreate queries"
			)),
		}
	}

	fn build_actor_gateway_url_with(
		&self,
		config: &ResolvedClientConfig,
		actor_id: &str,
		path: &str,
	) -> String {
		let token_segment = self
			.token_segment(config)
			.map(|token| format!("@{}", urlencoding::encode(token)))
			.unwrap_or_default();
		let gateway_path = format!(
			"/gateway/{}{}{}",
			urlencoding::encode(actor_id),
			token_segment,
			path,
		);
		combine_url_path(&config.endpoint, &gateway_path)
	}

	fn token_segment<'a>(&self, config: &'a ResolvedClientConfig) -> Option<&'a str> {
		config.token.as_deref()
	}

	fn build_actor_query_gateway_url(
		&self,
		config: &ResolvedClientConfig,
		name: &str,
		method: &str,
		key: Option<&ActorKey>,
		input: Option<&serde_json::Value>,
		region: Option<&str>,
		pool_name: Option<&str>,
		path: &str,
	) -> Result<String> {
		if config.namespace.is_empty() {
			return Err(anyhow!("actor query namespace must not be empty"));
		}
		let mut params = Vec::new();
		push_query_param(&mut params, "rvt-namespace", &config.namespace);
		push_query_param(&mut params, "rvt-method", method);
		if let Some(key) = key {
			if !key.is_empty() {
				push_query_param(&mut params, "rvt-key", &key.join(","));
			}
		}
		if let Some(input) = input {
			let encoded = serde_cbor::to_vec(input)?;
			if encoded.len() > self.max_input_size {
				return Err(anyhow!(
					"actor query input exceeds max_input_size ({} > {} bytes)",
					encoded.len(),
					self.max_input_size
				));
			}
			push_query_param(&mut params, "rvt-input", &URL_SAFE_NO_PAD.encode(encoded));
		}
		if method == "getOrCreate" {
			push_query_param(
				&mut params,
				"rvt-runner",
				pool_name.unwrap_or(&self.pool_name),
			);
			push_query_param(&mut params, "rvt-crash-policy", "sleep");
		}
		if let Some(region) = region {
			push_query_param(&mut params, "rvt-region", region);
		}
		if let Some(token) = &config.token {
			push_query_param(&mut params, "rvt-token", token);
		}

		let query = params.join("&");
		// Merge with the forwarded path's existing query string instead of
		// introducing a second `?`.
		let separator = if path.ends_with('?') || path.ends_with('&') {
			""
		} else if path.contains('?') {
			"&"
		} else {
			"?"
		};
		let gateway_path = format!(
			"/gateway/{}{}{}{}",
			urlencoding::encode(name),
			path,
			separator,
			query
		);
		Ok(combine_url_path(&config.endpoint, &gateway_path))
	}

	pub async fn open_websocket(
		&self,
		target: &GatewayTarget,
		encoding: EncodingKind,
		params: Option<serde_json::Value>,
		conn_id: Option<String>,
		conn_token: Option<String>,
	) -> Result<RawWebSocket> {
		use tokio_tungstenite::connect_async;

		let config = self.resolved_config().await?;
		let ws_url = self.websocket_url(&self.build_gateway_url(
			&config,
			target,
			PATH_CONNECT_WEBSOCKET,
		)?)?;

		// Actor target/id protocols are only sent for direct targets; query
		// targets are resolved by the gateway from the URL.
		let mut protocols = vec![WS_PROTOCOL_STANDARD.to_string()];
		if let GatewayTarget::Direct { actor_id } = target {
			protocols.push(format!("{}actor", WS_PROTOCOL_TARGET));
			protocols.push(format!("{}{}", WS_PROTOCOL_ACTOR, actor_id));
		}
		protocols.push(format!("{}{}", WS_PROTOCOL_ENCODING, encoding.as_str()));

		if let Some(token) = &config.token {
			protocols.push(format!("{}{}", WS_PROTOCOL_TOKEN, token));
		}

		if let Some(p) = params {
			let params_str = serde_json::to_string(&p)?;
			protocols.push(format!(
				"{}{}",
				WS_PROTOCOL_CONN_PARAMS,
				urlencoding::encode(&params_str)
			));
		}

		if let Some(cid) = conn_id {
			protocols.push(format!("{}{}", WS_PROTOCOL_CONN_ID, cid));
		}

		if let Some(ct) = conn_token {
			protocols.push(format!("{}{}", WS_PROTOCOL_CONN_TOKEN, ct));
		}

		let mut request = ws_url.into_client_request()?;
		request
			.headers_mut()
			.insert("Sec-WebSocket-Protocol", protocols.join(", ").parse()?);
		self.apply_websocket_headers(request.headers_mut())?;

		let (ws_stream, _) = connect_async(request).await?;
		Ok(ws_stream)
	}

	pub async fn open_raw_websocket(
		&self,
		target: &GatewayTarget,
		path: &str,
		params: Option<serde_json::Value>,
		protocols: Option<Vec<String>>,
	) -> Result<RawWebSocket> {
		use tokio_tungstenite::connect_async;

		let gateway_path = normalize_raw_websocket_path(path);
		let config = self.resolved_config().await?;
		let ws_url =
			self.websocket_url(&self.build_gateway_url(&config, target, &gateway_path)?)?;

		let mut all_protocols = vec![WS_PROTOCOL_STANDARD.to_string()];
		if let GatewayTarget::Direct { actor_id } = target {
			all_protocols.push(format!("{}actor", WS_PROTOCOL_TARGET));
			all_protocols.push(format!("{}{}", WS_PROTOCOL_ACTOR, actor_id));
		}
		if let Some(token) = &config.token {
			all_protocols.push(format!("{}{}", WS_PROTOCOL_TOKEN, token));
		}
		if let Some(p) = params {
			let params_str = serde_json::to_string(&p)?;
			all_protocols.push(format!(
				"{}{}",
				WS_PROTOCOL_CONN_PARAMS,
				urlencoding::encode(&params_str)
			));
		}
		if let Some(protocols) = protocols {
			all_protocols.extend(protocols);
		}

		let mut request = ws_url.into_client_request()?;
		request
			.headers_mut()
			.insert("Sec-WebSocket-Protocol", all_protocols.join(", ").parse()?);
		self.apply_websocket_headers(request.headers_mut())?;

		let (ws_stream, _) = connect_async(request).await?;
		Ok(ws_stream)
	}

	fn websocket_url(&self, url: &str) -> Result<String> {
		if let Some(rest) = url.strip_prefix("https://") {
			Ok(format!("wss://{rest}"))
		} else if let Some(rest) = url.strip_prefix("http://") {
			Ok(format!("ws://{rest}"))
		} else {
			Err(anyhow!("invalid endpoint URL"))
		}
	}

	fn apply_websocket_headers(
		&self,
		headers: &mut tokio_tungstenite::tungstenite::http::HeaderMap,
	) -> Result<()> {
		for (key, value) in &self.headers {
			if is_trace_context_header(key) {
				continue;
			}
			headers.insert(
				HeaderName::from_str(key)
					.with_context(|| format!("invalid configured header name `{key}`"))?,
				HeaderValue::from_str(value)
					.with_context(|| format!("invalid configured header value for `{key}`"))?,
			);
		}
		Ok(())
	}
}

fn combine_url_path(endpoint: &str, path: &str) -> String {
	format!("{}{}", endpoint.trim_end_matches('/'), path)
}

fn push_query_param(params: &mut Vec<String>, key: &str, value: &str) {
	params.push(format!(
		"{}={}",
		urlencoding::encode(key),
		urlencoding::encode(value)
	));
}

fn normalize_raw_websocket_path(path: &str) -> String {
	let mut path_portion = path;
	let mut query_portion = "";
	if let Some((left, right)) = path.split_once('?') {
		path_portion = left;
		query_portion = right;
	}
	let path_portion = path_portion.trim_start_matches('/');
	if query_portion.is_empty() {
		format!("{PATH_WEBSOCKET_PREFIX}{path_portion}")
	} else {
		format!("{PATH_WEBSOCKET_PREFIX}{path_portion}?{query_portion}")
	}
}

fn default_namespace() -> String {
	"default".to_string()
}

fn default_pool_name() -> String {
	"default".to_string()
}

fn default_max_input_size() -> usize {
	4 * 1024
}

/// A configured `traceparent` or `tracestate` is never sent: it would pin
/// every call to one span. Trace context comes from the span at the call.
fn is_trace_context_header(name: &str) -> bool {
	[HEADER_TRACEPARENT, HEADER_TRACESTATE]
		.iter()
		.any(|header| header.eq_ignore_ascii_case(name))
}
