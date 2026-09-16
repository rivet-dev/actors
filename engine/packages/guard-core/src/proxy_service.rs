use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
	Request, Response, StatusCode,
	body::{Body as _, Incoming as BodyIncoming},
	header::{HeaderName, HeaderValue},
};
use hyper_tungstenite;
use hyper_tungstenite::tungstenite::protocol::WebSocketConfig;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use moka::future::Cache;
use opentelemetry_http::{HeaderExtractor, HeaderInjector};
use rand::seq::SliceRandom;
use rivet_api_builder::{RequestIds, X_RIVET_RAY_ID};
use rivet_error::RivetError;
use rivet_metrics::GaugeGuardExt;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use rivet_runner_protocol as protocol;
use std::{
	net::SocketAddr,
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use url::Url;

use crate::RouteTarget;
use crate::request_context::RequestContext;
use crate::response_body::ResponseBody;
use crate::route::{CacheKeyFn, ResolveRouteOutput, RouteCache, RoutingFn, RoutingOutput};
use crate::utils::{AdmissionRejection, ClientState, InFlightPermit};
use crate::{
	WebSocketHandle, custom_serve::HibernationResult, errors, metrics, task_group::TaskGroup, utils,
};

pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
pub const X_RIVET_ERROR: HeaderName = HeaderName::from_static("x-rivet-error");

const WEBSOCKET_CLOSE_LINGER: Duration = Duration::from_millis(5); // Keep TCP connection open briefly after WebSocket close
const MAX_EXTERNAL_RAY_ID_LEN: usize = 128;

/// Returns `value` when it is a ray ID actors accept: 1 to 128 characters of
/// `[A-Za-z0-9_-]`. RivetKit enforces the same bound on its side. The engine
/// cannot depend on RivetKit crates, so the rule is repeated here.
fn bounded_external_ray_id(value: &str) -> Option<&str> {
	if !value.is_empty()
		&& value.len() <= MAX_EXTERNAL_RAY_ID_LEN
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
	{
		Some(value)
	} else {
		None
	}
}

fn websocket_config(guard_config: &rivet_config::config::guard::Guard) -> WebSocketConfig {
	WebSocketConfig::default()
		.max_message_size(Some(guard_config.websocket_max_message_size()))
		.max_frame_size(Some(guard_config.websocket_max_frame_size()))
}

// State shared across all request handlers
pub struct ProxyState {
	config: rivet_config::Config,
	routing_fn: RoutingFn,
	cache_key_fn: CacheKeyFn,
	// NOTE: Using the hyper legacy client is the only option currently.
	// This is what reqwest uses under the hood. Eventually we'll migrate to h3 once it's ready.
	client: Client<
		hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
		Full<Bytes>,
	>,
	route_cache: RouteCache,
	// We use moka::Cache instead of scc::HashMap because it automatically handles TTL and capacity
	client_states: Option<Cache<std::net::IpAddr, Arc<parking_lot::Mutex<ClientState>>>>,
	// Entries are owned by an InFlightPermit for the lifetime of each active request, so live IDs
	// cannot be evicted and handed out to a second concurrent request.
	in_flight_requests: Arc<scc::HashSet<protocol::RequestId>>,

	tasks: Arc<TaskGroup>,
}

impl ProxyState {
	pub fn new(
		config: rivet_config::Config,
		routing_fn: RoutingFn,
		cache_key_fn: CacheKeyFn,
	) -> Self {
		let guard_config = config.guard();
		let https_connector_builder =
			match hyper_rustls::HttpsConnectorBuilder::new().with_native_roots() {
				Ok(builder) => builder,
				Err(err) => {
					tracing::warn!(
						?err,
						"failed to load native TLS roots; falling back to webpki roots"
					);
					hyper_rustls::HttpsConnectorBuilder::new().with_webpki_roots()
				}
			};
		let https_connector = https_connector_builder
			.https_or_http()
			.enable_http1()
			.enable_http2()
			.build();
		let client = Client::builder(TokioExecutor::new())
			.pool_idle_timeout(guard_config.http_client_pool_idle_timeout())
			.build(https_connector);
		let route_cache_ttl = guard_config.route_cache_ttl();
		let client_states =
			if guard_config.rate_limit().is_some() || guard_config.max_in_flight().is_some() {
				Some(
					Cache::builder()
						.max_capacity(guard_config.admission_client_state_cache_capacity())
						.time_to_idle(guard_config.admission_client_state_cache_idle_timeout())
						.build(),
				)
			} else {
				metrics::RATE_LIMITER_COUNT.set(0);
				metrics::IN_FLIGHT_COUNTER_COUNT.set(0);
				None
			};

		Self {
			config,
			routing_fn,
			cache_key_fn,
			client,
			route_cache: RouteCache::new(route_cache_ttl),
			client_states,
			in_flight_requests: Arc::new(scc::HashSet::new()),
			tasks: TaskGroup::new(),
		}
	}

	#[tracing::instrument(skip_all)]
	async fn resolve_route(
		&self,
		req_ctx: &mut RequestContext,
		ignore_cache: bool,
	) -> Result<ResolveRouteOutput> {
		tracing::debug!(
			hostname = %req_ctx.hostname,
			path = %req_ctx.path,
			method = %req_ctx.method,
			"Resolving route for request"
		);

		let cache_key = (self.cache_key_fn)(req_ctx)?;

		// Check cache first
		let cache_res = if !ignore_cache {
			self.route_cache.get(&cache_key).await
		} else {
			None
		};

		let res = if let Some(res) = cache_res {
			res
		} else {
			// Not in cache, call routing function with a configured backstop timeout.
			// Primary timeout signals live in per-phase fences inside each routing module.
			let route_timeout = self.config.guard().route_timeout();
			tracing::debug!(
				hostname = %req_ctx.hostname,
				path = %req_ctx.path,
				cache_hit = false,
				timeout_seconds = route_timeout.as_secs(),
				"Cache miss, calling routing function"
			);

			let routing_res = timeout(route_timeout, (self.routing_fn)(req_ctx))
				.await
				.map_err(|_| {
					errors::RequestTimeout {
						phase: "route_resolution".to_owned(),
						timeout_seconds: route_timeout.as_secs(),
					}
					.build()
				})??;

			// TODO: Disable route caching for now, determine edge cases with gateway
			// // Cache the result
			// self.route_cache
			// 	.insert(cache_key, routing_res.clone())
			// 	.await;
			// tracing::debug!("Added route to cache");

			routing_res
		};

		match res {
			RoutingOutput::Route(result) => {
				tracing::debug!(
					hostname = %req_ctx.hostname,
					path = %req_ctx.path,
					targets_count = result.targets.len(),
					"Received routing result"
				);

				// Choose a random target
				if let Some(target) = choose_random_target(&result.targets) {
					tracing::debug!(
						hostname = %req_ctx.hostname,
						path = %req_ctx.path,
						target_host = %target.host,
						target_port = target.port,
						target_path = %target.path,
						"Selected target for request"
					);
					Ok(ResolveRouteOutput::Target(target.clone()))
				} else {
					tracing::warn!(
						hostname = %req_ctx.hostname,
						path = %req_ctx.path,
						"No route targets available from result"
					);
					Err(errors::NoRouteTargets {
						hostname: req_ctx.hostname.clone(),
						path: req_ctx.path.clone(),
					}
					.build())
				}
			}
			RoutingOutput::CustomServe(handler) => {
				tracing::debug!(
					hostname = %req_ctx.hostname,
					path = %req_ctx.path,
					"Routing returned custom serve handler"
				);
				Ok(ResolveRouteOutput::CustomServe(handler))
			}
		}
	}

	/// Get the throttling state for a client, creating it if it does not exist yet.
	#[tracing::instrument(level = "debug", skip_all)]
	async fn client_state(
		&self,
		req_ctx: &RequestContext,
	) -> Option<Arc<parking_lot::Mutex<ClientState>>> {
		let client_states = self.client_states.as_ref()?;
		let guard_config = self.config.guard();
		let entry = client_states
			.entry(req_ctx.client_ip)
			.or_insert_with(async {
				Arc::new(parking_lot::Mutex::new(ClientState::new(
					guard_config.rate_limit().map(|rate_limit| {
						(
							rate_limit.requests,
							Duration::from_millis(rate_limit.period_ms),
						)
					}),
					guard_config.max_in_flight(),
				)))
			})
			.await;

		if entry.is_fresh() {
			let count = client_states.entry_count() as i64;
			if guard_config.rate_limit().is_some() {
				metrics::RATE_LIMITER_COUNT.set(count);
			}
			if guard_config.max_in_flight().is_some() {
				metrics::IN_FLIGHT_COUNTER_COUNT.set(count);
			}
		}

		Some(entry.into_value())
	}

	/// Applies configured admission controls and assigns a unique protocol request ID. Request IDs
	/// are allocated even when both admission controls are disabled.
	///
	/// On success the in-flight slot and the request id are owned by the permit stored on the
	/// request context, and are released once every clone of that context is dropped.
	#[tracing::instrument(level = "debug", skip_all)]
	async fn admit_request(
		&self,
		req_ctx: &mut RequestContext,
	) -> Result<Option<AdmissionRejection>> {
		let client_state = self.client_state(req_ctx).await;

		if let Some(client_state) = &client_state
			&& let Err(rejection) = client_state.lock().try_admit()
		{
			return Ok(Some(rejection));
		}

		// The in-flight slot is held but not owned by anything yet. There are no await points before
		// the permit takes ownership below, so the slot cannot be lost to cancellation here and only
		// needs releasing on the error path.
		let request_id = match self.generate_unique_in_flight_request_id() {
			Ok(request_id) => request_id,
			Err(err) => {
				if let Some(client_state) = &client_state {
					client_state.lock().release_in_flight();
				}
				return Err(err);
			}
		};

		req_ctx.in_flight_permit = Some(Arc::new(InFlightPermit::new(
			client_state,
			self.in_flight_requests.clone(),
			request_id,
		)));

		Ok(None)
	}

	/// Generate a request ID that is not currently in flight.
	fn generate_unique_in_flight_request_id(&self) -> Result<protocol::RequestId> {
		const MAX_TRIES: u32 = 100;

		for attempt in 0..MAX_TRIES {
			let request_id = protocol::util::generate_request_id();

			if self.in_flight_requests.insert_sync(request_id).is_ok() {
				return Ok(request_id);
			}

			// Collision occurred (extremely rare with 4 bytes = 4 billion possibilities)
			// Generate a new ID and try again
			tracing::warn!(
				?request_id,
				attempt,
				"request id collision, generating new id"
			);
		}

		bail!(
			"failed to generate unique request id after {} attempts",
			MAX_TRIES
		);
	}
}

// Helper function to choose a random target from a list of targets
fn choose_random_target(targets: &[RouteTarget]) -> Option<&RouteTarget> {
	targets.choose(&mut rand::thread_rng())
}

// Proxy service
pub struct ProxyService {
	state: Arc<ProxyState>,
	remote_addr: SocketAddr,
	connection_start: Instant,
	client_disconnect: CancellationToken,
}

impl ProxyService {
	pub fn new(state: Arc<ProxyState>, remote_addr: SocketAddr) -> Self {
		Self {
			state,
			remote_addr,
			connection_start: Instant::now(),
			client_disconnect: CancellationToken::new(),
		}
	}

	pub(crate) fn client_disconnect_token(&self) -> CancellationToken {
		self.client_disconnect.clone()
	}

	/// Process an individual request.
	#[tracing::instrument(name = "guard_request", skip_all, fields(ray_id, req_id, uri=%req.uri()))]
	pub async fn process(&self, mut req: Request<BodyIncoming>) -> Result<Response<ResponseBody>> {
		let start_time = Instant::now();

		let request_ids = RequestIds::new(self.state.config.dc_label());
		req.extensions_mut().insert(request_ids);
		let external_ray_id = req
			.headers()
			.get(X_RIVET_RAY_ID)
			.and_then(|value| value.to_str().ok())
			.and_then(bounded_external_ray_id)
			.map(str::to_owned)
			.unwrap_or_else(|| request_ids.ray_id.to_string());
		if let Ok(value) = HeaderValue::from_str(&external_ray_id) {
			req.headers_mut().insert(X_RIVET_RAY_ID, value);
		}

		let current_span = tracing::Span::current();

		// Extract trace context from incoming request headers and use it as the parent of
		// the current span, then inject the current span's context back into the request
		// headers so upstream services see this request as a child of the current span.
		// The injected headers ride along via `req_ctx.headers` into both the HTTP and
		// WebSocket upstream paths.
		if self.state.config.guard().trace_propagation() {
			let parent_ctx = opentelemetry::global::get_text_map_propagator(|prop| {
				prop.extract(&HeaderExtractor(req.headers()))
			});
			current_span.set_parent(parent_ctx);

			let span_ctx = current_span.context();
			opentelemetry::global::get_text_map_propagator(|prop| {
				prop.inject_context(&span_ctx, &mut HeaderInjector(req.headers_mut()))
			});
		}

		current_span.record("req_id", request_ids.req_id.to_string());
		current_span.record("ray_id", external_ray_id.as_str());

		// Extract request information for logging and analytics before consuming the request
		let host = req
			.headers()
			.get(hyper::header::HOST)
			.and_then(|h| h.to_str().ok())
			.unwrap_or("unknown")
			.to_string();
		let uri_string = req.uri().to_string();
		let path = req
			.uri()
			.path_and_query()
			.map(|x| x.to_string())
			.unwrap_or_else(|| req.uri().path().to_string());
		let method = req.method().clone();

		current_span.set_attribute("http.request.method", method.to_string());
		current_span.set_attribute("http.path", uri_string.clone());

		let user_agent = req
			.headers()
			.get(hyper::header::USER_AGENT)
			.and_then(|h| h.to_str().ok())
			.map(|s| s.to_string());

		// Extract IP address from X-Forwarded-For header or fall back to remote_addr
		let client_ip = req
			.headers()
			.get(X_FORWARDED_FOR)
			.and_then(|h| h.to_str().ok())
			.and_then(|forwarded| {
				// X-Forwarded-For can be a comma-separated list, take the first IP
				forwarded.split(',').next().map(|s| s.trim())
			})
			.and_then(|ip_str| ip_str.parse::<std::net::IpAddr>().ok())
			.unwrap_or_else(|| self.remote_addr.ip());

		let is_websocket = hyper_tungstenite::is_upgrade_request(&req);
		let request_body_exact_size = req.body().size_hint().exact();
		let request_body_is_end_stream = req.body().is_end_stream();
		let mut req_ctx = RequestContext::new(
			self.remote_addr,
			request_ids.ray_id,
			external_ray_id,
			request_ids.req_id,
			host,
			path,
			req.method().clone(),
			req.headers().clone(),
			is_websocket,
			client_ip,
			start_time,
			self.client_disconnect.clone(),
			self.state.config.guard(),
		);
		req_ctx.set_request_body_metadata(request_body_exact_size, request_body_is_end_stream);

		// TLS information would be set here if available (for HTTPS connections)
		// This requires TLS connection introspection and is marked for future enhancement

		// Debug log request information with structured fields (Apache-like access log)
		tracing::debug!(
			ray_id=%req_ctx.external_ray_id,
			req_id=?req_ctx.req_id,
			method=%req_ctx.method,
			path=%req_ctx.path,
			host=%req_ctx.host,
			remote_addr=%req_ctx.remote_addr,
			uri=%uri_string,
			user_agent=?user_agent,
			"Request received"
		);

		// Used for ws error proxying later
		let mut mock_req_builder = Request::builder()
			.method(req.method().clone())
			.uri(req.uri().clone())
			.version(req.version().clone());
		if let Some(headers) = mock_req_builder.headers_mut() {
			*headers = req.headers().clone();
		}
		if let Some(extensions) = mock_req_builder.extensions_mut() {
			*extensions = req.extensions().clone();
		}
		let mock_req = mock_req_builder.body(())?;

		// Process the request
		let mut res = match self.handle_request(req, &mut req_ctx).await {
			Ok(res) => res,
			Err(err) => {
				// Log the error
				if err
					.chain()
					.find_map(|x| x.downcast_ref::<RivetError>())
					.is_some()
				{
					tracing::warn!(?err, "Request failed");
				} else {
					tracing::error!(?err, "Request failed");
				}

				metrics::PROXY_REQUEST_ERROR_TOTAL
					.with_label_values(&[&utils::error_metric_label(&err)])
					.inc();

				// If we receive an error during a websocket request, we attempt to open the websocket anyway
				// so we can send the error via websocket instead of http. Most websocket clients don't handle
				// HTTP errors in a meaningful way resulting in unhelpful errors for the user
				if is_websocket {
					tracing::debug!("Upgrading client connection to WebSocket for error proxy");
					match hyper_tungstenite::upgrade(
						mock_req,
						Some(websocket_config(self.state.config.guard())),
					) {
						Ok((client_response, client_ws)) => {
							tracing::debug!("Client WebSocket upgrade for error proxy successful");

							let active_guard = metrics::WEBSOCKET_ACTIVE.inc_guard();
							let external_ray_id = req_ctx.external_ray_id.clone();

							self.state.tasks.spawn(
								async move {
									let _active_guard = active_guard;

									let ws_handle = match WebSocketHandle::new(client_ws).await {
										Ok(ws_handle) => ws_handle,
										Err(err) => {
											tracing::debug!(
												?err,
												"failed initiating websocket handle for error proxy"
											);
											return;
										}
									};
									let frame = utils::err_to_close_frame(err, &external_ray_id);

									// Manual conversion to handle different tungstenite versions
									let code_num: u16 = frame.code.into();
									let reason = frame.reason.clone();

									if let Err(err) = ws_handle
										.send(tokio_tungstenite::tungstenite::Message::Close(Some(
											tokio_tungstenite::tungstenite::protocol::CloseFrame {
												code: code_num.into(),
												reason,
											},
										)))
										.await
									{
										tracing::debug!(
											?err,
											"failed sending websocket error proxy"
										);
									}

									// Flush to ensure close frame is sent
									if let Err(err) = ws_handle.flush().await {
										tracing::debug!(
											?err,
											"failed flushing websocket in error proxy"
										);
									}

									// Keep TCP connection open briefly to allow client to process close
									tokio::time::sleep(WEBSOCKET_CLOSE_LINGER).await;
								}
								.instrument(tracing::debug_span!("ws_error_proxy_task")),
							);

							// Return the response that will upgrade the client connection
							// For proper WebSocket handshaking, we need to preserve the original response
							// structure but convert it to our expected return type without modifying its content
							tracing::debug!(
								"Returning WebSocket upgrade response for error proxy to client"
							);
							// Extract the parts from the response but preserve all headers and status
							let (mut parts, _) = client_response.into_parts();

							// Add Sec-WebSocket-Protocol header to the response
							// Many WebSocket clients (e.g. node-ws & Cloudflare) require a protocol in the response
							parts.headers.insert(
								"sec-websocket-protocol",
								hyper::header::HeaderValue::from_static("rivet"),
							);

							// Create a new response with an empty body - WebSocket upgrades don't need a body
							Response::from_parts(
								parts,
								ResponseBody::Full(Full::<Bytes>::new(Bytes::new())),
							)
						}
						Err(err) => {
							tracing::error!(
								?err,
								"Failed to upgrade client WebSocket for error proxy"
							);

							utils::err_into_response(
								errors::ConnectionError {
									error_message: format!(
										"Failed to upgrade client WebSocket for error proxy: {}",
										err
									),
									remote_addr: req_ctx.remote_addr.to_string(),
								}
								.build(),
							)?
						}
					}
				} else {
					utils::err_into_response(err)?
				}
			}
		};

		if is_websocket && res.status() != StatusCode::SWITCHING_PROTOCOLS {
			tracing::debug!("returned non-101 response to websocket");
		}

		// Add ray_id to response headers
		if let Ok(ray_id_value) = HeaderValue::from_str(req_ctx.external_ray_id()) {
			if let Some(existing_ray_id_value) = res
				.headers()
				.get(X_RIVET_RAY_ID)
				.and_then(|h| h.to_str().ok())
			{
				if ray_id_value != existing_ray_id_value {
					tracing::warn!(
						expected_ray_id=%req_ctx.external_ray_id,
						received_ray_id=%existing_ray_id_value,
						"downstream service set ray id header to a different value",
					);
				}
			}

			res.headers_mut().insert(X_RIVET_RAY_ID, ray_id_value);
		}

		// Add cors headers to response
		if let Some(cors) = &req_ctx.cors {
			let headers = res.headers_mut();

			headers.insert(
				"access-control-allow-origin",
				HeaderValue::from_str(&cors.allow_origin)?,
			);
			headers.insert(
				"access-control-allow-credentials",
				HeaderValue::from_static(if cors.allow_credentials {
					"true"
				} else {
					"false"
				}),
			);
			headers.insert(
				"access-control-expose-headers",
				HeaderValue::from_str(&cors.expose_headers)?,
			);

			if let Some(allow_methods) = &cors.allow_methods {
				headers.insert(
					"access-control-allow-methods",
					HeaderValue::from_str(allow_methods)?,
				);
			}

			if let Some(allow_headers) = &cors.allow_headers {
				headers.insert(
					"access-control-allow-headers",
					HeaderValue::from_str(allow_headers)?,
				);
			}

			if let Some(max_age) = &cors.max_age {
				headers.insert(
					"access-control-max-age",
					HeaderValue::from_str(&max_age.to_string())?,
				);
			}

			// Add Vary header to prevent cache poisoning when echoing origin
			if cors.allow_origin != "*" {
				headers.insert("vary", HeaderValue::from_static("Origin"));
			}
		}

		// Set span status code
		let status = res.status().as_u16();
		current_span.set_attribute("http.response.status_code", status as i64);

		let content_length = res
			.headers()
			.get(hyper::header::CONTENT_LENGTH)
			.and_then(|h| h.to_str().ok())
			.and_then(|s| s.parse::<usize>().ok())
			.unwrap_or(0);

		// Log information about the completed request
		tracing::debug!(
			ray_id=%req_ctx.external_ray_id,
			req_id=?req_ctx.req_id,
			method = %req_ctx.method,
			path = %req_ctx.path,
			host = %req_ctx.host,
			remote_addr = %req_ctx.remote_addr,
			status = %status,
			content_length = %content_length,
			"Request completed"
		);

		Ok(res)
	}

	#[tracing::instrument(skip_all)]
	async fn handle_request(
		&self,
		req: Request<BodyIncoming>,
		req_ctx: &mut RequestContext,
	) -> Result<Response<ResponseBody>> {
		// Resolve target
		let target_res = self.state.resolve_route(req_ctx, false).await;

		let duration_secs = req_ctx.start_time.elapsed().as_secs_f64();
		metrics::RESOLVE_ROUTE_DURATION.observe(duration_secs);

		let target = target_res?;

		// Apply configured admission controls and assign the protocol request ID. The permit lives
		// on the request context and releases any in-flight slot plus the request ID when the context
		// is dropped.
		if let Some(rejection) = self.state.admit_request(req_ctx).await? {
			return match rejection {
				AdmissionRejection::RateLimit => {
					let rate_limit = self
						.state
						.config
						.guard()
						.rate_limit()
						.context("rate limit rejected request without rate limit config")?;
					Err(errors::RateLimit {
						method: req_ctx.method.to_string(),
						path: req_ctx.path.clone(),
						ip: req_ctx.client_ip.to_string(),
						requests: rate_limit.requests,
						period_ms: rate_limit.period_ms,
					}
					.build())
				}
				AdmissionRejection::MaxInFlight => {
					let max_in_flight =
						self.state.config.guard().max_in_flight().context(
							"in-flight limit rejected request without max-in-flight config",
						)?;
					Err(errors::MaxInFlight {
						method: req_ctx.method.to_string(),
						path: req_ctx.path.clone(),
						ip: req_ctx.client_ip.to_string(),
						max_in_flight,
					}
					.build())
				}
			};
		}

		// Increment metrics
		let _pending_guard = metrics::PROXY_REQUEST_PENDING.inc_guard();
		metrics::PROXY_REQUEST_TOTAL.inc();

		let is_websocket = hyper_tungstenite::is_upgrade_request(&req);
		let res = if is_websocket {
			self.handle_websocket_upgrade(req, req_ctx, target).await
		} else {
			self.handle_http_request(req, req_ctx, target).await
		};

		let status = match &res {
			Ok(resp) => resp.status().as_u16().to_string(),
			Err(_) => "error".to_string(),
		};

		// Record metrics
		let duration_secs = req_ctx.start_time.elapsed().as_secs_f64();
		metrics::PROXY_REQUEST_DURATION
			.with_label_values(&[status])
			.observe(duration_secs);

		res
	}

	#[tracing::instrument(skip_all)]
	async fn handle_http_request(
		&self,
		req: Request<BodyIncoming>,
		req_ctx: &mut RequestContext,
		resolved_route: ResolveRouteOutput,
	) -> Result<Response<ResponseBody>> {
		// Set up retry with backoff
		let timeout_duration = req_ctx.timeout.request_timeout;

		match resolved_route {
			ResolveRouteOutput::Target(mut target) => {
				// Read the request body before proceeding with retries
				let (req_parts, body) = req.into_parts();
				let req_body =
					Limited::new(body, self.state.config.guard().http_max_request_body_size())
						.collect()
						.await
						.map_err(|err| {
							errors::InvalidRequestBody {
								reason: err.to_string(),
							}
							.build()
						})?
						.to_bytes();

				// Use a value-returning loop to handle both errors and successful responses
				let mut attempts = 0;
				let mut last_status = "none".to_owned();
				let mut last_error_code = "none".to_owned();
				while attempts < req_ctx.retry.max_attempts {
					attempts += 1;

					// Use the common function to build request parts
					let builder = utils::proxied_request_builder(&req_parts, req_ctx, &target)
						.map_err(|err| errors::HttpRequestBuildFailed(err.to_string()).build())?;

					// Create the final request with body
					let proxied_req = builder
						// NOTE: the `Bytes` type is cheaply cloneable, this is not resource intensive
						.body(Full::new(req_body.clone()))
						.map_err(|err| errors::RequestBuildError(err.to_string()).build())?;

					// Send the request with timeout
					let res = timeout(timeout_duration, self.state.client.request(proxied_req))
						.await
						.map_err(|_| {
							errors::RequestTimeout {
								phase: "upstream_request".to_owned(),
								timeout_seconds: timeout_duration.as_secs(),
							}
							.build()
						})?;

					match res {
						Ok(resp) => {
							// Check if this is a retryable response
							if utils::should_retry_request_inner(resp.status(), resp.headers()) {
								last_status = resp.status().as_u16().to_string();
								last_error_code = resp
									.headers()
									.get(X_RIVET_ERROR)
									.and_then(|value| value.to_str().ok())
									.unwrap_or("retryable_status")
									.to_owned();

								// Request connect error, might retry
								tracing::debug!(
									"Request attempt {attempts} failed (service unavailable)"
								);

								// Use backoff and continue
								let backoff = utils::calculate_backoff(
									attempts,
									req_ctx.retry.initial_interval,
								);
								tokio::time::sleep(backoff).await;

								// Resolve target again, this time ignoring cache. This makes sure
								// we always re-fetch the route on error
								let ResolveRouteOutput::Target(new_target) =
									self.state.resolve_route(req_ctx, true).await?
								else {
									bail!("resolved route does not match Target");
								};
								target = new_target;

								continue;
							}

							let (parts, body) = resp.into_parts();

							// Check if this is a streaming response by examining headers
							// let is_streaming = parts.headers.get("content-type")
							// 	.and_then(|ct| ct.to_str().ok())
							// 	.map(|ct| ct.contains("text/event-stream") || ct.contains("application/stream"))
							// 	.unwrap_or(false);
							let is_streaming = true;

							if is_streaming {
								// For streaming responses, pass through the body without buffering
								tracing::debug!("Detected streaming response, preserving stream");

								let streaming_body = ResponseBody::Incoming(body);
								return Ok(Response::from_parts(parts, streaming_body));
							} else {
								// For non-streaming responses, buffer as before
								let body_bytes = Limited::new(
									body,
									self.state.config.guard().http_max_request_body_size(),
								)
								.collect()
								.await
								.map_err(|err| {
									errors::InvalidResponseBody {
										reason: err.to_string(),
									}
									.build()
								})?
								.to_bytes();

								let full_body = ResponseBody::Full(Full::new(body_bytes));
								return Ok(Response::from_parts(parts, full_body));
							}
						}
						Err(err) => {
							if !err.is_connect() || attempts >= req_ctx.retry.max_attempts {
								tracing::error!(
									?err,
									?target,
									"Request error after {} attempts",
									attempts
								);

								return Err(errors::UpstreamError(format!(
									"Failed to connect to runner: {err}. Make sure your runners are healthy."
								))
								.build());
							} else {
								// Request connect error, might retry
								tracing::debug!(?err, "Request attempt {attempts} failed");

								// Use backoff and continue
								let backoff = utils::calculate_backoff(
									attempts,
									req_ctx.retry.initial_interval,
								);
								tokio::time::sleep(backoff).await;

								// Resolve target again, this time ignoring cache. This makes sure
								// we always re-fetch the route on error
								let ResolveRouteOutput::Target(new_target) =
									self.state.resolve_route(req_ctx, true).await?
								else {
									bail!("resolved route does not match Target");
								};
								target = new_target;

								continue;
							}
						}
					}
				}

				// If we get here, all attempts failed
				return Err(errors::RetryAttemptsExceeded {
					attempts: req_ctx.retry.max_attempts,
					last_error_code,
					last_status,
					last_target_kind: "target".to_owned(),
				}
				.build());
			}
			ResolveRouteOutput::CustomServe(mut handler) => {
				let mut prepare_attempts = 0;
				while handler.streams_request_body() {
					prepare_attempts += 1;
					match handler.prepare_streaming_request(req_ctx).await {
						Ok(()) => return handler.handle_streaming_request(req, req_ctx).await,
						Err(error)
							if prepare_attempts < req_ctx.retry.max_attempts
								&& utils::should_retry_error(&error) =>
						{
							let backoff = utils::calculate_backoff(
								prepare_attempts,
								req_ctx.retry.initial_interval,
							);
							tokio::time::sleep(backoff).await;
							let ResolveRouteOutput::CustomServe(new_handler) =
								self.state.resolve_route(req_ctx, true).await?
							else {
								bail!("resolved route does not match CustomServe");
							};
							handler = new_handler;
						}
						Err(error) => return Err(error),
					}
				}

				// Collect request body
				let (req_parts, body) = req.into_parts();
				let req_body =
					Limited::new(body, self.state.config.guard().http_max_request_body_size())
						.collect()
						.await
						.map_err(|err| {
							errors::InvalidRequestBody {
								reason: err.to_string(),
							}
							.build()
						})?
						.to_bytes();
				let req_collected =
					hyper::Request::from_parts(req_parts, Full::<Bytes>::new(req_body));

				// Attempt request
				let mut attempts = 0;
				let mut last_status = "none".to_owned();
				let mut last_error_code = "none".to_owned();
				while attempts < req_ctx.retry.max_attempts {
					attempts += 1;

					let res = handler.handle_request(req_collected.clone(), req_ctx).await;
					if utils::should_retry_request(&res) {
						match &res {
							Ok(resp) => {
								last_status = resp.status().as_u16().to_string();
								last_error_code = resp
									.headers()
									.get(X_RIVET_ERROR)
									.and_then(|value| value.to_str().ok())
									.unwrap_or("retryable_status")
									.to_owned();
							}
							Err(err) => {
								last_status = "none".to_owned();
								last_error_code = err
									.chain()
									.find_map(|x| x.downcast_ref::<rivet_error::RivetError>())
									.map(|rivet_err| {
										format!("{}.{}", rivet_err.group(), rivet_err.code())
									})
									.unwrap_or_else(|| "unknown".to_owned());
							}
						}

						// Request connect error, might retry
						tracing::debug!("Request attempt {attempts} failed (service unavailable)");

						// Use backoff and continue
						let backoff =
							utils::calculate_backoff(attempts, req_ctx.retry.initial_interval);
						tokio::time::sleep(backoff).await;

						// Refresh route (ignore cache) so subsequent requests can hit new target
						let ResolveRouteOutput::CustomServe(new_handler) =
							self.state.resolve_route(req_ctx, true).await?
						else {
							bail!("resolved route does not match CustomServe");
						};
						handler = new_handler;

						continue;
					}

					return res;
				}

				// If we get here, all attempts failed
				return Err(errors::RetryAttemptsExceeded {
					attempts: req_ctx.retry.max_attempts,
					last_error_code,
					last_status,
					last_target_kind: "custom_serve".to_owned(),
				}
				.build());
			}
		}
	}

	#[tracing::instrument(skip_all)]
	async fn handle_websocket_upgrade(
		&self,
		req: Request<BodyIncoming>,
		req_ctx: &mut RequestContext,
		target: ResolveRouteOutput,
	) -> Result<Response<ResponseBody>> {
		// Log the headers for debugging
		tracing::debug!("WebSocket upgrade request headers:");
		for (name, value) in &req_ctx.headers {
			if let Ok(val) = value.to_str() {
				tracing::debug!("  {}: {}", name, val);
			}
		}

		// Handle WebSocket upgrade properly with hyper_tungstenite
		tracing::debug!(path=%req_ctx.path, "Upgrading client connection to WebSocket");
		let (client_response, client_ws) = match hyper_tungstenite::upgrade(
			req,
			Some(websocket_config(self.state.config.guard())),
		) {
			Ok(x) => {
				tracing::debug!("Client WebSocket upgrade successful");
				x
			}
			Err(err) => {
				tracing::error!(?err, "Failed to upgrade client WebSocket");
				return Err(errors::ConnectionError {
					error_message: format!("Failed to upgrade client WebSocket: {}", err),
					remote_addr: req_ctx.remote_addr.to_string(),
				}
				.build());
			}
		};

		// Log response status and headers
		tracing::debug!(
			"Client upgrade response status: {}",
			client_response.status()
		);
		for (name, value) in client_response.headers() {
			if let Ok(val) = value.to_str() {
				tracing::debug!("Client upgrade response header - {}: {}", name, val);
			}
		}

		// Clone needed values for the spawned task
		let state = self.state.clone();

		// Held by whichever task takes over the connection below, so the gauge follows the lifetime
		// of the client WebSocket instead of any single early return inside those tasks.
		let active_guard = metrics::WEBSOCKET_ACTIVE.inc_guard();

		// Spawn a new task to handle the WebSocket bidirectional communication
		match target {
			ResolveRouteOutput::Target(mut target) => {
				tracing::debug!("Spawning task to handle WebSocket communication");
				let mut req_ctx = req_ctx.clone();

				self.state.tasks.spawn(
					async move {
						let _active_guard = active_guard;
						let req_ctx = &mut req_ctx;

						let timeout_duration = state.config.guard().websocket_setup_timeout();
						tracing::debug!(
							"WebSocket proxy task started with {}s timeout",
							timeout_duration.as_secs()
						);

						// Use retry logic to connect to the upstream WebSocket server
						let mut attempts = 0;
						let mut upstream_ws = None;

						// First, wait for the client WebSocket to be ready (do this first to avoid race conditions)
						tracing::debug!("Waiting for client WebSocket to be ready...");
						let mut client_ws = match tokio::time::timeout(timeout_duration, client_ws)
							.instrument(tracing::debug_span!("connect_client_ws"))
							.await
						{
							Ok(Ok(ws)) => {
								tracing::debug!("Client WebSocket is ready");
								ws
							}
							Ok(Err(err)) => {
								tracing::error!(?err, "Failed to get client WebSocket");
								return;
							}
							Err(_) => {
								tracing::error!(
									"Timeout waiting for client WebSocket to be ready after {}s",
									timeout_duration.as_secs()
								);
								return;
							}
						};

						// Now attempt to connect to the upstream server
						tracing::debug!("Attempting connect to upstream WebSocket");
						let mut last_error_code;
						while attempts < req_ctx.retry.max_attempts {
							attempts += 1;

							// Build the WebSocket URL using the url crate to properly handle IPv6 addresses
							let mut ws_url = match Url::parse("ws://example.com") {
								Ok(url) => url,
								Err(err) => {
									tracing::error!(?err, "Failed to create base WebSocket URL");
									return;
								}
							};

							// Wrap IPv6 addresses in brackets if not already wrapped
							let host = if target.host.contains(':') && !target.host.starts_with('[')
							{
								format!("[{}]", target.host)
							} else {
								target.host.clone()
							};

							if let Err(err) = ws_url.set_host(Some(&host)) {
								tracing::error!(?err, ?host, "Failed to set WebSocket host");
								return;
							}
							if let Err(err) = ws_url.set_port(Some(target.port)) {
								tracing::error!(?err, "Failed to set WebSocket port");
								return;
							}

							// Split path and query string
							if let Some(query_pos) = target.path.find('?') {
								let (path, query) = target.path.split_at(query_pos);
								ws_url.set_path(path);
								// Remove the leading '?' from query
								ws_url.set_query(Some(&query[1..]));
							} else {
								ws_url.set_path(&target.path);
								ws_url.set_query(None);
							}

							let target_url = ws_url.to_string();

							tracing::debug!(
								"WebSocket request attempt {}/{} to {}",
								attempts,
								req_ctx.retry.max_attempts,
								target_url
							);

							// Build the websocket request with headers
							let mut ws_request = match target_url.into_client_request() {
								Ok(req) => req,
								Err(err) => {
									tracing::error!(?err, "Failed to create websocket request");
									return;
								}
							};

							// Add proxy headers to the websocket request
							if let Err(err) = utils::add_proxy_headers_with_addr(
								ws_request.headers_mut(),
								req_ctx,
							) {
								tracing::error!(
									?err,
									"Failed to add proxy headers to websocket request"
								);
								return;
							}

							match tokio::time::timeout(
								state.config.guard().websocket_connect_attempt_timeout(),
								tokio_tungstenite::connect_async_with_config(
									ws_request,
									Some(websocket_config(state.config.guard())),
									false,
								),
							)
							.instrument(tracing::debug_span!("connect_upstream_ws"))
							.await
							{
								Ok(Ok((ws_stream, resp))) => {
									tracing::debug!(
										"Successfully connected to upstream WebSocket server"
									);
									tracing::debug!(
										"Upstream connection response status: {:?}",
										resp.status()
									);

									// Log headers for debugging
									for (name, value) in resp.headers() {
										if let Ok(val) = value.to_str() {
											tracing::debug!(
												"Upstream response header - {}: {}",
												name,
												val
											);
										}
									}

									upstream_ws = Some(ws_stream);
									break;
								}
								Ok(Err(err)) => {
									last_error_code = Some(err.to_string());
									tracing::debug!(
										?err,
										"WebSocket request attempt {} failed",
										attempts
									);
								}
								Err(_) => {
									last_error_code = Some("websocket_connect_timeout".to_owned());
									tracing::debug!(
										attempts,
										timeout_ms = state
											.config
											.guard()
											.websocket_connect_attempt_timeout()
											.as_millis() as u64,
										"WebSocket request attempt timed out"
									);
								}
							}

							// Check if we've reached max attempts
							if attempts >= req_ctx.retry.max_attempts {
								tracing::debug!(
									"All {} WebSocket connection attempts failed",
									req_ctx.retry.max_attempts
								);

								// Send a close message to the client since we can't connect to upstream
								let err = errors::RetryAttemptsExceeded {
									attempts,
									last_error_code: last_error_code.unwrap_or_else(|| {
										"websocket_upstream_connect_failed".to_owned()
									}),
									last_status: "none".to_owned(),
									last_target_kind: "target".to_owned(),
								}
								.build();
								tracing::warn!(
									?err,
									"sending close message to client due to upstream connection failure"
								);
								let (mut client_sink, _) = client_ws.split();
								match client_sink
									.send(utils::to_hyper_close(Some(utils::err_to_close_frame(
										err,
										req_ctx.external_ray_id(),
									))))
									.await
								{
									Ok(_) => {
										tracing::trace!("Successfully sent close message to client")
									}
									Err(err) => {
										tracing::error!(
											?err,
											"Failed to send close message to client"
										)
									}
								};

								match client_sink.flush().await {
									Ok(_) => {
										tracing::trace!(
											"Successfully flushed client sink after close"
										)
									}
									Err(err) => {
										tracing::error!(
											?err,
											"Failed to flush client sink after close"
										)
									}
								};

								return;
							}

							// Use backoff for the next attempt
							let backoff =
								utils::calculate_backoff(attempts, req_ctx.retry.initial_interval);
							tracing::debug!(
								"Waiting for {:?} before next connection attempt",
								backoff
							);

							tokio::time::sleep(backoff)
								.instrument(tracing::debug_span!("backoff_sleep"))
								.await;

							// Resolve target again, this time ignoring cache. This makes sure
							// we always re-fetch the route on error
							let new_target = state.resolve_route(req_ctx, true).await;

							match new_target {
								Ok(ResolveRouteOutput::Target(new_target)) => {
									target = new_target;
								}
								Ok(ResolveRouteOutput::CustomServe(_)) => {
									let err = errors::WebSocketTargetChanged {
										phase: "upstream_websocket_retry".to_owned(),
										from_target_kind: "target".to_owned(),
										to_target_kind: "custom_serve".to_owned(),
									}
									.build();
									tracing::warn!(
										?err,
										"websocket target changed to custom serve"
									);
									let _ = client_ws
										.close(Some(utils::err_to_close_frame(
											err,
											req_ctx.external_ray_id(),
										)))
										.await;
									return;
								}
								Err(err) => {
									tracing::error!(?err, "Routing error");
								}
							}
						}

						// If we couldn't connect to the upstream server, exit the task
						let upstream_ws = match upstream_ws {
							Some(ws) => {
								tracing::debug!(
									"Successfully established upstream WebSocket connection"
								);
								ws
							}
							Option::None => {
								tracing::error!(
									"Failed to establish upstream WebSocket connection (unexpected)"
								);
								return; // Should never happen due to checks above, but just in case
							}
						};

						// Now set up bidirectional communication between the client and upstream WebSockets
						tracing::debug!("Setting up bidirectional WebSocket proxying");
						let (client_sink, client_stream) = client_ws.split();
						let (upstream_sink, upstream_stream) = upstream_ws.split();

						// Create channels for coordinating shutdown between client and upstream
						let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

						// Manually forward messages from client to upstream server with shutdown coordination
						let client_to_upstream = async {
							tracing::debug!("Starting client-to-upstream forwarder");
							let mut stream = client_stream;
							let mut sink = upstream_sink;
							let mut shutdown_rx = shutdown_rx.clone();

							loop {
								tokio::select! {
									// Check for shutdown signal
									shutdown_result = shutdown_rx.changed() => {
										match shutdown_result {
											Ok(_) => {
												if *shutdown_rx.borrow() {
													tracing::debug!("Client-to-upstream forwarder shutting down due to signal");
													break;
												}
											},
											Err(err) => {
												// Channel closed
												tracing::debug!(?err, "Client-to-upstream shutdown channel closed");
												break;
											}
										}
									}

									// Process next message from client
									msg_result = stream.next() => {
										match msg_result {
											Some(Ok(client_msg)) => {
												// Convert from hyper_tungstenite::Message to tokio_tungstenite::Message
												let upstream_msg = match client_msg {
													hyper_tungstenite::tungstenite::Message::Text(text) => {
														tokio_tungstenite::tungstenite::Message::Text(text)
													},
													hyper_tungstenite::tungstenite::Message::Binary(data) => {
														tokio_tungstenite::tungstenite::Message::Binary(data)
													},
													hyper_tungstenite::tungstenite::Message::Ping(data) => {
														tokio_tungstenite::tungstenite::Message::Ping(data)
													},
													hyper_tungstenite::tungstenite::Message::Pong(data) => {
														tokio_tungstenite::tungstenite::Message::Pong(data)
													},
													hyper_tungstenite::tungstenite::Message::Close(frame) => {
														// Signal shutdown to other direction
														let _ = shutdown_tx.send(true);

														utils::to_hyper_close(frame)
													},
													hyper_tungstenite::tungstenite::Message::Frame(_) => {
														// Skip frames - they're an implementation detail
														continue;
													},
												};

												// Send the message with a timeout
												tracing::trace!("Sending message to upstream server");
												let send_result = tokio::time::timeout(
													state.config.guard().websocket_send_timeout(),
													sink.send(upstream_msg)
												).await;

												match send_result {
													Ok(Ok(_)) => {
														tracing::trace!("Message sent to upstream successfully");
														// Flush the sink with a timeout
														tracing::trace!("Flushing upstream sink");
														let flush_result = tokio::time::timeout(
															state.config.guard().websocket_flush_timeout(),
															sink.flush()
														).await;

														if let Err(_) = flush_result {
															tracing::trace!("Timeout flushing upstream sink");
															let _ = shutdown_tx.send(true);
															break;
														} else if let Ok(Err(err)) = flush_result {
															tracing::trace!(?err, "Error flushing upstream sink");
															let _ = shutdown_tx.send(true);
															break;
														} else {
															tracing::trace!("Upstream sink flushed successfully");
														}
													},
													Ok(Err(err)) => {
														tracing::trace!(?err, "Error sending message to upstream");
														let _ = shutdown_tx.send(true);
														break;
													},
													Err(_) => {
														tracing::trace!("Timeout sending message to upstream");
														let _ = shutdown_tx.send(true);
														break;
													},
												}
											},
											Some(Err(err)) => {
												// Error receiving message from client
												tracing::trace!(?err, "Error receiving message from client");
												tracing::trace!(?err, "Error details");
												// Signal shutdown to other direction
												let _ = shutdown_tx.send(true);
												break;
											},
											None => {
												// End of stream
												tracing::trace!("Client WebSocket stream ended");
												// Signal shutdown to other direction
												let _ = shutdown_tx.send(true);
												break;
											}
										}
									}
								}
							}

							// Try to send a close frame - ignore errors as the connection might already be closed
							tracing::trace!("Attempting to send close message to upstream");
							match sink
								.send(tokio_tungstenite::tungstenite::Message::Close(None))
								.await
							{
								Ok(_) => {
									tracing::trace!("Close message sent to upstream successfully")
								}
								Err(err) => {
									tracing::trace!(
										?err,
										"Failed to send close message to upstream"
									)
								}
							};

							match sink.flush().await {
								Ok(_) => {
									tracing::trace!(
										"Upstream sink flushed successfully after close"
									)
								}
								Err(err) => {
									tracing::trace!(
										?err,
										"Failed to flush upstream sink after close"
									)
								}
							};

							tracing::debug!("Client-to-upstream task completed");
						};

						// Manually forward messages from upstream server to client with shutdown coordination
						let upstream_to_client = async {
							tracing::debug!("Starting upstream-to-client forwarder");
							let mut stream = upstream_stream;
							let mut sink = client_sink;
							let mut shutdown_rx = shutdown_rx.clone();

							loop {
								tokio::select! {
									// Check for shutdown signal
									shutdown_result = shutdown_rx.changed() => {
										match shutdown_result {
											Ok(_) => {
												if *shutdown_rx.borrow() {
													tracing::debug!("Upstream-to-client forwarder shutting down due to signal");
													break;
												}
											},
											Err(err) => {
												// Channel closed
												tracing::debug!(?err, "Upstream-to-client shutdown channel closed");
												break;
											}
										}
									}

									// Process next message from upstream
									msg_result = stream.next() => {
										match msg_result {
											Some(Ok(upstream_msg)) => {
												// Convert from tokio_tungstenite::Message to hyper_tungstenite::Message
												let client_msg = match upstream_msg {
													tokio_tungstenite::tungstenite::Message::Text(text) => {
														hyper_tungstenite::tungstenite::Message::Text(text)
													},
													tokio_tungstenite::tungstenite::Message::Binary(data) => {
														hyper_tungstenite::tungstenite::Message::Binary(data)
													},
													tokio_tungstenite::tungstenite::Message::Ping(data) => {
														hyper_tungstenite::tungstenite::Message::Ping(data)
													},
													tokio_tungstenite::tungstenite::Message::Pong(data) => {
														hyper_tungstenite::tungstenite::Message::Pong(data)
													},
													tokio_tungstenite::tungstenite::Message::Close(frame) => {
														// Signal shutdown to other direction
														let _ = shutdown_tx.send(true);

														utils::to_hyper_close(frame)
													},
													tokio_tungstenite::tungstenite::Message::Frame(_) => {
														// Skip frames - they're an implementation detail
														continue;
													},
												};

												// Send the message with a timeout
												tracing::trace!("Sending message to client");
												let send_result = tokio::time::timeout(
													state.config.guard().websocket_send_timeout(),
													sink.send(client_msg)
												).await;

												match send_result {
													Ok(Ok(_)) => {
														tracing::trace!("Message sent to client successfully");
														// Flush the sink with a timeout
														tracing::trace!("Flushing client sink");
														let flush_result = tokio::time::timeout(
															state.config.guard().websocket_flush_timeout(),
															sink.flush()
														).await;

														if let Err(_) = flush_result {
															tracing::trace!("Timeout flushing client sink");
															let _ = shutdown_tx.send(true);
															break;
														} else if let Ok(Err(err)) = flush_result {
															tracing::trace!(?err, "Error flushing client sink");
															let _ = shutdown_tx.send(true);
															break;
														} else {
															tracing::trace!("Client sink flushed successfully");
														}
													},
													Ok(Err(err)) => {
														tracing::trace!(?err, "Error sending message to client");
														let _ = shutdown_tx.send(true);
														break;
													},
													Err(_) => {
														tracing::trace!("Timeout sending message to client");
														let _ = shutdown_tx.send(true);
														break;
													},
												}
											},
											Some(Err(err)) => {
												// Error receiving message from upstream
												tracing::trace!(?err, "Error receiving message from upstream");
												// Signal shutdown to other direction
												let _ = shutdown_tx.send(true);
												break;
											},
											None => {
												// End of stream
												tracing::trace!("Upstream WebSocket stream ended");
												// Signal shutdown to other direction
												let _ = shutdown_tx.send(true);
												break;
											}
										}
									}
								}
							}

							// Try to send a close frame - ignore errors as the connection might already be closed
							tracing::trace!("Attempting to send close message to client");
							match sink.send(utils::to_hyper_close(None)).await {
								Ok(_) => {
									tracing::trace!("Close message sent to client successfully")
								}
								Err(err) => {
									tracing::trace!(?err, "Failed to send close message to client")
								}
							};

							match sink.flush().await {
								Ok(_) => {
									tracing::trace!("Client sink flushed successfully after close")
								}
								Err(err) => {
									tracing::trace!(?err, "Failed to flush client sink after close")
								}
							};

							tracing::trace!("Upstream-to-client task completed");
						};

						// Run both directions concurrently until either one completes or errors
						tracing::debug!("Starting bidirectional message forwarding");
						tokio::join!(client_to_upstream, upstream_to_client);
						tracing::debug!("Bidirectional message forwarding completed");
					}
					.instrument(tracing::debug_span!("handle_ws_task_target")),
				);
			}
			ResolveRouteOutput::CustomServe(mut handler) => {
				tracing::debug!(path=%req_ctx.path, "Spawning task to handle WebSocket communication");
				let state = self.state.clone();
				let mut req_ctx = req_ctx.clone();

				self.state.tasks.spawn(
					async move {
						let _active_guard = active_guard;
						let req_ctx = &mut req_ctx;
						let mut ws_hibernation_close = false;
						let mut after_hibernation = false;
						let mut attempts = 0u32;

						let ws_handle = WebSocketHandle::new(client_ws)
							.await
							.context("failed initiating websocket handle")?;

						loop {
							match handler
								.handle_websocket(req_ctx, ws_handle.clone(), after_hibernation)
								.await
							{
								Ok(close_frame) => {
									tracing::debug!("websocket handler complete, closing");

									// Send graceful close. This may fail if client already sent
									// close frame, which is normal.
									tracing::debug!(?close_frame, "sending close frame to client");
									match ws_handle.send(utils::to_hyper_close(close_frame)).await {
										Ok(_) => {
											tracing::debug!("close frame sent successfully");
										}
										Err(err) => {
											tracing::debug!(
												?err,
												"failed to send close frame (websocket may be already closing)"
											);
										}
									}

									// Flush to ensure close frame is sent
									tracing::debug!("flushing websocket");
									match ws_handle.flush().await {
										Ok(_) => {
											tracing::debug!("websocket flushed successfully");
										}
										Err(err) => {
											tracing::debug!(
												?err,
												"failed to flush websocket (websocket may be already closing)"
											);
										}
									}

									// Keep TCP connection open briefly to allow client to process close
									tokio::time::sleep(WEBSOCKET_CLOSE_LINGER).await;

									break;
								}
								Err(err) => {
									tracing::debug!(?err, "websocket handler error");

									// Denotes that the connection did not fail, but the downstream has closed
									let ws_hibernate = utils::is_ws_hibernate(&err);

									if ws_hibernate {
										attempts = 0;
									} else {
										attempts += 1;
									}

									if ws_hibernate {
										// This should be unreachable because as soon as the actor is
										// reconnected to after hibernation the gateway will consume the close
										// frame from the client ws stream
										ensure!(
											!ws_hibernation_close,
											"should not be hibernating again after receiving a close frame during hibernation"
										);

										// After this function returns:
										// - the route will be resolved again
										// - the websocket will connect to the new downstream target
										// - the gateway will continue reading messages from the client ws
										//   (starting with the message that caused the hibernation to end)
										let res = handler
											.handle_websocket_hibernation(
												req_ctx,
												ws_handle.clone(),
											)
											.await?;

										after_hibernation = true;

										// Despite receiving a close frame from the client during hibernation
										// we are going to reconnect to the actor so that it knows the
										// connection has closed
										if let HibernationResult::Close = res {
											tracing::debug!("starting hibernating websocket close");

											ws_hibernation_close = true;
										}
									} else if attempts > req_ctx.retry.max_attempts
										|| !utils::is_retryable_ws_error(&err)
									{
										tracing::debug!(
											?err,
											?attempts,
											max_attempts=?req_ctx.retry.max_attempts,
											"websocket failed"
										);

										// Close WebSocket with error
										ws_handle
											.send(utils::to_hyper_close(Some(
												utils::err_to_close_frame(
													err,
													req_ctx.external_ray_id(),
												),
											)))
											.await?;

										// Flush to ensure close frame is sent
										ws_handle.flush().await?;

										// Keep TCP connection open briefly to allow client to process close
										tokio::time::sleep(WEBSOCKET_CLOSE_LINGER).await;

										break;
									} else {
										let backoff = utils::calculate_backoff(
											attempts,
											req_ctx.retry.initial_interval,
										);

										tracing::debug!(
											?backoff,
											"WebSocket attempt {attempts} failed (service unavailable)"
										);

										// Apply backoff for retryable error
										tokio::time::sleep(backoff).await;
									}

									// Retry route resolution
									match state.resolve_route(req_ctx, true).await {
										Ok(ResolveRouteOutput::CustomServe(new_handler)) => {
											handler = new_handler;
											continue;
										}
										Ok(ResolveRouteOutput::Target(_)) => {
											let err = errors::WebSocketTargetChanged {
												phase: "custom_serve_websocket_retry".to_owned(),
												from_target_kind: "custom_serve".to_owned(),
												to_target_kind: "target".to_owned(),
											}
											.build();
											tracing::warn!(
												?err,
												"websocket target changed to target"
											);
											ws_handle
												.send(utils::to_hyper_close(Some(
													utils::err_to_close_frame(
														err,
														req_ctx.external_ray_id(),
													),
												)))
												.await?;

											// Flush to ensure close frame is sent
											ws_handle.flush().await?;

											// Keep TCP connection open briefly to allow client to process close
											tokio::time::sleep(WEBSOCKET_CLOSE_LINGER).await;

											break;
										}
										Err(err) => {
											tracing::warn!(
												?err,
												"closing websocket due to route resolution error"
											);
											ws_handle
												.send(utils::to_hyper_close(Some(
													utils::err_to_close_frame(
														err,
														req_ctx.external_ray_id(),
													),
												)))
												.await?;

											// Flush to ensure close frame is sent
											ws_handle.flush().await?;

											// Keep TCP connection open briefly to allow client to process close
											tokio::time::sleep(WEBSOCKET_CLOSE_LINGER).await;

											break;
										}
									}
								}
							}
						}

						Ok(())
					}
					.instrument(tracing::debug_span!("handle_ws_task_custom_serve")),
				);
			}
		}

		// Return the response that will upgrade the client connection
		// For proper WebSocket handshaking, we need to preserve the original response
		// structure but convert it to our expected return type without modifying its content
		tracing::debug!("Returning WebSocket upgrade response to client");
		// Extract the parts from the response but preserve all headers and status
		let (mut parts, _) = client_response.into_parts();

		// Add Sec-WebSocket-Protocol header to the response
		// Many WebSocket clients (e.g. node-ws & Cloudflare) require a protocol in the response
		parts.headers.insert(
			"sec-websocket-protocol",
			hyper::header::HeaderValue::from_static("rivet"),
		);

		// Create a new response with an empty body - WebSocket upgrades don't need a body
		Ok(Response::from_parts(
			parts,
			ResponseBody::Full(Full::<Bytes>::new(Bytes::new())),
		))
	}
}

impl Clone for ProxyService {
	fn clone(&self) -> Self {
		Self {
			state: self.state.clone(),
			remote_addr: self.remote_addr,
			connection_start: self.connection_start,
			client_disconnect: self.client_disconnect.clone(),
		}
	}
}

// Factory for creating proxy services
pub struct ProxyServiceFactory {
	state: Arc<ProxyState>,
}

impl ProxyServiceFactory {
	pub fn new(
		config: rivet_config::Config,
		routing_fn: RoutingFn,
		cache_key_fn: CacheKeyFn,
	) -> Self {
		let state = Arc::new(ProxyState::new(config, routing_fn, cache_key_fn));
		Self { state }
	}

	// Create a new proxy service for the given remote address
	pub fn create_service(&self, remote_addr: SocketAddr) -> ProxyService {
		ProxyService::new(self.state.clone(), remote_addr)
	}

	pub async fn wait_idle(&self) {
		self.state.tasks.wait_idle().await
	}

	pub fn remaining_tasks(&self) -> usize {
		self.state.tasks.remaining_tasks()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rivet_util::Id;

	fn test_state(guard: rivet_config::config::guard::Guard) -> ProxyState {
		let config = rivet_config::Config::from_root(rivet_config::config::Root {
			guard: Some(guard),
			..Default::default()
		});
		let routing_fn: RoutingFn =
			Arc::new(|_| Box::pin(async { anyhow::bail!("routing is not used by this test") }));
		let cache_key_fn: CacheKeyFn = Arc::new(|_| Ok(0));

		ProxyState::new(config, routing_fn, cache_key_fn)
	}

	fn test_request_context(state: &ProxyState) -> RequestContext {
		RequestContext::new(
			"127.0.0.1:12345".parse().unwrap(),
			Id::v1(uuid::Uuid::nil(), 0),
			"test-ray".to_owned(),
			Id::v1(uuid::Uuid::nil(), 1),
			"example.com".to_owned(),
			"/actors".to_owned(),
			hyper::Method::GET,
			hyper::HeaderMap::new(),
			false,
			"127.0.0.1".parse().unwrap(),
			Instant::now(),
			CancellationToken::new(),
			state.config.guard(),
		)
	}

	#[tokio::test]
	async fn default_admission_is_unlimited_and_still_allocates_request_ids() {
		let state = test_state(Default::default());
		assert!(state.client_states.is_none());

		let mut contexts = Vec::with_capacity(10_001);
		for _ in 0..10_001 {
			let mut req_ctx = test_request_context(&state);
			assert_eq!(state.admit_request(&mut req_ctx).await.unwrap(), None);
			assert!(req_ctx.in_flight_request_id().is_ok());
			contexts.push(req_ctx);
		}
	}

	#[tokio::test]
	async fn configured_admission_limits_are_enforced_independently() {
		let state = test_state(rivet_config::config::guard::Guard {
			rate_limit: Some(rivet_config::config::guard::GuardRateLimit {
				requests: 1,
				period_ms: 60_000,
			}),
			..Default::default()
		});
		let mut first = test_request_context(&state);
		let mut second = test_request_context(&state);
		assert_eq!(state.admit_request(&mut first).await.unwrap(), None);
		assert_eq!(
			state.admit_request(&mut second).await.unwrap(),
			Some(AdmissionRejection::RateLimit)
		);

		let state = test_state(rivet_config::config::guard::Guard {
			max_in_flight: Some(1),
			..Default::default()
		});
		let mut first = test_request_context(&state);
		let mut second = test_request_context(&state);
		assert_eq!(state.admit_request(&mut first).await.unwrap(), None);
		assert_eq!(
			state.admit_request(&mut second).await.unwrap(),
			Some(AdmissionRejection::MaxInFlight)
		);
	}

	#[test]
	fn websocket_config_uses_documented_limit() {
		let guard_config = rivet_config::config::guard::Guard::default();
		let config = websocket_config(&guard_config);

		assert_eq!(
			config.max_message_size,
			Some(rivet_config::config::guard::DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE)
		);
		assert_eq!(
			config.max_frame_size,
			Some(rivet_config::config::guard::DEFAULT_WEBSOCKET_MAX_FRAME_SIZE)
		);
	}

	#[test]
	fn websocket_config_uses_guard_overrides() {
		let guard_config = rivet_config::config::guard::Guard {
			websocket_max_message_size: Some(8 * 1024 * 1024),
			websocket_max_frame_size: Some(4 * 1024 * 1024),
			..Default::default()
		};
		let config = websocket_config(&guard_config);

		assert_eq!(config.max_message_size, Some(8 * 1024 * 1024));
		assert_eq!(config.max_frame_size, Some(4 * 1024 * 1024));
	}
}
