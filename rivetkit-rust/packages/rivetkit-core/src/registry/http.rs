use super::dispatch::*;
use super::inspector::*;
use super::*;
use crate::error::{ProtocolError, client_error_message, client_error_metadata};
use crate::serde_metrics;
use ::http;

const HEADER_RIVET_ACTOR: &str = "x-rivet-actor";
const HEADER_RIVET_ACTOR_GENERATION: &str = "x-rivet-actor-generation";
const HEADER_RIVET_ACTOR_KEY: &str = "x-rivet-actor-key";

impl RegistryDispatcher {
	pub(super) async fn handle_fetch(
		&self,
		actor_id: &str,
		request: HttpRequest,
	) -> Result<HttpResponse> {
		let original_path = request.path.clone();
		let request = build_http_request(request).await?;
		let route = RegistryHttpRoute::from_paths(
			&original_path,
			request.uri().path(),
			self.handle_inspector_http_in_runtime,
		)?;
		if matches!(
			route,
			RegistryHttpRoute::Framework(FrameworkHttpRoute::Metrics)
		) {
			return handle_metrics_fetch(&request);
		}
		let instance = match self.active_actor(actor_id).await {
			Ok(instance) => instance,
			Err(error) => {
				if matches!(route, RegistryHttpRoute::Framework(_)) {
					return message_boundary_error_response(
						request_encoding(request.headers()),
						framework_anyhow_status(&error),
						error,
					);
				}
				return Ok(inspector_anyhow_response(error));
			}
		};
		let actor = actor_specifier_for_instance(&instance);
		if let Some(mut response) = self.handle_inspector_fetch(&instance, &request).await? {
			attach_actor_response_headers(&mut response, &actor);
			return Ok(response);
		}

		instance.ctx.cancel_sleep_timer();

		let response = match route {
			RegistryHttpRoute::Framework(route) => {
				let response = self.handle_framework_fetch(&instance, request, route).await;
				rearm_sleep_after_request(instance.ctx.clone());
				response
			}
			RegistryHttpRoute::UserRawRequest => {
				self.handle_user_request_fetch(&instance, request).await
			}
		};
		response.map(|mut response| {
			attach_actor_response_headers(&mut response, &actor);
			response
		})
	}

	async fn handle_user_request_fetch(
		&self,
		instance: &ActorTaskHandle,
		request: Request,
	) -> Result<HttpResponse> {
		let encoding = request_encoding(request.headers());
		let (reply_tx, reply_rx) = oneshot::channel();
		try_send_dispatch_command(
			&instance.dispatch,
			DispatchCommand::Http {
				request,
				reply: reply_tx,
			},
		)
		.context("send actor task HTTP dispatch command")?;

		match reply_rx
			.await
			.context("receive actor task HTTP dispatch reply")?
		{
			Ok(response) => {
				rearm_sleep_after_request(instance.ctx.clone());
				build_envoy_response(response)
			}
			Err(error) => {
				tracing::error!(
					actor_id = instance.actor_id,
					?error,
					"actor request callback failed"
				);
				rearm_sleep_after_request(instance.ctx.clone());
				framework_anyhow_error_response_with_actor(
					encoding,
					error,
					Some(&actor_specifier_for_instance(instance)),
				)
			}
		}
	}

	async fn handle_framework_fetch(
		&self,
		instance: &ActorTaskHandle,
		request: Request,
		route: FrameworkHttpRoute,
	) -> Result<HttpResponse> {
		let actor = actor_specifier_for_instance(instance);
		match route {
			FrameworkHttpRoute::Action(name) => {
				self.handle_action_fetch(instance, request, name).await
			}
			FrameworkHttpRoute::Queue(name) => {
				self.handle_queue_fetch(instance, request, name).await
			}
			FrameworkHttpRoute::Metadata => handle_metadata_fetch(&request, Some(&actor)),
			FrameworkHttpRoute::Health => handle_health_fetch(&request, Some(&actor)),
			FrameworkHttpRoute::Metrics => handle_metrics_fetch(&request),
			FrameworkHttpRoute::Root => handle_root_fetch(&request, Some(&actor)),
			FrameworkHttpRoute::NotFound => handle_not_found_fetch(&request, Some(&actor)),
		}
	}

