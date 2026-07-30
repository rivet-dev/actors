use super::*;

mod moved_tests {
	use std::collections::HashMap;
	use std::time::Duration;

	use super::{
		HttpResponseEncoding, authorization_bearer_token, authorization_bearer_token_map,
		build_envoy_response, framework_action_error_response,
		framework_anyhow_error_response_with_actor, is_actor_request_path,
		message_boundary_error_response, message_boundary_error_response_with_actor,
		normalize_actor_request_path, request_encoding, workflow_dispatch_result,
	};
	use crate::actor::action::ActionDispatchError;
	use crate::actor::messages::{ActorHttpResponse, Request, Response, StreamingResponse};
	use crate::error::ActorLifecycle as ActorLifecycleError;
	use http::StatusCode;
	use rivet_envoy_client::config::ResponseChunk;
	use rivet_error::{ActorSpecifier, MacroMarker, RivetError, RivetErrorSchema};
	use serde_json::json;
	use tokio::sync::mpsc;
	use vbare::OwnedVersionedData;

	#[derive(RivetError)]
	#[error("message", "incoming_too_long", "Incoming message too long")]
	struct IncomingMessageTooLong;

	#[derive(RivetError)]
	#[error("message", "outgoing_too_long", "Outgoing message too long")]
	struct OutgoingMessageTooLong;

	#[test]
	fn workflow_dispatch_result_marks_handled_workflow_as_enabled() {
		assert_eq!(
			workflow_dispatch_result(Ok(Some(vec![1, 2, 3])))
				.expect("workflow dispatch should succeed"),
			(true, Some(vec![1, 2, 3])),
		);
		assert_eq!(
			workflow_dispatch_result(Ok(None)).expect("workflow dispatch should succeed"),
			(true, None),
		);
	}

	#[test]
	fn workflow_dispatch_result_treats_dropped_reply_as_disabled() {
		assert_eq!(
			workflow_dispatch_result(Err(ActorLifecycleError::DroppedReply.build()))
				.expect("dropped reply should map to workflow disabled"),
			(false, None),
		);
	}

