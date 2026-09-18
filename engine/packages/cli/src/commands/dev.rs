use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use reqwest::Client;
use rivetkit_engine_process::{EngineProcessManager, EngineStartup};
use serde_json::json;
use tempfile::NamedTempFile;
use tokio::process::{Child, Command};

use crate::{
	DEFAULT_ENGINE_ENDPOINT, LOCAL_NAMESPACE, POOL_NAME, SUPABASE_ENGINE_ENDPOINT,
	SUPABASE_FN_DEFAULT, engine_runner::engine_config, util::encode,
};

const HANDLER_METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const HANDLER_METADATA_RETRY: Duration = Duration::from_millis(200);
const HANDLER_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Parser)]
pub struct Opts {
	/// Serverless platform preset. Omit to run a custom dev server you point at
	/// with --port or --url.
	#[arg(long, value_enum)]
	provider: Option<Provider>,
	/// Handler port. Required in the default (no provider) mode unless --url is
	/// set. Overrides the provider's default port.
	#[arg(long)]
	port: Option<u16>,
	/// Supabase function name when --provider=supabase.
	#[arg(long, default_value = SUPABASE_FN_DEFAULT)]
	fn_name: String,
	/// Explicit full handler URL. Overrides port and path construction.
	#[arg(long)]
	url: Option<String>,
	/// Path to a rivet-engine binary. Defaults to RIVET_ENGINE_BINARY_PATH, a
	/// binary next to this CLI, a local build, or an auto-downloaded release.
	#[arg(long)]
	engine_binary: Option<PathBuf>,
	/// Dev server command to spawn. Everything after `--`.
	#[arg(trailing_var_arg = true, allow_hyphen_values = true)]
	command: Vec<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum Provider {
	/// Generic serverless handler. The CLI assigns a free port and passes it as
	/// the PORT environment variable.
	Serverless,
	Cloudflare,
	Supabase,
	/// Run only the engine, do not spawn a handler.
	None,
}

impl Opts {
	pub async fn execute(self) -> Result<()> {
		let mut config = engine_config(self.engine_binary.clone());
		config.public_url = resolve_engine_public_url(&self)?;
		if self.provider == Some(Provider::Supabase) {
			config.bind_host = Some("0.0.0.0".to_string());
		}

		// Engine-only mode: start (or reuse) the engine and wait.
		if matches!(self.provider, Some(Provider::None)) {
			let _engine = EngineProcessManager::start_or_reuse(config).await?;
			tracing::info!(
				engine = DEFAULT_ENGINE_ENDPOINT,
				"engine ready (no handler); press Ctrl-C to stop"
			);
			tokio::signal::ctrl_c().await.context("listen for ctrl-c")?;
			return Ok(());
		}

		let plan = HandlerPlan::resolve(&self, config.public_url.as_deref())?;

		// Captured before `config` is moved into the engine manager below, so a
		// reused engine's advertised endpoint can be compared against it.
		let needed_public_url = config.public_url.clone();

		// Start (or reuse) the engine. The engine is intentionally orphaned, so
		// it survives this process and a later `rivet dev` reattaches to it.
		let engine = EngineProcessManager::start_or_reuse(config).await?;

		if self.provider == Some(Provider::Supabase) {
			check_supabase_engine_reuse(engine.startup(), needed_public_url.as_deref())?;
		}

		let mut child = plan.spawn()?;

		tokio::select! {
			result = wait_for_handler_metadata(&plan.handler_url) => {
				if let Err(err) = result {
					let _ = child.kill().await;
					return Err(err);
				}
			}
			status = child.wait() => {
				let status = status.context("wait for dev server")?;
				bail!("dev server exited before the Rivet handler became ready: {status}");
			}
		}

		if let Err(err) =
			register_runner_config(DEFAULT_ENGINE_ENDPOINT, POOL_NAME, &plan.handler_url).await
		{
			let _ = child.kill().await;
			return Err(err);
		}

		tracing::info!(
			engine = DEFAULT_ENGINE_ENDPOINT,
			handler = %plan.handler_url,
			"rivet dev ready; press Ctrl-C to stop"
		);

		tokio::select! {
			status = child.wait() => {
				let status = status.context("wait for dev server")?;
				if !status.success() {
					bail!("dev server exited with {status}");
				}
			}
			_ = tokio::signal::ctrl_c() => {
				tracing::info!(
					"stopping dev server (engine keeps running; use `rivet engine` to manage it)"
				);
				let _ = child.kill().await;
			}
		}

		Ok(())
	}
}

/// Resolved spawn plan for the dev server: where it listens, the command to
/// run, and any environment the CLI injects.
#[derive(Debug)]
struct HandlerPlan {
	handler_url: String,
	program: String,
	args: Vec<String>,
	env: Vec<(String, String)>,
	/// Kept alive so the generated Supabase env file outlives the spawned
	/// dev server.
	_env_file: Option<NamedTempFile>,
}

impl HandlerPlan {
	fn resolve(opts: &Opts, engine_endpoint: Option<&str>) -> Result<Self> {
		let provider = opts.provider;
		let mut env_file = None;
		let port = resolve_port(provider, opts.port, opts.url.is_some())?;
		let handler_url = match &opts.url {
			Some(url) => url.clone(),
			None => build_handler_url(provider, &opts.fn_name, port),
		};

		let (program, args, env) = match provider {
			Some(Provider::Cloudflare) => {
				let mut args = vec![
					"wrangler".to_string(),
					"dev".to_string(),
					"--port".to_string(),
					port.to_string(),
					// Inject the local engine endpoint as a Worker variable so the
					// handler connects back without any wrangler.toml config. With
					// `nodejs_compat` this also lands in `process.env`. wrangler
					// splits `--var` on the first colon, so the URL stays intact.
					"--var".to_string(),
					format!("RIVET_ENDPOINT:{DEFAULT_ENGINE_ENDPOINT}"),
				];
				args.extend(opts.command.iter().cloned());
				("npx".to_string(), args, Vec::new())
			}
			Some(Provider::Supabase) => {
				let mut args = vec![
					"supabase".to_string(),
					"functions".to_string(),
					"serve".to_string(),
					opts.fn_name.clone(),
					"--no-verify-jwt".to_string(),
				];
				// `supabase functions serve` does not forward this process's
				// environment to the edge runtime worker, so the endpoint has to
				// reach the function through an env file. A user-supplied
				// `--env-file` owns the value instead.
				if supabase_env_file(&opts.command).is_none() {
					let endpoint = engine_endpoint.unwrap_or(SUPABASE_ENGINE_ENDPOINT);
					let file = write_supabase_env_file(endpoint)?;
					args.push("--env-file".to_string());
					args.push(file.path().display().to_string());
					env_file = Some(file);
				}
				args.extend(opts.command.iter().cloned());
				("npx".to_string(), args, Vec::new())
			}
			Some(Provider::Serverless) => {
				let (program, args) = split_command(&opts.command)?;
				// Serverless handlers learn their port from the PORT env var.
				(program, args, vec![("PORT".to_string(), port.to_string())])
			}
			// Default (no provider): spawn the user's command verbatim.
			None => {
				let (program, args) = split_command(&opts.command)?;
				(program, args, Vec::new())
			}
			Some(Provider::None) => unreachable!("engine-only mode handled before resolve"),
		};

		Ok(Self {
			handler_url,
			program,
			args,
			env,
			_env_file: env_file,
		})
	}

