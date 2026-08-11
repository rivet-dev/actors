use std::fmt::Debug;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::MutexGuard;

use crate::ctx::WorkflowCtx;

#[async_trait]
pub trait Workflow {
	type Input: WorkflowInput;
	type Output: Serialize + DeserializeOwned + Debug + Send;

	const NAME: &'static str;
	const PRUNE_VARIANT: PruneVariant;

	async fn run(ctx: &mut WorkflowCtx, input: &Self::Input) -> Result<Self::Output>;
}

pub trait WorkflowInput: Serialize + DeserializeOwned + Debug + Send {
	type Workflow: Workflow;
}

/// Wrapper around a mutex guard of a raw json value. Allows manipulating the value in deserialized form while
// holding the lock.
pub struct StateGuard<'a, T: DeserializeOwned + Serialize> {
	guard: MutexGuard<'a, (Box<serde_json::value::RawValue>, bool)>,
	inner: T,
}

impl<'a, T: DeserializeOwned + Serialize> StateGuard<'a, T> {
	pub(crate) fn new(
		guard: MutexGuard<'a, (Box<serde_json::value::RawValue>, bool)>,
	) -> Result<Self> {
		let value = rivet_util::serde::json_from_str!(guard.0.get())?;

		Ok(Self {
			guard,
			inner: value,
		})
	}
}

impl<'a, T: DeserializeOwned + Serialize> std::ops::Deref for StateGuard<'a, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}

impl<'a, T: DeserializeOwned + Serialize> std::ops::DerefMut for StateGuard<'a, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.guard.1 = true;
		&mut self.inner
	}
}

impl<'a, T: DeserializeOwned + Serialize> Drop for StateGuard<'a, T> {
	fn drop(&mut self) {
		// TODO: Somehow don't panic when committing state back into mutex
		self.guard.0 = rivet_util::serde::json_to_raw_value!(&self.inner).expect("bad state");
	}
}

/// Determines how the data related to a workflow is handled after the workflow completes.
#[derive(Debug, Default, Clone, Copy, strum::FromRepr)]
pub enum PruneVariant {
	/// Delete all of the workflow's data after it completes and is pruned (state + history).
	#[default]
	All = 0,
	/// Delete just the workflow's history after it completes and is pruned (state persists).
	History = 1,
	/// Don't delete any workflow data after it completes.
	None = 2,
}
