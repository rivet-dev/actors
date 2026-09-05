use super::dispatch::*;
use super::http::*;
use super::*;
use crate::error::{ProtocolError, client_error_message};
use crate::inspector::InspectorTabEntry;
use ::http;

#[derive(rivet_error::RivetError, serde::Serialize)]
#[error(
	"inspector",
	"invalid_request",
	"Invalid inspector request",
	"Invalid inspector request {field}: {reason}"
)]
struct InspectorInvalidRequest {
	field: String,
	reason: String,
}

impl RegistryDispatcher {
	pub(super) async fn handle_inspector_fetch(
		&self,
		instance: &ActorTaskHandle,
		request: &Request,
	) -> Result<Option<HttpResponse>> {
		let url = inspector_request_url(request)?;
		if !url.path().starts_with("/inspector/") {
			return Ok(None);
		}

		if let Some(response) =
			crate::inspector_bundle::serve_inspector_bundle(request.method().as_str(), url.path())
		{
			return Ok(Some(response));
		}

		// Tab config is delivered in the inspector WS `Init` message (protocol
		// v5+), not over HTTP, so both the tab list and the capability flags
		// land atomically. The inspector UI is bundled into the same runtime
		// that serves it, so its protocol always matches and no HTTP fallback
		// is needed. Only custom-tab assets are still served over HTTP here.
		#[cfg(not(target_arch = "wasm32"))]
		if request.method() == http::Method::GET {
			if let Some(rest) = url.path().strip_prefix("/inspector/custom-tabs/") {
				return Ok(Some(serve_custom_tab_asset(
					instance.factory.config(),
					rest,
				)));
			}
		}

		if self.handle_inspector_http_in_runtime {
			return Ok(None);
		}
		if InspectorAuth::new()
			.verify(&instance.ctx, authorization_bearer_token(request.headers()))
			.await
			.is_err()
		{
			return Ok(Some(inspector_unauthorized_response()));
		}

		let method = request.method().clone();
		let path = url.path();
		let response = match (method, path) {
			(http::Method::GET, "/inspector/state") => json_http_response(
				StatusCode::OK,
				&json!({
					"state": decode_cbor_json_or_null(&instance.ctx.state()),
					"isStateEnabled": instance.ctx.has_state(),
				}),
			),
			(http::Method::PATCH, "/inspector/state") => {
				let body: InspectorPatchStateBody = match parse_json_body(request) {
					Ok(body) => body,
					Err(response) => return Ok(Some(response)),
				};
				let state = encode_json_as_cbor(&body.state)?;
				match instance
					.ctx
					.save_state(vec![StateDelta::ActorState(state)])
					.await
				{
					Ok(_) => json_http_response(StatusCode::OK, &json!({ "ok": true })),
					Err(error) => Err(error).context("save inspector state patch"),
				}
			}
			(http::Method::GET, "/inspector/connections") => json_http_response(
				StatusCode::OK,
				&json!({
					"connections": inspector_connections(&instance.ctx),
				}),
			),
			(http::Method::GET, "/inspector/rpcs") => json_http_response(
				StatusCode::OK,
				&json!({
					"rpcs": inspector_rpcs(instance),
				}),
			),
			(http::Method::POST, action_path) if action_path.starts_with("/inspector/action/") => {
				let action_name = action_path
					.trim_start_matches("/inspector/action/")
					.to_owned();
				let body: InspectorActionBody = match parse_json_body(request) {
					Ok(body) => body,
					Err(response) => return Ok(Some(response)),
				};
				if !body.args.is_empty() && body.properties.is_some() {
					return Ok(Some(json_http_response(
						StatusCode::BAD_REQUEST,
						&json!({
							"error": "use either args or properties, not both",
						}),
					)?));
				}
				if body
					.properties
					.as_ref()
					.is_some_and(|properties| !properties.is_object())
				{
					return Ok(Some(json_http_response(
						StatusCode::BAD_REQUEST,
						&json!({
							"error": "properties must be an object",
						}),
					)?));
				}
				let args = if let Some(properties) = body.properties {
					vec![properties]
				} else {
					body.args
				};
				match self
					.execute_inspector_action(instance, &action_name, args)
					.await
				{
					Ok(output) => json_http_response(
						StatusCode::OK,
						&json!({
							"output": output,
						}),
					),
					Err(error) => Ok(action_error_response(error)),
				}
			}
			(http::Method::GET, "/inspector/queue") => {
				let limit = match parse_u32_query_param(&url, "limit", 100) {
					Ok(limit) => limit,
					Err(response) => return Ok(Some(response)),
				};
				let messages = match instance.ctx.queue().inspect_messages().await {
					Ok(messages) => messages,
					Err(error) => {
						return Ok(Some(inspector_anyhow_response(
							error.context("list inspector queue messages"),
						)));
					}
				};
				let queue_size = messages.len().try_into().unwrap_or(u32::MAX);
				let truncated = messages.len() > limit as usize;
				let messages = messages
					.into_iter()
					.take(limit as usize)
					.map(|message| InspectorQueueMessageJson {
						id: message.id,
						name: message.name,
						created_at_ms: message.created_at,
					})
					.collect();
				let payload = InspectorQueueResponseJson {
					size: queue_size,
					max_size: instance.ctx.queue().max_size(),
					truncated,
					messages,
				};
				json_http_response(StatusCode::OK, &payload)
			}
			(http::Method::DELETE, "/inspector/queue") => {
				match instance.ctx.queue().reset().await {
					Ok(()) => json_http_response(StatusCode::OK, &json!({})),
					Err(error) => Err(error).context("reset inspector queue"),
				}
			}
			(http::Method::POST, "/inspector/queue") => {
				let body: InspectorEnqueueBody = match parse_json_body(request) {
					Ok(body) => body,
					Err(response) => return Ok(Some(response)),
				};
				if body.name.is_empty() {
					return Ok(Some(inspector_error_response(
						StatusCode::BAD_REQUEST,
						"actor",
						"invalid_request",
						"Queue message name must not be empty",
					)));
				}
				let cbor_body = encode_json_as_cbor(&body.body.unwrap_or(serde_json::Value::Null))?;
				match instance.ctx.queue().send(&body.name, &cbor_body).await {
					Ok(message) => json_http_response(
						StatusCode::OK,
						&json!({
							"id": message.id.to_string(),
							"name": message.name,
							"createdAtMs": message.created_at,
						}),
					),
					Err(error) => Err(error).context("enqueue inspector queue message"),
				}
			}
			(http::Method::GET, "/inspector/workflow-history") => self
				.inspector_workflow_history(instance)
				.await
				.and_then(|(workflow_supported, history)| {
					json_http_response(
						StatusCode::OK,
						&json!({
							"history": history,
							"isWorkflowEnabled": workflow_supported,
						}),
					)
				}),
			(http::Method::POST, "/inspector/workflow/replay") => {
				let body: InspectorWorkflowReplayBody = match parse_json_body(request) {
					Ok(body) => body,
					Err(response) => return Ok(Some(response)),
				};
				self.inspector_workflow_replay(instance, body.entry_id)
					.await
					.and_then(|(workflow_supported, history)| {
						json_http_response(
							StatusCode::OK,
							&json!({
								"history": history,
								"isWorkflowEnabled": workflow_supported,
							}),
						)
					})
			}
			// TODO: Impelment when ready
			(http::Method::GET, "/inspector/traces") => json_http_response(
				StatusCode::OK,
				&json!({
					"otlp": Vec::<u8>::new(),
					"clamped": false,
				}),
			),
			(http::Method::GET, "/inspector/database/schema") => self
				.inspector_database_schema(&instance.ctx)
				.await
				.context("load inspector database schema")
				.and_then(|payload| {
					json_http_response(StatusCode::OK, &json!({ "schema": payload }))
				}),
			(http::Method::GET, "/inspector/database/rows") => {
				let table = match required_query_param(&url, "table") {
					Ok(table) => table,
					Err(response) => return Ok(Some(response)),
				};
				let limit = match parse_u32_query_param(&url, "limit", 100) {
					Ok(limit) => limit,
					Err(response) => return Ok(Some(response)),
				};
				let offset = match parse_u32_query_param(&url, "offset", 0) {
					Ok(offset) => offset,
					Err(response) => return Ok(Some(response)),
				};
				self.inspector_database_rows(&instance.ctx, &table, limit, offset)
					.await
					.context("load inspector database rows")
					.and_then(|rows| json_http_response(StatusCode::OK, &json!({ "rows": rows })))
			}
			(http::Method::POST, "/inspector/database/execute") => {
				let body: InspectorDatabaseExecuteBody = match parse_json_body(request) {
					Ok(body) => body,
					Err(response) => return Ok(Some(response)),
				};
				self.inspector_database_execute(&instance.ctx, body)
					.await
					.context("execute inspector database query")
					.and_then(|rows| json_http_response(StatusCode::OK, &json!({ "rows": rows })))
			}
			(http::Method::GET, "/inspector/summary") => self
				.inspector_summary(instance)
				.await
				.and_then(|summary| json_http_response(StatusCode::OK, &summary)),
			_ => Ok(inspector_error_response(
				StatusCode::NOT_FOUND,
				"actor",
				"not_found",
				"Inspector route was not found",
			)),
		};

		Ok(Some(match response {
			Ok(response) => response,
			Err(error) => inspector_anyhow_response(error),
		}))
	}

