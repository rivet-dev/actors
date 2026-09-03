use super::*;

#[test]
fn every_nonempty_http_upload_streams_after_request_start() {
	assert!(!should_stream_http_request_body(
		&Method::POST,
		Some(0),
		false
	));
	assert!(should_stream_http_request_body(
		&Method::POST,
		Some(1),
		false
	));
	assert!(should_stream_http_request_body(&Method::POST, None, false));
	assert!(!should_stream_http_request_body(
		&Method::GET,
		Some(1),
		false
	));
	assert!(!should_stream_http_request_body(
		&Method::POST,
		Some(1),
		true
	));
}

#[test]
fn streaming_request_body_size_is_cumulative_and_overflow_safe() {
	assert_eq!(
		next_request_body_size(6, 4, 10),
		RequestBodySize::WithinLimit(10)
	);
	assert_eq!(
		next_request_body_size(7, 4, 10),
		RequestBodySize::ExceedsLimit
	);
	assert_eq!(
		next_request_body_size(usize::MAX, 1, usize::MAX),
		RequestBodySize::ExceedsLimit
	);
}

#[test]
fn request_body_chunker_coalesces_tiny_frames() {
	let mut chunker = HttpRequestBodyChunker::default();
	let mut chunks = Vec::new();
	for _ in 0..HTTP_BODY_CHUNK_SIZE {
		chunks.extend(chunker.push(&[7]));
	}

	assert_eq!(chunks.len(), 1);
	assert_eq!(chunks[0].len(), HTTP_BODY_CHUNK_SIZE);
	assert!(chunker.is_empty());
}

#[test]
fn request_body_chunker_flushes_partial_data() {
	let mut chunker = HttpRequestBodyChunker::default();
	assert!(chunker.push(&[1, 2, 3]).is_empty());
	assert_eq!(chunker.flush(), Some(vec![1, 2, 3]));
	assert!(chunker.is_empty());
}
