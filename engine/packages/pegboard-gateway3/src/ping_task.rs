use anyhow::Result;
use rand::Rng;
use std::time::Duration;
use tokio::sync::watch;

use super::LifecycleResult;
use crate::shared_state::InFlightRequestHandle;

#[tracing::instrument(name = "ping_task", skip_all)]
pub async fn task(
	in_flight_req: InFlightRequestHandle,
	mut ping_abort_rx: watch::Receiver<()>,
	update_ping_interval: Duration,
) -> Result<LifecycleResult> {
	loop {
		tokio::select! {
			_ = tokio::time::sleep(update_ping_interval) => {}
			_ = ping_abort_rx.changed() => {
				return Ok(LifecycleResult::Aborted);
			}
		}

		// Jitter sleep to prevent stampeding herds
		let jitter = { rand::thread_rng().gen_range(0..128) };
		tokio::time::sleep(Duration::from_millis(jitter)).await;

		in_flight_req.send_and_check_ping().await?;
	}
}
