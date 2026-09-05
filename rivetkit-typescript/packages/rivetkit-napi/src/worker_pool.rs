use std::sync::{Arc, LazyLock, Weak};

use napi::threadsafe_function::{
	ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi::{Env, JsFunction, Status};
use rivetkit_core::registry::worker_pool::{
	ActorWorkerPool, ActorWorkerPoolCallbacks, WorkerClass, WorkerId, WorkerRegistrationEpoch,
	WorkerRegistrationHandle, WorkerSpawnRequest,
};
use scc::HashMap as SccHashMap;

use crate::{NapiInvalidArgument, napi_anyhow_error};

type SpawnTsfn = ThreadsafeFunction<Vec<WorkerSpawnRequest>, ErrorStrategy::Fatal>;
type RetireTsfn = ThreadsafeFunction<(WorkerId, WorkerRegistrationEpoch), ErrorStrategy::Fatal>;

static WORKER_POOLS: LazyLock<SccHashMap<String, Weak<ActorWorkerPool>>> =
	LazyLock::new(SccHashMap::new);
const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone)]
pub(crate) struct WorkerPoolHost {
	pool_id: String,
	pool: Arc<ActorWorkerPool>,
}

impl WorkerPoolHost {
	pub(crate) fn new(pool_id: String, pool: Arc<ActorWorkerPool>) -> napi::Result<Self> {
		WORKER_POOLS.retain_sync(|_, pool| pool.strong_count() > 0);
		WORKER_POOLS
			.insert_sync(pool_id.clone(), Arc::downgrade(&pool))
			.map_err(|_| invalid_argument("poolId", "worker pool id is already registered"))?;
		Ok(Self { pool_id, pool })
	}

	pub(crate) fn pool(&self) -> &Arc<ActorWorkerPool> {
		&self.pool
	}

	pub(crate) fn unregister_directory(&self) {
		let should_remove = WORKER_POOLS.get_sync(&self.pool_id).is_some_and(|entry| {
			entry
				.get()
				.upgrade()
				.is_some_and(|pool| Arc::ptr_eq(&pool, &self.pool))
		});
		if should_remove {
			WORKER_POOLS.remove_sync(&self.pool_id);
		}
	}
}

pub(crate) fn lookup_pool(pool_id: &str) -> napi::Result<Arc<ActorWorkerPool>> {
	let pool = WORKER_POOLS
		.get_sync(pool_id)
		.and_then(|entry| entry.get().upgrade());
	match pool {
		Some(pool) => Ok(pool),
		None => {
			WORKER_POOLS.remove_sync(pool_id);
			Err(invalid_argument(
				"poolId",
				"worker pool is missing, shut down, or belongs to another process",
			))
		}
	}
}

pub(crate) fn create_callbacks(
	env: &Env,
	request_spawns: JsFunction,
	retire_worker: JsFunction,
) -> napi::Result<ActorWorkerPoolCallbacks> {
	let mut request_spawns = create_spawn_tsfn(request_spawns)?;
	request_spawns.unref(env)?;
	let mut retire_worker = create_retire_tsfn(retire_worker)?;
	retire_worker.unref(env)?;
	Ok(ActorWorkerPoolCallbacks::new(
		move |requests| {
			check_tsfn_status(
				request_spawns.call(requests, ThreadsafeFunctionCallMode::NonBlocking),
			)
		},
		move |worker_id, worker_epoch| {
			check_tsfn_status(retire_worker.call(
				(worker_id, worker_epoch),
				ThreadsafeFunctionCallMode::NonBlocking,
			))
		},
	))
}

fn create_spawn_tsfn(callback: JsFunction) -> napi::Result<SpawnTsfn> {
	callback.create_threadsafe_function(0, |ctx: ThreadSafeCallContext<Vec<WorkerSpawnRequest>>| {
		let mut array = ctx.env.create_array_with_length(ctx.value.len())?;
		for (index, request) in ctx.value.into_iter().enumerate() {
			let mut object = ctx.env.create_object()?;
			object.set(
				"workerId",
				u64_to_js_integer(request.worker_id, "workerId")?,
			)?;
			object.set("spawnToken", request.spawn_token)?;
			object.set("class", worker_class_name(request.class))?;
			array.set_element(index as u32, object)?;
		}
		Ok(vec![array.into_unknown()])
	})
}

