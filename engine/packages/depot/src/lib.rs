pub mod burst_mode;
use gas::prelude::*;

mod compaction;
pub mod conveyer;
pub mod doctor;
#[cfg(feature = "test-faults")]
pub mod fault;
pub mod gc;
pub mod history_snapshot;
pub mod inspect;
pub mod metrics;
#[cfg(debug_assertions)]
pub mod takeover;
pub mod workflows;

pub use conveyer::constants::*;
#[cfg(debug_assertions)]
pub use conveyer::debug;
pub use conveyer::pitr_interval;
pub use conveyer::{constants, error, keys, ltx, page_index, policy, quota, types, udb};

pub fn registry() -> WorkflowResult<Registry> {
	let registry = Registry::new();
	// registry.register_workflow::<db_hot_compacter::DbHotCompacterWorkflow>()?;
	// registry.register_workflow::<db_manager::DbManagerWorkflow>()?;
	// registry.register_workflow::<db_reclaimer::DbReclaimerWorkflow>()?;

	Ok(registry)
}
