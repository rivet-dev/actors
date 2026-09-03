use anyhow::Result;

use crate::engine_process::{EngineProcessManager, EngineResolverConfig};
use crate::registry::ServeConfig;
use crate::services_process::{SERVICES_POOL_NAME, ServicesProcessConfig, ServicesProcessManager};

/// Owns the processes RivetKit starts for a local development environment.
///
/// The Engine and actor hosts have deliberately different lifetimes: the Engine
/// is reusable and outlives the application process, while Services is
/// tied to this host and shuts down with it. Keeping the orchestration here
/// prevents serverful and serverless runtimes from growing separate startup
/// behavior without forcing those incompatible lifetime rules into one generic
/// subprocess abstraction.
#[derive(Debug)]
pub(crate) struct DevelopmentProcessManager {
	_engine: EngineProcessManager,
	services: Option<ServicesProcessManager>,
}

impl DevelopmentProcessManager {
	pub(crate) async fn start(config: &ServeConfig) -> Result<Self> {
		let engine = EngineProcessManager::start_or_reuse(EngineResolverConfig::from_parts(
			&config.endpoint,
			config.engine_binary_path.clone(),
			config.engine_host.clone(),
			config.engine_port,
			config.engine_auto_download,
		))
		.await?;

		let services = if config.start_services {
			Some(
				ServicesProcessManager::start(ServicesProcessConfig {
					binary_path: config.services_binary_path.clone(),
					endpoint: config.endpoint.clone(),
					token: config.token.clone(),
					namespace: config.namespace.clone(),
					pool_name: SERVICES_POOL_NAME.to_owned(),
					engine_protocol_version: rivet_envoy_client::protocol::PROTOCOL_VERSION,
					rivetkit_version: env!("CARGO_PKG_VERSION").to_owned(),
				})
				.await?,
			)
		} else {
			None
		};

		Ok(Self {
			_engine: engine,
			services,
		})
	}

	pub(crate) async fn shutdown(mut self) {
		if let Some(services) = self.services.take() {
			services.shutdown().await;
		}
	}
}
