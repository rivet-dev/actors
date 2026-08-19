use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

mod error;

pub use error::EngineProcessError;

const ENGINE_RUNTIME: &str = "engine";
const RIVETKIT_RUNTIME: &str = "rivetkit";
const ENGINE_VERSION_ENV: &str = "RIVETKIT_ENGINE_VERSION";
const RELEASES_ENDPOINT_ENV: &str = "RIVETKIT_ENGINE_RELEASES_ENDPOINT";
const RELEASES_ENDPOINT: &str = "https://releases.rivet.dev";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
struct EngineHealthResponse {
	status: Option<String>,
	runtime: Option<String>,
	version: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EngineResolverConfig {
	pub endpoint: String,
	pub explicit_binary_path: Option<PathBuf>,
	/// Directory for mutable state owned by this managed engine instance.
	///
	/// Embedders can set this to isolate multiple managed engines. When unset,
	/// the legacy shared engine paths are used.
	pub instance_path: Option<PathBuf>,
	pub bind_host: Option<String>,
	pub bind_port: Option<u16>,
	pub public_url: Option<String>,
	pub auto_download: bool,
	pub version: String,
	pub releases_endpoint: String,
}

impl EngineResolverConfig {
	pub fn from_parts(
		endpoint: &str,
		explicit_binary_path: Option<PathBuf>,
		bind_host: Option<String>,
		bind_port: Option<u16>,
		auto_download: bool,
	) -> Self {
		Self {
			endpoint: endpoint.to_owned(),
			explicit_binary_path,
			instance_path: None,
			bind_host,
			bind_port,
			public_url: None,
			auto_download,
			version: std::env::var(ENGINE_VERSION_ENV)
				.unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned()),
			releases_endpoint: std::env::var(RELEASES_ENDPOINT_ENV)
				.unwrap_or_else(|_| RELEASES_ENDPOINT.to_owned()),
		}
	}
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolvedEngine {
	Existing,
	Binary(PathBuf),
}

/// Effective runtime parameters of a spawned engine, stamped to disk so a later
/// process that reattaches can tell how the running engine is configured without
/// probing it over the network.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineRuntimeStamp {
	pub pid: u32,
	pub endpoint: String,
	pub bind_host: String,
	pub public_url: Option<String>,
	#[serde(default)]
	pub instance_path: Option<PathBuf>,
}

impl EngineRuntimeStamp {
	/// True if the engine bound a loopback address only, so peers on other hosts
	/// (for example a container reaching the host) cannot connect to it.
	pub fn binds_loopback_only(&self) -> bool {
		if self.bind_host == "localhost" {
			return true;
		}
		self.bind_host
			.parse::<std::net::IpAddr>()
			.map(|ip| ip.is_loopback())
			.unwrap_or(false)
	}
}

/// How `start_or_reuse` satisfied the request.
#[derive(Debug)]
pub enum EngineStartup {
	/// A new engine was spawned with the requested configuration.
	Spawned,
	/// An already-running engine was reused. `stamp` is its recorded runtime
	/// parameters, or `None` when no live stamp was found (an engine from an
	/// older CLI, started by hand, or a stale stamp whose process is gone).
	Reused { stamp: Option<EngineRuntimeStamp> },
}

/// Manages the rivet-engine subprocess.
///
/// The engine is intentionally orphaned: dropping the manager (or having the
/// host process exit) must NOT terminate the engine. This lets a dev-server
/// restart of the rivetkit host reattach to the same long-lived engine and
/// keep all in-flight actor state. To honor that contract:
///
/// - `Command::kill_on_drop` is left at its default (false) so the tokio
///   `Child` does not send SIGKILL on drop.
/// - Stdout and stderr are routed to log files at spawn time so the engine's
///   write fds remain valid after the host's pipes close.
/// - On startup we probe the configured endpoint and reuse a healthy engine
///   instead of spawning a duplicate.
///
/// When we spawn the engine, `watcher` holds a tokio task that owns the
/// `Child` and awaits `child.wait()` so we get a log line if the engine dies
/// while rivetkit is still running. On Drop we abort the watcher; aborting
/// drops the `Child` without killing it (kill_on_drop=false), so the engine
/// stays running and gets reparented to init when rivetkit exits.
///
/// `watcher` is `None` when we attached to an already-running engine.
#[derive(Debug)]
pub struct EngineProcessManager {
	watcher: Option<JoinHandle<()>>,
	startup: EngineStartup,
}

impl EngineProcessManager {
	pub async fn start_or_reuse(config: EngineResolverConfig) -> Result<Self> {
		let resolved = resolve_engine_binary(&config).await?;
		Self::start_resolved(resolved, &config).await
	}

	/// How the engine was obtained: freshly spawned or reused.
	pub fn startup(&self) -> &EngineStartup {
		&self.startup
	}

