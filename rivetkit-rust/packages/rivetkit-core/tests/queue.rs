use super::*;

mod moved_tests {
	use super::{QueueNextBatchOpts, QueueNextOpts, QueueWaitOpts};

	use crate::actor::config::ActorConfig;
	use crate::actor::context::ActorContext;
	use crate::actor::keys::{
		QUEUE_METADATA_KEY, decode_queue_message_key, make_queue_message_key,
	};
	use crate::kv::Kv;
	use std::sync::atomic::{AtomicBool, Ordering};
	use std::sync::Arc;
	use std::time::Duration;
	use tokio::task::yield_now;
	use tokio_util::sync::CancellationToken;

	fn test_queue() -> ActorContext {
		ActorContext::new_with_kv(
			"actor-queue",
			"queue-test",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		)
	}

	fn assert_actor_aborted(error: anyhow::Error) {
		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "aborted");
	}

	#[tokio::test]
	async fn next_batch_filters_and_limits_messages_in_enqueue_order() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		for (name, body) in [
			("ignored", b"first".as_slice()),
			("target", b"second".as_slice()),
			("target", b"third".as_slice()),
			("target", b"fourth".as_slice()),
		] {
			queue.send(name, body).await.expect("send queue message");
		}

		let selected = queue
			.next_batch(QueueNextBatchOpts {
				names: Some(vec!["target".into()]),
				count: 2,
				timeout: None,
				signal: None,
				completable: false,
			})
			.await
			.expect("receive filtered queue batch");
		assert_eq!(
			selected
				.into_iter()
				.map(|message| message.body)
				.collect::<Vec<_>>(),
			vec![b"second".to_vec(), b"third".to_vec()]
		);

		let remaining = queue.inspect_messages().await.expect("inspect queue");
		assert_eq!(
			remaining
				.into_iter()
				.map(|message| message.body)
				.collect::<Vec<_>>(),
			vec![b"first".to_vec(), b"fourth".to_vec()]
		);
	}

	#[tokio::test]
	async fn wait_for_available_does_not_consume_or_reorder_messages() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		queue.send("first", b"one").await.expect("send first");
		queue.send("target", b"two").await.expect("send target");

		queue
			.wait_for_names_available(vec!["target".to_owned()], QueueWaitOpts::default())
			.await
			.expect("wait for matching queue message");

		let messages = queue.inspect_messages().await.expect("inspect queue");
		assert_eq!(
			messages
				.iter()
				.map(|message| message.name.as_str())
				.collect::<Vec<_>>(),
			vec!["first", "target"],
		);
	}

	#[tokio::test]
	async fn durable_completion_verifies_persisted_name_and_is_idempotent() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		let message = queue
			.send("expected", b"body")
			.await
			.expect("send queue message");
		let error = queue
			.complete_persisted_message(message.id, "wrong", None)
			.await
			.expect_err("wrong name must fail while completing");
		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "queue");
		assert_eq!(error.code(), "message_identity_mismatch");
		assert_eq!(queue.inspect_messages().await.expect("inspect").len(), 1);

		assert!(
			queue
				.complete_persisted_message(message.id, "expected", None)
				.await
				.expect("complete matching message")
		);
		assert!(
			!queue
				.complete_persisted_message(message.id, "expected", None)
				.await
				.expect("repeat completion is an idempotent miss")
		);
	}

	#[tokio::test]
	async fn durable_completion_survives_actor_context_recreation() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		let message = queue
			.send("durable", b"body")
			.await
			.expect("send queue message");
		let sql = queue.sql().clone();
		drop(queue);

		let recreated = ActorContext::build(
			"actor-queue".to_owned(),
			"queue-test".to_owned(),
			Vec::new(),
			"local".to_owned(),
			Some(2),
			"test-envoy".to_owned(),
			ActorConfig::default(),
			Kv::new_in_memory(),
			sql,
		);
		assert!(
			recreated
				.complete_persisted_message(message.id, "durable", None)
				.await
				.expect("complete persisted message from recreated context")
		);
		assert!(
			recreated
				.inspect_messages()
				.await
				.expect("inspect recreated queue")
				.is_empty()
		);
	}

	#[tokio::test]
	async fn stale_completion_does_not_decrement_queue_size_twice() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		queue.send("first", b"one").await.expect("send first");
		queue.send("second", b"two").await.expect("send second");

		let first = queue
			.next(QueueNextOpts {
				completable: true,
				..Default::default()
			})
			.await
			.expect("receive first")
			.expect("first message");
		assert!(
			queue
				.complete_persisted_message(first.id, &first.name, None)
				.await
				.expect("complete first by identity")
		);
		first
			.complete(None)
			.await
			.expect("stale completion is idempotent");

		assert_eq!(queue.0.queue_metadata.lock().await.size, 1);
		let remaining = queue.inspect_messages().await.expect("inspect queue");
		assert_eq!(remaining.len(), 1);
		assert_eq!(remaining[0].name, "second");
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn wait_for_available_observes_enqueue_before_waiter_parks() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		let runtime = tokio::runtime::Handle::current();
		let enqueued = Arc::new(AtomicBool::new(false));
		queue.set_wait_activity_callback(Some(Arc::new({
			let queue = queue.clone();
			let enqueued = enqueued.clone();
			move || {
				if enqueued.swap(true, Ordering::SeqCst) {
					return;
				}
				let queue = queue.clone();
				let runtime = runtime.clone();
				std::thread::spawn(move || {
					runtime.block_on(queue.send("target", b"ready"))
				})
				.join()
				.expect("enqueue thread joins")
				.expect("enqueue target before waiter parks");
			}
		})));

		queue
			.wait_for_names_available(
				vec!["target".to_owned()],
				QueueWaitOpts {
					timeout: Some(Duration::from_millis(100)),
					..Default::default()
				},
			)
			.await
			.expect("notification is retained until the waiter parks");
	}

	#[tokio::test]
	async fn next_batch_supports_large_name_filters_without_sql_bind_expansion() {
		let queue = test_queue();
		crate::actor::internal_storage::schema::ensure_internal_schema(queue.sql())
			.await
			.expect("initialize queue storage");
		let target = format!("queue-{:04}-{}", 1_099, "x".repeat(112));
		queue
			.send(&target, b"selected")
			.await
			.expect("send queue message");
		let names = (0..1_100)
			.map(|index| format!("queue-{index:04}-{}", "x".repeat(112)))
			.collect();

		let selected = queue
			.next_batch(QueueNextBatchOpts {
				names: Some(names),
				count: 1,
				timeout: None,
				signal: None,
				completable: false,
			})
			.await
			.expect("receive from large name filter");

		assert_eq!(selected.len(), 1);
		assert_eq!(selected[0].name, target);
		assert_eq!(selected[0].body, b"selected");
	}

	#[test]
	fn queue_message_keys_are_big_endian() {
		let first = make_queue_message_key(1);
		let second = make_queue_message_key(2);

		assert!(first < second);
		assert_eq!(QUEUE_METADATA_KEY, [5, 1, 1]);
		assert_eq!(first, vec![5, 1, 2, 0, 0, 0, 0, 0, 0, 0, 1]);
		assert_eq!(decode_queue_message_key(&first).expect("decode first"), 1);
		assert_eq!(decode_queue_message_key(&second).expect("decode second"), 2);
	}

	#[tokio::test]
	async fn wait_for_names_returns_aborted_when_signal_is_already_cancelled() {
		let queue = test_queue();
		let signal = CancellationToken::new();
		signal.cancel();

		let error = queue
			.wait_for_names(
				vec!["missing".to_owned()],
				QueueWaitOpts {
					signal: Some(signal),
					..Default::default()
				},
			)
			.await
			.expect_err("already-cancelled waits should abort immediately");

		assert_actor_aborted(error);
	}

	#[tokio::test(start_paused = true)]
	async fn wait_for_names_returns_aborted_when_signal_cancels_during_wait() {
		let queue = test_queue();
		let signal = CancellationToken::new();
		let wait_signal = signal.clone();
		let wait_queue = queue.clone();

		let wait = tokio::spawn(async move {
			wait_queue
				.wait_for_names(
					vec!["missing".to_owned()],
					QueueWaitOpts {
						timeout: Some(Duration::from_secs(60)),
						signal: Some(wait_signal),
						..Default::default()
					},
				)
				.await
		});

		yield_now().await;
		signal.cancel();

		let error = wait
			.await
			.expect("wait task should join")
			.expect_err("cancelled waits should abort");

		assert_actor_aborted(error);
	}

	#[tokio::test(start_paused = true)]
	async fn next_returns_aborted_when_actor_signal_cancels_during_wait() {
		let queue = test_queue();

		let wait = tokio::spawn({
			let queue = queue.clone();
			async move {
				queue
					.next(QueueNextOpts {
						names: Some(vec!["missing".to_owned()]),
						timeout: Some(Duration::from_secs(60)),
						..Default::default()
					})
					.await
			}
		});

		yield_now().await;
		queue.cancel_actor_abort_signal();

		let error = wait
			.await
			.expect("wait task should join")
			.expect_err("cancelled actor waits should abort");

		assert_actor_aborted(error);
	}
}