	async fn execute_inspector_action(
		&self,
		instance: &ActorTaskHandle,
		action_name: &str,
		args: Vec<JsonValue>,
	) -> std::result::Result<JsonValue, ActionDispatchError> {
		self.execute_inspector_action_bytes(
			instance,
			action_name,
			encode_json_as_cbor(&args).map_err(ActionDispatchError::from_anyhow)?,
		)
		.await
		.map(|payload| decode_cbor_json_or_null(&payload))
	}

	pub(super) async fn execute_inspector_action_bytes(
		&self,
		instance: &ActorTaskHandle,
		action_name: &str,
		args: Vec<u8>,
	) -> std::result::Result<Vec<u8>, ActionDispatchError> {
		tracing::info!(
			action_name,
			args_len = args.len(),
			"inspector RPC: connecting transient conn"
		);
		let conn = match instance
			.ctx
			.connect_conn(Vec::new(), false, None, None, async { Ok(Vec::new()) })
			.await
		{
			Ok(conn) => conn,
			Err(error) => {
				tracing::warn!(action_name, ?error, "inspector RPC: connect_conn failed");
				return Err(ActionDispatchError::from_anyhow(error));
			}
		};
		tracing::info!(
			action_name,
			conn_id = ?conn.id(),
			"inspector RPC: dispatching to actor task"
		);
		let output = dispatch_action_through_task(
			&instance.dispatch,
			conn.clone(),
			action_name.to_owned(),
			args,
			crate::telemetry::IncomingInvocationContext::default(),
		)
		.await;
		match &output {
			Ok(bytes) => tracing::info!(
				action_name,
				output_len = bytes.len(),
				"inspector RPC: action returned"
			),
			Err(error) => tracing::warn!(action_name, ?error, "inspector RPC: action errored"),
		}
		if let Err(error) = conn.disconnect(None).await {
			tracing::warn!(
				?error,
				action_name,
				"failed to disconnect inspector action connection"
			);
		}
		output
	}

