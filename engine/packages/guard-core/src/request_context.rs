use anyhow::{Context, Result};
use hyper::{Method, header::HeaderMap};
use rivet_api_builder::X_RIVET_RAY_ID;
use rivet_runner_protocol as protocol;
use rivet_util::Id;
use std::{
	collections::HashMap,
	net::{IpAddr, SocketAddr},
	sync::Arc,
	time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

use crate::utils::InFlightPermit;

#[derive(Clone)]
pub struct RequestContext {
	pub(crate) remote_addr: SocketAddr,
	pub(crate) ray_id: Id,
	pub(crate) external_ray_id: String,
	pub(crate) req_id: Id,
	/// Entire host including port (if present)
	pub(crate) host: String,
	/// Only hostname, no port.
	pub(crate) hostname: String,
	/// Includes path and query.
	pub(crate) path: String,
	pub(crate) method: Method,
	pub(crate) headers: HeaderMap,
	pub(crate) is_websocket: bool,
	pub(crate) client_ip: IpAddr,
	pub(crate) start_time: Instant,
	pub(crate) request_body_exact_size: Option<u64>,
	pub(crate) request_body_is_end_stream: bool,
	pub(crate) client_disconnect: CancellationToken,

	pub(crate) retry: RetryConfig,
	pub(crate) timeout: TimeoutConfig,

	/// Holds the client's in-flight slot and this request's unique request id. Released when the
	/// last clone of the context is dropped.
	pub(crate) in_flight_permit: Option<Arc<InFlightPermit>>,
	pub(crate) cors: Option<CorsConfig>,
}

impl RequestContext {
	pub(crate) fn new(
		remote_addr: SocketAddr,
		ray_id: Id,
		external_ray_id: String,
		req_id: Id,
		host: String,
		path: String,
		method: Method,
		headers: HeaderMap,
		is_websocket: bool,
		client_ip: IpAddr,
		start_time: Instant,
		client_disconnect: CancellationToken,
		guard_config: &rivet_config::config::guard::Guard,
	) -> Self {
		let hostname = host.split(':').next().unwrap_or(&host).to_string();

		RequestContext {
			remote_addr,
			ray_id,
			external_ray_id,
			req_id,
			host,
			hostname,
			path,
			method,
			headers,
			is_websocket,
			client_ip,
			start_time,
			request_body_exact_size: None,
			request_body_is_end_stream: true,
			client_disconnect,

			retry: RetryConfig {
				max_attempts: guard_config.proxy_retry_max_attempts(),
				initial_interval: guard_config.proxy_retry_initial_interval_ms(),
			},
			timeout: TimeoutConfig {
				request_timeout: guard_config.upstream_request_timeout(),
			},

			in_flight_permit: None,
			cors: None,
		}
	}

	pub fn ray_id(&self) -> Id {
		self.ray_id
	}

	pub fn external_ray_id(&self) -> &str {
		&self.external_ray_id
	}

	/// Adds this request's validated external ray ID to actor headers.
	pub fn forward_ray(&self, headers: &mut HashMap<String, String>) {
		headers.insert(
			X_RIVET_RAY_ID.as_str().to_owned(),
			self.external_ray_id.clone(),
		);
	}

	pub fn req_id(&self) -> Id {
		self.req_id
	}

	pub fn host(&self) -> &str {
		&self.host
	}

	pub fn hostname(&self) -> &str {
		&self.hostname
	}

	pub fn path(&self) -> &str {
		&self.path
	}

	pub fn method(&self) -> &Method {
		&self.method
	}

	pub fn headers(&self) -> &HeaderMap {
		&self.headers
	}

	pub fn is_websocket(&self) -> bool {
		self.is_websocket
	}

	pub fn elapsed(&self) -> Duration {
		self.start_time.elapsed()
	}

	pub fn request_body_exact_size(&self) -> Option<u64> {
		self.request_body_exact_size
	}

	pub fn request_body_is_end_stream(&self) -> bool {
		self.request_body_is_end_stream
	}

	pub fn client_disconnect_token(&self) -> CancellationToken {
		self.client_disconnect.clone()
	}

	pub(crate) fn set_request_body_metadata(
		&mut self,
		exact_size: Option<u64>,
		is_end_stream: bool,
	) {
		self.request_body_exact_size = exact_size;
		self.request_body_is_end_stream = is_end_stream;
	}

	pub fn in_flight_request_id(&self) -> Result<protocol::RequestId> {
		self.in_flight_permit
			.as_ref()
			.map(|permit| permit.request_id())
			.context("no in flight request id acquired")
	}

	pub fn set_cors(&mut self, cors_config: CorsConfig) {
		self.cors = Some(cors_config);
	}
}

#[derive(Clone, Debug)]
pub struct RetryConfig {
	pub max_attempts: u32,
	pub initial_interval: u64, // in milliseconds
}

#[derive(Clone, Debug)]
pub struct TimeoutConfig {
	pub request_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct CorsConfig {
	pub allow_origin: String,
	pub allow_credentials: bool,
	pub expose_headers: String,

	// Only set for OPTIONS requests
	// TODO: Vec of Method
	pub allow_methods: Option<String>,
	pub allow_headers: Option<String>,
	// Seconds
	pub max_age: Option<u32>,
}
