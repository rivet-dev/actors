use std::sync::{Arc, atomic::AtomicU64};

use anyhow::Result;
use tokio::sync::watch;

use super::{LifecycleResult, UPDATE_METRICS_INTERVAL};
use crate::request_metrics::RequestMetrics;

#[tracing::instrument(name = "metrics_task", skip_all)]
pub async fn task(
	metrics: RequestMetrics,
	ingress_bytes: Arc<AtomicU64>,
	egress_bytes: Arc<AtomicU64>,
	mut metrics_abort_rx: watch::Receiver<()>,
) -> Result<LifecycleResult> {
	let mut last_ingress_bytes = 0;
	let mut last_egress_bytes = 0;

	loop {
		tokio::select! {
			_ = tokio::time::sleep(UPDATE_METRICS_INTERVAL) => {}
			_ = metrics_abort_rx.changed() => {
				// Record final values before abort
				record_transfer(
					&metrics,
					&ingress_bytes,
					&egress_bytes,
					&mut last_ingress_bytes,
					&mut last_egress_bytes,
				).await?;

				return Ok(LifecycleResult::Aborted);
			}
		}

		record_transfer(
			&metrics,
			&ingress_bytes,
			&egress_bytes,
			&mut last_ingress_bytes,
			&mut last_egress_bytes,
		)
		.await?;
	}
}

async fn record_transfer(
	metrics: &RequestMetrics,
	ingress_bytes: &AtomicU64,
	egress_bytes: &AtomicU64,
	last_ingress_bytes: &mut u64,
	last_egress_bytes: &mut u64,
) -> Result<()> {
	let new_ingress_bytes = ingress_bytes.load(std::sync::atomic::Ordering::Acquire);
	let new_egress_bytes = egress_bytes.load(std::sync::atomic::Ordering::Acquire);
	let ingress_diff = new_ingress_bytes - *last_ingress_bytes;
	let egress_diff = new_egress_bytes - *last_egress_bytes;

	if ingress_diff > 0 || egress_diff > 0 {
		metrics
			.record_transfer(ingress_diff as usize, egress_diff as usize)
			.await?;
	}

	*last_ingress_bytes = new_ingress_bytes;
	*last_egress_bytes = new_egress_bytes;

	Ok(())
}