	async fn inspector_summary(&self, instance: &ActorTaskHandle) -> Result<InspectorSummaryJson> {
		let queue_messages = instance
			.ctx
			.queue()
			.inspect_messages()
			.await
			.context("list queue messages for inspector summary")?;
		let (workflow_supported, workflow_history) = self
			.inspector_workflow_history(instance)
			.await
			.context("load inspector workflow summary")?;
		Ok(InspectorSummaryJson {
			state: decode_cbor_json_or_null(&instance.ctx.state()),
			is_state_enabled: instance.ctx.has_state(),
			connections: inspector_connections(&instance.ctx),
			rpcs: inspector_rpcs(instance),
			queue_size: queue_messages.len().try_into().unwrap_or(u32::MAX),
			is_database_enabled: instance.ctx.sql().is_enabled(),
			workflow_supported,
			workflow_history,
		})
	}

	async fn inspector_workflow_history(
		&self,
		instance: &ActorTaskHandle,
	) -> Result<(bool, Option<JsonValue>)> {
		self.inspector_workflow_history_bytes(instance).await.map(
			|(workflow_supported, history)| {
				(workflow_supported, warn_and_omit_workflow_history(history))
			},
		)
	}

	async fn inspector_workflow_replay(
		&self,
		instance: &ActorTaskHandle,
		entry_id: Option<String>,
	) -> Result<(bool, Option<JsonValue>)> {
		self.inspector_workflow_replay_bytes(instance, entry_id)
			.await
			.map(|(workflow_supported, history)| {
				(workflow_supported, warn_and_omit_workflow_history(history))
			})
	}

