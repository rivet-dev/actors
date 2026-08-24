use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use reqwest::Method;
use serde_json::json;

use crate::{
	DEFAULT_CLOUD_API, DEFAULT_NAMESPACE,
	cloud::{
		CloudClient, TokenInspectResponse, create_or_update_pool, dashboard_endpoint,
		ensure_namespace, pool_exists, registry_endpoint, wait_for_pool,
	},
	credentials::{resolve_token, write_credentials},
	util::{
		build_resources, build_runner_config, default_image_tag, docker_build, docker_login,
		encode, parse_env_vars, run_command,
	},
};

#[derive(Parser)]
pub struct Opts {
	/// Rivet Cloud API token. Also writes ~/.rivet/credentials for later commands.
	#[arg(long)]
	token: Option<String>,
	/// Cloud namespace to deploy to.
	#[arg(long, default_value = DEFAULT_NAMESPACE)]
	namespace: String,
	/// Cloud compute pool to deploy to.
	#[arg(long, default_value = crate::POOL_NAME)]
	pool: String,
	/// Override project from /tokens/api/inspect.
	#[arg(long)]
	project: Option<String>,
	/// Override organization from /tokens/api/inspect.
	#[arg(long)]
	org: Option<String>,
	/// Dockerfile to build.
	#[arg(long, default_value = "Dockerfile")]
	dockerfile: PathBuf,
	/// Docker build context.
	#[arg(long, default_value = ".")]
	build_context: PathBuf,
	/// Environment override, repeatable as KEY=VAL.
	#[arg(long = "env")]
	env_vars: Vec<String>,
	/// Skip prompts.
	#[arg(long)]
	yes: bool,
	/// Cloud API endpoint.
	#[arg(long, default_value = DEFAULT_CLOUD_API)]
	cloud_api: String,
	/// Image repository name in Rivet's registry. Defaults to the project slug.
	#[arg(long)]
	image: Option<String>,
	/// Image tag. Defaults to the current git short SHA, or a timestamp outside git.
	#[arg(long)]
	tag: Option<String>,
	/// vCPU count per actor. Range 0.08 to 8; must be 1, 2, 4, or 8, or a value
	/// in [0.08, 1) with at most two decimals. Defaults to 1 server-side.
	#[arg(long)]
	cpu: Option<f64>,
	/// Memory per actor as <number>Mi or <number>Gi, between 512Mi and 4Gi.
	/// Defaults to 512Mi server-side.
	#[arg(long)]
	memory: Option<String>,
	/// Minimum number of actors to keep running. Range 0 to 100. Defaults to 0 server-side.
	#[arg(long)]
	min_scale: Option<u32>,
	/// Maximum number of actors to scale to. Range 1 to 5000. Defaults to 1 server-side.
	#[arg(long)]
	max_scale: Option<u32>,
	/// Maximum number of concurrent actors the pool runs. Range 1 to 50000.
	/// Defaults to 1000 server-side.
	#[arg(long)]
	max_concurrent_actors: Option<u32>,
	/// Seconds a draining runner instance waits for running actors to finish
	/// before stopping. Range 5 to 3200. Defaults to 1800 server-side.
	#[arg(long)]
	drain_grace_period: Option<u32>,
	/// Whether deploying a new version drains runner instances on the old
	/// version. Pass --drain-on-version-upgrade=false to disable. Defaults to
	/// true server-side.
	#[arg(long, num_args = 0..=1, default_missing_value = "true")]
	drain_on_version_upgrade: Option<bool>,
	/// Number of concurrent requests each actor instance handles. Range 1 to 500.
	/// Defaults to 80 server-side.
	#[arg(long)]
	instance_request_concurrency: Option<u32>,
	/// Reuse the image already deployed to the pool instead of building and
	/// pushing a new one. Skips the Docker build/push and leaves the pool's image
	/// unchanged, which is useful for updating resources without a rebuild.
	#[arg(long)]
	reuse_image: bool,
}

