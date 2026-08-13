use anyhow::{Context, Result};
use vbare::OwnedVersionedData;

use crate::serde_metrics;

pub(crate) fn encode_latest_with_embedded_version<T>(
	latest: T::Latest,
	version: u16,
	label: &str,
) -> Result<Vec<u8>>
where
	T: OwnedVersionedData,
{
	serde_metrics::measure_serialize("bare", label, || {
		T::wrap_latest(latest)
			.serialize_with_embedded_version(version)
			.with_context(|| format!("encode {label} versioned bare payload"))
	})
}

pub(crate) fn decode_latest_with_embedded_version<T>(
	payload: &[u8],
	label: &str,
) -> Result<T::Latest>
where
	T: OwnedVersionedData,
{
	serde_metrics::measure_deserialize("bare", label, payload.len(), || {
		<T as OwnedVersionedData>::deserialize_with_embedded_version(payload)
			.with_context(|| format!("decode {label} versioned bare payload"))
	})
}
