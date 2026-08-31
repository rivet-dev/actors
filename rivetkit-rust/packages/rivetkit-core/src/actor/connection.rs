use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use parking_lot::{RwLock, RwLockReadGuard};
use rivet_error::RivetError;
use rivetkit_actor_persist::{generated::v4 as persist_v4, versioned as persist_versioned};
use serde::Serialize;
use uuid::Uuid;

use tokio::sync::oneshot;

use crate::actor::config::ActorConfig;
use crate::actor::context::ActorContext;
use crate::actor::internal_storage;
use crate::actor::lifecycle_hooks::Reply;
use crate::actor::messages::{ActorEvent, Request};
use crate::actor::persist::{
	decode_latest_with_embedded_version, encode_latest_with_embedded_version,
};
use crate::actor::state::RequestSaveOpts;
use crate::error::ActorRuntime;
use crate::time::timeout;
use crate::types::ConnId;

pub(crate) type EventSendCallback = Arc<dyn Fn(OutgoingEvent) -> Result<()> + Send + Sync>;
pub(crate) type DisconnectCallback =
	Arc<dyn Fn(Option<String>) -> BoxFuture<'static, Result<()>> + Send + Sync>;
type StateChangeCallback = Arc<dyn Fn(&ConnHandle) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutgoingEvent {
	pub name: String,
	pub args: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HibernatableConnectionMetadata {
	pub gateway_id: [u8; 4],
	pub request_id: [u8; 4],
	pub server_message_index: u16,
	pub client_message_index: u16,
	pub request_path: String,
	pub request_headers: BTreeMap<String, String>,
}

pub(crate) type PersistedSubscription = persist_v4::Subscription;
pub(crate) type PersistedConnection = persist_v4::Conn;

#[derive(RivetError, Serialize)]
#[error(
	"actor",
	"invalid_request",
	"Invalid hibernatable websocket connection ID",
	"Hibernatable websocket {field} must be exactly 4 bytes, got {actual_len}."
)]
struct InvalidHibernatableConnectionId {
	field: String,
	actual_len: usize,
}

#[derive(RivetError, Serialize)]
#[error(
	"connection",
	"not_configured",
	"Connection callback is not configured",
	"Connection {component} is not configured."
)]
struct ConnectionNotConfigured {
	component: String,
}

#[derive(RivetError, Serialize)]
#[error(
	"connection",
	"not_found",
	"Connection was not found",
	"Connection '{conn_id}' was not found."
)]
struct ConnectionNotFound {
	conn_id: String,
}

#[derive(RivetError, Serialize)]
#[error(
	"connection",
	"not_hibernatable",
	"Connection is not hibernatable",
	"Connection '{conn_id}' is not hibernatable."
)]
struct ConnectionNotHibernatable {
	conn_id: String,
}

#[derive(RivetError, Serialize)]
#[error(
	"connection",
	"restore_not_found",
	"Hibernatable connection restore target was not found"
)]
struct ConnectionRestoreNotFound;

#[derive(RivetError, Serialize)]
#[error(
	"connection",
	"disconnect_failed",
	"Connection disconnect failed",
	"Disconnect transport failed for {count} connection(s): {details}"
)]
struct ConnectionDisconnectFailed {
	count: usize,
	details: String,
}

pub(crate) fn hibernatable_id_from_slice(field: &'static str, bytes: &[u8]) -> Result<[u8; 4]> {
	bytes.try_into().map_err(|_| {
		InvalidHibernatableConnectionId {
			field: field.to_owned(),
			actual_len: bytes.len(),
		}
		.build()
	})
}

pub(crate) fn encode_persisted_connection(connection: &PersistedConnection) -> Result<Vec<u8>> {
	encode_latest_with_embedded_version::<persist_versioned::Conn>(
		connection.clone(),
		rivetkit_actor_persist::CURRENT_VERSION,
		"persisted connection",
	)
}

pub(crate) fn decode_persisted_connection(payload: &[u8]) -> Result<PersistedConnection> {
	let connection = decode_latest_with_embedded_version::<persist_versioned::Conn>(
		payload,
		"persisted connection",
	)?;
	Ok(connection)
}

