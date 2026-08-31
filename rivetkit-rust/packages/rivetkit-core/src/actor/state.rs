use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::time::Instant as StdInstant;
use crate::time::sleep;

use anyhow::{Context, Result};
use rivetkit_actor_persist::{generated::v4 as persist_v4, versioned as persist_versioned};
#[cfg(not(feature = "wasm-runtime"))]
use tokio::runtime::Handle;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, mpsc, oneshot};
use tokio::task::JoinHandle;
#[cfg(test)]
use tokio::time::timeout;
use tracing::Instrument;

use crate::actor::connection::{PersistedConnection, decode_persisted_connection};
use crate::actor::context::ActorContext;
use crate::actor::internal_storage;
use crate::actor::lifecycle_hooks::Reply;
use crate::actor::messages::{StateDelta, WorkflowKvWrite};
use crate::actor::persist::decode_latest_with_embedded_version;
#[cfg(test)]
use crate::actor::persist::encode_latest_with_embedded_version;
use crate::actor::task::LifecycleEvent;
use crate::actor::task_types::StateMutationReason;
use crate::error::ActorRuntime;
#[cfg(feature = "wasm-runtime")]
use crate::runtime::RuntimeSpawner;
use crate::sqlite::{BindParam, ExecuteResult, SqliteTransaction};
use crate::types::SaveStateOpts;

#[cfg(test)]
const LAST_PUSHED_ALARM_VERSION: u16 = 1;

pub type PersistedScheduleEvent = persist_v4::ScheduleEvent;
pub type PersistedActor = persist_v4::Actor;

#[cfg(test)]
pub(crate) fn encode_persisted_actor(actor: &PersistedActor) -> Result<Vec<u8>> {
	encode_latest_with_embedded_version::<persist_versioned::Actor>(
		actor.clone(),
		rivetkit_actor_persist::CURRENT_VERSION,
		"persisted actor",
	)
}

pub(crate) fn decode_persisted_actor(payload: &[u8]) -> Result<PersistedActor> {
	let actor = decode_latest_with_embedded_version::<persist_versioned::Actor>(
		payload,
		"persisted actor",
	)?;
	Ok(actor)
}

#[cfg(test)]
pub(crate) fn encode_last_pushed_alarm(alarm_ts: Option<i64>) -> Result<Vec<u8>> {
	encode_latest_with_embedded_version::<persist_versioned::LastPushedAlarm>(
		alarm_ts,
		LAST_PUSHED_ALARM_VERSION,
		"last pushed alarm",
	)
}

