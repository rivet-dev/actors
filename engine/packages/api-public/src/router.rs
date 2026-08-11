use axum::{
	extract::Request,
	middleware::{self, Next},
	response::{IntoResponse, Redirect, Response},
};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use rivet_api_builder::{create_router, extract::FailedExtraction};
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;

use crate::{
	actors, ctx, datacenters, envoys, health, metadata, namespaces, runner_configs, runners, ui,
};

#[derive(OpenApi)]
#[openapi(
	paths(
		actors::list::list,
		actors::create::create,
		actors::delete::delete,
		actors::list_names::list_names,
		actors::get_or_create::get_or_create,
		actors::kv_get::kv_get,
		actors::sleep::sleep,
		actors::reschedule::reschedule,
		runners::list,
		runners::list_names,
		envoys::list,
		namespaces::list,
		namespaces::create,
		runner_configs::list::list,
		runner_configs::upsert::upsert,
		runner_configs::delete::delete,
		runner_configs::serverless_health_check::serverless_health_check,
		runner_configs::refresh_metadata::refresh_metadata,
		datacenters::list,
		health::fanout,
		metadata::get,
	),
	components(
		schemas(rivet_types::keys::namespace::runner_config::RunnerConfigVariant)
	),
	security( ("bearer_auth" = []) ),
	modifiers(&SecurityAddon),
)]
pub struct ApiDoc;

#[tracing::instrument(skip_all)]
pub async fn router(
	config: rivet_config::Config,
	pools: rivet_pools::Pools,
) -> anyhow::Result<axum::Router> {
	tracing::debug!("creating api-public router");

	create_router("api-public", config, pools, |router| {
		router
			// Root redirect
			.route(
				"/",
				axum::routing::get(|| async { Redirect::permanent("/ui/") }),
			)
			// MARK: Metadata
			.route("/metadata", axum::routing::get(metadata::get))
			// MARK: Namespaces
			.route("/namespaces", axum::routing::get(namespaces::list))
			.route("/namespaces", axum::routing::post(namespaces::create))
			.route("/runner-configs", axum::routing::get(runner_configs::list))
			.route(
				"/runner-configs/serverless-health-check",
				axum::routing::post(runner_configs::serverless_health_check),
			)
			.route(
				"/runner-configs/{runner_name}",
				axum::routing::put(runner_configs::upsert),
			)
			.route(
				"/runner-configs/{runner_name}",
				axum::routing::delete(runner_configs::delete),
			)
			.route(
				"/runner-configs/{runner_name}/refresh-metadata",
				axum::routing::post(runner_configs::refresh_metadata),
			)
			// MARK: Actors
			.route("/actors", axum::routing::get(actors::list::list))
			.route("/actors", axum::routing::post(actors::create::create))
			.route(
				"/actors",
				axum::routing::put(actors::get_or_create::get_or_create),
			)
			.route(
				"/actors/{actor_id}",
				axum::routing::delete(actors::delete::delete),
			)
			.route(
				"/actors/names",
				axum::routing::get(actors::list_names::list_names),
			)
			.route(
				"/actors/{actor_id}/kv/keys/{key}",
				axum::routing::get(actors::kv_get::kv_get),
			)
			.route(
				"/actors/{actor_id}/sleep",
				axum::routing::post(actors::sleep::sleep),
			)
			.route(
				"/actors/{actor_id}/reschedule",
				axum::routing::post(actors::reschedule::reschedule),
			)
			// MARK: Runners
			.route("/runners", axum::routing::get(runners::list))
			// MARK: Envoys
			.route("/envoys", axum::routing::get(envoys::list))
			.route("/runners/names", axum::routing::get(runners::list_names))
			// MARK: Datacenters
			.route("/datacenters", axum::routing::get(datacenters::list))
			// MARK: Health
			.route("/health/fanout", axum::routing::get(health::fanout))
			// MARK: UI
			.route("/ui", axum::routing::get(ui::serve_index))
			.route("/ui/", axum::routing::get(ui::serve_index))
			.route("/ui/{*path}", axum::routing::get(ui::serve_ui))
			.route("/assets/{*path}", axum::routing::get(ui::serve_asset))
			// MARK: Middleware (must go after all routes)
			// Add CORS layer that mirrors the request origin
			.layer(
				CorsLayer::new()
					.allow_origin(tower_http::cors::AllowOrigin::mirror_request())
					.allow_methods(tower_http::cors::AllowMethods::mirror_request())
					.allow_headers(tower_http::cors::AllowHeaders::mirror_request())
					.allow_credentials(true),
			)
			.layer(middleware::from_fn(auth_middleware))
	})
	.await
}

/// Middleware to wrap ApiCtx with auth handling capabilities and to throw an error if auth was not explicitly
// handled in an endpoint
#[tracing::instrument(skip_all)]
async fn auth_middleware(
	headers: HeaderMap,
	mut req: Request,
	next: Next,
) -> std::result::Result<Response, Response> {
	let ctx = req
		.extensions()
		.get::<rivet_api_builder::ApiCtx>()
		.ok_or_else(|| "ctx should exist".into_response())?;

	// Extract token
	let token = headers
		.get(AUTHORIZATION)
		.and_then(|x| x.to_str().ok().and_then(|x| x.strip_prefix("Bearer ")))
		.map(|x| x.to_string());

	// Insert the new ApiCtx into request extensions
	let ctx = ctx::ApiCtx::new(ctx.clone(), token);
	req.extensions_mut().insert(ctx.clone());

	let method = req.method().clone();
	let path = req.uri().path().to_string();

	// Run endpoint
	let res = next.run(req).await;

	// Verify auth was handled
	if res.extensions().get::<FailedExtraction>().is_none()
		&& method != reqwest::Method::OPTIONS
		&& res.status() != reqwest::StatusCode::METHOD_NOT_ALLOWED
		&& path != "/"
		&& path != "/ui"
		&& !path.starts_with("/ui/")
		&& !path.starts_with("/assets/")
		&& !ctx.is_auth_handled()
	{
		return Err((
			reqwest::StatusCode::INTERNAL_SERVER_ERROR,
			format!("developer error: must explicitly handle auth in all endpoints (path: {path})"),
		)
			.into_response());
	}

	Ok(res)
}

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
	fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
		openapi.components.as_mut().unwrap().add_security_scheme(
			"bearer_auth",
			utoipa::openapi::security::SecurityScheme::Http(
				utoipa::openapi::security::HttpBuilder::new()
					.scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
					// .bearer_format("Rivet")
					.build(),
			),
		);
	}
}
