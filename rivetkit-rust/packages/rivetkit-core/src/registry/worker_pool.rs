use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Weak};
use std::time::Duration;

use anyhow::Result;
use parking_lot::Mutex;
use rivet_error::RivetError;
use rivet_metrics::prometheus::{
	Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
};
use serde::Serialize;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::actor::factory::ActorFactory;
#[cfg(not(feature = "native-runtime"))]
use crate::runtime::RuntimeSpawner;
use crate::time::{Instant, sleep_until};

pub type WorkerId = u64;
pub type WorkerRegistrationEpoch = u64;

const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_IDLE_RETIRE_DELAY: Duration = Duration::from_secs(30);

struct WorkerPoolMetrics {
	workers: IntGaugeVec,
	leases: IntGaugeVec,
	available_slots: IntGaugeVec,
	queued_acquires: IntGauge,
	acquire_duration_seconds: Histogram,
	events: IntCounterVec,
	actors_failed_worker_loss: IntCounter,
}

static METRICS: LazyLock<WorkerPoolMetrics> = LazyLock::new(WorkerPoolMetrics::new);

impl WorkerPoolMetrics {
	fn new() -> Self {
		let workers = IntGaugeVec::new(
			Opts::new(
				"rivetkit_actor_worker_threads",
				"Node actor worker threads by class and lifecycle state",
			),
			&["class", "state"],
		)
		.expect("create worker thread gauge");
		let leases = IntGaugeVec::new(
			Opts::new(
				"rivetkit_actor_worker_leases",
				"Actor generations assigned to Node workers by class",
			),
			&["class"],
		)
		.expect("create worker lease gauge");
		let available_slots = IntGaugeVec::new(
			Opts::new(
				"rivetkit_actor_worker_available_slots",
				"Unreserved slots on ready Node workers by class",
			),
			&["class"],
		)
		.expect("create worker available-slot gauge");
		let queued_acquires = IntGauge::new(
			"rivetkit_actor_worker_queued_acquires",
			"Actor starts waiting to acquire a Node worker slot",
		)
		.expect("create worker queued-acquire gauge");
		let acquire_duration_seconds = Histogram::with_opts(
			HistogramOpts::new(
				"rivetkit_actor_worker_acquire_duration_seconds",
				"Time spent acquiring a Node worker slot",
			)
			.buckets(rivet_metrics::MICRO_BUCKETS.to_vec()),
		)
		.expect("create worker acquire duration histogram");
		let events = IntCounterVec::new(
			Opts::new(
				"rivetkit_actor_worker_events_total",
				"Node actor worker lifecycle events",
			),
			&["event", "class"],
		)
		.expect("create worker lifecycle counter");
		let actors_failed_worker_loss = IntCounter::new(
			"rivetkit_actor_worker_loss_actor_generations_total",
			"Actor generations failed because their Node worker exited",
		)
		.expect("create worker-loss actor counter");

		register_metric(&rivet_metrics::REGISTRY, workers.clone());
		register_metric(&rivet_metrics::REGISTRY, leases.clone());
		register_metric(&rivet_metrics::REGISTRY, available_slots.clone());
		register_metric(&rivet_metrics::REGISTRY, queued_acquires.clone());
		register_metric(&rivet_metrics::REGISTRY, acquire_duration_seconds.clone());
		register_metric(&rivet_metrics::REGISTRY, events.clone());
		register_metric(&rivet_metrics::REGISTRY, actors_failed_worker_loss.clone());

		Self {
			workers,
			leases,
			available_slots,
			queued_acquires,
			acquire_duration_seconds,
			events,
			actors_failed_worker_loss,
		}
	}
}

fn register_metric<M>(registry: &Registry, metric: M)
where
	M: rivet_metrics::prometheus::core::Collector + Clone + Send + Sync + 'static,
{
	if let Err(error) = registry.register(Box::new(metric)) {
		tracing::warn!(?error, "worker-pool metric registration failed");
	}
}

fn worker_class_label(class: WorkerClass) -> &'static str {
	match class {
		WorkerClass::Baseline => "baseline",
		WorkerClass::Overflow => "overflow",
	}
}

struct AcquireMetricGuard {
	started: Instant,
}