	fn spawn(&self) -> Result<Child> {
		let mut command = Command::new(&self.program);
		command.args(&self.args);
		for (key, value) in &self.env {
			command.env(key, value);
		}
		command
			.spawn()
			.with_context(|| format!("spawn dev server `{}`", self.program))
	}
}

/// Resolves the handler port for the given provider. Returns an error in the
/// default mode when neither a port nor an explicit URL is provided.
fn resolve_port(provider: Option<Provider>, port: Option<u16>, has_url: bool) -> Result<u16> {
	match provider {
		Some(Provider::Cloudflare) => Ok(port.unwrap_or(8787)),
		Some(Provider::Supabase) => Ok(port.unwrap_or(54321)),
		Some(Provider::Serverless) => match port {
			Some(port) => Ok(port),
			None => pick_free_port(),
		},
		// Default mode: the port is not managed by the CLI, so it must be
		// provided so the runner can be registered. `0` is a sentinel that
		// callers only reach when --url is set (and the port is unused).
		None if has_url => Ok(port.unwrap_or(0)),
		None => port.context("provide --port (or --url) for the default dev server mode"),
		Some(Provider::None) => unreachable!("engine-only mode handled before resolve"),
	}
}

fn build_handler_url(provider: Option<Provider>, fn_name: &str, port: u16) -> String {
	match provider {
		Some(Provider::Supabase) => {
			format!("http://127.0.0.1:{port}/functions/v1/{fn_name}/api/rivet")
		}
		_ => format!("http://127.0.0.1:{port}/api/rivet"),
	}
}

fn split_command(command: &[String]) -> Result<(String, Vec<String>)> {
	let Some((program, args)) = command.split_first() else {
		bail!(
			"provide a dev server command after `--` (for example `rivet dev -- npm run dev`), \
			 or use `--provider none` to run only the engine"
		);
	};
	Ok((program.clone(), args.to_vec()))
}

fn resolve_engine_public_url(opts: &Opts) -> Result<Option<String>> {
	if opts.provider != Some(Provider::Supabase) {
		return Ok(None);
	}

	if let Some(endpoint) = read_env_value("RIVET_ENDPOINT") {
		return Ok(Some(endpoint));
	}

	if let Some(env_file) = supabase_env_file(&opts.command) {
		if let Some(endpoint) = read_dotenv_value(&env_file, "RIVET_ENDPOINT") {
			return Ok(Some(endpoint));
		}
	}

	if let Some(endpoint) = read_dotenv_value(Path::new(".env.local"), "RIVET_ENDPOINT") {
		return Ok(Some(endpoint));
	}

	Ok(Some(SUPABASE_ENGINE_ENDPOINT.to_string()))
}

fn write_supabase_env_file(endpoint: &str) -> Result<NamedTempFile> {
	let mut file = NamedTempFile::new().context("create supabase env file")?;
	writeln!(file, "RIVET_ENDPOINT={endpoint}").context("write supabase env file")?;
	file.flush().context("flush supabase env file")?;
	Ok(file)
}

fn supabase_env_file(command: &[String]) -> Option<PathBuf> {
	// The Supabase CLI is pflag-based, so it accepts both `--env-file value` and
	// `--env-file=value`. Detect both so a user override is never missed and
	// silently overridden by the auto-injected temp file.
	let mut iter = command.iter();
	while let Some(arg) = iter.next() {
		if let Some(value) = arg.strip_prefix("--env-file=") {
			return Some(PathBuf::from(value));
		}
		if arg == "--env-file" {
			return iter.next().map(PathBuf::from);
		}
	}
	None
}

fn read_env_value(key: &str) -> Option<String> {
	std::env::var(key)
		.ok()
		.map(|value| value.trim().to_string())
		.filter(|value| !value.is_empty())
}

fn read_dotenv_value(path: &Path, key: &str) -> Option<String> {
	let contents = std::fs::read_to_string(path).ok()?;
	contents.lines().find_map(|line| {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') {
			return None;
		}
		let (name, value) = line.split_once('=')?;
		if name.trim() != key {
			return None;
		}
		Some(strip_env_quotes(value.trim()).to_string())
	})
}

