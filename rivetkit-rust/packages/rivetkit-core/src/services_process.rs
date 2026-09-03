use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, Url};
use rivet_error::RivetError;
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};

use crate::time::sleep;

/// Dedicated pool used by the first-party services actor host.
pub(crate) const SERVICES_POOL_NAME: &str = "services";

const READINESS_MAX_ATTEMPTS: usize = 60;
const READINESS_RETRY_DELAY: Duration = Duration::from_millis(500);
// The child is itself a RivetKit registry. SIGTERM starts its normal envoy and
// actor drain, so allow the same 30-minute fallback used by serverful RivetKit
// before escalating to SIGKILL.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug)]
pub(crate) struct ServicesProcessConfig {
	pub binary_path: Option<PathBuf>,
	pub endpoint: String,
	pub token: Option<String>,
	pub namespace: String,
	pub pool_name: String,
	pub engine_protocol_version: u16,
}

#[derive(Debug)]
pub(crate) struct ServicesProcessManager {
	child: Child,
}

#[derive(Debug, Deserialize)]
struct EnvoysResponse {
	envoys: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServicesVersionOutput {
	name: String,
	version: String,
	rivetkit_version: String,
	protocol_version: u16,
}

#[derive(RivetError, Debug, Serialize)]
#[error("services")]
enum ServicesProcessError {
	#[error(
		"binary_unavailable",
		"Services binary is unavailable.",
		"No Services binary was provided. Install @rivet-dev/services, set RIVET_SERVICES_BINARY, or set RIVET_RUN_SERVICES=0 to disable Services."
	)]
	BinaryUnavailable,

	#[error(
		"binary_not_found",
		"Services binary was not found.",
		"Services binary was not found at '{path}'."
	)]
	BinaryNotFound { path: String },

	#[error(
		"metadata_failed",
		"Services compatibility could not be verified.",
		"Services compatibility could not be verified: {reason}"
	)]
	MetadataFailed { reason: String },

	#[error(
		"protocol_mismatch",
		"Services is newer than the local Engine protocol.",
		"Services uses Envoy protocol {services_protocol_version}, but this RivetKit Engine supports {engine_protocol_version}. Upgrade RivetKit or install an older @rivet-dev/services version."
	)]
	ProtocolMismatch {
		services_protocol_version: u16,
		engine_protocol_version: u16,
	},

	#[error(
		"start_failed",
		"Services failed to start.",
		"Services failed to start: {reason}"
	)]
	StartFailed { reason: String },

	#[error(
		"readiness_failed",
		"Services did not become ready.",
		"Services did not register in pool '{pool_name}': {reason}"
	)]
	ReadinessFailed { pool_name: String, reason: String },
}

impl ServicesProcessManager {
	pub(crate) async fn start(config: ServicesProcessConfig) -> Result<Self> {
		let binary_path = config
			.binary_path
			.as_deref()
			.ok_or_else(|| ServicesProcessError::BinaryUnavailable.build())?;
		if !binary_path.exists() {
			return Err(ServicesProcessError::BinaryNotFound {
				path: binary_path.display().to_string(),
			}
			.build());
		}

		validate_binary_compatibility(binary_path, &config).await?;

		let mut command = Command::new(binary_path);
		command
			.arg("start")
			.env("RIVET_ENDPOINT", &config.endpoint)
			.env("RIVET_NAMESPACE", &config.namespace)
			.env("RIVET_POOL_NAME", &config.pool_name)
			.env("RIVETKIT_ENGINE_SPAWN", "never")
			// Prevent the child RivetKit registry from recursively starting another
			// Services process.
			.env("RIVET_RUN_SERVICES", "0")
			.stdin(Stdio::null())
			.stdout(Stdio::inherit())
			.stderr(Stdio::inherit())
			.kill_on_drop(true);
		if let Some(token) = &config.token {
			command.env("RIVET_TOKEN", token);
		} else {
			command.env_remove("RIVET_TOKEN");
		}

		let mut child = command.spawn().map_err(|error| {
			ServicesProcessError::StartFailed {
				reason: format!("could not spawn `{}`: {error}", binary_path.display()),
			}
			.build()
		})?;
		wait_for_readiness(
			&mut child,
			&config,
			READINESS_MAX_ATTEMPTS,
			READINESS_RETRY_DELAY,
		)
		.await?;

		tracing::info!(
			pid = child.id(),
			path = %binary_path.display(),
			endpoint = %config.endpoint,
			namespace = %config.namespace,
			pool_name = %config.pool_name,
			"Services process is ready"
		);
		Ok(Self { child })
	}

	pub(crate) async fn shutdown(mut self) {
		if self.child.try_wait().ok().flatten().is_some() {
			return;
		}

		#[cfg(unix)]
		let signaled = self.child.id().is_some_and(|pid| {
			use nix::sys::signal::{Signal, kill};
			use nix::unistd::Pid;

			kill(Pid::from_raw(pid as i32), Signal::SIGTERM).is_ok()
		});
		#[cfg(not(unix))]
		let signaled = false;

		if !signaled && self.child.start_kill().is_err() {
			return;
		}

		if tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait())
			.await
			.is_err()
		{
			tracing::warn!(pid = self.child.id(), "Services did not stop; killing it");
			let _ = self.child.start_kill();
			let _ = self.child.wait().await;
		}
	}
}

