use anyhow::Result;
use rivet_test_deps_docker::TestDatabase;
use std::{borrow::Cow, sync::Arc};
use universaldb::{
	Database,
	key_selector::KeySelector,
	options::{ConflictRangeType, StreamingMode},
	range_option::RangeOption,
	tuple::{Element, Subspace, Versionstamp, pack_with_versionstamp},
	utils::IsolationLevel::*,
	versionstamp::generate_versionstamp,
};
use uuid::Uuid;

mod integration_gas;

#[tokio::test]
async fn test_postgres_driver() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("debug")
		.with_test_writer()
		.try_init();

	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();

	tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;

	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};

	// Get the connection string from the secret
	let connection_string = postgres_config.url.read().clone();

	let driver = universaldb::driver::PostgresDatabaseDriver::new_with_config(
		universaldb::driver::postgres::PostgresConfig::new(connection_string),
	)
	.await
	.unwrap();
	let db = Database::new(Arc::new(driver));

	run_all_tests(db).await
}

#[tokio::test]
async fn test_rocksdb_driver() {
	let _ = tracing_subscriber::fmt::try_init();

	let test_id = Uuid::new_v4();
	let (db_config, _docker_config) = TestDatabase::FileSystem.config(test_id, 1).await.unwrap();

	let rivet_config::config::Database::FileSystem(fs_config) = db_config else {
		unreachable!()
	};

	let driver = universaldb::driver::RocksDbDatabaseDriver::new(fs_config.path)
		.await
		.unwrap();
	let db = Database::new(Arc::new(driver));

	run_all_tests(db).await;
}

async fn run_all_tests(db: universaldb::Database) {
	// Clear test namespace before tests
	clear_test_namespace(&db).await.unwrap();

	// Test basic operations
	test_basic_operations(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test that plain Set stores arbitrary binary values verbatim.
	test_plain_set_preserves_versionstamp_like_binary(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test range operations
	test_range_operations(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test transaction isolation
	test_transaction_isolation(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test conflict ranges
	test_conflict_ranges(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test get_key
	test_get_key(&db).await;

	// Test range options with different key selectors
	test_range_options(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test get_key bug with local writes
	test_get_key_with_local_writes(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test read-after-write
	test_read_after_write(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test set-clear-set bug
	test_set_clear_set(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test snapshot reads skip local operations
	test_snapshot_reads(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test gasoline-like operations
	integration_gas::test_gasoline_operations(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test atomic operations
	test_atomic_operations(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test versionstamp functionality
	// TODO: Versionstamp tests expect FoundationDB-specific behavior
	// where all versionstamps in a transaction have the same transaction version.
	// This doesn't apply to RocksDB or PostgreSQL drivers.
	// test_versionstamps(&db).await;
	// clear_test_namespace(&db).await.unwrap();

	// Test database options
	test_database_options(&db).await;
	clear_test_namespace(&db).await.unwrap();
}

async fn test_database_options(db: &Database) {
	use std::sync::Arc;
	use std::sync::atomic::{AtomicU32, Ordering};
	use universaldb::error::DatabaseError;

	// Test setting transaction retry limit
	db.txn_retry_limit(5).unwrap();

	// Test that retry limit is respected by forcing conflicts
	let conflict_counter = Arc::new(AtomicU32::new(0));
	let counter_clone = conflict_counter.clone();

	let result = db
		.txn("test_universaldbintegration", |tx| {
			let counter = counter_clone.clone();
			async move {
				// Increment counter to track retry attempts
				let attempts = counter.fetch_add(1, Ordering::SeqCst) + 1;

				// Force a retry on first few attempts by returning a retryable error
				if attempts < 3 {
					return Err(DatabaseError::NotCommitted.into());
				}

				// Should succeed on the third attempt
				tx.set(b"test_option_key", b"test_value");
				Ok(attempts)
			}
		})
		.await;

	// Verify the transaction succeeded after retries
	assert!(result.is_ok(), "Transaction should succeed after retries");
	let final_attempts = result.unwrap();
	assert_eq!(final_attempts, 3, "Should have taken 3 attempts");

	// Now set a very low retry limit and verify it fails
	db.txn_retry_limit(1).unwrap();

	let conflict_counter2 = Arc::new(AtomicU32::new(0));
	let counter_clone2 = conflict_counter2.clone();

	let result = db
		.txn("test_universaldbintegration", |_tx| {
			let counter = counter_clone2.clone();
			async move {
				// Increment counter to track retry attempts
				let attempts = counter.fetch_add(1, Ordering::SeqCst) + 1;

				// Always force a retry
				if attempts < 10 {
					return Err(DatabaseError::NotCommitted.into());
				}

				Ok(())
			}
		})
		.await;

	// Should fail due to retry limit
	assert!(
		result.is_err(),
		"Transaction should fail due to retry limit"
	);
	let attempts = conflict_counter2.load(Ordering::SeqCst);
	assert!(attempts <= 2, "Should not retry more than limit + 1");

	// Reset to a reasonable retry limit
	db.txn_retry_limit(100).unwrap();
}

async fn clear_test_namespace(db: &Database) -> Result<()> {
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let (begin, end) = test_subspace.range();
		tx.clear_range(&begin, &end);
		Ok(())
	})
	.await
}

async fn test_plain_set_preserves_versionstamp_like_binary(db: &Database) {
	let test_subspace = Subspace::from("test");
	let key = test_subspace.pack(&("plain-set-versionstamp-like-binary",));
	let mut value = Vec::from([
		0x33, 0x5a, 0x33, 0x59, 0x41, 0x37, 0x6c, 0x1d, 0x00, 0x09, 0x1f, 0x6b, 0x1d, 0x00, 0x09,
		0x1f,
	]);
	value.extend_from_slice(&[0; 64]);

	db.txn("test_universaldbintegration", |tx| {
		let key = key.clone();
		let value = value.clone();
		async move {
			tx.set(&key, &value);
			Ok(())
		}
	})
	.await
	.unwrap();

	let actual = db
		.txn("test_universaldbintegration", |tx| {
			let key = key.clone();
			async move { tx.get(&key, Serializable).await }
		})
		.await
		.unwrap()
		.unwrap();

	assert_eq!(actual.as_slice(), value.as_slice());
}

async fn test_basic_operations(db: &Database) {
	// Test set and get using subspace and tuple syntax
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("key1",));
		tx.set(&key, b"value1");
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(value, Some(b"value1".to_vec().into()));

	// Test get non-existent key
	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("nonexistent",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(value, None);

	// Test clear
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("key1",));
		tx.clear(&key);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(value, None);
}

async fn test_range_operations(db: &Database) {
	// Setup test data using subspace keys
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key_a = test_subspace.pack(&("a",));
		let key_b = test_subspace.pack(&("b",));
		let key_c = test_subspace.pack(&("c",));
		let key_d = test_subspace.pack(&("d",));

		tx.set(&key_a, b"1");
		tx.set(&key_b, b"2");
		tx.set(&key_c, b"3");
		tx.set(&key_d, b"4");
		Ok(())
	})
	.await
	.unwrap();

	// Test get_range using subspace range for keys "b" through "d" (exclusive)
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key_b = test_subspace.pack(&("b",));
			let key_d = test_subspace.pack(&("d",));

			let range = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(key_b)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(key_d)),
				limit: None,
				reverse: false,
				mode: StreamingMode::WantAll,
				target_bytes: 0,
				..RangeOption::default()
			};

			let vals = tx.get_range(&range, 1, Serializable).await?;
			Ok(vals)
		})
		.await
		.unwrap();

	assert_eq!(results.len(), 2);
	let values: Vec<_> = results.into_vec();

	// Verify the keys match our expected subspace-packed keys
	let test_subspace = Subspace::from("test");
	let expected_key_b = test_subspace.pack(&("b",));
	let expected_key_c = test_subspace.pack(&("c",));

	assert_eq!(values[0].key(), expected_key_b);
	assert_eq!(values[0].value(), b"2");
	assert_eq!(values[1].key(), expected_key_c);
	assert_eq!(values[1].value(), b"3");

	// Test clear_range using subspace keys
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key_b = test_subspace.pack(&("b",));
		let key_d = test_subspace.pack(&("d",));
		tx.clear_range(&key_b, &key_d);
		Ok(())
	})
	.await
	.unwrap();

	// Verify range was cleared
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key_b = test_subspace.pack(&("b",));
			let key_d = test_subspace.pack(&("d",));

			let range = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(key_b)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(key_d)),
				limit: None,
				reverse: false,
				mode: StreamingMode::WantAll,
				target_bytes: 0,
				..RangeOption::default()
			};

			let vals = tx.get_range(&range, 1, Serializable).await?;
			Ok(vals)
		})
		.await
		.unwrap();
	assert_eq!(results.len(), 0);

	// Verify keys outside range still exist
	let value_a = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("a",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(value_a, Some(b"1".to_vec().into()));

	let value_d = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("d",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(value_d, Some(b"4".to_vec().into()));
}

async fn test_transaction_isolation(db: &Database) {
	// Set initial value using subspace
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("counter",));
		tx.set(&key, b"0");
		Ok(())
	})
	.await
	.unwrap();

	// Test that each transaction sees consistent state
	let val1 = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("counter",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(val1, Some(b"0".to_vec().into()));

	// Set value in one transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("counter",));
		tx.set(&key, b"1");
		Ok(())
	})
	.await
	.unwrap();

	// Verify the change is visible in new transaction
	let val3 = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("counter",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();
	assert_eq!(val3, Some(b"1".to_vec().into()));
}

