use super::*;

#[test]
fn response_stream_message_index_advances_and_wraps() {
	assert_eq!(advance_http_stream_message_index(7, 7), Ok(8));
	assert_eq!(
		advance_http_stream_message_index(protocol::MessageIndex::MAX, protocol::MessageIndex::MAX),
		Ok(0)
	);
}

#[test]
fn response_stream_message_index_rejects_gaps() {
	assert_eq!(advance_http_stream_message_index(7, 8), Err(()));
	assert_eq!(advance_http_stream_message_index(7, 6), Err(()));
}
