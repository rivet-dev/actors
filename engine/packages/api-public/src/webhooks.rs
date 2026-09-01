use std::collections::HashMap;

use anyhow::{Context, Result};
use axum::response::{IntoResponse, Response};
use rivet_api_builder::{
	ApiError,
	extract::{Extension, Json, Path, Query},
};
use rivet_api_types::pagination::Pagination;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::ctx::ApiCtx;

// Config for a single webhook, keyed by an arbitrary name within a namespace. `subscriptions`
// names the event types to deliver; only webhook-safe types are accepted (see
// `webhook::types::WebhookEventType`).
#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
	pub url: String,
	#[serde(default)]
	pub headers: HashMap<String, String>,
	#[serde(default)]
	pub subscriptions: Vec<WebhookEventType>,
}

// Mirrors `webhook::types::WebhookEventType` for the public API. Only webhook-safe variants are
// exposed; high-throughput event types are not subscribable and so have no API representation.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, ToSchema)]
pub enum WebhookEventType {
	#[serde(rename = "runner_pool.error")]
	RunnerPoolError,
	#[serde(rename = "runner_pool.healthy")]
	RunnerPoolHealthy,
}

impl From<WebhookEventType> for webhook::types::WebhookEventType {
	fn from(value: WebhookEventType) -> Self {
		match value {
			WebhookEventType::RunnerPoolError => webhook::types::WebhookEventType::RunnerPoolError,
			WebhookEventType::RunnerPoolHealthy => {
				webhook::types::WebhookEventType::RunnerPoolHealthy
			}
		}
	}
}

impl WebhookEventType {
	// `None` for event types that have no API representation because they cannot be subscribed
	// to. Upsert validation rejects those, so a stored config should never contain one.
	fn from_internal(value: webhook::types::WebhookEventType) -> Option<Self> {
		match value {
			webhook::types::WebhookEventType::RunnerPoolError => {
				Some(WebhookEventType::RunnerPoolError)
			}
			webhook::types::WebhookEventType::RunnerPoolHealthy => {
				Some(WebhookEventType::RunnerPoolHealthy)
			}
			webhook::types::WebhookEventType::ActorHttpRequest => None,
		}
	}
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
						subscriptions: w
							.config
							.subscriptions
							.into_iter()
							.filter_map(WebhookEventType::from_internal)
							.collect(),
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
			subscriptions: body.0.subscriptions.into_iter().map(Into::into).collect(),
		},
	})
	.await?;

	// The config is durable in epoxy and the webhook workflow is dispatched or signaled by
	// the op above. Delivering triggered events over HTTP still needs to be built (see webhook
	// spec).

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

	Ok(DeleteResponse {})
}

// MARK: Retry delivery

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RetryDeliveryPath {
	pub webhook_name: String,
	pub delivery_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct RetryDeliveryQuery {
	pub namespace: String,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = WebhooksRetryDeliveryResponse)]
pub struct RetryDeliveryResponse {}

#[utoipa::path(
	post,
	operation_id = "webhooks_retry_delivery",
	path = "/webhooks/{webhook_name}/deliveries/{delivery_id}/retry",
	params(
		("webhook_name" = String, Path),
		("delivery_id" = String, Path),
		RetryDeliveryQuery,
	),
	responses(
		(status = 200, body = RetryDeliveryResponse),
	),
	security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all)]
pub async fn retry_delivery(
	Extension(ctx): Extension<ApiCtx>,
	Path(path): Path<RetryDeliveryPath>,
	Query(query): Query<RetryDeliveryQuery>,
) -> Response {
	match retry_delivery_inner(ctx, path, query).await {
		Ok(response) => Json(response).into_response(),
		Err(err) => ApiError::from(err).into_response(),
	}
}

#[tracing::instrument(skip_all)]
async fn retry_delivery_inner(
	ctx: ApiCtx,
	path: RetryDeliveryPath,
	query: RetryDeliveryQuery,
) -> Result<RetryDeliveryResponse> {
	ctx.auth().await?;

	let namespace = ctx
		.op(namespace::ops::resolve_for_name_global::Input {
			name: query.namespace.clone(),
		})
		.await?
		.ok_or_else(|| namespace::errors::Namespace::NotFound.build())?;

	ctx.op(webhook::ops::retry::Input {
		namespace_id: namespace.namespace_id,
		name: path.webhook_name.clone(),
		delivery_id: path.delivery_id.clone(),
	})
	.await?;

	Ok(RetryDeliveryResponse {})
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

// One delivery in a webhook's event history. `id` is the delivery id, which is also the
// CloudEvents `id` sent to the destination and what the retry endpoint takes.
#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookEvent {
	pub id: String,
	pub create_ts: i64,
	pub status: String,
	pub event_type: String,
	pub attempt_count: u32,
	pub last_error: Option<String>,
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

const DEFAULT_EVENTS_LIMIT: usize = 20;

#[tracing::instrument(skip_all)]
async fn events_inner(ctx: ApiCtx, path: EventsPath, query: EventsQuery) -> Result<EventsResponse> {
	ctx.auth().await?;

	let namespace = ctx
		.op(namespace::ops::resolve_for_name_global::Input {
			name: query.namespace.clone(),
		})
		.await?
		.ok_or_else(|| namespace::errors::Namespace::NotFound.build())?;

	let mut deliveries = ctx
		.op(webhook::ops::list_deliveries::Input {
			namespace_id: namespace.namespace_id,
			name: path.webhook_name.clone(),
		})
		.await?;

	// Most recent first. `delivery_id` breaks ties deterministically since two deliveries can
	// share a `created_at` millisecond.
	deliveries.sort_by(|a, b| {
		b.record
			.created_at
			.cmp(&a.record.created_at)
			.then_with(|| b.delivery_id.cmp(&a.delivery_id))
	});

	// The cursor is the `(created_at, delivery_id)` of the last item on the previous page;
	// resume strictly after it in the same sorted order.
	if let Some(cursor) = query.cursor {
		let (created_at, delivery_id) = cursor
			.split_once(':')
			.and_then(|(ts, id)| ts.parse::<i64>().ok().map(|ts| (ts, id.to_string())))
			.context("invalid cursor")?;

		deliveries.retain(|d| {
			(d.record.created_at, d.delivery_id.as_str()) < (created_at, delivery_id.as_str())
		});
	}

	let limit = query.limit.unwrap_or(DEFAULT_EVENTS_LIMIT);
	let has_more = deliveries.len() > limit;
	deliveries.truncate(limit);

	let cursor = has_more
		.then(|| deliveries.last())
		.flatten()
		.map(|last| format!("{}:{}", last.record.created_at, last.delivery_id));

	Ok(EventsResponse {
		events: deliveries
			.into_iter()
			.map(|d| WebhookEvent {
				id: d.delivery_id,
				create_ts: d.record.created_at,
				status: match d.record.status {
					webhook::types::DeliveryStatus::Pending => "pending".to_string(),
					webhook::types::DeliveryStatus::Succeeded => "succeeded".to_string(),
					webhook::types::DeliveryStatus::Failed => "failed".to_string(),
				},
				event_type: d.record.event_type.as_str().to_string(),
				attempt_count: d.record.attempt_count,
				last_error: d.record.last_error,
			})
			.collect(),
		pagination: Pagination { cursor },
	})
}