fn strip_env_quotes(value: &str) -> &str {
	if value.len() >= 2
		&& ((value.starts_with('"') && value.ends_with('"'))
			|| (value.starts_with('\'') && value.ends_with('\'')))
	{
		&value[1..value.len() - 1]
	} else {
		value
	}
}

/// Allocates a free TCP port for the serverless handler. There is a small
/// window between picking the port and the handler binding it, which is
/// acceptable for local development.
fn pick_free_port() -> Result<u16> {
	let listener = std::net::TcpListener::bind("127.0.0.1:0")
		.context("allocate a free port for the serverless handler")?;
	Ok(listener.local_addr().context("read allocated port")?.port())
}

async fn register_runner_config(endpoint: &str, runner: &str, handler_url: &str) -> Result<()> {
	let url = format!(
		"{}/runner-configs/{}?namespace={}",
		endpoint.trim_end_matches('/'),
		encode(runner),
		LOCAL_NAMESPACE
	);
	let body = json!({
		"datacenters": {
			"default": {
				"serverless": {
					"url": handler_url,
					"headers": {},
					"request_lifespan": 3600,
					"slots_per_runner": 1,
					"min_runners": 0,
					"max_runners": 100000,
					"runners_margin": 0,
					"metadata_poll_interval": 1000
				}
			}
		}
	});
	let response = Client::new()
		.put(url)
		.header("Content-Type", "application/json")
		.bearer_auth("dev")
		.json(&body)
		.send()
		.await
		.context("register local runner config")?;
	if !response.status().is_success() {
		let status = response.status();
		let text = response.text().await.unwrap_or_default();
		bail!("runner config update failed: {status}: {text}");
	}
	Ok(())
}

