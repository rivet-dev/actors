use anyhow::Result;
use gas::prelude::*;

#[tracing::instrument(skip_all)]
pub async fn start(config: rivet_config::Config, pools: rivet_pools::Pools) -> Result<()> {
	let reg = registry()?;
	let db = db::DatabaseKv::new(config.clone(), pools.clone()).await?;
	let worker = Worker::new(reg.handle(), db, config, pools);

	// Start worker
	worker.start(None).await
}

pub fn registry() -> Result<Registry> {
	pegboard::registry()?
		.merge(namespace::registry()?)?
		.merge(epoxy::registry()?)?
		.merge(gasoline_runtime::registry()?)?
		.merge(datacenter::registry()?)?
		.merge(depot::registry()?)?
		.merge(webhook::registry()?)
		.map_err(Into::into)
}
