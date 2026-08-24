use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::StreamExt;
use reqwest::Method;
use reqwest_eventsource::{Event, EventSource};

use crate::{
	DEFAULT_CLOUD_API, DEFAULT_NAMESPACE,
	cloud::{CloudClient, LogEntry, TokenInspectResponse, get_namespace},
	credentials::resolve_token,
	util::{color_enabled, encode},
};

#[derive(Parser)]
pub struct Opts {
	/// Rivet Cloud API token.
	#[arg(long)]
	token: Option<String>,
	/// Cloud namespace to read logs from.
	#[arg(long, default_value = DEFAULT_NAMESPACE)]
	namespace: String,
	/// Cloud compute pool to read logs from.
	#[arg(long, default_value = crate::POOL_NAME)]
	pool: String,
	/// Override project from /tokens/api/inspect.
	#[arg(long)]
	project: Option<String>,
	/// Override organization from /tokens/api/inspect.
	#[arg(long)]
	org: Option<String>,
	/// Follow the log output (live tail).
	#[arg(long, short = 'f')]
	follow: bool,
	/// Maximum number of history entries to fetch. Ignored with --follow.
	#[arg(long, short = 'n', default_value_t = 100)]
	limit: u32,
	/// Only show logs before this ISO 8601 timestamp (history pagination cursor).
	#[arg(long)]
	before: Option<String>,
	/// Filter by region(s), comma-separated.
	#[arg(long)]
	region: Option<String>,
	/// Only show lines containing this substring (case-insensitive).
	#[arg(long)]
	contains: Option<String>,
	/// Emit raw JSON lines instead of formatted output.
	#[arg(long)]
	json: bool,
	/// Cloud API endpoint.
	#[arg(long, default_value = DEFAULT_CLOUD_API)]
	cloud_api: String,
}

impl Opts {
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

		if self.follow {
			self.tail(&cloud, &project, &org, &namespace.name, &self.pool)
				.await
		} else {
			self.history(&cloud, &project, &org, &namespace.name, &self.pool)
				.await
		}
	}

	async fn history(
		&self,
		cloud: &CloudClient,
		project: &str,
		org: &str,
		namespace: &str,
		pool: &str,
	) -> Result<()> {
		let mut path = format!(
			"/projects/{}/namespaces/{}/managed-pools/{}/logs/history?org={}&limit={}",
			encode(project),
			encode(namespace),
			encode(pool),
			encode(org),
			self.limit,
		);
		if let Some(before) = &self.before {
			path.push_str(&format!("&before={}", encode(before)));
		}
		if let Some(region) = &self.region {
			path.push_str(&format!("&region={}", encode(region)));
		}
		if let Some(contains) = &self.contains {
			path.push_str(&format!("&contains={}", encode(contains)));
		}

		let entries: Vec<LogEntry> = cloud
			.request(Method::GET, &path, None)
			.await?
			.unwrap_or_default();

		let color = color_enabled();
		for entry in &entries {
			print_line(entry, self.json, color)?;
		}
		Ok(())
	}

	async fn tail(
		&self,
		cloud: &CloudClient,
		project: &str,
		org: &str,
		namespace: &str,
		pool: &str,
	) -> Result<()> {
		let mut path = format!(
			"/projects/{}/namespaces/{}/managed-pools/{}/logs?org={}",
			encode(project),
			encode(namespace),
			encode(pool),
			encode(org),
		);
		if let Some(region) = &self.region {
			path.push_str(&format!("&region={}", encode(region)));
		}
		if let Some(contains) = &self.contains {
			path.push_str(&format!("&contains={}", encode(contains)));
		}

		let builder = cloud.get_builder(&path)?;
		let mut events = EventSource::new(builder).context("failed to open log stream")?;
		let color = color_enabled();

		while let Some(event) = events.next().await {
			match event {
				Ok(Event::Open) => {}
				Ok(Event::Message(message)) => match message.event.as_str() {
					"connected" => {}
					"log" => {
						let entry: LogEntry = serde_json::from_str(&message.data)
							.context("failed to parse log entry")?;
						print_line(&entry, self.json, color)?;
					}
					"end" => {
						events.close();
						break;
					}
					"error" => {
						events.close();
						bail!("log stream error: {}", message.data);
					}
					_ => {}
				},
				Err(reqwest_eventsource::Error::StreamEnded) => break,
				Err(err) => {
					events.close();
					return Err(err).context("log stream failed");
				}
			}
		}
		Ok(())
	}
}

fn print_line(entry: &LogEntry, json: bool, color: bool) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string(entry)?);
		return Ok(());
	}

	let severity = if color {
		format!(
			"\x1b[{}m{}\x1b[0m",
			severity_color(&entry.severity),
			entry.severity
		)
	} else {
		entry.severity.clone()
	};
	println!(
		"{} [{}] {} {}",
		entry.timestamp, severity, entry.region, entry.message
	);
	Ok(())
}

/// Maps a GCP log severity to an ANSI SGR color code.
fn severity_color(severity: &str) -> &'static str {
	match severity {
		"DEBUG" => "90",
		"INFO" => "32",
		"NOTICE" => "36",
		"WARNING" => "33",
		"ERROR" => "31",
		"CRITICAL" | "ALERT" | "EMERGENCY" => "1;31",
		_ => "37",
	}
}
