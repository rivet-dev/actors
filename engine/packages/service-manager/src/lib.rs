use std::{
	future::Future,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};

use anyhow::{Context, Result, ensure};
use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct Service {
	pub name: &'static str,
	pub kind: ServiceKind,
	pub run: Arc<
		dyn Fn(
				rivet_config::Config,
				rivet_pools::Pools,
			) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>
			+ Send
			+ Sync,
	>,
	pub requires_graceful_shutdown: bool,
}

impl Service {
	pub fn new<F, Fut>(
		name: &'static str,
		kind: ServiceKind,
		run: F,
		requires_graceful_shutdown: bool,
	) -> Self
	where
		F: Fn(rivet_config::Config, rivet_pools::Pools) -> Fut + Send + Sync + 'static,
		Fut: Future<Output = Result<()>> + Send + 'static,
	{
		Self {
			name,
			kind,
			run: Arc::new(move |config, pools| Box::pin(run(config, pools))),
			requires_graceful_shutdown,
		}
	}
}

/// Defines the type of the service. Used for filtering service types to run.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceKind {
	ApiPublic,
	ApiPeer,
	Standalone,
	Singleton,
	Oneshot,
	Cron(CronConfig),
	/// Run no matter what.
	Core,
}

impl ServiceKind {
	fn behavior(&self) -> ServiceBehavior {
		use ServiceKind::*;

		match self {
			ApiPublic | ApiPeer | Standalone | Singleton | Core => ServiceBehavior::Service,
			Oneshot => ServiceBehavior::Oneshot,
			Cron(config) => ServiceBehavior::Cron(config.clone()),
		}
	}

	pub fn eq(&self, other: &Self) -> bool {
		use ServiceKind::*;

		match (self, other) {
			(ApiPublic, ApiPublic)
			| (ApiPeer, ApiPeer)
			| (Standalone, Standalone)
			| (Singleton, Singleton)
			| (Oneshot, Oneshot)
			| (Core, Core) => true,
			(Cron(_), Cron(_)) => true,
			_ => false,
		}
	}
}

