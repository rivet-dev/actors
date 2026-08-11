use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use futures_util::FutureExt;
use tracing::Instrument;

use crate::{
	ctx::WorkflowCtx,
	error::{WorkflowError, WorkflowResult},
	workflow::{PruneVariant, Workflow},
};

pub type RegistryHandle = Arc<Registry>;

/// Contains a lookup map for workflow run handlers by workflow name.
pub struct Registry {
	pub(crate) workflows: HashMap<String, Arc<RegistryWorkflow>>,
}

impl Default for Registry {
	fn default() -> Self {
		Self::new()
	}
}

impl Registry {
	pub fn new() -> Self {
		Registry {
			workflows: HashMap::new(),
		}
	}

	pub fn handle(self) -> RegistryHandle {
		Arc::new(self)
	}

	pub fn merge(mut self, registry: Registry) -> WorkflowResult<Registry> {
		// Check for duplicates
		for workflow_name in registry.workflows.keys() {
			if self.workflows.contains_key(workflow_name.as_str()) {
				return Err(WorkflowError::DuplicateRegisteredWorkflow(
					workflow_name.clone(),
				));
			}
		}

		self.workflows.extend(registry.workflows);

		Ok(self)
	}

	pub fn register_workflow<W: Workflow>(&mut self) -> WorkflowResult<()> {
		// Check for duplicates
		if self.workflows.contains_key(W::NAME) {
			return Err(WorkflowError::DuplicateRegisteredWorkflow(
				W::NAME.to_string(),
			));
		}

		self.workflows.insert(
			W::NAME.to_string(),
			Arc::new(RegistryWorkflow {
				run: |ctx| {
					async move {
						// Deserialize input
						let input = rivet_util::serde::json_from_str!(ctx.input().get())
							.map_err(WorkflowError::DeserializeWorkflowInput)?;

						// Run workflow
						let output = match W::run(ctx, &input).await {
							Ok(x) => x,
							// TODO: This should check .chain() for the error
							// Differentiate between WorkflowError and user error
							Err(err) => match err.downcast::<WorkflowError>() {
								Ok(inner_err) => return Err(inner_err),
								Err(err) => {
									return Err(WorkflowError::WorkflowFailure(W::NAME, err));
								}
							},
						};

						// Serialize output
						let output_val = rivet_util::serde::json_to_raw_value!(&output)
							.map_err(WorkflowError::SerializeWorkflowOutput)?;

						Ok(output_val)
					}
					.in_current_span()
					.boxed()
				},
				prune_variant: W::PRUNE_VARIANT,
			}),
		);

		Ok(())
	}

	pub fn get_workflow(&self, name: &str) -> WorkflowResult<&Arc<RegistryWorkflow>> {
		self.workflows
			.get(name)
			.ok_or(WorkflowError::WorkflowMissingFromRegistry(name.to_string()))
	}

	pub fn size(&self) -> usize {
		self.workflows.len()
	}

	pub fn names(&self) -> Vec<&str> {
		self.workflows.keys().map(|x| x.as_str()).collect()
	}
}

pub struct RegistryWorkflow {
	pub run: for<'a> fn(
		&'a mut WorkflowCtx,
	) -> Pin<
		Box<dyn Future<Output = WorkflowResult<Box<serde_json::value::RawValue>>> + Send + 'a>,
	>,
	pub prune_variant: PruneVariant,
}
