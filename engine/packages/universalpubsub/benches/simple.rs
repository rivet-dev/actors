//! Because criterion is a pain in my ass with async workloads

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::*;
use futures_util::future::join_all;
use rivet_test_deps_docker::TestPubSub;
use std::future::Future;
use tabled::{builder::Builder, settings::Style};
use universalpubsub::{NextOutput, PubSub, PublishOpts};
use uuid::Uuid;

#[derive(Clone, Debug)]
struct BenchResult {
	avg: Duration,
	min: Duration,
	max: Duration,
}

#[allow(dead_code)]
async fn warm_sleep(ms: u64) {
	tokio::time::sleep(Duration::from_millis(ms)).await
}

/// Subscribe to a channel and wait for the subscription to propagate
/// by sending and receiving an echo message on the same channel.
///
/// This is necessary in benchmarks because when client A subscribes and client B
/// publishes, client B doesn't know if the subscription has finished propagating
/// through the server. By sending an echo message and waiting for it, we ensure
/// the subscription is fully active before proceeding.
///
/// Note: This is specific to the benchmark tests. Production code on a single
/// server can assume that all code is using the same channel and doesn't need
/// this propagation check.
async fn subscribe_and_wait_propagate(
	pubsub: &PubSub,
	subject: &str,
) -> Result<universalpubsub::Subscriber> {
	let mut sub = pubsub.subscribe(subject).await?;

	// Send an echo message to validate propagation
	let echo_msg = b"echo_validation";
	pubsub
		.publish(subject, echo_msg, PublishOpts::broadcast())
		.await?;
	pubsub.flush().await?;

	// Wait for the echo message
	match sub.next().await? {
		NextOutput::Message(m) => {
			if m.payload != echo_msg {
				bail!("unexpected message during propagation check");
			}
		}
		NextOutput::Unsubscribed => bail!("unexpected unsubscribe during propagation check"),
	}

	Ok(sub)
}

async fn run_publish_once(
	pubsub: &PubSub,
	subject: &str,
	sub: Arc<tokio::sync::Mutex<universalpubsub::Subscriber>>,
) -> Result<()> {
	let msg = b"hello";
	pubsub
		.publish(subject, msg, PublishOpts::broadcast())
		.await?;
	pubsub.flush().await?;

	let mut guard = sub.lock().await;
	match guard.next().await? {
		NextOutput::Message(m) => assert_eq!(m.payload, msg),
		NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
	}
	Ok(())
}

#[allow(dead_code)]
async fn run_subscribe_publish_once(
	publisher: &PubSub,
	subscriber: &PubSub,
	subject: &str,
) -> Result<()> {
	let mut sub = subscriber.subscribe(subject).await?;
	// Ensure the SUB reaches the server before publishing
	subscriber.flush().await?;
	let msg = b"hello";
	publisher
		.publish(subject, msg, PublishOpts::broadcast())
		.await?;
	publisher.flush().await?;
	match sub.next().await? {
		NextOutput::Message(m) => assert_eq!(m.payload, msg),
		NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
	}
	Ok(())
}

async fn run_request_once(pubsub: &PubSub, subject: &str) -> Result<()> {
	let payload = b"request payload";
	let resp = pubsub.request(subject, payload).await?;
	assert_eq!(resp.payload, payload);
	Ok(())
}

async fn run_publish_pipelined_once(
	pubsub: &PubSub,
	subject: &str,
	sub: Arc<tokio::sync::Mutex<universalpubsub::Subscriber>>,
) -> Result<()> {
	let msg = b"hello";
	// Fire 32 publishes concurrently
	let futs = (0..32)
		.map(|_| pubsub.publish(subject, msg, PublishOpts::broadcast()))
		.collect::<Vec<_>>();
	for r in join_all(futs).await {
		r?;
	}
	pubsub.flush().await?;

	// Receive 32 messages
	let mut guard = sub.lock().await;
	for _ in 0..32 {
		match guard.next().await? {
			NextOutput::Message(m) => assert_eq!(m.payload, msg),
			NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
		}
	}
	Ok(())
}