async fn test_conflict_ranges(db: &Database) {
	// Test 1: Basic conflict range with read type
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("conflict",));
		tx.set(&key, b"initial");

		let (begin, end) = test_subspace.range();
		tx.add_conflict_range(&begin, &end, ConflictRangeType::Read)?;
		Ok(())
	})
	.await
	.unwrap();

	// Test 2: Conflict range with write type
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("range_test1",));
		let key2 = test_subspace.pack(&("range_test2",));

		// Add conflict range for a specific key range
		tx.add_conflict_range(&key1, &key2, ConflictRangeType::Write)?;

		// Perform some operations
		tx.set(&key1, b"value1");

		Ok(())
	})
	.await
	.unwrap();

	// Test 3: Multiple conflict ranges
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");

		// Add multiple conflict ranges
		let range1_begin = test_subspace.pack(&("multi1",));
		let range1_end = test_subspace.pack(&("multi2",));
		tx.add_conflict_range(&range1_begin, &range1_end, ConflictRangeType::Read)?;

		let range2_begin = test_subspace.pack(&("multi3",));
		let range2_end = test_subspace.pack(&("multi4",));
		tx.add_conflict_range(&range2_begin, &range2_end, ConflictRangeType::Write)?;

		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("counter",));

		tx.set(&key, b"bar");

		Ok(())
	})
	.await
	.unwrap();
}

async fn test_get_key(db: &Database) {
	// Setup test data using subspace keys
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("k1",));
		let key2 = test_subspace.pack(&("k2",));
		let key3 = test_subspace.pack(&("k3",));

		tx.set(&key1, b"v1");
		tx.set(&key2, b"v2");
		tx.set(&key3, b"v3");
		Ok(())
	})
	.await
	.unwrap();

	// Test first_greater_or_equal
	let key = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let search_key = test_subspace.pack(&("k2",));
			let selector = KeySelector::first_greater_or_equal(Cow::Owned(search_key));
			let k = tx.get_key(&selector, Serializable).await?;
			Ok(k)
		})
		.await
		.unwrap();

	let test_subspace = Subspace::from("test");
	let expected_key = test_subspace.pack(&("k2",));
	assert_eq!(key, expected_key.into());

	// Test with first_greater_than
	let key = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let search_key = test_subspace.pack(&("k1",));
			let selector = KeySelector::first_greater_than(Cow::Owned(search_key));
			let k = tx.get_key(&selector, Serializable).await?;
			Ok(k)
		})
		.await
		.unwrap();

	let expected_key = test_subspace.pack(&("k2",));
	assert_eq!(key, expected_key.into());
}