#[derive(Clone)]
pub struct ConnHandle(Arc<ConnHandleInner>);

struct ConnHandleInner {
	id: ConnId,
	params: Vec<u8>,
	// Forced-sync: connection handles expose synchronous state and callback
	// methods to foreign runtimes; callbacks are cloned before async work.
	state: RwLock<Vec<u8>>,
	is_hibernatable: bool,
	dirty: AtomicBool,
	subscriptions: RwLock<BTreeSet<String>>,
	hibernation: RwLock<Option<HibernatableConnectionMetadata>>,
	state_change_handler: RwLock<Option<StateChangeCallback>>,
	event_sender: RwLock<Option<EventSendCallback>>,
	transport_disconnect_handler: RwLock<Option<DisconnectCallback>>,
	disconnect_handler: RwLock<Option<DisconnectCallback>>,
}

impl ConnHandle {
	pub fn new(
		id: impl Into<ConnId>,
		params: Vec<u8>,
		state: Vec<u8>,
		is_hibernatable: bool,
	) -> Self {
		Self(Arc::new(ConnHandleInner {
			id: id.into(),
			params,
			state: RwLock::new(state),
			is_hibernatable,
			dirty: AtomicBool::new(false),
			subscriptions: RwLock::new(BTreeSet::new()),
			hibernation: RwLock::new(None),
			state_change_handler: RwLock::new(None),
			event_sender: RwLock::new(None),
			transport_disconnect_handler: RwLock::new(None),
			disconnect_handler: RwLock::new(None),
		}))
	}

	pub fn id(&self) -> &str {
		&self.0.id
	}

	pub fn params(&self) -> Vec<u8> {
		self.0.params.clone()
	}

	pub fn state(&self) -> Vec<u8> {
		self.0.state.read().clone()
	}

	pub fn set_state(&self, state: Vec<u8>) {
		self.set_state_inner(state, true);
	}

	#[doc(hidden)]
	pub fn set_state_initial(&self, state: Vec<u8>) {
		self.set_state_inner(state, false);
		self.clear_hibernation_dirty();
	}

	fn set_state_inner(&self, state: Vec<u8>, mark_dirty: bool) {
		*self.0.state.write() = state;
		if mark_dirty {
			self.mark_hibernation_dirty();
		}
	}

	fn mark_hibernation_dirty(&self) {
		if !self.is_hibernatable() {
			return;
		}
		self.0.dirty.store(true, Ordering::SeqCst);
		let handler = self.0.state_change_handler.read().clone();
		if let Some(handler) = handler {
			handler(self);
		}
	}

	pub(crate) fn clear_hibernation_dirty(&self) {
		self.0.dirty.store(false, Ordering::SeqCst);
	}

	pub fn is_hibernatable(&self) -> bool {
		self.0.is_hibernatable
	}

	pub fn send(&self, name: &str, args: &[u8]) {
		if let Err(error) = self.try_send(name, args) {
			tracing::error!(
				?error,
				conn_id = self.id(),
				event_name = name,
				"failed to send event to connection"
			);
		}
	}

	pub async fn disconnect(&self, reason: Option<&str>) -> Result<()> {
		if let Some(handler) = self.transport_disconnect_handler() {
			handler(reason.map(str::to_owned)).await?;
		}
		let handler = self.disconnect_handler()?;
		handler(reason.map(str::to_owned)).await
	}

	pub(crate) fn configure_event_sender(&self, event_sender: Option<EventSendCallback>) {
		*self.0.event_sender.write() = event_sender;
	}

	pub(crate) fn configure_disconnect_handler(
		&self,
		disconnect_handler: Option<DisconnectCallback>,
	) {
		*self.0.disconnect_handler.write() = disconnect_handler;
	}

	pub(crate) fn configure_transport_disconnect_handler(
		&self,
		disconnect_handler: Option<DisconnectCallback>,
	) {
		*self.0.transport_disconnect_handler.write() = disconnect_handler;
	}

	pub(crate) fn subscribe(&self, event_name: impl Into<String>) -> bool {
		self.0.subscriptions.write().insert(event_name.into())
	}

	pub(crate) fn unsubscribe(&self, event_name: &str) -> bool {
		self.0.subscriptions.write().remove(event_name)
	}

