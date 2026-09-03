use std::sync::Arc;

use anyhow::{Result, anyhow};
use depot::{
	conveyer::{Db, branch},
	keys::{
		branches_refcount_key, delta_chunk_key, meta_compact_key, meta_compactor_lease_key,
		meta_head_key, meta_quota_key, pidx_delta_key, shard_key,
	},
	types::{BucketId, DatabaseBranchId, DirtyPage},
};
use gas::prelude::Id;
use rivet_pools::NodeId;
use tempfile::Builder;
use universaldb::utils::IsolationLevel::Snapshot;

async fn test_db() -> Result<universaldb::Database> {
	let path = Builder::new()
		.prefix("pegboard-sqlite-destroy-")
		.tempdir()?
		.keep();
	let driver = universaldb::driver::RocksDbDatabaseDriver::new(path).await?;

	Ok(universaldb::Database::new(Arc::new(driver)))
}

fn sqlite_keys(actor_id: Id) -> Vec<Vec<u8>> {
	let actor_id = actor_id.to_string();
	vec![
		meta_head_key(&actor_id),
		meta_compact_key(&actor_id),
		meta_quota_key(&actor_id),
		meta_compactor_lease_key(&actor_id),
		pidx_delta_key(&actor_id, 1),
		delta_chunk_key(&actor_id, 1, 0),
		shard_key(&actor_id, 0),
	]
}