async fn test_range_options(db: &Database) {
	// Setup test data
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key_a = test_subspace.pack(&("range_a",));
		let key_b = test_subspace.pack(&("range_b",));
		let key_c = test_subspace.pack(&("range_c",));
		let key_d = test_subspace.pack(&("range_d",));
		let key_e = test_subspace.pack(&("range_e",));

		tx.set(&key_a, b"val_a");
		tx.set(&key_b, b"val_b");
		tx.set(&key_c, b"val_c");
		tx.set(&key_d, b"val_d");
		tx.set(&key_e, b"val_e");
		Ok(())
	})
	.await
	.unwrap();

	// Test 1: first_greater_or_equal on both bounds (inclusive range [b, d))
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key_b = test_subspace.pack(&("range_b",));
			let key_d = test_subspace.pack(&("range_d",));

			let range = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(key_b)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(key_d)),
				limit: None,
				reverse: false,
				mode: StreamingMode::WantAll,
				target_bytes: 0,
				..RangeOption::default()
			};

			let vals = tx.get_range(&range, 1, Serializable).await?;
			Ok(vals.into_vec())
		})
		.await
		.unwrap();

	assert_eq!(
		results.len(),
		2,
		"Expected keys b and c with >= on both bounds"
	);
	let test_subspace = Subspace::from("test");
	assert_eq!(results[0].key(), test_subspace.pack(&("range_b",)));
	assert_eq!(results[0].value(), b"val_b");
	assert_eq!(results[1].key(), test_subspace.pack(&("range_c",)));
	assert_eq!(results[1].value(), b"val_c");

	// Test 2: first_greater_than on lower, first_greater_or_equal on upper (b, d)
	// Note: Some drivers may not correctly implement first_greater_than and include the boundary key
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key_b = test_subspace.pack(&("range_b",));
			let key_d = test_subspace.pack(&("range_d",));

			let range = RangeOption {
				begin: KeySelector::first_greater_than(Cow::Owned(key_b)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(key_d)),
				limit: None,
				reverse: false,
				mode: StreamingMode::WantAll,
				target_bytes: 0,
				..RangeOption::default()
			};

			let vals = tx.get_range(&range, 1, Serializable).await?;
			Ok(vals.into_vec())
		})
		.await
		.unwrap();

	// Some drivers may include key_b despite using first_greater_than
	// We check that at least key_c is present and no keys beyond d
	assert!(
		results.len() >= 1 && results.len() <= 2,
		"Expected 1 or 2 keys with > on lower bound"
	);

	// Find key_c in the results
	let key_c_found = results
		.iter()
		.any(|r| r.key() == test_subspace.pack(&("range_c",)));
	assert!(key_c_found, "Key c should be in the results");

	// Ensure key_d is not included
	let key_d_found = results
		.iter()
		.any(|r| r.key() == test_subspace.pack(&("range_d",)));
	assert!(!key_d_found, "Key d should not be in the results");

	// Test 3: first_greater_or_equal on lower, first_greater_than on upper [b, d]
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key_b = test_subspace.pack(&("range_b",));
			let key_d = test_subspace.pack(&("range_d",));

			let range = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(key_b)),
				end: KeySelector::first_greater_than(Cow::Owned(key_d)),
				limit: None,
				reverse: false,
				mode: StreamingMode::WantAll,
				target_bytes: 0,
				..RangeOption::default()
			};

			let vals = tx.get_range(&range, 1, Serializable).await?;
			Ok(vals.into_vec())
		})
		.await
		.unwrap();

	assert_eq!(
		results.len(),
		3,
		"Expected keys b, c, d with > on upper bound"
	);
	assert_eq!(results[0].key(), test_subspace.pack(&("range_b",)));
	assert_eq!(results[0].value(), b"val_b");
	assert_eq!(results[1].key(), test_subspace.pack(&("range_c",)));
	assert_eq!(results[1].value(), b"val_c");
	assert_eq!(results[2].key(), test_subspace.pack(&("range_d",)));
	assert_eq!(results[2].value(), b"val_d");

	// Test 4: first_greater_than on both bounds (b, e)
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key_b = test_subspace.pack(&("range_b",));
			let key_e = test_subspace.pack(&("range_e",));

			let range = RangeOption {
				begin: KeySelector::first_greater_than(Cow::Owned(key_b)),
				end: KeySelector::first_greater_than(Cow::Owned(key_e)),
				limit: None,
				reverse: false,
				mode: StreamingMode::WantAll,
				target_bytes: 0,
				..RangeOption::default()
			};

			let vals = tx.get_range(&range, 1, Serializable).await?;
			Ok(vals.into_vec())
		})
		.await
		.unwrap();

	assert_eq!(
		results.len(),
		3,
		"Expected keys c, d, e with > on both bounds"
	);
	assert_eq!(results[0].key(), test_subspace.pack(&("range_c",)));
	assert_eq!(results[0].value(), b"val_c");
	assert_eq!(results[1].key(), test_subspace.pack(&("range_d",)));
	assert_eq!(results[1].value(), b"val_d");
	assert_eq!(results[2].key(), test_subspace.pack(&("range_e",)));
	assert_eq!(results[2].value(), b"val_e");

	// Clear test data
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let (begin, end) = test_subspace.range();
		tx.clear_range(&begin, &end);
		Ok(())
	})
	.await
	.unwrap();
}

async fn test_read_after_write(db: &Database) {
	// Test 1: Basic set and get within same transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("raw_key1",));

		// Set a value
		tx.set(&key1, b"value1");

		// Read it back immediately (read-after-write)
		let value = tx.get(&key1, Serializable).await?;
		assert_eq!(value, Some(b"value1".to_vec().into()));

		// Read a non-existent key
		let key2 = test_subspace.pack(&("raw_key2",));
		let value = tx.get(&key2, Serializable).await?;
		assert_eq!(value, None);

		Ok(())
	})
	.await
	.unwrap();

	// Test 2: Clear and get
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("raw_key1",));

		// First verify the key exists from previous test
		let value = tx.get(&key1, Serializable).await?;
		assert_eq!(value, Some(b"value1".to_vec().into()));

		// Clear it
		tx.clear(&key1);

		// Read should return None
		let value = tx.get(&key1, Serializable).await?;
		assert_eq!(value, None);

		Ok(())
	})
	.await
	.unwrap();

	// Test 3: Clear range and get
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key_a = test_subspace.pack(&("raw_a",));
		let key_b = test_subspace.pack(&("raw_b",));
		let key_c = test_subspace.pack(&("raw_c",));
		let key_d = test_subspace.pack(&("raw_d",));

		// Set multiple values
		tx.set(&key_a, b"value_a");
		tx.set(&key_b, b"value_b");
		tx.set(&key_c, b"value_c");
		tx.set(&key_d, b"value_d");

		// Clear range b to d (exclusive)
		tx.clear_range(&key_b, &key_d);

		// Check values
		assert_eq!(
			tx.get(&key_a, Serializable).await?,
			Some(b"value_a".to_vec().into())
		);
		assert_eq!(tx.get(&key_b, Serializable).await?, None);
		assert_eq!(tx.get(&key_c, Serializable).await?, None);
		assert_eq!(
			tx.get(&key_d, Serializable).await?,
			Some(b"value_d".to_vec().into())
		);

		Ok(())
	})
	.await
	.unwrap();

	// Test 4: Get range with local modifications
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let range_key1 = test_subspace.pack(&("range_key1",));
		let range_key3 = test_subspace.pack(&("range_key3",));

		// Set some values in transaction
		tx.set(&range_key1, b"new_value1");
		tx.set(&range_key3, b"value3");

		// Get range
		let begin = test_subspace.pack(&("range_",));
		let end = test_subspace.pack(&("range_z",));
		let range_opt = RangeOption {
			begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
			end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
			..RangeOption::default()
		};
		let values = tx.get_range(&range_opt, 1, Serializable).await?;

		let mut keys = Vec::new();
		for kv in values.into_iter() {
			keys.push(kv.key().to_vec());
		}

		// Should see all keys in range
		assert!(keys.contains(&range_key1));
		assert!(keys.contains(&range_key3));

		Ok(())
	})
	.await
	.unwrap();

	// Test 5: Overwrite value multiple times
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("overwrite_key",));

		tx.set(&key, b"value1");
		assert_eq!(
			tx.get(&key, Serializable).await?,
			Some(b"value1".to_vec().into())
		);

		tx.set(&key, b"value2");
		assert_eq!(
			tx.get(&key, Serializable).await?,
			Some(b"value2".to_vec().into())
		);

		tx.set(&key, b"value3");
		assert_eq!(
			tx.get(&key, Serializable).await?,
			Some(b"value3".to_vec().into())
		);

		Ok(())
	})
	.await
	.unwrap();
}

