use super::*;

mod moved_tests {
	use std::collections::BTreeSet;
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::sync::mpsc as std_mpsc;
	use std::time::Duration;

	use parking_lot::Mutex;
	use tokio::sync::{Barrier, mpsc};
	use tokio::task::yield_now;

	use super::{
		HibernatableConnectionMetadata, PersistedConnection, decode_persisted_connection,
		encode_persisted_connection, hibernatable_id_from_slice,
	};
	use crate::actor::context::ActorContext;
	use crate::actor::keys::make_connection_key;
	use crate::actor::messages::ActorEvent;
	use crate::actor::task::LifecycleEvent;
	use crate::kv::Kv;

	fn next_non_activity_lifecycle_event(
		rx: &mut mpsc::UnboundedReceiver<LifecycleEvent>,
	) -> Option<LifecycleEvent> {
		rx.try_recv().ok()
	}

	#[test]
	fn make_connection_key_matches_typescript_layout() {
		assert_eq!(make_connection_key("conn-1"), b"\x02conn-1".to_vec());
	}

	#[test]
	fn retained_connection_snapshot_does_not_block_disconnect_writer() {
		let ctx = ActorContext::new_with_kv(
			"actor-connection-snapshot",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		ctx.insert_existing(super::ConnHandle::new(
			"conn-snapshot",
			Vec::new(),
			Vec::new(),
			false,
		));
		let snapshot = ctx.conns();
		let (started_tx, started_rx) = std_mpsc::channel();
		let (removed_tx, removed_rx) = std_mpsc::channel();
		let writer = std::thread::spawn({
			let ctx = ctx.clone();
			move || {
				started_tx.send(()).expect("test receiver should stay open");
				let removed = ctx.remove_existing("conn-snapshot").is_some();
				removed_tx
					.send(removed)
					.expect("test receiver should stay open");
			}
		});

		started_rx.recv().expect("disconnect writer should start");
		let removed = removed_rx.recv_timeout(Duration::from_secs(2));
		let snapshot_ids = snapshot
			.map(|conn| conn.id().to_owned())
			.collect::<Vec<_>>();
		writer.join().expect("disconnect writer should join");

		assert!(removed.expect("retained snapshot must not block connection writer"));
		assert_eq!(snapshot_ids, vec!["conn-snapshot".to_owned()]);
		assert!(ctx.conns().is_empty());
	}

	#[tokio::test]
	async fn pending_connection_is_invisible_until_preflight_succeeds() {
		let ctx = ActorContext::new_with_kv(
			"actor-preflight-visibility",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		ctx.configure_connection_runtime(crate::actor::config::ActorConfig::default());
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(events_tx));

		let events_ctx = ctx.clone();
		let event_task = tokio::spawn(async move {
			let preflight_conn_id = match events_rx.recv().await.expect("preflight event") {
				ActorEvent::ConnectionPreflight { conn, reply, .. } => {
					assert!(events_ctx.connection(conn.id()).is_none());
					conn.set_state_initial(vec![7]);
					let conn_id = conn.id().to_owned();
					reply.send(Ok(()));
					conn_id
				}
				other => panic!("unexpected event: {other:?}"),
			};

			match events_rx.recv().await.expect("open event") {
				ActorEvent::ConnectionOpen { conn, reply, .. } => {
					assert_eq!(conn.id(), preflight_conn_id);
					let visible = events_ctx
						.connection(conn.id())
						.expect("connection should be visible for onConnect");
					assert_eq!(visible.state(), vec![7]);
					reply.send(Ok(()));
				}
				other => panic!("unexpected event: {other:?}"),
			}
		});

		let conn = ctx
			.connect_with_state(vec![1], false, None, None, async { Ok(vec![2]) })
			.await
			.expect("connection should succeed");

		assert_eq!(conn.state(), vec![7]);
		assert!(ctx.connection(conn.id()).is_some());
		event_task.await.expect("event task should complete");
	}

	#[tokio::test]
	async fn failed_preflight_never_exposes_connection() {
		let ctx = ActorContext::new_with_kv(
			"actor-preflight-failure",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		ctx.configure_connection_runtime(crate::actor::config::ActorConfig::default());
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(events_tx));
		let failed_conn_id = Arc::new(Mutex::new(None::<String>));

		let events_ctx = ctx.clone();
		let event_failed_conn_id = failed_conn_id.clone();
		let event_task = tokio::spawn(async move {
			match events_rx.recv().await.expect("preflight event") {
				ActorEvent::ConnectionPreflight { conn, reply, .. } => {
					assert!(events_ctx.connection(conn.id()).is_none());
					*event_failed_conn_id.lock() = Some(conn.id().to_owned());
					reply.send(Err(anyhow::anyhow!("reject preflight")));
				}
				other => panic!("unexpected event: {other:?}"),
			}
			assert!(
				tokio::time::timeout(Duration::from_millis(20), events_rx.recv())
					.await
					.is_err()
			);
		});

		let error = ctx
			.connect_with_state(vec![1], false, None, None, async { Ok(vec![2]) })
			.await
			.expect_err("connection should fail");

		assert!(format!("{error:#}").contains("reject preflight"));
		let conn_id = failed_conn_id
			.lock()
			.clone()
			.expect("failed connection id should be recorded");
		assert!(ctx.connection(&conn_id).is_none());
		event_task.await.expect("event task should complete");
	}

	#[tokio::test]
	async fn transport_close_during_preflight_never_emits_connection_closed() {
		let ctx = ActorContext::new_with_kv(
			"actor-preflight-transport-close",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		ctx.configure_connection_runtime(crate::actor::config::ActorConfig::default());
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(events_tx));
		let closed_conn_id = Arc::new(Mutex::new(None::<String>));

		let event_closed_conn_id = closed_conn_id.clone();
		let event_task = tokio::spawn(async move {
			match events_rx.recv().await.expect("preflight event") {
				ActorEvent::ConnectionPreflight { conn, reply, .. } => {
					conn.disconnect(Some("transport closed"))
						.await
						.expect("pending connection transport close should succeed");
					reply.send(Err(anyhow::anyhow!("reject after transport close")));
				}
				other => panic!("unexpected event: {other:?}"),
			}

			if let Ok(Some(ActorEvent::ConnectionClosed { conn })) =
				tokio::time::timeout(Duration::from_millis(20), events_rx.recv()).await
			{
				*event_closed_conn_id.lock() = Some(conn.id().to_owned());
			}
		});

		let error = ctx
			.connect_with_state(vec![1], false, None, None, async { Ok(vec![2]) })
			.await
			.expect_err("connection should fail");

		assert!(format!("{error:#}").contains("reject after transport close"));
		assert_eq!(*closed_conn_id.lock(), None);
		event_task.await.expect("event task should complete");
	}

	#[test]
	fn persisted_connection_uses_ts_v4_fixed_id_wire_format() {
		let persisted = PersistedConnection {
			id: "c".to_owned(),
			parameters: vec![1, 2],
			state: vec![3],
			gateway_id: [10, 11, 12, 13],
			request_id: [20, 21, 22, 23],
			server_message_index: 9,
			client_message_index: 10,
			request_path: "/".to_owned(),
			..PersistedConnection::default()
		};

		let encoded =
			encode_persisted_connection(&persisted).expect("persisted connection should encode");

		assert_eq!(
			encoded,
			vec![
				4, 0, 1, b'c', 2, 1, 2, 1, 3, 0, 10, 11, 12, 13, 20, 21, 22, 23, 9, 0, 10, 0, 1,
				b'/', 0,
			]
		);

		let decoded =
			decode_persisted_connection(&encoded).expect("persisted connection should decode");
		assert_eq!(decoded.gateway_id, [10, 11, 12, 13]);
		assert_eq!(decoded.request_id, [20, 21, 22, 23]);
	}

	#[test]
	fn hibernatable_id_validation_returns_rivet_error() {
		let error = hibernatable_id_from_slice("gateway_id", &[1, 2, 3])
			.expect_err("invalid id should fail");
		let error = rivet_error::RivetError::extract(&error);

		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "invalid_request");
	}

