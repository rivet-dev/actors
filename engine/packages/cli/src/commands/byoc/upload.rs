use std::{
	collections::{HashMap, HashSet},
	fs::{self, File},
	io::{self, Read},
	path::{Component, Path, PathBuf},
	process::Stdio,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use clap::{ArgGroup, Args};
use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use url::Url;

const MAX_ARCHIVE: u64 = 5 * 1024 * 1024 * 1024;
const MAX_EXPANDED: u64 = 32 * 1024 * 1024 * 1024;
const MAX_JSON: u64 = 1024 * 1024;

#[derive(Args)]
#[command(group(ArgGroup::new("source").required(true).multiple(false).args(["image", "archive", "dockerfile"])))]
pub struct Opts {
	/// Existing presigned object-storage PUT URL. Quote it; treat it as a credential.
	#[arg(long)]
	presigned_build_url: String,
	/// Local Docker image tag or ID. Must contain exactly one Linux/AMD64 image.
	#[arg(long)]
	image: Option<String>,
	/// Existing gzip-compressed docker-save archive. Docker is not required.
	#[arg(long)]
	archive: Option<PathBuf>,
	/// Build this Dockerfile for linux/amd64 before exporting and uploading.
	#[arg(long)]
	dockerfile: Option<PathBuf>,
	/// Local Docker build context, defaults to the current directory. Requires --dockerfile.
	#[arg(long, requires = "dockerfile", conflicts_with_all = ["image", "archive"])]
	context: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Uploaded {
	uploaded: bool,
	architecture: &'static str,
	image_id: String,
	sha256: String,
	size_bytes: u64,
	uncompressed_size_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Manifest {
	config: String,
	layers: Vec<String>,
}

#[derive(Deserialize)]
struct ImageConfig {
	os: String,
	architecture: String,
	rootfs: RootFs,
}

#[derive(Deserialize)]
struct RootFs {
	diff_ids: Vec<String>,
}

impl Opts {
	pub async fn execute(self) -> Result<()> {
		// Cancellation drops the private temporary directory and kills CLI subprocesses.
		tokio::select! {
			result = self.execute_inner() => result,
			signal = tokio::signal::ctrl_c() => {
				signal.context("install interrupt handler")?;
				bail!("upload canceled; no build finalization or deployment was performed")
			}
		}
	}

	async fn execute_inner(self) -> Result<()> {
		validate_url(&self.presigned_build_url)?;
		let scratch = tempfile::Builder::new()
			.prefix("rivet-worker-upload-")
			.tempdir()?;
		let archive = scratch.path().join("image.tar.gz");
		if let Some(input) = self.archive {
			let mut source = File::open(input).context("open --archive")?;
			ensure!(
				source.metadata()?.is_file(),
				"--archive must be a regular file"
			);
			ensure!(
				source.metadata()?.len() <= MAX_ARCHIVE,
				"archive exceeds 5 GiB"
			);
			// Snapshot the input so validation, upload and retries use the same bytes.
			let copied = io::copy(
				&mut Read::by_ref(&mut source).take(MAX_ARCHIVE + 1),
				&mut File::create(&archive)?,
			)?;
			ensure!(copied <= MAX_ARCHIVE, "archive exceeds 5 GiB");
		} else {
			let image = if let Some(dockerfile) = self.dockerfile {
				ensure!(
					dockerfile.is_file(),
					"--dockerfile must be an existing file"
				);
				let context = self.context.unwrap_or_else(|| PathBuf::from("."));
				ensure!(context.is_dir(), "--context must be a local directory");
				let iid = scratch.path().join("image-id");
				tracing::info!("building worker image for linux/amd64");
				docker(&[
					"build",
					"--load",
					"--platform",
					"linux/amd64",
					"--iidfile",
					path(&iid)?,
					"-f",
					path(&dockerfile)?,
					"--",
					path(&context)?,
				])
				.await?;
				fs::read_to_string(iid)?.trim().to_owned()
			} else {
				self.image.context("missing image input")?
			};
			let inspected = Command::new("docker")
				.args(["image", "inspect", "--format", "{{json .}}", "--", &image])
				.kill_on_drop(true)
				.output()
				.await
				.context("run Docker; install it and start its daemon")?;
			ensure!(
				inspected.status.success(),
				"Docker could not inspect the local image; check its tag and daemon availability"
			);
			let config: serde_json::Value = serde_json::from_slice(&inspected.stdout)
				.context("invalid Docker image metadata")?;
			ensure!(
				config["Os"] == "linux" && config["Architecture"] == "amd64",
				"worker image must be linux/amd64; rebuild with --platform linux/amd64"
			);
			let id = config["Id"]
				.as_str()
				.context("Docker returned no image ID")?;
			ensure!(
				id.starts_with("sha256:") && id.len() == 71,
				"invalid Docker image ID"
			);
			let tar = scratch.path().join("image.tar");
			tracing::info!("exporting worker image");
			// Save the inspected immutable ID, not a tag that could move before export.
			docker(&["image", "save", "--output", path(&tar)?, "--", id]).await?;
			ensure!(
				fs::metadata(&tar)?.len() <= MAX_EXPANDED,
				"image archive exceeds 32 GiB uncompressed"
			);
			let mut gzip = GzEncoder::new(File::create(&archive)?, Compression::fast());
			io::copy(&mut File::open(&tar)?, &mut gzip)?;
			gzip.finish()?;
			fs::remove_file(tar)?;
		}
		tracing::info!("validating worker archive and Linux/AMD64 image metadata");
		let verified = inspect_archive(&archive)?;
		upload(&self.presigned_build_url, &archive, verified.size_bytes).await?;
		// This JSON is the command result, not a log. Upload is not finalization.
		println!("{}", serde_json::to_string(&verified)?);
		drop(scratch);
		Ok(())
	}
}

fn path(path: &Path) -> Result<&str> {
	path.to_str().context("Docker paths must be valid UTF-8")
}

async fn docker(args: &[&str]) -> Result<()> {
	let status = Command::new("docker")
		.args(args)
		.stdout(Stdio::from(io::stderr()))
		.stderr(Stdio::inherit())
		.kill_on_drop(true)
		.status()
		.await
		.context("run Docker; install it and start its daemon")?;
	ensure!(
		status.success(),
		"Docker command failed; see Docker output above"
	);
	Ok(())
}

fn validate_url(value: &str) -> Result<()> {
	// Do not attach parsing errors or the input to an error chain.
	ensure!(
		!value.chars().any(char::is_control),
		"invalid presigned upload URL"
	);
	let parsed = Url::parse(value).map_err(|_| anyhow::anyhow!("invalid presigned upload URL"))?;
	let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
	ensure!(
		parsed.scheme() == "https" || (parsed.scheme() == "http" && local),
		"upload URL must use HTTPS (HTTP is only allowed on loopback for local testing)"
	);
	ensure!(
		parsed.host_str().is_some()
			&& parsed.username().is_empty()
			&& parsed.password().is_none()
			&& parsed.fragment().is_none(),
		"upload URL must have a host and no embedded login or fragment"
	);
	Ok(())
}

fn entry_name(path: &Path) -> Result<String> {
	ensure!(
		path.components()
			.all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
		"archive contains an unsafe path"
	);
	Ok(path
		.components()
		.filter_map(|c| match c {
			Component::Normal(s) => Some(s.to_string_lossy()),
			_ => None,
		})
		.collect::<Vec<_>>()
		.join("/"))
}

fn read_json<R: Read>(reader: &mut R, size: u64) -> Result<Vec<u8>> {
	ensure!(size <= MAX_JSON, "archive image metadata exceeds 1 MiB");
	let mut data = Vec::new();
	reader.take(MAX_JSON + 1).read_to_end(&mut data)?;
	ensure!(data.len() as u64 == size, "truncated archive metadata");
	Ok(data)
}

fn inspect_archive(path: &Path) -> Result<Uploaded> {
	let size = fs::metadata(path)?.len();
	ensure!(
		size > 0 && size <= MAX_ARCHIVE,
		"archive must be nonempty and no larger than 5 GiB"
	);
	let mut hash = Sha256::new();
	io::copy(&mut File::open(path)?, &mut hash)?;
	let mut archive =
		tar::Archive::new(MultiGzDecoder::new(File::open(path)?).take(MAX_EXPANDED + 1));
	let mut manifest = None;
	let mut files = HashSet::new();
	for entry in archive
		.entries()
		.context("expected gzip-compressed docker-save archive")?
	{
		let mut entry = entry.context("invalid or truncated Docker archive")?;
		let name = entry_name(&entry.path()?)?;
		if entry.header().entry_type().is_dir() {
			continue;
		}
		ensure!(
			entry.header().entry_type().is_file(),
			"Docker archive contains a non-regular file"
		);
		ensure!(
			files.len() < 100_000 && files.insert(name.clone()),
			"archive contains duplicate paths or too many entries"
		);
		if name == "manifest.json" {
			let size = entry.size();
			let parsed: Vec<Manifest> = serde_json::from_slice(&read_json(&mut entry, size)?)
				.context("invalid Docker manifest.json")?;
			ensure!(
				parsed.len() == 1,
				"archive must contain exactly one image; export one linux/amd64 image with docker save"
			);
			manifest = parsed.into_iter().next();
		}
	}
	let mut decoder = archive.into_inner();
	io::copy(&mut decoder, &mut io::sink())
		.context("invalid gzip checksum or truncated archive")?;
	let expanded = MAX_EXPANDED + 1 - decoder.limit();
	ensure!(
		expanded <= MAX_EXPANDED,
		"archive exceeds 32 GiB uncompressed"
	);
	let manifest = manifest.context(
		"missing Docker manifest.json; use docker save, not docker export or an OCI-only archive",
	)?;
	ensure!(
		files.contains(&manifest.config) && manifest.layers.iter().all(|p| files.contains(p)),
		"Docker archive is missing its config or layers"
	);
	let mut archive =
		tar::Archive::new(MultiGzDecoder::new(File::open(path)?).take(MAX_EXPANDED + 1));
	let mut config = None;
	let mut layer_hashes = HashMap::new();
	for entry in archive.entries()? {
		let mut entry = entry?;
		let name = entry_name(&entry.path()?)?;
		if name == manifest.config {
			let size = entry.size();
			config = Some(read_json(&mut entry, size)?);
		} else if manifest.layers.contains(&name) {
			let mut hash = Sha256::new();
			io::copy(&mut entry, &mut hash)?;
			layer_hashes.insert(name, format!("sha256:{}", hex::encode(hash.finalize())));
		}
	}
	let config = config.context("missing image config")?;
	let image: ImageConfig = serde_json::from_slice(&config).context("invalid image config")?;
	ensure!(
		image.os == "linux" && image.architecture == "amd64",
		"worker image must be linux/amd64; rebuild with --platform linux/amd64"
	);
	ensure!(
		image.rootfs.diff_ids.len() == manifest.layers.len(),
		"image config and layer count disagree"
	);
	for (layer, expected) in manifest.layers.iter().zip(&image.rootfs.diff_ids) {
		ensure!(
			layer_hashes.get(layer) == Some(expected),
			"image layer checksum does not match config"
		);
	}
	Ok(Uploaded {
		uploaded: true,
		architecture: "amd64",
		image_id: format!("sha256:{}", hex::encode(Sha256::digest(&config))),
		sha256: hex::encode(hash.finalize()),
		size_bytes: size,
		uncompressed_size_bytes: expanded,
	})
}

async fn upload(url: &str, archive: &Path, size: u64) -> Result<()> {
	upload_with_progress(url, archive, size, |line| eprintln!("{line}")).await
}

fn upload_progress(bytes: u64, size: u64, recent_bytes: u64, elapsed: Duration) -> String {
	const MIB: f64 = 1024.0 * 1024.0;
	let number = |value: f64| {
		let formatted = format!("{value:.1}");
		formatted
			.strip_suffix(".0")
			.unwrap_or(&formatted)
			.to_owned()
	};
	let bytes = bytes.min(size);
	let percent = bytes.saturating_mul(100).checked_div(size).unwrap_or(0);
	let amount = format!(
		"{}/{} MiB",
		number(bytes as f64 / MIB),
		number(size as f64 / MIB)
	);
	if bytes == size {
		return format!("Uploading {percent}% - {amount} - waiting for confirmation");
	}
	let speed = if elapsed.is_zero() {
		0.0
	} else {
		recent_bytes as f64 / elapsed.as_secs_f64()
	};
	let eta = if speed > 0.0 {
		format!("{:.0}s remaining", ((size - bytes) as f64 / speed).ceil())
	} else {
		"ETA unknown".to_owned()
	};
	format!(
		"Uploading {percent}% - {amount} - {} MiB/s - {eta}",
		number(speed / MIB)
	)
}

async fn upload_with_progress(
	url: &str,
	archive: &Path,
	size: u64,
	mut report: impl FnMut(&str),
) -> Result<()> {
	validate_url(url)?;
	// This standalone CLI deliberately does not depend on Engine's HTTP pools.
	// Inherit the workspace's native + bundled TLS roots, but never follow redirects.
	// Use a total deadline, not a short response-read timeout: R2 may send no
	// response headers until a large upload has finished transmitting.
	let client = reqwest::Client::builder()
		.redirect(reqwest::redirect::Policy::none())
		.connect_timeout(Duration::from_secs(15))
		.timeout(Duration::from_secs(1800))
		.build()
		.map_err(|_| {
			anyhow::anyhow!("initialize upload HTTP client; check TLS/proxy configuration")
		})?;
	for attempt in 1..=3 {
		tracing::info!(attempt, size_bytes = size, "uploading worker archive");
		// Reopen the private snapshot on every attempt so retries start at byte zero.
		// ReaderStream bounds memory instead of buffering the whole archive.
		let file = tokio::fs::File::open(archive)
			.await
			.context("open upload archive")?;
		let streamed = Arc::new(AtomicU64::new(0));
		let progress = streamed.clone();
		let body = ReaderStream::new(file).inspect_ok(move |chunk| {
			progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
		});
		let request = client
			.put(url)
			.header(reqwest::header::CONTENT_TYPE, "application/gzip")
			.header(reqwest::header::CONTENT_LENGTH, size)
			.body(reqwest::Body::wrap_stream(body))
			.send();
		tokio::pin!(request);
		let period = Duration::from_secs(2);
		let mut last_tick = tokio::time::Instant::now();
		let mut last_bytes = 0;
		let mut ticks = tokio::time::interval_at(last_tick + period, period);
		ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
		let response = loop {
			tokio::select! {
				biased;
				response = &mut request => break response,
				_ = ticks.tick() => {
					// This measures bytes consumed by the HTTP client, not storage acknowledgments.
					let bytes = streamed.load(Ordering::Relaxed);
					let now = tokio::time::Instant::now();
					report(&upload_progress(bytes, size, bytes.saturating_sub(last_bytes), now - last_tick));
					last_tick = now;
					last_bytes = bytes;
				}
			}
		};
		// Never retain or format reqwest errors: their URL and nested sources can
		// contain credentials. Response bodies and redirect locations are also private.
		let (status, transport_error, retry_transport) = match response {
			Ok(response) => (response.status().as_u16(), None, false),
			Err(error) => {
				let reason = if error.is_timeout() {
					"request timed out"
				} else if error.is_connect() {
					"connection failed; check network, TLS and proxy configuration"
				} else if error.is_body() {
					"could not stream the local archive"
				} else {
					"HTTP request failed"
				};
				let retry = !error.is_body()
					&& !error.is_builder()
					&& (error.is_timeout() || error.is_connect() || error.is_request());
				(0, Some(reason), retry)
			}
		};
		if (200..300).contains(&status) {
			tracing::info!(
				size_bytes = size,
				"archive uploaded; finalization and deployment are managed separately"
			);
			return Ok(());
		}
		if matches!(status, 401 | 403) {
			bail!(
				"upload authorization was rejected (HTTP {status}); the URL may have expired or its signed headers may not match the archive; obtain a matching URL from the UI"
			);
		}
		ensure!(
			!(300..400).contains(&status),
			"upload returned HTTP {status}; redirects are not followed; obtain a direct presigned PUT URL"
		);
		let retry = matches!(status, 408 | 429 | 500 | 502 | 503 | 504) || retry_transport;
		if !retry || attempt == 3 {
			if let Some(reason) = transport_error {
				bail!("upload failed: {reason}; no finalization was performed");
			}
			bail!(
				"upload failed (HTTP {status}); verify the presigned URL and required headers; no finalization was performed"
			);
		}
		tracing::warn!(
			attempt,
			status,
			"temporary upload failure; retrying the same bytes"
		);
		tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
	}
	unreachable!()
}

// Source-owned shim keeps transport and archive internals private.
#[cfg(test)]
#[path = "../../../tests/upload_presigned.rs"]
mod tests;