pub(crate) fn decode_last_pushed_alarm(payload: &[u8]) -> Result<Option<i64>> {
	decode_latest_with_embedded_version::<persist_versioned::LastPushedAlarm>(
		payload,
		"last pushed alarm",
	)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestSaveOpts {
	pub immediate: bool,
	pub max_wait_ms: Option<u32>,
}

pub(super) struct PendingSave {
	scheduled_at: StdInstant,
	handle: JoinHandle<()>,
}

pub struct OnStateChangeGuard {
	ctx: Option<ActorContext>,
}
/// A SQLite transaction that owns the actor state-save exclusion until it is
/// committed or rolled back.
#[derive(Clone)]
pub struct ActorStateTransaction {
	owner: Arc<AsyncMutex<ActorStateTransactionOwner>>,
}

struct ActorStateTransactionOwner {
	ctx: ActorContext,
	transaction: SqliteTransaction,
	save_guard: Option<OwnedMutexGuard<()>>,
	write_guard: Option<InFlightWrite>,
	finalized: bool,
	committed: bool,
	save_request_revision: u64,
	statement_count: usize,
	bind_payload_bytes: usize,
	rollback_connection_states: Vec<(String, Vec<u8>)>,
}

impl ActorStateTransactionOwner {
	fn restore_connection_states(&self) {
		for (conn_id, state) in &self.rollback_connection_states {
			if let Some(conn) = self.ctx.connection(conn_id) {
				conn.set_state(state.clone());
			}
		}
	}

	fn advance_epoch(&self) {
		self.ctx
			.0
			.state_transaction_epoch
			.fetch_add(1, Ordering::SeqCst);
	}
}

impl Drop for ActorStateTransactionOwner {
	fn drop(&mut self) {
		if !self.finalized {
			self.restore_connection_states();
			self.advance_epoch();
			let transaction = self.transaction.clone();
			#[cfg(not(feature = "wasm-runtime"))]
			if let Ok(handle) = Handle::try_current() {
				handle.spawn(async move {
					if let Err(error) = transaction.rollback().await {
						tracing::debug!(?error, "dropped actor state transaction rollback failed");
					}
				});
			}
			#[cfg(feature = "wasm-runtime")]
			wasm_bindgen_futures::spawn_local(async move {
				if let Err(error) = transaction.rollback().await {
					tracing::debug!(?error, "dropped actor state transaction rollback failed");
				}
			});
		}
		if !self.committed {
			self.ctx.schedule_save(None);
		}
	}
}

impl ActorStateTransaction {
	pub async fn execute(
		&self,
		sql: impl Into<String>,
		params: Option<Vec<BindParam>>,
	) -> Result<ExecuteResult> {
		let mut owner = self.owner.lock().await;
		if owner.finalized {
			return Err(anyhow::anyhow!(
				"actor state transaction is already finalized"
			));
		}
		let sql = sql.into();
		if is_transaction_terminator(&sql) {
			return Err(anyhow::anyhow!(
				"cannot execute transaction terminator inside an actor state transaction; use the transaction commit or rollback method"
			));
		}
		let payload_bytes = match params.as_deref() {
			Some(params) => params
				.iter()
				.map(internal_storage::bind_param_payload_len)
				.fold(0usize, usize::saturating_add),
			None => sql.len(),
		};
		let result = owner.transaction.execute(sql, params).await?;
		owner.statement_count = owner.statement_count.saturating_add(1);
		owner.bind_payload_bytes = owner.bind_payload_bytes.saturating_add(payload_bytes);
		Ok(result)
	}

	pub async fn commit(&self, deltas: Vec<StateDelta>) -> Result<()> {
		let mut owner = self.owner.lock().await;
		if owner.finalized {
			return Err(anyhow::anyhow!(
				"actor state transaction is already finalized"
			));
		}

		let save_request_revision = owner.save_request_revision;
		let transaction = owner.transaction.clone();
		let statement_count = owner.statement_count;
		let bind_payload_bytes = owner.bind_payload_bytes;
		let result = owner
			.ctx
			.commit_state_transaction(
				&transaction,
				deltas,
				save_request_revision,
				statement_count,
				bind_payload_bytes,
			)
			.await;
		owner.finalized = true;
		owner.committed = result.is_ok();
		if result.is_err() {
			owner.restore_connection_states();
		}
		owner.advance_epoch();
		owner.write_guard.take();
		owner.save_guard.take();
		if result.is_err() {
			owner.ctx.schedule_save(None);
		}
		result
	}

	pub async fn rollback(&self) -> Result<()> {
		let mut owner = self.owner.lock().await;
		if owner.finalized {
			return Err(anyhow::anyhow!(
				"actor state transaction is already finalized"
			));
		}
		let result = owner.transaction.rollback().await;
		owner.finalized = true;
		owner.restore_connection_states();
		owner.advance_epoch();
		owner.write_guard.take();
		owner.save_guard.take();
		owner.ctx.schedule_save(None);
		result
	}
}

fn is_transaction_terminator(sql: &str) -> bool {
	let mut offset = 0;
	let Some(first) = next_sql_keyword(sql, &mut offset) else {
		return false;
	};
	if first.eq_ignore_ascii_case("COMMIT") || first.eq_ignore_ascii_case("END") {
		return true;
	}
	if !first.eq_ignore_ascii_case("ROLLBACK") {
		return false;
	}

	let mut next = next_sql_keyword(sql, &mut offset);
	if next.is_some_and(|keyword| keyword.eq_ignore_ascii_case("TRANSACTION")) {
		next = next_sql_keyword(sql, &mut offset);
	}
	!next.is_some_and(|keyword| keyword.eq_ignore_ascii_case("TO"))
}

fn next_sql_keyword<'a>(sql: &'a str, offset: &mut usize) -> Option<&'a str> {
	let bytes = sql.as_bytes();
	loop {
		while bytes.get(*offset).is_some_and(u8::is_ascii_whitespace) {
			*offset += 1;
		}
		if bytes.get(*offset..*offset + 2) == Some(b"--") {
			*offset += 2;
			while bytes.get(*offset).is_some_and(|byte| *byte != b'\n') {
				*offset += 1;
			}
			continue;
		}
		if bytes.get(*offset..*offset + 2) == Some(b"/*") {
			*offset += 2;
			while bytes.get(*offset..*offset + 2) != Some(b"*/") {
				bytes.get(*offset)?;
				*offset += 1;
			}
			*offset += 2;
			continue;
		}
		break;
	}

	let start = *offset;
	while bytes.get(*offset).is_some_and(u8::is_ascii_alphabetic) {
		*offset += 1;
	}
	(start != *offset).then(|| &sql[start..*offset])
}

impl OnStateChangeGuard {
	fn new(ctx: ActorContext) -> Self {
		ctx.on_state_change_started();
		Self { ctx: Some(ctx) }
	}
}

impl Drop for OnStateChangeGuard {
	fn drop(&mut self) {
		if let Some(ctx) = self.ctx.take() {
			ctx.on_state_change_finished();
		}
	}
}