	async fn handle_action_fetch(
		&self,
		instance: &ActorTaskHandle,
		request: Request,
		action_name: String,
	) -> Result<HttpResponse> {
		let encoding = request_encoding(request.headers());
		let actor = actor_specifier_for_instance(instance);
		if request.method() != http::Method::POST {
			return message_boundary_error_response_with_actor(
				encoding,
				framework_error_status("actor", "method_not_allowed"),
				MethodNotAllowed {
					method: request.method().to_string(),
					path: request.uri().path().to_owned(),
				}
				.build(),
				Some(&actor),
			);
		}

		let config = instance.factory.config();
		if request.body().len() > config.max_incoming_message_size as usize {
			return message_boundary_error_response_with_actor(
				encoding,
				StatusCode::BAD_REQUEST,
				IncomingMessageTooLong.build(),
				Some(&actor),
			);
		}

		let args = match decode_http_action_args(encoding, request.body()) {
			Ok(args) => args,
			Err(error) => {
				return message_boundary_error_response_with_actor(
					encoding,
					StatusCode::BAD_REQUEST,
					error.context("decode HTTP action request"),
					Some(&actor),
				);
			}
		};
		let conn_params = match http_conn_params(request.headers()) {
			Ok(params) => params,
			Err(error) => {
				return message_boundary_error_response_with_actor(
					encoding,
					StatusCode::BAD_REQUEST,
					error.context("decode HTTP action connection params"),
					Some(&actor),
				);
			}
		};
		let conn = match instance
			.ctx
			.connect_conn_with_request(conn_params, Some(request.clone()), async {
				Ok::<Vec<u8>, anyhow::Error>(Vec::new())
			})
			.await
		{
			Ok(conn) => conn,
			Err(error) => {
				return message_boundary_error_response_with_actor(
					encoding,
					framework_anyhow_status(&error),
					error.context("connect HTTP action request"),
					Some(&actor),
				);
			}
		};

		let dispatch_result = with_action_dispatch_timeout(
			config.action_timeout,
			dispatch_action_through_task(
				&instance.dispatch,
				conn.clone(),
				action_name.clone(),
				args,
			),
		)
		.await;
		let disconnect_result = conn.disconnect(None).await;

		match dispatch_result {
			Ok(output) => {
				if let Err(error) = disconnect_result {
					tracing::warn!(
						actor_id = instance.actor_id,
						conn_id = conn.id(),
						?error,
						"failed to disconnect HTTP action connection"
					);
				}
				let response = encode_http_action_response(encoding, output)?;
				if response.body.as_ref().map(Vec::len).unwrap_or_default()
					> config.max_outgoing_message_size as usize
				{
					return message_boundary_error_response_with_actor(
						encoding,
						StatusCode::BAD_REQUEST,
						OutgoingMessageTooLong.build(),
						Some(&actor),
					);
				}
				Ok(response)
			}
			Err(error) => {
				if let Err(disconnect_error) = disconnect_result {
					tracing::warn!(
						actor_id = instance.actor_id,
						conn_id = conn.id(),
						?disconnect_error,
						"failed to disconnect HTTP action connection after error"
					);
				}
				framework_action_error_response(encoding, error, Some(&actor))
			}
		}
	}

