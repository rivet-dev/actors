use std::future;

use tokio::sync::mpsc;

use super::*;
use crate::ActorConfig;

const ACTOR_NAME: &str = "counter";

#[tokio::test]
async fn idle_timers_do_not_keep_a_closed_pool_and_host_callbacks_alive() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let (baseline, baseline_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "timer-baseline", 1).await;
	let (overflow, overflow_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "timer-overflow", 1).await;
	drop((baseline, overflow));
	pool.shutdown();
	baseline_registration.detach();
	overflow_registration.detach();
	let weak = Arc::downgrade(&pool);
	drop(pool);
	assert!(
		weak.upgrade().is_none(),
		"idle timers must not own the pool"
	);
}

fn actor_factories() -> HashMap<String, Arc<ActorFactory>> {
	HashMap::from([(
		ACTOR_NAME.to_owned(),
		Arc::new(ActorFactory::new(ActorConfig::default(), |_start| {
			Box::pin(future::pending())
		})),
	)])
}

fn test_pool(
	actors_per_thread: usize,
	baseline_worker_limit: usize,
	idle_retire_delay: Duration,
) -> (
	Arc<ActorWorkerPool>,
	mpsc::UnboundedReceiver<WorkerSpawnRequest>,
	mpsc::UnboundedReceiver<(WorkerId, WorkerRegistrationEpoch)>,
) {
	let (spawn_tx, spawn_rx) = mpsc::unbounded_channel();
	let (retire_tx, retire_rx) = mpsc::unbounded_channel();
	let config = ActorWorkerPoolConfig::new(actors_per_thread, baseline_worker_limit)
		.unwrap()
		.with_timeouts(Duration::from_secs(1), idle_retire_delay);
	let expected = [(
		ACTOR_NAME.to_owned(),
		ActorConfig::default().worker_pool_fingerprint(),
	)];
	let pool = ActorWorkerPool::new(
		config,
		expected,
		ActorWorkerPoolCallbacks::new(
			move |requests| {
				for request in requests {
					spawn_tx.send(request)?;
				}
				Ok(())
			},
			move |worker_id, epoch| {
				retire_tx.send((worker_id, epoch))?;
				Ok(())
			},
		),
	);
	(pool, spawn_rx, retire_rx)
}

async fn acquire_with_spawn(
	pool: &Arc<ActorWorkerPool>,
	spawn_rx: &mut mpsc::UnboundedReceiver<WorkerSpawnRequest>,
	actor_id: &str,
	generation: u32,
) -> (ActorFactoryLease, WorkerRegistrationHandle) {
	let acquire = tokio::spawn({
		let pool = pool.clone();
		let actor_id = actor_id.to_owned();
		async move { pool.acquire(&actor_id, generation, ACTOR_NAME).await }
	});
	let request = spawn_rx.recv().await.expect("spawn request");
	let registration = pool
		.register_worker(
			request.worker_id,
			&request.spawn_token,
			request.class,
			actor_factories(),
		)
		.expect("register worker");
	let lease = acquire.await.expect("join acquire").expect("acquire");
	(lease, registration)
}

#[tokio::test]
async fn spreads_baseline_then_bin_packs_overflow() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(2, 2, Duration::from_secs(60));
	let (first, _first_registration) = acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	let (second, _second_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-2", 1).await;
	assert_ne!(first.worker_id(), second.worker_id());

	let third = pool.acquire("actor-3", 1, ACTOR_NAME).await.unwrap();
	let fourth = pool.acquire("actor-4", 1, ACTOR_NAME).await.unwrap();
	assert_eq!(third.worker_id(), first.worker_id());
	assert_eq!(fourth.worker_id(), second.worker_id());

	let (fifth, _overflow_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-5", 1).await;
	assert_ne!(fifth.worker_id(), first.worker_id());
	assert_ne!(fifth.worker_id(), second.worker_id());
}

#[tokio::test]
async fn actors_per_thread_is_a_hard_limit() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let (first, _first_registration) = acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	let (second, _second_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-2", 1).await;
	assert_ne!(first.worker_id(), second.worker_id());
}

#[tokio::test]
async fn concurrent_acquires_spawn_only_required_capacity() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(2, 2, Duration::from_secs(60));
	let acquires = (0..5)
		.map(|index| {
			let pool = pool.clone();
			tokio::spawn(
				async move { pool.acquire(&format!("actor-{index}"), 1, ACTOR_NAME).await },
			)
		})
		.collect::<Vec<_>>();
	let mut registrations = Vec::new();
	let mut requests = Vec::new();
	for _ in 0..3 {
		let request = spawn_rx.recv().await.expect("spawn request");
		registrations.push(
			pool.register_worker(
				request.worker_id,
				&request.spawn_token,
				request.class,
				actor_factories(),
			)
			.unwrap(),
		);
		requests.push(request);
	}
	assert!(spawn_rx.try_recv().is_err());
	assert_eq!(
		requests
			.iter()
			.filter(|request| request.class == WorkerClass::Baseline)
			.count(),
		2,
	);
	assert_eq!(
		requests
			.iter()
			.filter(|request| request.class == WorkerClass::Overflow)
			.count(),
		1,
	);

	let mut occupancy = BTreeMap::new();
	for acquire in acquires {
		let lease = acquire.await.unwrap().unwrap();
		*occupancy.entry(lease.worker_id()).or_insert(0) += 1;
	}
	assert_eq!(occupancy.values().sum::<usize>(), 5);
	assert!(occupancy.values().all(|count| *count <= 2));
	drop(registrations);
}