impl ActorContext {
	pub async fn begin_state_transaction(
		&self,
		timeout: Option<Duration>,
	) -> Result<ActorStateTransaction> {
		self.clear_pending_save();
		let save_guard = Arc::clone(&self.0.save_guard).lock_owned().await;
		self.0
			.state_transaction_epoch
			.fetch_add(1, Ordering::SeqCst);
		self.wait_for_in_flight_writes().await;
		let rollback_connection_states = self
			.iter_connections()
			.filter(|conn| conn.is_hibernatable())
			.map(|conn| (conn.id().to_owned(), conn.state()))
			.collect();
		let transaction = match self.sql().begin_transaction(timeout).await {
			Ok(transaction) => transaction,
			Err(error) => {
				self.0
					.state_transaction_epoch
					.fetch_add(1, Ordering::SeqCst);
				drop(save_guard);
				self.schedule_save(None);
				return Err(error);
			}
		};
		let write_guard = self.begin_write();
		let save_request_revision = self.save_request_revision();
		Ok(ActorStateTransaction {
			owner: Arc::new(AsyncMutex::new(ActorStateTransactionOwner {
				ctx: self.clone(),
				transaction,
				save_guard: Some(save_guard),
				write_guard: Some(write_guard),
				finalized: false,
				committed: false,
				save_request_revision,
				statement_count: 0,
				bind_payload_bytes: 0,
				rollback_connection_states,
			})),
		})
	}
	pub fn state(&self) -> Vec<u8> {
		self.0.current_state.read().clone()
	}

	pub(crate) async fn persist_state(&self, opts: SaveStateOpts) -> Result<()> {
		if !self.is_dirty() {
			return Ok(());
		}

		let result = if opts.immediate {
			self.clear_pending_save();
			self.persist_if_dirty().await
		} else {
			let delay = self.compute_save_delay(None);
			if !delay.is_zero() {
				sleep(delay).await;
			}
			self.persist_if_dirty().await
		};
		result?;
		self.record_state_updated();
		Ok(())
	}

	/// Foreign-runtime bootstrap hook for installing the actor state snapshot
	/// before the actor starts handling lifecycle/dispatch work.
	pub fn set_state_initial(&self, state: Vec<u8>) {
		self.set_initial_state(state);
	}

	/// Fire-and-forget save request helper.
	///
	/// If the lifecycle event inbox is unavailable, this only logs a warning and
	/// returns. That `warn!` is the sole failure signal for this path; callers do
	/// not receive a `Result`. Call
	/// [`Self::request_save_and_wait`] when the caller must observe
	/// save-request delivery failures.
	pub fn request_save(&self, opts: RequestSaveOpts) {
		#[cfg(target_arch = "wasm32")]
		{
			self.request_save_best_effort(opts);
		}

		#[cfg(not(target_arch = "wasm32"))]
		if let Err(error) = self.request_save_with_revision(opts) {
			tracing::warn!(?error, "failed to request actor state save");
		}
	}

	#[cfg(target_arch = "wasm32")]
	fn request_save_best_effort(&self, opts: RequestSaveOpts) {
		let immediate = opts.immediate;
		let _save_request_revision =
			self.0.save_request_revision.fetch_add(1, Ordering::SeqCst) + 1;
		self.notify_request_save_hooks(opts);
		let already_requested = self.0.save_requested.swap(true, Ordering::SeqCst);
		let immediate_already_requested = if immediate {
			self.0.save_requested_immediate.swap(true, Ordering::SeqCst)
		} else {
			self.0.save_requested_immediate.load(Ordering::SeqCst)
		};

		if let Some(max_wait_ms) = opts.max_wait_ms {
			let deadline = StdInstant::now() + Duration::from_millis(u64::from(max_wait_ms));
			let mut requested_deadline = self.0.save_requested_within_deadline.lock();
			*requested_deadline = Some(match *requested_deadline {
				Some(existing) => existing.min(deadline),
				None => deadline,
			});
		}

		let Some(sender) = self.lifecycle_event_sender() else {
			return;
		};

		if opts.max_wait_ms.is_none()
			&& already_requested
			&& (!immediate || immediate_already_requested)
		{
			return;
		}

		let _ = sender.send(LifecycleEvent::SaveRequested { immediate });
	}

	pub async fn request_save_and_wait(&self, opts: RequestSaveOpts) -> Result<()> {
		let save_request_revision = self.request_save_with_revision(opts)?;
		self.wait_for_save_request(save_request_revision).await;
		Ok(())
	}

	pub async fn save_state(&self, deltas: Vec<StateDelta>) -> Result<()> {
		let save_request_revision = self.save_request_revision();
		self.save_state_with_revision(deltas, save_request_revision)
			.await
	}

