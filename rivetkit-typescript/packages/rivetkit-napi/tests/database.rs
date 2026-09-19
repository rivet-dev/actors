use super::wait_for_runtime;

#[test]
fn synchronous_wait_uses_the_active_multithreaded_runtime() {
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.unwrap();
	let _guard = runtime.enter();
	let result = wait_for_runtime(async {
		tokio::task::yield_now().await;
		Ok::<_, anyhow::Error>(42)
	})
	.unwrap();
	assert_eq!(result, 42);
}

#[test]
fn synchronous_wait_rejects_current_thread_runtime_without_panicking() {
	let runtime = tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.unwrap();
	let _guard = runtime.enter();
	assert!(wait_for_runtime(async { Ok::<_, anyhow::Error>(42) }).is_err());
}

#[test]
fn synchronous_wait_rejects_missing_runtime() {
	assert!(wait_for_runtime(async { Ok::<_, anyhow::Error>(42) }).is_err());
}
