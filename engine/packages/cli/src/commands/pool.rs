use std::{
	io::{IsTerminal, Write},
	time::Duration,
};

use anstyle::{AnsiColor, Style};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use reqwest::Method;
use tokio::time::sleep;

use crate::{
	DEFAULT_CLOUD_API, DEFAULT_NAMESPACE,
	cloud::{
		CloudClient, PoolConfig, PoolResources, PoolSummary, TokenInspectResponse, delete_pool,
		get_namespace, list_pools,
	},
	credentials::resolve_token,
	util::color_enabled,
};

#[derive(Parser)]
pub struct Opts {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	/// List the compute pools in a namespace.
	List(ListOpts),
	/// Delete a compute pool from a namespace.
	Delete(DeleteOpts),
}

#[derive(Parser)]
pub struct ListOpts {
	/// Rivet Cloud API token.
	#[arg(long)]
	token: Option<String>,
	/// Cloud namespace to list pools from.
	#[arg(long, default_value = DEFAULT_NAMESPACE)]
	namespace: String,
	/// Override project from /tokens/api/inspect.
	#[arg(long)]
	project: Option<String>,
	/// Override organization from /tokens/api/inspect.
	#[arg(long)]
	org: Option<String>,
	/// Emit raw JSON instead of formatted output.
	#[arg(long)]
	json: bool,
	/// Include environment variable values in the output. Env var values are
	/// secret and are omitted unless this flag is passed.
	#[arg(long)]
	with_env_vars: bool,
	/// Cloud API endpoint.
	#[arg(long, default_value = DEFAULT_CLOUD_API)]
	cloud_api: String,
}

#[derive(Parser)]
pub struct DeleteOpts {
	/// Name of the compute pool to delete.
	name: String,
	/// Rivet Cloud API token.
	#[arg(long)]
	token: Option<String>,
	/// Cloud namespace the pool belongs to.
	#[arg(long, default_value = DEFAULT_NAMESPACE)]
	namespace: String,
	/// Override project from /tokens/api/inspect.
	#[arg(long)]
	project: Option<String>,
	/// Override organization from /tokens/api/inspect.
	#[arg(long)]
	org: Option<String>,
	/// Skip the confirmation prompt.
	#[arg(long)]
	yes: bool,
	/// Cloud API endpoint.
	#[arg(long, default_value = DEFAULT_CLOUD_API)]
	cloud_api: String,
}

impl Opts {
	pub async fn execute(self) -> Result<()> {
		match self.command {
			Commands::List(opts) => opts.execute().await,
			Commands::Delete(opts) => opts.execute().await,
		}
	}
}

impl ListOpts {
	pub async fn execute(self) -> Result<()> {
		let token = resolve_token(self.token.as_deref())?;
		let cloud = CloudClient::new(&self.cloud_api, token)?;

		let inspect: TokenInspectResponse = cloud
			.request(Method::GET, "/tokens/api/inspect", None)
			.await?
			.context("token inspect returned no body")?;
		let project = self.project.clone().unwrap_or(inspect.project);
		let org = self.org.clone().unwrap_or(inspect.organization);
		let namespace = get_namespace(&cloud, &project, &org, &self.namespace).await?;

		let mut pools = list_pools(&cloud, &project, &org, &namespace.name).await?;

		if self.json {
			if !self.with_env_vars {
				for pool in &mut pools {
					if let Some(config) = &mut pool.config {
						config.environment.clear();
					}
				}
			}
			println!("{}", serde_json::to_string(&pools)?);
			return Ok(());
		}

		if pools.is_empty() {
			eprintln!("no compute pools in namespace {}", namespace.name);
			return Ok(());
		}

		let color = color_enabled();
		for pool in &pools {
			print_pool(pool, color, self.with_env_vars);
		}
		Ok(())
	}
}

impl DeleteOpts {
	pub async fn execute(self) -> Result<()> {
		let token = resolve_token(self.token.as_deref())?;
		let cloud = CloudClient::new(&self.cloud_api, token)?;

		let inspect: TokenInspectResponse = cloud
			.request(Method::GET, "/tokens/api/inspect", None)
			.await?
			.context("token inspect returned no body")?;
		let project = self.project.clone().unwrap_or(inspect.project);
		let org = self.org.clone().unwrap_or(inspect.organization);
		let namespace = get_namespace(&cloud, &project, &org, &self.namespace).await?;

		if !self.yes && !confirm(&self.name, &namespace.name)? {
			eprintln!("aborted");
			return Ok(());
		}

		delete_pool(&cloud, &project, &org, &namespace.name, &self.name).await?;
		eprintln!(
			"deleting pool '{}' in namespace '{}'",
			self.name, namespace.name
		);

		wait_for_deletion(&cloud, &project, &org, &namespace.name, &self.name).await
	}
}