	/// Requests one logical workflow-engine flush. The actor lifecycle owns
	/// serializing actor state and committing both sides in one SQLite transaction.
	pub async fn save_state_and_workflow_batch(
		&self,
		workflow_writes: Vec<WorkflowKvWrite>,
	) -> Result<()> {
		let Some(sender) = self.lifecycle_event_sender() else {
			return Err(ActorRuntime::NotConfigured {
				component: "lifecycle events".to_owned(),
			}
			.build());
		};
		let (reply_tx, reply_rx) = oneshot::channel();
		sender
			.send(LifecycleEvent::WorkflowFlushRequested {
				writes: workflow_writes,
				reply: Reply::from(reply_tx),
			})
			.map_err(|_| {
				ActorRuntime::NotConfigured {
					component: "lifecycle events".to_owned(),
				}
				.build()
			})?;
		reply_rx
			.await
			.context("receive workflow flush lifecycle reply")?
	}

	/// Commits an already serialized snapshot and workflow flush atomically for
	/// storage-level fault tests. Runtime bridges must use the lifecycle-owned API.
	#[cfg(test)]
	pub(crate) async fn commit_serialized_state_and_workflow_batch(
		&self,
		deltas: Vec<StateDelta>,
		workflow_writes: Vec<WorkflowKvWrite>,
	) -> Result<()> {
		let save_request_revision = self.save_request_revision();
		self.save_state_and_workflow_batch_with_revision(
			deltas,
			workflow_writes,
			save_request_revision,
		)
		.await
	}

	pub(crate) fn request_save_with_revision(&self, opts: RequestSaveOpts) -> Result<u64> {
		let immediate = opts.immediate;
		let save_request_revision = self.0.save_request_revision.fetch_add(1, Ordering::SeqCst) + 1;
		self.notify_request_save_hooks(opts);
		let already_requested = self.0.save_requested.swap(true, Ordering::SeqCst);
		let immediate_already_requested = if immediate {
			self.0.save_requested_immediate.swap(true, Ordering::SeqCst)
		} else {
			self.0.save_requested_immediate.load(Ordering::SeqCst)
		};

		if let Some(max_wait_ms) = opts.max_wait_ms {
			let deadline = StdInstant::now() + Duration::from_millis(u64::from(max_wait_ms));
			let mut requested_deadline = self.0.save_requested_within_deadline.lock();
			*requested_deadline = Some(match *requested_deadline {
				Some(existing) => existing.min(deadline),
				None => deadline,
			});
		}

		let Some(sender) = self.lifecycle_event_sender() else {
			return Err(ActorRuntime::NotConfigured {
				component: "lifecycle events".to_owned(),
			}
			.build());
		};

		if opts.max_wait_ms.is_none()
			&& already_requested
			&& (!immediate || immediate_already_requested)
		{
			return Ok(save_request_revision);
		}

		sender
			.send(LifecycleEvent::SaveRequested { immediate })
			.map(|()| save_request_revision)
			.map_err(|_| {
				ActorRuntime::NotConfigured {
					component: "lifecycle events".to_owned(),
				}
				.build()
			})
	}

	pub(crate) async fn wait_for_save_request(&self, save_request_revision: u64) {
		loop {
			if self.0.save_completed_revision.load(Ordering::SeqCst) >= save_request_revision {
				return;
			}

			self.0.save_completion.notified().await;
		}
	}

	pub(crate) fn save_requested(&self) -> bool {
		self.0.save_requested.load(Ordering::SeqCst)
	}

	pub(crate) fn save_requested_immediate(&self) -> bool {
		self.0.save_requested_immediate.load(Ordering::SeqCst)
	}

	pub(crate) fn save_deadline(&self, immediate: bool) -> StdInstant {
		self.compute_save_deadline(immediate)
	}

	pub(crate) fn compute_save_deadline(&self, immediate: bool) -> StdInstant {
		if immediate || self.save_requested_immediate() {
			return StdInstant::now();
		}

		let throttled_deadline = StdInstant::now() + self.compute_save_delay(None);
		let requested_deadline = *self.0.save_requested_within_deadline.lock();

		match requested_deadline {
			Some(requested_deadline) => throttled_deadline.min(requested_deadline),
			None => throttled_deadline,
		}
	}

	pub(crate) fn save_request_revision(&self) -> u64 {
		self.0.save_request_revision.load(Ordering::SeqCst)
	}

	pub(crate) async fn apply_state_deltas(
		&self,
		deltas: Vec<StateDelta>,
		save_request_revision: u64,
	) -> Result<()> {
		self.apply_state_deltas_inner(deltas, None, save_request_revision, None)
			.await
			.map(|_| ())
	}