	async fn handle_queue_fetch(
		&self,
		instance: &ActorTaskHandle,
		request: Request,
		queue_name: String,
	) -> Result<HttpResponse> {
		let encoding = request_encoding(request.headers());
		let actor = actor_specifier_for_instance(instance);
		if request.method() != http::Method::POST {
			return message_boundary_error_response_with_actor(
				encoding,
				framework_error_status("actor", "method_not_allowed"),
				MethodNotAllowed {
					method: request.method().to_string(),
					path: request.uri().path().to_owned(),
				}
				.build(),
				Some(&actor),
			);
		}

		let config = instance.factory.config();
		if request.body().len() > config.max_incoming_message_size as usize {
			return message_boundary_error_response_with_actor(
				encoding,
				StatusCode::BAD_REQUEST,
				IncomingMessageTooLong.build(),
				Some(&actor),
			);
		}

		let queue_request = match decode_http_queue_request(encoding, request.body()) {
			Ok(queue_request) => queue_request,
			Err(error) => {
				return message_boundary_error_response_with_actor(
					encoding,
					StatusCode::BAD_REQUEST,
					error.context("decode HTTP queue request"),
					Some(&actor),
				);
			}
		};
		let conn_params = match http_conn_params(request.headers()) {
			Ok(params) => params,
			Err(error) => {
				return message_boundary_error_response_with_actor(
					encoding,
					StatusCode::BAD_REQUEST,
					error.context("decode HTTP queue connection params"),
					Some(&actor),
				);
			}
		};
		let conn = match instance
			.ctx
			.connect_conn_with_request(conn_params, Some(request.clone()), async {
				Ok::<Vec<u8>, anyhow::Error>(Vec::new())
			})
			.await
		{
			Ok(conn) => conn,
			Err(error) => {
				return message_boundary_error_response_with_actor(
					encoding,
					framework_anyhow_status(&error),
					error.context("connect HTTP queue request"),
					Some(&actor),
				);
			}
		};

		let (reply_tx, reply_rx) = oneshot::channel();
		let dispatch_result = try_send_dispatch_command(
			&instance.dispatch,
			DispatchCommand::QueueSend {
				name: queue_name,
				body: queue_request.body,
				conn: conn.clone(),
				request,
				wait: queue_request.wait,
				timeout_ms: queue_request.timeout,
				reply: reply_tx,
			},
		);

		let queue_result = match dispatch_result {
			Ok(()) => {
				with_framework_action_timeout(config.action_timeout, async {
					reply_rx
						.await
						.context("receive actor task queue send reply")?
				})
				.await
			}
			Err(error) => Err(error),
		};
		let disconnect_result = conn.disconnect(None).await;

		match queue_result {
			Ok(result) => {
				if let Err(error) = disconnect_result {
					tracing::warn!(
						actor_id = instance.actor_id,
						conn_id = conn.id(),
						?error,
						"failed to disconnect HTTP queue connection"
					);
				}
				let response = encode_http_queue_response(encoding, result)?;
				if response.body.as_ref().map(Vec::len).unwrap_or_default()
					> config.max_outgoing_message_size as usize
				{
					return message_boundary_error_response_with_actor(
						encoding,
						StatusCode::BAD_REQUEST,
						OutgoingMessageTooLong.build(),
						Some(&actor),
					);
				}
				Ok(response)
			}
			Err(error) => {
				if let Err(disconnect_error) = disconnect_result {
					tracing::warn!(
						actor_id = instance.actor_id,
						conn_id = conn.id(),
						?disconnect_error,
						"failed to disconnect HTTP queue connection after error"
					);
				}
				message_boundary_error_response_with_actor(
					encoding,
					framework_anyhow_status(&error),
					error,
					Some(&actor),
				)
			}
		}
	}
}

enum RegistryHttpRoute {
	Framework(FrameworkHttpRoute),
	UserRawRequest,
}

impl RegistryHttpRoute {
	fn from_paths(
		original_path: &str,
		normalized_path: &str,
		handle_inspector_http_in_runtime: bool,
	) -> Result<Self> {
		if let Some(stripped) = original_path.strip_prefix("/request") {
			if stripped.is_empty() || matches!(stripped.as_bytes().first(), Some(b'/') | Some(b'?'))
			{
				return Ok(Self::UserRawRequest);
			}
		}
		if handle_inspector_http_in_runtime && normalized_path.starts_with("/inspector/") {
			return Ok(Self::UserRawRequest);
		}

		if let Some(segment) = single_path_segment(normalized_path, "/action/") {
			return Ok(Self::Framework(FrameworkHttpRoute::Action(
				percent_decode_path_segment(segment)?,
			)));
		}
		if let Some(segment) = single_path_segment(normalized_path, "/queue/") {
			return Ok(Self::Framework(FrameworkHttpRoute::Queue(
				percent_decode_path_segment(segment)?,
			)));
		}

		match normalized_path {
			"/metadata" => Ok(Self::Framework(FrameworkHttpRoute::Metadata)),
			"/health" => Ok(Self::Framework(FrameworkHttpRoute::Health)),
			"/metrics" => Ok(Self::Framework(FrameworkHttpRoute::Metrics)),
			"/" => Ok(Self::Framework(FrameworkHttpRoute::Root)),
			_ => Ok(Self::Framework(FrameworkHttpRoute::NotFound)),
		}
	}
}

