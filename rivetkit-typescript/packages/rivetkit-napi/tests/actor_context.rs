use super::*;

mod moved_tests {
	use super::*;

	#[test]
	fn actor_generations_do_not_share_foreign_runtime_state() {
		let harness = rivetkit_core::testing::ActorContextHarness::new();
		let context = |generation| {
			harness.context_with_config_and_generation(
				"worker-generation-cache",
				"actor",
				Vec::new(),
				"local",
				rivetkit_core::ActorConfig::default(),
				generation,
			)
		};
		let first_core = context(1);
		let first = ActorContext::new(first_core.clone());
		let first_again = ActorContext::new(first_core);
		assert!(Arc::ptr_eq(&first.shared, &first_again.shared));
		first.set_end_reason(EndReason::Sleep);
		let abort = CoreCancellationToken::new();
		first.attach_napi_abort_token(abort.clone());

		let second = ActorContext::new(context(2));
		second.reset_runtime_shared_state();
		assert!(!Arc::ptr_eq(&first.shared, &second.shared));
		assert!(first.has_end_reason());
		assert!(!second.has_end_reason());
		assert!(first.shared.abort_token.lock().as_ref().is_some());
		assert!(second.shared.abort_token.lock().is_none());
	}

	#[test]
	fn reset_runtime_state_clears_end_reason_without_touching_core_lifecycle_flags() {
		let shared = ActorContextShared::default();
		let ctx = rivetkit_core::testing::actor_context("actor-test", "actor", Vec::new(), "local");

		ctx.set_started(true);
		shared.set_end_reason(EndReason::Sleep);
		assert!(shared.has_end_reason());
		assert!(ctx.started());

		shared.reset_runtime_state();

		assert!(!shared.has_end_reason());
		assert!(ctx.started());
	}
}
