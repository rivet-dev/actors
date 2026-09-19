use std::collections::HashMap;
use std::future;
use std::sync::Arc;

use rivetkit_core::registry::worker_pool::{
	ActorWorkerPool, ActorWorkerPoolCallbacks, ActorWorkerPoolConfig, WorkerSpawnRequest,
};
use rivetkit_core::{ActorConfig, ActorFactory};
use tokio::sync::mpsc;

fn pool() -> (
	Arc<ActorWorkerPool>,
	mpsc::UnboundedReceiver<WorkerSpawnRequest>,
) {
	let (tx, rx) = mpsc::unbounded_channel();
	let pool = ActorWorkerPool::new(
		ActorWorkerPoolConfig::new(2, 1).unwrap(),
		[(
			"counter".to_owned(),
			ActorConfig::default().worker_pool_fingerprint(),
		)],
		ActorWorkerPoolCallbacks::new(
			move |requests| {
				for request in requests {
					tx.send(request)?;
				}
				Ok(())
			},
			|_, _| Ok(()),
		),
	);
	(pool, rx)
}

fn factories() -> HashMap<String, Arc<ActorFactory>> {
	HashMap::from([(
		"counter".to_owned(),
		Arc::new(ActorFactory::new(ActorConfig::default(), |_| {
			Box::pin(future::pending())
		})),
	)])
}

#[tokio::test]
async fn registration_before_timeout_denies_termination_and_preserves_capacity() {
	let (pool, mut requests) = pool();
	let acquire = tokio::spawn({
		let pool = pool.clone();
		async move { pool.acquire("first", 1, "counter").await }
	});
	let request = requests.recv().await.unwrap();
	let registration = pool
		.register_worker(
			request.worker_id,
			&request.spawn_token,
			request.class,
			factories(),
		)
		.unwrap();
	let first = acquire.await.unwrap().unwrap();

	assert!(!pool.fail_worker_spawn(
		request.worker_id,
		&request.spawn_token,
		"late ready".to_owned()
	));
	assert!(!first.worker_lost().is_cancelled());
	let second = pool.acquire("second", 1, "counter").await.unwrap();
	assert_eq!(second.worker_id(), first.worker_id());
	assert!(requests.try_recv().is_err());

	drop((first, second));
	pool.shutdown();
	registration.detach();
}

#[tokio::test]
async fn timeout_before_registration_authorizes_termination_and_rejects_late_attach() {
	let (pool, mut requests) = pool();
	let acquire = tokio::spawn({
		let pool = pool.clone();
		async move { pool.acquire("first", 1, "counter").await }
	});
	let request = requests.recv().await.unwrap();
	assert!(!pool.fail_worker_spawn(request.worker_id, "stale-token", "stale".to_owned()));
	assert!(pool.fail_worker_spawn(
		request.worker_id,
		&request.spawn_token,
		"timeout".to_owned()
	));
	assert!(!pool.fail_worker_spawn(
		request.worker_id,
		&request.spawn_token,
		"duplicate".to_owned()
	));
	assert!(
		pool.register_worker(
			request.worker_id,
			&request.spawn_token,
			request.class,
			factories()
		)
		.is_err()
	);
	assert!(acquire.await.unwrap().is_err());
	pool.shutdown();
}