	pub(super) async fn inspector_workflow_history_bytes(
		&self,
		instance: &ActorTaskHandle,
	) -> Result<(bool, Option<Vec<u8>>)> {
		let result = instance
			.ctx
			.internal_keep_awake(dispatch_workflow_history_through_task(&instance.dispatch))
			.await
			.context("load inspector workflow history");

		workflow_dispatch_result(result)
	}

	pub(super) async fn inspector_workflow_replay_bytes(
		&self,
		instance: &ActorTaskHandle,
		entry_id: Option<String>,
	) -> Result<(bool, Option<Vec<u8>>)> {
		let result = instance
			.ctx
			.internal_keep_awake(dispatch_workflow_replay_request_through_task(
				&instance.dispatch,
				entry_id,
			))
			.await
			.context("replay inspector workflow history");
		let (workflow_supported, history) = workflow_dispatch_result(result)?;
		if workflow_supported {
			instance.inspector.record_workflow_history_updated();
		}

		Ok((workflow_supported, history))
	}

	async fn inspector_database_schema(&self, ctx: &ActorContext) -> Result<JsonValue> {
		self.inspector_database_schema_bytes(ctx)
			.await
			.map(|payload| decode_cbor_json_or_null(&payload))
	}

	pub(super) async fn inspector_database_schema_bytes(
		&self,
		ctx: &ActorContext,
	) -> Result<Vec<u8>> {
		let tables = decode_cbor_json_or_null(
			&ctx
				.db_query(
					"SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '__drizzle_%' ORDER BY name",
					None,
				)
				.await
				.context("query sqlite master tables")?,
		);
		let JsonValue::Array(tables) = tables else {
			return encode_json_as_cbor(&json!({ "tables": [] }));
		};

		let mut inspector_tables = Vec::with_capacity(tables.len());
		for table in tables {
			let name = table
				.get("name")
				.and_then(JsonValue::as_str)
				.ok_or_else(|| {
					ProtocolError::InvalidPersistedData {
						label: "sqlite schema row".to_owned(),
						reason: "missing table name".to_owned(),
					}
					.build()
				})?;
			let table_type = table
				.get("type")
				.and_then(JsonValue::as_str)
				.unwrap_or("table");
			let quoted = quote_sql_identifier(name);

			let columns = decode_cbor_json_or_null(
				&ctx.db_query(&format!("PRAGMA table_info({quoted})"), None)
					.await
					.with_context(|| format!("query pragma table_info for `{name}`"))?,
			);
			let foreign_keys = decode_cbor_json_or_null(
				&ctx.db_query(&format!("PRAGMA foreign_key_list({quoted})"), None)
					.await
					.with_context(|| format!("query pragma foreign_key_list for `{name}`"))?,
			);
			let count_rows = decode_cbor_json_or_null(
				&ctx.db_query(&format!("SELECT COUNT(*) as count FROM {quoted}"), None)
					.await
					.with_context(|| format!("count rows for `{name}`"))?,
			);
			let records = count_rows
				.as_array()
				.and_then(|rows| rows.first())
				.and_then(|row| row.get("count"))
				.and_then(JsonValue::as_u64)
				.unwrap_or(0);

			inspector_tables.push(json!({
				"table": {
					"schema": "main",
					"name": name,
					"type": table_type,
				},
				"columns": columns,
				"foreignKeys": foreign_keys,
				"records": records,
			}));
		}

		encode_json_as_cbor(&json!({ "tables": inspector_tables }))
	}