async fn run_publish_one_once(
	pubsub: &PubSub,
	subject: &str,
	sub: Arc<tokio::sync::Mutex<universalpubsub::Subscriber>>,
) -> Result<()> {
	let msg = b"hello";
	pubsub.publish(subject, msg, PublishOpts::one()).await?;
	pubsub.flush().await?;

	let mut guard = sub.lock().await;
	match guard.next().await? {
		NextOutput::Message(m) => assert_eq!(m.payload, msg),
		NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
	}
	Ok(())
}

#[allow(dead_code)]
async fn run_subscribe_publish_one_once(
	publisher: &PubSub,
	subscriber: &PubSub,
	subject: &str,
) -> Result<()> {
	let mut sub = subscriber.subscribe(subject).await?;
	// Ensure the SUB reaches the server before publishing
	subscriber.flush().await?;
	let msg = b"hello";
	publisher.publish(subject, msg, PublishOpts::one()).await?;
	publisher.flush().await?;
	match sub.next().await? {
		NextOutput::Message(m) => assert_eq!(m.payload, msg),
		NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
	}
	Ok(())
}

async fn run_publish_pipelined_one_once(
	pubsub: &PubSub,
	subject: &str,
	subs: Vec<Arc<tokio::sync::Mutex<universalpubsub::Subscriber>>>,
) -> Result<()> {
	let msg = b"hello";
	// Fire 32 publishes concurrently with PublishOpts::one()
	let futs = (0..32)
		.map(|_| pubsub.publish(subject, msg, PublishOpts::one()))
		.collect::<Vec<_>>();
	for r in join_all(futs).await {
		r?;
	}
	pubsub.flush().await?;

	// Each subscriber should receive some messages (load balanced)
	let mut total_received = 0;
	for sub in &subs {
		let mut guard = sub.lock().await;
		// Drain messages from this subscriber
		loop {
			// Use timeout to avoid hanging if no more messages
			let timeout_result =
				tokio::time::timeout(Duration::from_millis(10), guard.next()).await;
			match timeout_result {
				std::result::Result::Ok(next_result) => match next_result {
					std::result::Result::Ok(NextOutput::Message(m)) => {
						// Skip echo validation messages
						if m.payload == b"echo_validation" {
							continue;
						}
						assert_eq!(m.payload, msg);
						total_received += 1;
					}
					_ => break,
				},
				_ => break,
			}
		}
	}
	// Should have received all 32 messages distributed among subscribers
	// Note: some drivers (like memory) may deliver to all subscribers even with PublishOpts::one()
	assert!(
		total_received >= 32,
		"Expected at least 32 messages total across all subscribers, got {}",
		total_received
	);
	Ok(())
}

async fn run_bench<
	SetupF,
	SetupFut,
	IterF,
	IterFut,
	TeardownIterF,
	TeardownIterFut,
	TeardownF,
	TeardownFut,
	Ctx,
	IterOut,
>(
	bench_name: &str,
	iters: usize,
	setup_fn: SetupF,
	mut iter_fn: IterF,
	mut teardown_iter_fn: TeardownIterF,
	teardown_fn: TeardownF,
) -> Result<BenchResult>
where
	SetupF: FnOnce() -> SetupFut,
	SetupFut: Future<Output = Result<Ctx>>,
	Ctx: Clone,
	IterF: FnMut(Ctx) -> IterFut,
	IterFut: Future<Output = Result<IterOut>>,
	TeardownIterF: FnMut(Ctx, IterOut) -> TeardownIterFut,
	TeardownIterFut: Future<Output = Result<()>>,
	TeardownF: FnOnce(Ctx) -> TeardownFut,
	TeardownFut: Future<Output = Result<()>>,
{
	// Announce bench start
	eprintln!("  {}:", bench_name);
	eprintln!("    starting (iters={})", iters);
	let ctx = setup_fn().await?;

	// Warm-up
	let warm_out = iter_fn(ctx.clone()).await?;
	teardown_iter_fn(ctx.clone(), warm_out).await?;

	// Measure
	let mut total = Duration::ZERO;
	let mut min = Duration::MAX;
	let mut max = Duration::ZERO;
	//let mut last_progress = Instant::now();
	for _i in 0..iters {
		let start = Instant::now();
		let out = iter_fn(ctx.clone()).await?;
		let elapsed = start.elapsed();
		total += elapsed;
		if elapsed < min {
			min = elapsed;
		}
		if elapsed > max {
			max = elapsed;
		}
		teardown_iter_fn(ctx.clone(), out).await?;
		//if last_progress.elapsed() >= Duration::from_millis(500) {
		//	eprintln!("    progress: {}/{} iterations", i + 1, iters);
		//	last_progress = Instant::now();
		//}
	}

	// Final teardown for the context
	teardown_fn(ctx).await?;

	let avg = total / (iters as u32);
	eprintln!(
		"    finished (iters={}, avg={:?}, min={:?}, max={:?})",
		iters, avg, min, max
	);
	Ok(BenchResult { avg, min, max })
}

