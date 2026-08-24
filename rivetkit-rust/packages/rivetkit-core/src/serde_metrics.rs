//! Duration and size metrics for serialization and deserialization hot paths.
//!
//! These mirror the engine-side serde observability but follow rivetkit's
//! metric conventions: `rivetkit_`-prefixed names registered through a
//! `LazyLock` collector struct, and `crate::time::Instant` so the same code
//! compiles for the wasm runtime.
//!
//! The `format` label is the wire format (`bare`, `json`, `cbor`). The
//! `location` label identifies the call site and must be a bounded, code-defined
//! string, never user input.

use std::sync::LazyLock;
use std::time::Duration;

use rivet_metrics::{
	MICRO_BUCKETS,
	prometheus::{HistogramOpts, HistogramVec, Registry},
};

use crate::time::Instant;

const SERDE_LABELS: &[&str] = &["format", "location"];

/// Byte-size buckets shared by serialize and deserialize size histograms.
fn serde_size_buckets() -> Vec<f64> {
	vec![
		16.0, 32.0, 64.0, 128.0, 256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0,
		4194304.0, 16777216.0,
	]
}

struct SerdeMetricCollectors {
	serialize_size: HistogramVec,
	deserialize_size: HistogramVec,
	serialize_duration_seconds: HistogramVec,
	deserialize_duration_seconds: HistogramVec,
}

static METRICS: LazyLock<SerdeMetricCollectors> = LazyLock::new(SerdeMetricCollectors::new);

impl SerdeMetricCollectors {
	fn new() -> Self {
		let serialize_size = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_serialize_size",
				"size in bytes for any serialization",
			)
			.buckets(serde_size_buckets()),
			SERDE_LABELS,
		)
		.expect("create rivetkit_serialize_size histogram");
		let deserialize_size = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_deserialize_size",
				"size in bytes for any deserialization",
			)
			.buckets(serde_size_buckets()),
			SERDE_LABELS,
		)
		.expect("create rivetkit_deserialize_size histogram");
		let serialize_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_serialize_duration_seconds",
				"duration in seconds for any serialization",
			)
			.buckets(MICRO_BUCKETS.to_vec()),
			SERDE_LABELS,
		)
		.expect("create rivetkit_serialize_duration_seconds histogram");
		let deserialize_duration_seconds = HistogramVec::new(
			HistogramOpts::new(
				"rivetkit_deserialize_duration_seconds",
				"duration in seconds for any deserialization",
			)
			.buckets(MICRO_BUCKETS.to_vec()),
			SERDE_LABELS,
		)
		.expect("create rivetkit_deserialize_duration_seconds histogram");

		register_metric(&rivet_metrics::REGISTRY, serialize_size.clone());
		register_metric(&rivet_metrics::REGISTRY, deserialize_size.clone());
		register_metric(&rivet_metrics::REGISTRY, serialize_duration_seconds.clone());
		register_metric(
			&rivet_metrics::REGISTRY,
			deserialize_duration_seconds.clone(),
		);

		Self {
			serialize_size,
			deserialize_size,
			serialize_duration_seconds,
			deserialize_duration_seconds,
		}
	}
}

/// Records the duration and output size of a serialization producing `Vec<u8>`.
///
/// The size is only recorded when the closure succeeds.
pub(crate) fn measure_serialize(
	format: &str,
	location: &str,
	f: impl FnOnce() -> anyhow::Result<Vec<u8>>,
) -> anyhow::Result<Vec<u8>> {
	let started = Instant::now();
	let result = f();
	observe(
		&METRICS.serialize_duration_seconds,
		format,
		location,
		started.elapsed(),
	);
	if let Ok(bytes) = &result {
		observe_size(&METRICS.serialize_size, format, location, bytes.len());
	}
	result
}

/// Records the duration and input size of a deserialization.
///
/// The input size is recorded unconditionally because the bytes are available
/// regardless of whether decoding succeeds.
pub(crate) fn measure_deserialize<T>(
	format: &str,
	location: &str,
	input_len: usize,
	f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
	observe_size(&METRICS.deserialize_size, format, location, input_len);
	let started = Instant::now();
	let result = f();
	observe(
		&METRICS.deserialize_duration_seconds,
		format,
		location,
		started.elapsed(),
	);
	result
}

fn observe(metric: &HistogramVec, format: &str, location: &str, elapsed: Duration) {
	metric
		.with_label_values(&[format, location])
		.observe(elapsed.as_secs_f64());
}

fn observe_size(metric: &HistogramVec, format: &str, location: &str, size: usize) {
	metric
		.with_label_values(&[format, location])
		.observe(size as f64);
}

fn register_metric<M>(registry: &Registry, metric: M)
where
	M: rivet_metrics::prometheus::core::Collector + Clone + Send + Sync + 'static,
{
	if let Err(error) = registry.register(Box::new(metric)) {
		tracing::warn!(
			?error,
			"serde metric registration failed, using existing collector"
		);
	}
}
