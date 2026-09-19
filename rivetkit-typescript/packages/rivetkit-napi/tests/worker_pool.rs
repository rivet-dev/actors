use super::*;
use rivetkit_core::registry::worker_pool::ActorWorkerPoolConfig;

fn test_pool() -> Arc<ActorWorkerPool> {
	ActorWorkerPool::new(
		ActorWorkerPoolConfig::new(1, 1).expect("valid test config"),
		[],
		ActorWorkerPoolCallbacks::new(|_| Ok(()), |_, _| Ok(())),
	)
}

#[test]
fn directory_shares_and_unregisters_pool() {
	let pool = test_pool();
	let pool_id = uuid::Uuid::new_v4().to_string();
	let host = WorkerPoolHost::new(pool_id.clone(), pool.clone()).expect("register pool");
	let found = lookup_pool(&pool_id).expect("find pool");
	assert!(Arc::ptr_eq(&pool, &found));

	host.unregister_directory();
	assert!(lookup_pool(&pool_id).is_err());
}

#[test]
fn worker_class_parser_rejects_unknown_values() {
	assert_eq!(
		parse_worker_class("baseline").unwrap(),
		WorkerClass::Baseline
	);
	assert_eq!(
		parse_worker_class("overflow").unwrap(),
		WorkerClass::Overflow
	);
	assert!(parse_worker_class("other").is_err());
}

#[test]
fn numeric_boundaries_require_safe_integers() {
	assert_eq!(parse_worker_id(42.0).unwrap(), 42);
	assert!(parse_worker_id(-1.0).is_err());
	assert!(parse_worker_id(1.5).is_err());
	assert!(parse_worker_id(f64::NAN).is_err());
	assert!(parse_worker_id(JAVASCRIPT_MAX_SAFE_INTEGER as f64 + 1.0).is_err());
	assert!(parse_positive_usize(0.0, "actorsPerThread").is_err());
}