async fn validate_binary_compatibility(
	binary_path: &Path,
	config: &ServicesProcessConfig,
) -> Result<()> {
	let output = run_metadata_command(binary_path, "--version").await?;
	let output: ServicesVersionOutput = serde_json::from_str(&output)
		.map_err(|error| metadata_error(format!("invalid --version output: {error}")))?;
	if output.name != "rivet-services" {
		return Err(metadata_error(format!(
			"unexpected binary name `{}` in --version output",
			output.name
		)));
	}
	tracing::debug!(
		services_version = %output.version,
		services_rivetkit_version = %output.rivetkit_version,
		services_protocol_version = output.protocol_version,
		engine_protocol_version = config.engine_protocol_version,
		"validated Services binary metadata"
	);
	// This protocol comparison is the compatibility safeguard. Services package
	// semver is diagnostic only and must be bumped whenever its Envoy protocol
	// version changes so package resolution can select a compatible binary.
	validate_protocol_version(output.protocol_version, config.engine_protocol_version)
}

fn validate_protocol_version(services_version: u16, engine_version: u16) -> Result<()> {
	if services_version > engine_version {
		return Err(ServicesProcessError::ProtocolMismatch {
			services_protocol_version: services_version,
			engine_protocol_version: engine_version,
		}
		.build());
	}
	Ok(())
}

async fn run_metadata_command(binary_path: &Path, argument: &str) -> Result<String> {
	let output = Command::new(binary_path)
		.arg(argument)
		.stdin(Stdio::null())
		.kill_on_drop(true)
		.output()
		.await
		.map_err(|error| {
			metadata_error(format!(
				"could not run `{}` {argument}: {error}",
				binary_path.display()
			))
		})?;
	if !output.status.success() {
		return Err(metadata_error(format!(
			"`{}` {argument} exited with {}: {}",
			binary_path.display(),
			output.status,
			String::from_utf8_lossy(&output.stderr).trim()
		)));
	}

	String::from_utf8(output.stdout)
		.map(|output| output.trim().to_owned())
		.map_err(|error| metadata_error(format!("{argument} returned invalid UTF-8: {error}")))
}

fn metadata_error(reason: impl Into<String>) -> anyhow::Error {
	ServicesProcessError::MetadataFailed {
		reason: reason.into(),
	}
	.build()
}

async fn wait_for_readiness(
	child: &mut Child,
	config: &ServicesProcessConfig,
	max_attempts: usize,
	retry_delay: Duration,
) -> Result<()> {
	let client = Client::builder()
		.build()
		.context("build Services readiness client")?;
	let mut url = Url::parse(&config.endpoint)
		.with_context(|| format!("parse Engine endpoint `{}`", config.endpoint))?;
	url.set_path("/envoys");
	url.set_query(None);
	url.query_pairs_mut()
		.append_pair("namespace", &config.namespace)
		.append_pair("name", &config.pool_name);

	let max_attempts = max_attempts.max(1);
	let mut last_reason = "the Engine has not listed the Services Envoy".to_owned();
	for attempt in 1..=max_attempts {
		if let Some(status) = child.try_wait().map_err(|error| {
			ServicesProcessError::StartFailed {
				reason: format!("could not inspect child process: {error}"),
			}
			.build()
		})? {
			return Err(ServicesProcessError::StartFailed {
				reason: format!("process exited with status {status}"),
			}
			.build());
		}

		let mut request = client.get(url.clone());
		if let Some(token) = &config.token {
			request = request.bearer_auth(token);
		}
		match request.send().await {
			Ok(response) if response.status().is_success() => {
				match response.json::<EnvoysResponse>().await {
					Ok(response) if !response.envoys.is_empty() => return Ok(()),
					Ok(_) => {
						last_reason = "the Engine returned no matching envoys".to_owned();
					}
					Err(error) => last_reason = format!("invalid Engine response: {error}"),
				}
			}
			Ok(response) => {
				let status = response.status();
				let body = response.text().await.unwrap_or_default();
				last_reason = format!("Engine returned {status}: {body}");
			}
			Err(error) => last_reason = error.to_string(),
		}

		if attempt < max_attempts {
			sleep(retry_delay).await;
		}
	}

	Err(ServicesProcessError::ReadinessFailed {
		pool_name: config.pool_name.clone(),
		reason: last_reason,
	}
	.build())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn config(binary_path: Option<PathBuf>) -> ServicesProcessConfig {
		ServicesProcessConfig {
			binary_path,
			endpoint: "http://127.0.0.1:6420".to_owned(),
			token: Some("dev".to_owned()),
			namespace: "default".to_owned(),
			pool_name: SERVICES_POOL_NAME.to_owned(),
			engine_protocol_version: 7,
		}
	}

	#[test]
	fn parses_structured_version_output() {
		let output: ServicesVersionOutput = serde_json::from_str(
			r#"{"name":"rivet-services","version":"0.2.0-rc.1","rivetkitVersion":"2.3.11","protocolVersion":7}"#,
		)
		.expect("version output should parse");
		assert_eq!(output.name, "rivet-services");
		assert_eq!(output.version, "0.2.0-rc.1");
		assert_eq!(output.rivetkit_version, "2.3.11");
		assert_eq!(output.protocol_version, 7);
	}

	#[tokio::test]
	async fn missing_binary_is_a_structured_error() {
		let error = ServicesProcessManager::start(config(None))
			.await
			.expect_err("missing binary should fail");

		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "services");
		assert_eq!(error.code(), "binary_unavailable");
	}

	#[test]
	fn rejects_a_newer_protocol() {
		let error =
			validate_protocol_version(8, 7).expect_err("newer Services protocol should fail");
		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "services");
		assert_eq!(error.code(), "protocol_mismatch");
	}

	#[test]
	fn accepts_an_equal_or_older_protocol() {
		validate_protocol_version(7, 7).expect("equal protocol should be compatible");
		validate_protocol_version(6, 7).expect("older protocol should be compatible");
	}
}
