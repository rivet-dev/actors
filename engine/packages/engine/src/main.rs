use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use once_cell::sync::Lazy;
use rivet_engine::{SubCommand, run_config};
use rivet_util::build_meta;

static LONG_VERSION: Lazy<String> = Lazy::new(|| {
	format!(
		"{}\nGit SHA: {}\nBuild Timestamp: {}\nRustc Version: {}\nRustc Host: {}\nCargo Target: {}\nCargo Profile: {}",
		build_meta::VERSION,
		build_meta::GIT_SHA,
		build_meta::BUILD_TIMESTAMP,
		build_meta::RUSTC_VERSION,
		build_meta::RUSTC_HOST,
		build_meta::CARGO_TARGET,
		build_meta::cargo_profile()
	)
});

#[derive(Parser)]
#[command(name = "Rivet", version, long_version = LONG_VERSION.as_str(), about)]
struct Cli {
	#[command(subcommand)]
	command: SubCommand,

	/// Path to the config file or directory of config files
	#[clap(long, global = true)]
	config: Vec<PathBuf>,
}

fn main() -> Result<()> {
	rivet_runtime::run(main_inner()).transpose()?;
	Ok(())
}

async fn main_inner() -> Result<()> {
	tracing::info!(version=%build_meta::VERSION, git_sha=%build_meta::GIT_SHA, built_at=%build_meta::BUILD_TIMESTAMP, "starting rivet");

	let cli = Cli::parse();

	// Load config
	let config = rivet_config::Config::load(&cli.config).await?;
	tracing::info!(config=?*config, "loaded config");

	// Initialize telemetry (does nothing if telemetry is disabled)
	let _guard = rivet_telemetry::init(&config);

	// Build run config
	let run_config = Arc::new(run_config::config(config.clone()).inspect_err(|err| {
		rivet_telemetry::capture_error(err);
	})?);

	// Execute command
	cli.command
		.execute(config, run_config)
		.await
		.inspect_err(|err| {
			rivet_telemetry::capture_error(err);
		})
}