	async fn start_resolved(
		resolved: ResolvedEngine,
		config: &EngineResolverConfig,
	) -> Result<Self> {
		let endpoint = &config.endpoint;
		if matches!(resolved, ResolvedEngine::Existing) {
			tracing::info!(
				endpoint = %endpoint,
				"reusing already-running engine process"
			);
			return Ok(Self {
				watcher: None,
				startup: EngineStartup::Reused {
					stamp: read_engine_stamp(config),
				},
			});
		}

		let ResolvedEngine::Binary(binary_path) = resolved else {
			unreachable!("existing engine handled above");
		};
		if let Some(health) = probe_existing_engine(endpoint).await? {
			tracing::info!(
				endpoint = %endpoint,
				status = ?health.status,
				runtime = ?health.runtime,
				version = ?health.version,
				"reusing already-running engine process"
			);
			return Ok(Self {
				watcher: None,
				startup: EngineStartup::Reused {
					stamp: read_engine_stamp(config),
				},
			});
		}

		if !binary_path.exists() {
			return Err(EngineProcessError::BinaryNotFound {
				path: binary_path.display().to_string(),
			}
			.build());
		}

		let env = engine_env(config)?;
		let config_path = write_engine_config(config)?;
		let db_path = engine_db_path(config)?;
		let logs_dir = engine_logs_dir(config)?;
		ensure_dir(&db_path).context("create engine db directory")?;
		ensure_dir(&logs_dir).context("create engine logs directory")?;

		let timestamp = log_timestamp();
		let stdout_log_path = logs_dir.join(format!("engine-{timestamp}-stdout.log"));
		let stderr_log_path = logs_dir.join(format!("engine-{timestamp}-stderr.log"));
		let stdout_file = open_log_file(&stdout_log_path)
			.with_context(|| format!("open engine stdout log `{}`", stdout_log_path.display()))?;
		let stderr_file = open_log_file(&stderr_log_path)
			.with_context(|| format!("open engine stderr log `{}`", stderr_log_path.display()))?;

		let mut command = Command::new(&binary_path);
		command.arg("start");
		if let Some(config_path) = &config_path {
			command.arg("--config").arg(config_path);
		}
		for (key, value) in &env {
			command.env(key, value);
		}
		command
			.stdin(Stdio::null())
			.stdout(Stdio::from(stdout_file))
			.stderr(Stdio::from(stderr_file));

		// Put the engine in its own process group so terminal signals
		// (Ctrl+C, Ctrl+Z, SIGHUP on terminal close) targeting our foreground
		// process group do not reach the engine. Combined with no-kill-on-drop
		// and file-fd stdio, this gives the engine a real "intentional orphan"
		// lifetime that survives the host being killed for any reason.
		#[cfg(unix)]
		command.process_group(0);

		let mut child = command
			.spawn()
			.with_context(|| format!("spawn engine binary `{}`", binary_path.display()))?;
		let pid = child
			.id()
			.ok_or_else(|| EngineProcessError::MissingPid.build())?;

		tracing::info!(
			pid,
			path = %binary_path.display(),
			endpoint = %endpoint,
			db_path = %db_path.display(),
			"spawned engine process (intentionally orphaned, will outlive this process)"
		);
		tracing::info!(
			stdout_log = %stdout_log_path.display(),
			stderr_log = %stderr_log_path.display(),
			"engine stdout/stderr piped to log files"
		);

		let health_url = engine_health_url(endpoint);
		let health = match wait_for_engine_health(&health_url).await {
			Ok(health) => health,
			Err(error) => {
				let error = match child.try_wait() {
					Ok(Some(status)) => error.context(format!(
						"engine process exited before becoming healthy with status {status}"
					)),
					Ok(None) => error,
					Err(wait_error) => error.context(format!(
						"failed to inspect engine process status: {wait_error:#}"
					)),
				};
				if let Err(cleanup_error) = terminate_failed_spawn(&mut child).await {
					tracing::warn!(
						?cleanup_error,
						"failed to terminate engine process that never became healthy"
					);
				}
				return Err(error);
			}
		};

		tracing::info!(
			pid,
			status = ?health.status,
			runtime = ?health.runtime,
			version = ?health.version,
			"engine process is healthy"
		);

		// Record the effective runtime parameters so a later reattaching process
		// can tell how this engine is bound. Best-effort: a failed write only
		// costs that later process its reuse diagnostics.
		let stamp = EngineRuntimeStamp {
			pid,
			endpoint: endpoint.clone(),
			bind_host: resolve_bind_host(config)?,
			public_url: config.public_url.clone(),
			instance_path: config.instance_path.clone(),
		};
		if let Err(error) = write_engine_stamp(config, &stamp) {
			tracing::warn!(?error, "failed to write engine runtime stamp");
		}

		Ok(Self {
			watcher: Some(spawn_engine_watcher(child, pid)),
			startup: EngineStartup::Spawned,
		})
	}
}

