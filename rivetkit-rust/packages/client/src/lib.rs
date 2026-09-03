//! Rust client for RivetKit actors.
//!
//! See `docs-internal/engine/rivetkit-rust-client.md` for actor-to-actor
//! client usage and idiomatic cancellation patterns with `tokio::select!`,
//! dropped futures, websocket handle drop, and optional
//! `tokio_util::sync::CancellationToken` threading.

mod backoff;
pub mod client;
mod common;
pub mod connection;
pub mod drivers;
pub mod handle;
pub mod protocol;
mod remote_manager;

pub use client::{
	Client, ClientConfig, CreateOptions, GetOptions, GetOrCreateOptions, GetWithIdOptions,
};
pub use common::{EncodingKind, RawWebSocket, TransportKind};
pub use connection::{ConnectionStatus, Event, SubscriptionHandle};
pub use handle::{
	QueueSendOptions, QueueSendResult, QueueSendStatus, ResolvedActorHandle, SendAndWaitOpts,
	SendOpts,
};