	async fn inspector_database_rows(
		&self,
		ctx: &ActorContext,
		table: &str,
		limit: u32,
		offset: u32,
	) -> Result<JsonValue> {
		self.inspector_database_rows_bytes(ctx, table, limit, offset)
			.await
			.map(|payload| decode_cbor_json_or_null(&payload))
	}

	pub(super) async fn inspector_database_rows_bytes(
		&self,
		ctx: &ActorContext,
		table: &str,
		limit: u32,
		offset: u32,
	) -> Result<Vec<u8>> {
		let params = encode_json_as_cbor(&vec![json!(limit.min(500)), json!(offset)])?;
		ctx.db_query(
			&format!(
				"SELECT * FROM {} LIMIT ? OFFSET ?",
				quote_sql_identifier(table)
			),
			Some(&params),
		)
		.await
		.with_context(|| format!("query rows for `{table}`"))
	}

	async fn inspector_database_execute(
		&self,
		ctx: &ActorContext,
		body: InspectorDatabaseExecuteBody,
	) -> Result<JsonValue> {
		if body.sql.trim().is_empty() {
			return Err(InspectorInvalidRequest {
				field: "sql".to_owned(),
				reason: "must be non-empty".to_owned(),
			}
			.build());
		}
		if !body.args.is_empty() && body.properties.is_some() {
			return Err(InspectorInvalidRequest {
				field: "parameters".to_owned(),
				reason: "use either args or properties, not both".to_owned(),
			}
			.build());
		}

		let params = if let Some(properties) = body.properties {
			Some(encode_json_as_cbor(&properties)?)
		} else if body.args.is_empty() {
			None
		} else {
			Some(encode_json_as_cbor(&body.args)?)
		};

		let rows = ctx
			.db_execute(&body.sql, params.as_deref())
			.await
			.context("run inspector database statement")?;
		Ok(decode_cbor_json_or_null(&rows))
	}
}

pub(super) fn inspector_connections(ctx: &ActorContext) -> Vec<InspectorConnectionJson> {
	ctx.conns()
		.map(|conn| InspectorConnectionJson {
			connection_type: None,
			id: conn.id().to_owned(),
			details: InspectorConnectionDetailsJson {
				connection_type: None,
				params: decode_cbor_json_or_null(&conn.params()),
				state_enabled: true,
				state: decode_cbor_json_or_null(&conn.state()),
				subscriptions: conn.subscriptions().len(),
				is_hibernatable: conn.is_hibernatable(),
			},
		})
		.collect()
}

pub(super) fn decode_inspector_overlay_state(payload: &[u8]) -> Result<Option<Vec<u8>>> {
	let deltas: Vec<StateDelta> =
		ciborium::from_reader(Cursor::new(payload)).context("decode inspector overlay deltas")?;
	Ok(deltas.into_iter().find_map(|delta| match delta {
		StateDelta::ActorState(bytes) => Some(bytes),
		StateDelta::ConnHibernation { .. } | StateDelta::ConnHibernationRemoved(_) => None,
	}))
}

pub(super) fn inspector_wire_connections(
	ctx: &ActorContext,
) -> Vec<inspector_protocol::ConnectionDetails> {
	ctx.conns()
		.map(|conn| {
			let details = json!({
				"type": JsonValue::Null,
				"params": decode_cbor_json_or_null(&conn.params()),
				"stateEnabled": true,
				"state": decode_cbor_json_or_null(&conn.state()),
				"subscriptions": conn.subscriptions().len(),
				"isHibernatable": conn.is_hibernatable(),
			});
			let details = match encode_json_as_cbor(&details) {
				Ok(details) => details,
				Err(error) => {
					tracing::warn!(
						?error,
						conn_id = conn.id(),
						"failed to encode inspector connection details"
					);
					Vec::new()
				}
			};
			inspector_protocol::ConnectionDetails {
				id: conn.id().to_owned(),
				details,
			}
		})
		.collect()
}

