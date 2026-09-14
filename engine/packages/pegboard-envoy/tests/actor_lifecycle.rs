use std::sync::Arc;

use anyhow::Result;
use depot::{
	conveyer::Db,
	keys::{
		delta_chunk_key, meta_compact_key, meta_compactor_lease_key, meta_head_key, meta_quota_key,
		pidx_delta_key, shard_key,
	},
};
use gas::prelude::Id;
use rivet_envoy_protocol as protocol;
use rivet_pools::NodeId;
use tempfile::Builder;
use universaldb::utils::IsolationLevel::Snapshot;

mod conn {
	use std::sync::Arc;

	use depot::conveyer::Db;
	use depot_client::database::NativeDatabaseHandle;
	use scc::HashMap;

	pub type RemoteSqliteExecutors =
		HashMap<(String, u64), Arc<tokio::sync::OnceCell<NativeDatabaseHandle>>>;

	pub struct Conn {
		pub actor_dbs: HashMap<String, Arc<Db>>,
		pub remote_sqlite_executors: RemoteSqliteExecutors,
	}

	impl Conn {
		pub fn new() -> Self {
			Self {
				actor_dbs: HashMap::new(),
				remote_sqlite_executors: HashMap::new(),
			}
		}
	}
}

#[allow(dead_code)]
#[path = "../src/actor_lifecycle.rs"]
mod actor_lifecycle;

const TEST_ACTOR: &str = "actor-lifecycle-test";
const TEST_NAMESPACE_LABEL: u16 = 1;

async fn test_db() -> Result<universaldb::Database> {
	let path = Builder::new()
		.prefix("pegboard-envoy-actor-lifecycle-")
		.tempdir()?
		.keep();
	let driver = universaldb::driver::RocksDbDatabaseDriver::new(path).await?;

	Ok(universaldb::Database::new(Arc::new(driver)))
}

fn checkpoint(actor_id: &str) -> protocol::ActorCheckpoint {
	protocol::ActorCheckpoint {
		actor_id: actor_id.to_string(),
		generation: 1,
		index: 2,
	}
}

async fn seed(db: &universaldb::Database, keys: &[Vec<u8>]) -> Result<()> {
	let writes = keys
		.iter()
		.cloned()
		.map(|key| (key, b"present".to_vec()))
		.collect::<Vec<_>>();
	db.txn("test_pegboard_envoyactor_lifecycle", move |tx| {
		let writes = writes.clone();
		async move {
			for (key, value) in writes {
				tx.informal().set(&key, &value);
			}
			Ok(())
		}
	})
	.await
}

async fn value_exists(db: &universaldb::Database, key: Vec<u8>) -> Result<bool> {
	db.txn("test_pegboard_envoyactor_lifecycle", move |tx| {
		let key = key.clone();
		async move { Ok(tx.informal().get(&key, Snapshot).await?.is_some()) }
	})
	.await
}

fn sqlite_keys(actor_id: &str) -> Vec<Vec<u8>> {
	vec![
		meta_head_key(actor_id),
		meta_compact_key(actor_id),
		meta_quota_key(actor_id),
		meta_compactor_lease_key(actor_id),
		pidx_delta_key(actor_id, 1),
		delta_chunk_key(actor_id, 1, 0),
		shard_key(actor_id, 0),
	]
}

fn new_actor_db(db: Arc<universaldb::Database>, namespace_label: u16, actor_id: &str) -> Arc<Db> {
	Arc::new(Db::new(
		db,
		Id::new_v1(namespace_label),
		actor_id.to_string(),
		NodeId::new(),
	))
}

#[tokio::test]
async fn stop_actor_evicts_cached_actor_db() -> Result<()> {
	let db = Arc::new(test_db().await?);
	let actor_db = new_actor_db(db, TEST_NAMESPACE_LABEL, TEST_ACTOR);
	let conn = conn::Conn::new();

	assert!(
		conn.actor_dbs
			.insert_async(TEST_ACTOR.to_string(), actor_db)
			.await
			.is_ok()
	);

	actor_lifecycle::stop_actor(&conn, &checkpoint(TEST_ACTOR)).await?;

	assert!(!conn.actor_dbs.contains_async(TEST_ACTOR).await);
	Ok(())
}

#[tokio::test]
async fn stop_actor_does_not_touch_udb() -> Result<()> {
	let db = Arc::new(test_db().await?);
	let actor_db = new_actor_db(Arc::clone(&db), TEST_NAMESPACE_LABEL, TEST_ACTOR);
	let conn = conn::Conn::new();
	assert!(
		conn.actor_dbs
			.insert_async(TEST_ACTOR.to_string(), actor_db)
			.await
			.is_ok()
	);

	let keys = sqlite_keys(TEST_ACTOR);
	seed(&db, &keys).await?;

	actor_lifecycle::stop_actor(&conn, &checkpoint(TEST_ACTOR)).await?;

	for key in keys {
		assert!(value_exists(&db, key).await?);
	}

	Ok(())
}

