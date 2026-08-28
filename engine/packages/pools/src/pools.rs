use std::sync::Arc;

use anyhow::*;
use rivet_config::Config;
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::{ClickHousePool, Error, NodeId, UdbPool, UpsPool};

// TODO: Automatically shutdown all pools on drop
pub(crate) struct PoolsInner {
	pub(crate) config: Config,
	pub(crate) node_id: NodeId,
	pub(crate) _guard: DropGuard,
	pub(crate) ups: Option<UpsPool>,
	pub(crate) clickhouse: Option<clickhouse::Client>,
	pub(crate) udb: Option<UdbPool>,
}

#[derive(Clone)]
pub struct Pools(Arc<PoolsInner>);

impl Pools {
	#[tracing::instrument(skip(config))]
	pub async fn new(config: Config) -> Result<Pools> {
		// TODO: Choose client name for this service
		let client_name = "rivet";
		let token = CancellationToken::new();
		let node_id = NodeId::new();

		install_rustls_provider();

		let (ups, udb) = tokio::try_join!(
			crate::db::ups::setup(&config, client_name),
			crate::db::udb::setup(&config),
		)?;
		let clickhouse = crate::db::clickhouse::setup(&config)?;

		let pool = Pools(Arc::new(PoolsInner {
			config,
			node_id,
			_guard: token.clone().drop_guard(),
			ups: Some(ups),
			clickhouse,
			udb,
		}));

		// Initialize here to avoid cold starts elsewhere
		crate::reqwest::client().await?;
		crate::reqwest::guarded_client(pool.config()).await?;
		crate::reqwest::guarded_client_no_timeout(pool.config()).await?;

		Ok(pool)
	}

	// Only for tests
	#[tracing::instrument(skip(config))]
	pub async fn test(config: Config) -> Result<Pools> {
		// TODO: Choose client name for this service
		let client_name = "rivet";
		let token = CancellationToken::new();
		let node_id = NodeId::new();

		install_rustls_provider();

		let (ups, udb) = tokio::try_join!(
			crate::db::ups::setup(&config, client_name),
			crate::db::udb::setup(&config),
		)?;

		let pool = Pools(Arc::new(PoolsInner {
			config,
			node_id,
			_guard: token.clone().drop_guard(),
			ups: Some(ups),
			clickhouse: None,
			udb,
		}));

		Ok(pool)
	}

	// MARK: Getters
	pub fn config(&self) -> &Config {
		&self.0.config
	}

	pub fn node_id(&self) -> NodeId {
		self.0.node_id
	}

	pub fn ups_option(&self) -> Option<&UpsPool> {
		self.0.ups.as_ref()
	}

	// MARK: Pool lookups
	pub fn ups(&self) -> Result<UpsPool> {
		self.0.ups.clone().ok_or(Error::MissingUpsPool.into())
	}

	pub fn clickhouse(&self) -> Result<ClickHousePool> {
		self.0.clickhouse.clone().context("missing clickhouse pool")
	}

	pub fn clickhouse_option(&self) -> Option<&ClickHousePool> {
		self.0.clickhouse.as_ref()
	}

	pub fn udb(&self) -> Result<UdbPool> {
		self.0.udb.clone().ok_or(Error::MissingUdbPool.into())
	}
}

fn install_rustls_provider() {
	let provider = rustls::crypto::ring::default_provider();
	if provider.install_default().is_err() {
		tracing::debug!("crypto provider already installed in this process");
	}
}