/// Builds the inspector tab-config descriptors in wire form for the WS `Init`
/// message (protocol v5+). This is the single source of truth for the tab list.
pub(super) fn inspector_wire_tab_config(
	config: &crate::ActorConfig,
) -> Vec<inspector_protocol::TabConfigEntry> {
	config
		.inspector_tabs
		.iter()
		.map(|entry| match entry {
			InspectorTabEntry::Custom {
				id, label, icon, ..
			} => inspector_protocol::TabConfigEntry {
				id: id.clone(),
				label: Some(label.clone()),
				icon: icon.clone(),
				hidden: false,
			},
			InspectorTabEntry::HideBuiltin { id } => inspector_protocol::TabConfigEntry {
				id: id.clone(),
				label: None,
				icon: None,
				hidden: true,
			},
		})
		.collect()
}

pub(super) fn build_actor_inspector() -> Inspector {
	Inspector::new()
}

pub(super) fn inspector_rpcs(instance: &ActorTaskHandle) -> Vec<String> {
	instance
		.factory
		.config()
		.actions
		.iter()
		.map(|action| action.name.clone())
		.collect()
}

pub(super) fn inspector_request_url(request: &Request) -> Result<Url> {
	Url::parse(&format!("http://inspector{}", request.uri())).context("parse inspector request url")
}

pub(super) fn decode_cbor_json_or_null(payload: &[u8]) -> JsonValue {
	decode_cbor_json(payload).unwrap_or(JsonValue::Null)
}

pub(super) fn decode_cbor_json(payload: &[u8]) -> Result<JsonValue> {
	if payload.is_empty() {
		return Ok(JsonValue::Null);
	}

	ciborium::from_reader::<JsonValue, _>(Cursor::new(payload))
		.context("decode cbor payload as json")
}

pub(super) fn encode_json_as_cbor(value: &impl Serialize) -> Result<Vec<u8>> {
	let mut encoded = Vec::new();
	ciborium::into_writer(value, &mut encoded).context("encode inspector payload as cbor")?;
	Ok(encoded)
}

