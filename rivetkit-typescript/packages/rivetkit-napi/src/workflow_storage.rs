use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use rivetkit_core::{ActorContext as CoreActorContext, WorkflowKvWrite};

use crate::actor_context::WorkflowKvWritePayload;
use crate::napi_anyhow_error;

#[napi(object)]
pub struct WorkflowStorageEntry {
	pub key: Buffer,
	pub value: Buffer,
}

/// N-API binding for RivetKit's closed workflow-storage capability. It exposes
/// byte operations only; physical table and namespace details stay in core.
#[napi]
pub struct WorkflowStorage {
	inner: CoreActorContext,
}

impl WorkflowStorage {
	pub(crate) fn new(inner: CoreActorContext) -> Self {
		Self { inner }
	}
}

#[napi]
impl WorkflowStorage {
	#[napi]
	pub async fn get(&self, key: Buffer) -> napi::Result<Option<Buffer>> {
		self.inner
			.workflow_storage()
			.get(key.as_ref())
			.await
			.map(|value| value.map(Buffer::from))
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn set(&self, key: Buffer, value: Buffer) -> napi::Result<()> {
		self.inner
			.workflow_storage()
			.set(key.as_ref(), value.as_ref())
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn delete(&self, key: Buffer) -> napi::Result<()> {
		self.inner
			.workflow_storage()
			.delete(key.as_ref())
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn delete_prefix(&self, prefix: Buffer) -> napi::Result<()> {
		self.inner
			.workflow_storage()
			.delete_prefix(prefix.as_ref())
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn delete_range(&self, start: Buffer, end: Buffer) -> napi::Result<()> {
		self.inner
			.workflow_storage()
			.delete_range(start.as_ref(), end.as_ref())
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn list(&self, prefix: Buffer) -> napi::Result<Vec<WorkflowStorageEntry>> {
		self.inner
			.workflow_storage()
			.list(prefix.as_ref())
			.await
			.map(|entries| {
				entries
					.into_iter()
					.map(|(key, value)| WorkflowStorageEntry {
						key: Buffer::from(key),
						value: Buffer::from(value),
					})
					.collect()
			})
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn batch(&self, entries: Vec<WorkflowStorageEntry>) -> napi::Result<()> {
		let refs = entries
			.iter()
			.map(|entry| (entry.key.as_ref(), entry.value.as_ref()))
			.collect::<Vec<_>>();
		self.inner
			.workflow_storage()
			.batch(&refs)
			.await
			.map_err(napi_anyhow_error)
	}

	#[napi]
	pub async fn flush_with_state(&self, writes: Vec<WorkflowKvWritePayload>) -> napi::Result<()> {
		self.inner
			.workflow_storage()
			.flush_with_state(
				writes
					.into_iter()
					.map(|write| WorkflowKvWrite {
						key: write.key.to_vec(),
						value: write.value.to_vec(),
					})
					.collect(),
			)
			.await
			.map_err(napi_anyhow_error)
	}
}