/// Path to the rivet-engine database directory.
pub fn engine_db_path(config: &EngineResolverConfig) -> Result<PathBuf> {
	Ok(engine_instance_path(config)?.join("db"))
}

/// Default directory for mutable state owned by a managed engine instance.
pub fn default_engine_instance_path() -> Result<PathBuf> {
	Ok(storage_root()?.join("var").join("engine"))
}

fn engine_instance_path(config: &EngineResolverConfig) -> Result<PathBuf> {
	match &config.instance_path {
		Some(path) => Ok(path.clone()),
		None => default_engine_instance_path(),
	}
}

fn engine_stamp_path(config: &EngineResolverConfig) -> Result<PathBuf> {
	Ok(engine_instance_path(config)?.join("runtime.json"))
}

fn engine_logs_dir(config: &EngineResolverConfig) -> Result<PathBuf> {
	match &config.instance_path {
		Some(path) => Ok(path.join("logs")),
		None => Ok(storage_root()?
			.join("var")
			.join("logs")
			.join("rivet-engine")),
	}
}

/// The address the engine actually binds: the explicit `bind_host` override, or
/// the endpoint host when unset. Must match `engine_env`'s guard host.
fn resolve_bind_host(config: &EngineResolverConfig) -> Result<String> {
	if let Some(bind_host) = &config.bind_host {
		return Ok(bind_host.clone());
	}
	let endpoint = &config.endpoint;
	let endpoint_url =
		Url::parse(endpoint).with_context(|| format!("parse engine endpoint `{endpoint}`"))?;
	Ok(endpoint_url
		.host_str()
		.ok_or_else(|| invalid_endpoint(endpoint, "missing host"))?
		.to_owned())
}

fn write_engine_stamp(config: &EngineResolverConfig, stamp: &EngineRuntimeStamp) -> Result<()> {
	let path = engine_stamp_path(config)?;
	if let Some(dir) = path.parent() {
		ensure_dir(dir)?;
	}
	let contents = serde_json::to_vec_pretty(stamp).context("serialize engine stamp")?;
	std::fs::write(&path, contents)
		.with_context(|| format!("write engine stamp `{}`", path.display()))?;
	Ok(())
}

/// Reads the runtime stamp of the currently running engine, if its recorded
/// process is still alive. A stale stamp (process gone) reads as `None` so
/// callers treat the binding as unknown rather than trusting old data. On
/// platforms without a liveness check (see `pid_is_alive`) the stamp is always
/// treated as unknown for the same reason.
fn read_engine_stamp(config: &EngineResolverConfig) -> Option<EngineRuntimeStamp> {
	let path = engine_stamp_path(config).ok()?;
	let contents = std::fs::read(&path).ok()?;
	let stamp: EngineRuntimeStamp = serde_json::from_slice(&contents).ok()?;
	pid_is_alive(stamp.pid).then_some(stamp)
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
	// `kill(pid, 0)` sends no signal but performs the existence and permission
	// checks: success or EPERM means the process exists, ESRCH means it is gone.
	let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
	result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
	// No portable liveness check here, so fail safe: report the process as gone
	// so a stale stamp is never trusted. The binding check then falls back to
	// its "unknown, warn" branch instead of silently trusting old data.
	false
}

/// Computes the environment variables that configure a rivet-engine process
/// for the given endpoint.
///
/// Shared by the spawn path and by callers that exec the engine binary
/// directly (for example the CLI `engine` proxy) so both operate on the same
/// database, guard, api-peer, and metrics ports.
pub fn engine_env(config: &EngineResolverConfig) -> Result<Vec<(String, String)>> {
	let endpoint = &config.endpoint;
	let endpoint_url =
		Url::parse(endpoint).with_context(|| format!("parse engine endpoint `{endpoint}`"))?;
	let guard_host = resolve_bind_host(config)?;
	let guard_port = endpoint_url
		.port_or_known_default()
		.ok_or_else(|| invalid_endpoint(endpoint, "missing port"))?;
	let guard_port = config.bind_port.unwrap_or(guard_port);
	let api_peer_port = guard_port
		.checked_add(1)
		.ok_or_else(|| invalid_endpoint(endpoint, "port is too large"))?;
	let metrics_port = guard_port
		.checked_add(10)
		.ok_or_else(|| invalid_endpoint(endpoint, "port is too large"))?;

	let db_path = engine_db_path(config)?;

	Ok(vec![
		("RIVET__GUARD__HOST".to_owned(), guard_host.clone()),
		("RIVET__GUARD__PORT".to_owned(), guard_port.to_string()),
		("RIVET__API_PEER__HOST".to_owned(), guard_host.clone()),
		(
			"RIVET__API_PEER__PORT".to_owned(),
			api_peer_port.to_string(),
		),
		("RIVET__METRICS__HOST".to_owned(), guard_host),
		("RIVET__METRICS__PORT".to_owned(), metrics_port.to_string()),
		(
			"RIVET__FILE_SYSTEM__PATH".to_owned(),
			db_path.to_string_lossy().into_owned(),
		),
	])
}