	pub(super) async fn apply_state_deltas_inner(
		&self,
		deltas: Vec<StateDelta>,
		workflow_writes: Option<Vec<WorkflowKvWrite>>,
		save_request_revision: u64,
		expected_state_transaction_epoch: Option<u64>,
	) -> Result<bool> {
		let delta_count = deltas.len();
		let delta_bytes: usize = deltas.iter().map(StateDelta::payload_len).sum();
		let workflow_write_count = workflow_writes.as_ref().map_or(0, Vec::len);
		let current_revision = self.0.state_revision.load(Ordering::SeqCst);
		tracing::debug!(
			delta_count,
			delta_bytes,
			state_revision = current_revision,
			save_request_revision,
			"applying actor state deltas"
		);
		self.clear_pending_save();

		if deltas.is_empty() && workflow_write_count == 0 {
			self.mark_save_request_completed(save_request_revision);
			self.finish_save_request(save_request_revision);
			tracing::debug!(
				delta_count,
				state_revision = current_revision,
				save_request_revision,
				"actor state deltas applied without kv write"
			);
			return Ok(true);
		}

		let prepared = {
			let _save_guard = self.0.save_guard.lock().await;
			if expected_state_transaction_epoch.is_some_and(|expected| {
				self.0.state_transaction_epoch.load(Ordering::SeqCst) != expected
			}) {
				None
			} else {
				let revision = self.0.state_revision.load(Ordering::SeqCst);
				let mut persisted = self.persisted();
				let mut next_state = None;
				let mut actor_to_persist = None;
				let mut connections_to_persist: Vec<PersistedConnection> = Vec::new();
				let mut connections_to_delete = Vec::new();

				for delta in deltas {
					match delta {
						StateDelta::ActorState(bytes) => {
							next_state = Some(bytes.clone());
							persisted.state = bytes;
						}
						StateDelta::ConnHibernation { conn: _, bytes } => {
							connections_to_persist.push(
								decode_persisted_connection(&bytes)
									.context("decode hibernatable connection state delta")?,
							);
						}
						StateDelta::ConnHibernationRemoved(conn) => {
							connections_to_delete.push(conn);
						}
					}
				}

				if next_state.is_some() {
					actor_to_persist = Some(persisted.clone());
				}

				Some((
					next_state,
					actor_to_persist,
					connections_to_persist,
					connections_to_delete,
					revision,
					self.begin_write(),
				))
			}
		};
		let Some((
			next_state,
			actor_to_persist,
			connections_to_persist,
			connections_to_delete,
			revision,
			_write_guard,
		)) = prepared
		else {
			tracing::debug!(
				save_request_revision,
				"discarding state serialized across a state transaction boundary"
			);
			return Ok(false);
		};

		if let Some(workflow_writes) = workflow_writes.as_deref() {
			internal_storage::persist_actor_core_connections_and_workflow(
				self.sql(),
				actor_to_persist.as_ref(),
				&connections_to_persist,
				&connections_to_delete,
				workflow_writes,
			)
			.await
			.context("atomically persist actor state, connection deltas, and workflow kv")?;
		} else if actor_to_persist.is_some()
			|| !connections_to_persist.is_empty()
			|| !connections_to_delete.is_empty()
		{
			internal_storage::persist_actor_core_and_connections(
				self.sql(),
				actor_to_persist.as_ref(),
				&connections_to_persist,
				&connections_to_delete,
			)
			.await
			.context("persist actor state and connection deltas to sqlite")?;
		}

		if let Some(state) = next_state {
			self.0.persisted.write().state = state.clone();
			*self.0.current_state.write() = state;
		}
		for connection in &connections_to_persist {
			if let Some(handle) = self.connection(&connection.id) {
				handle.set_state_initial(connection.state.clone());
			}
		}

		*self.0.last_save_at.lock() = Some(StdInstant::now());

		if self.0.state_revision.load(Ordering::SeqCst) == revision {
			self.0.state_dirty.store(false, Ordering::SeqCst);
		}

		self.mark_save_request_completed(save_request_revision);
		self.finish_save_request(save_request_revision);
		tracing::debug!(
			delta_count,
			delta_bytes,
			workflow_write_count,
			state_revision = self.0.state_revision.load(Ordering::SeqCst),
			save_request_revision,
			"actor state deltas applied"
		);
		Ok(true)
	}
	async fn commit_state_transaction(
		&self,
		transaction: &SqliteTransaction,
		deltas: Vec<StateDelta>,
		save_request_revision: u64,
		statement_count: usize,
		bind_payload_bytes: usize,
	) -> Result<()> {
		let (deltas, pending_hibernation_changes) = match self.prepare_state_deltas(deltas) {
			Ok(prepared) => prepared,
			Err(error) => {
				let _ = transaction.rollback().await;
				return Err(error);
			}
		};
		let commit_result = async {
			let revision = self.0.state_revision.load(Ordering::SeqCst);
			let mut persisted = self.persisted();
			let mut next_state = None;
			let mut actor_to_persist = None;
			let mut connections_to_persist: Vec<PersistedConnection> = Vec::new();
			let mut connections_to_delete = Vec::new();

			for delta in deltas {
				match delta {
					StateDelta::ActorState(bytes) => {
						next_state = Some(bytes.clone());
						persisted.state = bytes;
					}
					StateDelta::ConnHibernation { conn: _, bytes } => {
						connections_to_persist.push(
							decode_persisted_connection(&bytes)
								.context("decode hibernatable connection state delta")?,
						);
					}
					StateDelta::ConnHibernationRemoved(conn) => {
						connections_to_delete.push(conn);
					}
				}
			}

			if next_state.is_some() {
				actor_to_persist = Some(persisted);
			}
			let statements = internal_storage::build_actor_core_and_connection_statements(
				actor_to_persist.as_ref(),
				&connections_to_persist,
				&connections_to_delete,
			)?;
			internal_storage::validate_atomic_state_transaction_budget(
				statement_count.saturating_add(statements.len()),
				bind_payload_bytes
					.saturating_add(internal_storage::statement_bind_payload_len(&statements)),
			)?;
			for statement in statements {
				transaction
					.execute(statement.sql, statement.params)
					.await
					.context("persist actor state inside sqlite transaction")?;
			}
			transaction
				.commit()
				.await
				.context("commit sqlite transaction with actor state")?;

			if let Some(state) = next_state {
				self.0.persisted.write().state = state.clone();
				*self.0.current_state.write() = state;
			}
			for connection in &connections_to_persist {
				if let Some(handle) = self.connection(&connection.id) {
					handle.set_state_initial(connection.state.clone());
				}
			}
			*self.0.last_save_at.lock() = Some(StdInstant::now());
			if self.0.state_revision.load(Ordering::SeqCst) == revision {
				self.0.state_dirty.store(false, Ordering::SeqCst);
			}
			self.mark_save_request_completed(save_request_revision);
			self.finish_save_request(save_request_revision);
			self.record_state_updated();
			Ok(())
		}
		.await;

		if let Err(error) = commit_result {
			self.restore_pending_hibernation_changes(pending_hibernation_changes);
			let _ = transaction.rollback().await;
			return Err(error);
		}
		Ok(())
	}

