pub mod cert_resolver;
pub mod custom_serve;
pub mod errors;
pub mod metrics;
pub mod proxy_service;
pub mod request_context;
mod response_body;
mod route;
mod server;
mod task_group;
pub mod types;
pub mod utils;
pub mod websocket_handle;

pub use cert_resolver::CertResolverFn;
pub use custom_serve::CustomServeTrait;
pub use proxy_service::{ProxyService, ProxyState};
pub use response_body::{ResponseBody, ResponseBodyTerminal};
pub use route::{CacheKeyFn, RouteConfig, RouteTarget, RoutingFn, RoutingOutput};
pub use websocket_handle::WebSocketHandle;

// Re-export hyper StatusCode for use in other crates
pub mod status {
	pub use hyper::StatusCode;
}
pub use server::run_server;
pub use types::{EndpointType, GameGuardProtocol};
