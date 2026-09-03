use hyper::header::HeaderValue;

use super::*;

#[test]
fn retries_guard_actor_ready_timeout_response() {
	let mut headers = hyper::HeaderMap::new();
	headers.insert(
		X_RIVET_ERROR,
		HeaderValue::from_static("guard.actor_ready_timeout"),
	);

	assert!(should_retry_request_inner(
		StatusCode::SERVICE_UNAVAILABLE,
		&headers,
	));
}

#[test]
fn skips_service_unavailable_without_rivet_error_header() {
	let headers = hyper::HeaderMap::new();

	assert!(!should_retry_request_inner(
		StatusCode::SERVICE_UNAVAILABLE,
		&headers,
	));
}

#[test]
fn skips_non_service_unavailable_with_rivet_error_header() {
	let mut headers = hyper::HeaderMap::new();
	headers.insert(X_RIVET_ERROR, HeaderValue::from_static("guard.no_route"));

	assert!(!should_retry_request_inner(StatusCode::NOT_FOUND, &headers));
}

#[test]
fn does_not_retry_unconfirmed_request_delivery() {
	let error = crate::errors::RequestDeliveryUnconfirmed {
		phase: "request_start".to_owned(),
		reason: "envoy_handoff_ack_timeout".to_owned(),
	}
	.build();

	assert!(!should_retry_error(&error));
}

#[test]
fn retries_a_definitive_no_responders_request_start_failure() {
	let error = crate::errors::TunnelMessageTimeout {
		phase: "request_start".to_owned(),
		reason: "no_responders_after_retry_budget_exhausted".to_owned(),
	}
	.build();

	assert!(should_retry_error(&error));
}

#[test]
fn structured_error_responses_include_the_matching_error_header() {
	let response = err_into_response(
		crate::errors::RequestDeliveryUnconfirmed {
			phase: "request_start".to_owned(),
			reason: "envoy_handoff_ack_timeout".to_owned(),
		}
		.build(),
	)
	.expect("build error response");

	assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
	assert_eq!(
		response.headers().get(X_RIVET_ERROR),
		Some(&HeaderValue::from_static(
			"guard.request_delivery_unconfirmed"
		)),
	);
}
