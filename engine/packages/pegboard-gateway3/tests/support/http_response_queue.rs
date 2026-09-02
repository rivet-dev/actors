use super::*;

const MAX_MESSAGES: usize = 384;
const MAX_BYTES: usize = 20 * 1024 * 1024;

#[test]
fn http_response_queue_budget_bounds_messages_and_releases_capacity() {
	let budget = Arc::new(HttpResponseQueueBudget::new(
		MAX_BYTES,
		MAX_MESSAGES,
		MAX_BYTES,
	));
	let permits = (0..MAX_MESSAGES)
		.map(|_| budget.try_reserve(0, true).expect("message should fit"))
		.collect::<Vec<_>>();

	assert!(budget.try_reserve(0, true).is_none());
	drop(permits);
	assert!(budget.try_reserve(0, true).is_some());
}

#[test]
fn http_response_queue_budget_bounds_bytes_without_limiting_total_stream_size() {
	let budget = Arc::new(HttpResponseQueueBudget::new(
		MAX_BYTES,
		MAX_MESSAGES,
		MAX_BYTES,
	));
	let permit = budget
		.try_reserve(MAX_BYTES, true)
		.expect("queue-sized chunk should fit");

	assert!(budget.try_reserve(1, true).is_none());
	drop(permit);
	assert!(budget.try_reserve(MAX_BYTES, true).is_some());
}