	pub(crate) fn is_subscribed(&self, event_name: &str) -> bool {
		self.0.subscriptions.read().contains(event_name)
	}

	pub(crate) fn subscriptions(&self) -> Vec<String> {
		self.0.subscriptions.read().iter().cloned().collect()
	}

	pub(crate) fn clear_subscriptions(&self) {
		self.0.subscriptions.write().clear();
	}

	pub(crate) fn configure_hibernation(
		&self,
		hibernation: Option<HibernatableConnectionMetadata>,
	) {
		*self.0.hibernation.write() = hibernation;
	}

	pub(crate) fn hibernation(&self) -> Option<HibernatableConnectionMetadata> {
		self.0.hibernation.read().clone()
	}

	pub(crate) fn configure_state_change_handler(&self, handler: Option<StateChangeCallback>) {
		*self.0.state_change_handler.write() = handler;
	}

	pub(crate) fn set_server_message_index(
		&self,
		message_index: u16,
	) -> Option<HibernatableConnectionMetadata> {
		let mut hibernation = self.0.hibernation.write();
		let hibernation = hibernation.as_mut()?;
		hibernation.server_message_index = message_index;
		Some(hibernation.clone())
	}

	pub(crate) fn persisted_with_state(&self, state: Vec<u8>) -> Option<PersistedConnection> {
		let hibernation = self.0.hibernation.read().clone()?;

		Some(PersistedConnection {
			id: self.id().to_owned(),
			parameters: self.params(),
			state,
			subscriptions: self
				.subscriptions()
				.into_iter()
				.map(|event_name| PersistedSubscription { event_name })
				.collect(),
			gateway_id: hibernation.gateway_id,
			request_id: hibernation.request_id,
			server_message_index: hibernation.server_message_index,
			client_message_index: hibernation.client_message_index,
			request_path: hibernation.request_path,
			request_headers: hibernation.request_headers.into_iter().collect(),
		})
	}

	pub(crate) fn from_persisted(persisted: PersistedConnection) -> Self {
		let conn = Self::new(
			persisted.id.clone(),
			persisted.parameters,
			persisted.state,
			true,
		);
		conn.configure_hibernation(Some(HibernatableConnectionMetadata {
			gateway_id: persisted.gateway_id,
			request_id: persisted.request_id,
			server_message_index: persisted.server_message_index,
			client_message_index: persisted.client_message_index,
			request_path: persisted.request_path,
			request_headers: persisted.request_headers.into_iter().collect(),
		}));
		for subscription in persisted.subscriptions {
			conn.subscribe(subscription.event_name);
		}
		conn.clear_hibernation_dirty();
		conn
	}

	pub(crate) fn try_send(&self, name: &str, args: &[u8]) -> Result<()> {
		let event_sender = self.event_sender()?;
		event_sender(OutgoingEvent {
			name: name.to_owned(),
			args: args.to_vec(),
		})
	}

	fn event_sender(&self) -> Result<EventSendCallback> {
		self.0
			.event_sender
			.read()
			.clone()
			.ok_or_else(|| connection_not_configured("event sender"))
	}

	fn disconnect_handler(&self) -> Result<DisconnectCallback> {
		self.0
			.disconnect_handler
			.read()
			.clone()
			.ok_or_else(|| connection_not_configured("disconnect handler"))
	}

	pub(crate) fn managed_disconnect_handler(&self) -> Result<DisconnectCallback> {
		self.disconnect_handler()
	}

	pub(crate) async fn disconnect_transport_only(&self) -> Result<()> {
		let Some(handler) = self.transport_disconnect_handler() else {
			return Ok(());
		};
		handler(None).await
	}

	fn transport_disconnect_handler(&self) -> Option<DisconnectCallback> {
		self.0.transport_disconnect_handler.read().clone()
	}
}

impl Default for ConnHandle {
	fn default() -> Self {
		Self::new("", Vec::new(), Vec::new(), false)
	}
}

impl fmt::Debug for ConnHandle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ConnHandle")
			.field("id", &self.0.id)
			.field("is_hibernatable", &self.0.is_hibernatable)
			.field("subscriptions", &self.subscriptions())
			.finish()
	}
}

