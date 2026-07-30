use std::fmt;

use anyhow::Result;

use crate::ActorConfig;
use crate::actor::lifecycle_hooks::ActorStart;
use crate::runtime::RuntimeBoxFuture;

#[cfg(feature = "wasm-runtime")]
pub type ActorEntryFn = dyn Fn(ActorStart) -> RuntimeBoxFuture<Result<()>>;

#[cfg(not(feature = "wasm-runtime"))]
pub type ActorEntryFn = dyn Fn(ActorStart) -> RuntimeBoxFuture<Result<()>> + Send + Sync;

/// Runtime extension point for building actor receive loops.
pub struct ActorFactory {
	config: ActorConfig,
	entry: Box<ActorEntryFn>,
	manual_startup_ready: bool,
	streams_request_body: bool,
}

#[cfg(feature = "wasm-runtime")]
unsafe impl Send for ActorFactory {}

#[cfg(feature = "wasm-runtime")]
unsafe impl Sync for ActorFactory {}

impl ActorFactory {
	pub fn new<F>(config: ActorConfig, entry: F) -> Self
	where
		F: ActorEntry,
	{
		Self {
			config,
			entry: Box::new(entry),
			manual_startup_ready: false,
			streams_request_body: false,
		}
	}

	/// Builds a factory whose runtime will explicitly signal `startup_ready`
	/// after its own startup preamble finishes.
	pub fn new_with_manual_startup_ready<F>(config: ActorConfig, entry: F) -> Self
	where
		F: ActorEntry,
	{
		Self {
			config,
			entry: Box::new(entry),
			manual_startup_ready: true,
			streams_request_body: false,
		}
	}

	/// Declares that this runtime consumes request bodies incrementally.
	pub fn with_streaming_request_body(mut self) -> Self {
		self.streams_request_body = true;
		self
	}

	pub fn config(&self) -> &ActorConfig {
		&self.config
	}

	pub(crate) fn requires_manual_startup_ready(&self) -> bool {
		self.manual_startup_ready
	}

	pub(crate) fn streams_request_body(&self) -> bool {
		self.streams_request_body
	}

	pub async fn start(&self, start: ActorStart) -> Result<()> {
		(self.entry)(start).await
	}
}

#[cfg(feature = "wasm-runtime")]
pub trait ActorEntry: Fn(ActorStart) -> RuntimeBoxFuture<Result<()>> + 'static {}

#[cfg(feature = "wasm-runtime")]
impl<F> ActorEntry for F where F: Fn(ActorStart) -> RuntimeBoxFuture<Result<()>> + 'static {}

#[cfg(not(feature = "wasm-runtime"))]
pub trait ActorEntry:
	Fn(ActorStart) -> RuntimeBoxFuture<Result<()>> + Send + Sync + 'static
{
}

#[cfg(not(feature = "wasm-runtime"))]
impl<F> ActorEntry for F where
	F: Fn(ActorStart) -> RuntimeBoxFuture<Result<()>> + Send + Sync + 'static
{
}

impl fmt::Debug for ActorFactory {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ActorFactory")
			.field("config", &self.config)
			.field("manual_startup_ready", &self.manual_startup_ready)
			.field("streams_request_body", &self.streams_request_body)
			.field("entry", &"<boxed entry>")
			.finish()
	}
}