/// Validates that an engine `rivet dev --provider supabase` is about to reuse is
/// actually usable by the Supabase edge runtime.
///
/// The edge runtime runs in a container and reaches the engine through a
/// non-loopback host address, so a loopback-only engine is unreachable no matter
/// what `RIVET_ENDPOINT` says, and an engine advertising a different public URL
/// gets rejected with an endpoint mismatch. A freshly spawned engine is
/// configured correctly by construction, so only the reused case is uncertain,
/// and it is decided from the engine's runtime stamp rather than a network probe.
///
/// A loopback binding is a hard failure (deterministic and unrecoverable). A
/// public-URL difference only warns: matching mirrors the engine's own endpoint
/// normalization, so we avoid blocking a setup the engine would actually accept.
fn check_supabase_engine_reuse(
	startup: &EngineStartup,
	needed_public_url: Option<&str>,
) -> Result<()> {
	let stamp = match startup {
		// Just spawned with the requested configuration; nothing to verify.
		EngineStartup::Spawned => return Ok(()),
		EngineStartup::Reused { stamp } => stamp,
	};

	let Some(stamp) = stamp else {
		// No usable stamp: an engine from an older CLI or one started by hand. We
		// cannot tell how it is configured, so warn rather than block a setup
		// that may be fine.
		tracing::warn!(
			"reusing an already-running engine whose configuration is unknown; if the Supabase \
			 edge runtime cannot reach it, stop the engine and re-run `rivet dev`"
		);
		return Ok(());
	};

	if stamp.binds_loopback_only() {
		bail!(
			"a Rivet engine is already running but is bound to loopback only ({}), so the Supabase \
			 edge runtime cannot reach it from its container. This usually means an engine from a \
			 previous `rivet dev` (a different provider or an older CLI) is still running. Stop it \
			 and re-run `rivet dev` so the engine rebinds for Supabase.",
			stamp.bind_host
		);
	}

	if let Some(needed) = needed_public_url {
		let advertised = stamp.public_url.as_deref();
		if !advertised.is_some_and(|url| public_urls_match(url, needed)) {
			tracing::warn!(
				advertised = advertised.unwrap_or("none"),
				needed,
				"reusing an already-running engine that advertises a different endpoint than this \
				 project needs; the Supabase edge runtime may be rejected with an endpoint \
				 mismatch. If actors fail to start, stop the engine and re-run `rivet dev`"
			);
		}
	}

	Ok(())
}