#[derive(Default)]
pub(crate) struct PendingHibernationChanges {
	pub updated: BTreeSet<ConnId>,
	pub removed: BTreeSet<ConnId>,
}

/// Lock-backed iterator over live connection handles.
///
/// Do not hold this iterator across `.await`. It keeps a read lock on the
/// connection map until dropped, which blocks writers such as add/remove or
/// connection reconfiguration.
#[must_use = "connection iterators hold a read lock until dropped"]
pub struct ConnHandles<'a> {
	guard: RwLockReadGuard<'a, BTreeMap<ConnId, ConnHandle>>,
	next_after: Option<ConnId>,
}

impl<'a> ConnHandles<'a> {
	fn new(guard: RwLockReadGuard<'a, BTreeMap<ConnId, ConnHandle>>) -> Self {
		Self {
			guard,
			next_after: None,
		}
	}

	pub fn len(&self) -> usize {
		self.guard.len()
	}

	pub fn is_empty(&self) -> bool {
		self.guard.is_empty()
	}
}

impl Iterator for ConnHandles<'_> {
	type Item = ConnHandle;

	fn next(&mut self) -> Option<Self::Item> {
		let (conn_id, conn) = match self.next_after.as_ref() {
			Some(conn_id) => self
				.guard
				.range((Excluded(conn_id.clone()), Unbounded))
				.next()?,
			None => self.guard.iter().next()?,
		};
		self.next_after = Some(conn_id.clone());
		Some(conn.clone())
	}
}

impl ActorContext {
	pub(crate) fn configure_connection_storage(&self, config: ActorConfig) {
		*self.0.connection_config.write() = config;
	}

