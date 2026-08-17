use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result, ensure};
use depot::{
	conveyer::{Db, branch as depot_branch},
	keys as depot_keys,
	types::{BucketId, DBHead, DirtyPage, SQLITE_PAGE_SIZE, decode_db_head},
};
use gas::prelude::{Id, util::timestamp};
use rivet_envoy_protocol as protocol;
use rivet_pools::NodeId;

use crate::{actor_kv::Recipient, metrics};

const SQLITE_V1_PREFIX: u8 = 0x08;
const SQLITE_V1_SCHEMA_VERSION: u8 = 0x01;
const SQLITE_V1_META_PREFIX: u8 = 0x00;
const SQLITE_V1_CHUNK_PREFIX: u8 = 0x01;
const SQLITE_V1_META_VERSION: u16 = 1;
const SQLITE_V1_META_LEN: usize = 10;
const SQLITE_V1_CHUNK_SIZE: usize = 4096;
const SQLITE_V1_MAX_MIGRATION_BYTES: u64 = 128 * 1024 * 1024;
const FILE_TAG_MAIN: u8 = 0x00;
const FILE_TAG_JOURNAL: u8 = 0x01;
const FILE_TAG_WAL: u8 = 0x02;
const FILE_TAG_SHM: u8 = 0x03;
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

pub fn clear_v2_storage_for_destroy(tx: &universaldb::Transaction, actor_id: Id) {
	let actor_id = actor_id.to_string();

	tx.informal().clear(&depot_keys::meta_head_key(&actor_id));
	tx.informal()
		.clear(&depot_keys::meta_compact_key(&actor_id));
	tx.informal().clear(&depot_keys::meta_quota_key(&actor_id));
	// Clear the lease with the rest of Depot.
	// Otherwise dead lease keys accumulate in UDB indefinitely.
	tx.informal()
		.clear(&depot_keys::meta_compactor_lease_key(&actor_id));

	for prefix in [
		depot_keys::shard_prefix(&actor_id),
		depot_keys::delta_prefix(&actor_id),
		depot_keys::pidx_delta_prefix(&actor_id),
	] {
		let (begin, end) = prefix_range(&prefix);
		tx.informal().clear_range(&begin, &end);
	}
}

pub async fn clear_branch_storage_for_destroy(
	db: &universaldb::Database,
	namespace_id: Id,
	actor_id: Id,
) -> Result<()> {
	let bucket = BucketId::from_gas_id(namespace_id);
	let database_name = actor_id.to_string();

	let branch_id = match resolve_branch_for_destroy(db, bucket, &database_name).await? {
		Some(id) => id,
		None => return Ok(()),
	};

	match depot_branch::delete_database(db, bucket, branch_id).await {
		Ok(()) => Ok(()),
		Err(err) if is_database_not_found(&err) => {
			tracing::debug!(?branch_id, %database_name, "branch already deleted on destroy");
			Ok(())
		}
		Err(err) => Err(err),
	}
}

async fn resolve_branch_for_destroy(
	db: &universaldb::Database,
	bucket: BucketId,
	database_name: &str,
) -> Result<Option<depot::conveyer::types::DatabaseBranchId>> {
	let database_name_owned = database_name.to_string();
	let database_name_for_txn = database_name_owned.clone();
	let res = db
		.txn("pegboard_actor_sqlite_resolve_branch", move |tx| {
			let database_name = database_name_for_txn.clone();
			async move {
				depot_branch::resolve_database_branch(
					&tx,
					bucket,
					&database_name,
					universaldb::utils::IsolationLevel::Snapshot,
				)
				.await
			}
		})
		.await;

	match res {
		Ok(branch) => Ok(branch),
		Err(err) if is_database_not_found(&err) => {
			tracing::debug!(%database_name_owned, "database not found while resolving branch for destroy");
			Ok(None)
		}
		Err(err) => Err(err),
	}
}

fn is_database_not_found(err: &anyhow::Error) -> bool {
	err.chain()
		.find_map(|c| c.downcast_ref::<depot::conveyer::error::SqliteStorageError>())
		.is_some_and(|e| matches!(e, depot::conveyer::error::SqliteStorageError::DatabaseNotFound))
}

fn prefix_range(prefix: &[u8]) -> (Vec<u8>, Vec<u8>) {
	universaldb::tuple::Subspace::from_bytes(prefix.to_vec()).range()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Hash)]
pub struct MigrateV1ToV2Input {
	pub actor_id: Id,
	pub namespace_id: Id,
	pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Hash)]
pub struct MigrateV1ToV2Output {
	pub migrated: bool,
}