	#[tokio::test(start_paused = true)]
	async fn concurrent_disconnects_only_emit_one_close_and_one_hibernation_removal() {
		let ctx = ActorContext::new_with_kv(
			"actor-race",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		ctx.configure_connection_runtime(crate::actor::config::ActorConfig::default());
		let (events_tx, mut events_rx) = mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(events_tx));
		let closed = Arc::new(AtomicUsize::new(0));
		let observed_conn_id = Arc::new(Mutex::new(None::<String>));

		let recv = tokio::spawn({
			let closed = closed.clone();
			let observed_conn_id = observed_conn_id.clone();
			async move {
				while let Some(event) = events_rx.recv().await {
					match event {
						ActorEvent::ConnectionPreflight { reply, .. } => reply.send(Ok(())),
						ActorEvent::ConnectionOpen { reply, .. } => reply.send(Ok(())),
						ActorEvent::ConnectionClosed { conn } => {
							*observed_conn_id.lock() = Some(conn.id().to_owned());
							closed.fetch_add(1, Ordering::SeqCst);
							break;
						}
						other => panic!("unexpected event: {other:?}"),
					}
				}
			}
		});

		let conn = ctx
			.connect_with_state(
				vec![1],
				true,
				Some(HibernatableConnectionMetadata {
					gateway_id: [1, 2, 3, 4],
					request_id: [5, 6, 7, 8],
					..HibernatableConnectionMetadata::default()
				}),
				None,
				async { Ok(vec![9]) },
			)
			.await
			.expect("connection should open");
		let conn_id = conn.id().to_owned();
		ctx.record_connections_updated();
		ctx.reset_sleep_timer();

		let barrier = Arc::new(Barrier::new(2));
		conn.configure_transport_disconnect_handler(Some(Arc::new({
			let barrier = barrier.clone();
			move |_reason| {
				let barrier = barrier.clone();
				Box::pin(async move {
					barrier.wait().await;
					Ok(())
				})
			}
		})));

		let first = tokio::spawn({
			let conn = conn.clone();
			async move { conn.disconnect(Some("first")).await }
		});
		let second = tokio::spawn({
			let conn = conn.clone();
			async move { conn.disconnect(Some("second")).await }
		});

		yield_now().await;
		first
			.await
			.expect("first disconnect task should join")
			.expect("first disconnect should succeed");
		second
			.await
			.expect("second disconnect task should join")
			.expect("second disconnect should succeed");
		recv.await.expect("event receiver should join");

		assert_eq!(closed.load(Ordering::SeqCst), 1);
		assert_eq!(observed_conn_id.lock().as_deref(), Some(conn_id.as_str()));
		assert!(ctx.connection(&conn_id).is_none());

		let pending = ctx.take_pending_hibernation_changes_inner();
		assert!(pending.updated.is_empty());
		assert_eq!(pending.removed, BTreeSet::from([conn_id]));
	}