fn create_retire_tsfn(callback: JsFunction) -> napi::Result<RetireTsfn> {
	callback.create_threadsafe_function(
		0,
		|ctx: ThreadSafeCallContext<(WorkerId, WorkerRegistrationEpoch)>| {
			let mut object = ctx.env.create_object()?;
			object.set("workerId", u64_to_js_integer(ctx.value.0, "workerId")?)?;
			object.set(
				"workerEpoch",
				u64_to_js_integer(ctx.value.1, "workerEpoch")?,
			)?;
			Ok(vec![object.into_unknown()])
		},
	)
}

pub(crate) fn parse_worker_class(value: &str) -> napi::Result<WorkerClass> {
	match value {
		"baseline" => Ok(WorkerClass::Baseline),
		"overflow" => Ok(WorkerClass::Overflow),
		_ => Err(invalid_argument(
			"class",
			"must be either \"baseline\" or \"overflow\"",
		)),
	}
}

pub(crate) fn parse_worker_id(value: f64) -> napi::Result<WorkerId> {
	parse_js_safe_integer(value, "workerId")
}

pub(crate) fn parse_worker_epoch(value: f64) -> napi::Result<WorkerRegistrationEpoch> {
	parse_js_safe_integer(value, "workerEpoch")
}

pub(crate) fn parse_positive_usize(value: f64, argument: &str) -> napi::Result<usize> {
	let value = parse_js_safe_integer(value, argument)?;
	if value == 0 {
		return Err(invalid_argument(argument, "must be greater than zero"));
	}
	value
		.try_into()
		.map_err(|_| invalid_argument(argument, "is too large for this platform"))
}

pub(crate) fn registration_result(
	handle: &WorkerRegistrationHandle,
) -> napi::Result<JsWorkerRegistration> {
	Ok(JsWorkerRegistration {
		worker_id: u64_to_js_integer(handle.worker_id(), "workerId")?,
		worker_epoch: u64_to_js_integer(handle.worker_epoch(), "workerEpoch")?,
	})
}

#[napi_derive::napi(object)]
pub struct JsWorkerRegistration {
	pub worker_id: i64,
	pub worker_epoch: i64,
}

fn worker_class_name(class: WorkerClass) -> &'static str {
	match class {
		WorkerClass::Baseline => "baseline",
		WorkerClass::Overflow => "overflow",
	}
}

fn check_tsfn_status(status: Status) -> anyhow::Result<()> {
	if status == Status::Ok {
		Ok(())
	} else {
		anyhow::bail!("worker pool control callback is unavailable: {status:?}")
	}
}

fn parse_js_safe_integer(value: f64, argument: &str) -> napi::Result<u64> {
	if !value.is_finite()
		|| value < 0.0
		|| value.fract() != 0.0
		|| value > JAVASCRIPT_MAX_SAFE_INTEGER as f64
	{
		return Err(invalid_argument(
			argument,
			"must be a non-negative JavaScript safe integer",
		));
	}
	Ok(value as u64)
}

fn u64_to_js_integer(value: u64, argument: &str) -> napi::Result<i64> {
	if value > JAVASCRIPT_MAX_SAFE_INTEGER {
		return Err(invalid_argument(
			argument,
			"exceeded JavaScript safe integer range",
		));
	}
	Ok(value as i64)
}

fn invalid_argument(argument: &str, reason: &str) -> napi::Error {
	napi_anyhow_error(
		NapiInvalidArgument {
			argument: argument.to_owned(),
			reason: reason.to_owned(),
		}
		.build(),
	)
}

#[cfg(test)]
mod tests {
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
}
