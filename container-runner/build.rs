//! Captures the git commit SHA at build time and exposes it to the binary as the
//! `CONTAINER_RUNNER_GIT_SHA` compile-time env var.
//!
//! Resolution order, chosen so the build never fails when git is unavailable:
//!   1. `OVERRIDE_GIT_SHA` env var. The release image build context excludes
//!      `.git` (see `.dockerignore`), so CI/Docker injects the SHA this way.
//!   2. A local `git rev-parse HEAD`, for colocated dev builds.
//!   3. `"unknown"`.

use std::process::Command;

fn main() {
	println!("cargo:rerun-if-env-changed=OVERRIDE_GIT_SHA");

	let git_sha = std::env::var("OVERRIDE_GIT_SHA")
		.ok()
		.filter(|sha| !sha.trim().is_empty())
		.or_else(git_head_sha)
		.unwrap_or_else(|| "unknown".to_string());

	println!("cargo:rustc-env=CONTAINER_RUNNER_GIT_SHA={git_sha}");
}

fn git_head_sha() -> Option<String> {
	let output = Command::new("git")
		.args(["rev-parse", "HEAD"])
		.output()
		.ok()?;
	if !output.status.success() {
		return None;
	}
	let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
	if sha.is_empty() { None } else { Some(sha) }
}