fn write_engine_config(config: &EngineResolverConfig) -> Result<Option<PathBuf>> {
	let Some(public_url) = &config.public_url else {
		return Ok(None);
	};

	let public_url = Url::parse(public_url)
		.with_context(|| format!("parse engine public URL `{public_url}`"))?;
	let peer_url = peer_url_for_public_url(&public_url)?;
	let dir = engine_instance_path(config)?;
	ensure_dir(&dir)?;
	let path = dir.join("config.json");
	let config = serde_json::json!({
		"topology": {
			"datacenter_label": 1,
			"datacenters": {
				"default": {
					"datacenter_label": 1,
					"is_leader": true,
					"public_url": public_url.as_str(),
					"peer_url": peer_url,
				}
			}
		}
	});
	std::fs::write(&path, serde_json::to_string_pretty(&config)?)
		.with_context(|| format!("write engine config `{}`", path.display()))?;
	Ok(Some(path))
}

fn peer_url_for_public_url(public_url: &Url) -> Result<String> {
	let mut peer_url = public_url.clone();
	let port = public_url
		.port_or_known_default()
		.ok_or_else(|| invalid_endpoint(public_url.as_str(), "missing port"))?
		.checked_add(1)
		.ok_or_else(|| invalid_endpoint(public_url.as_str(), "port is too large"))?;
	if peer_url.set_port(Some(port)).is_err() {
		return Err(invalid_endpoint(
			public_url.as_str(),
			"could not derive peer URL port",
		));
	}
	peer_url.set_path("");
	peer_url.set_query(None);
	peer_url.set_fragment(None);
	Ok(peer_url.to_string().trim_end_matches('/').to_string())
}

pub async fn resolve_engine_binary(config: &EngineResolverConfig) -> Result<ResolvedEngine> {
	if let Some(path) = config.explicit_binary_path.as_ref() {
		return verify_binary_path(path);
	}

	if let Some(path) = std::env::var_os("RIVET_ENGINE_BINARY_PATH").map(PathBuf::from) {
		return verify_binary_path(&path);
	}

	if probe_existing_engine(&config.endpoint).await?.is_some() {
		return Ok(ResolvedEngine::Existing);
	}

	let local_roots = local_engine_search_roots();
	let cached = cached_engine_path(&config.version)?;
	resolve_engine_binary_after_probe(config, false, &local_roots, cached).await
}

/// Resolves the engine binary path without probing for an already-running
/// engine.
///
/// Used by callers that exec the binary directly (for example the CLI `engine`
/// proxy), where a running engine on the endpoint should not short-circuit
/// resolution to `Existing` and leave the caller without a path to run.
pub async fn resolve_engine_binary_path(config: &EngineResolverConfig) -> Result<PathBuf> {
	if let Some(path) = config.explicit_binary_path.as_ref() {
		verify_binary_path(path)?;
		return Ok(path.clone());
	}

	if let Some(path) = std::env::var_os("RIVET_ENGINE_BINARY_PATH").map(PathBuf::from) {
		verify_binary_path(&path)?;
		return Ok(path);
	}

	let local_roots = local_engine_search_roots();
	let cached = cached_engine_path(&config.version)?;
	match resolve_engine_binary_after_probe(config, false, &local_roots, cached).await? {
		ResolvedEngine::Binary(path) => Ok(path),
		ResolvedEngine::Existing => {
			unreachable!("no-probe resolution never returns Existing")
		}
	}
}

async fn resolve_engine_binary_after_probe(
	config: &EngineResolverConfig,
	existing_engine: bool,
	local_roots: &[PathBuf],
	cached: PathBuf,
) -> Result<ResolvedEngine> {
	if existing_engine {
		return Ok(ResolvedEngine::Existing);
	}

	if let Some(path) = find_local_engine_binary_in_roots(local_roots) {
		return Ok(ResolvedEngine::Binary(path));
	}

	if cached.exists() {
		return Ok(ResolvedEngine::Binary(cached));
	}

	if !config.auto_download {
		return Err(EngineProcessError::BinaryUnavailable {
			version: config.version.clone(),
		}
		.build());
	}

	download_engine_binary(config, &cached).await?;
	Ok(ResolvedEngine::Binary(cached))
}

fn verify_binary_path(path: &Path) -> Result<ResolvedEngine> {
	if !path.exists() {
		return Err(EngineProcessError::BinaryNotFound {
			path: path.display().to_string(),
		}
		.build());
	}
	Ok(ResolvedEngine::Binary(path.to_path_buf()))
}

fn local_engine_search_roots() -> Vec<PathBuf> {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.ancestors()
		.map(Path::to_path_buf)
		.collect()
}