async fn run_benches(
	prefix: &str,
	publisher: PubSub,
	subscriber: PubSub,
	iters: usize,
) -> Result<HashMap<String, BenchResult>> {
	// Print group name once
	eprintln!("{}:", prefix);
	let mut results = HashMap::new();

	// publish
	let result = run_bench(
		"publish",
		iters,
		|| {
			let subscriber = subscriber.clone();
			let subject_pub = format!("bench.{}.publish", prefix);
			let publisher = publisher.clone();
			async move {
				let sub = subscribe_and_wait_propagate(&subscriber, &subject_pub).await?;
				let sub = Arc::new(tokio::sync::Mutex::new(sub));
				Ok((publisher, subject_pub, sub))
			}
		},
		|(publisher, subject, sub)| async move {
			run_publish_once(&publisher, &subject, sub.clone())
				.await
				.map(|_| ())
		},
		// Per-iteration teardown: nothing to do
		|_, _| async move { Ok(()) },
		// Final teardown: drop the subscriber to unsubscribe
		|(_publisher, _subject, sub)| async move {
			drop(sub);
			Ok(())
		},
	)
	.await?;
	results.insert("publish".to_string(), result);

	// subscribe_publish
	let result = run_bench(
		"subscribe_publish",
		iters,
		|| {
			let publisher = publisher.clone();
			let subscriber = subscriber.clone();
			let subject = format!("bench.{}.subscribe_publish", prefix);
			async move { Ok((publisher, subscriber, subject)) }
		},
		|(publisher, subscriber, subject)| async move {
			// Create a subscription, ensure it's registered, run once, then return the sub for teardown
			let mut sub = subscribe_and_wait_propagate(&subscriber, &subject).await?;
			let msg = b"hello";
			publisher
				.publish(&subject, msg, PublishOpts::broadcast())
				.await?;
			publisher.flush().await?;
			match sub.next().await? {
				NextOutput::Message(m) => assert_eq!(m.payload, msg),
				NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
			}
			// Return the subscriber so the teardown can explicitly drop/unsubscribe
			Ok(sub)
		},
		// Per-iteration teardown: drop the per-iteration subscription to unsubscribe
		|_, sub| async move {
			drop(sub);
			Ok(())
		},
		// Final teardown: nothing to do
		|_| async move { Ok(()) },
	)
	.await?;
	results.insert("subscribe_publish".to_string(), result);

	// publish_pipelined (32 at once)
	let result = run_bench(
		"publish_pipelined",
		iters,
		|| {
			let subscriber = subscriber.clone();
			let subject = format!("bench.{}.publish_pipelined", prefix);
			let publisher = publisher.clone();
			async move {
				let sub = subscribe_and_wait_propagate(&subscriber, &subject).await?;
				let sub = Arc::new(tokio::sync::Mutex::new(sub));
				Ok((publisher, subject, sub))
			}
		},
		|(publisher, subject, sub)| async move {
			run_publish_pipelined_once(&publisher, &subject, sub.clone()).await
		},
		// Per-iteration teardown: nothing special needed
		|_, _| async move { Ok(()) },
		// Final teardown: drop the subscription
		|(_, _, sub)| async move {
			drop(sub);
			Ok(())
		},
	)
	.await?;
	results.insert("publish_pipelined".to_string(), result);

	// request
	let result = run_bench(
		"request",
		iters,
		|| {
			let subscriber = subscriber.clone();
			let publisher = publisher.clone();
			let subject = format!("bench.{}.request", prefix);
			async move {
				let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
				let subject_clone = subject.clone();
				let subscriber_clone = subscriber.clone();
				tokio::spawn(async move {
					// Use subscribe_and_wait_propagate to ensure subscription is ready
					let mut sub = subscribe_and_wait_propagate(&subscriber_clone, &subject_clone)
						.await
						.unwrap();
					let _ = ready_tx.send(());
					loop {
						match sub.next().await {
							std::result::Result::Ok(NextOutput::Message(msg)) => {
								let _ = msg.reply(&msg.payload).await;
							}
							_ => break,
						}
					}
				});
				let _ = ready_rx.await;
				Ok((publisher, subject))
			}
		},
		|(publisher, subject)| async move { run_request_once(&publisher, &subject).await },
		// Per-iteration teardown: nothing needed
		|_, _| async move { Ok(()) },
		// Final teardown: nothing needed
		|_| async move { Ok(()) },
	)
	.await?;
	results.insert("request".to_string(), result);

	// publish_one (with PublishOpts::one())
	let result = run_bench(
		"publish_one",
		iters,
		|| {
			let subscriber = subscriber.clone();
			let subject = format!("bench.{}.publish_one", prefix);
			let publisher = publisher.clone();
			async move {
				let sub = subscribe_and_wait_propagate(&subscriber, &subject).await?;
				let sub = Arc::new(tokio::sync::Mutex::new(sub));
				Ok((publisher, subject, sub))
			}
		},
		|(publisher, subject, sub)| async move {
			run_publish_one_once(&publisher, &subject, sub.clone())
				.await
				.map(|_| ())
		},
		// Per-iteration teardown: nothing to do
		|_, _| async move { Ok(()) },
		// Final teardown: drop the subscriber to unsubscribe
		|(_publisher, _subject, sub)| async move {
			drop(sub);
			Ok(())
		},
	)
	.await?;
	results.insert("publish_one".to_string(), result);

	// subscribe_publish_one (with PublishOpts::one())
	let result = run_bench(
		"subscribe_publish_one",
		iters,
		|| {
			let publisher = publisher.clone();
			let subscriber = subscriber.clone();
			let subject = format!("bench.{}.subscribe_publish_one", prefix);
			async move { Ok((publisher, subscriber, subject)) }
		},
		|(publisher, subscriber, subject)| async move {
			// Create a subscription, ensure it's registered, run once, then return the sub for teardown
			let mut sub = subscribe_and_wait_propagate(&subscriber, &subject).await?;
			let msg = b"hello";
			publisher.publish(&subject, msg, PublishOpts::one()).await?;
			publisher.flush().await?;
			match sub.next().await? {
				NextOutput::Message(m) => assert_eq!(m.payload, msg),
				NextOutput::Unsubscribed => bail!("unexpected unsubscribe"),
			}
			// Return the subscriber so the teardown can explicitly drop/unsubscribe
			Ok(sub)
		},
		// Per-iteration teardown: drop the per-iteration subscription to unsubscribe
		|_, sub| async move {
			drop(sub);
			Ok(())
		},
		// Final teardown: nothing to do
		|_| async move { Ok(()) },
	)
	.await?;
	results.insert("subscribe_publish_one".to_string(), result);

	// publish_pipelined_one (32 messages with PublishOpts::one() to multiple subscribers)
	let result = run_bench(
		"publish_pipelined_one",
		iters,
		|| {
			let subscriber = subscriber.clone();
			let subject = format!("bench.{}.publish_pipelined_one", prefix);
			let publisher = publisher.clone();
			async move {
				// Create 4 subscribers for load balancing
				let mut subs = Vec::new();
				for _ in 0..4 {
					let sub = subscribe_and_wait_propagate(&subscriber, &subject).await?;
					subs.push(Arc::new(tokio::sync::Mutex::new(sub)));
				}
				Ok((publisher, subject, subs))
			}
		},
		|(publisher, subject, subs)| async move {
			run_publish_pipelined_one_once(&publisher, &subject, subs.clone()).await
		},
		// Per-iteration teardown: nothing special needed
		|_, _| async move { Ok(()) },
		// Final teardown: drop the subscriptions
		|(_, _, subs)| async move {
			drop(subs);
			Ok(())
		},
	)
	.await?;
	results.insert("publish_pipelined_one".to_string(), result);

	Ok(results)
}