/// Defines how a service should be ran.
#[derive(Debug, Clone, PartialEq)]
enum ServiceBehavior {
	/// Spawns a service that will run indefinitely.
	///
	/// If crashes or exits, will be restarted.
	Service,
	/// Runs a task that will exit upon completion.
	///
	/// If crashes, it will be retried indefinitely.
	Oneshot,
	/// Runs a task on a schedule.
	Cron(CronConfig),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CronConfig {
	pub run_immediately: bool,
	pub schedule: String,
}

pub type RunConfig = Arc<RunConfigData>;

pub struct RunConfigData {
	pub services: Vec<Service>,
}

impl RunConfigData {
	/// Replaces an existing service. Throws an error if cannot find service.
	pub fn replace_service(&mut self, service: Service) -> Result<()> {
		let old_len = self.services.len();
		self.services.retain(|x| x.name != service.name);
		ensure!(
			self.services.len() < old_len,
			"could not find instance of service {} to replace",
			service.name
		);
		self.services.push(service);
		Ok(())
	}
}

struct ServiceTask {
	name: String,
	handle: JoinHandle<()>,
	requires_graceful_shutdown: bool,
}

/// Runs services & waits for completion.
///
/// Useful in order to allow for easily configuring an entrypoint where a custom set of services
/// run.
pub async fn start(
	config: rivet_config::Config,
	pools: rivet_pools::Pools,
	mut services: Vec<Service>,
) -> Result<()> {
	services.push(Service::new(
		"metrics",
		ServiceKind::Core,
		|config, _pools| rivet_metrics_server::run_standalone(config),
		false,
	));

	// Spawn services
	tracing::info!(services=?services.len(), "starting services");
	let mut running_services = Vec::new();
	let cron_schedule = tokio_cron_scheduler::JobScheduler::new().await?;

	let mut term_signal = rivet_runtime::TermSignal::get();
	let shutting_down = Arc::new(AtomicBool::new(false));

	for service in services {
		match service.kind.behavior() {
			ServiceBehavior::Service => {
				let config = config.clone();
				let pools = pools.clone();
				let shutting_down = shutting_down.clone();
				let task_name = format!("rivet::service::{}", service.name);

				let join_handle = tokio::task::Builder::new()
					.name(&task_name)
					.spawn(async move {
						tracing::debug!(service=%service.name, "starting service");

						loop {
							match (service.run)(config.clone(), pools.clone()).await {
								Result::Ok(_) => {
									if shutting_down.load(Ordering::SeqCst) {
										tracing::info!(service=%service.name, "service exited");
										break;
									} else {
										tracing::error!(service=%service.name, "service exited unexpectedly");
									}
								}
								Err(err) => {
									tracing::error!(service=%service.name, ?err, "service crashed");

									if shutting_down.load(Ordering::SeqCst) {
										break;
									}
								}
							}

							tokio::time::sleep(Duration::from_secs(1)).await;

							tracing::info!(service=%service.name, "restarting service");
						}
					})
					.context("failed to spawn service")?;

				running_services.push(ServiceTask {
					name: task_name,
					handle: join_handle,
					requires_graceful_shutdown: service.requires_graceful_shutdown,
				});
			}
			ServiceBehavior::Oneshot => {
				let config = config.clone();
				let pools = pools.clone();
				let shutting_down = shutting_down.clone();
				let task_name = format!("rivet::oneoff::{}", service.name);

				let join_handle = tokio::task::Builder::new()
					.name(&task_name)
					.spawn(async move {
						tracing::debug!(oneoff=%service.name, "starting oneoff");

						loop {
							match (service.run)(config.clone(), pools.clone()).await {
								Result::Ok(_) => {
									tracing::debug!(oneoff=%service.name, "oneoff finished");
									break;
								}
								Err(err) => {
									tracing::error!(oneoff=%service.name, ?err, "oneoff crashed");

									if shutting_down.load(Ordering::SeqCst) {
										break;
									} else {
										tokio::time::sleep(Duration::from_secs(1)).await;

										tracing::info!(oneoff=%service.name, "restarting oneoff");
									}
								}
							}
						}
					})
					.context("failed to spawn oneoff")?;

				running_services.push(ServiceTask {
					name: task_name,
					handle: join_handle,
					requires_graceful_shutdown: service.requires_graceful_shutdown,
				});
			}
			ServiceBehavior::Cron(cron_config) => {
				// Spawn immediate task
				if cron_config.run_immediately {
					let service = service.clone();
					let config = config.clone();
					let pools = pools.clone();
					let shutting_down = shutting_down.clone();
					let task_name = format!("rivet::cron_immediate::{}", service.name);

					let join_handle = tokio::task::Builder::new()
						.name(&task_name)
						.spawn(async move {
							tracing::debug!(cron=%service.name, "starting immediate cron");

							for attempt in 1..=8 {
								match (service.run)(config.clone(), pools.clone()).await {
									Result::Ok(_) => {
										tracing::debug!(cron=%service.name, ?attempt, "cron finished");
										break;
									}
									Err(err) => {
										tracing::error!(cron=%service.name, ?attempt, ?err, "cron crashed");

										if shutting_down.load(Ordering::SeqCst) {
											return;
										} else {
											tokio::time::sleep(Duration::from_secs(1)).await;

											tracing::info!(cron=%service.name, ?attempt, "restarting cron");
										}
									}
								}
							}

							tracing::error!(cron=%service.name, "cron failed all restart attempts");
						})
						.context("failed to spawn cron")?;

					running_services.push(ServiceTask {
						name: task_name,
						handle: join_handle,
						requires_graceful_shutdown: service.requires_graceful_shutdown,
					});
				}

				// Spawn cron
				let config = config.clone();
				let pools = pools.clone();
				let service2 = service.clone();
				let shutting_down = shutting_down.clone();
				let task_name = format!("rivet::cron_dummy::{}", service.name);

				cron_schedule
					.add(tokio_cron_scheduler::Job::new_async_tz(
						&cron_config.schedule,
						chrono::Utc,
						move |notification, _| {
							let config = config.clone();
							let pools = pools.clone();
							let service = service2.clone();
							let shutting_down = shutting_down.clone();
							Box::pin(async move {
								tracing::debug!(cron=%service.name, ?notification, "running cron");

								for attempt in 1..=8 {
									match (service.run)(config.clone(), pools.clone()).await {
										Result::Ok(_) => {
											tracing::debug!(cron=%service.name, ?attempt, "cron finished");
											return;
										}
										Err(err) => {
											tracing::error!(cron=%service.name, ?attempt, ?err, "cron crashed");

											if shutting_down.load(Ordering::SeqCst) {
												return;
											} else {
												tokio::time::sleep(Duration::from_secs(1)).await;

												tracing::info!(cron=%service.name, ?attempt, "restarting cron");
											}
										}
									}
								}

								tracing::error!(cron=%service.name, "cron failed all restart attempts");
							})
						},
					)?)
					.await?;

				// Add dummy task to prevent start command from stopping if theres a cron
				let join_handle = tokio::task::Builder::new()
					.name(&task_name)
					.spawn(std::future::pending())
					.context("failed creating dummy cron task")?;

				running_services.push(ServiceTask {
					name: task_name,
					handle: join_handle,
					requires_graceful_shutdown: false,
				});
			}
		}
	}

	cron_schedule.start().await?;

	loop {
		// Waits for all service tasks to complete
		let join_fut = async {
			let mut handle_futs = running_services
				.iter_mut()
				.filter_map(|task| (!task.handle.is_finished()).then_some(&mut task.handle))
				.collect::<FuturesUnordered<_>>();

			while let Some(_) = handle_futs.next().await {}
		};

		tokio::select! {
			_ = join_fut => {
				tracing::info!("all services finished");
				break;
			}
			abort = term_signal.recv() => {
				if !shutting_down.load(Ordering::SeqCst) {
					// Spawn force exit task in case of a lingering task
					let force_shutdown_duration = config.runtime.force_shutdown_duration();
					tokio::spawn(async move {
						tracing::info!(?force_shutdown_duration, "force shutdown timer started");
						tokio::time::sleep(force_shutdown_duration).await;
						tracing::warn!("force shutdown timeout reached, exiting process, this indicates a bug");
						std::process::exit(1);
					});
				}

				shutting_down.store(true, Ordering::SeqCst);

				// Abort services that don't require graceful shutdown
				running_services.retain(|task| {
					if !task.requires_graceful_shutdown {
						tracing::debug!(name=%task.name, "aborting service");
						task.handle.abort();
					}

					task.requires_graceful_shutdown
				});

				if abort {
					// Give time for services to handle final abort
					tokio::time::sleep(Duration::from_millis(50)).await;
					rivet_runtime::shutdown().await;

					break;
				}
			}
		}
	}

	// Shut down udb
	pools.udb()?.shutdown().await;

	// Stops term signal handler bg task
	rivet_runtime::TermSignal::stop();

	Ok(())
}
