pub mod generated;
pub mod util;
pub mod versioned;

// Re-export latest
pub use generated::v7::*;

pub use generated::PROTOCOL_VERSION;

/// Initial unacknowledged body bytes allowed in either HTTP stream direction.
/// Receivers return credit with cumulative window-update messages as the body
/// is consumed by the next layer.
pub const HTTP_STREAM_INITIAL_WINDOW_BYTES: u64 = 1024 * 1024;