fn find_local_engine_binary_in_roots(roots: &[PathBuf]) -> Option<PathBuf> {
	for root in roots {
		for profile in ["debug", "release"] {
			let candidate = root
				.join("target")
				.join(profile)
				.join(exe_name("rivet-engine"));
			if candidate.exists() {
				return Some(candidate);
			}
		}
	}
	None
}

fn cached_engine_path(version: &str) -> Result<PathBuf> {
	Ok(storage_root()?
		.join("engine")
		.join(version)
		.join(engine_artifact_name()))
}

async fn download_engine_binary(config: &EngineResolverConfig, destination: &Path) -> Result<()> {
	let artifact = engine_artifact_name();
	let base = config.releases_endpoint.trim_end_matches('/');
	let artifact_url = format!("{base}/rivet/{}/engine/{artifact}", config.version);
	let manifest_url = format!("{base}/rivet/{}/engine/SHA256SUMS", config.version);
	let client = Client::builder()
		.timeout(DOWNLOAD_TIMEOUT)
		.build()
		.context("build reqwest client for engine download")?;

	let manifest = fetch_text(&client, &manifest_url).await?;
	let expected = checksum_for_artifact(&manifest, &artifact).ok_or_else(|| {
		EngineProcessError::DownloadFailed {
			url: manifest_url.clone(),
			reason: format!("manifest does not contain `{artifact}`"),
		}
		.build()
	})?;

	let bytes = fetch_bytes(&client, &artifact_url).await?;
	let received = sha256_hex(&bytes);
	if !received.eq_ignore_ascii_case(&expected) {
		return Err(EngineProcessError::ChecksumMismatch {
			artifact,
			expected,
			received,
		}
		.build());
	}

	let parent = destination
		.parent()
		.context("engine cache destination has no parent")?;
	ensure_dir(parent)?;
	std::fs::write(destination, bytes)
		.with_context(|| format!("write engine binary `{}`", destination.display()))?;
	make_executable(destination)?;
	Ok(())
}

async fn fetch_text(client: &Client, url: &str) -> Result<String> {
	let response = client.get(url).send().await.map_err(|error| {
		EngineProcessError::DownloadFailed {
			url: url.to_owned(),
			reason: error.to_string(),
		}
		.build()
	})?;
	if !response.status().is_success() {
		let status = response.status();
		return Err(EngineProcessError::DownloadFailed {
			url: url.to_owned(),
			reason: format!("unexpected status {status}"),
		}
		.build());
	}
	response.text().await.map_err(|error| {
		EngineProcessError::DownloadFailed {
			url: url.to_owned(),
			reason: error.to_string(),
		}
		.build()
	})
}

async fn fetch_bytes(client: &Client, url: &str) -> Result<Vec<u8>> {
	let response = client.get(url).send().await.map_err(|error| {
		EngineProcessError::DownloadFailed {
			url: url.to_owned(),
			reason: error.to_string(),
		}
		.build()
	})?;
	if !response.status().is_success() {
		let status = response.status();
		return Err(EngineProcessError::DownloadFailed {
			url: url.to_owned(),
			reason: format!("unexpected status {status}"),
		}
		.build());
	}
	response
		.bytes()
		.await
		.map(|bytes| bytes.to_vec())
		.map_err(|error| {
			EngineProcessError::DownloadFailed {
				url: url.to_owned(),
				reason: error.to_string(),
			}
			.build()
		})
}

fn checksum_for_artifact(manifest: &str, artifact: &str) -> Option<String> {
	manifest.lines().find_map(|line| {
		let mut parts = line.split_whitespace();
		let checksum = parts.next()?;
		let name = parts.next()?.trim_start_matches('*');
		(checksum.len() == 64 && name == artifact).then(|| checksum.to_owned())
	})
}

fn sha256_hex(bytes: &[u8]) -> String {
	let digest = Sha256::digest(bytes);
	let mut out = String::with_capacity(digest.len() * 2);
	for byte in digest {
		use std::fmt::Write;
		let _ = write!(&mut out, "{byte:02x}");
	}
	out
}

fn engine_artifact_name() -> String {
	let arch = match std::env::consts::ARCH {
		"x86_64" => "x86_64",
		"aarch64" => "aarch64",
		other => other,
	};
	let target = match std::env::consts::OS {
		"linux" => format!("{arch}-unknown-linux-musl"),
		"macos" => format!("{arch}-apple-darwin"),
		"windows" => format!("{arch}-pc-windows-gnu.exe"),
		other => format!("{arch}-{other}"),
	};
	format!("rivet-engine-{target}")
}

fn exe_name(base: &str) -> String {
	if cfg!(windows) {
		format!("{base}.exe")
	} else {
		base.to_owned()
	}
}

fn make_executable(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let mut permissions = std::fs::metadata(path)
			.with_context(|| format!("read metadata for `{}`", path.display()))?
			.permissions();
		permissions.set_mode(0o755);
		std::fs::set_permissions(path, permissions)
			.with_context(|| format!("mark `{}` executable", path.display()))?;
	}
	#[cfg(not(unix))]
	{
		let _ = path;
	}
	Ok(())
}