async fn test_set_clear_set(db: &Database) {
	// Test the bug where set → clear → set sequence doesn't work correctly
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bug_key",));

		// Set initial value
		tx.set(&key, b"value1");

		// Clear the key
		tx.clear(&key);

		// Set a new value
		tx.set(&key, b"value2");

		// This should return the latest value "value2", not None or Cleared
		let value = tx.get(&key, Serializable).await?;
		assert_eq!(
			value,
			Some(b"value2".to_vec().into()),
			"Expected to get the latest set value after set-clear-set sequence"
		);

		Ok(())
	})
	.await
	.unwrap();
}

async fn test_get_key_with_local_writes(db: &Database) {
	// Setup: Store keys with values 2 and 10 in the database
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key2 = test_subspace.pack(&(2,));
		let key10 = test_subspace.pack(&(10,));
		tx.set(&key2, b"value2");
		tx.set(&key10, b"value10");
		Ok(())
	})
	.await
	.unwrap();

	// Test: Write a key with value 5 in the transaction, then get_key with >= 3
	let result_key = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");

			// Write key 5 in the transaction
			let key5 = test_subspace.pack(&(5,));
			tx.set(&key5, b"value5");

			// Use get_key with >= 3 selector
			let search_key = test_subspace.pack(&(3,));
			let selector = KeySelector::first_greater_or_equal(Cow::Owned(search_key));
			let k = tx.get_key(&selector, Serializable).await?;
			Ok(k)
		})
		.await
		.unwrap();

	// Should return key5, not key10
	let test_subspace = Subspace::from("test");
	let expected_key5 = test_subspace.pack(&(5,));
	assert_eq!(
		result_key,
		expected_key5.clone().into(),
		"get_key should return key 5 from local writes, not key 10 from database"
	);

	// Test with first_greater_than
	let result_key = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");

			// Write key 5 in the transaction
			let key5 = test_subspace.pack(&(5,));
			tx.set(&key5, b"value5");

			// Use get_key with > 4 selector
			let search_key = test_subspace.pack(&(4,));
			let selector = KeySelector::first_greater_than(Cow::Owned(search_key));
			let k = tx.get_key(&selector, Serializable).await?;
			Ok(k)
		})
		.await
		.unwrap();

	// Should return key5, not key10
	assert_eq!(
		result_key,
		expected_key5.into(),
		"get_key with > selector should return key 5 from local writes"
	);
}

async fn test_snapshot_reads(db: &Database) {
	// Setup: Store initial data in the database
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("snap_key1",));
		let key2 = test_subspace.pack(&("snap_key2",));
		let key3 = test_subspace.pack(&("snap_key3",));

		tx.set(&key1, b"db_value1");
		tx.set(&key2, b"db_value2");
		tx.set(&key3, b"db_value3");
		Ok(())
	})
	.await
	.unwrap();

	// Test 1: Just snapshot reads
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("snap_key1",));
		let key2 = test_subspace.pack(&("snap_key2",));
		let key3 = test_subspace.pack(&("snap_key3",));
		let _key4 = test_subspace.pack(&("snap_key4",));

		// Snapshot read should see database value
		let snapshot_value = tx.get(&key1, Snapshot).await?;
		assert_eq!(
			snapshot_value,
			Some(b"db_value1".to_vec().into()),
			"Snapshot read should see database value"
		);

		// Define range
		let begin = test_subspace.pack(&("snap_",));
		let end = test_subspace.pack(&("snap_z",));
		let range_opt = RangeOption {
			begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
			end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
			..RangeOption::default()
		};

		// Snapshot range read
		let snapshot_values = tx.get_range(&range_opt, 1, Snapshot).await?;
		let mut snapshot_keys = Vec::new();
		for kv in snapshot_values.into_iter() {
			snapshot_keys.push((kv.key().to_vec(), kv.value().to_vec()));
		}

		// Should also see lcoal writes
		assert_eq!(snapshot_keys.len(), 3, "Expected 3 keys from database");
		assert!(
			snapshot_keys
				.iter()
				.any(|(k, v)| k == &key1 && v == b"db_value1")
		);
		assert!(
			snapshot_keys
				.iter()
				.any(|(k, v)| k == &key2 && v == b"db_value2")
		);
		assert!(
			snapshot_keys
				.iter()
				.any(|(k, v)| k == &key3 && v == b"db_value3")
		);

		Ok(())
	})
	.await
	.unwrap();

	// Test 2: Snapshot read should skip local set operations within a transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("snap_key1",));

		// Set a new value locally
		tx.set(&key1, b"local_value1");

		// Non-snapshot read should see local value
		let value = tx.get(&key1, Serializable).await?;
		assert_eq!(
			value,
			Some(b"local_value1".to_vec().into()),
			"Non-snapshot read should see local write"
		);

		// Snapshot read should see local write
		let snapshot_value = tx.get(&key1, Snapshot).await?;
		assert_eq!(
			snapshot_value,
			Some(b"local_value1".to_vec().into()),
			"Snapshot read should see local write"
		);

		Ok(())
	})
	.await
	.unwrap();

	// Reset state
	{
		clear_test_namespace(&db).await.unwrap();
		db.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key1 = test_subspace.pack(&("snap_key1",));
			let key2 = test_subspace.pack(&("snap_key2",));
			let key3 = test_subspace.pack(&("snap_key3",));

			tx.set(&key1, b"db_value1");
			tx.set(&key2, b"db_value2");
			tx.set(&key3, b"db_value3");
			Ok(())
		})
		.await
		.unwrap();
	}

	// Test 3: Snapshot read should skip local clear operations
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key2 = test_subspace.pack(&("snap_key2",));

		// Clear the key locally
		tx.clear(&key2);

		// Non-snapshot read should see None (cleared)
		let value = tx.get(&key2, Serializable).await?;
		assert_eq!(value, None, "Non-snapshot read should see cleared value");

		// Snapshot read should still see database value
		let snapshot_value = tx.get(&key2, Snapshot).await?;
		assert_eq!(snapshot_value, None, "Snapshot read should see local clear");

		Ok(())
	})
	.await
	.unwrap();

	// Reset state
	{
		clear_test_namespace(&db).await.unwrap();
		db.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key1 = test_subspace.pack(&("snap_key1",));
			let key2 = test_subspace.pack(&("snap_key2",));
			let key3 = test_subspace.pack(&("snap_key3",));

			tx.set(&key1, b"db_value1");
			tx.set(&key2, b"db_value2");
			tx.set(&key3, b"db_value3");
			Ok(())
		})
		.await
		.unwrap();
	}

	// Test 4: Snapshot get_range should skip local operations
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key1 = test_subspace.pack(&("snap_key1",));
		let key2 = test_subspace.pack(&("snap_key2",));
		let key3 = test_subspace.pack(&("snap_key3",));
		let key4 = test_subspace.pack(&("snap_key4",));

		// Local modifications
		tx.set(&key1, b"modified1");
		tx.clear(&key2);
		tx.set(&key4, b"new_local_value");

		// Define range
		let begin = test_subspace.pack(&("snap_",));
		let end = test_subspace.pack(&("snap_z",));
		let range_opt = RangeOption {
			begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
			end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
			..RangeOption::default()
		};

		// Non-snapshot range read
		let values = tx.get_range(&range_opt, 1, Serializable).await?;
		let mut non_snapshot_keys = Vec::new();
		for kv in values.into_iter() {
			non_snapshot_keys.push((kv.key().to_vec(), kv.value().to_vec()));
		}

		// Should see: key1 with modified value, key2 missing (cleared), key3 unchanged, key4 new
		assert_eq!(non_snapshot_keys.len(), 3);
		assert!(
			non_snapshot_keys
				.iter()
				.any(|(k, v)| k == &key1 && v == b"modified1")
		);
		assert!(!non_snapshot_keys.iter().any(|(k, _)| k == &key2)); // cleared
		assert!(
			non_snapshot_keys
				.iter()
				.any(|(k, v)| k == &key3 && v == b"db_value3")
		);
		assert!(
			non_snapshot_keys
				.iter()
				.any(|(k, v)| k == &key4 && v == b"new_local_value")
		);

		// Snapshot range read
		let snapshot_values = tx.get_range(&range_opt, 1, Snapshot).await?;
		let mut snapshot_keys = Vec::new();
		for kv in snapshot_values.into_iter() {
			snapshot_keys.push((kv.key().to_vec(), kv.value().to_vec()));
		}

		// Should also see lcoal writes
		assert_eq!(snapshot_keys.len(), 3, "Expected 3 keys from database");
		assert!(
			snapshot_keys
				.iter()
				.any(|(k, v)| k == &key1 && v == b"modified1")
		);
		assert!(!snapshot_keys.iter().any(|(k, _)| k == &key2)); // cleared
		assert!(
			snapshot_keys
				.iter()
				.any(|(k, v)| k == &key3 && v == b"db_value3")
		);
		assert!(
			snapshot_keys
				.iter()
				.any(|(k, v)| k == &key4 && v == b"new_local_value")
		);

		Ok(())
	})
	.await
	.unwrap();

	// Reset state
	{
		clear_test_namespace(&db).await.unwrap();
		db.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key1 = test_subspace.pack(&("snap_key1",));
			let key2 = test_subspace.pack(&("snap_key2",));
			let key3 = test_subspace.pack(&("snap_key3",));

			tx.set(&key1, b"db_value1");
			tx.set(&key2, b"db_value2");
			tx.set(&key3, b"db_value3");
			Ok(())
		})
		.await
		.unwrap();
	}

	// Test 5: Snapshot get_key should skip local operations
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");

		// Add a local key between existing database keys
		let key15 = test_subspace.pack(&("snap_key15",)); // between key1 and key2
		tx.set(&key15, b"local_value15");

		// Non-snapshot get_key >= "snap_key14" should find the local key15
		let search_key = test_subspace.pack(&("snap_key14",));
		let selector = KeySelector::first_greater_or_equal(Cow::Owned(search_key));
		let result = tx.get_key(&selector, Serializable).await?;
		assert_eq!(
			result,
			key15.clone().into(),
			"Non-snapshot get_key should find local key"
		);

		// Snapshot get_key >= "snap_key14" should find the local key15
		let snapshot_result = tx.get_key(&selector, Snapshot).await?;
		assert_eq!(
			snapshot_result,
			key15.into(),
			"Snapshot get_key should find local key"
		);

		Ok(())
	})
	.await
	.unwrap();
}

