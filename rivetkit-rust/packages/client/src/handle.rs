use crate::{
	common::{EncodingKind, RawWebSocket, TransportKind, HEADER_CONN_PARAMS, HEADER_ENCODING},
	connection::{start_connection, ActorConnection, ActorConnectionInner},
	protocol::{codec, query::*},
	remote_manager::{GatewayTarget, RemoteManager},
};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use reqwest::{
	header::{HeaderMap, HeaderValue},
	Method, Response,
};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::{
	ops::Deref,
	sync::{Arc, Mutex},
	time::Duration,
};

pub use crate::protocol::codec::{QueueSendResult, QueueSendStatus};

#[derive(Debug, Clone, Copy, Default)]
pub struct SendOpts {}

#[derive(Debug, Clone, Copy, Default)]
pub struct SendAndWaitOpts {
	pub timeout: Option<Duration>,
}

pub type QueueSendOptions = SendAndWaitOpts;

pub struct ActorHandleStateless {
	remote_manager: RemoteManager,
	params: Option<JsonValue>,
	encoding_kind: EncodingKind,
	// Mutex (not RefCell) so the handle is `Sync` and `&handle` futures
	// remain `Send` — required to call `.action(...)` from within axum
	// middleware that needs `Send` futures.
	query: Mutex<ActorQuery>,
}

/// An actor handle whose query has already resolved to a concrete actor ID.
/// Raw requests through this handle never repeat Engine actor lookup.
#[derive(Clone)]
pub struct ResolvedActorHandle {
	remote_manager: RemoteManager,
	actor_id: String,
}

impl ResolvedActorHandle {
	pub fn actor_id(&self) -> &str {
		&self.actor_id
	}

	pub async fn fetch(
		&self,
		path: &str,
		method: Method,
		headers: HeaderMap,
		body: Option<Bytes>,
	) -> Result<Response> {
		let path = normalize_fetch_path(path);
		let target = GatewayTarget::Direct {
			actor_id: self.actor_id.clone(),
		};
		self.remote_manager
			.send_request(&target, &path, method, headers, body)
			.await
	}
}

impl ActorHandleStateless {
	pub fn new(
		remote_manager: RemoteManager,
		params: Option<JsonValue>,
		encoding_kind: EncodingKind,
		query: ActorQuery,
	) -> Self {
		Self {
			remote_manager,
			params,
			encoding_kind,
			query: Mutex::new(query),
		}
	}

	pub async fn action(&self, name: &str, args: Vec<JsonValue>) -> Result<JsonValue> {
		// Resolve gateway target (query targets are resolved by the gateway).
		let query = self.query.lock().expect("query lock poisoned").clone();
		let target = self.remote_manager.gateway_target(&query).await?;

		let body = codec::encode_http_action_request(self.encoding_kind, &args)?;

		let headers = self.protocol_headers()?;

		// Send request via gateway
		let path = format!("/action/{}", urlencoding::encode(name));
		let res = self
			.remote_manager
			.send_request(
				&target,
				&path,
				Method::POST,
				headers,
				Some(Bytes::from(body)),
			)
			.await?;

		if !res.status().is_success() {
			let status = res.status();
			let body = res.bytes().await?;
			if let Ok((group, code, message, metadata)) =
				codec::decode_http_error(self.encoding_kind, &body)
			{
				return Err(anyhow!(
					"action failed ({group}/{code}): {message}, metadata={metadata:?}"
				));
			}
			return Err(anyhow!("action failed: {status}"));
		}

		// Decode response
		let output = res.bytes().await?;
		codec::decode_http_action_response(self.encoding_kind, &output)
	}

	pub async fn send(&self, name: &str, body: impl Serialize, _opts: SendOpts) -> Result<()> {
		self.send_queue(name, &body, false, None).await.map(|_| ())
	}

	pub async fn send_and_wait(
		&self,
		name: &str,
		body: impl Serialize,
		opts: SendAndWaitOpts,
	) -> Result<QueueSendResult> {
		let result = self.send_queue(name, &body, true, opts.timeout).await?;
		result.ok_or_else(|| anyhow!("queue wait response missing"))
	}

	async fn send_queue<T: Serialize>(
		&self,
		name: &str,
		body: &T,
		wait: bool,
		timeout: Option<Duration>,
	) -> Result<Option<QueueSendResult>> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		let target = self.remote_manager.gateway_target(&query).await?;
		let timeout_ms =
			timeout.map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
		let request_body =
			codec::encode_http_queue_request(self.encoding_kind, name, body, wait, timeout_ms)?;

		let headers = self.protocol_headers()?;

		let path = format!("/queue/{}", urlencoding::encode(name));
		let res = self
			.remote_manager
			.send_request(
				&target,
				&path,
				Method::POST,
				headers,
				Some(Bytes::from(request_body)),
			)
			.await?;

		if !res.status().is_success() {
			let status = res.status();
			let body = res.bytes().await?;
			if let Ok((group, code, message, metadata)) =
				codec::decode_http_error(self.encoding_kind, &body)
			{
				return Err(anyhow!(
					"queue send failed ({group}/{code}): {message}, metadata={metadata:?}"
				));
			}
			return Err(anyhow!("queue send failed: {status}"));
		}