impl Drop for EngineProcessManager {
	fn drop(&mut self) {
		if let Some(handle) = self.watcher.take() {
			// Aborting drops the `Child` owned by the task. With
			// `kill_on_drop=false`, dropping the `Child` does NOT signal the
			// engine, so the engine survives and gets reparented to init.
			// We give up our crash-detection log line here, but if we are
			// being dropped the rivetkit host is shutting down anyway.
			handle.abort();
			tracing::debug!(
				"aborted engine watcher; engine continues running (intentional orphan)"
			);
		}
	}
}

/// Spawns a background task that owns the `Child` and awaits `wait()` so we
/// log a clear message if the engine dies while rivetkit is still up. Taking
/// the `Child` into the task also reaps it via `waitpid` on exit, so a
/// crashed engine never lingers as a zombie in our process table.
fn spawn_engine_watcher(mut child: Child, pid: u32) -> JoinHandle<()> {
	tokio::spawn(async move {
		match child.wait().await {
			Ok(status) if status.success() => {
				tracing::warn!(
					pid,
					?status,
					"engine process exited cleanly while rivetkit was still running; \
					 rivetkit expected the engine to outlive it"
				);
			}
			Ok(status) => {
				tracing::error!(
					pid,
					?status,
					"engine process crashed while rivetkit was still running"
				);
			}
			Err(error) => {
				tracing::error!(
					pid,
					?error,
					"failed to wait on engine process; cannot detect crashes"
				);
			}
		}
	})
}

/// Probes the configured endpoint for an already-running, healthy engine.
///
/// Returns `Ok(Some(health))` if the endpoint is serving a `runtime: "engine"`
/// health response that we can reattach to. Returns `Ok(None)` if the port is
/// free. Returns `Err(...)` if the port is occupied by a non-engine process
/// (for example a stale rivetkit) which would conflict with a fresh spawn.
async fn probe_existing_engine(endpoint: &str) -> Result<Option<EngineHealthResponse>> {
	let health_url = engine_health_url(endpoint);
	let client = Client::builder()
		.build()
		.context("build reqwest client for engine probe")?;

	let response = match client
		.get(&health_url)
		.timeout(Duration::from_secs(1))
		.send()
		.await
	{
		Ok(response) => response,
		Err(_) => return Ok(None),
	};

	if !response.status().is_success() {
		return Ok(None);
	}

	let health = response
		.json::<EngineHealthResponse>()
		.await
		.context("decode existing engine health response")?;

	match health.runtime.as_deref() {
		Some(ENGINE_RUNTIME) => Ok(Some(health)),
		Some(RIVETKIT_RUNTIME) => Err(EngineProcessError::PortOccupied {
			endpoint: endpoint.to_owned(),
			runtime: RIVETKIT_RUNTIME.to_owned(),
		}
		.build()),
		Some(other) => Err(EngineProcessError::PortOccupied {
			endpoint: endpoint.to_owned(),
			runtime: other.to_owned(),
		}
		.build()),
		None => Err(EngineProcessError::PortOccupied {
			endpoint: endpoint.to_owned(),
			runtime: "unknown".to_owned(),
		}
		.build()),
	}
}

fn engine_health_url(endpoint: &str) -> String {
	format!("{}/health", endpoint.trim_end_matches('/'))
}

fn storage_root() -> Result<PathBuf> {
	if let Ok(path) = std::env::var("RIVETKIT_STORAGE_PATH") {
		return Ok(PathBuf::from(path).join(".rivetkit"));
	}
	let home = std::env::var("HOME")
		.map(PathBuf::from)
		.or_else(|_| std::env::current_dir())
		.context("locate home directory for engine storage path")?;
	Ok(home.join(".rivetkit"))
}

fn ensure_dir(path: &Path) -> Result<()> {
	std::fs::create_dir_all(path).with_context(|| format!("create directory `{}`", path.display()))
}

fn open_log_file(path: &Path) -> Result<std::fs::File> {
	std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(path)
		.with_context(|| format!("open log file `{}`", path.display()))
}

fn log_timestamp() -> String {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default();
	format!("{}", now.as_secs())
}