async fn setup_nats_pair() -> Result<(PubSub, PubSub)> {
	let test_id = Uuid::new_v4();
	let (pubsub_config, mut docker) = TestPubSub::Nats.config(test_id, 1).await?;
	if let Some(ref mut d) = docker {
		d.start().await?;
	}
	tokio::time::sleep(Duration::from_secs(1)).await;
	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!()
	};
	use std::str::FromStr;
	let server_addrs = nats
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
		.collect::<Result<Vec<_>, _>>()?;
	let driver_pub = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await?;
	let driver_sub = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await?;
	Ok((
		PubSub::new_with_memory_optimization(Arc::new(driver_pub), false),
		PubSub::new_with_memory_optimization(Arc::new(driver_sub), false),
	))
}

async fn setup_nats_single() -> Result<(PubSub, PubSub)> {
	let test_id = Uuid::new_v4();
	let (pubsub_config, mut docker) = TestPubSub::Nats.config(test_id, 1).await?;
	if let Some(ref mut d) = docker {
		d.start().await?;
	}
	tokio::time::sleep(Duration::from_secs(1)).await;
	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!()
	};
	use std::str::FromStr;
	let server_addrs = nats
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
		.collect::<Result<Vec<_>, _>>()?;
	let driver = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await?;
	let pubsub = PubSub::new_with_memory_optimization(Arc::new(driver), false);
	Ok((pubsub.clone(), pubsub))
}

