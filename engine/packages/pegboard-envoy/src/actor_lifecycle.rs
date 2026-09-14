use anyhow::Result;
use depot_client::database::NativeDatabaseHandle;
use futures_util::future::join_all;
use rivet_envoy_protocol as protocol;

use crate::conn::Conn;

pub async fn stop_actor(conn: &Conn, checkpoint: &protocol::ActorCheckpoint) -> Result<()> {
	// Depot owns SQLite correctness in FDB. The connection only holds perf caches, so
	// lifecycle stop evicts stale local state without touching storage.
	conn.actor_dbs.remove_async(&checkpoint.actor_id).await;

	// This runs on the connection's command path, so it must use async map operations. A sync
	// operation parks the Tokio worker, which deadlocks the connection when the remote SQLite task
	// queued on the same bucket can only run on that worker.
	let stopped_generation = u64::from(checkpoint.generation);
	let mut stopped = Vec::new();
	conn.remote_sqlite_executors
		.iter_mut_async(|entry| {
			let (actor_id, generation) = entry.key();
			if actor_id != &checkpoint.actor_id {
				return true;
			}
			// A newer generation may already be serving SQL, so it is only evicted from the cache.
			let close = *generation <= stopped_generation;
			let (_, executor) = entry.consume();
			if close && let Some(handle) = executor.get() {
				stopped.push(handle.clone());
			}
			true
		})
		.await;
	close_in_background(stopped);

	Ok(())
}

pub async fn shutdown_conn_actors(conn: &Conn) {
	// See `stop_actor`. The connection is going away, so every cached executor is closed.
	conn.actor_dbs.clear_async().await;

	let mut stopped = Vec::new();
	conn.remote_sqlite_executors
		.iter_mut_async(|entry| {
			let (_, executor) = entry.consume();
			if let Some(handle) = executor.get() {
				stopped.push(handle.clone());
			}
			true
		})
		.await;
	close_in_background(stopped);
}

/// Closes evicted remote SQLite executors on one background task.
///
/// Closing waits for each SQLite worker to drain, and eviction runs on the connection's command
/// path, which must keep delivering commands while that happens.
fn close_in_background(handles: Vec<NativeDatabaseHandle>) {
	if handles.is_empty() {
		return;
	}

	tokio::spawn(async move {
		for result in join_all(handles.iter().map(NativeDatabaseHandle::close)).await {
			if let Err(err) = result {
				tracing::warn!(
					?err,
					"failed to close evicted remote sqlite executor, the sqlite worker may still be draining"
				);
			}
		}
	});
}