async fn wait_for_engine_health(health_url: &str) -> Result<EngineHealthResponse> {
	const HEALTH_MAX_WAIT: Duration = Duration::from_secs(10);
	const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
	const HEALTH_INITIAL_BACKOFF: Duration = Duration::from_millis(100);
	const HEALTH_MAX_BACKOFF: Duration = Duration::from_secs(1);

	let client = Client::builder()
		.build()
		.context("build reqwest client for engine health check")?;
	let deadline = Instant::now() + HEALTH_MAX_WAIT;
	let mut attempt = 0u32;
	let mut backoff = HEALTH_INITIAL_BACKOFF;

	loop {
		attempt += 1;

		let last_error = match client
			.get(health_url)
			.timeout(HEALTH_REQUEST_TIMEOUT)
			.send()
			.await
		{
			Ok(response) if response.status().is_success() => {
				let health = response
					.json::<EngineHealthResponse>()
					.await
					.context("decode engine health response")?;
				return Ok(health);
			}
			Ok(response) => format!("unexpected status {}", response.status()),
			Err(error) => error.to_string(),
		};

		if Instant::now() >= deadline {
			return Err(EngineProcessError::HealthCheckFailed {
				attempts: attempt,
				reason: last_error,
			}
			.build());
		}

		tokio::time::sleep(backoff).await;
		backoff = std::cmp::min(backoff * 2, HEALTH_MAX_BACKOFF);
	}
}

/// Cleanup path for a spawn that never reached `healthy`. We *do* kill here
/// because the half-started engine has no useful state to preserve and
/// leaving it running would conflict with a retry. This is the only place
/// allowed to terminate the engine.
async fn terminate_failed_spawn(child: &mut Child) -> Result<()> {
	const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

	if child
		.try_wait()
		.context("check engine process status")?
		.is_some()
	{
		return Ok(());
	}

	child
		.start_kill()
		.context("kill half-started engine process")?;
	match tokio::time::timeout(SHUTDOWN_TIMEOUT, child.wait()).await {
		Ok(result) => {
			let status = result.context("wait for half-started engine to exit")?;
			tracing::info!(?status, "half-started engine process exited");
			Ok(())
		}
		Err(_) => {
			tracing::warn!("half-started engine process did not exit within timeout");
			Ok(())
		}
	}
}

