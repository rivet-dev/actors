use anyhow::*;
use std::{ops::Deref, sync::Arc};
use universalpubsub::PubSub;

#[derive(Clone)]
pub struct SharedState(Arc<SharedStateInner>);

impl SharedState {
	pub fn new(config: &rivet_config::Config, pubsub: PubSub) -> SharedState {
		SharedState(Arc::new(SharedStateInner {
			pegboard_gateway: pegboard_gateway::shared_state::SharedState::new(
				config,
				pubsub.clone(),
			),
			pegboard_gateway2: pegboard_gateway2::shared_state::SharedState::new(
				config,
				pubsub.clone(),
			),
			pegboard_gateway3: pegboard_gateway3::shared_state::SharedState::new(config, pubsub),
		}))
	}

	pub async fn start(&self) -> Result<()> {
		tokio::try_join!(
			self.pegboard_gateway.start(),
			self.pegboard_gateway2.start(),
			self.pegboard_gateway3.start(),
		)?;

		Ok(())
	}
}

impl Deref for SharedState {
	type Target = SharedStateInner;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

pub struct SharedStateInner {
	pub pegboard_gateway: pegboard_gateway::shared_state::SharedState,
	pub pegboard_gateway2: pegboard_gateway2::shared_state::SharedState,
	pub pegboard_gateway3: pegboard_gateway3::shared_state::SharedState,
}
