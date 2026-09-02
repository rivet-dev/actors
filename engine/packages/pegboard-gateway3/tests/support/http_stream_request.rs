use super::*;

#[test]
fn request_body_streams_only_after_one_chunk() {
	let hint = |size| SizeHint::with_exact(size as u64);
	assert!(!should_stream_http_request_body_hint(&hint(0)));
	assert!(!should_stream_http_request_body_hint(&hint(
		HTTP_BODY_CHUNK_SIZE
	)));
	assert!(should_stream_http_request_body_hint(&hint(
		HTTP_BODY_CHUNK_SIZE + 1
	)));
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