impl Opts {
	pub async fn execute(self) -> Result<()> {
		let token = resolve_token(self.token.as_deref())?;
		if let Some(token) = &self.token {
			write_credentials(token)?;
		}

		if !self.reuse_image && !self.dockerfile.exists() {
			bail!("Dockerfile not found: {}", self.dockerfile.display());
		}

		// Validate compute resources client-side before doing any build work.
		let resources = build_resources(
			self.cpu,
			self.memory.as_deref(),
			self.min_scale,
			self.max_scale,
			self.instance_request_concurrency,
		)?;
		let runner_config = build_runner_config(
			self.max_concurrent_actors,
			self.drain_grace_period,
			self.drain_on_version_upgrade,
		)?;

		let cloud = CloudClient::new(&self.cloud_api, token.clone())?;
		tracing::info!("inspecting Rivet Cloud token");
		let inspect: TokenInspectResponse = cloud
			.request(Method::GET, "/tokens/api/inspect", None)
			.await?
			.context("token inspect returned no body")?;
		let project = self.project.unwrap_or(inspect.project);
		let organization = self.org.unwrap_or(inspect.organization);
		let namespace = ensure_namespace(&cloud, &project, &organization, &self.namespace).await?;

		// Surface the pool in logs only when it is not the default, so the common
		// single-pool case stays uncluttered. A `None` field is omitted by tracing.
		let pool_field = (self.pool != crate::POOL_NAME).then_some(self.pool.as_str());

		let dashboard = dashboard_endpoint(&self.cloud_api)?;
		let dashboard_url = format!(
			"{dashboard}/orgs/{}/projects/{}/ns/{}?skipOnboarding=1",
			encode(&organization),
			encode(&project),
			encode(&namespace.name)
		);

		if !self.yes {
			tracing::info!(
				context = %self.build_context.display(),
				%project,
				namespace = %namespace.name,
				pool = pool_field,
				reuse_image = self.reuse_image,
				"deploying"
			);
		}

		if self.reuse_image {
			// Reusing an image requires a pool that already has one; do not enable
			// a fresh pool here.
			if !pool_exists(&cloud, &project, &organization, &namespace.name, &self.pool).await? {
				bail!(
					"cannot reuse image: no managed pool exists for this namespace, deploy an image first"
				);
			}
		} else {
			tracing::info!(pool = pool_field, "enabling managed pool");
			create_or_update_pool(
				&cloud,
				&project,
				&organization,
				&namespace.name,
				&self.pool,
				json!({ "displayName": self.pool }),
			)
			.await?;
			wait_for_pool(
				&cloud,
				&project,
				&organization,
				&namespace.name,
				&self.pool,
				false,
			)
			.await?;
		}

		// Build and push a new image unless reusing the image already deployed to
		// the pool. When reusing, the `image` field is omitted from the upsert so
		// the pool keeps its current build. Sending `null` would instead clear it.
		let image = if self.reuse_image {
			tracing::info!(
				pool = pool_field,
				"reusing existing pool image, skipping build"
			);
			None
		} else {
			let registry = registry_endpoint(&self.cloud_api)?;
			let image_name = self.image.unwrap_or_else(|| project.clone());
			let tag = self.tag.unwrap_or_else(default_image_tag);
			let image_ref = format!("{registry}/{image_name}:{tag}");

			tracing::info!("logging in to Rivet registry");
			docker_login(&registry, &token)?;

			tracing::info!("building Docker image");
			docker_build(&self.build_context, &self.dockerfile, &image_ref)?;

			tracing::info!("pushing Docker image");
			run_command("docker", &["push", &image_ref], None)?;

			Some((image_name, tag))
		};

		tracing::info!(pool = pool_field, "upserting managed pool");
		let mut pool_body = json!({
			"displayName": self.pool,
		});
		if let Some((image_name, tag)) = image {
			pool_body["image"] = json!({
				"repository": image_name,
				"tag": tag,
			});
		}
		let env_map = parse_env_vars(&self.env_vars)?;
		if !env_map.is_empty() {
			pool_body["environment"] = serde_json::to_value(env_map)?;
		}
		if let Some(runner_config) = runner_config {
			pool_body["runnerConfig"] = runner_config;
		}
		if let Some(resources) = resources {
			pool_body["resources"] = resources;
		}
		create_or_update_pool(
			&cloud,
			&project,
			&organization,
			&namespace.name,
			&self.pool,
			pool_body,
		)
		.await?;
		wait_for_pool(
			&cloud,
			&project,
			&organization,
			&namespace.name,
			&self.pool,
			true,
		)
		.await?;

		// The dashboard URL is the command's result; print it to stdout so it
		// can be captured by scripts.
		println!("{dashboard_url}");
		Ok(())
	}
}
