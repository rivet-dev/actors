use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use crate::actor::connection::ConnHandle;
use crate::actor::context::ActorContext;
use crate::actor::messages::ActorEvent;

pub struct Reply<T> {
	tx: Option<oneshot::Sender<Result<T>>>,
	on_reply: Option<Box<dyn FnOnce(&Result<T>) + Send>>,
}

impl<T> Reply<T> {
	/// Runs `on_reply` with the result just before it is sent. A reply dropped
	/// unsent runs it with the dropped-reply error, so every outcome is seen.
	pub(crate) fn on_reply(mut self, on_reply: impl FnOnce(&Result<T>) + Send + 'static) -> Self {
		self.on_reply = Some(Box::new(on_reply));
		self
	}

	pub fn send(mut self, result: Result<T>) {
		self.deliver(result);
	}

	fn deliver(&mut self, result: Result<T>) {
		if let Some(on_reply) = self.on_reply.take() {
			on_reply(&result);
		}
		if let Some(tx) = self.tx.take() {
			let _ = tx.send(result);
		}
	}
}

impl<T> Drop for Reply<T> {
	fn drop(&mut self) {
		if self.tx.is_some() {
			self.deliver(Err(crate::error::ActorLifecycle::DroppedReply.build()));
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
			on_reply: None,
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