pub(super) enum FrameworkHttpRoute {
	Action(String),
	Queue(String),
	Metadata,
	Health,
	Metrics,
	Root,
	NotFound,
}

pub(super) struct DecodedHttpQueueRequest {
	body: Vec<u8>,
	wait: bool,
	timeout: Option<u64>,
}

fn handle_metadata_fetch(
	request: &Request,
	actor: Option<&ActorSpecifier>,
) -> Result<HttpResponse> {
	if request.method() != http::Method::GET {
		return method_not_allowed_response(request, actor);
	}
	json_http_response(
		StatusCode::OK,
		&serde_json::json!({
			"runtime": "rivetkit",
			"version": env!("CARGO_PKG_VERSION"),
			"type": crate::metrics_endpoint::runtime_type(),
		}),
	)
}

fn handle_health_fetch(request: &Request, actor: Option<&ActorSpecifier>) -> Result<HttpResponse> {
	if request.method() != http::Method::GET {
		return method_not_allowed_response(request, actor);
	}
	text_response(StatusCode::OK, "ok")
}

fn handle_metrics_fetch(request: &Request) -> Result<HttpResponse> {
	if request.method() != http::Method::GET {
		return method_not_allowed_response(request, None);
	}

	let metrics = crate::metrics_endpoint::render_prometheus_metrics()?;
	bytes_response(StatusCode::OK, &metrics.content_type, metrics.body)
}

fn handle_root_fetch(request: &Request, actor: Option<&ActorSpecifier>) -> Result<HttpResponse> {
	if request.method() != http::Method::GET {
		return method_not_allowed_response(request, actor);
	}
	text_response(
		StatusCode::OK,
		"This is an RivetKit actor.\n\nLearn more at https://rivetkit.org",
	)
}

fn handle_not_found_fetch(
	request: &Request,
	actor: Option<&ActorSpecifier>,
) -> Result<HttpResponse> {
	let encoding = request_encoding(request.headers());
	message_boundary_error_response_with_actor(
		encoding,
		framework_error_status("actor", "not_found"),
		ActorRuntime::NotFound {
			resource: "route".to_owned(),
			id: request.uri().path().to_owned(),
		}
		.build(),
		actor,
	)
}

fn text_response(status: StatusCode, body: &str) -> Result<HttpResponse> {
	bytes_response(
		status,
		"text/plain; charset=utf-8",
		body.as_bytes().to_vec(),
	)
}

fn bytes_response(status: StatusCode, content_type: &str, body: Vec<u8>) -> Result<HttpResponse> {
	let mut headers = HashMap::new();
	headers.insert(
		http::header::CONTENT_TYPE.to_string(),
		content_type.to_owned(),
	);
	Ok(HttpResponse {
		status: status.as_u16(),
		headers,
		body: Some(body),
		body_stream: None,
	})
}

fn method_not_allowed_response(
	request: &Request,
	actor: Option<&ActorSpecifier>,
) -> Result<HttpResponse> {
	let encoding = request_encoding(request.headers());
	message_boundary_error_response_with_actor(
		encoding,
		framework_error_status("actor", "method_not_allowed"),
		MethodNotAllowed {
			method: request.method().to_string(),
			path: request.uri().path().to_owned(),
		}
		.build(),
		actor,
	)
}

fn rearm_sleep_after_request(ctx: ActorContext) {
	let sleep_ctx = ctx.clone();
	ctx.wait_until(async move {
		sleep_ctx.wait_for_http_requests_idle().await;
		sleep_ctx.reset_sleep_timer();
	});
}

pub(super) fn single_path_segment<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
	let segment = path.strip_prefix(prefix)?;
	(!segment.is_empty() && !segment.contains('/')).then_some(segment)
}

pub(super) fn percent_decode_path_segment(segment: &str) -> Result<String> {
	let bytes = segment.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'%' {
			if i + 2 >= bytes.len() {
				return Err(invalid_path_segment("incomplete percent escape"));
			}
			let hi = hex_value(bytes[i + 1])
				.ok_or_else(|| invalid_path_segment("invalid percent escape"))?;
			let lo = hex_value(bytes[i + 2])
				.ok_or_else(|| invalid_path_segment("invalid percent escape"))?;
			out.push((hi << 4) | lo);
			i += 3;
		} else {
			out.push(bytes[i]);
			i += 1;
		}
	}
	String::from_utf8(out).context("path segment is not valid utf-8")
}

