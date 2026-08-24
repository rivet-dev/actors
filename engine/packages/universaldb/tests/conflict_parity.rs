//! FoundationDB conflict-semantics parity for the non-FDB drivers.
//!
//! FDB resolves conflicts directionally: a transaction aborts when something it *read* was *written*
//! under it. Reads are never the aggressor, blind write-vs-write does not conflict, and a read-only
//! transaction is never committed at all so it can never abort.

use std::{pin::Pin, sync::Arc};

use rivet_test_deps_docker::TestDatabase;
use tokio::sync::Notify;
use universaldb::{Database, utils::IsolationLevel::*};
use uuid::Uuid;

const KEY: &[u8] = b"conflict_parity/key";
const OTHER_KEY: &[u8] = b"conflict_parity/other";

#[tokio::test]
async fn rocksdb_conflict_parity() {
	let _ = tracing_subscriber::fmt::try_init();

	run_all_tests(&|| async {
		let (db_config, _docker_config) = TestDatabase::FileSystem
			.config(Uuid::new_v4(), 1)
			.await
			.unwrap();
		let rivet_config::config::Database::FileSystem(fs_config) = db_config else {
			unreachable!()
		};

		let driver = universaldb::driver::RocksDbDatabaseDriver::new(fs_config.path)
			.await
			.unwrap();

		Database::new(Arc::new(driver))
	})
	.await;
}

#[tokio::test]
async fn postgres_conflict_parity() {
	let _ = tracing_subscriber::fmt::try_init();

	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();

	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};
	let connection_string = postgres_config.url.read().clone();

	wait_for_postgres(&connection_string).await;

	// Every database in this test shares one Postgres, and the leader lease is exclusive, so the
	// drivers are created and shut down one at a time rather than through `run_all_tests`.
	let cases: [(&str, fn(Database) -> BoxFut); 5] = [
		("read_only_txn_does_not_abort_a_writer", |db| {
			Box::pin(read_only_txn_does_not_abort_a_writer(db))
		}),
		("read_only_txn_never_conflicts", |db| {
			Box::pin(read_only_txn_never_conflicts(db))
		}),
		("reads_are_repeatable_within_a_txn", |db| {
			Box::pin(reads_are_repeatable_within_a_txn(db))
		}),
		("blind_write_vs_write_does_not_conflict", |db| {
			Box::pin(blind_write_vs_write_does_not_conflict(db))
		}),
		("writer_conflicts_when_a_key_it_read_was_written", |db| {
			Box::pin(writer_conflicts_when_a_key_it_read_was_written(db))
		}),
	];

	for (name, case) in cases {
		tracing::info!(name, "running postgres conflict parity case");

		let driver = universaldb::driver::PostgresDatabaseDriver::new_with_config(
			universaldb::driver::postgres::PostgresConfig::new(connection_string.clone()),
		)
		.await
		.unwrap();
		let db = Database::new(Arc::new(driver));

		db.txn("clear", |tx| async move {
			tx.clear(KEY);
			tx.clear(OTHER_KEY);
			Ok(())
		})
		.await
		.unwrap();

		case(db.clone()).await;

		db.shutdown().await;
	}
}

type BoxFut = Pin<Box<dyn Future<Output = ()>>>;

/// Block until the freshly started Postgres container accepts connections. The container reports
/// started before the server is listening.
async fn wait_for_postgres(connection_string: &str) {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
	loop {
		match tokio_postgres::connect(connection_string, tokio_postgres::NoTls).await {
			Ok((_client, connection)) => {
				drop(connection);
				return;
			}
			Err(err) => {
				assert!(
					std::time::Instant::now() < deadline,
					"postgres never became reachable: {err}"
				);
				tokio::time::sleep(std::time::Duration::from_millis(250)).await;
			}
		}
	}
}

/// Every case runs against a freshly created database because they change the retry limit and race
/// two transactions against one key.
async fn run_all_tests<F, Fut>(new_db: &F)
where
	F: Fn() -> Fut,
	Fut: Future<Output = Database>,
{
	read_only_txn_does_not_abort_a_writer(new_db().await).await;
	read_only_txn_never_conflicts(new_db().await).await;
	reads_are_repeatable_within_a_txn(new_db().await).await;
	blind_write_vs_write_does_not_conflict(new_db().await).await;
	writer_conflicts_when_a_key_it_read_was_written(new_db().await).await;
}

/// A transaction that only read a key must not abort a writer of that key, even though their version
/// windows overlap. FDB would commit the writer: it checks the writer's reads against writes, and the
/// reader wrote nothing.
async fn read_only_txn_does_not_abort_a_writer(db: Database) {
	db.txn_retry_limit(1).unwrap();

	let writer_started = Arc::new(Notify::new());
	let read_committed = Arc::new(Notify::new());

	let writer = {
		let db = db.clone();
		let writer_started = writer_started.clone();
		let read_committed = read_committed.clone();

		tokio::spawn(async move {
			db.txn("writer", |tx| {
				let writer_started = writer_started.clone();
				let read_committed = read_committed.clone();

				async move {
					tx.set(KEY, b"written");

					// The write is staged, so the reader can now open a transaction whose version
					// window overlaps this one.
					writer_started.notify_one();
					read_committed.notified().await;

					Ok(())
				}
			})
			.await
		})
	};

	// Open the read-only transaction after the writer so both windows overlap.
	writer_started.notified().await;
	db.txn("reader", |tx| async move {
		tx.get(KEY, Serializable).await?;
		Ok(())
	})
	.await
	.expect("read-only transaction should commit");
	read_committed.notify_one();

	writer
		.await
		.unwrap()
		.expect("a concurrent read must not abort a writer");
}

