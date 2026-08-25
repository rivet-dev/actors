use std::collections::HashMap;

use anyhow::{Result, bail};
use axum::response::{IntoResponse, Response};
use rivet_api_builder::{
	ApiError,
	extract::{Extension, Json, Path, Query},
};
use rivet_api_types::pagination::Pagination;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::ctx::ApiCtx;

// Config for a single webhook, keyed by an arbitrary name within a namespace. Fields are
// provisional pending the CloudEvents-shaped trigger payload design (see webhook spec).
#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
	pub url: String,
	#[serde(default)]
	pub headers: HashMap<String, String>,
}

// MARK: List

#[derive(Debug, Deserialize, Serialize, Clone, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
	pub namespace: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhooksListResponse)]
pub struct ListResponse {
	pub webhooks: HashMap<String, WebhookConfig>,
	pub pagination: Pagination,
}

#[utoipa::path(
	get,
	operation_id = "webhooks_list",
	path = "/webhooks",
	params(ListQuery),
	responses(
		(status = 200, body = ListResponse),
	),
	security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all)]
pub async fn list(Extension(ctx): Extension<ApiCtx>, Query(query): Query<ListQuery>) -> Response {
	match list_inner(ctx, query).await {
		Ok(response) => Json(response).into_response(),
		Err(err) => ApiError::from(err).into_response(),
	}
}

#[tracing::instrument(skip_all)]
async fn list_inner(ctx: ApiCtx, query: ListQuery) -> Result<ListResponse> {
	ctx.auth().await?;

	let namespace = ctx
		.op(namespace::ops::resolve_for_name_global::Input {
			name: query.namespace.clone(),
		})
		.await?
		.ok_or_else(|| namespace::errors::Namespace::NotFound.build())?;

	let webhooks = ctx
		.op(webhook::ops::list::Input {
			namespace_id: namespace.namespace_id,
		})
		.await?;

	Ok(ListResponse {
		webhooks: webhooks
			.into_iter()
			.map(|w| {
				(
					w.name,
					WebhookConfig {
						url: w.config.url,
						headers: w.config.headers,
					},
				)
			})
			.collect(),
		pagination: Pagination { cursor: None },
	})
}

// MARK: Upsert

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct UpsertPath {
	pub webhook_name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct UpsertQuery {
	pub namespace: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhooksUpsertRequestBody)]
pub struct UpsertRequest(pub WebhookConfig);

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhooksUpsertResponse)]
pub struct UpsertResponse {}

#[utoipa::path(
	put,
	operation_id = "webhooks_upsert",
	path = "/webhooks/{webhook_name}",
	params(
		("webhook_name" = String, Path),
		UpsertQuery,
	),
	request_body(content = UpsertRequest, content_type = "application/json"),
	responses(
		(status = 200, body = UpsertResponse),
	),
	security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all)]
pub async fn upsert(
	Extension(ctx): Extension<ApiCtx>,
	Path(path): Path<UpsertPath>,
	Query(query): Query<UpsertQuery>,
	Json(body): Json<UpsertRequest>,
) -> Response {
	match upsert_inner(ctx, path, query, body).await {
		Ok(response) => Json(response).into_response(),
		Err(err) => ApiError::from(err).into_response(),
	}
}

#[tracing::instrument(skip_all)]
async fn upsert_inner(
	ctx: ApiCtx,
	path: UpsertPath,
	query: UpsertQuery,
	body: UpsertRequest,
) -> Result<UpsertResponse> {
	ctx.auth().await?;

	// Resolve and validate namespace
	let namespace = ctx
		.op(namespace::ops::resolve_for_name_global::Input {
			name: query.namespace.clone(),
		})
		.await?
		.ok_or_else(|| namespace::errors::Namespace::NotFound.build())?;

	// Upsert operation
	ctx.op(webhook::ops::upsert::Input {
		namespace_id: namespace.namespace_id,
		name: path.webhook_name.clone(),
		config: webhook::types::WebhookConfig {
			url: body.0.url,
			headers: body.0.headers,
		},
	})
	.await?;

	// TODO: Spawn or signal the per-(namespace, webhook name, dc) webhook workflow
	// (see webhook spec). The config is now durable in epoxy; the workflow still
	// needs to be built to actually deliver events.

	Ok(UpsertResponse {})
}

// MARK: Delete

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct DeletePath {
	pub webhook_name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct DeleteQuery {
	pub namespace: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhooksDeleteResponse)]
pub struct DeleteResponse {}

#[utoipa::path(
	delete,
	operation_id = "webhooks_delete",
	path = "/webhooks/{webhook_name}",
	params(
		("webhook_name" = String, Path),
		DeleteQuery,
	),
	responses(
		(status = 200, body = DeleteResponse),
	),
	security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all)]
pub async fn delete(
	Extension(ctx): Extension<ApiCtx>,
	Path(path): Path<DeletePath>,
	Query(query): Query<DeleteQuery>,
) -> Response {
	match delete_inner(ctx, path, query).await {
		Ok(response) => Json(response).into_response(),
		Err(err) => ApiError::from(err).into_response(),
	}
}

#[tracing::instrument(skip_all)]
async fn delete_inner(ctx: ApiCtx, path: DeletePath, query: DeleteQuery) -> Result<DeleteResponse> {
	ctx.auth().await?;

	let namespace = ctx
		.op(namespace::ops::resolve_for_name_global::Input {
			name: query.namespace.clone(),
		})
		.await?
		.ok_or_else(|| namespace::errors::Namespace::NotFound.build())?;

	ctx.op(webhook::ops::delete::Input {
		namespace_id: namespace.namespace_id,
		name: path.webhook_name.clone(),
	})
	.await?;

	// TODO: Let the webhook workflow auto-exit once it observes the config is
	// gone (see webhook spec).

	Ok(DeleteResponse {})
}

// MARK: Events

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct EventsPath {
	pub webhook_name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
	pub namespace: String,
	pub limit: Option<usize>,
	pub cursor: Option<String>,
}

// Placeholder shape for one delivery attempt in a webhook's event history.
#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookEvent {
	pub id: String,
	pub create_ts: i64,
	pub status: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhooksEventsResponse)]
pub struct EventsResponse {
	pub events: Vec<WebhookEvent>,
	pub pagination: Pagination,
}

#[utoipa::path(
	get,
	operation_id = "webhooks_events",
	path = "/webhooks/{webhook_name}/events",
	params(
		("webhook_name" = String, Path),
		EventsQuery,
	),
	responses(
		(status = 200, body = EventsResponse),
	),
	security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all)]
pub async fn events(
	Extension(ctx): Extension<ApiCtx>,
	Path(path): Path<EventsPath>,
	Query(query): Query<EventsQuery>,
) -> Response {
	match events_inner(ctx, path, query).await {
		Ok(response) => Json(response).into_response(),
		Err(err) => ApiError::from(err).into_response(),
	}
}

#[tracing::instrument(skip_all)]
async fn events_inner(
	ctx: ApiCtx,
	_path: EventsPath,
	_query: EventsQuery,
) -> Result<EventsResponse> {
	ctx.auth().await?;

	// TODO: Read the webhook workflow's event history for this (namespace,
	// webhook name, dc), paginated (see webhook spec).
	bail!("webhooks_events is not implemented yet");
}