	#[tokio::test]
	async fn hibernatable_set_state_queues_save_and_non_hibernatable_stays_memory_only() {
		let ctx = ActorContext::new_with_kv(
			"actor-state-dirty",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		let (actor_events_tx, mut actor_events_rx) = mpsc::unbounded_channel();
		let (lifecycle_events_tx, mut lifecycle_events_rx) = mpsc::unbounded_channel();
		ctx.configure_actor_events(Some(actor_events_tx));
		ctx.configure_lifecycle_events(Some(lifecycle_events_tx));

		let open_replies = tokio::spawn(async move {
			for _ in 0..4 {
				match actor_events_rx
					.recv()
					.await
					.expect("open event should arrive")
				{
					ActorEvent::ConnectionPreflight { reply, .. } => reply.send(Ok(())),
					ActorEvent::ConnectionOpen { reply, .. } => reply.send(Ok(())),
					other => panic!("unexpected actor event: {other:?}"),
				}
			}
		});

		let non_hibernatable = ctx
			.connect_with_state(vec![1], false, None, None, async { Ok(vec![2]) })
			.await
			.expect("non-hibernatable connection should open");
		non_hibernatable.set_state(vec![3]);
		assert_eq!(non_hibernatable.state(), vec![3]);
		assert!(
			ctx.dirty_hibernatable_conns_inner().is_empty(),
			"non-hibernatable state changes should not queue persistence"
		);
		assert!(
			next_non_activity_lifecycle_event(&mut lifecycle_events_rx).is_none(),
			"non-hibernatable state changes should not request actor save"
		);

		let hibernatable = ctx
			.connect_with_state(
				vec![4],
				true,
				Some(HibernatableConnectionMetadata {
					gateway_id: [1, 2, 3, 4],
					request_id: [5, 6, 7, 8],
					..HibernatableConnectionMetadata::default()
				}),
				None,
				async { Ok(vec![5]) },
			)
			.await
			.expect("hibernatable connection should open");
		hibernatable.set_state(vec![6]);

		assert_eq!(
			ctx.dirty_hibernatable_conns_inner()
				.into_iter()
				.map(|conn| conn.id().to_owned())
				.collect::<Vec<_>>(),
			vec![hibernatable.id().to_owned()]
		);
		assert_eq!(
			next_non_activity_lifecycle_event(&mut lifecycle_events_rx)
				.expect("hibernatable state change should request save"),
			LifecycleEvent::SaveRequested { immediate: false }
		);

		open_replies
			.await
			.expect("open reply task should join cleanly");
	}

	#[tokio::test(start_paused = true)]
	async fn remove_existing_for_disconnect_has_exactly_one_winner() {
		let ctx = ActorContext::new_with_kv(
			"actor-race",
			"actor",
			Vec::new(),
			"local",
			Kv::new_in_memory(),
		);
		let conn = super::ConnHandle::new("conn-race", vec![1], vec![2], true);
		conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: [1, 2, 3, 4],
			request_id: [5, 6, 7, 8],
			..HibernatableConnectionMetadata::default()
		}));
		ctx.insert_existing(conn);

		let barrier = Arc::new(Barrier::new(2));
		let first = tokio::spawn({
			let ctx = ctx.clone();
			let barrier = barrier.clone();
			async move {
				barrier.wait().await;
				ctx.remove_existing_for_disconnect("conn-race")
					.map(|conn| conn.id().to_owned())
			}
		});
		let second = tokio::spawn({
			let ctx = ctx.clone();
			let barrier = barrier.clone();
			async move {
				barrier.wait().await;
				ctx.remove_existing_for_disconnect("conn-race")
					.map(|conn| conn.id().to_owned())
			}
		});

		let first = first.await.expect("first task should join");
		let second = second.await.expect("second task should join");
		let winners = [first, second].into_iter().flatten().collect::<Vec<_>>();

		assert_eq!(winners, vec!["conn-race".to_owned()]);
		assert!(ctx.connection("conn-race").is_none());

		let pending = ctx.take_pending_hibernation_changes_inner();
		assert!(pending.updated.is_empty());
		assert_eq!(pending.removed, BTreeSet::from(["conn-race".to_owned()]));
	}
}
