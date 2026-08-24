use anyhow::Result;
use rivet_test_deps_docker::TestPubSub;
use std::{sync::Arc, time::Duration};
use universalpubsub::{NextOutput, PubSub, PublishOpts};
use uuid::Uuid;

fn setup_logging() {
	let _ = tracing_subscriber::fmt()
		.with_env_filter("debug")
		.with_ansi(false)
		.with_test_writer()
		.try_init();
}

#[tokio::test]
async fn test_nats_driver_with_memory_reconnect() {
	setup_logging();

	let test_id = Uuid::new_v4();
	let (pubsub_config, docker_config) = TestPubSub::Nats.config(test_id, 1).await.unwrap();
	let mut docker = docker_config.unwrap();
	docker.start().await.unwrap();
	tokio::time::sleep(Duration::from_secs(1)).await;

	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!();
	};

	use std::str::FromStr;
	let server_addrs = nats
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
		.collect::<Result<Vec<_>, _>>()
		.unwrap();

	let driver = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await
	.unwrap();
	let pubsub = PubSub::new_with_memory_optimization(Arc::new(driver), true);

	test_all_inner(&pubsub, &docker, true).await;
}

#[tokio::test]
async fn test_nats_driver_without_memory_reconnect() {
	setup_logging();

	let test_id = Uuid::new_v4();
	let (pubsub_config, docker_config) = TestPubSub::Nats.config(test_id, 1).await.unwrap();
	let mut docker = docker_config.unwrap();
	docker.start().await.unwrap();
	tokio::time::sleep(Duration::from_secs(1)).await;

	let rivet_config::config::PubSub::Nats(nats) = pubsub_config else {
		unreachable!();
	};

	use std::str::FromStr;
	let server_addrs = nats
		.addresses
		.iter()
		.map(|addr| format!("nats://{addr}"))
		.map(|url| async_nats::ServerAddr::from_str(url.as_ref()))
		.collect::<Result<Vec<_>, _>>()
		.unwrap();

	let driver = universalpubsub::driver::nats::NatsDriver::connect(
		async_nats::ConnectOptions::new(),
		&server_addrs[..],
	)
	.await
	.unwrap();
	let pubsub = PubSub::new_with_memory_optimization(Arc::new(driver), false);

	test_all_inner(&pubsub, &docker, true).await;
}

async fn test_all_inner(
	pubsub: &PubSub,
	docker: &rivet_test_deps_docker::DockerRunConfig,
	supports_subscribe_while_stopped: bool,
) {
	test_reconnect_inner(&pubsub, &docker).await;
	test_publish_while_stopped(&pubsub, &docker).await;
	// The table-backed Postgres driver must read its cursor and register in the
	// responder table when subscribing, so it cannot subscribe while the backend is
	// fully down. NATS buffers the subscribe and reconnects, so it can.
	if supports_subscribe_while_stopped {
		test_subscribe_while_stopped(&pubsub, &docker).await;
	}
}

async fn test_reconnect_inner(pubsub: &PubSub, docker: &rivet_test_deps_docker::DockerRunConfig) {
	tracing::info!("testing reconnect functionality");

	// Open subscription
	let mut subscriber = pubsub.subscribe("test.reconnect").await.unwrap();
	tracing::info!("opened initial subscription");

	// Test publish/receive message before restart
	let message_before = b"message before restart";
	pubsub
		.publish("test.reconnect", message_before, PublishOpts::broadcast())
		.await
		.unwrap();
	pubsub.flush().await.unwrap();

	match subscriber.next().await.unwrap() {
		NextOutput::Message(msg) => {
			assert_eq!(
				msg.payload, message_before,
				"message before restart should match"
			);
			tracing::info!("received message before restart");
		}
		NextOutput::Unsubscribed => {
			panic!("unexpected unsubscribe before restart");
		}
		NextOutput::NoResponders => {
			panic!("unexpected no responders before restart");
		}
	}

	// Restart container
	tracing::info!("restarting docker container");
	docker.restart().await.unwrap();

	// Test publish/receive message after restart
	//
	// This should retry under the hood, since the container will still be starting
	let message_after = b"message after restart";
	pubsub
		.publish("test.reconnect", message_after, PublishOpts::broadcast())
		.await
		.unwrap();

	pubsub.flush().await.unwrap();

	// Try to receive with timeout to handle reconnection delays
	let receive_timeout = Duration::from_secs(10);
	let receive_result = tokio::time::timeout(receive_timeout, subscriber.next()).await;

	match receive_result {
		Ok(Ok(NextOutput::Message(msg))) => {
			assert_eq!(
				msg.payload, message_after,
				"message after restart should match"
			);
			tracing::info!("received message after restart - reconnection successful");
		}
		Ok(Ok(NextOutput::Unsubscribed)) => {
			panic!("unexpected unsubscribe after restart");
		}
		Ok(Ok(NextOutput::NoResponders)) => {
			panic!("unexpected no responders after restart");
		}
		Ok(Err(e)) => {
			panic!("error receiving message after restart: {}", e);
		}
		Err(_) => {
			panic!("timeout receiving message after restart");
		}
	}

	tracing::info!("reconnect test completed successfully");
}