#[tokio::test]
async fn invalid_registration_does_not_consume_spawn_token() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let acquire = tokio::spawn({
		let pool = pool.clone();
		async move { pool.acquire("actor-1", 1, ACTOR_NAME).await }
	});
	let request = spawn_rx.recv().await.unwrap();
	assert!(
		pool.register_worker(
			request.worker_id,
			"wrong-token",
			request.class,
			actor_factories(),
		)
		.is_err()
	);
	pool.register_worker(
		request.worker_id,
		&request.spawn_token,
		request.class,
		actor_factories(),
	)
	.unwrap();
	assert!(acquire.await.unwrap().is_ok());
}

#[tokio::test]
async fn timed_out_acquire_keeps_late_worker_for_next_actor() {
	let (spawn_tx, mut spawn_rx) = mpsc::unbounded_channel();
	let config = ActorWorkerPoolConfig::new(1, 1)
		.unwrap()
		.with_timeouts(Duration::from_millis(10), Duration::from_secs(60));
	let pool = ActorWorkerPool::new(
		config,
		[(
			ACTOR_NAME.to_owned(),
			ActorConfig::default().worker_pool_fingerprint(),
		)],
		ActorWorkerPoolCallbacks::new(
			move |requests| {
				for request in requests {
					spawn_tx.send(request)?;
				}
				Ok(())
			},
			|_, _| Ok(()),
		),
	);
	let acquire = tokio::spawn({
		let pool = pool.clone();
		async move { pool.acquire("actor-1", 1, ACTOR_NAME).await }
	});
	let request = spawn_rx.recv().await.unwrap();
	assert!(acquire.await.unwrap().is_err());
	let _registration = pool
		.register_worker(
			request.worker_id,
			&request.spawn_token,
			request.class,
			actor_factories(),
		)
		.unwrap();
	let lease = pool.acquire("actor-2", 1, ACTOR_NAME).await.unwrap();
	assert_eq!(lease.worker_id(), request.worker_id);
}

#[tokio::test]
async fn losing_worker_cancels_existing_leases() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let (lease, registration) = acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	assert!(!lease.worker_lost().is_cancelled());
	registration.environment_dropped();
	assert!(lease.worker_lost().is_cancelled());
}

#[tokio::test]
async fn lost_baseline_worker_is_replaced_without_new_demand() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let (_lease, registration) = acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	registration.environment_dropped();
	let replacement = spawn_rx.recv().await.expect("replacement spawn");
	assert_eq!(replacement.class, WorkerClass::Baseline);
	assert_ne!(replacement.worker_id, registration.worker_id());
}

#[tokio::test]
async fn shutdown_fails_queued_acquire() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let acquire = tokio::spawn({
		let pool = pool.clone();
		async move { pool.acquire("actor-1", 1, ACTOR_NAME).await }
	});
	let _pending = spawn_rx.recv().await.expect("spawn request");
	pool.shutdown();
	let error = match acquire.await.unwrap() {
		Ok(_) => panic!("acquire unexpectedly succeeded"),
		Err(error) => error,
	};
	assert!(error.to_string().contains("Worker pool is closed"));
}

#[tokio::test]
async fn one_spawn_failure_does_not_fail_every_waiter() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let acquires = (0..2)
		.map(|index| {
			let pool = pool.clone();
			tokio::spawn(
				async move { pool.acquire(&format!("actor-{index}"), 1, ACTOR_NAME).await },
			)
		})
		.collect::<Vec<_>>();
	let first = spawn_rx.recv().await.unwrap();
	let second = spawn_rx.recv().await.unwrap();
	pool.fail_worker_spawn(first.worker_id, &first.spawn_token, "boom".to_owned());
	let _second_registration = pool
		.register_worker(
			second.worker_id,
			&second.spawn_token,
			second.class,
			actor_factories(),
		)
		.unwrap();
	let replacement = spawn_rx.recv().await.unwrap();
	let _replacement_registration = pool
		.register_worker(
			replacement.worker_id,
			&replacement.spawn_token,
			replacement.class,
			actor_factories(),
		)
		.unwrap();

	let mut results = Vec::new();
	for acquire in acquires {
		results.push(acquire.await.unwrap());
	}
	assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
	assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
}