fn invalid_path_segment(reason: &str) -> anyhow::Error {
	ProtocolError::InvalidHttpRequest {
		field: "path segment".to_owned(),
		reason: reason.to_owned(),
	}
	.build()
}

pub(super) fn hex_value(byte: u8) -> Option<u8> {
	match byte {
		b'0'..=b'9' => Some(byte - b'0'),
		b'a'..=b'f' => Some(byte - b'a' + 10),
		b'A'..=b'F' => Some(byte - b'A' + 10),
		_ => None,
	}
}

pub(super) fn http_conn_params(headers: &http::HeaderMap) -> Result<Vec<u8>> {
	let Some(raw) = headers
		.get("x-rivet-conn-params")
		.and_then(|value| value.to_str().ok())
	else {
		return encode_json_as_cbor(&JsonValue::Null);
	};
	let value: JsonValue = serde_json::from_str(raw).context("parse x-rivet-conn-params header")?;
	encode_json_as_cbor(&value)
}

pub(super) fn authorization_bearer_token(headers: &http::HeaderMap) -> Option<&str> {
	headers
		.get(http::header::AUTHORIZATION)
		.and_then(|value| value.to_str().ok())
		.and_then(bearer_token_from_authorization)
}

pub(super) fn authorization_bearer_token_map(headers: &HashMap<String, String>) -> Option<&str> {
	headers
		.iter()
		.find(|(name, _)| name.eq_ignore_ascii_case(http::header::AUTHORIZATION.as_str()))
		.and_then(|(_, value)| bearer_token_from_authorization(value))
}

pub(super) async fn build_http_request(request: HttpRequest) -> Result<Request> {
	let mut body = request.body.unwrap_or_default();
	if let Some(mut body_stream) = request.body_stream {
		while let Some(chunk) = body_stream.recv().await {
			body.extend_from_slice(&chunk);
		}
	}

	let request_path = normalize_actor_request_path(&request.path);
	Request::from_parts(&request.method, &request_path, request.headers, body)
		.with_context(|| format!("build actor request for `{}`", request.path))
}

pub(super) fn normalize_actor_request_path(path: &str) -> String {
	let Some(stripped) = path.strip_prefix("/request") else {
		return path.to_owned();
	};

	if stripped.is_empty() {
		return "/".to_owned();
	}

	match stripped.as_bytes().first() {
		Some(b'/') | Some(b'?') => stripped.to_owned(),
		_ => path.to_owned(),
	}
}

#[cfg(test)]
pub(super) fn is_actor_request_path(path: &str) -> bool {
	let Some(stripped) = path.strip_prefix("/request") else {
		return false;
	};

	stripped.is_empty()
		|| stripped
			.as_bytes()
			.first()
			.is_some_and(|byte| matches!(byte, b'/' | b'?'))
}

pub(super) fn build_envoy_response(response: Response) -> Result<HttpResponse> {
	let (status, headers, body) = response.to_parts();

	Ok(HttpResponse {
		status,
		headers,
		body: Some(body),
		body_stream: None,
	})
}

fn actor_specifier_for_instance(instance: &ActorTaskHandle) -> ActorSpecifier {
	ActorSpecifier::new(instance.actor_id.clone(), instance.generation as u64)
		.with_key(format_actor_key(instance.ctx.key()))
}

