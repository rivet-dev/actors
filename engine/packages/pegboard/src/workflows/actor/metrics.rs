use futures_util::{FutureExt, TryStreamExt};
use gas::prelude::*;
use rand::Rng;
use std::time::{Duration, Instant};
use universaldb::prelude::*;

use crate::keys;

const METRICS_INTERVAL_MS: i64 = util::duration::seconds(60);
const EARLY_TXN_TIMEOUT: Duration = Duration::from_millis(2500);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Input {
	pub actor_id: Id,
	pub namespace_id: Id,
	pub name: String,
}

#[derive(Deserialize, Serialize)]
struct State {
	actor_id: Id,
	namespace_id: Id,
	name: String,
	last_kv_storage_size: i64,
}

#[derive(Deserialize, Serialize)]
struct LifecycleState {
	paused: bool,
	last_recorded_awake_ts: Option<i64>,
	last_kv_storage_size: i64,
}

impl LifecycleState {
	pub fn new(paused: bool) -> Self {
		LifecycleState {
			paused,
			last_recorded_awake_ts: None,
			last_kv_storage_size: 0,
		}
	}
}

#[workflow]
pub(crate) async fn pegboard_actor_metrics(ctx: &mut WorkflowCtx, input: &Input) -> Result<()> {
	let start_paused = ctx
		.activity(InitStateInput {
			actor_id: input.actor_id,
			namespace_id: input.namespace_id,
			name: input.name.clone(),
		})
		.await?;

	ctx.loope(LifecycleState::new(start_paused), |ctx, state| {
		async move {
			let sigs = if state.paused {
				ctx.listen_n::<Main>(256).await?
			} else {
				// Jitter sleep to prevent stampeding herds
				let jitter = { rand::thread_rng().gen_range(0..METRICS_INTERVAL_MS / 10) };

				ctx.listen_n_with_timeout::<Main>(METRICS_INTERVAL_MS + jitter, 256)
					.await?
			};

			let mut new_awake_duration = 0;
			let mut destroy = false;
			for sig in &sigs {
				match sig {
					Main::Pause(sig) => {
						if let Some(last_recorded_awake_ts) = state.last_recorded_awake_ts {
							new_awake_duration += sig.ts - last_recorded_awake_ts;
						}
						state.last_recorded_awake_ts = None;
						state.paused = true;
					}
					Main::Resume(sig) => {
						if state.last_recorded_awake_ts.is_none() {
							state.last_recorded_awake_ts = Some(sig.ts);
						}
						state.paused = false;
					}
					Main::Destroy(sig) => {
						if let Some(last_recorded_awake_ts) = state.last_recorded_awake_ts {
							new_awake_duration += sig.ts - last_recorded_awake_ts;
						}

						destroy = true;
						break;
					}
				}
			}

			// Timeout was reached, record duration up till now
			if sigs.is_empty() {
				let now = ctx.v(2).activity(GetTsInput {}).await?;
				if let Some(last_recorded_awake_ts) = state.last_recorded_awake_ts {
					new_awake_duration += now - last_recorded_awake_ts;
				}
				state.last_recorded_awake_ts = Some(now);
			}

			// NOTE: Cannot join these activities, they read from state
			if new_awake_duration > 0 {
				ctx.activity(RecordMetricsInput {
					awake_duration: new_awake_duration,
				})
				.await?;
			}

			ctx.activity(RecordKvMetricsInput {}).await?;

			if destroy {
				Ok(Loop::Break(()))
			} else {
				Ok(Loop::Continue)
			}
		}
		.boxed()
	})
	.await?;

	// The actor's kv is deleted on destroy, so remove its contribution from the running kv
	// storage total.
	ctx.v(2).activity(RemoveKvMetricsInput {}).await?;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct InitStateInput {
	actor_id: Id,
	namespace_id: Id,
	name: String,
}

#[activity(InitState)]
async fn init_state(ctx: &ActivityCtx, input: &InitStateInput) -> Result<bool> {
	let mut state = ctx.state::<Option<State>>()?;

	*state = Some(State {
		actor_id: input.actor_id,
		namespace_id: input.namespace_id,
		name: input.name.clone(),
		last_kv_storage_size: 0,
	});

	// Check if actor is sleeping when this workflow was created. This can return true if this workflow was
	// backfilled
	ctx.udb()?
		.txn("pegboard_actor_read_sleeping", |tx| async move {
			let tx = tx.with_subspace(keys::subspace());

			tx.exists(&keys::actor::SleepTsKey::new(input.actor_id), Serializable)
				.await
		})
		.custom_instrument(tracing::info_span!("actor_read_sleeping_tx"))
		.await
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct GetTsInput {}

#[activity(GetTs)]
async fn get_ts(ctx: &ActivityCtx, input: &GetTsInput) -> Result<i64> {
	Ok(util::timestamp::now())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct RecordMetricsInput {
	/// Milliseconds.
	awake_duration: i64,
}

#[activity(RecordMetrics)]
async fn record_metrics(ctx: &ActivityCtx, input: &RecordMetricsInput) -> Result<()> {
	let state = ctx.state::<State>()?;

	// Seconds (rounded up)
	let awake_duration =
		util::math::div_ceil_i64((input.awake_duration).max(0), util::duration::seconds(1));

	let namespace_id = state.namespace_id;
	let name = &state.name;
	ctx.udb()?
		.txn("pegboard_actor_record_metrics", |tx| async move {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::ActorAwake(name.clone()),
				awake_duration,
			);

			Ok(())
		})
		.custom_instrument(tracing::info_span!("actor_record_metrics_tx"))
		.await?;

	Ok(())
}

enum KvStorageQueryResult {
	GoodEstimate(i64),
	Chunk(i64, Vec<u8>),
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct RecordKvMetricsInput {}

#[activity(RecordKvMetrics)]
async fn record_kv_metrics(ctx: &ActivityCtx, input: &RecordKvMetricsInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	let actor_id = state.actor_id;
	let namespace_id = state.namespace_id;
	let name = &state.name;
	let last_kv_storage_size = state.last_kv_storage_size;

	let mut new_kv_storage_size = 0;
	let mut last_key = Vec::new();
	let mut already_estimated = false;

	// Full scan the entire actor kv in chunks. If a good estimate is received, we don't scan.
	loop {
		let res = ctx
			.udb()?
			.txn("pegboard_actor_kv_size_scan", |tx| {
				let last_key = &last_key;
				async move {
					let start = Instant::now();
					let tx = tx.with_subspace(keys::subspace());

					if !already_estimated {
						let estimate_kv_storage_size =
							crate::actor_kv::estimate_kv_size(&tx, actor_id).await?;

						// FDB recommends you do not trust size estimates below 3mb. (See
						// https://apple.github.io/foundationdb/api-c.html#c.fdb_transaction_get_estimated_range_size_bytes)
						if estimate_kv_storage_size > util::size::mebibytes(3) as i64 {
							namespace::keys::metric::inc(
								&tx.with_subspace(namespace::keys::subspace()),
								namespace_id,
								namespace::keys::metric::Metric::KvStorageUsed(name.to_string()),
								estimate_kv_storage_size - last_kv_storage_size,
							);

							return Ok(KvStorageQueryResult::GoodEstimate(
								estimate_kv_storage_size,
							));
						}
					}

					let mut chunk_size = 0;
					let mut new_last_key = Vec::new();

					let ns_metrics_subspace = keys::actor_kv::subspace(actor_id);
					let range = ns_metrics_subspace.range();

					let range_start = if last_key.is_empty() {
						&range.0
					} else {
						&last_key
					};
					let range_end = &ns_metrics_subspace.range().1;

					let mut stream = tx.get_ranges_keyvalues(
						universaldb::RangeOption {
							mode: StreamingMode::WantAll,
							..(range_start.as_slice(), range_end.as_slice()).into()
						},
						Snapshot,
					);

					loop {
						if start.elapsed() > EARLY_TXN_TIMEOUT {
							tracing::warn!(?actor_id, "timed out processing actor kv size");
							break;
						}

						let Some(entry) = stream.try_next().await? else {
							new_last_key = Vec::new();
							break;
						};

						chunk_size += entry.key().len() + entry.value().len();
						new_last_key = [entry.key(), &[0xff]].concat();
					}

					Ok(KvStorageQueryResult::Chunk(chunk_size as i64, new_last_key))
				}
			})
			.custom_instrument(tracing::info_span!("record_kv_metrics_tx"))
			.await?;

		match res {
			KvStorageQueryResult::GoodEstimate(size) => {
				state.last_kv_storage_size = size;
				return Ok(());
			}
			KvStorageQueryResult::Chunk(chunk_size, new_last_key) => {
				already_estimated = true;
				new_kv_storage_size += chunk_size;
				last_key = new_last_key;

				if last_key.is_empty() {
					break;
				}
			}
		}
	}

	ctx.udb()?
		.txn("pegboard_actor_record_kv_metrics", |tx| async move {
			namespace::keys::metric::inc(
				&tx.with_subspace(namespace::keys::subspace()),
				namespace_id,
				namespace::keys::metric::Metric::KvStorageUsed(name.to_string()),
				new_kv_storage_size - last_kv_storage_size,
			);

			Ok(())
		})
		.await?;

	state.last_kv_storage_size = new_kv_storage_size;

	Ok(())
}

#[derive(Debug, Serialize, Deserialize, Hash)]
struct RemoveKvMetricsInput {}

#[activity(RemoveKvMetrics)]
async fn remove_kv_metrics(ctx: &ActivityCtx, input: &RemoveKvMetricsInput) -> Result<()> {
	let mut state = ctx.state::<State>()?;

	let namespace_id = state.namespace_id;
	let name = &state.name;
	let last_kv_storage_size = state.last_kv_storage_size;

	if last_kv_storage_size != 0 {
		ctx.udb()?
			.txn("pegboard_actor_remove_kv_metrics", |tx| async move {
				namespace::keys::metric::inc(
					&tx.with_subspace(namespace::keys::subspace()),
					namespace_id,
					namespace::keys::metric::Metric::KvStorageUsed(name.to_string()),
					-last_kv_storage_size,
				);

				Ok(())
			})
			.await?;
	}

	state.last_kv_storage_size = 0;

	Ok(())
}

#[signal("pegboard_actor_metrics_pause")]
pub struct Pause {
	pub ts: i64,
}

#[signal("pegboard_actor_metrics_resume")]
pub struct Resume {
	pub ts: i64,
}

#[signal("pegboard_actor_metrics_destroy")]
pub struct Destroy {
	pub ts: i64,
}

join_signal!(Main {
	Pause,
	Resume,
	Destroy,
});
