use rivet_metrics::{BUCKETS, MICRO_BUCKETS, REGISTRY, prometheus::*};

lazy_static::lazy_static! {
	pub static ref OBSERVATION_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"observation_duration",
		"Duration of any code observation.",
		&["location"],
		MICRO_BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();
	pub static ref LONG_OBSERVATION_DURATION: HistogramVec = register_histogram_vec_with_registry!(
		"long_observation_duration",
		"Duration of any long code observation.",
		&["location"],
		BUCKETS.to_vec(),
		*REGISTRY
	).unwrap();

	pub static ref SERIALIZE_SIZE: HistogramVec = register_histogram_vec_with_registry!(
		"serialize_size",
		"Size in bytes for any serialization.",
		&["format", "location"],
		vec![16.0, 32.0, 64.0, 128.0, 256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0, 16777216.0],
		*REGISTRY
	).unwrap();

	pub static ref DESERIALIZE_SIZE: HistogramVec = register_histogram_vec_with_registry!(
		"deserialize_size",
		"Size in bytes for any deserialization.",
		&["format", "location"],
		vec![16.0, 32.0, 64.0, 128.0, 256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0, 16777216.0],
		*REGISTRY
	).unwrap();
}
