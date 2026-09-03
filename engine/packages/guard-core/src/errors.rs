use rivet_error::*;
use serde::{Deserialize, Serialize};

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"invalid_request_body",
	"Unable to parse request body.",
	"Unable to parse request body: {reason}."
)]
pub struct InvalidRequestBody {
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"invalid_response_body",
	"Unable to parse response body.",
	"Unable to parse response body: {reason}."
)]
pub struct InvalidResponseBody {
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"rate_limit",
	"Too many requests. Try again later.",
	"Too many requests to '{method} {path}' from IP {ip}."
)]
pub struct RateLimit {
	pub method: String,
	pub path: String,
	pub ip: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"http_request_build_failed",
	"Failed to build HTTP request.",
	"Failed to build HTTP request: {0}."
)]
pub struct HttpRequestBuildFailed(pub String);

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"uri_parse_error",
	"URI parse error.",
	"URI parse error: {0}."
)]
pub struct UriParseError(pub String);

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"request_build_error",
	"Request build error.",
	"Request build error: {0}."
)]
pub struct RequestBuildError(pub String);

#[derive(RivetError)]
#[error("guard", "upstream_error", "Upstream error.", "Upstream error: {0}.")]
pub struct UpstreamError(pub String);

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"request_timeout",
	"Request timed out.",
	"Request timed out during {phase} after {timeout_seconds} seconds."
)]
pub struct RequestTimeout {
	pub phase: String,
	pub timeout_seconds: u64,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"no_route_targets",
	"No targets found.",
	"No route targets found for {hostname}{path}."
)]
pub struct NoRouteTargets {
	pub hostname: String,
	pub path: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"retry_attempts_exceeded",
	"Retry attempts exceeded.",
	"All {attempts} retry attempts failed for {last_target_kind}. Last status: {last_status}. Last error: {last_error_code}."
)]
pub struct RetryAttemptsExceeded {
	pub attempts: u32,
	pub last_error_code: String,
	pub last_status: String,
	pub last_target_kind: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error("guard", "connection_error", "Connection error: {error_message}.")]
pub struct ConnectionError {
	pub error_message: String,
	pub remote_addr: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"actor_wake_retries_exceeded",
	"Actor wake retries exceeded.",
	"Actor {actor_id} stopped before becoming ready after {wake_retries} wake retries: {reason}."
)]
pub struct ActorWakeRetriesExceeded {
	pub actor_id: String,
	pub wake_retries: u32,
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"actor_stopped_while_waiting",
	"Actor stopped while waiting for a response.",
	"Actor {actor_id} stopped during {phase}."
)]
pub struct ActorStoppedWhileWaiting {
	pub actor_id: String,
	pub phase: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"tunnel_request_aborted",
	"Actor tunnel aborted the request.",
	"Actor tunnel aborted the request during {phase}."
)]
pub struct TunnelRequestAborted {
	pub phase: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"tunnel_message_timeout",
	"Actor tunnel message timed out.",
	"Actor tunnel message timed out during {phase}: {reason}."
)]
pub struct TunnelMessageTimeout {
	pub phase: String,
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"request_delivery_unconfirmed",
	"Actor did not respond to request start.",
	"Actor did not respond to request start during {phase}: {reason}."
)]
pub struct RequestDeliveryUnconfirmed {
	pub phase: String,
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"tunnel_response_closed",
	"Actor tunnel closed before sending a response.",
	"Actor tunnel closed before sending a response during {phase}."
)]
pub struct TunnelResponseClosed {
	pub phase: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"gateway_response_start_timeout",
	"Timed out waiting for actor response start.",
	"Timed out during {phase} after {timeout_ms} ms."
)]
pub struct GatewayResponseStartTimeout {
	pub phase: String,
	pub timeout_ms: u64,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_closed_before_open",
	"WebSocket closed before opening.",
	"WebSocket closed before opening with code {close_code}: {close_reason}."
)]
pub struct WebSocketClosedBeforeOpen {
	pub close_code: String,
	pub close_reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"actor_stopped_while_waiting_for_websocket_open",
	"Actor stopped while waiting for WebSocket open.",
	"Actor {actor_id} stopped while waiting for WebSocket open during {phase}."
)]
pub struct ActorStoppedWhileWaitingForWebSocketOpen {
	pub actor_id: String,
	pub phase: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_open_dropped",
	"WebSocket open was dropped.",
	"WebSocket open was dropped during {phase}: {reason}."
)]
pub struct WebSocketOpenDropped {
	pub phase: String,
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_open_response_closed",
	"WebSocket open response closed.",
	"WebSocket open response closed during {phase}."
)]
pub struct WebSocketOpenResponseClosed {
	pub phase: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_open_timeout",
	"Timed out waiting for WebSocket open.",
	"Timed out waiting for WebSocket open after {timeout_ms} ms."
)]
pub struct WebSocketOpenTimeout {
	pub timeout_ms: u64,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_tunnel_subscription_closed",
	"WebSocket tunnel subscription closed.",
	"WebSocket tunnel subscription closed during {phase}."
)]
pub struct WebSocketTunnelSubscriptionClosed {
	pub phase: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_service_hibernate",
	"Initiate WebSocket service hibernation."
)]
pub struct WebSocketServiceHibernate;

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_tunnel_ping_timeout",
	"WebSocket tunnel ping timed out.",
	"WebSocket tunnel ping timed out after {timeout_ms} ms. Last pong was {last_pong_age_ms} ms ago."
)]
pub struct WebSocketTunnelPingTimeout {
	pub timeout_ms: u64,
	pub last_pong_age_ms: u64,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"websocket_garbage_collected",
	"WebSocket request was garbage collected.",
	"WebSocket request was garbage collected during {phase}: {reason}."
)]
pub struct WebSocketGarbageCollected {
	pub phase: String,
	pub reason: String,
}

#[derive(RivetError, Serialize, Deserialize)]
#[error(
	"guard",
	"target_changed",
	"WebSocket target changed, retry not possible.",
	"WebSocket target changed during {phase} from {from_target_kind} to {to_target_kind}."
)]
pub struct WebSocketTargetChanged {
	pub phase: String,
	pub from_target_kind: String,
	pub to_target_kind: String,
}