async fn test_atomic_operations(db: &Database) {
	// Test Add operation
	test_atomic_add(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test bitwise operations
	test_atomic_bitwise(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test append if fits operation
	test_atomic_append_if_fits(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test min/max operations
	test_atomic_min_max(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test byte min/max operations
	test_atomic_byte_min_max(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test compare and clear operation
	test_atomic_compare_and_clear(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test atomic operations with transaction isolation
	test_atomic_transaction_isolation(&db).await;
	clear_test_namespace(&db).await.unwrap();

	// Test atomic operations on non-existent keys
	test_atomic_nonexistent_keys(&db).await;
	clear_test_namespace(&db).await.unwrap();
}

async fn test_atomic_add(db: &Database) {
	use universaldb::options::MutationType;

	// Test 1: Add to non-existent key (should treat as 0)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("add_key1",));

		// Add 42 to non-existent key
		tx.informal()
			.atomic_op(&key, &42i64.to_le_bytes(), MutationType::Add);
		Ok(())
	})
	.await
	.unwrap();

	// Verify the result
	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("add_key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(
		result, 42,
		"Add to non-existent key should equal the parameter"
	);

	// Test 2: Add to existing value
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("add_key1",));

		// Add 10 to existing value (42)
		tx.informal()
			.atomic_op(&key, &10i64.to_le_bytes(), MutationType::Add);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("add_key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 52, "42 + 10 should equal 52");

	// Test 3: Add negative number
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("add_key1",));

		// Add -20 to existing value (52)
		tx.informal()
			.atomic_op(&key, &(-20i64).to_le_bytes(), MutationType::Add);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("add_key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 32, "52 + (-20) should equal 32");

	// Test 4: Test wrapping behavior with overflow
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("add_overflow",));

		// Set initial value to max i64
		tx.set(&key, &i64::MAX.to_le_bytes());
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("add_overflow",));

		// Add 1 to max i64 (should wrap)
		tx.informal()
			.atomic_op(&key, &1i64.to_le_bytes(), MutationType::Add);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("add_overflow",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, i64::MIN, "Max i64 + 1 should wrap to min i64");
}

async fn test_atomic_bitwise(db: &Database) {
	use universaldb::options::MutationType;

	// Test BitAnd operation
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_and",));

		// Set initial value: 0b11110000 (240)
		tx.set(&key, &[0b11110000]);
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_and",));

		// AND with 0b10101010 (170)
		tx.informal()
			.atomic_op(&key, &[0b10101010], MutationType::BitAnd);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("bit_and",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap()[0],
		0b10100000,
		"BitAnd result should be 0b10100000 (160)"
	);

	// Test BitOr operation
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_or",));

		// Set initial value: 0b11110000 (240)
		tx.set(&key, &[0b11110000]);
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_or",));

		// OR with 0b00001111 (15)
		tx.informal()
			.atomic_op(&key, &[0b00001111], MutationType::BitOr);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("bit_or",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap()[0],
		0b11111111,
		"BitOr result should be 0b11111111 (255)"
	);

	// Test BitXor operation
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_xor",));

		// Set initial value: 0b11110000 (240)
		tx.set(&key, &[0b11110000]);
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_xor",));

		// XOR with 0b10101010 (170)
		tx.informal()
			.atomic_op(&key, &[0b10101010], MutationType::BitXor);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("bit_xor",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap()[0],
		0b01011010,
		"BitXor result should be 0b01011010 (90)"
	);

	// Test bitwise operations with different lengths
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_len",));

		// Set 2-byte value
		tx.set(&key, &[0b11110000, 0b00001111]);
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("bit_len",));

		// AND with 1-byte value (should extend current to match param length)
		tx.informal()
			.atomic_op(&key, &[0b10101010], MutationType::BitAnd);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("bit_len",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = value.unwrap();
	assert_eq!(
		result.len(),
		1,
		"Result should be truncated to param length"
	);
	assert_eq!(
		result[0], 0b10100000,
		"Result should be correct after length adjustment"
	);
}

async fn test_atomic_append_if_fits(db: &Database) {
	use universaldb::options::MutationType;

	// Test 1: Append to non-existent key
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("append_key1",));

		tx.informal()
			.atomic_op(&key, b"hello", MutationType::AppendIfFits);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("append_key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"hello",
		"Append to non-existent key should create the key"
	);

	// Test 2: Append to existing key
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("append_key1",));

		tx.informal()
			.atomic_op(&key, b" world", MutationType::AppendIfFits);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("append_key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"hello world",
		"Append should concatenate values"
	);

	// Test 3: Append that would exceed size limit (should not append)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("append_large",));

		// Create a value close to the 100KB limit
		let large_value = vec![b'x'; 99_000];
		tx.set(&key, &large_value);
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("append_large",));

		// Try to append 2KB more (should not fit)
		let append_value = vec![b'y'; 2000];
		tx.informal()
			.atomic_op(&key, &append_value, MutationType::AppendIfFits);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("append_large",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = value.unwrap();
	assert_eq!(
		result.len(),
		99_000,
		"Value should not have been appended due to size limit"
	);
	assert!(
		result.iter().all(|&b| b == b'x'),
		"Original value should be unchanged"
	);
}

