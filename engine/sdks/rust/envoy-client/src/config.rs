use std::{collections::HashMap, sync::Arc};

// Retain the established `config::*` API while keeping implementations in their domain modules.
pub use crate::{
	callbacks::{ActorStopHandle, BoxFuture, EnvoyCallbacks},
	http::{
		HTTP_BODY_MAX_CHUNK_SIZE, HTTP_BODY_STREAM_CHANNEL_CAPACITY, HttpRequest,
		HttpRequestBodyError, HttpRequestBodyStream, HttpResponse, HttpResponseBodyStream,
		ResponseChunk,
	},
	websocket::{WebSocketHandler, WebSocketMessage, WebSocketSender},
};

pub struct EnvoyConfig {
	pub version: u32,
	pub endpoint: String,
	pub token: Option<String>,
	pub namespace: String,
	pub pool_name: String,
	pub prepopulate_actor_names: HashMap<String, ActorName>,
	pub metadata: Option<serde_json::Value>,
	/// When `start_envoy` is called, create a new envoy every time instead of using a single global envoy
	/// instance for the entire runtime.
	pub not_global: bool,

	/// Debug option to inject artificial latency (in ms) into WebSocket communication.
	pub debug_latency_ms: Option<u64>,

	pub callbacks: Arc<dyn EnvoyCallbacks>,
}

pub struct ActorName {
	pub metadata: serde_json::Value,
}
