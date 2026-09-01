pub mod generated;
pub mod telemetry_headers;
pub mod versioned;

// Re-export latest.
pub use generated::v4::*;

pub const PROTOCOL_VERSION: u16 = 4;
