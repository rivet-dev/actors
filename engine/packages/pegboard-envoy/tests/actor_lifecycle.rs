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
use scc::HashMap;
use tempfile::Builder;
use universaldb::utils::IsolationLevel::Snapshot;

mod conn {
	use std::sync::Arc;

	use depot::conveyer::Db;
	use depot_client::database::NativeDatabaseHandle;
	use scc::HashMap;
	use tokio::sync::OnceCell;

	pub struct Conn {
		pub actor_dbs: HashMap<String, Arc<Db>>,
		pub remote_sqlite_executors: HashMap<(String, u64), Arc<OnceCell<NativeDatabaseHandle>>>,
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

fn empty_conn() -> conn::Conn {
	conn::Conn {
		actor_dbs: HashMap::new(),
		remote_sqlite_executors: HashMap::new(),
	}
}

#[tokio::test]
async fn stop_actor_evicts_cached_actor_db() -> Result<()> {
	let db = Arc::new(test_db().await?);
	let actor_db = new_actor_db(db, TEST_NAMESPACE_LABEL, TEST_ACTOR);
	let conn = empty_conn();

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
	let conn = empty_conn();
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
async fn replayed_stop_for_unknown_actor_allows_replacement_start() -> Result<()> {
	let conn = empty_conn();

	actor_lifecycle::stop_actor(&conn, &checkpoint(TEST_ACTOR)).await?;

	assert!(!conn.actor_dbs.contains_async(TEST_ACTOR).await);
	let db = Arc::new(test_db().await?);
	let replacement_actor_db = new_actor_db(db, TEST_NAMESPACE_LABEL, TEST_ACTOR);
	assert!(
		conn.actor_dbs
			.insert_async(TEST_ACTOR.to_string(), replacement_actor_db)
			.await
			.is_ok()
	);
	assert!(conn.actor_dbs.contains_async(TEST_ACTOR).await);
	Ok(())
}

#[tokio::test]
async fn shutdown_conn_actors_evicts_all_cached_actor_dbs() -> Result<()> {
	let db = Arc::new(test_db().await?);
	let conn = empty_conn();

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