#[tokio::test]
async fn empty_overflow_worker_retires_after_idle_delay() {
	let (pool, mut spawn_rx, mut retire_rx) = test_pool(1, 1, Duration::from_millis(10));
	let (_baseline, _baseline_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	let (overflow, overflow_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-2", 1).await;
	let expected = (overflow.worker_id(), overflow_registration.worker_epoch());
	overflow.release();
	assert_eq!(retire_rx.recv().await, Some(expected));
}

#[tokio::test]
async fn baseline_worker_never_retires_from_idleness() {
	let (pool, mut spawn_rx, mut retire_rx) = test_pool(1, 1, Duration::from_millis(10));
	let (baseline, _registration) = acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	baseline.release();
	assert!(
		tokio::time::timeout(Duration::from_millis(30), retire_rx.recv())
			.await
			.is_err(),
	);
}

#[tokio::test]
async fn stale_generation_release_cannot_free_current_generation() {
	let (pool, mut spawn_rx, _retire_rx) = test_pool(1, 1, Duration::from_secs(60));
	let (first, registration) = acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	let worker_id = first.worker_id();
	first.release();
	let second = pool.acquire("actor-1", 2, ACTOR_NAME).await.unwrap();
	assert_eq!(second.worker_id(), worker_id);
	pool.release_assignment(
		&ActorGenerationKey {
			actor_id: "actor-1".to_owned(),
			generation: 1,
		},
		worker_id,
		registration.worker_epoch(),
	);
	assert!(
		pool.state
			.lock()
			.assignment_owners
			.contains_key(&ActorGenerationKey {
				actor_id: "actor-1".to_owned(),
				generation: 2,
			}),
	);
}

#[tokio::test]
async fn stale_retirement_timer_cannot_drain_reused_worker() {
	let (pool, mut spawn_rx, mut retire_rx) = test_pool(1, 1, Duration::from_millis(10));
	let (_baseline, _baseline_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	let (overflow, overflow_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-2", 1).await;
	let overflow_id = overflow.worker_id();
	overflow.release();
	let reused = pool.acquire("actor-3", 1, ACTOR_NAME).await.unwrap();
	assert_eq!(reused.worker_id(), overflow_id);
	assert!(
		tokio::time::timeout(Duration::from_millis(30), retire_rx.recv())
			.await
			.is_err(),
	);
	drop(overflow_registration);
}

#[tokio::test]
async fn late_empty_overflow_worker_still_retires() {
	let (spawn_tx, mut spawn_rx) = mpsc::unbounded_channel();
	let (retire_tx, mut retire_rx) = mpsc::unbounded_channel();
	let config = ActorWorkerPoolConfig::new(1, 1)
		.unwrap()
		.with_timeouts(Duration::from_millis(10), Duration::from_millis(10));
	let pool = ActorWorkerPool::new(
		config,
		[(
			ACTOR_NAME.to_owned(),
			ActorConfig::default().worker_pool_fingerprint(),
		)],
		ActorWorkerPoolCallbacks::new(
			move |requests| {
				for request in requests {
					spawn_tx.send(request)?;
				}
				Ok(())
			},
			move |worker_id, epoch| {
				retire_tx.send((worker_id, epoch))?;
				Ok(())
			},
		),
	);
	let (_baseline, _baseline_registration) =
		acquire_with_spawn(&pool, &mut spawn_rx, "actor-1", 1).await;
	let acquire = tokio::spawn({
		let pool = pool.clone();
		async move { pool.acquire("actor-2", 1, ACTOR_NAME).await }
	});
	let request = spawn_rx.recv().await.unwrap();
	assert_eq!(request.class, WorkerClass::Overflow);
	assert!(acquire.await.unwrap().is_err());
	let registration = pool
		.register_worker(
			request.worker_id,
			&request.spawn_token,
			request.class,
			actor_factories(),
		)
		.unwrap();
	assert_eq!(
		tokio::time::timeout(Duration::from_secs(1), retire_rx.recv())
			.await
			.expect("late overflow worker should retire"),
		Some((request.worker_id, registration.worker_epoch())),
	);
}