pub async fn migrate_v1_to_v2(
	db: universaldb::Database,
	input: MigrateV1ToV2Input,
) -> Result<MigrateV1ToV2Output> {
	let recipient = Recipient {
		actor_id: input.actor_id,
		namespace_id: input.namespace_id,
		name: input.name,
	};

	let migrated = maybe_migrate_v1_to_v2(&db, &recipient).await?;

	Ok(MigrateV1ToV2Output { migrated })
}

async fn maybe_migrate_v1_to_v2(db: &universaldb::Database, recipient: &Recipient) -> Result<bool> {
	if !crate::actor_kv::sqlite_v1_data_exists(db, recipient.actor_id).await? {
		return Ok(false);
	}

	let actor_id = recipient.actor_id.to_string();

	if load_v2_head(db, recipient.namespace_id, &actor_id)
		.await?
		.is_some()
	{
		return Ok(false);
	}

	metrics::SQLITE_MIGRATION_ATTEMPTS_TOTAL.inc();
	let start = Instant::now();

	// Known failure case: if the v1 main file is corrupt or otherwise
	// unparseable (for example a torn write left by a crash mid-transaction),
	// reading or validating it returns a deterministic error. That error
	// propagates out of the activity, so the activity fails and keeps retrying
	// without making progress. There is no automatic recovery for this. The
	// actor stays on v1 and requires manual triage to recover or discard its
	// database.
	let main = read_v1_main(db, recipient)
		.await
		.map_err(|err| migration_error(&actor_id, "read_v1", err))?;
	let recovered = validate_v1_main(&actor_id, main)
		.map_err(|err| migration_error(&actor_id, "validate", err))?;
	metrics::SQLITE_MIGRATION_PAGES.observe(recovered.total_pages as f64);
	tracing::info!(
		actor_id = %actor_id,
		pages = recovered.total_pages,
		size_bytes = recovered.bytes.len(),
		"starting v1→v2 migration"
	);

	let dirty_pages = recovered
		.bytes
		.chunks(SQLITE_PAGE_SIZE as usize)
		.enumerate()
		.map(|(idx, bytes)| DirtyPage {
			pgno: idx as u32 + 1,
			bytes: bytes.to_vec(),
		})
		.collect::<Vec<_>>();
	let actor_db = Db::new(
		Arc::new(db.clone()),
		recipient.namespace_id,
		actor_id.clone(),
		NodeId::new(),
	);
	actor_db
		.commit(dirty_pages, recovered.total_pages, timestamp::now())
		.await
		.map_err(|err| migration_error(&actor_id, "finalize", err))?;

	metrics::SQLITE_MIGRATION_SUCCESSES_TOTAL.inc();
	metrics::SQLITE_MIGRATION_DURATION.observe(start.elapsed().as_secs_f64());
	tracing::info!(
		actor_id = %actor_id,
		pages = recovered.total_pages,
		duration_ms = start.elapsed().as_millis(),
		"v1→v2 migration complete"
	);

	Ok(true)
}

async fn load_v2_head(
	db: &universaldb::Database,
	namespace_id: Id,
	actor_id: &str,
) -> Result<Option<DBHead>> {
	let actor_id = actor_id.to_string();
	let bucket_id = BucketId::from_gas_id(namespace_id);
	db.txn("pegboard_actor_sqlite_get_head", move |tx| {
		let actor_id = actor_id.clone();
		let bucket_id = bucket_id;
		async move {
			let key = if let Some(branch_id) = depot_branch::resolve_database_branch(
				&tx,
				bucket_id,
				&actor_id,
				universaldb::utils::IsolationLevel::Snapshot,
			)
			.await?
			{
				depot_keys::branch_meta_head_key(branch_id)
			} else {
				depot_keys::meta_head_key(&actor_id)
			};
			tx.informal()
				.get(&key, universaldb::utils::IsolationLevel::Snapshot)
				.await?
				.map(|bytes| decode_db_head(bytes.as_ref()))
				.transpose()
				.context("decode sqlite db head")
		}
	})
	.await
}

fn migration_error(actor_id: &str, phase: &'static str, err: anyhow::Error) -> anyhow::Error {
	metrics::SQLITE_MIGRATION_FAILURES_TOTAL
		.with_label_values(&[phase])
		.inc();
	tracing::error!(actor_id = %actor_id, phase, ?err, "v1→v2 migration failed");
	err
}