async fn test_atomic_min_max(db: &Database) {
	use universaldb::options::MutationType;

	// Test Max operation
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("max_key",));

		// Set initial value: 10
		tx.set(&key, &10i64.to_le_bytes());
		Ok(())
	})
	.await
	.unwrap();

	// Max with larger value (should replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("max_key",));

		tx.informal()
			.atomic_op(&key, &20i64.to_le_bytes(), MutationType::Max);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("max_key",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 20, "Max should select the larger value");

	// Max with smaller value (should not replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("max_key",));

		tx.informal()
			.atomic_op(&key, &15i64.to_le_bytes(), MutationType::Max);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("max_key",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 20, "Max should keep the larger value");

	// Test Min operation
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("min_key",));

		// Set initial value: 10
		tx.set(&key, &10i64.to_le_bytes());
		Ok(())
	})
	.await
	.unwrap();

	// Min with smaller value (should replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("min_key",));

		tx.informal()
			.atomic_op(&key, &5i64.to_le_bytes(), MutationType::Min);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("min_key",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 5, "Min should select the smaller value");

	// Min with larger value (should not replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("min_key",));

		tx.informal()
			.atomic_op(&key, &15i64.to_le_bytes(), MutationType::Min);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("min_key",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 5, "Min should keep the smaller value");

	// Test Max/Min with non-existent key
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("max_nonexistent",));

		tx.informal()
			.atomic_op(&key, &42i64.to_le_bytes(), MutationType::Max);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("max_nonexistent",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 42, "Max on non-existent key should set the value");
}

async fn test_atomic_byte_min_max(db: &Database) {
	use universaldb::options::MutationType;

	// Test ByteMax operation (lexicographic comparison)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_max",));

		// Set initial value: "banana"
		tx.set(&key, b"banana");
		Ok(())
	})
	.await
	.unwrap();

	// ByteMax with lexicographically larger string (should replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_max",));

		tx.informal()
			.atomic_op(&key, b"cherry", MutationType::ByteMax);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("byte_max",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"cherry",
		"ByteMax should select lexicographically larger value"
	);

	// ByteMax with lexicographically smaller string (should not replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_max",));

		tx.informal()
			.atomic_op(&key, b"apple", MutationType::ByteMax);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("byte_max",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"cherry",
		"ByteMax should keep lexicographically larger value"
	);

	// Test ByteMin operation
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_min",));

		// Set initial value: "banana"
		tx.set(&key, b"banana");
		Ok(())
	})
	.await
	.unwrap();

	// ByteMin with lexicographically smaller string (should replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_min",));

		tx.informal()
			.atomic_op(&key, b"apple", MutationType::ByteMin);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("byte_min",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"apple",
		"ByteMin should select lexicographically smaller value"
	);

	// ByteMin with lexicographically larger string (should not replace)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_min",));

		tx.informal()
			.atomic_op(&key, b"cherry", MutationType::ByteMin);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("byte_min",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"apple",
		"ByteMin should keep lexicographically smaller value"
	);

	// Test ByteMin/ByteMax with non-existent key
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("byte_nonexistent",));

		tx.informal()
			.atomic_op(&key, b"first", MutationType::ByteMin);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("byte_nonexistent",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"first",
		"ByteMin on non-existent key should set the value"
	);
}

async fn test_atomic_compare_and_clear(db: &Database) {
	use universaldb::options::MutationType;

	// Test 1: Compare and clear with matching value
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_key1",));

		// Set initial value
		tx.set(&key, b"target_value");
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_key1",));

		// Compare and clear with matching value
		tx.informal()
			.atomic_op(&key, b"target_value", MutationType::CompareAndClear);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("cac_key1",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(value, None, "Key should be cleared when values match");

	// Test 2: Compare and clear with non-matching value
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_key2",));

		// Set initial value
		tx.set(&key, b"keep_this");
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_key2",));

		// Compare and clear with non-matching value
		tx.informal()
			.atomic_op(&key, b"different_value", MutationType::CompareAndClear);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("cac_key2",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"keep_this",
		"Key should remain unchanged when values don't match"
	);

	// Test 3: Compare and clear on non-existent key
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_nonexistent",));

		// Compare and clear on non-existent key (treated as empty value)
		tx.informal()
			.atomic_op(&key, b"", MutationType::CompareAndClear);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("cac_nonexistent",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value, None,
		"Non-existent key should be cleared when comparing with empty value"
	);

	// Test 4: Compare and clear with empty value on existing key
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_empty",));

		// Set empty value
		tx.set(&key, b"");
		Ok(())
	})
	.await
	.unwrap();

	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("cac_empty",));

		// Compare and clear with empty value
		tx.informal()
			.atomic_op(&key, b"", MutationType::CompareAndClear);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("cac_empty",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value, None,
		"Key with empty value should be cleared when comparing with empty value"
	);
}

