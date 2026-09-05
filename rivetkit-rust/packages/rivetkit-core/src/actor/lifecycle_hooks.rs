use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use crate::actor::connection::ConnHandle;
use crate::actor::context::ActorContext;
use crate::actor::messages::ActorEvent;

pub struct Reply<T> {
	tx: Option<oneshot::Sender<Result<T>>>,
	invocation_telemetry: Option<crate::ActorInvocationTelemetry>,
}

impl<T> Reply<T> {
	#[doc(hidden)]
	pub fn with_invocation_telemetry(mut self, telemetry: crate::ActorInvocationTelemetry) -> Self {
		self.invocation_telemetry = Some(telemetry);
		self
	}

	#[doc(hidden)]
	pub fn invocation_telemetry(&self) -> Option<crate::ActorInvocationTelemetry> {
		self.invocation_telemetry.clone()
	}

	pub fn send(mut self, result: Result<T>) {
		if let Some(tx) = self.tx.take() {
			let _ = tx.send(result);
		}
	}
}

impl<T> Drop for Reply<T> {
	fn drop(&mut self) {
		if let Some(tx) = self.tx.take() {
			let _ = tx.send(Err(crate::error::ActorLifecycle::DroppedReply.build()));
		}
	}
}

impl<T> std::fmt::Debug for Reply<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Reply")
			.field("pending", &self.tx.is_some())
			.finish()
	}
}

impl<T> From<oneshot::Sender<Result<T>>> for Reply<T> {
	fn from(tx: oneshot::Sender<Result<T>>) -> Self {
		Self {
			tx: Some(tx),
			invocation_telemetry: None,
		}
	}
}

pub struct ActorEvents {
	actor_id: String,
	inner: mpsc::UnboundedReceiver<ActorEvent>,
}

impl ActorEvents {
	pub(crate) fn new(actor_id: String, inner: mpsc::UnboundedReceiver<ActorEvent>) -> Self {
		Self { actor_id, inner }
	}

	pub async fn recv(&mut self) -> Option<ActorEvent> {
		let event = self.inner.recv().await;
		if let Some(event) = &event {
			tracing::debug!(
				actor_id = %self.actor_id,
				event = event.kind(),
				"actor event drained"
			);
		}
		event
	}

	pub fn try_recv(&mut self) -> Option<ActorEvent> {
		let event = self.inner.try_recv().ok();
		if let Some(event) = &event {
			tracing::debug!(
				actor_id = %self.actor_id,
				event = event.kind(),
				"actor event drained"
			);
		}
		event
	}
}

impl std::fmt::Debug for ActorEvents {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("ActorEvents(..)")
	}
}

impl From<mpsc::UnboundedReceiver<ActorEvent>> for ActorEvents {
	fn from(value: mpsc::UnboundedReceiver<ActorEvent>) -> Self {
		Self::new("unknown".to_owned(), value)
	}
}

#[derive(Debug)]
pub struct ActorStart {
	pub ctx: ActorContext,
	pub input: Option<Vec<u8>>,
	pub is_new: bool,
	pub snapshot: Option<Vec<u8>>,
	pub hibernated: Vec<(ConnHandle, Vec<u8>)>,
	pub events: ActorEvents,
	pub startup_ready: Option<oneshot::Sender<Result<()>>>,
}
