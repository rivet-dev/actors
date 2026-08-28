//! Destination policy for outbound HTTP requests to user-configured URLs.
//!
//! Serverless runner URLs are supplied by whoever can write a runner config, and the engine dials
//! them from inside its own trusted network. This crate is the trust boundary for those requests:
//! it decides which destinations are reachable, and supplies the resolver and redirect policy that
//! enforce that decision at connect time.
//!
//! Clients are built in `rivet_pools::reqwest`, which owns every `reqwest::Client` in the process.

mod client;
mod policy;

pub use client::{GuardedResolver, block_reason, redirect_policy};
pub use policy::{AddressClass, BlockReason, Policy};