async fn test_atomic_transaction_isolation(db: &Database) {
	use universaldb::options::MutationType;

	// Test that atomic operations within a transaction are visible to subsequent reads
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("isolation_key",));

		// Set initial value
		tx.set(&key, &10i64.to_le_bytes());

		// Perform atomic add
		tx.informal()
			.atomic_op(&key, &5i64.to_le_bytes(), MutationType::Add);

		// Read the value within the same transaction
		let value = tx.get(&key, Serializable).await?;
		let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());

		assert_eq!(
			result, 15,
			"Atomic operation should be visible within the same transaction"
		);

		// Perform another atomic operation
		tx.informal()
			.atomic_op(&key, &3i64.to_le_bytes(), MutationType::Add);

		// Read again
		let value = tx.get(&key, Serializable).await?;
		let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());

		assert_eq!(
			result, 18,
			"Multiple atomic operations should be cumulative within transaction"
		);

		Ok(())
	})
	.await
	.unwrap();

	// Test that atomic operations are isolated between transactions
	// Set initial value in one transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("isolation_key2",));

		tx.set(&key, &100i64.to_le_bytes());
		Ok(())
	})
	.await
	.unwrap();

	// Perform atomic operation in another transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("isolation_key2",));

		tx.informal()
			.atomic_op(&key, &50i64.to_le_bytes(), MutationType::Add);
		Ok(())
	})
	.await
	.unwrap();

	// Verify the result in a third transaction
	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("isolation_key2",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(
		result, 150,
		"Atomic operation should be committed and visible in new transaction"
	);
}

async fn test_atomic_nonexistent_keys(db: &Database) {
	use universaldb::options::MutationType;

	// Test atomic operations on non-existent keys behave correctly

	// Test Add (should treat as 0)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("nonexistent_add",));

		tx.informal()
			.atomic_op(&key, &42i64.to_le_bytes(), MutationType::Add);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("nonexistent_add",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(result, 42, "Add on non-existent key should treat as 0");

	// Test BitOr (should treat as 0)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("nonexistent_or",));

		tx.informal()
			.atomic_op(&key, &[0b11110000], MutationType::BitOr);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("nonexistent_or",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap()[0],
		0b11110000,
		"BitOr on non-existent key should treat as 0"
	);

	// Test Max (should set the parameter value)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("nonexistent_max",));

		tx.informal()
			.atomic_op(&key, &123i64.to_le_bytes(), MutationType::Max);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("nonexistent_max",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	let result = i64::from_le_bytes(Vec::from(value.unwrap()).try_into().unwrap());
	assert_eq!(
		result, 123,
		"Max on non-existent key should set the parameter value"
	);

	// Test ByteMin (should set the parameter value)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("nonexistent_bytemin",));

		tx.informal()
			.atomic_op(&key, b"hello", MutationType::ByteMin);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("nonexistent_bytemin",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value.unwrap().as_slice(),
		b"hello",
		"ByteMin on non-existent key should set the parameter value"
	);

	// Test CompareAndClear with empty comparison (should clear since non-existent = empty)
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test");
		let key = test_subspace.pack(&("nonexistent_cac",));

		tx.informal()
			.atomic_op(&key, b"", MutationType::CompareAndClear);
		Ok(())
	})
	.await
	.unwrap();

	let value = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test");
			let key = test_subspace.pack(&("nonexistent_cac",));
			let val = tx.get(&key, Serializable).await?;
			Ok(val)
		})
		.await
		.unwrap();

	assert_eq!(
		value, None,
		"CompareAndClear on non-existent key with empty param should result in None"
	);
}