fn attach_actor_response_headers(response: &mut HttpResponse, actor: &ActorSpecifier) {
	response
		.headers
		.insert(HEADER_RIVET_ACTOR.to_owned(), actor.actor_id.clone());
	response.headers.insert(
		HEADER_RIVET_ACTOR_GENERATION.to_owned(),
		actor.generation.to_string(),
	);
	if let Some(key) = actor.key.as_ref() {
		if !key.contains('\r') && !key.contains('\n') {
			response
				.headers
				.insert(HEADER_RIVET_ACTOR_KEY.to_owned(), key.clone());
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HttpResponseEncoding {
	Json,
	Cbor,
	Bare,
}

pub(super) fn request_encoding(headers: &http::HeaderMap) -> HttpResponseEncoding {
	headers
		.get("x-rivet-encoding")
		.and_then(|value| value.to_str().ok())
		.map(|value| match value {
			"cbor" => HttpResponseEncoding::Cbor,
			"bare" => HttpResponseEncoding::Bare,
			_ => HttpResponseEncoding::Json,
		})
		.unwrap_or(HttpResponseEncoding::Json)
}

pub(super) fn message_boundary_error_response(
	encoding: HttpResponseEncoding,
	status: StatusCode,
	error: anyhow::Error,
) -> Result<HttpResponse> {
	message_boundary_error_response_with_actor(encoding, status, error, None)
}

pub(super) fn message_boundary_error_response_with_actor(
	encoding: HttpResponseEncoding,
	status: StatusCode,
	error: anyhow::Error,
	fallback_actor: Option<&ActorSpecifier>,
) -> Result<HttpResponse> {
	let error = RivetError::extract(&error);
	let actor = error.actor().or(fallback_actor);
	let metadata = error.metadata();
	if let Some(actor) = actor {
		tracing::warn!(
			actor_id = %actor.actor_id,
			generation = actor.generation,
			actor_key = ?actor.key,
			group = error.group(),
			code = error.code(),
			"actor http error response"
		);
	}
	let body = serialize_http_response_error(
		encoding,
		error.group(),
		error.code(),
		client_error_message(error.group(), error.code(), error.message()),
		client_error_metadata(error.group(), error.code(), metadata.as_ref()),
		actor,
	)?;

	let mut response = HttpResponse {
		status: status.as_u16(),
		headers: HashMap::from([(
			http::header::CONTENT_TYPE.to_string(),
			content_type_for_encoding(encoding).to_owned(),
		)]),
		body: Some(body),
		body_stream: None,
	};
	if let Some(actor) = actor {
		attach_actor_response_headers(&mut response, actor);
	}
	Ok(response)
}

pub(super) fn content_type_for_encoding(encoding: HttpResponseEncoding) -> &'static str {
	match encoding {
		HttpResponseEncoding::Json => "application/json",
		HttpResponseEncoding::Cbor | HttpResponseEncoding::Bare => "application/octet-stream",
	}
}

/// Bounded serde metric `format` label for the request/response encoding.
fn encoding_format_label(encoding: HttpResponseEncoding) -> &'static str {
	match encoding {
		HttpResponseEncoding::Json => "json",
		HttpResponseEncoding::Cbor => "cbor",
		HttpResponseEncoding::Bare => "bare",
	}
}

pub(super) fn serialize_http_response_error(
	encoding: HttpResponseEncoding,
	group: &str,
	code: &str,
	message: &str,
	metadata: Option<&JsonValue>,
	actor: Option<&ActorSpecifier>,
) -> Result<Vec<u8>> {
	let mut json_body = json!({
		"group": group,
		"code": code,
		"message": message,
	});
	if let Some(metadata) = metadata {
		json_body["metadata"] = metadata.clone();
	}
	if let Some(actor) = actor {
		json_body["actor"] = serde_json::to_value(actor)?;
	}

	match encoding {
		HttpResponseEncoding::Json => Ok(serde_json::to_vec(&json_body)?),
		HttpResponseEncoding::Cbor => {
			let mut out = Vec::new();
			ciborium::into_writer(&json_body, &mut out)?;
			Ok(out)
		}
		HttpResponseEncoding::Bare => {
			let metadata = metadata
				.map(|value| {
					let mut out = Vec::new();
					ciborium::into_writer(value, &mut out)?;
					Ok::<Vec<u8>, anyhow::Error>(out)
				})
				.transpose()?;
			client_protocol::versioned::HttpResponseError::wrap_latest(
				client_protocol::HttpResponseError {
					group: group.to_owned(),
					code: code.to_owned(),
					message: message.to_owned(),
					metadata,
					actor: actor.map(|actor| client_protocol::ActorSpecifier {
						actor_id: actor.actor_id.clone(),
						generation: serde_bare::Uint(actor.generation),
						key: actor.key.clone(),
					}),
				},
			)
			.serialize_with_embedded_version(client_protocol::PROTOCOL_VERSION)
		}
	}
}

pub(super) fn decode_http_action_args(
	encoding: HttpResponseEncoding,
	body: &[u8],
) -> Result<Vec<u8>> {
	serde_metrics::measure_deserialize(
		encoding_format_label(encoding),
		"http_action_request",
		body.len(),
		|| match encoding {
			HttpResponseEncoding::Json => {
				let request: HttpActionRequestJson =
					serde_json::from_slice(body).context("decode json HTTP action request")?;
				let args = normalize_json_args(request.args);
				encode_json_as_cbor(&args)
			}
			HttpResponseEncoding::Cbor => {
				let request: HttpActionRequestJson = ciborium::from_reader(Cursor::new(body))
					.context("decode cbor HTTP action request")?;
				let args = normalize_json_args(request.args);
				encode_json_as_cbor(&args)
			}
			HttpResponseEncoding::Bare => {
				let request =
					<client_protocol::versioned::HttpActionRequest as OwnedVersionedData>::deserialize_with_embedded_version(body)
						.context("decode bare HTTP action request")?;
				Ok(request.args)
			}
		},
	)
}

fn normalize_json_args(args: JsonValue) -> Vec<JsonValue> {
	match args {
		JsonValue::Array(args) => args,
		JsonValue::Null => Vec::new(),
		value => vec![value],
	}
}

pub(super) fn decode_http_queue_request(
	encoding: HttpResponseEncoding,
	body: &[u8],
) -> Result<DecodedHttpQueueRequest> {
	match encoding {
		HttpResponseEncoding::Json => {
			let request: HttpQueueSendRequestJson =
				serde_json::from_slice(body).context("decode json HTTP queue request")?;
			Ok(DecodedHttpQueueRequest {
				body: encode_json_as_cbor(&request.body)?,
				wait: request.wait.unwrap_or(false),
				timeout: request.timeout,
			})
		}
		HttpResponseEncoding::Cbor => {
			let request: HttpQueueSendRequestJson = ciborium::from_reader(Cursor::new(body))
				.context("decode cbor HTTP queue request")?;
			Ok(DecodedHttpQueueRequest {
				body: encode_json_as_cbor(&request.body)?,
				wait: request.wait.unwrap_or(false),
				timeout: request.timeout,
			})
		}
		HttpResponseEncoding::Bare => {
			let request =
				<client_protocol::versioned::HttpQueueSendRequest as OwnedVersionedData>::deserialize_with_embedded_version(body)
					.context("decode bare HTTP queue request")?;
			Ok(DecodedHttpQueueRequest {
				body: request.body,
				wait: request.wait.unwrap_or(false),
				timeout: request.timeout,
			})
		}
	}
}

pub(super) fn encode_http_action_response(
	encoding: HttpResponseEncoding,
	output: Vec<u8>,
) -> Result<HttpResponse> {
	let body = serde_metrics::measure_serialize(
		encoding_format_label(encoding),
		"http_action_response",
		|| {
			let body = match encoding {
				HttpResponseEncoding::Json => serde_json::to_vec(&json!({
					"output": decode_cbor_json_or_null(&output),
				}))?,
				HttpResponseEncoding::Cbor => {
					let mut out = Vec::new();
					ciborium::into_writer(
						&json!({
							"output": decode_cbor_json_or_null(&output),
						}),
						&mut out,
					)?;
					out
				}
				HttpResponseEncoding::Bare => {
					client_protocol::versioned::HttpActionResponse::wrap_latest(
						client_protocol::HttpActionResponse { output },
					)
					.serialize_with_embedded_version(client_protocol::PROTOCOL_VERSION)?
				}
			};
			Ok(body)
		},
	)?;
	Ok(HttpResponse {
		status: StatusCode::OK.as_u16(),
		headers: HashMap::from([(
			http::header::CONTENT_TYPE.to_string(),
			content_type_for_encoding(encoding).to_owned(),
		)]),
		body: Some(body),
		body_stream: None,
	})
}

pub(super) fn encode_http_queue_response(
	encoding: HttpResponseEncoding,
	result: QueueSendResult,
) -> Result<HttpResponse> {
	let body = match encoding {
		HttpResponseEncoding::Json => {
			let mut value = serde_json::Map::new();
			value.insert("status".to_owned(), json!(result.status.as_str()));
			if let Some(response) = result.response {
				value.insert("response".to_owned(), decode_cbor_json_or_null(&response));
			}
			serde_json::to_vec(&JsonValue::Object(value))?
		}
		HttpResponseEncoding::Cbor => {
			let mut value = serde_json::Map::new();
			value.insert("status".to_owned(), json!(result.status.as_str()));
			if let Some(response) = result.response {
				value.insert("response".to_owned(), decode_cbor_json_or_null(&response));
			}
			let mut out = Vec::new();
			ciborium::into_writer(&JsonValue::Object(value), &mut out)?;
			out
		}
		HttpResponseEncoding::Bare => {
			client_protocol::versioned::HttpQueueSendResponse::wrap_latest(
				client_protocol::HttpQueueSendResponse {
					status: result.status.as_str().to_owned(),
					response: result.response,
				},
			)
			.serialize_with_embedded_version(client_protocol::PROTOCOL_VERSION)?
		}
	};
	Ok(HttpResponse {
		status: StatusCode::OK.as_u16(),
		headers: HashMap::from([(
			http::header::CONTENT_TYPE.to_string(),
			content_type_for_encoding(encoding).to_owned(),
		)]),
		body: Some(body),
		body_stream: None,
	})
}

pub(super) fn framework_action_error_response(
	encoding: HttpResponseEncoding,
	error: ActionDispatchError,
	fallback_actor: Option<&ActorSpecifier>,
) -> Result<HttpResponse> {
	let status = framework_error_status(&error.group, &error.code);
	let actor = error.actor.as_ref().or(fallback_actor);
	if let Some(actor) = actor {
		tracing::warn!(
			actor_id = %actor.actor_id,
			generation = actor.generation,
			actor_key = ?actor.key,
			group = %error.group,
			code = %error.code,
			"actor framework action error response"
		);
	}
	let mut response = HttpResponse {
		status: status.as_u16(),
		headers: HashMap::from([(
			http::header::CONTENT_TYPE.to_string(),
			content_type_for_encoding(encoding).to_owned(),
		)]),
		body: Some(serialize_http_response_error(
			encoding,
			&error.group,
			&error.code,
			error.client_message(),
			error.client_metadata(),
			actor,
		)?),
		body_stream: None,
	};
	if let Some(actor) = actor {
		attach_actor_response_headers(&mut response, actor);
	}
	Ok(response)
}

pub(super) fn framework_anyhow_error_response_with_actor(
	encoding: HttpResponseEncoding,
	error: anyhow::Error,
	actor: Option<&ActorSpecifier>,
) -> Result<HttpResponse> {
	let status = framework_anyhow_status(&error);
	message_boundary_error_response_with_actor(encoding, status, error, actor)
}

pub(super) fn framework_anyhow_status(error: &anyhow::Error) -> StatusCode {
	let error = RivetError::extract(error);
	framework_error_status(error.group(), error.code())
}

pub(super) fn framework_error_status(group: &str, code: &str) -> StatusCode {
	match (group, code) {
		("auth", "forbidden") => StatusCode::FORBIDDEN,
		("actor", "action_not_found") => StatusCode::NOT_FOUND,
		("actor", "not_found") => StatusCode::NOT_FOUND,
		("actor", "action_timed_out") => StatusCode::REQUEST_TIMEOUT,
		("actor", "invalid_request") => StatusCode::BAD_REQUEST,
		("actor", "method_not_allowed") => StatusCode::METHOD_NOT_ALLOWED,
		("message", "incoming_too_long" | "outgoing_too_long") => StatusCode::BAD_REQUEST,
		("queue", _) => StatusCode::BAD_REQUEST,
		_ => StatusCode::INTERNAL_SERVER_ERROR,
	}
}

fn bearer_token_from_authorization(value: &str) -> Option<&str> {
	let value = value.trim_start();
	let scheme = value.get(..6)?;
	if !scheme.eq_ignore_ascii_case("bearer") {
		return None;
	}

	let rest = value.get(6..)?;
	if !rest.chars().next().is_some_and(char::is_whitespace) {
		return None;
	}

	let token = rest.trim_start();
	if token.is_empty() { None } else { Some(token) }
}

#[cfg(test)]
#[path = "../../tests/registry_http.rs"]
mod tests;
