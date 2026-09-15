use anyhow::Result;
use clap::{Args, Subcommand};

mod upload;

#[derive(Args)]
pub struct Opts {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Manage BYOC worker builds.
	Workers(Workers),
}

#[derive(Args)]
struct Workers {
	#[command(subcommand)]
	command: WorkerCommand,
}

#[derive(Subcommand)]
enum WorkerCommand {
	/// Upload worker image archives.
	Builds(Builds),
}

#[derive(Args)]
struct Builds {
	#[command(subcommand)]
	command: BuildCommand,
}

#[derive(Subcommand)]
enum BuildCommand {
	/// Upload a Linux/AMD64 image to an existing presigned PUT URL. Does not finalize or deploy.
	UploadPresigned(upload::Opts),
}

impl Opts {
	pub async fn execute(self) -> Result<()> {
		match self.command {
			Command::Workers(Workers {
				command:
					WorkerCommand::Builds(Builds {
						command: BuildCommand::UploadPresigned(opts),
					}),
			}) => opts.execute().await,
		}
	}
}