	#[test]
	fn workflow_dispatch_result_preserves_non_dropped_reply_errors() {
		let error = workflow_dispatch_result(Err(ActorLifecycleError::Destroying.build()))
			.expect_err("non-dropped reply errors should be preserved");
		let error = rivet_error::RivetError::extract(&error);
		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "destroying");
	}

	#[tokio::test]
	async fn build_envoy_response_preserves_stream_body() {
		let (tx, rx) = mpsc::channel(1);
		let stream = StreamingResponse::from_parts(
			StatusCode::OK.as_u16(),
			HashMap::from([("content-type".to_owned(), "text/event-stream".to_owned())]),
			rx,
		)
		.expect("streaming response should build");

		let mut response =
			build_envoy_response(ActorHttpResponse::Stream(stream), &http::Method::GET)
				.expect("envoy response should build");

		assert_eq!(response.status, StatusCode::OK.as_u16());
		assert_eq!(response.body, None);
		assert!(response.body_stream.is_some());

		tx.send(ResponseChunk::Data {
			data: b"data: ok\n\n".to_vec(),
			finish: true,
		})
		.await
		.expect("send response chunk");
		let chunk = response
			.body_stream
			.as_mut()
			.expect("body stream should exist")
			.recv()
			.await
			.expect("stream chunk should arrive");
		match chunk {
			ResponseChunk::Data { data, finish } => {
				assert_eq!(data, b"data: ok\n\n");
				assert!(finish);
			}
			ResponseChunk::Error(error) => panic!("unexpected response chunk error: {error}"),
		}
	}

	#[tokio::test]
	async fn non_streaming_factory_request_is_buffered_before_dispatch() {
		let (body_tx, body_rx) = mpsc::channel(1);
		let (abort_tx, abort_rx) = tokio::sync::watch::channel::<
			Option<rivet_envoy_client::config::HttpRequestBodyError>,
		>(None);
		body_tx
			.send(b"-stream".to_vec())
			.await
			.expect("send body chunk");
		drop(body_tx);
		drop(abort_tx);
		let request = Request::from_parts_with_stream(
			"POST",
			"/",
			HashMap::new(),
			b"prefix".to_vec(),
			Some(rivet_envoy_client::config::HttpRequestBodyStream::new(
				body_rx, abort_rx,
			)),
		)
		.expect("build streaming request");

		let request = request.into_buffered().await.expect("buffer request");

		assert!(!request.has_body_stream());
		assert_eq!(request.into_body(), b"prefix-stream");
	}

	#[test]
	fn build_envoy_response_drops_stream_for_body_forbidden_status() {
		let (_tx, rx) = mpsc::channel(1);
		let stream =
			StreamingResponse::from_parts(StatusCode::NO_CONTENT.as_u16(), HashMap::new(), rx)
				.expect("streaming response should build");

		let response = build_envoy_response(ActorHttpResponse::Stream(stream), &http::Method::GET)
			.expect("envoy response should build");

		assert_eq!(response.status, StatusCode::NO_CONTENT.as_u16());
		assert_eq!(response.body, Some(Vec::new()));
		assert!(response.body_stream.is_none());
	}

	#[test]
	fn build_envoy_response_drops_buffered_body_for_head() {
		let response =
			Response::from_parts(StatusCode::OK.as_u16(), HashMap::new(), b"body".to_vec())
				.expect("buffered response should build");

		let response =
			build_envoy_response(ActorHttpResponse::Buffered(response), &http::Method::HEAD)
				.expect("envoy response should build");

		assert_eq!(response.status, StatusCode::OK.as_u16());
		assert_eq!(response.body, Some(Vec::new()));
		assert!(response.body_stream.is_none());
	}

	#[test]
	fn inspector_error_status_maps_action_timeout_to_408() {
		assert_eq!(
			super::inspector_error_status("actor", "action_timed_out"),
			StatusCode::REQUEST_TIMEOUT,
		);
	}

	#[test]
	fn authorization_bearer_token_accepts_case_insensitive_scheme_and_whitespace() {
		let mut headers = http::HeaderMap::new();
		headers.insert(
			http::header::AUTHORIZATION,
			"bearer   test-token".parse().unwrap(),
		);

		assert_eq!(authorization_bearer_token(&headers), Some("test-token"));

		let map = HashMap::from([(
			http::header::AUTHORIZATION.as_str().to_owned(),
			"BEARER\ttest-token".to_owned(),
		)]);
		assert_eq!(authorization_bearer_token_map(&map), Some("test-token"));
	}

	#[tokio::test]
	async fn action_dispatch_timeout_returns_structured_error() {
		let error = super::with_action_dispatch_timeout(Duration::from_millis(1), async {
			tokio::time::sleep(Duration::from_secs(60)).await;
			Ok::<Vec<u8>, ActionDispatchError>(Vec::new())
		})
		.await
		.expect_err("timeout should return an action dispatch error");

		assert_eq!(error.group, "actor");
		assert_eq!(error.code, "action_timed_out");
		assert_eq!(error.message, "Action timed out");
	}

	#[tokio::test]
	async fn framework_action_timeout_returns_structured_error() {
		let error = super::with_framework_action_timeout(Duration::from_millis(1), async {
			tokio::time::sleep(Duration::from_secs(60)).await;
			Ok::<(), anyhow::Error>(())
		})
		.await
		.expect_err("timeout should return a framework error");
		let error = RivetError::extract(&error);

		assert_eq!(error.group(), "actor");
		assert_eq!(error.code(), "action_timed_out");
		assert_eq!(error.message(), "Action timed out");
	}

	#[test]
	fn framework_action_error_response_maps_timeout_to_408() {
		let response = framework_action_error_response(
			HttpResponseEncoding::Json,
			ActionDispatchError {
				group: "actor".to_owned(),
				code: "action_timed_out".to_owned(),
				message: "Action timed out".to_owned(),
				metadata: None,
				actor: None,
			},
			None,
		)
		.expect("timeout error response should serialize");

		assert_eq!(response.status, StatusCode::REQUEST_TIMEOUT.as_u16());
		assert_eq!(
			response.body,
			Some(
				serde_json::to_vec(&json!({
					"group": "actor",
					"code": "action_timed_out",
					"message": "Action timed out",
				}))
				.expect("json body should encode")
			)
		);
	}

	#[test]
	fn framework_action_error_response_sanitizes_internal_message() {
		let response = framework_action_error_response(
			HttpResponseEncoding::Json,
			ActionDispatchError {
				group: "rivetkit".to_owned(),
				code: "internal_error".to_owned(),
				message: "plain failure".to_owned(),
				metadata: Some(json!({ "error": "plain failure" })),
				actor: None,
			},
			None,
		)
		.expect("internal error response should serialize");

		assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR.as_u16());
		assert_eq!(
			response.body,
			Some(
				serde_json::to_vec(&json!({
					"group": "rivetkit",
					"code": "internal_error",
					"message": "An internal error occurred",
				}))
				.expect("json body should encode")
			)
		);
	}

	#[test]
	fn message_boundary_error_response_defaults_to_json() {
		let response = message_boundary_error_response(
			HttpResponseEncoding::Json,
			StatusCode::BAD_REQUEST,
			IncomingMessageTooLong.build(),
		)
		.expect("json response should serialize");

		assert_eq!(response.status, StatusCode::BAD_REQUEST.as_u16());
		assert_eq!(
			response.headers.get(http::header::CONTENT_TYPE.as_str()),
			Some(&"application/json".to_owned())
		);
		assert_eq!(
			response.body,
			Some(
				serde_json::to_vec(&json!({
					"group": "message",
					"code": "incoming_too_long",
					"message": "Incoming message too long",
				}))
				.expect("json body should encode")
			)
		);
	}

	#[test]
	fn framework_anyhow_error_response_sanitizes_internal_message() {
		static TEST_ERROR: RivetErrorSchema = RivetErrorSchema {
			group: "rivetkit",
			code: "internal_error",
			default_message: "plain failure",
			meta_type: None,
			_macro_marker: MacroMarker { _private: () },
		};
		let response = framework_anyhow_error_response_with_actor(
			HttpResponseEncoding::Json,
			TEST_ERROR.build(),
			None,
		)
		.expect("internal error response should serialize");

		assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR.as_u16());
		assert_eq!(
			response.body,
			Some(
				serde_json::to_vec(&json!({
					"group": "rivetkit",
					"code": "internal_error",
					"message": "An internal error occurred",
				}))
				.expect("json body should encode")
			)
		);
	}

	#[test]
	fn request_encoding_reads_cbor_header() {
		let mut headers = http::HeaderMap::new();
		headers.insert("x-rivet-encoding", "cbor".parse().unwrap());

		assert_eq!(request_encoding(&headers), HttpResponseEncoding::Cbor);
	}

	#[test]
	fn normalize_actor_request_path_preserves_raw_root_paths() {
		assert!(is_actor_request_path("/request"));
		assert!(is_actor_request_path("/request/"));
		assert!(is_actor_request_path("/request/users/1"));
		assert!(is_actor_request_path("/request?foo=bar"));
		assert_eq!(normalize_actor_request_path("/request"), "/");
		assert_eq!(normalize_actor_request_path("/request/"), "/");
		assert_eq!(normalize_actor_request_path("/request/users/1"), "/users/1",);
		assert_eq!(normalize_actor_request_path("/request?foo=bar"), "?foo=bar");
	}

	#[test]
	fn normalize_actor_request_path_does_not_mark_framework_routes_as_raw() {
		assert!(!is_actor_request_path("/"));
		assert!(!is_actor_request_path("/action/ping"));
		assert!(!is_actor_request_path("/requestfoo"));
		assert_eq!(normalize_actor_request_path("/"), "/");
		assert_eq!(normalize_actor_request_path("/action/ping"), "/action/ping");
		assert_eq!(normalize_actor_request_path("/requestfoo"), "/requestfoo");
	}

	#[test]
	fn message_boundary_error_response_with_actor_sets_body_and_headers() {
		let actor = ActorSpecifier::new("actor-http", 9).with_key("user/1");
		let response = message_boundary_error_response_with_actor(
			HttpResponseEncoding::Json,
			StatusCode::BAD_REQUEST,
			IncomingMessageTooLong.build(),
			Some(&actor),
		)
		.expect("json response should serialize");

		assert_eq!(
			response.headers.get("x-rivet-actor"),
			Some(&"actor-http".to_owned())
		);
		assert_eq!(
			response.headers.get("x-rivet-actor-generation"),
			Some(&"9".to_owned())
		);
		assert_eq!(
			response.headers.get("x-rivet-actor-key"),
			Some(&"user/1".to_owned())
		);
		assert_eq!(
			response.body,
			Some(
				serde_json::to_vec(&json!({
					"group": "message",
					"code": "incoming_too_long",
					"message": "Incoming message too long",
					"actor": {
						"actorId": "actor-http",
						"generation": 9,
						"key": "user/1",
					},
				}))
				.expect("json body should encode")
			)
		);
	}

	#[test]
	fn message_boundary_error_response_serializes_bare_v3() {
		let response = message_boundary_error_response(
			HttpResponseEncoding::Bare,
			StatusCode::BAD_REQUEST,
			OutgoingMessageTooLong.build(),
		)
		.expect("bare response should serialize");

		assert_eq!(
			response.headers.get(http::header::CONTENT_TYPE.as_str()),
			Some(&"application/octet-stream".to_owned())
		);

		let body = response.body.expect("bare response should include body");
		let decoded =
			<rivetkit_client_protocol::versioned::HttpResponseError as OwnedVersionedData>::deserialize_with_embedded_version(&body)
				.expect("bare error should decode");
		assert_eq!(decoded.group, "message");
		assert_eq!(decoded.code, "outgoing_too_long");
		assert_eq!(decoded.message, "Outgoing message too long");
		assert_eq!(decoded.metadata, None);
	}
}
