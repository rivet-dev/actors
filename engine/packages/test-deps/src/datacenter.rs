use anyhow::Result;
use std::collections::HashMap;
use uuid::Uuid;

use crate::TestDeps;
use rivet_test_deps_docker::{TestDatabase, TestPubSub};

/// Datacenters for the same test ID & datacenter label will preserve the same storage if stopped
/// and started again. This is helpful for testing restarting datacenters with new configs.
pub async fn setup_single_datacenter(
	test_id: Uuid,
	dc_label: u16,
	datacenters: HashMap<String, rivet_config::config::topology::Datacenter>,
	api_peer_port: u16,
	guard_port: u16,
) -> Result<TestDeps> {
	let test_database = TestDatabase::from_env();
	let test_pubsub = TestPubSub::from_env();

	let dc = datacenters
		.iter()
		.find(|(_, x)| x.datacenter_label == dc_label)
		.expect("invalid dc label")
		.1
		.clone();

	tracing::info!(
		dc = dc.datacenter_label,
		?test_database,
		?test_pubsub,
		"setting up test dependencies with configuration"
	);

	// Setup database
	let mut container_names = Vec::new();
	let (db_config, mut db_docker_config) =
		test_database.config(test_id, dc.datacenter_label).await?;
	if let Some(docker_config) = &mut db_docker_config {
		let was_started = docker_config.start().await?;
		container_names.push(docker_config.container_name.clone());

		if was_started {
			tracing::info!(
				dc = dc.datacenter_label,
				port = docker_config.port_mapping.0,
				"waiting for database to be ready"
			);
			test_database.wait_for_ready(&docker_config).await?;
		}
	}

	// Setup pubsub
	let (pubsub_config, mut pubsub_docker_config) =
		test_pubsub.config(test_id, dc.datacenter_label).await?;
	if let Some(docker_config) = &mut pubsub_docker_config {
		docker_config.start().await?;
		container_names.push(docker_config.container_name.clone());
	}

	tracing::info!(
		dc = dc.datacenter_label,
		"containers started, waiting for services to be ready"
	);

	tracing::info!(
		dc = dc.datacenter_label,
		api_peer_port,
		guard_port,
		"using ports for test services"
	);

	// Setup config
	let mut root = rivet_config::config::Root::default();
	root.database = Some(db_config);
	root.pubsub = Some(pubsub_config);
	root.api_peer = Some(rivet_config::config::ApiPeer {
		port: Some(api_peer_port),
		..Default::default()
	});

	root.topology = Some(rivet_config::config::topology::Topology {
		datacenter_label: dc.datacenter_label,
		datacenters: rivet_config::config::topology::DatacentersRepr::Map(datacenters),
	});

	root.guard = Some(rivet_config::config::guard::Guard {
		host: None,
		port: Some(guard_port),
		https: None,
		..Default::default()
	});
	root.metrics = rivet_config::config::metrics::Metrics {
		host: None,
		port: Some(0),
	};

	// Use short timeouts for tests
	root.pegboard = Some(rivet_config::config::pegboard::Pegboard {
		actor_start_threshold: Some(3_000), // 3 seconds instead of 30
		serverless_base_retry_timeout: Some(500), // 500ms instead of 2s
		serverless_backoff_max_exponent: Some(2), // Max 2^2 = 4x base = 2s
		..Default::default()
	});

	tracing::info!(
		dc = dc.datacenter_label,
		"creating test configuration and pools"
	);
	let config = rivet_config::Config::from_root(root);
	let pools = rivet_pools::Pools::test(config.clone()).await?;

	tracing::info!(dc = dc.datacenter_label, "test dependencies setup complete");
	tracing::info!(dc = dc.datacenter_label, config = ?*config, "test dependencies config");
	Ok(TestDeps {
		pools,
		config,
		container_names,
		api_peer_port,
		guard_port,
		stop_docker_containers_on_drop: true,
	})
}
