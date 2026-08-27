use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use gas::prelude::*;
use reqwest::header::{HeaderMap as ReqwestHeaderMap, HeaderName, HeaderValue};
use rivet_envoy_protocol::PROTOCOL_VERSION;
use rivetkit_shared_types::serverless_metadata::ServerlessMetadataPayload;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const RESPONSE_BODY_MAX_LEN: usize = 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Input {
	pub url: String,
	pub headers: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerlessMetadataError {
	InvalidRequest {},
	DestinationBlocked { reason: String },
	RequestFailed {},
	RequestTimedOut {},
	NonSuccessStatus { status_code: u16, body: String },
	InvalidResponseJson { body: String, parse_error: String },
	InvalidResponseSchema { runtime: String, version: String },
	InvalidEnvoyProtocolVersion { version: u16, max_supported: u16 },
}

/// Wire-format envelope for serverless metadata errors.
///
/// Surfaced to API clients with a stable `{message, details, metadata}` shape
/// regardless of which internal `ServerlessMetadataError` variant produced it.
/// `metadata.kind` discriminates the variant; per-variant fields live alongside
/// `kind`.
#[derive(Deserialize, Serialize, ToSchema, Clone, Debug, PartialEq, Eq)]
#[schema(as = RunnerConfigsServerlessMetadataError)]
pub struct ServerlessMetadataErrorEnvelope {
	pub message: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub details: Option<String>,
	#[serde(default)]
	pub metadata: serde_json::Value,
}

impl std::fmt::Display for ServerlessMetadataErrorEnvelope {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.message)
	}
}

impl From<ServerlessMetadataError> for ServerlessMetadataErrorEnvelope {
	fn from(err: ServerlessMetadataError) -> Self {
		match err {
			ServerlessMetadataError::InvalidRequest {} => Self {
				message: "invalid serverless metadata request".to_string(),
				details: None,
				metadata: serde_json::json!({ "kind": "invalid_request" }),
			},
			ServerlessMetadataError::DestinationBlocked { reason } => Self {
				message: "serverless endpoint is not an allowed destination".to_string(),
				details: Some(reason.clone()),
				metadata: serde_json::json!({
					"kind": "destination_blocked",
					"reason": reason,
				}),
			},
			ServerlessMetadataError::RequestFailed {} => Self {
				message: "failed to reach serverless endpoint".to_string(),
				details: None,
				metadata: serde_json::json!({ "kind": "request_failed" }),
			},
			ServerlessMetadataError::RequestTimedOut {} => Self {
				message: "serverless metadata request timed out".to_string(),
				details: None,
				metadata: serde_json::json!({ "kind": "request_timed_out" }),
			},
			ServerlessMetadataError::NonSuccessStatus { status_code, body } => Self {
				message: format!("serverless metadata request returned status {status_code}"),
				details: None,
				metadata: serde_json::json!({
					"kind": "non_success_status",
					"status_code": status_code,
					"body": body,
				}),
			},
			ServerlessMetadataError::InvalidResponseJson { body, parse_error } => Self {
				message: "serverless metadata response is not valid JSON".to_string(),
				details: None,
				metadata: serde_json::json!({
					"kind": "invalid_response_json",
					"body": body,
					"parse_error": parse_error,
				}),
			},
			ServerlessMetadataError::InvalidResponseSchema { runtime, version } => Self {
				message: format!("serverless runtime {runtime} version {version} is unsupported"),
				details: None,
				metadata: serde_json::json!({
					"kind": "invalid_response_schema",
					"runtime": runtime,
					"version": version,
				}),
			},
			ServerlessMetadataError::InvalidEnvoyProtocolVersion {
				version,
				max_supported,
			} => Self {
				message: format!(
					"envoy protocol version {version} is not supported (max supported: {max_supported})"
				),
				details: None,
				metadata: serde_json::json!({
					"kind": "invalid_envoy_protocol_version",
					"envoy_protocol_version": version,
					"max_supported_envoy_protocol_version": max_supported,
				}),
			},
		}
	}
}

