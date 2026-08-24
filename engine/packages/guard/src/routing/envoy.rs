use anyhow::Result;
use gas::prelude::*;
use rivet_guard_core::{RoutingOutput, request_context::RequestContext};
use std::sync::Arc;
use subtle::ConstantTimeEq;

use super::{SEC_WEBSOCKET_PROTOCOL, WS_PROTOCOL_TOKEN, X_RIVET_TOKEN, validate_regional_host};

/// Route requests to the envoy service using header-based routing
#[tracing::instrument(skip_all)]
pub async fn route_request(
	ctx: &StandaloneCtx,
	req_ctx: &RequestContext,
	target: &str,
) -> Result<Option<RoutingOutput>> {
	if target != "envoy" {
		return Ok(None);
	}

	tracing::debug!(hostname=%req_ctx.hostname(), path=%req_ctx.path(), "routing to envoy via header");

	route_envoy_internal(ctx, req_ctx).await.map(Some)
}

/// Route requests to the envoy service using path-based routing
/// Matches path: /envoys/connect
#[tracing::instrument(skip_all)]
pub async fn route_request_path_based(
	ctx: &StandaloneCtx,
	req_ctx: &RequestContext,
) -> Result<Option<RoutingOutput>> {
	// Check if path matches /envoys/connect
	let path_without_query = req_ctx.path().split('?').next().unwrap_or(req_ctx.path());
	if path_without_query != "/envoys/connect" && path_without_query != "/envoys/connect/" {
		return Ok(None);
	}

	tracing::debug!(hostname=%req_ctx.hostname(), path=%req_ctx.path(), "routing to envoy via path");

	route_envoy_internal(ctx, req_ctx).await.map(Some)
}

/// Internal envoy routing logic shared by both header-based and path-based routing
#[tracing::instrument(skip_all)]
async fn route_envoy_internal(
	ctx: &StandaloneCtx,
	req_ctx: &RequestContext,
) -> Result<RoutingOutput> {
	validate_regional_host(ctx, req_ctx)?;

	// Check auth (if enabled)
	if let Some(auth) = &ctx.config().auth {
		// Extract token from protocol or header
		let token = if req_ctx.is_websocket() {
			req_ctx
				.headers()
				.get(SEC_WEBSOCKET_PROTOCOL)
				.and_then(|protocols| protocols.to_str().ok())
				.and_then(|protocols| {
					protocols
						.split(',')
						.map(|p| p.trim())
						.find_map(|p| p.strip_prefix(WS_PROTOCOL_TOKEN))
				})
				.ok_or_else(|| {
					crate::errors::MissingHeader {
						header: "`rivet_token.*` protocol in sec-websocket-protocol".to_string(),
					}
					.build()
				})?
		} else {
			req_ctx
				.headers()
				.get(X_RIVET_TOKEN)
				.and_then(|x| x.to_str().ok())
				.ok_or_else(|| {
					crate::errors::MissingHeader {
						header: X_RIVET_TOKEN.to_string(),
					}
					.build()
				})?
		};

		// Validate token
		if token
			.as_bytes()
			.ct_ne(auth.admin_token.read().as_bytes())
			.into()
		{
			return Err(rivet_api_builder::ApiForbidden.build());
		}

		tracing::debug!("authenticated envoy connection");
	}

	let tunnel = pegboard_envoy::PegboardEnvoyWs::new(&ctx);
	Ok(RoutingOutput::CustomServe(Arc::new(tunnel)))
}