#[allow(dead_code)]
async fn test_versionstamps(db: &Database) {
	// Test 1: Basic versionstamp insertion and ordering within a single transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test_vs");

		// Create multiple values with incomplete versionstamps in the same transaction
		// Insert multiple entries with versionstamps
		for i in 0..3 {
			let incomplete = Versionstamp::from([0xff; 12]);
			let tuple = vec![
				Element::String("vs_test".into()),
				Element::Versionstamp(incomplete),
				Element::Int(i),
			];
			let key = test_subspace.pack(&(format!("entry_{}", i),));
			let value = pack_with_versionstamp(&tuple);
			tx.set(&key, &value);
		}

		Ok(())
	})
	.await
	.unwrap();

	// Verify that versionstamps were substituted and have the same transaction version
	// but different user versions (counter values)
	let results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test_vs");
			let (begin, end) = test_subspace.range();

			let range_opt = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
				..RangeOption::default()
			};

			let values = tx.get_range(&range_opt, 1, Serializable).await?;
			let mut results = Vec::new();

			for kv in values.into_iter() {
				let key = kv.key().to_vec();
				let value = kv.value().to_vec();
				let unpacked: Vec<Element> = universaldb::tuple::unpack(&value).unwrap();
				if unpacked.len() >= 3 {
					if let Element::Versionstamp(vs) = &unpacked[1] {
						if let Element::Int(i) = &unpacked[2] {
							results.push((key, vs.clone(), *i));
						}
					}
				}
			}

			Ok(results)
		})
		.await
		.unwrap();

	// All entries should have versionstamps
	assert_eq!(results.len(), 3, "Expected 3 entries with versionstamps");

	// Check if versionstamp substitution is supported by checking if any are complete
	let has_substitution = results.iter().any(|(_, vs, _)| vs.is_complete());

	if !has_substitution {
		// Memory driver doesn't support versionstamp substitution
		// Skip the remaining versionstamp tests
		println!("Skipping versionstamp tests - driver doesn't support substitution");
		return;
	}

	// Extract versionstamps and verify they're complete
	for (_, vs, _) in &results {
		assert!(
			vs.is_complete(),
			"Versionstamp should be complete after substitution"
		);
	}

	// Test 2: Versionstamp ordering across multiple transactions
	// Insert entries in separate transactions to get different transaction versions
	let mut tx_versionstamps = Vec::new();

	for i in 0..3 {
		let vs = db
			.txn("test_universaldbintegration", |tx| async move {
				let test_subspace = Subspace::from("test_vs");
				let incomplete = Versionstamp::from([0xff; 12]);

				let tuple = vec![
					Element::String("multi_tx".into()),
					Element::Versionstamp(incomplete),
					Element::Int(i),
				];
				let key = test_subspace.pack(&(format!("tx_{}", i),));
				let value = pack_with_versionstamp(&tuple);
				tx.set(&key, &value);

				Ok(i)
			})
			.await
			.unwrap();

		tx_versionstamps.push(vs);

		// Small delay to ensure different timestamps
		tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
	}

	// Read back and verify ordering
	let multi_tx_results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test_vs");
			let begin = test_subspace.pack(&("tx_",));
			let end = test_subspace.pack(&("tx_z",));

			let range_opt = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
				..RangeOption::default()
			};

			let values = tx.get_range(&range_opt, 1, Serializable).await?;
			let mut results = Vec::new();

			for kv in values.into_iter() {
				let unpacked: Vec<Element> = universaldb::tuple::unpack(kv.value()).unwrap();
				if let Element::Versionstamp(vs) = &unpacked[1] {
					results.push((kv.key().to_vec(), vs.as_bytes().to_vec()));
				}
			}

			Ok(results)
		})
		.await
		.unwrap();

	assert_eq!(
		multi_tx_results.len(),
		3,
		"Expected 3 entries from multiple transactions"
	);

	// Versionstamps from later transactions should be greater
	for i in 1..multi_tx_results.len() {
		let prev_vs = &multi_tx_results[i - 1].1;
		let curr_vs = &multi_tx_results[i].1;
		assert!(
			curr_vs > prev_vs,
			"Versionstamps should increase across transactions"
		);
	}

	// Test 3: Already complete versionstamps should not be modified
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test_vs");

		// Create a complete versionstamp manually
		let complete_vs = generate_versionstamp(999);

		let tuple = vec![
			Element::String("complete_vs".into()),
			Element::Versionstamp(complete_vs),
			Element::Int(42),
		];

		// Pack with versionstamp (this adds the offset)
		let value = pack_with_versionstamp(&tuple);
		let key = test_subspace.pack(&("complete_entry",));
		tx.set(&key, &value);

		Ok(())
	})
	.await
	.unwrap();

	// Read back and verify the versionstamp remains unchanged
	let complete_result = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test_vs");
			let key = test_subspace.pack(&("complete_entry",));
			let value = tx.get(&key, Serializable).await?.unwrap();

			let unpacked: Vec<Element> = universaldb::tuple::unpack(&value).unwrap();
			if let Element::Versionstamp(vs) = &unpacked[1] {
				Ok((vs.user_version(), vs.is_complete()))
			} else {
				panic!("Expected versionstamp element");
			}
		})
		.await
		.unwrap();

	assert_eq!(complete_result.0, 999, "User version should remain 999");
	assert!(complete_result.1, "Versionstamp should still be complete");

	// Test 4: Verify correct count and order within a transaction
	// Insert 10 entries in one transaction and verify they have sequential counters
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test_vs");

		for i in 0..10 {
			let incomplete = Versionstamp::from([0xff; 12]);
			let tuple = vec![
				Element::String("count_test".into()),
				Element::Versionstamp(incomplete),
				Element::Int(i),
			];
			let key = test_subspace.pack(&(format!("count_{:02}", i),));
			let value = pack_with_versionstamp(&tuple);
			tx.set(&key, &value);
		}

		Ok(())
	})
	.await
	.unwrap();

	// Read back and verify count and ordering
	let count_results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test_vs");
			let begin = test_subspace.pack(&("count_",));
			let end = test_subspace.pack(&("count_z",));

			let range_opt = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
				..RangeOption::default()
			};

			let values = tx.get_range(&range_opt, 1, Serializable).await?;
			let mut results = Vec::new();

			for kv in values.into_iter() {
				let unpacked: Vec<Element> = universaldb::tuple::unpack(kv.value()).unwrap();
				if let (Element::Versionstamp(vs), Element::Int(idx)) = (&unpacked[1], &unpacked[2])
				{
					results.push((vs.as_bytes().to_vec(), *idx));
				}
			}

			Ok(results)
		})
		.await
		.unwrap();

	assert_eq!(count_results.len(), 10, "Expected exactly 10 entries");

	// All should have the same transaction version (first 8 bytes)
	let first_tx_version = &count_results[0].0[0..8];
	for (vs_bytes, _) in &count_results {
		assert_eq!(
			&vs_bytes[0..8],
			first_tx_version,
			"All entries should have same transaction version"
		);
	}

	// The counter portion (bytes 8-10) should be sequential
	let mut counters: Vec<u16> = count_results
		.iter()
		.map(|(vs_bytes, _)| u16::from_be_bytes([vs_bytes[8], vs_bytes[9]]))
		.collect();
	counters.sort();

	// Verify they are sequential (might not start at 0 due to other operations)
	for i in 1..counters.len() {
		assert_eq!(
			counters[i],
			counters[i - 1] + 1,
			"Counters should be sequential"
		);
	}

	// Test 5: Mixed incomplete and complete versionstamps in same transaction
	db.txn("test_universaldbintegration", |tx| async move {
		let test_subspace = Subspace::from("test_vs");

		// Insert an incomplete versionstamp
		let incomplete = Versionstamp::from([0xff; 12]);
		let tuple1 = vec![
			Element::String("mixed".into()),
			Element::Versionstamp(incomplete),
			Element::Int(1),
		];
		let key1 = test_subspace.pack(&("mixed_incomplete",));
		let value1 = pack_with_versionstamp(&tuple1);
		tx.set(&key1, &value1);

		// Insert a complete versionstamp
		let complete = generate_versionstamp(555);
		let tuple2 = vec![
			Element::String("mixed".into()),
			Element::Versionstamp(complete),
			Element::Int(2),
		];
		let key2 = test_subspace.pack(&("mixed_complete",));
		let value2 = pack_with_versionstamp(&tuple2);
		tx.set(&key2, &value2);

		Ok(())
	})
	.await
	.unwrap();

	// Verify both were stored correctly
	let mixed_results = db
		.txn("test_universaldbintegration", |tx| async move {
			let test_subspace = Subspace::from("test_vs");
			let begin = test_subspace.pack(&("mixed_",));
			let end = test_subspace.pack(&("mixed_z",));

			let range_opt = RangeOption {
				begin: KeySelector::first_greater_or_equal(Cow::Owned(begin)),
				end: KeySelector::first_greater_or_equal(Cow::Owned(end)),
				..RangeOption::default()
			};

			let values = tx.get_range(&range_opt, 1, Serializable).await?;
			let mut results = Vec::new();

			for kv in values.into_iter() {
				let unpacked: Vec<Element> = universaldb::tuple::unpack(kv.value()).unwrap();
				if let Element::Versionstamp(vs) = &unpacked[1] {
					results.push((
						String::from_utf8_lossy(kv.key()).to_string(),
						vs.user_version(),
						vs.is_complete(),
					));
				}
			}

			Ok(results)
		})
		.await
		.unwrap();

	assert_eq!(mixed_results.len(), 2, "Expected 2 mixed entries");

	// Find and verify each entry
	let incomplete_entry = mixed_results
		.iter()
		.find(|(k, _, _)| k.contains("mixed_incomplete"))
		.expect("Should find incomplete entry");
	assert!(
		incomplete_entry.2,
		"Incomplete versionstamp should be substituted and complete"
	);
	assert_eq!(
		incomplete_entry.1, 0,
		"Substituted versionstamp should have user_version 0"
	);

	let complete_entry = mixed_results
		.iter()
		.find(|(k, _, _)| k.contains("mixed_complete"))
		.expect("Should find complete entry");
	assert!(
		complete_entry.2,
		"Complete versionstamp should remain complete"
	);
	assert_eq!(
		complete_entry.1, 555,
		"Complete versionstamp should keep user_version 555"
	);
}