/// A read-only transaction is never committed, so a write landing under it cannot abort it.
async fn read_only_txn_never_conflicts(db: Database) {
	db.txn_retry_limit(1).unwrap();

	let read_taken = Arc::new(Notify::new());
	let write_committed = Arc::new(Notify::new());

	let reader = {
		let db = db.clone();
		let read_taken = read_taken.clone();
		let write_committed = write_committed.clone();

		tokio::spawn(async move {
			db.txn("reader", |tx| {
				let read_taken = read_taken.clone();
				let write_committed = write_committed.clone();

				async move {
					tx.get(KEY, Serializable).await?;

					read_taken.notify_one();
					write_committed.notified().await;

					Ok(())
				}
			})
			.await
		})
	};

	read_taken.notified().await;
	db.txn("writer", |tx| async move {
		tx.set(KEY, b"written");
		Ok(())
	})
	.await
	.unwrap();
	write_committed.notify_one();

	reader
		.await
		.unwrap()
		.expect("a read-only transaction must never conflict");
}

/// All reads in a transaction come from one point in time, so a commit landing mid-transaction is
/// never partially visible.
async fn reads_are_repeatable_within_a_txn(db: Database) {
	db.txn_retry_limit(1).unwrap();

	db.txn("seed", |tx| async move {
		tx.set(KEY, b"first");
		Ok(())
	})
	.await
	.unwrap();

	let read_taken = Arc::new(Notify::new());
	let write_committed = Arc::new(Notify::new());

	let reader = {
		let db = db.clone();
		let read_taken = read_taken.clone();
		let write_committed = write_committed.clone();

		tokio::spawn(async move {
			db.txn("reader", |tx| {
				let read_taken = read_taken.clone();
				let write_committed = write_committed.clone();

				async move {
					let before = tx.get(KEY, Serializable).await?;

					read_taken.notify_one();
					write_committed.notified().await;

					let after = tx.get(KEY, Serializable).await?;

					Ok((before, after))
				}
			})
			.await
		})
	};

	read_taken.notified().await;
	db.txn("writer", |tx| async move {
		tx.set(KEY, b"second");
		Ok(())
	})
	.await
	.unwrap();
	write_committed.notify_one();

	let (before, after) = reader.await.unwrap().unwrap();
	assert_eq!(
		before.as_ref().map(|v| v.as_slice()),
		Some(b"first".as_slice()),
		"the first read should see the seeded value"
	);
	assert_eq!(
		after, before,
		"a commit landing mid-transaction must not be visible to a later read"
	);
}

/// Neither transaction read what the other wrote, so neither aborts.
async fn blind_write_vs_write_does_not_conflict(db: Database) {
	db.txn_retry_limit(1).unwrap();

	let first_staged = Arc::new(Notify::new());
	let second_committed = Arc::new(Notify::new());

	let first = {
		let db = db.clone();
		let first_staged = first_staged.clone();
		let second_committed = second_committed.clone();

		tokio::spawn(async move {
			db.txn("first", |tx| {
				let first_staged = first_staged.clone();
				let second_committed = second_committed.clone();

				async move {
					tx.set(KEY, b"first");

					first_staged.notify_one();
					second_committed.notified().await;

					Ok(())
				}
			})
			.await
		})
	};

	first_staged.notified().await;
	db.txn("second", |tx| async move {
		tx.set(KEY, b"second");
		Ok(())
	})
	.await
	.unwrap();
	second_committed.notify_one();

	first
		.await
		.unwrap()
		.expect("blind write-vs-write must not conflict");
}

/// The one case that must still abort: a transaction that writes and whose read was invalidated by a
/// write committed inside its version window.
async fn writer_conflicts_when_a_key_it_read_was_written(db: Database) {
	db.txn_retry_limit(1).unwrap();

	let read_taken = Arc::new(Notify::new());
	let write_committed = Arc::new(Notify::new());

	let reader = {
		let db = db.clone();
		let read_taken = read_taken.clone();
		let write_committed = write_committed.clone();

		tokio::spawn(async move {
			db.txn("read_then_write", |tx| {
				let read_taken = read_taken.clone();
				let write_committed = write_committed.clone();

				async move {
					tx.get(KEY, Serializable).await?;
					tx.set(OTHER_KEY, b"derived");

					read_taken.notify_one();
					write_committed.notified().await;

					Ok(())
				}
			})
			.await
		})
	};

	read_taken.notified().await;
	db.txn("writer", |tx| async move {
		tx.set(KEY, b"invalidated");
		Ok(())
	})
	.await
	.unwrap();
	write_committed.notify_one();

	let err = reader
		.await
		.unwrap()
		.expect_err("a write to a key this transaction read must abort it");
	assert!(
		err.chain().any(|x| matches!(
			x.downcast_ref::<universaldb::error::DatabaseError>(),
			Some(universaldb::error::DatabaseError::MaxRetriesReached)
		)),
		"expected the conflict to exhaust retries, got {err:?}"
	);
}