async fn read_v1_main(db: &universaldb::Database, recipient: &Recipient) -> Result<V1File> {
	// v1->v2 migration is best-effort: abandon sidecars and import the main
	// database file as-is.
	abandon_v1_sidecar_if_exists(db, recipient, FILE_TAG_JOURNAL, "journal").await?;
	abandon_v1_sidecar_if_exists(db, recipient, FILE_TAG_WAL, "wal").await?;
	abandon_v1_sidecar_if_exists(db, recipient, FILE_TAG_SHM, "shm").await?;

	read_v1_file(db, recipient, FILE_TAG_MAIN)
		.await?
		.context("sqlite v1 main file missing metadata")
}

async fn abandon_v1_sidecar_if_exists(
	db: &universaldb::Database,
	recipient: &Recipient,
	file_tag: u8,
	sidecar: &'static str,
) -> Result<()> {
	if v1_file_exists(db, recipient, file_tag).await? {
		metrics::SQLITE_MIGRATION_ABANDONED_SIDECAR_TOTAL
			.with_label_values(&[sidecar])
			.inc();
		tracing::warn!(
			actor_id = %recipient.actor_id,
			sidecar,
			"abandoning sqlite v1 sidecar during v1→v2 migration"
		);
	}

	Ok(())
}

async fn v1_file_exists(
	db: &universaldb::Database,
	recipient: &Recipient,
	file_tag: u8,
) -> Result<bool> {
	let (meta_keys, _, _) =
		crate::actor_kv::get(db, recipient, vec![v1_meta_key(file_tag).to_vec()]).await?;
	if !meta_keys.is_empty() {
		return Ok(true);
	}

	let (keys, _, _) = crate::actor_kv::list(
		db,
		recipient,
		protocol::KvListQuery::KvListPrefixQuery(protocol::KvListPrefixQuery {
			key: v1_chunk_prefix(file_tag).to_vec(),
		}),
		false,
		Some(1),
	)
	.await?;

	Ok(!keys.is_empty())
}

async fn read_v1_file(
	db: &universaldb::Database,
	recipient: &Recipient,
	file_tag: u8,
) -> Result<Option<V1File>> {
	let meta_key = v1_meta_key(file_tag).to_vec();
	let (meta_keys, meta_values, _) =
		crate::actor_kv::get(db, recipient, vec![meta_key.clone()]).await?;

	if meta_keys.is_empty() && !v1_file_exists(db, recipient, file_tag).await? {
		return Ok(None);
	}
	ensure!(
		!meta_keys.is_empty(),
		"sqlite v1 file tag {file_tag} has chunks but no metadata"
	);
	ensure!(
		meta_keys.len() == 1 && meta_keys[0] == meta_key,
		"unexpected sqlite v1 metadata layout for file tag {file_tag}"
	);

	let size_bytes = decode_v1_meta(&meta_values[0])
		.with_context(|| format!("decode sqlite v1 metadata for file tag {file_tag}"))?;
	ensure!(
		size_bytes <= SQLITE_V1_MAX_MIGRATION_BYTES,
		"sqlite v1 file tag {file_tag} exceeded migration limit of {} bytes",
		SQLITE_V1_MAX_MIGRATION_BYTES
	);
	let expected_chunks = size_bytes.div_ceil(SQLITE_V1_CHUNK_SIZE as u64);
	let chunk_limit = usize::try_from(expected_chunks)
		.context("sqlite v1 expected chunk count exceeded usize")?
		.checked_add(1)
		.context("sqlite v1 chunk limit overflow")?
		.max(1);
	let (chunk_keys, chunk_values, _) = crate::actor_kv::list(
		db,
		recipient,
		protocol::KvListQuery::KvListPrefixQuery(protocol::KvListPrefixQuery {
			key: v1_chunk_prefix(file_tag).to_vec(),
		}),
		false,
		Some(chunk_limit),
	)
	.await?;
	let mut chunks = chunk_keys
		.into_iter()
		.zip(chunk_values.into_iter())
		.map(|(key, value)| {
			let chunk_idx = decode_v1_chunk_index(file_tag, &key)?;
			Ok((chunk_idx, value))
		})
		.collect::<Result<Vec<_>>>()?;
	chunks.sort_by_key(|(chunk_idx, _)| *chunk_idx);

	let bytes = rebuild_v1_file(
		size_bytes,
		expected_chunks
			.try_into()
			.context("sqlite v1 expected chunk count exceeded usize")?,
		&chunks,
	)
	.with_context(|| format!("rebuild sqlite v1 file tag {file_tag}"))?;

	Ok(Some(V1File { bytes }))
}