async fn seed(db: &universaldb::Database, keys: &[Vec<u8>]) -> Result<()> {
	let writes = keys
		.iter()
		.cloned()
		.map(|key| (key, b"present".to_vec()))
		.collect::<Vec<_>>();
	db.txn("test_pegboardactor_sqlite_destroy", move |tx| {
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
	db.txn("test_pegboardactor_sqlite_destroy", move |tx| {
		let key = key.clone();
		async move { Ok(tx.informal().get(&key, Snapshot).await?.is_some()) }
	})
	.await
}

#[tokio::test]
async fn actor_destroy_clears_compactor_lease() -> Result<()> {
	let db = test_db().await?;
	let actor_id = Id::new_v1(1);
	let keys = sqlite_keys(actor_id);
	seed(&db, &keys).await?;

	db.txn("test_pegboardactor_sqlite_destroy", move |tx| async move {
		pegboard::actor_sqlite::clear_v2_storage_for_destroy(&tx, actor_id);
		Ok(())
	})
	.await?;

	for key in keys {
		assert!(!value_exists(&db, key).await?);
	}

	Ok(())
}

#[tokio::test]
async fn actor_destroy_in_one_tx() -> Result<()> {
	let db = test_db().await?;
	let actor_id = Id::new_v1(1);
	let keys = sqlite_keys(actor_id);
	seed(&db, &keys).await?;

	db.txn("test_pegboardactor_sqlite_destroy", move |tx| async move {
		pegboard::actor_sqlite::clear_v2_storage_for_destroy(&tx, actor_id);
		Err::<(), anyhow::Error>(anyhow!("rollback sqlite destroy"))
	})
	.await
	.expect_err("failed transaction should roll back sqlite clears");

	for key in keys {
		assert!(value_exists(&db, key).await?);
	}

	Ok(())
}

async fn read_refcount(db: &universaldb::Database, branch: DatabaseBranchId) -> Result<i64> {
	let bytes = db
		.txn("test_pegboardactor_sqlite_destroy", move |tx| async move {
			Ok(tx.informal().get(&branches_refcount_key(branch), Snapshot).await?)
		})
		.await?
		.expect("branch refcount should exist");
	let arr: [u8; 8] = bytes.as_slice().try_into().expect("refcount i64 LE");
	Ok(i64::from_le_bytes(arr))
}

fn branch_page(pgno: u32) -> DirtyPage {
	DirtyPage {
		pgno,
		bytes: vec![0xAA; depot::keys::PAGE_SIZE as usize],
	}
}

#[tokio::test]
async fn actor_destroy_clears_branch_backed_database() -> Result<()> {
	let db = test_db().await?;
	let namespace_id = Id::new_v1(0x1111);
	let actor_id = Id::new_v1(0x2222);
	let bucket = BucketId::from_gas_id(namespace_id);
	let db_arc = Arc::new(db.clone());
	let depot_db = Db::new(db_arc.clone(), namespace_id, actor_id.to_string(), NodeId::new());
	depot_db.commit(vec![branch_page(1)], 1, 1_000).await?;

	let branch = db
		.txn("test_resolve", |tx| {
			let actor_id = actor_id.to_string();
			async move {
				branch::resolve_database_branch(&tx, bucket, &actor_id, Snapshot).await
			}
		})
		.await?
		.expect("branch should exist before destroy");
	assert_eq!(read_refcount(&db, branch).await?, 1);
	assert_eq!(branch::list_databases(&db, bucket).await?, vec![branch]);

	pegboard::actor_sqlite::clear_branch_storage_for_destroy(&db, namespace_id, actor_id).await?;

	assert_eq!(read_refcount(&db, branch).await?, 0);
	assert_eq!(
		branch::list_databases(&db, bucket).await?,
		Vec::<DatabaseBranchId>::new()
	);

	Ok(())
}

#[tokio::test]
async fn actor_destroy_branch_is_idempotent() -> Result<()> {
	let db = test_db().await?;
	let namespace_id = Id::new_v1(0x3333);
	let actor_id = Id::new_v1(0x4444);
	let bucket = BucketId::from_gas_id(namespace_id);
	let db_arc = Arc::new(db.clone());
	let depot_db = Db::new(db_arc, namespace_id, actor_id.to_string(), NodeId::new());
	depot_db.commit(vec![branch_page(1)], 1, 1_000).await?;

	let branch = db
		.txn("test_resolve", |tx| {
			let actor_id = actor_id.to_string();
			async move {
				branch::resolve_database_branch(&tx, bucket, &actor_id, Snapshot).await
			}
		})
		.await?
		.expect("branch should exist");
	pegboard::actor_sqlite::clear_branch_storage_for_destroy(&db, namespace_id, actor_id).await?;
	// Second destroy is a no-op.
	pegboard::actor_sqlite::clear_branch_storage_for_destroy(&db, namespace_id, actor_id).await?;

	assert_eq!(read_refcount(&db, branch).await?, 0);
	Ok(())
}

#[tokio::test]
async fn actor_destroy_branch_noop_when_no_database() -> Result<()> {
	let db = test_db().await?;
	let namespace_id = Id::new_v1(0x5555);
	let actor_id = Id::new_v1(0x6666);

	// No depot database was ever created.
	pegboard::actor_sqlite::clear_branch_storage_for_destroy(&db, namespace_id, actor_id).await?;

	// Also works when bucket itself was never created.
	pegboard::actor_sqlite::clear_branch_storage_for_destroy(&db, namespace_id, actor_id).await?;
	Ok(())
}

#[tokio::test]
async fn actor_destroy_clears_both_layouts_like_workflow() -> Result<()> {
	// Simulates ClearKv wiring.
	let db = test_db().await?;
	let namespace_id = Id::new_v1(0x7777);
	let actor_id = Id::new_v1(0x8888);
	let bucket = BucketId::from_gas_id(namespace_id);

	// Legacy keys.
	let legacy_keys = sqlite_keys(actor_id);
	seed(&db, &legacy_keys).await?;

	// Branch-backed DB.
	let depot_db = Db::new(Arc::new(db.clone()), namespace_id, actor_id.to_string(), NodeId::new());
	depot_db.commit(vec![branch_page(1)], 1, 1_000).await?;
	let branch = db
		.txn("test_resolve", |tx| {
			let actor_id = actor_id.to_string();
			async move { branch::resolve_database_branch(&tx, bucket, &actor_id, Snapshot).await }
		})
		.await?
		.expect("branch should exist");

	db.txn("test_workflow_clear_kv", |tx| async move {
		pegboard::actor_sqlite::clear_v2_storage_for_destroy(&tx, actor_id);
		Ok(())
	})
	.await?;
	pegboard::actor_sqlite::clear_branch_storage_for_destroy(&db, namespace_id, actor_id).await?;

	// Both layouts gone.
	for key in legacy_keys {
		assert!(!value_exists(&db, key).await?);
	}
	assert_eq!(read_refcount(&db, branch).await?, 0);
	assert_eq!(branch::list_databases(&db, bucket).await?, Vec::<DatabaseBranchId>::new());

	Ok(())
}
