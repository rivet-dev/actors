pub mod action;
#[cfg(all(feature = "sqlite-local", unix))]
pub mod actor_runtime_socket;
#[cfg(all(feature = "sqlite-local", not(unix)))]
#[path = "actor_runtime_socket_unsupported.rs"]
pub mod actor_runtime_socket;
pub mod config;
pub mod connection;
pub mod context;
pub(crate) mod diagnostics;
pub mod factory;
pub(crate) mod internal_storage;
pub(crate) mod keys;
pub mod kv;
pub mod lifecycle_hooks;
pub mod messages;
pub mod metrics;
pub(crate) mod migrate_kv_to_sqlite;
pub mod persist;
pub mod queue;
pub mod schedule;
pub mod sleep;
#[cfg(test)]
#[path = "../../tests/sql_efficiency.rs"]
mod sql_efficiency;
pub mod sqlite;
pub mod state;
pub mod task;
pub mod task_types;
pub(crate) mod work_registry;

pub use action::ActionDispatchError;
#[cfg(feature = "sqlite-local")]
pub use actor_runtime_socket::ActorRuntimeSocketEndpointInfo;
pub use config::{ActionDefinition, ActorConfig, ActorConfigOverrides, CanHibernateWebSocket};
pub use connection::ConnHandle;
pub use context::{ActorContext, ActorWorkRegion, KeepAwakeRegion, WebSocketCallbackRegion};
pub use factory::{ActorEntryFn, ActorFactory};
pub use lifecycle_hooks::{ActorEvents, ActorStart, Reply};
pub use messages::{
	ActorEvent, QueueSendResult, QueueSendStatus, Request, Response, StateDelta, WorkflowKvWrite,
};
pub use queue::{
	CompletableQueueMessage, EnqueueAndWaitOpts, QueueMessage, QueueNextBatchOpts, QueueNextOpts,
	QueueTryNextBatchOpts, QueueTryNextOpts, QueueWaitOpts,
};
pub use sqlite::{
	BindParam, ColumnValue, ExecResult, ExecuteResult, QueryResult, SqliteBackend,
	SqliteBatchStatement, SqliteDb, SqliteTransaction,
};
pub use state::{ActorStateTransaction, RequestSaveOpts};
pub use task::{
	ActionDispatchResult, ActorTask, DispatchCommand, HttpDispatchResult, LifecycleCommand,
	LifecycleEvent, LifecycleState,
};
pub use task_types::{ActorChildOutcome, ShutdownKind, StateMutationReason, UserTaskKind};
pub use work_registry::{ActorWorkKind, ActorWorkPolicy};