#[derive(Debug, Clone)]
pub struct Output {
	pub runtime: String,
	pub version: String,
	pub envoy_protocol_version: Option<u16>,
	pub actor_names: Vec<ActorNameMetadata>,
	pub runner_version: Option<u32>,
	pub envoy_version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub struct ActorNameMetadata {
	pub name: String,
	pub metadata: serde_json::Map<String, serde_json::Value>,
}

fn truncate_response_body(body: &str) -> String {
	let mut chars = body.chars();
	let mut truncated: String = chars.by_ref().take(RESPONSE_BODY_MAX_LEN).collect();
	if chars.next().is_some() {
		truncated.push_str("...[truncated]");
	}

	truncated
}

/// Fetches metadata from a serverless runner at the given URL.
///
/// Returns metadata including runtime, version, and actor names if available.
#[operation]
#[tracing::instrument(skip_all)]
pub async fn pegboard_serverless_metadata_fetch(
	ctx: &OperationCtx,
	input: &Input,
) -> Result<std::result::Result<Output, ServerlessMetadataError>> {
	tracing::debug!(url = ?input.url, "fetching serverless runner metadata");

	let trimmed_url = input.url.trim();
	if trimmed_url.is_empty() {
		return Ok(Err(ServerlessMetadataError::InvalidRequest {}));
	}

	let metadata_url = format!("{}/metadata", trimmed_url.trim_end_matches('/'));

	let Ok(parsed_url) = reqwest::Url::parse(&metadata_url) else {
		return Ok(Err(ServerlessMetadataError::InvalidRequest {}));
	};

	// The client only sees this URL as a host to connect to: an address literal never reaches its
	// resolver, and its redirect policy only runs on later hops. So the scheme, credentials, and a
	// literal destination have to be checked here. This op also backs the health check endpoint,
	// where an unchecked URL is a probe for whatever the engine can reach.
	let policy = rivet_pools::reqwest::outbound_policy(ctx.config()).await?;
	if let Err(reason) = policy.check_url(&parsed_url) {
		return Ok(Err(ServerlessMetadataError::DestinationBlocked {
			reason: reason.to_string(),
		}));
	}

	let mut header_map = ReqwestHeaderMap::new();
	for (name, value) in &input.headers {
		let header_name = match HeaderName::from_bytes(name.trim().as_bytes()) {
			Ok(n) => n,
			Err(_) => return Ok(Err(ServerlessMetadataError::InvalidRequest {})),
		};

		let header_value = match HeaderValue::from_str(value.trim()) {
			Ok(v) => v,
			Err(_) => return Ok(Err(ServerlessMetadataError::InvalidRequest {})),
		};

		header_map.insert(header_name, header_value);
	}

	let client = match rivet_pools::reqwest::guarded_client(ctx.config()).await {
		Ok(c) => c,
		Err(_) => return Ok(Err(ServerlessMetadataError::RequestFailed {})),
	};

	tracing::debug!("sending metadata request");
	let response = match client
		.get(&metadata_url)
		.headers(header_map)
		.timeout(REQUEST_TIMEOUT)
		.send()
		.custom_instrument(tracing::info_span!("fetch_metadata_request"))
		.await
	{
		Ok(r) => r,
		Err(err) => {
			let is_timeout = err.is_timeout();
			let err = anyhow::Error::from(err);

			// A hostname that only resolves to disallowed addresses is rejected by the resolver,
			// which the pre-flight check above cannot see.
			if let Some(reason) = rivet_outbound_guard::block_reason(&err) {
				return Ok(Err(ServerlessMetadataError::DestinationBlocked {
					reason: reason.to_string(),
				}));
			}

			return Ok(Err(if is_timeout {
				ServerlessMetadataError::RequestTimedOut {}
			} else {
				ServerlessMetadataError::RequestFailed {}
			}));
		}
	};

	let status = response.status();
	tracing::debug!(?status, "received metadata response");
	let body_raw = response
		.text()
		.await
		.unwrap_or_else(|_| String::from("<failed to read body>"));
	let body_for_user = truncate_response_body(&body_raw);

	if !status.is_success() {
		return Ok(Err(ServerlessMetadataError::NonSuccessStatus {
			status_code: status.as_u16(),
			body: body_for_user,
		}));
	}

	let payload = match serde_json::from_str::<ServerlessMetadataPayload>(&body_raw) {
		Ok(p) => p,
		Err(err) => {
			return Ok(Err(ServerlessMetadataError::InvalidResponseJson {
				body: body_for_user,
				parse_error: err.to_string(),
			}));
		}
	};

	let ServerlessMetadataPayload {
		runtime,
		version,
		envoy_protocol_version,
		actor_names,
		envoy,
		runner,
		..
	} = payload;

	let runner_version = runner.and_then(|r| r.version);
	let envoy_version = envoy.and_then(|e| e.version);

	tracing::debug!(
		?runtime,
		?version,
		actor_names_count = actor_names.len(),
		?envoy_version,
		"parsed metadata payload"
	);

	let trimmed_version = version.trim();
	if runtime != "rivetkit" || trimmed_version.is_empty() {
		return Ok(Err(ServerlessMetadataError::InvalidResponseSchema {
			runtime,
			version,
		}));
	}

	if let Some(envoy_protocol_version) = envoy_protocol_version {
		if envoy_protocol_version < 1 || envoy_protocol_version > PROTOCOL_VERSION {
			return Ok(Err(ServerlessMetadataError::InvalidEnvoyProtocolVersion {
				version: envoy_protocol_version,
				max_supported: PROTOCOL_VERSION,
			}));
		}
	}

	// Convert actor names, filtering out non-object metadata
	let actor_names: Vec<ActorNameMetadata> = actor_names
		.into_iter()
		.filter_map(|(name, data)| {
			let metadata = data.metadata?.as_object()?.clone();
			Some(ActorNameMetadata { name, metadata })
		})
		.collect();

	Ok(Ok(Output {
		runtime,
		version: trimmed_version.to_owned(),
		envoy_protocol_version,
		actor_names,
		runner_version,
		envoy_version,
	}))
}
