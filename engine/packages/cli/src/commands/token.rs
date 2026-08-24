use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use reqwest::Method;

use crate::{
	DEFAULT_CLOUD_API, DEFAULT_NAMESPACE,
	cloud::{CloudClient, TokenInspectResponse, create_token, ensure_namespace, get_namespace},
	credentials::resolve_token,
};

#[derive(Parser)]
pub struct Opts {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	/// Create a namespace-scoped token.
	Create(CreateOpts),
}

impl Opts {
	pub async fn execute(self) -> Result<()> {
		match self.command {
			Commands::Create(opts) => opts.execute().await,
		}
	}
}

#[derive(Clone, Copy, ValueEnum)]
enum TokenKind {
	/// Server-side runner/runtime token, prefixed `sk_`.
	Secret,
	/// Client-safe publishable token, prefixed `pk_`.
	Public,
	/// Connection token, prefixed `ck_`. A strict subset of a publishable
	/// token: resolves actors and opens connections, but cannot manage them.
	Connection,
}

impl TokenKind {
	/// The Cloud API path segment under `.../tokens/` for this kind.
	fn endpoint(self) -> &'static str {
		match self {
			TokenKind::Secret => "secret",
			TokenKind::Public => "publishable",
			TokenKind::Connection => "connection",
		}
	}
}

#[derive(Parser)]
pub struct CreateOpts {
	/// Rivet Cloud API token used to authorize the mint.
	#[arg(long)]
	token: Option<String>,
	/// Namespace to mint the token for.
	#[arg(long, default_value = DEFAULT_NAMESPACE)]
	namespace: String,
	/// Kind of token to create.
	#[arg(long, value_enum)]
	kind: TokenKind,
	/// Create the namespace if it does not already exist. By default the
	/// namespace must exist.
	#[arg(long)]
	create_namespace: bool,
	/// Override project from /tokens/api/inspect.
	#[arg(long)]
	project: Option<String>,
	/// Override organization from /tokens/api/inspect.
	#[arg(long)]
	org: Option<String>,
	/// Cloud API endpoint.
	#[arg(long, default_value = DEFAULT_CLOUD_API)]
	cloud_api: String,
}

impl CreateOpts {
	pub async fn execute(self) -> Result<()> {
		let token = resolve_token(self.token.as_deref())?;

		let cloud = CloudClient::new(&self.cloud_api, token)?;
		tracing::info!("inspecting Rivet Cloud token");
		let inspect: TokenInspectResponse = cloud
			.request(Method::GET, "/tokens/api/inspect", None)
			.await?
			.context("token inspect returned no body")?;
		let project = self.project.unwrap_or(inspect.project);
		let organization = self.org.unwrap_or(inspect.organization);

		let namespace = if self.create_namespace {
			ensure_namespace(&cloud, &project, &organization, &self.namespace).await?
		} else {
			get_namespace(&cloud, &project, &organization, &self.namespace).await?
		};

		let token = create_token(
			&cloud,
			&project,
			&organization,
			&namespace.name,
			self.kind.endpoint(),
		)
		.await?;

		// The token is the command's result; print only it to stdout so scripts
		// can capture it directly. Everything else logs to stderr via tracing.
		println!("{token}");

		Ok(())
	}
}