	pub(crate) async fn wait_for_pending_writes(&self) {
		loop {
			if let Some(handle) = self.take_tracked_persist() {
				let _ = handle.await;
				continue;
			}

			let save_guard = self.0.save_guard.lock().await;
			if self.has_tracked_persist() {
				drop(save_guard);
				continue;
			}

			if self.0.in_flight_state_writes.load(Ordering::SeqCst) == 0 {
				return;
			}
			drop(save_guard);

			self.wait_for_in_flight_writes().await;
		}
	}

	pub(crate) async fn wait_for_pending_state_writes(&self) {
		self.wait_for_pending_writes().await;
	}

	pub fn begin_on_state_change(&self) -> OnStateChangeGuard {
		OnStateChangeGuard::new(self.clone())
	}

	pub fn on_state_change_started(&self) {
		self.0
			.on_state_change_in_flight
			.fetch_add(1, Ordering::SeqCst);
		self.0.sleep.work.keep_awake.increment();
		self.reset_sleep_timer();
	}

	pub fn on_state_change_finished(&self) {
		let previous = self.0.on_state_change_in_flight.fetch_update(
			Ordering::SeqCst,
			Ordering::SeqCst,
			|count| count.checked_sub(1),
		);

		match previous {
			Ok(1) => {
				self.0.sleep.work.keep_awake.decrement();
				self.0.on_state_change_idle.notify_waiters();
				self.reset_sleep_timer();
			}
			Ok(_) => {
				self.0.sleep.work.keep_awake.decrement();
				self.reset_sleep_timer();
			}
			Err(_) => {
				tracing::warn!(
					actor_id = %self.actor_id(),
					"on_state_change finished without a matching start"
				);
			}
		}
	}

	#[cfg(test)]
	#[allow(dead_code)]
	pub(crate) async fn wait_for_on_state_change_idle(&self, timeout_duration: Duration) -> bool {
		if self.0.on_state_change_in_flight.load(Ordering::SeqCst) == 0 {
			return true;
		}

		timeout(timeout_duration, async {
			loop {
				let idle = self.0.on_state_change_idle.notified();
				tokio::pin!(idle);
				idle.as_mut().enable();

				if self.0.on_state_change_in_flight.load(Ordering::SeqCst) == 0 {
					return;
				}

				idle.await;
			}
		})
		.await
		.is_ok()
	}

	pub fn persisted(&self) -> PersistedActor {
		self.0.persisted.read().clone()
	}

	pub fn load_persisted(&self, persisted: PersistedActor) {
		let state = persisted.state.clone();
		*self.0.persisted.write() = persisted;
		*self.0.current_state.write() = state;
		self.0.state_dirty.store(false, Ordering::SeqCst);
		self.finish_save_request(self.save_request_revision());
		self.0
			.metrics
			.inc_state_mutation(StateMutationReason::InternalReplace);
	}

	pub(crate) fn load_last_pushed_alarm(&self, alarm_ts: Option<i64>) {
		*self.0.last_pushed_alarm.write() = alarm_ts;
	}