fn invalid_endpoint(endpoint: &str, reason: &str) -> anyhow::Error {
	EngineProcessError::InvalidEndpoint {
		endpoint: endpoint.to_owned(),
		reason: reason.to_owned(),
	}
	.build()
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	use tokio::net::TcpListener;

	use super::*;

	fn test_config(releases_endpoint: String, auto_download: bool) -> EngineResolverConfig {
		EngineResolverConfig {
			endpoint: "http://127.0.0.1:1".to_owned(),
			explicit_binary_path: None,
			instance_path: None,
			bind_host: None,
			bind_port: None,
			public_url: None,
			auto_download,
			version: "test-version".to_owned(),
			releases_endpoint,
		}
	}

	fn stamp_with_bind(bind_host: &str) -> EngineRuntimeStamp {
		EngineRuntimeStamp {
			pid: 1,
			endpoint: "http://127.0.0.1:6420".to_owned(),
			bind_host: bind_host.to_owned(),
			public_url: None,
			instance_path: None,
		}
	}

	#[test]
	fn caller_defined_instance_path_scopes_mutable_state() {
		let temp = tempfile::tempdir().expect("create temp dir");
		let instance_path = temp.path().join("engine-6421");
		let mut config = test_config(String::new(), false);
		config.instance_path = Some(instance_path.clone());

		assert_eq!(engine_db_path(&config).unwrap(), instance_path.join("db"));
		assert_eq!(
			engine_stamp_path(&config).unwrap(),
			instance_path.join("runtime.json")
		);
		assert_eq!(engine_logs_dir(&config).unwrap(), instance_path.join("logs"));
	}

	#[test]
	fn binds_loopback_only_detects_loopback_addresses() {
		assert!(stamp_with_bind("127.0.0.1").binds_loopback_only());
		assert!(stamp_with_bind("localhost").binds_loopback_only());
		assert!(stamp_with_bind("::1").binds_loopback_only());
	}

	#[test]
	fn binds_loopback_only_false_for_reachable_addresses() {
		assert!(!stamp_with_bind("0.0.0.0").binds_loopback_only());
		assert!(!stamp_with_bind("::").binds_loopback_only());
		assert!(!stamp_with_bind("192.168.1.5").binds_loopback_only());
	}

	#[test]
	fn resolve_bind_host_prefers_override_then_endpoint() {
		let mut config = test_config(String::new(), false);
		config.endpoint = "http://127.0.0.1:6420".to_owned();
		assert_eq!(resolve_bind_host(&config).unwrap(), "127.0.0.1");
		config.bind_host = Some("0.0.0.0".to_owned());
		assert_eq!(resolve_bind_host(&config).unwrap(), "0.0.0.0");
	}

	#[cfg(unix)]
	#[test]
	fn pid_is_alive_true_for_self_false_for_missing() {
		assert!(pid_is_alive(std::process::id()));
		// PID 0 targets the process group on unix, so use a very high, unused PID.
		assert!(!pid_is_alive(0x7FFF_FFFF));
	}

	#[tokio::test]
	async fn resolver_prefers_existing_engine_before_filesystem_paths() {
		let temp = tempfile::tempdir().expect("create temp dir");
		let local = temp
			.path()
			.join("target")
			.join("debug")
			.join(exe_name("rivet-engine"));
		std::fs::create_dir_all(local.parent().expect("local parent")).expect("create local dir");
		std::fs::write(&local, b"local").expect("write local binary");
		let cached = temp.path().join("cache").join(exe_name("rivet-engine"));

		let resolved = resolve_engine_binary_after_probe(
			&test_config("http://127.0.0.1:1".to_owned(), false),
			true,
			&[temp.path().to_path_buf()],
			cached,
		)
		.await
		.expect("resolve engine");

		assert_eq!(resolved, ResolvedEngine::Existing);
	}

	#[tokio::test]
	async fn resolver_prefers_local_binary_before_cached_binary() {
		let temp = tempfile::tempdir().expect("create temp dir");
		let local = temp
			.path()
			.join("target")
			.join("debug")
			.join(exe_name("rivet-engine"));
		std::fs::create_dir_all(local.parent().expect("local parent")).expect("create local dir");
		std::fs::write(&local, b"local").expect("write local binary");
		let cached = temp.path().join("cache").join(exe_name("rivet-engine"));
		std::fs::create_dir_all(cached.parent().expect("cached parent")).expect("create cache dir");
		std::fs::write(&cached, b"cached").expect("write cached binary");

		let resolved = resolve_engine_binary_after_probe(
			&test_config("http://127.0.0.1:1".to_owned(), false),
			false,
			&[temp.path().to_path_buf()],
			cached,
		)
		.await
		.expect("resolve engine");

		assert_eq!(resolved, ResolvedEngine::Binary(local));
	}

	#[tokio::test]
	async fn resolver_reuses_cached_binary_without_download() {
		let temp = tempfile::tempdir().expect("create temp dir");
		let cached = temp.path().join("cache").join(exe_name("rivet-engine"));
		std::fs::create_dir_all(cached.parent().expect("cached parent")).expect("create cache dir");
		std::fs::write(&cached, b"cached").expect("write cached binary");

		let resolved = resolve_engine_binary_after_probe(
			&test_config("http://127.0.0.1:1".to_owned(), false),
			false,
			&[],
			cached.clone(),
		)
		.await
		.expect("resolve engine");

		assert_eq!(resolved, ResolvedEngine::Binary(cached));
	}

	#[tokio::test]
	async fn resolver_reports_actionable_error_without_binary_or_download() {
		let temp = tempfile::tempdir().expect("create temp dir");
		let cached = temp.path().join("cache").join(exe_name("rivet-engine"));

		let error = resolve_engine_binary_after_probe(
			&test_config("http://127.0.0.1:1".to_owned(), false),
			false,
			&[],
			cached,
		)
		.await
		.expect_err("missing binary should fail");
		let message = error.to_string();

		assert!(message.contains("No usable engine binary was found"));
		assert!(message.contains("Build `rivet-engine`"));
		assert!(message.contains("RIVET_ENGINE_BINARY_PATH"));
	}

	#[tokio::test]
	async fn resolver_download_checks_manifest_checksum() {
		let temp = tempfile::tempdir().expect("create temp dir");
		let cached = temp.path().join("cache").join(exe_name("rivet-engine"));
		let artifact = engine_artifact_name();
		let expected = sha256_hex(b"different bytes");
		let manifest = format!("{expected}  {artifact}\n");
		let releases_endpoint = spawn_download_server(HashMap::from([
			(
				format!("/rivet/test-version/engine/SHA256SUMS"),
				manifest.into_bytes(),
			),
			(
				format!("/rivet/test-version/engine/{artifact}"),
				b"actual bytes".to_vec(),
			),
		]))
		.await;

		let error = resolve_engine_binary_after_probe(
			&test_config(releases_endpoint, true),
			false,
			&[],
			cached,
		)
		.await
		.expect_err("checksum mismatch should fail");

		assert!(
			error
				.to_string()
				.contains("Engine binary checksum mismatch")
		);
	}

	async fn spawn_download_server(routes: HashMap<String, Vec<u8>>) -> String {
		let listener = TcpListener::bind("127.0.0.1:0")
			.await
			.expect("bind download server");
		let addr = listener.local_addr().expect("download server address");
		tokio::spawn(async move {
			for _ in 0..routes.len() {
				let (mut socket, _) = listener.accept().await.expect("accept download request");
				let mut buffer = [0_u8; 2048];
				let n = socket
					.read(&mut buffer)
					.await
					.expect("read download request");
				let request = String::from_utf8_lossy(&buffer[..n]);
				let path = request
					.split_whitespace()
					.nth(1)
					.expect("request path")
					.to_owned();
				let body = routes.get(&path).expect("route body");
				let header = format!(
					"HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
					body.len()
				);
				socket
					.write_all(header.as_bytes())
					.await
					.expect("write response header");
				socket.write_all(body).await.expect("write response body");
			}
		});
		format!("http://{addr}")
	}
}
