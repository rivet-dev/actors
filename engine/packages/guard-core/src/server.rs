use std::{
	net::SocketAddr,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, Instant},
};

use anyhow::Result;
use futures_util::FutureExt;
use hyper::service::service_fn;
use rivet_runtime::TermSignal;
use tokio_rustls::TlsAcceptor;
use tracing::Instrument;

use crate::cert_resolver::{CertResolverFn, create_tls_config};
use crate::metrics;
use crate::proxy_service::ProxyServiceFactory;
use crate::route::{CacheKeyFn, RoutingFn};

const SHUTDOWN_PROGRESS_INTERVAL: Duration = Duration::from_secs(7);

// Start the server
#[tracing::instrument(skip_all)]
pub async fn run_server(
	config: rivet_config::Config,
	routing_fn: RoutingFn,
	cache_key_fn: CacheKeyFn,
	cert_resolver_fn: Option<CertResolverFn>,
) -> Result<()> {
	// Set up HTTP server
	let http_addr: std::net::SocketAddr = (config.guard().host(), config.guard().port()).into();
	let http_factory = Arc::new(ProxyServiceFactory::new(
		config.clone(),
		routing_fn.clone(),
		cache_key_fn.clone(),
	));
	let http_listener = tokio::net::TcpListener::bind(http_addr).await?;

	// Set up HTTPS server (if configured)
	let (https_addr, https_factory, https_listener, https_acceptor) = if let Some(https) =
		&config.guard().https
	{
		let https_addr: std::net::SocketAddr = ([0, 0, 0, 0], https.port).into();
		let https_factory = Arc::new(ProxyServiceFactory::new(
			config.clone(),
			routing_fn.clone(),
			cache_key_fn.clone(),
		));
		let listener = tokio::net::TcpListener::bind(https_addr).await?;

		// Configure TLS if resolver function is provided
		let acceptor = if let Some(resolver_fn) = cert_resolver_fn {
			// Create a TLS server config using our certificate resolver
			let server_config = create_tls_config(resolver_fn);

			Some(TlsAcceptor::from(Arc::new(server_config)))
		} else {
			tracing::warn!("No TLS certificate resolver provided, HTTPS will not work properly");
			None
		};

		(
			Some(https_addr),
			Some(https_factory),
			Some(listener),
			acceptor,
		)
	} else {
		(None, None, None, None)
	};

	let server = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
	let graceful = hyper_util::server::graceful::GracefulShutdown::new();
	let mut term_signal = TermSignal::get();
	let tcp_nodelay = config.guard().tcp_nodelay();

	tracing::info!("HTTP server listening on {}", http_addr);
	if let Some(addr) = &https_addr {
		tracing::info!("HTTPS server listening on {}", addr);
	}

	// Helper function to process regular connections
	#[tracing::instrument(skip_all, fields(?remote_addr))]
	fn process_connection(
		tcp_stream: tokio::net::TcpStream,
		remote_addr: SocketAddr,
		factory_clone: Arc<ProxyServiceFactory>,
		tcp_nodelay: bool,
		server: &hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
		graceful: &hyper_util::server::graceful::GracefulShutdown,
		port_type_str: String,
	) {
		let connection_start = Instant::now();
		metrics::TCP_CONNECTION_PENDING.inc();
		metrics::TCP_CONNECTION_TOTAL.inc();

		if tcp_nodelay && let Err(err) = tcp_stream.set_nodelay(true) {
			tracing::debug!(?err, "failed to enable tcp nodelay");
		}

		let io = hyper_util::rt::TokioIo::new(tcp_stream);

		// Create a proxy service instance for this connection
		let proxy_service = factory_clone.create_service(remote_addr);
		let client_disconnect = proxy_service.client_disconnect_token();

		// Using service_fn to convert our function into a hyper service
		let service = service_fn(move |req| {
			let service_clone = proxy_service.clone();
			async move { service_clone.process(req).await }
		});

		// Serve the connection with graceful shutdown support
		let conn = server.serve_connection_with_upgrades(io, service);
		let conn = graceful.watch(conn.into_owned());

		tokio::spawn(
			async move {
				if let Err(err) = conn.await {
					tracing::warn!("{} connection error: {}", port_type_str, err);
				}
				client_disconnect.cancel();
				tracing::debug!("{} connection dropped: {}", port_type_str, remote_addr);

				let connection_duration = connection_start.elapsed().as_secs_f64();
				metrics::TCP_CONNECTION_DURATION.observe(connection_duration);
				metrics::TCP_CONNECTION_PENDING.dec();
			}
			.instrument(tracing::info_span!(parent: None, "process_connection_task")),
		);
	}

	// Accept connections until we receive a shutdown signal
	loop {
		let res = tokio::select! {
			conn = http_listener.accept() => {
				match conn {
					Result::Ok((tcp_stream, remote_addr)) => {
						process_connection(
							tcp_stream,
							remote_addr,
							http_factory.clone(),
							tcp_nodelay,
							&server,
							&graceful,
							"HTTP".to_string()
						);
					},
					Err(err) => {
						tracing::debug!(?err, "accept error on HTTP port");
						tokio::time::sleep(Duration::from_secs(1)).await;
					}
				}
				Ok(())
			}
			conn = async {
				match &https_listener {
					Some(listener) => Some(listener.accept().await),
					None => {
						// If HTTPS is not configured, this future never returns
						std::future::pending::<Option<_>>().await
					}
				}
			} => {
				if let Some(conn) = conn {
					match conn {
						Ok((tcp_stream, remote_addr)) => {
							if let Some(factory) = &https_factory {
								// Check if we have a TLS acceptor
								if let Some(acceptor) = &https_acceptor {
									// Handle TLS connection
									let https_factory_clone = factory.clone();
									let acceptor_clone = acceptor.clone();

									// Accept TLS connection in a separate task to avoid ownership issues
									tokio::spawn(async move {
										let connection_start = Instant::now();
										metrics::TCP_CONNECTION_PENDING.inc();
										metrics::TCP_CONNECTION_TOTAL.inc();

										if tcp_nodelay
											&& let Err(err) = tcp_stream.set_nodelay(true)
										{
											tracing::debug!(?err, "failed to enable tcp nodelay");
										}

										match acceptor_clone
											.accept(tcp_stream)
											.instrument(tracing::info_span!("accept"))
											.await
										{
											Result::Ok(tls_stream) => {
												tracing::debug!("TLS handshake successful for {}", remote_addr);

												// Create service for this connection
												let io = hyper_util::rt::TokioIo::new(tls_stream);
												let proxy_service = https_factory_clone.create_service(remote_addr);
												let client_disconnect = proxy_service.client_disconnect_token();

												// Using service_fn to convert our function into a hyper service
												let service = service_fn(move |req| {
													let service_clone = proxy_service.clone();

													async move {
														service_clone.process(req).await
													}
												});

												// Create a new server for each connection
												let conn_server = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());

												// Serve the connection (no graceful shutdown in spawned task)
												if let Err(err) = conn_server.serve_connection_with_upgrades(io, service).await {
													tracing::debug!(?err, "HTTPS connection error");
												}
												client_disconnect.cancel();

												tracing::debug!("HTTPS connection dropped: {}", remote_addr);
											},
											Err(err) => {
												tracing::debug!(?err, "TLS handshake failed for {}", remote_addr);
											}
										}

										let connection_duration = connection_start.elapsed().as_secs_f64();
										metrics::TCP_CONNECTION_DURATION.observe(connection_duration);
										metrics::TCP_CONNECTION_PENDING.dec();
									}.instrument(tracing::info_span!(parent: None, "process_tls_connection_task")));
								} else {
									// Fallback to non-TLS handling (useful for testing)
									// In production, this would not secure the connection
									tracing::warn!("HTTPS port configured but no TLS acceptor available");
									process_connection(
										tcp_stream,
										remote_addr,
										factory.clone(),
										tcp_nodelay,
										&server,
										&graceful,
										"HTTPS (unsecured)".to_string()
									);
								}
							}
						},
						Err(err) => {
							tracing::debug!(?err, "accept error on HTTPS port");
							tokio::time::sleep(Duration::from_secs(1)).await;
						}
					}
				}

				anyhow::Ok(())
			}
			_ = term_signal.recv() => {
				break;
			}
		};

		if let Err(err) = res {
			tracing::error!(?err, "error in guard server loop");
		}
	}

	let shutdown_duration = config.runtime.guard_shutdown_duration();
	let remaining_tasks = http_factory.remaining_tasks()
		+ https_factory
			.as_ref()
			.map(|f| f.remaining_tasks())
			.unwrap_or(0);
	tracing::info!(%remaining_tasks, hyper_shutdown=%false, duration=?shutdown_duration, "starting guard shutdown");

	// Signifies that the hyper graceful shutdown completed
	let hyper_shutdown = Arc::new(AtomicBool::new(false));

	let hyper_shutdown2 = hyper_shutdown.clone();
	let http_factory2 = http_factory.clone();
	let https_factory2 = https_factory.clone();
	let mut complete_fut = async move {
		// Wait until remaining requests finish
		graceful.shutdown().await;
		hyper_shutdown2.store(true, Ordering::Release);

		// Wait until remaining tasks finish
		http_factory2.wait_idle().await;

		if let Some(https_factory) = https_factory2 {
			https_factory.wait_idle().await;
		}
	}
	.boxed();

	let mut progress_interval = tokio::time::interval(SHUTDOWN_PROGRESS_INTERVAL);
	progress_interval.tick().await;

	let shutdown_start = Instant::now();
	loop {
		tokio::select! {
			_ = &mut complete_fut => {
				tracing::info!("all guard tasks completed");
				break;
			}
			abort = term_signal.recv() => {
				if abort {
					tracing::warn!("aborting guard shutdown");
					break;
				}
			}
			_ = progress_interval.tick() => {
				let remaining_tasks = http_factory.remaining_tasks() +
					https_factory.as_ref().map(|f| f.remaining_tasks()).unwrap_or(0);
				let hyper_shutdown = hyper_shutdown.load(Ordering::Acquire);

				tracing::info!(%remaining_tasks, hyper_shutdown, "guard still shutting down");
			}
			_ = tokio::time::sleep(shutdown_duration.saturating_sub(shutdown_start.elapsed())) => {
				tracing::warn!("guard shutdown timed out before all tasks completed");
				break;
			}
		}
	}

	tracing::info!("guard shutdown complete");

	Ok(())
}
