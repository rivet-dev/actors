use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::time::sleep;
use url::Url;

use crate::util::encode;

#[derive(Deserialize)]
pub struct TokenInspectResponse {
	pub project: String,
	pub organization: String,
}

#[derive(Deserialize)]
pub struct NamespaceResponse {
	pub namespace: Namespace,
}

#[derive(Deserialize)]
struct TokenResponse {
	token: String,
}

#[derive(Deserialize)]
struct NamespacesResponse {
	namespaces: Vec<Namespace>,
	pagination: Option<Pagination>,
}

#[derive(Deserialize)]
struct Pagination {
	cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Namespace {
	pub name: String,
	pub display_name: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
	pub timestamp: String,
	pub severity: String,
	pub message: String,
	pub region: String,
	pub insert_id: String,
	pub stream: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedPoolResponse {
	managed_pool: Option<ManagedPool>,
}

#[derive(Deserialize)]
struct ManagedPool {
	status: Option<String>,
	error: Option<ManagedPoolError>,
}

#[derive(Deserialize)]
struct ManagedPoolError {
	message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedPoolsListResponse {
	managed_pools: Vec<PoolSummary>,
	pagination: Option<Pagination>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolSummary {
	pub name: String,
	pub status: Option<String>,
	#[serde(default)]
	pub config: Option<PoolConfig>,
	#[serde(default)]
	pub error: Option<PoolError>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolConfig {
	pub display_name: Option<String>,
	#[serde(default)]
	pub image: Option<ImageRef>,
	#[serde(default)]
	pub resources: Option<PoolResources>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub environment: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
pub struct ImageRef {
	pub repository: String,
	pub tag: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolResources {
	pub cpu: Option<f64>,
	pub memory: Option<String>,
	pub min_scale: Option<u32>,
	pub max_scale: Option<u32>,
	pub instance_request_concurrency: Option<u32>,
}

#[derive(Deserialize, Serialize)]
pub struct PoolError {
	pub message: String,
}

pub struct CloudClient {
	http: reqwest::Client,
	base: Url,
	token: String,
}

impl CloudClient {
	pub fn new(base: &str, token: String) -> Result<Self> {
		Ok(Self {
			http: reqwest::Client::new(),
			base: Url::parse(base).context("invalid Cloud API endpoint")?,
			token,
		})
	}

	/// Builds an authenticated JSON request. A bodyless non-GET request gets an
	/// explicit `Content-Length: 0`, since some Cloud API frontends (e.g.
	/// Google's load balancer) reject a bodyless POST/PUT/DELETE with 411 Length
	/// Required.
	fn build_request(
		&self,
		method: Method,
		path: &str,
		body: Option<Value>,
	) -> Result<reqwest::RequestBuilder> {
		let url = self.base.join(path.trim_start_matches('/'))?;
		let needs_content_length = !matches!(method, Method::GET | Method::HEAD);
		let mut request = self
			.http
			.request(method, url)
			.bearer_auth(&self.token)
			.header("Content-Type", "application/json");
		match body {
			Some(body) => request = request.json(&body),
			None if needs_content_length => {
				request = request.header(reqwest::header::CONTENT_LENGTH, "0")
			}
			None => {}
		}
		Ok(request)
	}

	pub async fn request<T: DeserializeOwned>(
		&self,
		method: Method,
		path: &str,
		body: Option<Value>,
	) -> Result<Option<T>> {
		let response = self
			.build_request(method, path, body)?
			.send()
			.await
			.context("Cloud API request failed")?;
		if response.status() == StatusCode::NOT_FOUND {
			return Ok(None);
		}
		let status = response.status();
		let text = response.text().await.unwrap_or_default();
		if !status.is_success() {
			bail!("Cloud API error {status}: {text}");
		}
		if text.trim().is_empty() {
			return Ok(None);
		}
		Ok(Some(serde_json::from_str(&text).with_context(|| {
			format!("Cloud API returned invalid JSON for {path}")
		})?))
	}

	/// Builds an authenticated GET request for the given path. Used for
	/// streaming responses (e.g. SSE log tails) that the buffered `request`
	/// helpers cannot consume.
	pub fn get_builder(&self, path: &str) -> Result<reqwest::RequestBuilder> {
		let url = self.base.join(path.trim_start_matches('/'))?;
		Ok(self.http.request(Method::GET, url).bearer_auth(&self.token))
	}

	pub async fn request_ok<T: DeserializeOwned>(
		&self,
		method: Method,
		path: &str,
		body: Option<Value>,
	) -> Result<Option<T>> {
		let response = self
			.build_request(method, path, body)?
			.send()
			.await
			.context("Cloud API request failed")?;
		let status = response.status();
		let text = response.text().await.unwrap_or_default();
		if !status.is_success() {
			bail!("Cloud API error {status}: {text}");
		}
		if text.trim().is_empty() {
			return Ok(None);
		}
		Ok(Some(serde_json::from_str(&text).with_context(|| {
			format!("Cloud API returned invalid JSON for {path}")
		})?))
	}
}

/// Resolves a namespace by its machine name or display name. First tries a
/// direct lookup by name, then falls back to listing namespaces and matching
/// either the machine name or the (case-insensitive) display name. This lets a
/// user pass a display name like `production` when the real machine name is
/// something like `production-qvra`. Returns `None` when no namespace matches.
async fn resolve_namespace(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
) -> Result<Option<Namespace>> {
	let path = format!(
		"/projects/{}/namespaces/{}?org={}",
		encode(project),
		encode(namespace),
		encode(org)
	);
	if let Some(response) = cloud
		.request::<NamespaceResponse>(Method::GET, &path, None)
		.await?
	{
		return Ok(Some(response.namespace));
	}

	let mut cursor: Option<String> = None;
	loop {
		let mut list_path = format!(
			"/projects/{}/namespaces?org={}&limit=100",
			encode(project),
			encode(org)
		);
		if let Some(cursor) = &cursor {
			list_path.push_str(&format!("&cursor={}", encode(cursor)));
		}

		let Some(response) = cloud
			.request::<NamespacesResponse>(Method::GET, &list_path, None)
			.await?
		else {
			return Ok(None);
		};

		let next_cursor = response.pagination.and_then(|p| p.cursor);
		if let Some(found) = response.namespaces.into_iter().find(|ns| {
			ns.name == namespace
				|| ns
					.display_name
					.as_ref()
					.is_some_and(|display| display.eq_ignore_ascii_case(namespace))
		}) {
			return Ok(Some(found));
		}

		match next_cursor {
			Some(next) => cursor = Some(next),
			None => return Ok(None),
		}
	}
}

pub async fn ensure_namespace(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
) -> Result<Namespace> {
	if let Some(found) = resolve_namespace(cloud, project, org, namespace).await? {
		return Ok(found);
	}

	tracing::info!(%namespace, "creating namespace");
	let create_path = format!(
		"/projects/{}/namespaces?org={}",
		encode(project),
		encode(org)
	);
	let response: NamespaceResponse = cloud
		.request(
			Method::POST,
			&create_path,
			Some(json!({ "displayName": namespace })),
		)
		.await?
		.context("namespace create returned no body")?;
	Ok(response.namespace)
}

/// Looks up an existing namespace, erroring if it does not exist. Unlike
/// `ensure_namespace`, this never creates the namespace, which is the correct
/// behavior for read-only commands.
pub async fn get_namespace(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
) -> Result<Namespace> {
	resolve_namespace(cloud, project, org, namespace)
		.await?
		.with_context(|| format!("namespace not found: {namespace}"))
}

pub async fn create_or_update_pool(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	pool: &str,
	body: Value,
) -> Result<()> {
	let path = format!(
		"/projects/{}/namespaces/{}/managed-pools/{}?org={}",
		encode(project),
		encode(namespace),
		encode(pool),
		encode(org)
	);
	let _: Option<Value> = cloud.request_ok(Method::PUT, &path, Some(body)).await?;
	Ok(())
}

/// Returns whether a managed pool already exists for the namespace. Used by
/// `--reuse-image` deploys, which must not enable a new pool and instead require
/// an existing one to reuse its image.
pub async fn pool_exists(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	pool: &str,
) -> Result<bool> {
	Ok(get_pool(cloud, project, org, namespace, pool)
		.await?
		.is_some())
}

async fn get_pool(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	pool: &str,
) -> Result<Option<ManagedPool>> {
	let path = format!(
		"/projects/{}/namespaces/{}/managed-pools/{}?org={}",
		encode(project),
		encode(namespace),
		encode(pool),
		encode(org)
	);
	Ok(cloud
		.request::<ManagedPoolResponse>(Method::GET, &path, None)
		.await?
		.and_then(|r| r.managed_pool))
}

pub async fn wait_for_pool(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	pool: &str,
	throw_on_error: bool,
) -> Result<()> {
	// Include the pool name in status logs only when it is not the default, so
	// the common single-pool case stays uncluttered. A `None` field is omitted.
	let pool_field = (pool != crate::POOL_NAME).then_some(pool);
	for _ in 0..180 {
		let pool = get_pool(cloud, project, org, namespace, pool)
			.await?
			.context("managed pool disappeared while polling")?;
		let status = pool.status.unwrap_or_else(|| "unknown".to_string());
		tracing::info!(pool = pool_field, %status, "pool status");
		match status.as_str() {
			"ready" => return Ok(()),
			"error" if throw_on_error => {
				bail!(
					"managed pool entered error state: {}",
					pool.error
						.and_then(|e| e.message)
						.unwrap_or_else(|| "unknown error".to_string())
				);
			}
			"error" => return Ok(()),
			_ => sleep(Duration::from_secs(2)).await,
		}
	}
	bail!("timed out waiting for managed pool to become ready")
}

pub async fn list_pools(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
) -> Result<Vec<PoolSummary>> {
	let mut pools = Vec::new();
	let mut cursor: Option<String> = None;
	loop {
		let mut path = format!(
			"/projects/{}/namespaces/{}/managed-pools?org={}&limit=100",
			encode(project),
			encode(namespace),
			encode(org)
		);
		if let Some(cursor) = &cursor {
			path.push_str(&format!("&cursor={}", encode(cursor)));
		}
		let Some(response) = cloud
			.request::<ManagedPoolsListResponse>(Method::GET, &path, None)
			.await?
		else {
			break;
		};
		pools.extend(response.managed_pools);
		match response.pagination.and_then(|p| p.cursor) {
			Some(next) => cursor = Some(next),
			None => break,
		}
	}
	Ok(pools)
}

/// Deletes a managed pool. The Cloud API is fire-and-forget: it starts the
/// teardown and returns without waiting for it to finish.
pub async fn delete_pool(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	pool: &str,
) -> Result<()> {
	let path = format!(
		"/projects/{}/namespaces/{}/managed-pools/{}?org={}",
		encode(project),
		encode(namespace),
		encode(pool),
		encode(org)
	);
	let response = cloud
		.build_request(Method::DELETE, &path, None)?
		.send()
		.await
		.context("Cloud API request failed")?;
	let status = response.status();
	if status == StatusCode::NOT_FOUND {
		bail!("pool not found: {pool}");
	}
	if !status.is_success() {
		let text = response.text().await.unwrap_or_default();
		bail!("Cloud API error {status}: {text}");
	}
	Ok(())
}

/// Mints a namespace-scoped token via `POST .../tokens/{kind}`, where `kind` is
/// the Cloud API path segment (`secret`, `publishable`, or `connection`). These
/// endpoints are get-or-create: repeated calls return the same token.
pub async fn create_token(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	kind: &str,
) -> Result<String> {
	let path = format!(
		"/projects/{}/namespaces/{}/tokens/{}?org={}",
		encode(project),
		encode(namespace),
		kind,
		encode(org)
	);
	let response: TokenResponse = cloud
		.request_ok(Method::POST, &path, None)
		.await?
		.context("token create returned no body")?;
	Ok(response.token)
}

pub fn registry_endpoint(cloud_api: &str) -> Result<String> {
	let url = derive_endpoint(cloud_api, "registry")?;
	// Strip the scheme for Docker image references
	Ok(url
		.trim_start_matches("https://")
		.trim_start_matches("http://")
		.to_string())
}

pub fn dashboard_endpoint(cloud_api: &str) -> Result<String> {
	derive_endpoint(cloud_api, "dashboard")
}

fn derive_endpoint(input: &str, subdomain: &str) -> Result<String> {
	let mut url = Url::parse(input)?;
	let host = url.host_str().context("endpoint missing host")?;
	let next_host = if let Some(rest) = host.strip_prefix("cloud-api.") {
		format!("{subdomain}.{rest}")
	} else if let Some(rest) = host.strip_prefix("api.") {
		format!("{subdomain}.{rest}")
	} else {
		format!("{subdomain}.{host}")
	};
	url.set_host(Some(&next_host))?;
	url.set_path("");
	url.set_query(None);
	url.set_fragment(None);
	Ok(url.as_str().trim_end_matches('/').to_string())
}