fn validate_v1_main(actor_id: &str, main: V1File) -> Result<RecoveredDb> {
	let bytes = main.bytes;
	if bytes.is_empty() {
		return Ok(RecoveredDb {
			bytes,
			total_pages: 0,
		});
	}

	ensure!(
		bytes.len() >= SQLITE_MAGIC.len() + 2,
		"sqlite v1 main file too small for actor {actor_id}"
	);
	ensure!(
		&bytes[..SQLITE_MAGIC.len()] == SQLITE_MAGIC,
		"sqlite v1 magic bytes mismatch for actor {actor_id}"
	);
	let raw_page_size = u16::from_be_bytes([bytes[16], bytes[17]]);
	let page_size = if raw_page_size == 1 {
		65_536_u32
	} else {
		u32::from(raw_page_size)
	};
	ensure!(
		(512..=65_536).contains(&page_size),
		"sqlite v1 page size {page_size} is outside the supported range for actor {actor_id}"
	);
	ensure!(
		page_size == SQLITE_PAGE_SIZE,
		"sqlite v1 page size {page_size} is not supported by sqlite v2 for actor {actor_id}"
	);
	ensure!(
		bytes.len() % page_size as usize == 0,
		"sqlite v1 database size {} is not page aligned to {} for actor {actor_id}",
		bytes.len(),
		page_size
	);

	Ok(RecoveredDb {
		total_pages: (bytes.len() / page_size as usize) as u32,
		bytes,
	})
}

fn decode_v1_meta(bytes: &[u8]) -> Result<u64> {
	ensure!(
		bytes.len() == SQLITE_V1_META_LEN,
		"sqlite v1 metadata had invalid length {}",
		bytes.len()
	);
	let version = u16::from_le_bytes(
		bytes[..2]
			.try_into()
			.expect("sqlite v1 metadata version bytes should exist"),
	);
	ensure!(
		version == SQLITE_V1_META_VERSION,
		"unsupported sqlite v1 metadata version {version}"
	);
	Ok(u64::from_le_bytes(
		bytes[2..10]
			.try_into()
			.expect("sqlite v1 metadata size bytes should exist"),
	))
}

fn rebuild_v1_file(
	size_bytes: u64,
	expected_chunks: usize,
	chunks: &[(u32, Vec<u8>)],
) -> Result<Vec<u8>> {
	let size_bytes: usize = size_bytes
		.try_into()
		.context("sqlite v1 file exceeded usize")?;
	ensure!(
		chunks.len() == expected_chunks,
		"sqlite v1 file expected {expected_chunks} chunks for size {size_bytes}, found {}",
		chunks.len()
	);
	let mut bytes = vec![0; size_bytes];

	for (expected_chunk_idx, (chunk_idx, chunk)) in chunks.iter().enumerate() {
		ensure!(
			*chunk_idx == expected_chunk_idx as u32,
			"sqlite v1 file missing or duplicated chunk at index {expected_chunk_idx}"
		);
		ensure!(
			chunk.len() <= SQLITE_V1_CHUNK_SIZE,
			"sqlite v1 chunk {chunk_idx} exceeded {} bytes",
			SQLITE_V1_CHUNK_SIZE
		);
		let start = (*chunk_idx as usize)
			.checked_mul(SQLITE_V1_CHUNK_SIZE)
			.context("sqlite v1 chunk offset overflow")?;
		let end = start
			.checked_add(chunk.len())
			.context("sqlite v1 chunk end overflow")?;
		ensure!(
			end <= bytes.len(),
			"sqlite v1 chunk {chunk_idx} overflowed file size {}",
			bytes.len()
		);
		bytes[start..end].copy_from_slice(chunk);
	}

	Ok(bytes)
}

fn decode_v1_chunk_index(file_tag: u8, key: &[u8]) -> Result<u32> {
	let prefix = v1_chunk_prefix(file_tag);
	ensure!(
		key.starts_with(&prefix),
		"sqlite v1 chunk key for file tag {file_tag} had the wrong prefix"
	);
	ensure!(
		key.len() == prefix.len() + 4,
		"sqlite v1 chunk key for file tag {file_tag} had invalid length {}",
		key.len()
	);

	Ok(u32::from_be_bytes(
		key[prefix.len()..]
			.try_into()
			.expect("sqlite v1 chunk key index bytes should exist"),
	))
}

fn v1_meta_key(file_tag: u8) -> [u8; 4] {
	[
		SQLITE_V1_PREFIX,
		SQLITE_V1_SCHEMA_VERSION,
		SQLITE_V1_META_PREFIX,
		file_tag,
	]
}

fn v1_chunk_prefix(file_tag: u8) -> [u8; 4] {
	[
		SQLITE_V1_PREFIX,
		SQLITE_V1_SCHEMA_VERSION,
		SQLITE_V1_CHUNK_PREFIX,
		file_tag,
	]
}

struct V1File {
	bytes: Vec<u8>,
}

struct RecoveredDb {
	bytes: Vec<u8>,
	total_pages: u32,
}
