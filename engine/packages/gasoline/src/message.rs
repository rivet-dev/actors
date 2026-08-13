use std::fmt::Debug;

use rivet_util::Id;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::{WorkflowError, WorkflowResult};

pub trait Message: Debug + Send + Sync + Serialize + DeserializeOwned + 'static {
	const NAME: &'static str;
	const TAIL_TTL: std::time::Duration;

	fn subject() -> String {
		format!("gasoline.msg.{}", Self::NAME)
	}
}

/// A message received from a pubsub subscription.
#[derive(Debug)]
pub struct PubsubMessage<M>
where
	M: Message,
{
	pub(crate) ray_id: Id,
	pub(crate) req_id: Id,
	pub(crate) ts: i64,
	pub(crate) body: M,
}

impl<M> PubsubMessage<M>
where
	M: Message,
{
	#[tracing::instrument(skip_all)]
	pub(crate) fn deserialize_from_wrapper(
		wrapper: PubsubMessageWrapper<'_>,
	) -> WorkflowResult<Self> {
		// Deserialize the body
		let body = rivet_util::serde::json_from_str!(wrapper.body.get())
			.map_err(WorkflowError::DeserializeMessageBody)?;

		Ok(PubsubMessage {
			ray_id: wrapper.ray_id,
			req_id: wrapper.req_id,
			ts: wrapper.ts,
			body,
		})
	}

	// Only returns the message wrapper
	#[tracing::instrument(skip_all)]
	pub(crate) fn deserialize_wrapper<'a>(
		buf: &'a [u8],
	) -> WorkflowResult<PubsubMessageWrapper<'a>> {
		rivet_util::serde::json_from_slice!(buf).map_err(WorkflowError::DeserializeMessage)
	}
}

impl<M> std::ops::Deref for PubsubMessage<M>
where
	M: Message,
{
	type Target = M;

	fn deref(&self) -> &Self::Target {
		&self.body
	}
}

impl<M> PubsubMessage<M>
where
	M: Message,
{
	pub fn ray_id(&self) -> Id {
		self.ray_id
	}

	pub fn req_id(&self) -> Id {
		self.req_id
	}

	/// Timestamp at which the message was created.
	pub fn msg_ts(&self) -> i64 {
		self.ts
	}

	pub fn body(&self) -> &M {
		&self.body
	}

	pub fn into_body(self) -> M {
		self.body
	}
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PubsubMessageWrapper<'a> {
	pub(crate) ray_id: Id,
	pub(crate) req_id: Id,
	pub(crate) topic: String,
	pub(crate) ts: i64,
	#[serde(borrow)]
	pub(crate) body: &'a serde_json::value::RawValue,
}