async fn test_publish_while_stopped(
	pubsub: &PubSub,
	docker: &rivet_test_deps_docker::DockerRunConfig,
) {
	tracing::info!("testing publish while container stopped");

	// 1. Subscribe
	let mut subscriber = pubsub.subscribe("test.publish_stopped").await.unwrap();
	tracing::info!("opened subscription");

	// 2. Stop container
	tracing::info!("stopping docker container");
	docker.stop_container().await.unwrap();
	tokio::time::sleep(Duration::from_secs(2)).await;

	// 3. Publish while stopped (should queue/retry)
	let message = b"message while stopped";
	let publish_handle = tokio::spawn({
		let pubsub = pubsub.clone();
		let message = message.to_vec();
		async move {
			pubsub
				.publish("test.publish_stopped", &message, PublishOpts::broadcast())
				.await
		}
	});

	// 4. Start container
	tokio::time::sleep(Duration::from_secs(3)).await;
	tracing::info!("starting docker container");
	docker.start_container().await.unwrap();
	tokio::time::sleep(Duration::from_secs(5)).await;

	// Wait for publish to complete
	publish_handle.await.unwrap().unwrap();
	pubsub.flush().await.unwrap();

	// 5. Receive message
	tracing::info!("waiting for message");
	let receive_timeout = Duration::from_secs(5);
	let receive_result = tokio::time::timeout(receive_timeout, subscriber.next()).await;

	match receive_result {
		Ok(Ok(NextOutput::Message(msg))) => {
			assert_eq!(
				msg.payload, message,
				"message published while stopped should be received"
			);
			tracing::info!("received message published while stopped - reconnection successful");
		}
		Ok(Ok(NextOutput::Unsubscribed)) => {
			panic!("unexpected unsubscribe");
		}
		Ok(Ok(NextOutput::NoResponders)) => {
			panic!("unexpected no responders");
		}
		Ok(Err(e)) => {
			panic!("error receiving message: {}", e);
		}
		Err(_) => {
			panic!("timeout receiving message");
		}
	}

	tracing::info!("publish while stopped test completed successfully");
}

async fn test_subscribe_while_stopped(
	pubsub: &PubSub,
	docker: &rivet_test_deps_docker::DockerRunConfig,
) {
	tracing::info!("testing subscribe while container stopped");

	// 1. Subscribe & test publish & unsubscribe
	let mut subscriber = pubsub.subscribe("test.subscribe_stopped").await.unwrap();
	tracing::info!("opened initial subscription");

	let test_message = b"test message";
	pubsub
		.publish(
			"test.subscribe_stopped",
			test_message,
			PublishOpts::broadcast(),
		)
		.await
		.unwrap();
	pubsub.flush().await.unwrap();

	match subscriber.next().await.unwrap() {
		NextOutput::Message(msg) => {
			assert_eq!(msg.payload, test_message, "initial message should match");
			tracing::info!("received initial test message");
		}
		NextOutput::Unsubscribed => {
			panic!("unexpected unsubscribe");
		}
		NextOutput::NoResponders => {
			panic!("unexpected no responders");
		}
	}

	drop(subscriber); // Drop to unsubscribe
	tracing::info!("unsubscribed from initial subscription");

	// 2. Stop container
	tracing::info!("stopping docker container");
	docker.stop_container().await.unwrap();
	tokio::time::sleep(Duration::from_secs(2)).await;

	// 3. Subscribe while stopped
	let subscribe_handle = tokio::spawn({
		let pubsub = pubsub.clone();
		async move { pubsub.subscribe("test.subscribe_stopped").await }
	});

	// 4. Start container
	tokio::time::sleep(Duration::from_secs(3)).await;
	tracing::info!("starting docker container");
	docker.start_container().await.unwrap();
	tokio::time::sleep(Duration::from_secs(5)).await;

	// Wait for subscription to complete
	let mut new_subscriber = subscribe_handle.await.unwrap().unwrap();
	tracing::info!("new subscription established after reconnect");

	// 5. Publish message
	let final_message = b"message after reconnect";
	pubsub
		.publish(
			"test.subscribe_stopped",
			final_message,
			PublishOpts::broadcast(),
		)
		.await
		.unwrap();
	pubsub.flush().await.unwrap();

	// 6. Receive
	let receive_timeout = Duration::from_secs(10);
	let receive_result = tokio::time::timeout(receive_timeout, new_subscriber.next()).await;

	match receive_result {
		Ok(Ok(NextOutput::Message(msg))) => {
			assert_eq!(
				msg.payload, final_message,
				"message after reconnect should match"
			);
			tracing::info!("received message after reconnect - subscription successful");
		}
		Ok(Ok(NextOutput::Unsubscribed)) => {
			panic!("unexpected unsubscribe");
		}
		Ok(Ok(NextOutput::NoResponders)) => {
			panic!("unexpected no responders");
		}
		Ok(Err(e)) => {
			panic!("error receiving message: {}", e);
		}
		Err(_) => {
			panic!("timeout receiving message");
		}
	}

	tracing::info!("subscribe while stopped test completed successfully");
}
