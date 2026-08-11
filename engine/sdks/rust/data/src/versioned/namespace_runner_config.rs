use anyhow::{Ok, Result, bail};
use vbare::OwnedVersionedData;

use crate::generated::*;

pub enum NamespaceRunnerConfig {
	V1(pegboard_namespace_runner_config_v1::Data),
	V2(pegboard_namespace_runner_config_v2::RunnerConfig),
	V3(pegboard_namespace_runner_config_v3::RunnerConfig),
	V4(pegboard_namespace_runner_config_v4::RunnerConfig),
	V5(pegboard_namespace_runner_config_v5::RunnerConfig),
	V6(pegboard_namespace_runner_config_v6::RunnerConfig),
}

impl OwnedVersionedData for NamespaceRunnerConfig {
	type Latest = pegboard_namespace_runner_config_v6::RunnerConfig;

	fn wrap_latest(latest: pegboard_namespace_runner_config_v6::RunnerConfig) -> Self {
		NamespaceRunnerConfig::V6(latest)
	}

	fn unwrap_latest(self) -> Result<Self::Latest> {
		#[allow(irrefutable_let_patterns)]
		if let NamespaceRunnerConfig::V6(data) = self {
			Ok(data)
		} else {
			bail!("version not latest");
		}
	}

	fn deserialize_version(payload: &[u8], version: u16) -> Result<Self> {
		match version {
			1 => Ok(NamespaceRunnerConfig::V1(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			2 => Ok(NamespaceRunnerConfig::V2(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			3 => Ok(NamespaceRunnerConfig::V3(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			4 => Ok(NamespaceRunnerConfig::V4(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			5 => Ok(NamespaceRunnerConfig::V5(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			6 => Ok(NamespaceRunnerConfig::V6(
				rivet_util::serde::bare_from_slice!(payload)?,
			)),
			_ => bail!("invalid version: {version}"),
		}
	}

	fn serialize_version(self, _version: u16) -> Result<Vec<u8>> {
		match self {
			NamespaceRunnerConfig::V1(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			NamespaceRunnerConfig::V2(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			NamespaceRunnerConfig::V3(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			NamespaceRunnerConfig::V4(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			NamespaceRunnerConfig::V5(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
			NamespaceRunnerConfig::V6(data) => {
				rivet_util::serde::bare_to_vec!(&data).map_err(Into::into)
			}
		}
	}

	fn deserialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v1_to_v2,
			Self::v2_to_v3,
			Self::v3_to_v4,
			Self::v4_to_v5,
			Self::v5_to_v6,
		]
	}

	fn serialize_converters() -> Vec<impl Fn(Self) -> Result<Self>> {
		vec![
			Self::v6_to_v5,
			Self::v5_to_v4,
			Self::v4_to_v3,
			Self::v3_to_v2,
			Self::v2_to_v1,
		]
	}
}

impl NamespaceRunnerConfig {
	fn v1_to_v2(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V1(pegboard_namespace_runner_config_v1::Data::Serverless(
			serverless,
		)) = self
		{
			let pegboard_namespace_runner_config_v1::Serverless {
				url,
				headers,
				request_lifespan,
				slots_per_runner,
				min_runners,
				max_runners,
				runners_margin,
			} = serverless;

			Ok(NamespaceRunnerConfig::V2(
				pegboard_namespace_runner_config_v2::RunnerConfig {
					metadata: None,
					kind: pegboard_namespace_runner_config_v2::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v2::Serverless {
							url,
							headers,
							request_lifespan,
							slots_per_runner,
							min_runners,
							max_runners,
							runners_margin,
						},
					),
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v1(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V2(config) = self {
			let pegboard_namespace_runner_config_v2::RunnerConfig { kind, .. } = config;

			match kind {
				pegboard_namespace_runner_config_v2::RunnerConfigKind::Serverless(serverless) => {
					let pegboard_namespace_runner_config_v2::Serverless {
						url,
						headers,
						request_lifespan,
						slots_per_runner,
						min_runners,
						max_runners,
						runners_margin,
					} = serverless;

					Ok(NamespaceRunnerConfig::V1(
						pegboard_namespace_runner_config_v1::Data::Serverless(
							pegboard_namespace_runner_config_v1::Serverless {
								url,
								headers,
								request_lifespan,
								slots_per_runner,
								min_runners,
								max_runners,
								runners_margin,
							},
						),
					))
				}
				pegboard_namespace_runner_config_v2::RunnerConfigKind::Normal => {
					bail!("namespace runner config v1 does not support normal runner config")
				}
			}
		} else {
			bail!("unexpected version");
		}
	}

	fn v2_to_v3(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V2(config) = self {
			let pegboard_namespace_runner_config_v2::RunnerConfig { kind, metadata } = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v2::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v3::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v3::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
						},
					)
				}
				pegboard_namespace_runner_config_v2::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v3::RunnerConfigKind::Normal
				}
			};

			Ok(NamespaceRunnerConfig::V3(
				pegboard_namespace_runner_config_v3::RunnerConfig {
					kind,
					metadata,
					// Default to false for v2 -> v3 migration
					drain_on_version_upgrade: false,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v2(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V3(config) = self {
			let pegboard_namespace_runner_config_v3::RunnerConfig { kind, metadata, .. } = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v3::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v2::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v2::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
						},
					)
				}
				pegboard_namespace_runner_config_v3::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v2::RunnerConfigKind::Normal
				}
			};

			Ok(NamespaceRunnerConfig::V2(
				pegboard_namespace_runner_config_v2::RunnerConfig { kind, metadata },
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v3_to_v4(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V3(config) = self {
			let pegboard_namespace_runner_config_v3::RunnerConfig {
				kind,
				metadata,
				drain_on_version_upgrade,
			} = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v3::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v4::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v4::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
							// Default to None for v3 -> v4 migration
							metadata_poll_interval: None,
						},
					)
				}
				pegboard_namespace_runner_config_v3::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v4::RunnerConfigKind::Normal
				}
			};

			Ok(NamespaceRunnerConfig::V4(
				pegboard_namespace_runner_config_v4::RunnerConfig {
					kind,
					metadata,
					drain_on_version_upgrade,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v4_to_v3(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V4(config) = self {
			let pegboard_namespace_runner_config_v4::RunnerConfig {
				kind,
				metadata,
				drain_on_version_upgrade,
			} = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v4::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v3::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v3::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
							// metadata_poll_interval is dropped in downgrade
						},
					)
				}
				pegboard_namespace_runner_config_v4::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v3::RunnerConfigKind::Normal
				}
			};

			Ok(NamespaceRunnerConfig::V3(
				pegboard_namespace_runner_config_v3::RunnerConfig {
					kind,
					metadata,
					drain_on_version_upgrade,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v4_to_v5(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V4(config) = self {
			let pegboard_namespace_runner_config_v4::RunnerConfig {
				kind,
				metadata,
				drain_on_version_upgrade,
			} = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v4::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v5::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v5::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							// Default to max_runners for v4 -> v5 migration
							max_concurrent_actors: serverless.max_runners as u64,
							// Default to the runner stop window.
							drain_grace_period: 30 * 60,
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
							metadata_poll_interval: serverless.metadata_poll_interval,
						},
					)
				}
				pegboard_namespace_runner_config_v4::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v5::RunnerConfigKind::Normal
				}
			};

			Ok(NamespaceRunnerConfig::V5(
				pegboard_namespace_runner_config_v5::RunnerConfig {
					kind,
					metadata,
					drain_on_version_upgrade,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v5_to_v6(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V5(config) = self {
			let pegboard_namespace_runner_config_v5::RunnerConfig {
				kind,
				metadata,
				drain_on_version_upgrade,
			} = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v5::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v6::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v6::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							max_concurrent_actors: serverless.max_concurrent_actors,
							drain_grace_period: serverless.drain_grace_period,
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
							metadata_poll_interval: serverless.metadata_poll_interval,
							drain_on_version_upgrade,
							// Default to 0 for v5 -> v6 migration
							actor_eviction_delay: 0,
							actor_eviction_period: 0,
							actor_eviction_rate: 1.0,
						},
					)
				}
				pegboard_namespace_runner_config_v5::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v6::RunnerConfigKind::Normal(
						pegboard_namespace_runner_config_v6::Normal {
							drain_on_version_upgrade,
							// Default to 0 for v5 -> v6 migration
							actor_eviction_delay: 0,
							actor_eviction_period: 0,
							actor_eviction_rate: 1.0,
						},
					)
				}
			};

			Ok(NamespaceRunnerConfig::V6(
				pegboard_namespace_runner_config_v6::RunnerConfig { kind, metadata },
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v6_to_v5(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V6(config) = self {
			let pegboard_namespace_runner_config_v6::RunnerConfig { kind, metadata } = config;

			let (kind, drain_on_version_upgrade) = match kind {
				pegboard_namespace_runner_config_v6::RunnerConfigKind::Serverless(serverless) => {
					let drain_on_version_upgrade = serverless.drain_on_version_upgrade;
					(
						pegboard_namespace_runner_config_v5::RunnerConfigKind::Serverless(
							pegboard_namespace_runner_config_v5::Serverless {
								url: serverless.url,
								headers: serverless.headers,
								request_lifespan: serverless.request_lifespan,
								max_concurrent_actors: serverless.max_concurrent_actors,
								drain_grace_period: serverless.drain_grace_period,
								slots_per_runner: serverless.slots_per_runner,
								min_runners: serverless.min_runners,
								max_runners: serverless.max_runners,
								runners_margin: serverless.runners_margin,
								metadata_poll_interval: serverless.metadata_poll_interval,
								// actor_eviction_period and actor_eviction_rate are dropped in downgrade
							},
						),
						drain_on_version_upgrade,
					)
				}
				pegboard_namespace_runner_config_v6::RunnerConfigKind::Normal(normal) => (
					pegboard_namespace_runner_config_v5::RunnerConfigKind::Normal,
					normal.drain_on_version_upgrade,
				),
			};

			Ok(NamespaceRunnerConfig::V5(
				pegboard_namespace_runner_config_v5::RunnerConfig {
					kind,
					metadata,
					drain_on_version_upgrade,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}

	fn v5_to_v4(self) -> Result<Self> {
		if let NamespaceRunnerConfig::V5(config) = self {
			let pegboard_namespace_runner_config_v5::RunnerConfig {
				kind,
				metadata,
				drain_on_version_upgrade,
			} = config;

			let kind = match kind {
				pegboard_namespace_runner_config_v5::RunnerConfigKind::Serverless(serverless) => {
					pegboard_namespace_runner_config_v4::RunnerConfigKind::Serverless(
						pegboard_namespace_runner_config_v4::Serverless {
							url: serverless.url,
							headers: serverless.headers,
							request_lifespan: serverless.request_lifespan,
							// max_concurrent_actors is dropped in downgrade
							slots_per_runner: serverless.slots_per_runner,
							min_runners: serverless.min_runners,
							max_runners: serverless.max_runners,
							runners_margin: serverless.runners_margin,
							metadata_poll_interval: serverless.metadata_poll_interval,
						},
					)
				}
				pegboard_namespace_runner_config_v5::RunnerConfigKind::Normal => {
					pegboard_namespace_runner_config_v4::RunnerConfigKind::Normal
				}
			};

			Ok(NamespaceRunnerConfig::V4(
				pegboard_namespace_runner_config_v4::RunnerConfig {
					kind,
					metadata,
					drain_on_version_upgrade,
				},
			))
		} else {
			bail!("unexpected version");
		}
	}
}
