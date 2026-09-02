use std::time::Duration;

use anyhow::{Context, Result};
use gas::prelude::*;
use tracing::Instrument;
use universaldb::utils::IsolationLevel::Serializable;

const RECORD_REQUEST_METRICS_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug)]
pub(super) enum RequestKind {
	Http,
	Websocket,
	HibernatingWebsocket,
}

impl RequestKind {
	fn label(self) -> &'static str {
		match self {
			Self::Http => "http",
			Self::Websocket => "ws",
			Self::HibernatingWebsocket => "hws",
		}
	}
}

#[derive(Clone)]
pub(super) struct RequestMetrics {
	ctx: StandaloneCtx,
	actor_id: Id,
	namespace_id: Id,
	envoy_key: String,
	kind: RequestKind,
}

impl RequestMetrics {
	pub(super) fn new(
		ctx: StandaloneCtx,
		actor_id: Id,
		namespace_id: Id,
		envoy_key: String,
		kind: RequestKind,
	) -> Self {
		Self {
			ctx,
			actor_id,
			namespace_id,
			envoy_key,
			kind,
		}
	}

	pub(super) async fn begin(&self, ingress_bytes: usize) -> ActiveRequestMetrics {
		let active = match self.record(ingress_bytes, 0, 1, 1).await {
			Ok(()) => true,
			Err(err) => {
				tracing::error!(
					?err,
					kind = ?self.kind,
					namespace_id = ?self.namespace_id,
					envoy_key = %self.envoy_key,
					"request start metrics failed",
				);
				false
			}
		};

		ActiveRequestMetrics {
			metrics: self.clone(),
			active,
		}
	}

	pub(super) async fn record_transfer(
		&self,
		ingress_bytes: usize,
		egress_bytes: usize,
	) -> Result<()> {
		self.record(ingress_bytes, egress_bytes, 0, 0).await
	}

	#[tracing::instrument(
		skip_all,
		fields(actor_id = ?self.actor_id, kind = ?self.kind, ingress_bytes, egress_bytes)
	)]
	async fn record(
		&self,
		ingress_bytes: usize,
		egress_bytes: usize,
		active_delta: i64,
		request_delta: i64,
	) -> Result<()> {
		tokio::time::timeout(
			RECORD_REQUEST_METRICS_TIMEOUT,
			self.ctx
				.udb()?
				.txn("gateway_record_req_metrics", |tx| async move {
					let tx = tx.with_subspace(pegboard::keys::subspace());
					let actor_name = tx
						.read(
							&pegboard::keys::actor::NameKey::new(self.actor_id),
							Serializable,
						)
						.await?;

					apply_update(
						&tx,
						self.namespace_id,
						&actor_name,
						self.kind,
						ingress_bytes,
						egress_bytes,
						active_delta,
						request_delta,
					);
					Ok(())
				})
				.instrument(tracing::info_span!("record_req_metrics_tx")),
		)
		.await
		.context("timed out recording request metrics")??;

		Ok(())
	}
}

pub(super) struct ActiveRequestMetrics {
	metrics: RequestMetrics,
	active: bool,
}

impl ActiveRequestMetrics {
	pub(super) fn finish_in_background(self, egress_bytes: usize) {
		tokio::spawn(self.finish(egress_bytes).in_current_span());
	}

	pub(super) async fn finish(self, egress_bytes: usize) {
		if !self.active {
			return;
		}

		if let Err(err) = self.metrics.record(0, egress_bytes, -1, 0).await {
			tracing::error!(
				?err,
				kind = ?self.metrics.kind,
				namespace_id = ?self.metrics.namespace_id,
				envoy_key = %self.metrics.envoy_key,
				"request finish metrics failed",
			);
		}
	}
}

fn apply_update(
	tx: &universaldb::Transaction,
	namespace_id: Id,
	actor_name: &str,
	kind: RequestKind,
	ingress_bytes: usize,
	egress_bytes: usize,
	active_delta: i64,
	request_delta: i64,
) {
	let namespace_tx = tx.with_subspace(namespace::keys::subspace());
	let kind = kind.label().to_owned();

	if ingress_bytes != 0 {
		namespace::keys::metric::inc(
			&namespace_tx,
			namespace_id,
			namespace::keys::metric::Metric::GatewayIngress(actor_name.to_owned(), kind.clone()),
			ingress_bytes.try_into().unwrap_or_default(),
		);
	}
	if egress_bytes != 0 {
		namespace::keys::metric::inc(
			&namespace_tx,
			namespace_id,
			namespace::keys::metric::Metric::GatewayEgress(actor_name.to_owned(), kind.clone()),
			egress_bytes.try_into().unwrap_or_default(),
		);
	}
	if request_delta != 0 {
		namespace::keys::metric::inc(
			&namespace_tx,
			namespace_id,
			namespace::keys::metric::Metric::Requests(actor_name.to_owned(), kind.clone()),
			request_delta,
		);
	}
	if active_delta != 0 {
		namespace::keys::metric::inc(
			&namespace_tx,
			namespace_id,
			namespace::keys::metric::Metric::ActiveRequests(actor_name.to_owned(), kind),
			active_delta,
		);
	}
}