	pub(crate) fn last_pushed_alarm(&self) -> Option<i64> {
		*self.0.last_pushed_alarm.read()
	}

	pub(crate) async fn persist_last_pushed_alarm(&self, alarm_ts: Option<i64>) -> Result<()> {
		internal_storage::persist_last_pushed_alarm(self.sql(), alarm_ts)
			.await
			.context("persist last pushed alarm to sqlite")?;
		self.load_last_pushed_alarm(alarm_ts);
		Ok(())
	}

	pub(crate) fn load_run_wake_at(&self, wake_at: Option<i64>) {
		*self.0.run_wake_at.write() = wake_at;
	}

	pub(crate) fn run_wake_at(&self) -> Option<i64> {
		*self.0.run_wake_at.read()
	}

	pub(crate) async fn persist_run_wake_at(&self, wake_at: Option<i64>) -> Result<()> {
		internal_storage::persist_run_wake_at(self.sql(), wake_at)
			.await
			.context("persist run wake deadline to sqlite")?;
		self.load_run_wake_at(wake_at);
		Ok(())
	}

	pub(crate) fn set_initial_state(&self, state: Vec<u8>) {
		*self.0.current_state.write() = state.clone();
		self.0.persisted.write().state = state;
		self.0.state_dirty.store(true, Ordering::SeqCst);
		self.0.state_revision.fetch_add(1, Ordering::SeqCst);
	}

	pub fn scheduled_events(&self) -> Vec<PersistedScheduleEvent> {
		self.0.persisted.read().scheduled_events.clone()
	}

	pub fn set_scheduled_events(&self, scheduled_events: Vec<PersistedScheduleEvent>) {
		self.0.persisted.write().scheduled_events = scheduled_events;
		self.0
			.metrics
			.inc_state_mutation(StateMutationReason::ScheduledEventsUpdate);
		self.mark_dirty();
		self.schedule_save(None);
	}

	pub fn set_input(&self, input: Option<Vec<u8>>) {
		self.0.persisted.write().input = input;
		self.0
			.metrics
			.inc_state_mutation(StateMutationReason::InputSet);
		self.mark_dirty();
		self.schedule_save(None);
	}

	pub fn input(&self) -> Option<Vec<u8>> {
		self.0.persisted.read().input.clone()
	}

	pub fn set_has_initialized(&self, has_initialized: bool) {
		{
			let mut persisted = self.0.persisted.write();
			if persisted.has_initialized == has_initialized {
				return;
			}
			persisted.has_initialized = has_initialized;
		}
		self.0
			.metrics
			.inc_state_mutation(StateMutationReason::HasInitialized);
		self.mark_dirty();
		self.schedule_save(None);
	}

	pub fn has_initialized(&self) -> bool {
		self.0.persisted.read().has_initialized
	}

	pub fn flush_on_shutdown(&self) {
		self.persist_now_tracked("shutdown_flush");
	}

	pub fn on_request_save(&self, hook: Box<dyn Fn(RequestSaveOpts) + Send + Sync>) {
		self.0.request_save_hooks.write().push(Arc::from(hook));
	}

	fn is_dirty(&self) -> bool {
		self.0.state_dirty.load(Ordering::SeqCst)
	}

	fn mark_dirty(&self) {
		self.0.state_dirty.store(true, Ordering::SeqCst);
		self.0.state_revision.fetch_add(1, Ordering::SeqCst);
	}

	fn lifecycle_event_sender(&self) -> Option<mpsc::UnboundedSender<LifecycleEvent>> {
		self.0.lifecycle_events.read().clone()
	}

	fn compute_save_delay(&self, max_wait: Option<Duration>) -> Duration {
		let elapsed = self
			.0
			.last_save_at
			.lock()
			.map(|instant| instant.elapsed())
			.unwrap_or_default();

		throttled_save_delay(self.0.state_save_interval, elapsed, max_wait)
	}

	fn schedule_save(&self, max_wait: Option<Duration>) {
		if !self.is_dirty() {
			return;
		}

		let delay = self.compute_save_delay(max_wait);
		let scheduled_at = StdInstant::now() + delay;

		let mut pending_save = self.0.pending_save.lock();

		if let Some(existing) = pending_save.as_ref() {
			if existing.scheduled_at <= scheduled_at {
				return;
			}

			existing.handle.abort();
		}

		let state = self.clone();
		// Intentionally detached but abortable: pending delayed saves are
		// retained in `pending_save`, replaced by newer saves, and awaited at
		// shutdown through the state save guard.
		let task = async move {
			if !delay.is_zero() {
				sleep(delay).await;
			}

			state.take_pending_save();

			if let Err(error) = state.persist_if_dirty().await {
				tracing::error!(?error, "failed to persist actor state");
			}
		}
		.in_current_span();

		#[cfg(not(feature = "wasm-runtime"))]
		let handle = {
			let Ok(tokio_handle) = Handle::try_current() else {
				return;
			};
			tokio_handle.spawn(task)
		};

		#[cfg(feature = "wasm-runtime")]
		let handle = RuntimeSpawner::spawn(task);

		*pending_save = Some(PendingSave {
			scheduled_at,
			handle,
		});
	}

