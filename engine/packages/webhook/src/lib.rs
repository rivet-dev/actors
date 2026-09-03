use gas::prelude::*;

pub mod errors;
pub mod keys;
pub mod ops;
pub mod types;
pub mod workflows;

pub fn registry() -> WorkflowResult<Registry> {
	use workflows::*;

	let mut registry = Registry::new();
	registry.register_workflow::<webhook::Workflow>()?;

	Ok(registry)
}
