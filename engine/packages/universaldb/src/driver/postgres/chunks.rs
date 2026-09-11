use std::{
	collections::HashMap,
	time::{Duration, Instant},
};

/// A partially received commit request is dropped once no piece of it has arrived for this long. The
/// follower gives up on an attempt after its request timeout and resends from the first piece, so
/// anything older can never complete.
pub const PENDING_CHUNK_MAX_IDLE: Duration = Duration::from_secs(30);

/// One piece of a commit request that a follower split across several NATS messages.
pub struct CommitChunk {
	pub client_node_id: Vec<u8>,
	pub client_seq: u64,
	pub attempt: u32,
	pub index: u32,
	pub count: u32,
	pub data: Vec<u8>,
}

struct PendingRequest {
	attempt: u32,
	count: u32,
	next_index: u32,
	bytes: Vec<u8>,
	updated_at: Instant,
}

/// Reassembles chunked commit requests on the leader.
///
/// NATS delivers one publisher's messages on a subject in order, so pieces of an attempt arrive in
/// sequence unless a reconnect loses some. Any gap abandons the attempt instead of waiting for the
/// missing piece: the follower's request times out and it resends every piece under a new attempt.
/// Owned by the single commit subscriber task.
#[derive(Default)]
pub struct ChunkAssembler {
	pending: HashMap<(Vec<u8>, u64), PendingRequest>,
}

impl ChunkAssembler {
	/// Accepts one piece and returns the complete encoded request once its last piece arrives.
	pub fn push(&mut self, chunk: CommitChunk, now: Instant) -> Option<Vec<u8>> {
		let key = (chunk.client_node_id, chunk.client_seq);

		if chunk.index == 0 {
			if let Some(existing) = self.pending.get(&key) {
				if chunk.attempt < existing.attempt {
					return None;
				}
			}
			if chunk.count <= 1 {
				self.pending.remove(&key);
				return Some(chunk.data);
			}
			self.pending.insert(
				key,
				PendingRequest {
					attempt: chunk.attempt,
					count: chunk.count,
					next_index: 1,
					bytes: chunk.data,
					updated_at: now,
				},
			);
			return None;
		}

		let Some(pending) = self.pending.get_mut(&key) else {
			return None;
		};
		if chunk.attempt < pending.attempt {
			return None;
		}
		if chunk.attempt != pending.attempt
			|| chunk.count != pending.count
			|| chunk.index != pending.next_index
		{
			self.pending.remove(&key);
			return None;
		}

		pending.bytes.extend_from_slice(&chunk.data);
		pending.next_index += 1;
		pending.updated_at = now;

		if pending.next_index == pending.count {
			return self.pending.remove(&key).map(|pending| pending.bytes);
		}
		None
	}

	/// Drops requests that stopped receiving pieces, so a follower that died mid-send does not pin its
	/// partial request in memory.
	pub fn evict_idle(&mut self, now: Instant) {
		self.pending
			.retain(|_, pending| now.duration_since(pending.updated_at) < PENDING_CHUNK_MAX_IDLE);
	}
}

#[cfg(test)]
#[path = "../../../tests/unit/postgres_chunks.rs"]
mod tests;
