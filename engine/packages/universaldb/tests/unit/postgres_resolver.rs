//! Query-plan checks for the leader's batch apply statements.
//!
//! These run against a real Postgres and read back the plan each statement actually executed through
//! `auto_explain`, which reports plans to the session as notices.

use futures_util::future::poll_fn;
use rivet_test_deps_docker::TestDatabase;
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};
use uuid::Uuid;

use super::{super::database::SCHEMA, clear_ranges};

const WORKFLOWS: i64 = 12_500;
const CHUNKS_PER_WORKFLOW: i64 = 8;
const CLEARED_WORKFLOWS: i64 = 8;

fn state_range(workflow: i64) -> (Vec<u8>, Vec<u8>) {
	let begin = format!("wf/{workflow:08}/state/").into_bytes();
	let mut end = begin.clone();
	end.push(0xff);
	(begin, end)
}

/// Clearing ranges must walk the primary key even when the planner prices a full scan of `kv` as
/// competitive.
///
/// In production the planner makes that call once `kv` outgrows the page cache and random reads get
/// expensive, and a range clear that falls back to a full scan holds the leader's batch transaction
/// for minutes. Raising `random_page_cost` prices random reads the same way on a table small enough
/// to build here.
#[tokio::test]
async fn clear_ranges_uses_primary_key_when_scans_look_cheap() {
	let (db_config, docker_config) = TestDatabase::Postgres
		.config(Uuid::new_v4(), 1)
		.await
		.unwrap();
	let mut docker_config = docker_config.unwrap();
	docker_config.start().await.unwrap();
	TestDatabase::Postgres
		.wait_for_ready(&docker_config)
		.await
		.unwrap();
	let rivet_config::config::Database::Postgres(postgres_config) = db_config else {
		unreachable!();
	};
	let url = postgres_config.url.read().clone();

	let (mut client, mut connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
	let (notice_tx, mut notice_rx) = mpsc::unbounded_channel();
	tokio::spawn(async move {
		// The connection yields a notice before it routes the response that follows it, so every plan
		// is in the channel by the time the statement that produced it resolves.
		while let Some(message) = poll_fn(|cx| connection.poll_message(cx)).await {
			match message {
				Ok(AsyncMessage::Notice(notice)) => {
					let _ = notice_tx.send(notice.message().to_string());
				}
				// `AsyncMessage` is non-exhaustive, so other messages need a catch-all.
				Ok(_) => {}
				Err(_) => break,
			}
		}
	});

	client.batch_execute(SCHEMA).await.unwrap();
	// Random insertion order leaves no correlation between key order and heap order, as in
	// production, so the planner cannot count on range reads touching adjacent pages.
	client
		.execute(
			"INSERT INTO kv (key, value)
			 SELECT convert_to(format('wf/%s/state/%s', lpad(w::text, 8, '0'), lpad(c::text, 4, '0')), 'UTF8'),
			        repeat('x', 64)::bytea
			 FROM generate_series(1, $1::bigint) w, generate_series(1, $2::bigint) c
			 ORDER BY random()",
			&[&WORKFLOWS, &CHUNKS_PER_WORKFLOW],
		)
		.await
		.unwrap();
	client
		.batch_execute(
			"ANALYZE kv;
			 LOAD 'auto_explain';
			 SET auto_explain.log_min_duration = 0;
			 SET auto_explain.log_level = notice;
			 SET random_page_cost = 40;",
		)
		.await
		.unwrap();

	let ranges: Vec<_> = (1..=CLEARED_WORKFLOWS).map(state_range).collect();
	let txn = client.transaction().await.unwrap();
	clear_ranges(&txn, &ranges).await.unwrap();

	let plans: Vec<String> = std::iter::from_fn(|| notice_rx.try_recv().ok())
		.filter(|notice| notice.contains("plan:"))
		.collect();
	for plan in &plans {
		assert!(
			!plan.contains("Seq Scan on kv"),
			"range clear fell back to a full scan of kv:\n{plan}"
		);
		assert!(
			plan.contains("kv_pkey"),
			"range clear did not use the kv primary key:\n{plan}"
		);
	}
	assert_eq!(
		plans.len(),
		ranges.len(),
		"expected one executed plan per cleared range: {plans:#?}"
	);

	let remaining: i64 = txn
		.query_one("SELECT count(*) FROM kv", &[])
		.await
		.unwrap()
		.get(0);
	assert_eq!(
		remaining,
		(WORKFLOWS - CLEARED_WORKFLOWS) * CHUNKS_PER_WORKFLOW,
		"range clear removed the wrong rows"
	);
}