async fn setup_nats_pair_mem() -> Result<(PubSub, PubSub)> {
	let test_id = Uuid::new_v4();
	let (pubsub_config, mut docker) = TestPubSub::Nats.config(test_id, 1).await?;
	if let Some(ref mut d) = docker {
		d.start().await?;
	}
	tokio::time::sleep(Duration::from_secs(1)).await;
	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!()
	};
	use std::str::FromStr;
	let server_addrs = nats
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
		.collect::<Result<Vec<_>, _>>()?;
	let driver_pub = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await?;
	let driver_sub = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await?;
	Ok((
		PubSub::new_with_memory_optimization(Arc::new(driver_pub), true),
		PubSub::new_with_memory_optimization(Arc::new(driver_sub), true),
	))
}

async fn setup_nats_single_mem() -> Result<(PubSub, PubSub)> {
	let test_id = Uuid::new_v4();
	let (pubsub_config, mut docker) = TestPubSub::Nats.config(test_id, 1).await?;
	if let Some(ref mut d) = docker {
		d.start().await?;
	}
	tokio::time::sleep(Duration::from_secs(1)).await;
	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!()
	};
	use std::str::FromStr;
	let server_addrs = nats
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
		.collect::<Result<Vec<_>, _>>()?;
	let driver = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await?;
	let pubsub = PubSub::new_with_memory_optimization(Arc::new(driver), true);
	Ok((pubsub.clone(), pubsub))
}

async fn setup_mem_pair() -> Result<(PubSub, PubSub)> {
	let test_id = Uuid::new_v4();
	let (pubsub_config, _docker) = TestPubSub::Memory.config(test_id, 1).await?;
	let rivet_config::config::PubSub::Memory(memory) = pubsub_config else {
		unreachable!()
	};
	let driver = universalpubsub::driver::memory::MemoryDriver::new(memory.channel.clone());
	let pubsub = PubSub::new(Arc::new(driver));
	// Memory driver must use the same PubSub instance to exercise the in-process fast path
	Ok((pubsub.clone(), pubsub.clone()))
}

fn iterations_from_env(default: usize) -> usize {
	std::env::var("UPS_BENCH_ITERS")
		.ok()
		.and_then(|s| s.parse().ok())
		.unwrap_or(default)
}