	fn clear_pending_save(&self) {
		if let Some(pending_save) = self.take_pending_save() {
			pending_save.handle.abort();
		}
	}

	pub(crate) fn persist_now_tracked(&self, description: &'static str) {
		self.clear_pending_save();

		let state = self.clone();
		let mut tracked_persist = self.0.tracked_persist.lock();
		let previous = tracked_persist.take();
		let task = async move {
			if let Some(previous) = previous {
				let _ = previous.await;
			}

			if let Err(error) = state.persist_state(SaveStateOpts { immediate: true }).await {
				tracing::error!(?error, description, "failed to persist actor state");
			}
		}
		.in_current_span();

		#[cfg(not(feature = "wasm-runtime"))]
		let handle = {
			let Ok(tokio_handle) = Handle::try_current() else {
				tracing::warn!(
					description,
					"skipping tracked actor state persistence without runtime"
				);
				return;
			};
			tokio_handle.spawn(task)
		};

		#[cfg(feature = "wasm-runtime")]
		let handle = RuntimeSpawner::spawn(task);

		*tracked_persist = Some(handle);
	}

	fn take_pending_save(&self) -> Option<PendingSave> {
		self.0.pending_save.lock().take()
	}

	fn take_tracked_persist(&self) -> Option<JoinHandle<()>> {
		self.0.tracked_persist.lock().take()
	}

	fn has_tracked_persist(&self) -> bool {
		self.0.tracked_persist.lock().is_some()
	}

	#[cfg(test)]
	pub(crate) fn tracked_persist_pending(&self) -> bool {
		self.has_tracked_persist()
	}

	async fn persist_if_dirty(&self) -> Result<()> {
		if !self.is_dirty() {
			return Ok(());
		}

		let (revision, actor_to_persist, _write_guard) = {
			let _save_guard = self.0.save_guard.lock().await;
			if !self.is_dirty() {
				return Ok(());
			}

			let revision = self.0.state_revision.load(Ordering::SeqCst);
			let persisted = self.persisted();
			(revision, persisted, self.begin_write())
		};

		internal_storage::persist_actor_snapshot(self.sql(), &actor_to_persist)
			.await
			.context("persist actor state to sqlite")?;

		*self.0.last_save_at.lock() = Some(StdInstant::now());

		if self.0.state_revision.load(Ordering::SeqCst) == revision {
			self.0.state_dirty.store(false, Ordering::SeqCst);
		}

		Ok(())
	}

	fn begin_write(&self) -> InFlightWrite {
		self.0.in_flight_state_writes.fetch_add(1, Ordering::SeqCst);
		InFlightWrite { ctx: self.clone() }
	}

	async fn wait_for_in_flight_writes(&self) {
		loop {
			if self.0.in_flight_state_writes.load(Ordering::SeqCst) == 0 {
				return;
			}
			self.0.state_write_completion.notified().await;
		}
	}

	fn finish_save_request(&self, save_request_revision: u64) {
		if self.0.save_request_revision.load(Ordering::SeqCst) == save_request_revision {
			self.0.save_requested.store(false, Ordering::SeqCst);
			self.0
				.save_requested_immediate
				.store(false, Ordering::SeqCst);
			*self.0.save_requested_within_deadline.lock() = None;
		}
	}

	fn mark_save_request_completed(&self, save_request_revision: u64) {
		self.0
			.save_completed_revision
			.fetch_max(save_request_revision, Ordering::SeqCst);
		self.0.save_completion.notify_waiters();
	}

	fn notify_request_save_hooks(&self, opts: RequestSaveOpts) {
		let hooks = self.0.request_save_hooks.read().clone();
		for hook in hooks {
			hook(opts);
		}
	}
}

struct InFlightWrite {
	ctx: ActorContext,
}

impl Drop for InFlightWrite {
	fn drop(&mut self) {
		if self
			.ctx
			.0
			.in_flight_state_writes
			.fetch_sub(1, Ordering::SeqCst)
			== 1
		{
			self.ctx.0.state_write_completion.notify_waiters();
			self.ctx.0.state_write_completion.notify_one();
		}
	}
}

fn throttled_save_delay(
	save_interval: Duration,
	time_since_last_save: Duration,
	max_wait: Option<Duration>,
) -> Duration {
	let save_delay = save_interval.saturating_sub(time_since_last_save);
	if let Some(max_wait) = max_wait {
		save_delay.min(max_wait)
	} else {
		save_delay
	}
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/state.rs"]
mod tests;