#[tokio::test]
async fn stop_actor_allows_missing_cache_entry() -> Result<()> {
	let conn = conn::Conn::new();

	actor_lifecycle::stop_actor(&conn, &checkpoint(TEST_ACTOR)).await?;

	assert!(!conn.actor_dbs.contains_async(TEST_ACTOR).await);
	Ok(())
}

#[tokio::test]
async fn shutdown_conn_actors_evicts_all_cached_actor_dbs() -> Result<()> {
	let db = Arc::new(test_db().await?);
	let conn = conn::Conn::new();

	for (idx, actor_id) in ["shutdown-actor-a", "shutdown-actor-b"]
		.into_iter()
		.enumerate()
	{
		let actor_db = new_actor_db(Arc::clone(&db), TEST_NAMESPACE_LABEL + idx as u16, actor_id);
		assert!(
			conn.actor_dbs
				.insert_async(actor_id.to_string(), actor_db)
				.await
				.is_ok()
		);
	}

	actor_lifecycle::shutdown_conn_actors(&conn).await;

	assert!(conn.actor_dbs.is_empty());
	Ok(())
}

/// Runs `body` on a single-threaded runtime and fails if it does not finish in time.
///
/// A deadlocked runtime thread never returns, so the watchdog runs on the test thread and the
/// stuck runtime thread is left behind when the test fails.
fn run_with_deadlock_watchdog<F, Fut>(name: &str, body: F)
where
	F: FnOnce() -> Fut + Send + 'static,
	Fut: std::future::Future<Output = ()>,
{
	let (done_tx, done_rx) = std::sync::mpsc::channel();
	std::thread::spawn(move || {
		let rt = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("build runtime");
		rt.block_on(body());
		let _ = done_tx.send(());
	});

	done_rx
		.recv_timeout(std::time::Duration::from_secs(10))
		.unwrap_or_else(|_| panic!("{name} blocked the runtime thread and never completed"));
}

/// Makes an async executor lookup queue behind an entry held on another thread, then releases it
/// so the lookup is next in line when the lifecycle eviction runs on the same runtime thread.
async fn queue_executor_lookup_behind_held_entry(
	conn: &Arc<conn::Conn>,
	key: (String, u64),
) -> tokio::task::JoinHandle<()> {
	assert!(
		conn.remote_sqlite_executors
			.insert_async(key.clone(), Arc::new(tokio::sync::OnceCell::new()))
			.await
			.is_ok()
	);

	let (locked_tx, locked_rx) = std::sync::mpsc::channel();
	let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
	let holder_conn = Arc::clone(conn);
	let holder_key = key.clone();
	let holder = std::thread::spawn(move || {
		let entry = holder_conn.remote_sqlite_executors.get_sync(&holder_key);
		locked_tx.send(()).expect("signal entry held");
		release_rx.recv().expect("wait for release");
		drop(entry);
	});
	locked_rx.recv().expect("entry held");

	let lookup_conn = Arc::clone(conn);
	let lookup = tokio::spawn(async move {
		let _cell = lookup_conn
			.remote_sqlite_executors
			.entry_async(key)
			.await
			.or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
			.get()
			.clone();
	});

	// Poll the lookup until it is parked on the held entry.
	for _ in 0..16 {
		tokio::task::yield_now().await;
	}

	release_tx.send(()).expect("release entry");
	holder.join().expect("holder thread");
	lookup
}

#[test]
fn stop_actor_does_not_block_runtime_behind_queued_executor_lookup() {
	run_with_deadlock_watchdog("stop_actor", || async {
		let conn = Arc::new(conn::Conn::new());
		let lookup =
			queue_executor_lookup_behind_held_entry(&conn, (TEST_ACTOR.to_string(), 1)).await;

		actor_lifecycle::stop_actor(&conn, &checkpoint(TEST_ACTOR))
			.await
			.expect("stop actor");
		lookup.await.expect("executor lookup");
	});
}

#[test]
fn shutdown_conn_actors_does_not_block_runtime_behind_queued_executor_lookup() {
	run_with_deadlock_watchdog("shutdown_conn_actors", || async {
		let conn = Arc::new(conn::Conn::new());
		let lookup =
			queue_executor_lookup_behind_held_entry(&conn, (TEST_ACTOR.to_string(), 1)).await;

		actor_lifecycle::shutdown_conn_actors(&conn).await;
		lookup.await.expect("executor lookup");
	});
}

#[tokio::test]
async fn stop_actor_evicts_only_the_stopped_actor_executors() -> Result<()> {
	let conn = conn::Conn::new();
	for key in [
		(TEST_ACTOR.to_string(), 1),
		(TEST_ACTOR.to_string(), 2),
		("other-actor".to_string(), 1),
	] {
		assert!(
			conn.remote_sqlite_executors
				.insert_async(key, Arc::new(tokio::sync::OnceCell::new()))
				.await
				.is_ok()
		);
	}

	actor_lifecycle::stop_actor(&conn, &checkpoint(TEST_ACTOR)).await?;

	assert_eq!(conn.remote_sqlite_executors.len(), 1);
	assert!(
		conn.remote_sqlite_executors
			.contains_async(&("other-actor".to_string(), 1))
			.await
	);
	Ok(())
}
