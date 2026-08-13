use anyhow::{Result, ensure};
use axum::body::Bytes;
use epoxy_protocol::{protocol, versioned};
use rivet_api_builder::prelude::*;
use rivet_perf::{perf_finish, perf_start};
use vbare::OwnedVersionedData;

use crate::metrics;

#[derive(Deserialize)]
pub struct ProtocolPath {
	pub version: u16,
}

pub fn mount_routes(
	router: axum::Router<rivet_api_builder::GlobalApiCtx>,
) -> axum::Router<rivet_api_builder::GlobalApiCtx> {
	router
		.route("/v{version}/epoxy/message", bin::post(message))
		.route(
			"/v{version}/epoxy/changelog-read",
			bin::post(changelog_read),
		)
}

pub async fn message(ctx: ApiCtx, path: ProtocolPath, _query: (), body: Bytes) -> Result<Vec<u8>> {
	let request = versioned::Request::deserialize_version(&body, path.version)?.unwrap_latest()?;
	ensure!(
		!matches!(request.kind, protocol::RequestKind::ChangelogReadRequest(_)),
		"use /epoxy/changelog-read for changelog reads"
	);

	handle_request(ctx, request).await
}

pub async fn changelog_read(
	ctx: ApiCtx,
	path: ProtocolPath,
	_query: (),
	body: Bytes,
) -> Result<Vec<u8>> {
	let request = versioned::Request::deserialize_version(&body, path.version)?.unwrap_latest()?;
	ensure!(
		matches!(request.kind, protocol::RequestKind::ChangelogReadRequest(_)),
		"/epoxy/changelog-read only accepts changelog read requests"
	);

	handle_request(ctx, request).await
}

fn request_kind_label(kind: &protocol::RequestKind) -> &'static str {
	match kind {
		protocol::RequestKind::UpdateConfigRequest(_) => "update_config",
		protocol::RequestKind::PrepareRequest(_) => "prepare",
		protocol::RequestKind::PreAcceptRequest(_) => "pre_accept",
		protocol::RequestKind::AcceptRequest(_) => "accept",
		protocol::RequestKind::CommitRequest(_) => "commit",
		protocol::RequestKind::ChangelogReadRequest(_) => "changelog_read",
		protocol::RequestKind::HealthCheckRequest => "health_check",
		protocol::RequestKind::CoordinatorUpdateReplicaStatusRequest(_) => {
			"coordinator_update_replica_status"
		}
		protocol::RequestKind::BeginLearningRequest(_) => "begin_learning",
		protocol::RequestKind::KvGetRequest(_) => "kv_get",
		protocol::RequestKind::KvPurgeCacheRequest(_) => "kv_purge_cache",
	}
}

async fn handle_request(ctx: ApiCtx, request: protocol::Request) -> Result<Vec<u8>> {
	let current_replica_id = ctx.config().epoxy_replica_id();
	ensure!(
		request.to_replica_id == current_replica_id,
		"request intended for replica {} but received by replica {}",
		request.to_replica_id,
		current_replica_id
	);

	let kind_label = request_kind_label(&request.kind);
	let measure = perf_start!(
		&metrics::REQUEST_DURATION,
		slow_ms = 1000,
		"epoxy_request",
		labels: { request_type = %kind_label },
		fields: {
			to_replica_id = %request.to_replica_id,
			current_replica_id = %current_replica_id,
		},
	);
	let res = crate::replica::message_request::message_request(&ctx, request).await;
	let result_label = if res.is_ok() { "ok" } else { "err" };
	metrics::record_request_result(kind_label, result_label);
	perf_finish!(measure, fields: { result = %result_label });

	rivet_util::serde::bare_to_vec!(&res?).map_err(Into::into)
}