/// Loose endpoint equality for the reuse warning: trailing-slash and
/// case-insensitive. Deliberately lenient so an equivalent endpoint spelled
/// differently does not warn; the engine performs the authoritative match.
fn public_urls_match(a: &str, b: &str) -> bool {
	a.trim_end_matches('/')
		.eq_ignore_ascii_case(b.trim_end_matches('/'))
}

async fn wait_for_handler_metadata(handler_url: &str) -> Result<()> {
	let metadata_url = format!("{}/metadata", handler_url.trim_end_matches('/'));
	let client = Client::new();
	let deadline = Instant::now() + HANDLER_METADATA_TIMEOUT;
	let mut last_error: Option<String> = None;

	loop {
		if Instant::now() >= deadline {
			bail!(
				"Rivet handler metadata did not become ready at {metadata_url} within {}s (last error: {})",
				HANDLER_METADATA_TIMEOUT.as_secs(),
				last_error.as_deref().unwrap_or("no request attempted")
			);
		}

		match client
			.get(&metadata_url)
			.timeout(HANDLER_METADATA_REQUEST_TIMEOUT)
			.send()
			.await
		{
			Ok(response) if response.status().is_success() => return Ok(()),
			Ok(response) => {
				let status = response.status();
				let body = response.text().await.unwrap_or_default();
				last_error = Some(format!("HTTP {status}: {body}"));
			}
			Err(err) => {
				last_error = Some(err.to_string());
			}
		}

		tokio::time::sleep(HANDLER_METADATA_RETRY).await;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rivetkit_engine_process::EngineRuntimeStamp;

	fn stamp(bind_host: &str, public_url: Option<&str>) -> EngineRuntimeStamp {
		EngineRuntimeStamp {
			pid: std::process::id(),
			endpoint: DEFAULT_ENGINE_ENDPOINT.to_string(),
			bind_host: bind_host.to_string(),
			public_url: public_url.map(str::to_string),
			instance_path: None,
		}
	}

	const SUPABASE_URL: &str = "http://host.docker.internal:6420";

	#[test]
	fn supabase_reuse_ok_when_freshly_spawned() {
		assert!(check_supabase_engine_reuse(&EngineStartup::Spawned, Some(SUPABASE_URL)).is_ok());
	}

	#[test]
	fn supabase_reuse_ok_when_engine_binds_all_interfaces_and_matches_url() {
		let startup = EngineStartup::Reused {
			stamp: Some(stamp("0.0.0.0", Some(SUPABASE_URL))),
		};
		assert!(check_supabase_engine_reuse(&startup, Some(SUPABASE_URL)).is_ok());
	}

	#[test]
	fn supabase_reuse_fails_when_engine_is_loopback() {
		let startup = EngineStartup::Reused {
			stamp: Some(stamp("127.0.0.1", Some(SUPABASE_URL))),
		};
		let error = check_supabase_engine_reuse(&startup, Some(SUPABASE_URL))
			.unwrap_err()
			.to_string();
		assert!(error.contains("loopback"));
		assert!(error.contains("127.0.0.1"));
	}

	#[test]
	fn supabase_reuse_ok_when_binding_unknown() {
		let startup = EngineStartup::Reused { stamp: None };
		assert!(check_supabase_engine_reuse(&startup, Some(SUPABASE_URL)).is_ok());
	}

	#[test]
	fn supabase_reuse_ok_but_warns_on_public_url_mismatch() {
		// A reachable engine advertising a different endpoint only warns, so the
		// call still succeeds.
		let startup = EngineStartup::Reused {
			stamp: Some(stamp("0.0.0.0", Some("http://127.0.0.1:6420"))),
		};
		assert!(check_supabase_engine_reuse(&startup, Some(SUPABASE_URL)).is_ok());
	}

	#[test]
	fn public_urls_match_is_lenient_on_trailing_slash_and_case() {
		assert!(public_urls_match(
			"http://host.docker.internal:6420",
			"http://host.docker.internal:6420/"
		));
		assert!(public_urls_match("http://HOST:6420", "http://host:6420"));
		assert!(!public_urls_match(
			"http://127.0.0.1:6420",
			"http://host.docker.internal:6420"
		));
	}

	fn opts(provider: Option<Provider>) -> Opts {
		Opts {
			provider,
			port: None,
			fn_name: SUPABASE_FN_DEFAULT.to_string(),
			url: None,
			engine_binary: None,
			command: Vec::new(),
		}
	}

	#[test]
	fn cloudflare_provider_uses_default_port_and_wrangler_command() {
		let plan = HandlerPlan::resolve(&opts(Some(Provider::Cloudflare)), None).unwrap();

		assert_eq!(plan.handler_url, "http://127.0.0.1:8787/api/rivet");
		assert_eq!(plan.program, "npx");
		assert_eq!(
			plan.args,
			[
				"wrangler",
				"dev",
				"--port",
				"8787",
				"--var",
				"RIVET_ENDPOINT:http://127.0.0.1:6420"
			]
		);
		assert!(plan.env.is_empty());
	}

	#[test]
	fn cloudflare_provider_allows_custom_port_and_appended_args() {
		let mut opts = opts(Some(Provider::Cloudflare));
		opts.port = Some(8788);
		opts.command = vec!["--local-protocol".into(), "http".into()];

		let plan = HandlerPlan::resolve(&opts, None).unwrap();

		assert_eq!(plan.handler_url, "http://127.0.0.1:8788/api/rivet");
		assert_eq!(
			plan.args,
			[
				"wrangler",
				"dev",
				"--port",
				"8788",
				"--var",
				"RIVET_ENDPOINT:http://127.0.0.1:6420",
				"--local-protocol",
				"http"
			]
		);
	}

	#[test]
	fn supabase_provider_uses_default_port_function_and_no_verify_jwt() {
		let plan = HandlerPlan::resolve(&opts(Some(Provider::Supabase)), None).unwrap();

		assert_eq!(
			plan.handler_url,
			"http://127.0.0.1:54321/functions/v1/rivet/api/rivet"
		);
		assert_eq!(plan.program, "npx");
		assert_eq!(
			plan.args[..5],
			["supabase", "functions", "serve", "rivet", "--no-verify-jwt"]
		);
		assert_eq!(plan.args[5], "--env-file");
		assert!(plan.env.is_empty());

		let written = std::fs::read_to_string(&plan.args[6]).unwrap();
		assert_eq!(
			written.trim(),
			format!("RIVET_ENDPOINT={SUPABASE_ENGINE_ENDPOINT}")
		);
	}

	#[test]
	fn supabase_env_file_uses_resolved_engine_endpoint() {
		let plan = HandlerPlan::resolve(
			&opts(Some(Provider::Supabase)),
			Some("http://192.168.1.5:6420"),
		)
		.unwrap();

		let written = std::fs::read_to_string(&plan.args[6]).unwrap();
		assert_eq!(written.trim(), "RIVET_ENDPOINT=http://192.168.1.5:6420");
	}

	#[test]
	fn supabase_public_engine_url_defaults_to_docker_host() {
		let public_url = resolve_engine_public_url(&opts(Some(Provider::Supabase))).unwrap();

		assert_eq!(public_url, Some(SUPABASE_ENGINE_ENDPOINT.to_string()));
	}

	#[test]
	fn supabase_provider_allows_custom_function_port_and_appended_args() {
		let mut opts = opts(Some(Provider::Supabase));
		opts.port = Some(4000);
		opts.fn_name = "actors".into();
		opts.command = vec!["--env-file".into(), ".env.local".into()];

		let plan = HandlerPlan::resolve(&opts, None).unwrap();

		assert_eq!(
			plan.handler_url,
			"http://127.0.0.1:4000/functions/v1/actors/api/rivet"
		);
		assert_eq!(
			plan.args,
			[
				"supabase",
				"functions",
				"serve",
				"actors",
				"--no-verify-jwt",
				"--env-file",
				".env.local"
			]
		);
	}

	#[test]
	fn serverless_provider_injects_port_env_for_command() {
		let mut opts = opts(Some(Provider::Serverless));
		opts.port = Some(3001);
		opts.command = vec!["node".into(), "handler.js".into()];

		let plan = HandlerPlan::resolve(&opts, None).unwrap();

		assert_eq!(plan.handler_url, "http://127.0.0.1:3001/api/rivet");
		assert_eq!(plan.program, "node");
		assert_eq!(plan.args, ["handler.js"]);
		assert_eq!(plan.env, [("PORT".to_string(), "3001".to_string())]);
	}

	#[test]
	fn default_mode_requires_port_or_url() {
		let mut opts = opts(None);
		opts.command = vec!["npm".into(), "run".into(), "dev".into()];

		let error = HandlerPlan::resolve(&opts, None).unwrap_err().to_string();

		assert!(error.contains("provide --port"));
	}

	#[test]
	fn explicit_url_overrides_handler_url() {
		let mut opts = opts(None);
		opts.url = Some("http://127.0.0.1:9000/custom".into());
		opts.command = vec!["npm".into(), "run".into(), "dev".into()];

		let plan = HandlerPlan::resolve(&opts, None).unwrap();

		assert_eq!(plan.handler_url, "http://127.0.0.1:9000/custom");
		assert_eq!(plan.program, "npm");
		assert_eq!(plan.args, ["run", "dev"]);
	}

	#[test]
	fn supabase_public_engine_url_reads_passed_env_file() {
		let temp = tempfile::tempdir().unwrap();
		let env_path = temp.path().join(".env.local");
		std::fs::write(
			&env_path,
			"RIVET_ENDPOINT=\"http://host.docker.internal:6420\"\n",
		)
		.unwrap();
		let mut opts = opts(Some(Provider::Supabase));
		opts.command = vec!["--env-file".into(), env_path.to_string_lossy().into_owned()];

		let public_url = resolve_engine_public_url(&opts).unwrap();

		assert_eq!(
			public_url,
			Some("http://host.docker.internal:6420".to_string())
		);
	}

	#[test]
	fn supabase_env_file_detects_equals_form() {
		let mut opts = opts(Some(Provider::Supabase));
		opts.command = vec!["--env-file=custom.env".into()];

		let plan = HandlerPlan::resolve(&opts, None).unwrap();

		// A user-supplied `--env-file=value` must suppress the auto-injected temp
		// file rather than being appended after and overriding it.
		assert_eq!(
			plan.args,
			[
				"supabase",
				"functions",
				"serve",
				"rivet",
				"--no-verify-jwt",
				"--env-file=custom.env"
			]
		);
	}

	#[test]
	fn supabase_public_engine_url_reads_equals_form_env_file() {
		let temp = tempfile::tempdir().unwrap();
		let env_path = temp.path().join(".env.local");
		std::fs::write(
			&env_path,
			"RIVET_ENDPOINT=\"http://host.docker.internal:6420\"\n",
		)
		.unwrap();
		let mut opts = opts(Some(Provider::Supabase));
		opts.command = vec![format!("--env-file={}", env_path.to_string_lossy())];

		let public_url = resolve_engine_public_url(&opts).unwrap();

		assert_eq!(
			public_url,
			Some("http://host.docker.internal:6420".to_string())
		);
	}
}