fn format_duration(d: Duration) -> String {
	if d.as_secs() > 0 {
		format!("{:.2}s", d.as_secs_f64())
	} else if d.as_millis() > 0 {
		format!("{:.2}ms", d.as_micros() as f64 / 1000.0)
	} else {
		format!("{:.2}µs", d.as_micros() as f64)
	}
}

fn print_results_table(all_results: &HashMap<String, HashMap<String, BenchResult>>) {
	eprintln!("\n\n=== BENCHMARK RESULTS TABLE ===\n");

	// Get all unique benchmark names
	let bench_names = vec![
		"publish",
		"subscribe_publish",
		"publish_pipelined",
		"request",
		"publish_one",
		"subscribe_publish_one",
		"publish_pipelined_one",
	];

	// Order of groups for consistent display
	let group_order = vec![
		"mem",
		"nats-nomem",
		"nats-mem",
		"nats-single-nomem",
		"nats-single-mem",
		"pg-nomem",
		"pg-mem",
		"pg-single-nomem",
		"pg-single-mem",
	];

	// Build the table
	let mut builder = Builder::default();

	// Header row
	let mut header = vec!["Group".to_string()];
	for bench in &bench_names {
		header.push(format!("{} (avg)", bench));
	}
	builder.push_record(header);

	// Data rows
	for group in &group_order {
		if let Some(results) = all_results.get(*group) {
			let mut row = vec![group.to_string()];
			for bench in &bench_names {
				if let Some(result) = results.get(*bench) {
					row.push(format_duration(result.avg));
				} else {
					row.push("-".to_string());
				}
			}
			builder.push_record(row);
		}
	}

	let table = builder.build().with(Style::rounded()).to_string();
	println!("{}", table);

	// Also print a min/max table
	eprintln!("\n=== MIN/MAX VALUES ===\n");
	let mut builder = Builder::default();

	// Header row
	let mut header = vec!["Group".to_string()];
	for bench in &bench_names {
		header.push(format!("{} (min/max)", bench));
	}
	builder.push_record(header);

	// Data rows
	for group in &group_order {
		if let Some(results) = all_results.get(*group) {
			let mut row = vec![group.to_string()];
			for bench in &bench_names {
				if let Some(result) = results.get(*bench) {
					row.push(format!(
						"{}/{}",
						format_duration(result.min),
						format_duration(result.max)
					));
				} else {
					row.push("-".to_string());
				}
			}
			builder.push_record(row);
		}
	}

	let table = builder.build().with(Style::rounded()).to_string();
	println!("{}", table);
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
	// Reduce noisy logs by default
	let _ = tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
		.try_init();

	let iters = iterations_from_env(100);
	eprintln!("[universalpubsub-bench] running with iters={iters}");

	let mut all_results = HashMap::new();

	// Memory driver
	let (publisher, subscriber) = setup_mem_pair().await?;
	let results = run_benches("mem", publisher.clone(), subscriber.clone(), iters).await?;
	all_results.insert("mem".to_string(), results);

	// NATS (no memory optimization)
	let (publisher, subscriber) = setup_nats_pair().await?;
	let results = run_benches("nats-nomem", publisher.clone(), subscriber.clone(), iters).await?;
	all_results.insert("nats-nomem".to_string(), results);

	// NATS (memory optimization)
	let (publisher, subscriber) = setup_nats_pair_mem().await?;
	let results = run_benches("nats-mem", publisher.clone(), subscriber.clone(), iters).await?;
	all_results.insert("nats-mem".to_string(), results);

	// NATS single connection (no memory optimization)
	let (publisher, subscriber) = setup_nats_single().await?;
	let results = run_benches(
		"nats-single-nomem",
		publisher.clone(),
		subscriber.clone(),
		iters,
	)
	.await?;
	all_results.insert("nats-single-nomem".to_string(), results);

	// NATS single connection (memory optimization)
	let (publisher, subscriber) = setup_nats_single_mem().await?;
	let results = run_benches(
		"nats-single-mem",
		publisher.clone(),
		subscriber.clone(),
		iters,
	)
	.await?;
	all_results.insert("nats-single-mem".to_string(), results);

	// Print results table
	print_results_table(&all_results);

	Ok(())
}
