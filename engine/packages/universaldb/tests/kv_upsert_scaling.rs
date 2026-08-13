//! Throwaway diagnostic: isolates the pure-Postgres cost of the resolver's kv write from all of the
//! coordination code around it (conflict tracker, fold, claim SELECT, doorbell/reply NOTIFY, queue
//! round-trips). It does NOTHING but the `kv` upsert the leader's apply CTE performs, so the delta
//! between this floor and the real `batch_ms` is "our code", not Postgres.
//!
//! Run:
//!   cargo test -p universaldb --test kv_upsert_scaling -- --ignored --nocapture
//!
//! Two sweeps:
//!   1. batch-size sweep, single serial writer (mirrors the single leader drain): one txn per batch,
//!      distinct keys, `unnest` upsert. Shows the per-batch fixed floor + marginal per-row cost of a
//!      pure PG write as batch length grows.
//!   2. concurrency sweep: N independent writers doing single-row upsert txns. Shows the raw PG write
//!      concurrency the single-leader design forgoes (PG itself scales writers; the leader serializes
//!      them through one drain task).

use std::time::{Duration, Instant};

use rivet_test_deps_docker::TestDatabase;
use tokio_postgres::NoTls;
use uuid::Uuid;

async fn connect(conn_str: &str) -> tokio_postgres::Client {
	let (client, connection) = tokio_postgres::connect(conn_str, NoTls)
		.await
		.expect("connect");
	tokio::spawn(async move {
		let _ = connection.await;
	});
	client
}

/// Percentile from a pre-sorted slice of microsecond samples, returned as milliseconds.
fn pct_ms(sorted_us: &[u128], p: f64) -> f64 {
	if sorted_us.is_empty() {
		return 0.0;
	}
	let idx = ((sorted_us.len() as f64 * p) as usize).min(sorted_us.len() - 1);
	sorted_us[idx] as f64 / 1000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "diagnostic benchmark; run explicitly with --ignored --nocapture"]
async fn kv_upsert_scaling() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("warn")
		.with_test_writer()
		.try_init();

	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();
	tokio::time::sleep(Duration::from_secs(4)).await;

	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};
	let conn_str = postgres_config.url.read().clone();

	let setup = connect(&conn_str).await;
	setup
		.batch_execute(
			"CREATE TABLE IF NOT EXISTS kv (key BYTEA PRIMARY KEY, value BYTEA NOT NULL)",
		)
		.await
		.unwrap();

	// Value size roughly matching a small UDB kv write.
	const VALUE_LEN: usize = 64;

	// ===== Sweep 1: batch-size, single serial writer (mirrors the leader drain) =====
	println!("\n=== batch-size sweep (single serial writer, 1 txn/batch + COMMIT) ===");
	println!(
		"{:>10} {:>10} {:>16} {:>16} {:>16}",
		"batch_len", "batches", "batch_ms p50", "batch_ms p95", "per_row_ms p50"
	);
	{
		let mut client = connect(&conn_str).await;
		let mut key_ctr: u64 = 0;
		for &bs in &[1usize, 2, 4, 8, 16, 32, 64, 128, 256] {
			// Aim for a similar number of total rows per size so timings are comparable.
			let iters = (4096 / bs).max(64);
			// Warm up so the first-statement plan/parse cost is not counted.
			for _ in 0..3 {
				let (keys, vals) = next_batch(&mut key_ctr, bs, VALUE_LEN);
				upsert_batch(&mut client, &keys, &vals).await;
			}
			let mut batch_us = Vec::with_capacity(iters);
			for _ in 0..iters {
				let (keys, vals) = next_batch(&mut key_ctr, bs, VALUE_LEN);
				let t = Instant::now();
				upsert_batch(&mut client, &keys, &vals).await;
				batch_us.push(t.elapsed().as_micros());
			}
			batch_us.sort_unstable();
			let p50 = pct_ms(&batch_us, 0.5);
			println!(
				"{:>10} {:>10} {:>16.3} {:>16.3} {:>16.4}",
				bs,
				iters,
				p50,
				pct_ms(&batch_us, 0.95),
				p50 / bs as f64
			);
		}
	}

	// ===== Sweep 2: concurrency, N independent single-row writers =====
	println!("\n=== concurrency sweep (N parallel writers, single-row txn each, 3s) ===");
	println!(
		"{:>6} {:>14} {:>14} {:>14}",
		"N", "ops/s", "op_ms p50", "op_ms p95"
	);
	for &n in &[1usize, 2, 4, 8, 16, 32, 64] {
		let run = Duration::from_secs(3);
		let mut handles = Vec::with_capacity(n);
		for w in 0..n {
			let cs = conn_str.clone();
			handles.push(tokio::spawn(async move {
				let mut client = connect(&cs).await;
				// Disjoint key space per worker so writers do not contend on the same row lock.
				let mut key: u64 = (w as u64) << 40;
				let mut lat_us = Vec::new();
				let deadline = Instant::now() + run;
				while Instant::now() < deadline {
					key += 1;
					let k = key.to_be_bytes().to_vec();
					let v = vec![0u8; VALUE_LEN];
					let t = Instant::now();
					let txn = client.transaction().await.unwrap();
					txn.execute(
						"INSERT INTO kv (key, value) VALUES ($1, $2)
						 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
						&[&k, &v],
					)
					.await
					.unwrap();
					txn.commit().await.unwrap();
					lat_us.push(t.elapsed().as_micros());
				}
				lat_us
			}));
		}
		let mut all = Vec::new();
		for h in handles {
			all.extend(h.await.unwrap());
		}
		all.sort_unstable();
		println!(
			"{:>6} {:>14.0} {:>14.3} {:>14.3}",
			n,
			all.len() as f64 / 3.0,
			pct_ms(&all, 0.5),
			pct_ms(&all, 0.95)
		);
	}
}

/// Build a batch of `bs` distinct keys and equal-length values, advancing the shared counter.
fn next_batch(key_ctr: &mut u64, bs: usize, value_len: usize) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
	let mut keys = Vec::with_capacity(bs);
	let mut vals = Vec::with_capacity(bs);
	for _ in 0..bs {
		*key_ctr += 1;
		keys.push(key_ctr.to_be_bytes().to_vec());
		vals.push(vec![0u8; value_len]);
	}
	(keys, vals)
}

/// One batch upsert in its own transaction with a real COMMIT, matching the leader's apply shape.
async fn upsert_batch(client: &mut tokio_postgres::Client, keys: &[Vec<u8>], vals: &[Vec<u8>]) {
	let txn = client.transaction().await.unwrap();
	txn.execute(
		"INSERT INTO kv (key, value) SELECT * FROM unnest($1::bytea[], $2::bytea[])
		 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
		&[&keys, &vals],
	)
	.await
	.unwrap();
	txn.commit().await.unwrap();
}