impl Drop for AcquireMetricGuard {
	fn drop(&mut self) {
		METRICS
			.acquire_duration_seconds
			.observe(self.started.elapsed().as_secs_f64());
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum WorkerClass {
	Baseline,
	Overflow,
}

#[derive(Clone, Debug)]
pub struct WorkerSpawnRequest {
	pub worker_id: WorkerId,
	pub spawn_token: String,
	pub class: WorkerClass,
}

#[derive(Clone)]
pub struct ActorWorkerPoolCallbacks {
	request_spawns: Arc<dyn Fn(Vec<WorkerSpawnRequest>) -> Result<()> + Send + Sync>,
	retire_worker: Arc<dyn Fn(WorkerId, WorkerRegistrationEpoch) -> Result<()> + Send + Sync>,
}

impl ActorWorkerPoolCallbacks {
	pub fn new(
		request_spawns: impl Fn(Vec<WorkerSpawnRequest>) -> Result<()> + Send + Sync + 'static,
		retire_worker: impl Fn(WorkerId, WorkerRegistrationEpoch) -> Result<()> + Send + Sync + 'static,
	) -> Self {
		Self {
			request_spawns: Arc::new(request_spawns),
			retire_worker: Arc::new(retire_worker),
		}
	}
}

#[derive(Clone, Debug)]
pub struct ActorWorkerPoolConfig {
	pub actors_per_thread: usize,
	pub baseline_worker_limit: usize,
	pub acquire_timeout: Duration,
	pub idle_retire_delay: Duration,
}

impl ActorWorkerPoolConfig {
	pub fn new(actors_per_thread: usize, baseline_worker_limit: usize) -> Result<Self> {
		if actors_per_thread == 0 {
			return Err(WorkerPoolInvalidConfig {
				reason: "actors_per_thread must be greater than zero".to_owned(),
			}
			.build());
		}
		if baseline_worker_limit == 0 {
			return Err(WorkerPoolInvalidConfig {
				reason: "baseline_worker_limit must be greater than zero".to_owned(),
			}
			.build());
		}
		Ok(Self {
			actors_per_thread,
			baseline_worker_limit,
			acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
			idle_retire_delay: DEFAULT_IDLE_RETIRE_DELAY,
		})
	}

	#[cfg(test)]
	pub(crate) fn with_timeouts(
		mut self,
		acquire_timeout: Duration,
		idle_retire_delay: Duration,
	) -> Self {
		self.acquire_timeout = acquire_timeout;
		self.idle_retire_delay = idle_retire_delay;
		self
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ActorGenerationKey {
	actor_id: String,
	generation: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerState {
	Ready,
	Draining,
}

struct WorkerRecord {
	id: WorkerId,
	epoch: WorkerRegistrationEpoch,
	class: WorkerClass,
	state: WorkerState,
	factories: Arc<HashMap<String, Arc<ActorFactory>>>,
	assignments: BTreeSet<ActorGenerationKey>,
	lost: CancellationToken,
	created_sequence: u64,
	last_selected_sequence: u64,
	retirement_epoch: u64,
}

impl WorkerRecord {
	fn baseline_key(&self) -> (usize, u64, WorkerId) {
		(self.assignments.len(), self.last_selected_sequence, self.id)
	}

	fn overflow_key(&self) -> (Reverse<usize>, u64, WorkerId) {
		(
			Reverse(self.assignments.len()),
			self.created_sequence,
			self.id,
		)
	}
}

struct PendingSpawn {
	request: WorkerSpawnRequest,
}

#[derive(Default)]
struct SchedulerState {
	workers: BTreeMap<WorkerId, WorkerRecord>,
	assignment_owners: BTreeMap<ActorGenerationKey, (WorkerId, WorkerRegistrationEpoch)>,
	baseline_available: BTreeSet<(usize, u64, WorkerId)>,
	overflow_available: BTreeSet<(Reverse<usize>, u64, WorkerId)>,
	pending_spawns: BTreeMap<WorkerId, PendingSpawn>,
	queued_acquires: usize,
	baseline_target: usize,
	baseline_workers: usize,
	pending_baseline: usize,
	pending_overflow: usize,
	ready_free_slots: usize,
	next_worker_id: WorkerId,
	next_sequence: u64,
	next_registration_epoch: WorkerRegistrationEpoch,
	spawn_failures: VecDeque<String>,
	shutting_down: bool,
}

pub struct ActorWorkerPool {
	config: ActorWorkerPoolConfig,
	expected_factories: BTreeMap<String, String>,
	callbacks: ActorWorkerPoolCallbacks,
	state: Mutex<SchedulerState>,
	changed: Notify,
	retirement_shutdown: CancellationToken,
}

pub struct ActorFactoryLease {
	pool: Weak<ActorWorkerPool>,
	actor: ActorGenerationKey,
	worker_id: WorkerId,
	worker_epoch: WorkerRegistrationEpoch,
	factory: Arc<ActorFactory>,
	worker_lost: CancellationToken,
	released: bool,
}

impl ActorFactoryLease {
	pub fn factory(&self) -> Arc<ActorFactory> {
		self.factory.clone()
	}

	pub fn worker_lost(&self) -> CancellationToken {
		self.worker_lost.clone()
	}

	pub fn worker_id(&self) -> WorkerId {
		self.worker_id
	}

	pub fn release(mut self) {
		self.release_inner();
	}

	fn release_inner(&mut self) {
		if self.released {
			return;
		}
		self.released = true;
		if let Some(pool) = self.pool.upgrade() {
			pool.release_assignment(&self.actor, self.worker_id, self.worker_epoch);
		}
	}
}

impl Drop for ActorFactoryLease {
	fn drop(&mut self) {
		self.release_inner();
	}
}

#[derive(Clone)]
pub struct WorkerRegistrationHandle {
	pool: Weak<ActorWorkerPool>,
	worker_id: WorkerId,
	worker_epoch: WorkerRegistrationEpoch,
}

impl WorkerRegistrationHandle {
	pub fn worker_id(&self) -> WorkerId {
		self.worker_id
	}

	pub fn worker_epoch(&self) -> WorkerRegistrationEpoch {
		self.worker_epoch
	}

	pub fn environment_dropped(&self) {
		if let Some(pool) = self.pool.upgrade() {
			pool.worker_lost(self.worker_id, self.worker_epoch);
		}
	}

	pub fn detach(&self) {
		if let Some(pool) = self.pool.upgrade() {
			pool.detach_worker(self.worker_id, self.worker_epoch);
		}
	}
}

struct AcquireWaiter {
	pool: Weak<ActorWorkerPool>,
	active: bool,
}

impl AcquireWaiter {
	fn finish_locked(&mut self, state: &mut SchedulerState) {
		if !self.active {
			return;
		}
		self.active = false;
		state.queued_acquires = state.queued_acquires.saturating_sub(1);
		METRICS.queued_acquires.dec();
		state.spawn_failures.truncate(state.queued_acquires);
	}

	fn finish(&mut self) {
		if !self.active {
			return;
		}
		if let Some(pool) = self.pool.upgrade() {
			let mut state = pool.state.lock();
			self.finish_locked(&mut state);
			drop(state);
			pool.changed.notify_waiters();
		} else {
			self.active = false;
		}
	}
}

impl Drop for AcquireWaiter {
	fn drop(&mut self) {
		self.finish();
	}
}

impl ActorWorkerPool {
	pub fn new(
		config: ActorWorkerPoolConfig,
		expected_factories: impl IntoIterator<Item = (String, String)>,
		callbacks: ActorWorkerPoolCallbacks,
	) -> Arc<Self> {
		Arc::new(Self {
			config,
			expected_factories: expected_factories.into_iter().collect(),
			callbacks,
			state: Mutex::new(SchedulerState {
				next_worker_id: 1,
				next_sequence: 1,
				next_registration_epoch: 1,
				..SchedulerState::default()
			}),
			changed: Notify::new(),
			retirement_shutdown: CancellationToken::new(),
		})
	}

	pub async fn acquire(
		self: &Arc<Self>,
		actor_id: &str,
		generation: u32,
		actor_name: &str,
	) -> Result<ActorFactoryLease> {
		let _metric_guard = AcquireMetricGuard {
			started: Instant::now(),
		};
		if !self.expected_factories.contains_key(actor_name) {
			return Err(WorkerPoolActorNotRegistered {
				actor_name: actor_name.to_owned(),
			}
			.build());
		}

		let mut waiter = {
			let mut state = self.state.lock();
			if state.shutting_down {
				return Err(WorkerPoolClosed.build());
			}
			state.queued_acquires += 1;
			METRICS.queued_acquires.inc();
			state.baseline_target = state.baseline_target.max(
				(state.assignment_owners.len() + state.queued_acquires)
					.min(self.config.baseline_worker_limit),
			);
			AcquireWaiter {
				pool: Arc::downgrade(self),
				active: true,
			}
		};
		let deadline = Instant::now() + self.config.acquire_timeout;
		let actor = ActorGenerationKey {
			actor_id: actor_id.to_owned(),
			generation,
		};

		loop {
			let notified = self.changed.notified();
			tokio::pin!(notified);
			notified.as_mut().enable();

			let (lease, spawn_requests, failure) = {
				let mut state = self.state.lock();
				if state.shutting_down {
					return Err(WorkerPoolClosed.build());
				}
				if state.assignment_owners.contains_key(&actor) {
					return Err(WorkerPoolDuplicateAssignment {
						actor_id: actor.actor_id.clone(),
						generation: actor.generation,
					}
					.build());
				}

				let desired_baseline = self.desired_baseline_workers(&state);
				let ready_baseline = state.baseline_workers;
				let should_wait_for_baseline = ready_baseline < desired_baseline;
				let selected = if should_wait_for_baseline {
					None
				} else {
					self.reserve_available_worker(&mut state, &actor, actor_name)
				};
				if let Some(lease) = selected {
					// Decrement the waiter in the same critical section as the
					// reservation. Spawn planning can never observe both the new
					// assignment and a stale queued-acquire count.
					waiter.finish_locked(&mut state);
					(Some(lease), Vec::new(), None)
				} else {
					let failure = state.spawn_failures.pop_front();
					let requests = if failure.is_none() {
						self.plan_spawns(&mut state)
					} else {
						Vec::new()
					};
					(None, requests, failure)
				}
			};

			if let Some(lease) = lease {
				return Ok(lease);
			}
			if let Some(reason) = failure {
				return Err(WorkerPoolSpawnFailed { reason }.build());
			}
			if !spawn_requests.is_empty()
				&& let Err(error) = (self.callbacks.request_spawns)(spawn_requests.clone())
			{
				self.fail_spawn_requests(&spawn_requests, format!("{error:#}"));
				continue;
			}

			tokio::select! {
				_ = notified => {}
				_ = sleep_until(deadline) => {
					return Err(WorkerPoolAcquireTimedOut {
						actor_id: actor.actor_id.clone(),
						generation: actor.generation,
					}.build());
				}
			}
		}
	}

	pub fn register_worker(
		self: &Arc<Self>,
		worker_id: WorkerId,
		spawn_token: &str,
		class: WorkerClass,
		factories: HashMap<String, Arc<ActorFactory>>,
	) -> Result<WorkerRegistrationHandle> {
		if let Err(error) = self.validate_factories(&factories) {
			METRICS
				.events
				.with_label_values(&["registration_rejected", worker_class_label(class)])
				.inc();
			return Err(error);
		}
		let mut state = self.state.lock();
		if state.shutting_down {
			return Err(WorkerPoolClosed.build());
		}
		let pending = state.pending_spawns.get(&worker_id).ok_or_else(|| {
			WorkerPoolRegistrationRejected {
				reason: format!("worker {worker_id} has no pending spawn"),
			}
			.build()
		})?;
		if pending.request.spawn_token != spawn_token || pending.request.class != class {
			return Err(WorkerPoolRegistrationRejected {
				reason: format!("worker {worker_id} spawn identity did not match"),
			}
			.build());
		}
		Self::remove_pending_spawn(&mut state, worker_id);
		if state.workers.contains_key(&worker_id) {
			return Err(WorkerPoolRegistrationRejected {
				reason: format!("worker {worker_id} is already registered"),
			}
			.build());
		}

		let epoch = state.next_registration_epoch;
		state.next_registration_epoch += 1;
		let created_sequence = state.next_sequence;
		state.next_sequence += 1;
		let record = WorkerRecord {
			id: worker_id,
			epoch,
			class,
			state: WorkerState::Ready,
			factories: Arc::new(factories),
			assignments: BTreeSet::new(),
			lost: CancellationToken::new(),
			created_sequence,
			last_selected_sequence: 0,
			retirement_epoch: 0,
		};
		self.insert_availability(&mut state, &record);
		state.workers.insert(worker_id, record);
		state.ready_free_slots += self.config.actors_per_thread;
		if class == WorkerClass::Baseline {
			state.baseline_workers += 1;
		}
		let should_arm_idle_retirement = class == WorkerClass::Overflow;
		let class_label = worker_class_label(class);
		METRICS
			.workers
			.with_label_values(&[class_label, "ready"])
			.inc();
		METRICS
			.available_slots
			.with_label_values(&[class_label])
			.add(self.config.actors_per_thread as i64);
		METRICS
			.events
			.with_label_values(&["ready", class_label])
			.inc();
		drop(state);
		self.changed.notify_waiters();
		if should_arm_idle_retirement {
			self.schedule_retirement(worker_id, epoch, 0);
		}
		Ok(WorkerRegistrationHandle {
			pool: Arc::downgrade(self),
			worker_id,
			worker_epoch: epoch,
		})
	}

	/// Atomically cancels a matching pending spawn. Only a successful cancellation
	/// authorizes the host to terminate a worker whose readiness message is late.
	pub fn fail_worker_spawn(
		&self,
		worker_id: WorkerId,
		spawn_token: &str,
		reason: String,
	) -> bool {
		let mut state = self.state.lock();
		let class = state
			.pending_spawns
			.get(&worker_id)
			.filter(|pending| pending.request.spawn_token == spawn_token)
			.map(|pending| pending.request.class);
		let Some(class) = class else {
			return false;
		};
		Self::remove_pending_spawn(&mut state, worker_id);
		METRICS
			.events
			.with_label_values(&["bootstrap_failure", worker_class_label(class)])
			.inc();
		if state.spawn_failures.len() < state.queued_acquires {
			state.spawn_failures.push_back(reason);
		}
		drop(state);
		self.changed.notify_waiters();
		true
	}

	pub fn worker_lost(&self, worker_id: WorkerId, epoch: WorkerRegistrationEpoch) {
		let (worker, spawn_requests) = {
			let mut state = self.state.lock();
			let matches = state
				.workers
				.get(&worker_id)
				.is_some_and(|worker| worker.epoch == epoch);
			if !matches {
				return;
			}
			let worker = state
				.workers
				.remove(&worker_id)
				.expect("worker checked above");
			self.remove_availability(&mut state, &worker);
			let class_label = worker_class_label(worker.class);
			let state_label = match worker.state {
				WorkerState::Ready => "ready",
				WorkerState::Draining => "draining",
			};
			METRICS
				.workers
				.with_label_values(&[class_label, state_label])
				.dec();
			METRICS
				.events
				.with_label_values(&[
					if worker.state == WorkerState::Ready {
						"unexpected_exit"
					} else {
						"retired"
					},
					class_label,
				])
				.inc();
			if !worker.assignments.is_empty() {
				METRICS
					.leases
					.with_label_values(&[class_label])
					.sub(worker.assignments.len() as i64);
				METRICS
					.actors_failed_worker_loss
					.inc_by(worker.assignments.len() as u64);
			}
			if worker.state == WorkerState::Ready {
				METRICS
					.available_slots
					.with_label_values(&[class_label])
					.sub((self.config.actors_per_thread - worker.assignments.len()) as i64);
				state.ready_free_slots = state
					.ready_free_slots
					.saturating_sub(self.config.actors_per_thread - worker.assignments.len());
			}
			if worker.class == WorkerClass::Baseline {
				state.baseline_workers = state.baseline_workers.saturating_sub(1);
			}
			for actor in &worker.assignments {
				state.assignment_owners.remove(actor);
			}
			let requests = self.plan_spawns(&mut state);
			(worker, requests)
		};
		worker.lost.cancel();
		self.changed.notify_waiters();
		if !spawn_requests.is_empty()
			&& let Err(error) = (self.callbacks.request_spawns)(spawn_requests.clone())
		{
			self.fail_spawn_requests(&spawn_requests, format!("{error:#}"));
		}
	}

	pub fn shutdown(&self) {
		self.retirement_shutdown.cancel();
		let workers = {
			let mut state = self.state.lock();
			if state.shutting_down {
				return;
			}
			state.shutting_down = true;
			for (class, count) in [
				(WorkerClass::Baseline, state.pending_baseline),
				(WorkerClass::Overflow, state.pending_overflow),
			] {
				METRICS
					.workers
					.with_label_values(&[worker_class_label(class), "starting"])
					.sub(count as i64);
			}
			state.pending_spawns.clear();
			state.pending_baseline = 0;
			state.pending_overflow = 0;
			state.spawn_failures.clear();
			state.baseline_available.clear();
			state.overflow_available.clear();
			state.ready_free_slots = 0;
			state
				.workers
				.values_mut()
				.map(|worker| {
					let class_label = worker_class_label(worker.class);
					if worker.state == WorkerState::Ready {
						METRICS
							.workers
							.with_label_values(&[class_label, "ready"])
							.dec();
						METRICS
							.workers
							.with_label_values(&[class_label, "draining"])
							.inc();
						METRICS
							.available_slots
							.with_label_values(&[class_label])
							.sub((self.config.actors_per_thread - worker.assignments.len()) as i64);
					}
					worker.state = WorkerState::Draining;
					(worker.id, worker.epoch)
				})
				.collect::<Vec<_>>()
		};
		self.changed.notify_waiters();
		for (worker_id, epoch) in workers {
			if let Err(error) = (self.callbacks.retire_worker)(worker_id, epoch) {
				tracing::warn!(
					worker_id,
					epoch,
					?error,
					"failed to retire worker during shutdown"
				);
				self.worker_lost(worker_id, epoch);
			}
		}
	}

	fn desired_baseline_workers(&self, state: &SchedulerState) -> usize {
		state.baseline_target.max(
			(state.assignment_owners.len() + state.queued_acquires)
				.min(self.config.baseline_worker_limit),
		)
	}

	fn reserve_available_worker(
		self: &Arc<Self>,
		state: &mut SchedulerState,
		actor: &ActorGenerationKey,
		actor_name: &str,
	) -> Option<ActorFactoryLease> {
		let worker_id = state
			.baseline_available
			.first()
			.map(|key| key.2)
			.or_else(|| state.overflow_available.first().map(|key| key.2))?;
		let worker = state.workers.get_mut(&worker_id)?;
		if worker.assignments.contains(actor) {
			return None;
		}
		let old_baseline_key = worker.baseline_key();
		let old_overflow_key = worker.overflow_key();
		match worker.class {
			WorkerClass::Baseline => {
				state.baseline_available.remove(&old_baseline_key);
			}
			WorkerClass::Overflow => {
				state.overflow_available.remove(&old_overflow_key);
			}
		}
		let factory = worker
			.factories
			.get(actor_name)
			.expect("registered workers were validated against expected factories")
			.clone();
		let inserted = worker.assignments.insert(actor.clone());
		debug_assert!(inserted, "actor assignment was checked before insertion");
		state
			.assignment_owners
			.insert(actor.clone(), (worker_id, worker.epoch));
		state.ready_free_slots = state.ready_free_slots.saturating_sub(1);
		worker.retirement_epoch += 1;
		worker.last_selected_sequence = state.next_sequence;
		state.next_sequence += 1;
		let lease = ActorFactoryLease {
			pool: Arc::downgrade(self),
			actor: actor.clone(),
			worker_id,
			worker_epoch: worker.epoch,
			factory,
			worker_lost: worker.lost.clone(),
			released: false,
		};
		let baseline_key = worker.baseline_key();
		let overflow_key = worker.overflow_key();
		let should_insert = worker.assignments.len() < self.config.actors_per_thread;
		let class = worker.class;
		let class_label = worker_class_label(class);
		METRICS.leases.with_label_values(&[class_label]).inc();
		METRICS
			.available_slots
			.with_label_values(&[class_label])
			.dec();
		if should_insert {
			match class {
				WorkerClass::Baseline => {
					state.baseline_available.insert(baseline_key);
				}
				WorkerClass::Overflow => {
					state.overflow_available.insert(overflow_key);
				}
			}
		}
		Some(lease)
	}

	fn plan_spawns(&self, state: &mut SchedulerState) -> Vec<WorkerSpawnRequest> {
		if state.shutting_down {
			return Vec::new();
		}
		let desired_baseline = self.desired_baseline_workers(state);
		let current_baseline = state.baseline_workers + state.pending_baseline;
		let baseline_to_spawn = desired_baseline.saturating_sub(current_baseline);
		let mut requests = Vec::new();
		for _ in 0..baseline_to_spawn {
			requests.push(Self::insert_pending_spawn(state, WorkerClass::Baseline));
		}

		let pending_slots =
			(state.pending_baseline + state.pending_overflow) * self.config.actors_per_thread;
		let uncovered = state
			.queued_acquires
			.saturating_sub(state.ready_free_slots + pending_slots);
		let overflow_to_spawn = uncovered.div_ceil(self.config.actors_per_thread);
		for _ in 0..overflow_to_spawn {
			requests.push(Self::insert_pending_spawn(state, WorkerClass::Overflow));
		}
		requests
	}

	fn insert_pending_spawn(state: &mut SchedulerState, class: WorkerClass) -> WorkerSpawnRequest {
		let worker_id = state.next_worker_id;
		state.next_worker_id += 1;
		let request = WorkerSpawnRequest {
			worker_id,
			spawn_token: Uuid::new_v4().to_string(),
			class,
		};
		state.pending_spawns.insert(
			worker_id,
			PendingSpawn {
				request: request.clone(),
			},
		);
		match class {
			WorkerClass::Baseline => state.pending_baseline += 1,
			WorkerClass::Overflow => state.pending_overflow += 1,
		}
		let class_label = worker_class_label(class);
		METRICS
			.workers
			.with_label_values(&[class_label, "starting"])
			.inc();
		METRICS
			.events
			.with_label_values(&["spawn_requested", class_label])
			.inc();
		request
	}

	fn remove_pending_spawn(
		state: &mut SchedulerState,
		worker_id: WorkerId,
	) -> Option<PendingSpawn> {
		let pending = state.pending_spawns.remove(&worker_id)?;
		match pending.request.class {
			WorkerClass::Baseline => {
				state.pending_baseline = state.pending_baseline.saturating_sub(1);
			}
			WorkerClass::Overflow => {
				state.pending_overflow = state.pending_overflow.saturating_sub(1);
			}
		}
		METRICS
			.workers
			.with_label_values(&[worker_class_label(pending.request.class), "starting"])
			.dec();
		Some(pending)
	}

	fn fail_spawn_requests(&self, requests: &[WorkerSpawnRequest], reason: String) {
		let mut state = self.state.lock();
		for request in requests {
			Self::remove_pending_spawn(&mut state, request.worker_id);
			METRICS
				.events
				.with_label_values(&["bootstrap_failure", worker_class_label(request.class)])
				.inc();
			if state.spawn_failures.len() < state.queued_acquires {
				state.spawn_failures.push_back(reason.clone());
			}
		}
		drop(state);
		self.changed.notify_waiters();
	}

	fn validate_factories(&self, factories: &HashMap<String, Arc<ActorFactory>>) -> Result<()> {
		if factories.len() != self.expected_factories.len() {
			return Err(WorkerPoolRegistrationRejected {
				reason: "actor factory names did not match the main registry".to_owned(),
			}
			.build());
		}
		for (name, expected_fingerprint) in &self.expected_factories {
			let factory = factories.get(name).ok_or_else(|| {
				WorkerPoolRegistrationRejected {
					reason: format!("actor factory {name:?} was missing"),
				}
				.build()
			})?;
			if factory.config().worker_pool_fingerprint() != *expected_fingerprint {
				return Err(WorkerPoolRegistrationRejected {
					reason: format!("actor factory {name:?} configuration did not match"),
				}
				.build());
			}
		}
		Ok(())
	}

	fn insert_availability(&self, state: &mut SchedulerState, worker: &WorkerRecord) {
		if worker.state != WorkerState::Ready
			|| worker.assignments.len() >= self.config.actors_per_thread
		{
			return;
		}
		match worker.class {
			WorkerClass::Baseline => {
				state.baseline_available.insert(worker.baseline_key());
			}
			WorkerClass::Overflow => {
				state.overflow_available.insert(worker.overflow_key());
			}
		}
	}

	fn remove_availability(&self, state: &mut SchedulerState, worker: &WorkerRecord) {
		match worker.class {
			WorkerClass::Baseline => {
				state.baseline_available.remove(&worker.baseline_key());
			}
			WorkerClass::Overflow => {
				state.overflow_available.remove(&worker.overflow_key());
			}
		}
	}

	fn release_assignment(
		self: &Arc<Self>,
		actor: &ActorGenerationKey,
		worker_id: WorkerId,
		epoch: WorkerRegistrationEpoch,
	) {
		let retire = {
			let mut state = self.state.lock();
			if state.assignment_owners.get(actor) != Some(&(worker_id, epoch)) {
				return;
			}
			let Some(worker) = state.workers.get(&worker_id) else {
				return;
			};
			if worker.epoch != epoch || !worker.assignments.contains(actor) {
				return;
			}
			let old_baseline_key = worker.baseline_key();
			let old_overflow_key = worker.overflow_key();
			let class = worker.class;
			match class {
				WorkerClass::Baseline => {
					state.baseline_available.remove(&old_baseline_key);
				}
				WorkerClass::Overflow => {
					state.overflow_available.remove(&old_overflow_key);
				}
			}
			state.assignment_owners.remove(actor);
			let worker = state
				.workers
				.get_mut(&worker_id)
				.expect("worker checked above");
			worker.assignments.remove(actor);
			let class_label = worker_class_label(class);
			METRICS.leases.with_label_values(&[class_label]).dec();
			worker.retirement_epoch += 1;
			let retirement_epoch = worker.retirement_epoch;
			let should_retire = worker.class == WorkerClass::Overflow
				&& worker.assignments.is_empty()
				&& worker.state == WorkerState::Ready;
			let baseline_key = worker.baseline_key();
			let overflow_key = worker.overflow_key();
			if worker.state == WorkerState::Ready {
				state.ready_free_slots += 1;
				METRICS
					.available_slots
					.with_label_values(&[class_label])
					.inc();
				match class {
					WorkerClass::Baseline => {
						state.baseline_available.insert(baseline_key);
					}
					WorkerClass::Overflow => {
						state.overflow_available.insert(overflow_key);
					}
				}
			}
			if should_retire {
				Some((worker_id, epoch, retirement_epoch))
			} else {
				None
			}
		};
		self.changed.notify_waiters();
		if let Some((worker_id, epoch, retirement_epoch)) = retire {
			self.schedule_retirement(worker_id, epoch, retirement_epoch);
		}
	}

	fn schedule_retirement(
		self: &Arc<Self>,
		worker_id: WorkerId,
		epoch: WorkerRegistrationEpoch,
		retirement_epoch: u64,
	) {
		let pool = Arc::downgrade(self);
		let shutdown = self.retirement_shutdown.clone();
		let deadline = Instant::now() + self.config.idle_retire_delay;
		#[cfg(feature = "native-runtime")]
		let future = async move {
			tokio::select! {
				_ = shutdown.cancelled() => return,
				_ = sleep_until(deadline) => {},
			}
			if let Some(pool) = pool.upgrade() {
				pool.retire_if_still_idle(worker_id, epoch, retirement_epoch);
			}
		};
		#[cfg(feature = "native-runtime")]
		match tokio::runtime::Handle::try_current() {
			Ok(runtime) => {
				runtime.spawn(future);
			}
			Err(error) => {
				// A lease can be released by a final synchronous owner during
				// environment teardown. Shutdown retires all workers separately;
				// skipping this idle timer is safer than panicking off-runtime.
				tracing::warn!(
					worker_id,
					epoch,
					?error,
					"could not arm worker idle retirement outside the async runtime",
				);
			}
		}
		#[cfg(not(feature = "native-runtime"))]
		RuntimeSpawner::spawn(async move {
			tokio::select! {
				_ = shutdown.cancelled() => return,
				_ = sleep_until(deadline) => {},
			}
			if let Some(pool) = pool.upgrade() {
				pool.retire_if_still_idle(worker_id, epoch, retirement_epoch);
			}
		});
	}

	fn retire_if_still_idle(
		self: &Arc<Self>,
		worker_id: WorkerId,
		epoch: WorkerRegistrationEpoch,
		retirement_epoch: u64,
	) {
		let (should_retire, should_retry) = {
			let mut state = self.state.lock();
			if state.shutting_down {
				return;
			}
			let Some(worker) = state.workers.get(&worker_id) else {
				return;
			};
			if worker.epoch != epoch
				|| worker.retirement_epoch != retirement_epoch
				|| worker.class != WorkerClass::Overflow
				|| worker.state != WorkerState::Ready
				|| !worker.assignments.is_empty()
			{
				return;
			}
			if state.queued_acquires > 0 {
				(true, true)
			} else {
				let worker = state
					.workers
					.get_mut(&worker_id)
					.expect("worker checked above");
				let key = worker.overflow_key();
				worker.state = WorkerState::Draining;
				METRICS
					.workers
					.with_label_values(&["overflow", "ready"])
					.dec();
				METRICS
					.workers
					.with_label_values(&["overflow", "draining"])
					.inc();
				METRICS
					.available_slots
					.with_label_values(&["overflow"])
					.sub(self.config.actors_per_thread as i64);
				METRICS
					.events
					.with_label_values(&["retire_requested", "overflow"])
					.inc();
				state.overflow_available.remove(&key);
				state.ready_free_slots = state
					.ready_free_slots
					.saturating_sub(self.config.actors_per_thread);
				(true, false)
			}
		};
		if should_retry {
			self.schedule_retirement(worker_id, epoch, retirement_epoch);
			return;
		}
		if should_retire && let Err(error) = (self.callbacks.retire_worker)(worker_id, epoch) {
			tracing::warn!(
				worker_id,
				epoch,
				?error,
				"failed to request idle worker retirement"
			);
			self.worker_lost(worker_id, epoch);
		}
	}

	fn detach_worker(&self, worker_id: WorkerId, epoch: WorkerRegistrationEpoch) {
		self.worker_lost(worker_id, epoch);
	}
}

impl Drop for ActorWorkerPool {
	fn drop(&mut self) {
		self.retirement_shutdown.cancel();
		let state = self.state.get_mut();
		// Environment cleanup handles are weak. The host can release the last
		// pool owner before worker exit notifications arrive.
		for worker in state.workers.values() {
			worker.lost.cancel();
			let class = worker_class_label(worker.class);
			let lifecycle = match worker.state {
				WorkerState::Ready => "ready",
				WorkerState::Draining => "draining",
			};
			METRICS.workers.with_label_values(&[class, lifecycle]).dec();
			METRICS
				.leases
				.with_label_values(&[class])
				.sub(worker.assignments.len() as i64);
			if worker.state == WorkerState::Ready {
				METRICS
					.available_slots
					.with_label_values(&[class])
					.sub((self.config.actors_per_thread - worker.assignments.len()) as i64);
			}
		}
		for (class, count) in [
			("baseline", state.pending_baseline),
			("overflow", state.pending_overflow),
		] {
			METRICS
				.workers
				.with_label_values(&[class, "starting"])
				.sub(count as i64);
		}
	}
}

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"worker_pool_invalid_config",
	"Invalid worker pool configuration",
	"Invalid worker pool configuration: {reason}"
)]
struct WorkerPoolInvalidConfig {
	reason: String,
}

#[derive(RivetError)]
#[error("actor", "worker_pool_closed", "Worker pool is closed")]
struct WorkerPoolClosed;

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"worker_pool_actor_not_registered",
	"Actor is not registered",
	"Actor {actor_name:?} is not registered in the worker pool"
)]
struct WorkerPoolActorNotRegistered {
	actor_name: String,
}

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"worker_pool_duplicate_assignment",
	"Actor generation is already assigned",
	"Actor {actor_id:?} generation {generation} is already assigned to a worker"
)]
struct WorkerPoolDuplicateAssignment {
	actor_id: String,
	generation: u32,
}

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"worker_spawn_failed",
	"Worker thread failed to start",
	"Worker thread failed to start: {reason}"
)]
struct WorkerPoolSpawnFailed {
	reason: String,
}

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"worker_acquire_timed_out",
	"Timed out waiting for a worker thread",
	"Timed out waiting for a worker thread for actor {actor_id:?} generation {generation}"
)]
struct WorkerPoolAcquireTimedOut {
	actor_id: String,
	generation: u32,
}

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"worker_registration_rejected",
	"Worker thread registration was rejected",
	"Worker thread registration was rejected: {reason}"
)]
struct WorkerPoolRegistrationRejected {
	reason: String,
}

#[cfg(test)]
#[path = "../../tests/modules/worker_pool.rs"]
mod tests;