/// Polls the pool list until the named pool disappears. The coordinator drops a
/// pool from the list once its teardown completes, so its absence is the
/// completion signal. Status changes are printed as they happen, and the poll is
/// bounded so the command never hangs if teardown stalls.
async fn wait_for_deletion(
	cloud: &CloudClient,
	project: &str,
	org: &str,
	namespace: &str,
	name: &str,
) -> Result<()> {
	let mut last_status: Option<String> = None;
	for _ in 0..180 {
		let pools = list_pools(cloud, project, org, namespace).await?;
		let Some(pool) = pools.iter().find(|pool| pool.name == name) else {
			eprintln!("pool '{name}' deleted");
			return Ok(());
		};

		let status = pool.status.as_deref().unwrap_or("unknown");
		if last_status.as_deref() != Some(status) {
			eprintln!("pool '{name}' status: {status}");
			last_status = Some(status.to_string());
		}
		sleep(Duration::from_secs(2)).await;
	}

	bail!("timed out waiting for pool '{name}' to delete; check `rivet pool list`")
}

/// Prompts for confirmation on a destructive delete. Fails closed when stdin is
/// not a terminal so scripts must pass `--yes` explicitly rather than hang.
fn confirm(pool: &str, namespace: &str) -> Result<bool> {
	if !std::io::stdin().is_terminal() {
		bail!("refusing to delete without confirmation; pass --yes to delete non-interactively");
	}
	eprint!("delete pool '{pool}' in namespace '{namespace}'? [y/N] ");
	std::io::stderr().flush()?;
	let mut input = String::new();
	std::io::stdin().read_line(&mut input)?;
	let answer = input.trim().to_ascii_lowercase();
	Ok(answer == "y" || answer == "yes")
}

/// Prints a pool as a header line with its colored status followed by indented
/// config detail and, if present, its error in red.
fn print_pool(pool: &PoolSummary, color: bool, with_env_vars: bool) {
	let status = pool.status.as_deref().unwrap_or("unknown");
	println!(
		"{}  {}",
		pool.name,
		paint(status, status_style(status), color)
	);

	if let Some(config) = &pool.config {
		print_config(config, with_env_vars);
	}

	if let Some(error) = &pool.error {
		let style = Style::new().fg_color(Some(AnsiColor::Red.into()));
		println!("  {:<10} {}", "error", paint(&error.message, style, color));
	}
}

fn print_config(config: &PoolConfig, with_env_vars: bool) {
	if let Some(display_name) = &config.display_name {
		println!("  {:<10} {display_name}", "display");
	}

	match &config.image {
		Some(image) => println!("  {:<10} {}:{}", "image", image.repository, image.tag),
		None => println!("  {:<10} none", "image"),
	}

	// Fall back to "defaults" both when there is no resources object and when it
	// is present but has no populated fields, since either means the pool uses
	// the server-side defaults.
	let parts = config
		.resources
		.as_ref()
		.map(resource_parts)
		.unwrap_or_default();
	if parts.is_empty() {
		println!("  {:<10} defaults", "resources");
	} else {
		println!("  {:<10} {}", "resources", parts.join(" · "));
	}

	let env_count = config.environment.len();
	if env_count > 0 {
		if with_env_vars {
			println!("  {:<10} {env_count}", "env vars");
			for (key, value) in &config.environment {
				println!("  {:<10} {key}={value}", "");
			}
		} else {
			println!("  {:<10} {env_count} (use --with-env-vars to show)", "env vars");
		}
	}
}

/// Builds the human-readable resource fragments for a pool, including whichever
/// of `min_scale` / `max_scale` are set (they are independently optional).
fn resource_parts(res: &PoolResources) -> Vec<String> {
	let mut parts = Vec::new();
	if let Some(cpu) = res.cpu {
		parts.push(format!("{cpu} vCPU"));
	}
	if let Some(memory) = &res.memory {
		parts.push(memory.clone());
	}
	match (res.min_scale, res.max_scale) {
		(Some(min), Some(max)) => parts.push(format!("scale {min}-{max}")),
		(Some(min), None) => parts.push(format!("min scale {min}")),
		(None, Some(max)) => parts.push(format!("max scale {max}")),
		(None, None) => {}
	}
	if let Some(concurrency) = res.instance_request_concurrency {
		parts.push(format!("concurrency {concurrency}"));
	}
	parts
}

/// Maps a compute pool status to its display color. Unknown statuses render
/// without color.
fn status_style(status: &str) -> Style {
	let color = match status {
		"initializing" => Some(AnsiColor::BrightBlack),
		"allocating" | "destroying" => Some(AnsiColor::White),
		"deploying" | "binding" => Some(AnsiColor::Cyan),
		"ready" => Some(AnsiColor::Green),
		"error" => Some(AnsiColor::Red),
		_ => None,
	};
	Style::new().fg_color(color.map(Into::into))
}

fn paint(text: &str, style: Style, enabled: bool) -> String {
	if enabled {
		format!("{}{text}{}", style.render(), style.render_reset())
	} else {
		text.to_string()
	}
}