	pub(crate) fn iter_connections(&self) -> ConnHandles<'_> {
		ConnHandles::new(self.0.connections.read())
	}

	pub(crate) fn active_connection_count(&self) -> u32 {
		self.0
			.connections
			.read()
			.len()
			.try_into()
			.unwrap_or(u32::MAX)
	}

	pub(crate) fn insert_existing(&self, conn: ConnHandle) {
		let conn_id = conn.id().to_owned();
		let is_hibernatable = conn.is_hibernatable();
		let active_count = {
			let mut connections = self.0.connections.write();
			connections.insert(conn_id.clone(), conn);
			connections.len()
		};
		self.0.metrics.set_active_connections(active_count);
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id = %conn_id,
			is_hibernatable,
			active_count,
			"connection added"
		);
	}

	pub(crate) fn remove_existing(&self, conn_id: &str) -> Option<ConnHandle> {
		let (removed, active_count) = {
			let mut connections = self.0.connections.write();
			let removed = connections.remove(conn_id);
			(removed, connections.len())
		};
		self.0.metrics.set_active_connections(active_count);
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id,
			removed = removed.is_some(),
			active_count,
			"connection removed"
		);
		removed
	}

	fn remove_existing_for_disconnect(&self, conn_id: &str) -> Option<ConnHandle> {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		let (removed, active_count) = {
			let mut connections = self.0.connections.write();
			let removed = connections.remove(conn_id)?;

			if removed.is_hibernatable() {
				self.0.pending_hibernation_updates.write().remove(conn_id);
				self.0
					.pending_hibernation_removals
					.write()
					.insert(conn_id.to_owned());
			}

			(removed, connections.len())
		};
		self.0.metrics.set_active_connections(active_count);
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id,
			is_hibernatable = removed.is_hibernatable(),
			active_count,
			"connection removed for disconnect"
		);
		Some(removed)
	}

	pub(crate) fn queue_hibernation_update(&self, conn_id: impl Into<ConnId>) {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		let conn_id = conn_id.into();
		self.0
			.pending_hibernation_updates
			.write()
			.insert(conn_id.clone());
		self.0.pending_hibernation_removals.write().remove(&conn_id);
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id = %conn_id,
			"hibernatable connection transport queued for save"
		);
	}

	pub(crate) fn dirty_hibernatable_conns_inner(&self) -> Vec<ConnHandle> {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		let update_ids: Vec<_> = self
			.0
			.pending_hibernation_updates
			.read()
			.iter()
			.cloned()
			.collect();
		let connections = self.0.connections.read();
		update_ids
			.into_iter()
			.filter_map(|conn_id| connections.get(&conn_id).cloned())
			.filter(|conn| conn.is_hibernatable() && conn.hibernation().is_some())
			.collect()
	}

	pub(crate) fn queue_hibernation_removal_inner(&self, conn_id: impl Into<ConnId>) {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		let conn_id = conn_id.into();
		self.0.pending_hibernation_updates.write().remove(&conn_id);
		self.0
			.pending_hibernation_removals
			.write()
			.insert(conn_id.clone());
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id = %conn_id,
			"hibernatable connection transport queued for removal"
		);
	}

	pub(crate) fn take_pending_hibernation_changes_inner(&self) -> PendingHibernationChanges {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		PendingHibernationChanges {
			updated: std::mem::take(&mut *self.0.pending_hibernation_updates.write()),
			removed: std::mem::take(&mut *self.0.pending_hibernation_removals.write()),
		}
	}

	pub(crate) fn pending_hibernation_removals(&self) -> Vec<ConnId> {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		self.0
			.pending_hibernation_removals
			.read()
			.iter()
			.cloned()
			.collect()
	}

	pub(crate) fn has_pending_hibernation_changes_inner(&self) -> bool {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		let has_updates = !self.0.pending_hibernation_updates.read().is_empty();
		let has_removals = !self.0.pending_hibernation_removals.read().is_empty();
		has_updates || has_removals
	}

	pub(crate) fn restore_pending_hibernation_changes(&self, pending: PendingHibernationChanges) {
		let _disconnect_state = self.0.connection_disconnect_state.lock();
		if !pending.updated.is_empty() {
			self.0
				.pending_hibernation_updates
				.write()
				.extend(pending.updated);
		}
		if !pending.removed.is_empty() {
			self.0
				.pending_hibernation_removals
				.write()
				.extend(pending.removed);
		}
	}

	pub(crate) async fn connect_with_state<F>(
		&self,
		params: Vec<u8>,
		is_hibernatable: bool,
		hibernation: Option<HibernatableConnectionMetadata>,
		request: Option<Request>,
		create_state: F,
	) -> Result<ConnHandle>
	where
		F: std::future::Future<Output = Result<Vec<u8>>> + Send,
	{
		self.connect_with_state_and_prepare(
			params,
			is_hibernatable,
			hibernation,
			request,
			create_state,
			|_| Ok(()),
		)
		.await
	}

	pub(crate) async fn connect_with_state_and_prepare<F, P>(
		&self,
		params: Vec<u8>,
		is_hibernatable: bool,
		hibernation: Option<HibernatableConnectionMetadata>,
		request: Option<Request>,
		create_state: F,
		prepare_connection: P,
	) -> Result<ConnHandle>
	where
		F: std::future::Future<Output = Result<Vec<u8>>> + Send,
		P: FnOnce(&ConnHandle) -> Result<()>,
	{
		let config = self.connection_config();

		let state = timeout(config.create_conn_state_timeout, create_state)
			.await
			.with_context(|| {
				timeout_message("create_conn_state", config.create_conn_state_timeout)
			})??;

		let conn = ConnHandle::new(
			Uuid::new_v4().to_string(),
			params.clone(),
			state,
			is_hibernatable,
		);
		conn.configure_hibernation(hibernation);
		self.prepare_managed_conn(&conn);

		if let Err(error) = prepare_connection(&conn) {
			return Err(error);
		}

		self.emit_connection_preflight(&conn, params.clone(), request.clone())
			.await?;
		self.insert_existing(conn.clone());

		if let Err(error) = self.emit_connection_open(&conn, request).await {
			self.remove_existing(conn.id());
			return Err(error);
		}
		self.0.metrics.inc_connections_total();
		self.record_connections_updated();
		self.reset_sleep_timer();

		Ok(conn)
	}

	pub(crate) fn encode_hibernation_delta(
		&self,
		conn_id: &str,
		bytes: Vec<u8>,
	) -> Result<Vec<u8>> {
		let conn = self.connection(conn_id).ok_or_else(|| {
			ConnectionNotFound {
				conn_id: conn_id.to_owned(),
			}
			.build()
		})?;
		let persisted = conn.persisted_with_state(bytes).ok_or_else(|| {
			ConnectionNotHibernatable {
				conn_id: conn_id.to_owned(),
			}
			.build()
		})?;
		encode_persisted_connection(&persisted).context("encode persisted connection")
	}

	pub(crate) async fn restore_persisted(&self) -> Result<Vec<ConnHandle>> {
		let persisted_connections = internal_storage::load_connections(self.sql())
			.await
			.context("load hibernatable connections from sqlite")?;
		let mut restored = Vec::new();

		for persisted in persisted_connections {
			let conn = ConnHandle::from_persisted(persisted);
			self.prepare_managed_conn(&conn);
			self.insert_existing(conn.clone());
			tracing::debug!(
				actor_id = %self.actor_id(),
				conn_id = conn.id(),
				"hibernatable connection restored"
			);
			restored.push(conn);
		}

		Ok(restored)
	}

	pub(crate) fn reconnect_hibernatable(
		&self,
		gateway_id: &[u8],
		request_id: &[u8],
	) -> Result<ConnHandle> {
		let gateway_id = hibernatable_id_from_slice("gateway_id", gateway_id)?;
		let request_id = hibernatable_id_from_slice("request_id", request_id)?;
		let Some(conn) = self
			.iter_connections()
			.find(|conn| match conn.hibernation() {
				Some(hibernation) => {
					hibernation.gateway_id == gateway_id && hibernation.request_id == request_id
				}
				None => false,
			})
		else {
			return Err(ConnectionRestoreNotFound.build());
		};

		self.record_connections_updated();
		self.reset_sleep_timer();
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id = conn.id(),
			"hibernatable connection transport restored"
		);
		Ok(conn)
	}

	fn prepare_managed_conn(&self, conn: &ConnHandle) {
		let ctx = self.downgrade();
		let conn_id = conn.id().to_owned();

		conn.configure_state_change_handler(Some(Arc::new({
			let ctx = ctx.clone();
			move |conn| {
				let Some(ctx) = ActorContext::from_weak(&ctx) else {
					tracing::warn!(
						conn_id = conn.id(),
						"skipping hibernatable connection state save without actor context"
					);
					return;
				};
				ctx.queue_hibernation_update(conn.id().to_owned());
				ctx.request_save(RequestSaveOpts::default());
			}
		})));

		conn.configure_disconnect_handler(Some(Arc::new(move |reason| {
			let ctx = ctx.clone();
			let conn_id = conn_id.clone();
			Box::pin(async move {
				let ctx = ActorContext::from_weak(&ctx).ok_or_else(|| {
					ActorRuntime::NotConfigured {
						component: "actor context".to_owned(),
					}
					.build()
				})?;
				ctx.with_disconnect_callback(|| async {
					ctx.disconnect_managed(&conn_id, reason).await
				})
				.await
			})
		})));
	}

	fn connection_config(&self) -> ActorConfig {
		self.0.connection_config.read().clone()
	}

	#[cfg(test)]
	pub(crate) fn connection_config_for_tests(&self) -> ActorConfig {
		self.connection_config()
	}

	async fn disconnect_managed(&self, conn_id: &str, reason: Option<String>) -> Result<()> {
		let Some(conn) = self.remove_existing_for_disconnect(conn_id) else {
			tracing::debug!(
				actor_id = %self.actor_id(),
				conn_id,
				reason = ?reason.as_deref(),
				"connection disconnect skipped because connection was already removed"
			);
			return Ok(());
		};
		conn.clear_subscriptions();

		self.try_send_actor_event(ActorEvent::ConnectionClosed { conn }, "connection_closed")
			.with_context(|| disconnect_message(conn_id, reason.as_deref()))?;

		self.record_connections_updated();
		self.reset_sleep_timer();
		tracing::debug!(
			actor_id = %self.actor_id(),
			conn_id,
			reason = ?reason.as_deref(),
			"connection disconnected"
		);
		Ok(())
	}

	async fn emit_connection_open(
		&self,
		conn: &ConnHandle,
		request: Option<Request>,
	) -> Result<()> {
		let config = self.connection_config();
		let (reply_tx, reply_rx) = oneshot::channel();
		self.try_send_actor_event(
			ActorEvent::ConnectionOpen {
				conn: conn.clone(),
				request,
				reply: Reply::from(reply_tx),
			},
			"connection_open",
		)?;
		timeout(config.on_connect_timeout, reply_rx)
			.await
			.with_context(|| timeout_message("connection_open", config.on_connect_timeout))?
			.context("receive connection_open reply")??;
		Ok(())
	}

	async fn emit_connection_preflight(
		&self,
		conn: &ConnHandle,
		params: Vec<u8>,
		request: Option<Request>,
	) -> Result<()> {
		let config = self.connection_config();
		let timeout_duration = config
			.on_before_connect_timeout
			.saturating_add(config.create_conn_state_timeout);
		let (reply_tx, reply_rx) = oneshot::channel();
		self.try_send_actor_event(
			ActorEvent::ConnectionPreflight {
				conn: conn.clone(),
				params,
				request,
				reply: Reply::from(reply_tx),
			},
			"connection_preflight",
		)?;
		timeout(timeout_duration, reply_rx)
			.await
			.with_context(|| timeout_message("connection_preflight", timeout_duration))?
			.context("receive connection_preflight reply")??;
		Ok(())
	}

	pub(crate) fn connection(&self, conn_id: &str) -> Option<ConnHandle> {
		self.0.connections.read().get(conn_id).cloned()
	}

	pub(crate) async fn disconnect_transport_only<F>(&self, mut predicate: F) -> Result<()>
	where
		F: FnMut(&ConnHandle) -> bool,
	{
		let connections: Vec<_> = self
			.iter_connections()
			.filter(|conn| predicate(conn))
			.collect();
		let mut disconnected_ids = Vec::new();
		let mut failures = Vec::new();

		for conn in &connections {
			match conn.disconnect_transport_only().await {
				Ok(()) => {
					tracing::debug!(
						actor_id = %self.actor_id(),
						conn_id = conn.id(),
						"connection transport disconnect completed"
					);
					disconnected_ids.push(conn.id().to_owned());
				}
				Err(error) => {
					tracing::error!(
						conn_id = %conn.id(),
						?error,
						"failed transport-only connection disconnect"
					);
					failures.push((conn.id().to_owned(), format!("{error:#}")));
				}
			}
		}

		let mut removed_any = false;
		for conn_id in disconnected_ids {
			let Some(conn) = self.remove_existing_for_disconnect(&conn_id) else {
				tracing::debug!(
					actor_id = %self.actor_id(),
					conn_id = %conn_id,
					"connection transport removal skipped because connection was already removed"
				);
				continue;
			};
			conn.clear_subscriptions();
			removed_any = true;
			tracing::debug!(
				actor_id = %self.actor_id(),
				conn_id = %conn_id,
				"connection transport removed"
			);
		}

		if removed_any {
			self.record_connections_updated();
			self.reset_sleep_timer();
		}

		if failures.is_empty() {
			return Ok(());
		}

		let count = failures.len();
		Err(ConnectionDisconnectFailed {
			count,
			details: failures
				.into_iter()
				.map(|(conn_id, error)| format!("{conn_id}: {error}"))
				.collect::<Vec<_>>()
				.join("; "),
		}
		.build())
	}
}

fn connection_not_configured(component: &str) -> anyhow::Error {
	ConnectionNotConfigured {
		component: component.to_owned(),
	}
	.build()
}

fn timeout_message(callback_name: &str, timeout: Duration) -> String {
	format!(
		"`{callback_name}` timed out after {} ms",
		timeout.as_millis()
	)
}

fn disconnect_message(conn_id: &str, reason: Option<&str>) -> String {
	match reason {
		Some(reason) => format!("disconnect connection `{conn_id}` with reason `{reason}`"),
		None => format!("disconnect connection `{conn_id}`"),
	}
}

// Test shim keeps moved tests in crate-root tests/ with private-module access.
#[cfg(test)]
#[path = "../../tests/connection.rs"]
mod tests;