pub(super) fn quote_sql_identifier(identifier: &str) -> String {
	format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub(super) fn json_http_response(
	status: StatusCode,
	payload: &impl Serialize,
) -> Result<HttpResponse> {
	let mut headers = HashMap::new();
	headers.insert(
		http::header::CONTENT_TYPE.to_string(),
		"application/json".to_owned(),
	);
	// The gateway accepts a path-segment token (`/gateway/<id>@<token>/...`),
	// which the dashboard iframe uses to ride X-Rivet-Token auth into
	// header-less navigations. Without `Referrer-Policy: no-referrer` the
	// token would leak via `Referer` on any cross-origin sub-resource
	// fetch from inside the iframe (fonts, images, analytics).
	headers.insert(
		http::header::REFERRER_POLICY.to_string(),
		"no-referrer".to_owned(),
	);
	Ok(HttpResponse {
		status: status.as_u16(),
		headers,
		body: Some(serde_json::to_vec(payload).context("serialize inspector json response")?),
		body_stream: None,
	})
}

pub(super) fn inspector_unauthorized_response() -> HttpResponse {
	inspector_error_response(
		StatusCode::UNAUTHORIZED,
		"inspector",
		"unauthorized",
		"Inspector request requires a valid bearer token",
	)
}

pub(super) fn action_error_response(error: ActionDispatchError) -> HttpResponse {
	let status = if error.code == "action_not_found" {
		StatusCode::NOT_FOUND
	} else {
		StatusCode::INTERNAL_SERVER_ERROR
	};
	inspector_error_response(status, &error.group, &error.code, error.client_message())
}

pub(super) fn inspector_anyhow_response(error: anyhow::Error) -> HttpResponse {
	let error = RivetError::extract(&error);
	let status = inspector_error_status(error.group(), error.code());
	inspector_error_response(
		status,
		error.group(),
		error.code(),
		client_error_message(error.group(), error.code(), error.message()),
	)
}

pub(super) fn inspector_error_response(
	status: StatusCode,
	group: &str,
	code: &str,
	message: &str,
) -> HttpResponse {
	match json_http_response(
		status,
		&json!({
			"group": group,
			"code": code,
			"message": message,
			"metadata": JsonValue::Null,
		}),
	) {
		Ok(response) => response,
		Err(error) => {
			tracing::error!(
				?error,
				group,
				code,
				"failed to serialize inspector error response"
			);
			let mut headers = HashMap::new();
			headers.insert(
				http::header::CONTENT_TYPE.to_string(),
				"application/json".to_owned(),
			);
			HttpResponse {
				status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
				headers,
				body: Some(
					br#"{"group":"inspector","code":"internal_error","message":"Internal error.","metadata":null}"#
						.to_vec(),
				),
				body_stream: None,
			}
		}
	}
}

pub(super) fn inspector_error_status(group: &str, code: &str) -> StatusCode {
	match (group, code) {
		("auth", "unauthorized") | ("inspector", "unauthorized") => StatusCode::UNAUTHORIZED,
		("actor", "action_timed_out") => StatusCode::REQUEST_TIMEOUT,
		(_, "action_not_found") => StatusCode::NOT_FOUND,
		(_, "invalid_request") | (_, "state_not_enabled") | ("database", "not_enabled") => {
			StatusCode::BAD_REQUEST
		}
		_ => StatusCode::INTERNAL_SERVER_ERROR,
	}
}

// These JSON routes document `history` as decoded workflow JSON, but the bytes are BARE and the
// only codec lives in the TypeScript workflow engine, which serves these routes itself. Runtimes
// that fall through to core have no workflow engine, so history is expected to be absent. Decoding
// the bytes as CBOR here yields a garbage value rather than an error, so refuse instead.
fn warn_and_omit_workflow_history(history: Option<Vec<u8>>) -> Option<JsonValue> {
	if let Some(history) = history {
		tracing::warn!(
			byte_len = history.len(),
			"omitting workflow history from json inspector response, BARE decoding is not available in core",
		);
	}

	None
}

pub(super) fn parse_json_body<T>(request: &Request) -> std::result::Result<T, HttpResponse>
where
	T: serde::de::DeserializeOwned,
{
	serde_json::from_slice(request.body()).map_err(|error| {
		inspector_error_response(
			StatusCode::BAD_REQUEST,
			"actor",
			"invalid_request",
			&format!("Invalid inspector JSON body: {error}"),
		)
	})
}

pub(super) fn required_query_param(
	url: &Url,
	key: &str,
) -> std::result::Result<String, HttpResponse> {
	url.query_pairs()
		.find(|(name, _)| name == key)
		.map(|(_, value)| value.into_owned())
		.ok_or_else(|| {
			inspector_error_response(
				StatusCode::BAD_REQUEST,
				"actor",
				"invalid_request",
				&format!("Missing required query parameter `{key}`"),
			)
		})
}

pub(super) fn parse_u32_query_param(
	url: &Url,
	key: &str,
	default: u32,
) -> std::result::Result<u32, HttpResponse> {
	let Some(value) = url
		.query_pairs()
		.find(|(name, _)| name == key)
		.map(|(_, value)| value)
	else {
		return Ok(default);
	};
	value.parse::<u32>().map_err(|error| {
		inspector_error_response(
			StatusCode::BAD_REQUEST,
			"actor",
			"invalid_request",
			&format!("Invalid query parameter `{key}`: {error}"),
		)
	})
}

#[cfg(not(target_arch = "wasm32"))]
fn inspector_ui_response(path: &str, body: Vec<u8>) -> HttpResponse {
	let mut headers = HashMap::new();
	headers.insert(
		http::header::CONTENT_TYPE.to_string(),
		inspector_ui_mime(path).to_owned(),
	);
	headers.insert(
		http::header::CACHE_CONTROL.to_string(),
		"no-cache".to_owned(),
	);
	// See note on json_http_response: keep the iframe's path-segment token
	// out of `Referer` on any cross-origin sub-resource fetch.
	headers.insert(
		http::header::REFERRER_POLICY.to_string(),
		"no-referrer".to_owned(),
	);
	HttpResponse {
		status: StatusCode::OK.as_u16(),
		headers,
		body: Some(body),
		body_stream: None,
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn serve_custom_tab_asset(config: &crate::ActorConfig, rest: &str) -> HttpResponse {
	use std::path::{Component, Path};

	// Split <tab-id>/<asset-path>. An empty asset path means index.html.
	let (tab_id, asset_rel) = rest.split_once('/').unwrap_or((rest, ""));

	let Some(root) = config.inspector_tabs.iter().find_map(|entry| match entry {
		InspectorTabEntry::Custom { id, root, .. } if id == tab_id => Some(root),
		_ => None,
	}) else {
		return inspector_error_response(
			StatusCode::NOT_FOUND,
			"inspector",
			"custom_tab_not_found",
			"Inspector custom tab id was not registered on this actor",
		);
	};

	let candidate = if asset_rel.is_empty() || asset_rel.ends_with('/') {
		format!("{asset_rel}index.html")
	} else {
		asset_rel.to_owned()
	};

	// Path-traversal guard — only Normal path components allowed.
	let rel_path = Path::new(&candidate);
	if rel_path
		.components()
		.any(|c| !matches!(c, Component::Normal(_)))
	{
		return inspector_error_response(
			StatusCode::BAD_REQUEST,
			"inspector",
			"invalid_request",
			"Invalid inspector custom-tab asset path",
		);
	}

	let absolute = root.join(rel_path);

	// Containment check: even though the component guard rejects `..`, a
	// symlink inside the bundle directory could still resolve to a target
	// outside `root`. `canonicalize` follows every symlink in the chain;
	// requiring the result to start with the canonical root closes that
	// edge case. Canonicalize on every request because the bundle root
	// can be deleted or replaced after registry construction.
	let Ok(canonical_root) = root.canonicalize() else {
		// Debug rather than warn: this is "happy path is deleted" and the
		// TS layer already statSync'd the root at registry construction.
		// In a deployment where the directory was rm-rf'd post-startup we
		// don't want to spam warn per request.
		tracing::debug!(
			tab_id,
			root = %root.display(),
			"inspector custom-tab root failed to canonicalize"
		);
		return inspector_error_response(
			StatusCode::NOT_FOUND,
			"inspector",
			"custom_tab_asset_not_found",
			"Inspector custom-tab asset was not found in the tab's source directory",
		);
	};
	let Ok(canonical) = absolute.canonicalize() else {
		return inspector_error_response(
			StatusCode::NOT_FOUND,
			"inspector",
			"custom_tab_asset_not_found",
			"Inspector custom-tab asset was not found in the tab's source directory",
		);
	};
	if !canonical.starts_with(&canonical_root) {
		tracing::warn!(
			tab_id,
			root = %canonical_root.display(),
			resolved = %canonical.display(),
			"inspector custom-tab asset escaped its source root via symlink"
		);
		return inspector_error_response(
			StatusCode::BAD_REQUEST,
			"inspector",
			"invalid_request",
			"Invalid inspector custom-tab asset path",
		);
	}

	let Ok(bytes) = std::fs::read(&canonical) else {
		return inspector_error_response(
			StatusCode::NOT_FOUND,
			"inspector",
			"custom_tab_asset_not_found",
			"Inspector custom-tab asset was not found in the tab's source directory",
		);
	};
	tracing::debug!(
		tab_id,
		path = %canonical.display(),
		"served inspector custom-tab asset"
	);
	inspector_ui_response(&candidate, bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn inspector_ui_mime(path: &str) -> &'static str {
	match path.rsplit('.').next().unwrap_or("") {
		"html" => "text/html; charset=utf-8",
		"js" | "mjs" => "text/javascript; charset=utf-8",
		"css" => "text/css; charset=utf-8",
		"json" => "application/json; charset=utf-8",
		"map" => "application/json; charset=utf-8",
		"svg" => "image/svg+xml",
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"woff" => "font/woff",
		"woff2" => "font/woff2",
		"wasm" => "application/wasm",
		_ => "application/octet-stream",
	}
}