		let body = res.bytes().await?;
		let result = codec::decode_http_queue_response(self.encoding_kind, &body)?;
		Ok(wait.then_some(result))
	}

	pub async fn fetch(
		&self,
		path: &str,
		method: Method,
		headers: HeaderMap,
		body: Option<Bytes>,
	) -> Result<Response> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		let target = self.remote_manager.gateway_target(&query).await?;
		let path = normalize_fetch_path(path);
		self.remote_manager
			.send_request(&target, &path, method, headers, body)
			.await
	}

	/// Resolves this query to a concrete actor, preserving get-or-create
	/// behavior for creating handles.
	pub async fn resolve_handle(&self) -> Result<ResolvedActorHandle> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		let actor_id = self.remote_manager.resolve_actor_id(&query).await?;
		Ok(ResolvedActorHandle {
			remote_manager: self.remote_manager.clone(),
			actor_id,
		})
	}

	/// Resolves a non-creating actor query without collapsing absence into a
	/// stringly error. Get-or-create and create queries are rejected because
	/// their absence has different semantics.
	pub async fn resolve_optional(&self) -> Result<Option<ResolvedActorHandle>> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		let actor_id = match &query {
			ActorQuery::GetForId { get_for_id } => {
				self.remote_manager
					.get_for_id(&get_for_id.name, &get_for_id.actor_id)
					.await?
			}
			ActorQuery::GetForKey { get_for_key } => {
				self.remote_manager
					.get_with_key(&get_for_key.name, &get_for_key.key)
					.await?
			}
			ActorQuery::GetOrCreateForKey { .. } | ActorQuery::Create { .. } => {
				return Err(anyhow!(
					"resolve_optional is only valid for non-creating actor queries"
				));
			}
		};
		Ok(actor_id.map(|actor_id| ResolvedActorHandle {
			remote_manager: self.remote_manager.clone(),
			actor_id,
		}))
	}

	pub async fn web_socket(
		&self,
		path: &str,
		protocols: Option<Vec<String>>,
	) -> Result<RawWebSocket> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		let target = self.remote_manager.gateway_target(&query).await?;
		self.remote_manager
			.open_raw_websocket(&target, path, self.params.clone(), protocols)
			.await
	}

	pub fn gateway_url(&self) -> Result<String> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		self.remote_manager.gateway_url(&query)
	}

	pub fn get_gateway_url(&self) -> Result<String> {
		self.gateway_url()
	}

	pub async fn reload(&self) -> Result<()> {
		let query = self.query.lock().expect("query lock poisoned").clone();
		let target = self.remote_manager.gateway_target(&query).await?;
		let res = self
			.remote_manager
			.send_request(
				&target,
				"/dynamic/reload",
				Method::PUT,
				HeaderMap::new(),
				None,
			)
			.await?;
		if !res.status().is_success() {
			let status = res.status();
			let body = res.text().await.unwrap_or_default();
			return Err(anyhow!("reload failed with status {status}: {body}"));
		}
		Ok(())
	}

	pub async fn resolve(&self) -> Result<String> {
		let query = {
			let Ok(query) = self.query.lock() else {
				return Err(anyhow!("Failed to lock actor query"));
			};
			query.clone()
		};

		match query {
			ActorQuery::Create { .. } => Err(anyhow!("actor query cannot be create")),
			ActorQuery::GetForId { get_for_id } => Ok(get_for_id.actor_id.clone()),
			_ => {
				let actor_id = self.remote_manager.resolve_actor_id(&query).await?;

				// Get name from the original query
				let name = match &query {
					ActorQuery::GetForKey { get_for_key } => get_for_key.name.clone(),
					ActorQuery::GetOrCreateForKey {
						get_or_create_for_key,
					} => get_or_create_for_key.name.clone(),
					_ => return Err(anyhow!("unexpected query type")),
				};

				{
					let Ok(mut query_mut) = self.query.lock() else {
						return Err(anyhow!("Failed to lock actor query mutably"));
					};

					*query_mut = ActorQuery::GetForId {
						get_for_id: GetForIdRequest {
							name,
							actor_id: actor_id.clone(),
						},
					};
				}

				Ok(actor_id)
			}
		}
	}

	fn protocol_headers(&self) -> Result<HeaderMap> {
		let mut headers = HeaderMap::new();
		headers.insert(
			HEADER_ENCODING,
			HeaderValue::from_str(self.encoding_kind.as_str())?,
		);

		if let Some(params) = &self.params {
			headers.insert(
				HEADER_CONN_PARAMS,
				HeaderValue::from_str(&serde_json::to_string(params)?)?,
			);
		}

		Ok(headers)
	}
}

fn normalize_fetch_path(path: &str) -> String {
	let path = path.trim_start_matches('/');
	if path.is_empty() {
		"/request".to_string()
	} else {
		format!("/request/{path}")
	}
}

pub struct ActorHandle {
	handle: ActorHandleStateless,
	remote_manager: RemoteManager,
	params: Option<JsonValue>,
	query: ActorQuery,
	client_shutdown_tx: Arc<tokio::sync::broadcast::Sender<()>>,
	transport_kind: crate::TransportKind,
	encoding_kind: EncodingKind,
}

impl ActorHandle {
	pub fn new(
		remote_manager: RemoteManager,
		params: Option<JsonValue>,
		query: ActorQuery,
		client_shutdown_tx: Arc<tokio::sync::broadcast::Sender<()>>,
		transport_kind: TransportKind,
		encoding_kind: EncodingKind,
	) -> Self {
		let handle = ActorHandleStateless::new(
			remote_manager.clone(),
			params.clone(),
			encoding_kind,
			query.clone(),
		);

		Self {
			handle,
			remote_manager,
			params,
			query,
			client_shutdown_tx,
			transport_kind,
			encoding_kind,
		}
	}

	pub fn connect(&self) -> ActorConnection {
		let conn = ActorConnectionInner::new(
			self.remote_manager.clone(),
			self.query.clone(),
			self.transport_kind,
			self.encoding_kind,
			self.params.clone(),
		);

		let rx = self.client_shutdown_tx.subscribe();
		start_connection(&conn, rx);

		conn
	}
}

impl Deref for ActorHandle {
	type Target = ActorHandleStateless;

	fn deref(&self) -> &Self::Target {
		&self.handle
	}
}
