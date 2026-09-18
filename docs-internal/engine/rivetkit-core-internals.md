# rivetkit-core internals

Internal wiring reference for `rivetkit-rust/packages/rivetkit-core/`. These are facts about the current implementation. For the principles that govern how new code is added, see the root `CLAUDE.md` layer + fail-by-default sections. For state-mutation semantics, see `docs-internal/engine/rivetkit-core-state-management.md`.

## Storage organization

Actor subsystems are composed into `ActorContextInner`, not separate managers.

- Queue storage lives on `ActorContextInner`. Behavior sits in `actor/queue.rs` `impl ActorContext` blocks. Do not reintroduce `Arc<QueueInner>` or a public `Queue` re-export.
- Connection storage lives on `ActorContextInner`. Behavior sits in `actor/connection.rs` `impl ActorContext` blocks. Do not reintroduce `Arc<ConnectionManagerInner>` or a public `ConnectionManager` re-export.
- Actor state storage lives on `ActorContextInner`. Behavior sits in `actor/state.rs` `impl ActorContext` blocks. Do not reintroduce `Arc<ActorStateInner>` or a public `ActorState` re-export.
- Schedule storage lives on `ActorContextInner`. Behavior sits in `actor/schedule.rs` `impl ActorContext` blocks. Do not reintroduce `Arc<ScheduleInner>` or a public `Schedule` re-export.
- Event fanout lives directly in `ActorContext::broadcast`. Do not reintroduce a separate `EventBroadcaster` subsystem.

## Internal SQLite storage

Runtime actor persistence lives in internal SQLite tables. The `_rivet_` table prefix is reserved for RivetKit runtime data and must not be used by user schemas.

`_rivet_meta` is a tiny bootstrap root, not a general-purpose runtime KV store. It is created before the numbered migration ladder, so `schema_version` cannot live in a table created by that ladder. Its other key, `kv_import_state`, deliberately survives `clear_imported_storage` so startup can detect an interrupted legacy import, clear partial destination rows, and retry safely. Generic text accessors for this table are limited to that migration bookkeeping.

| Table | Contents |
|---|---|
| `_rivet_meta` | Bootstrap and import bookkeeping key-value rows such as `schema_version` and `kv_import_state` |
| `_rivet_actor` | Cold actor startup fields such as `has_initialized` and input |
| `_rivet_actor_state` | Hot serialized user state |
| `_rivet_schedule_events` | Durable scheduled actions |
| `_rivet_conns` / `_rivet_conn_state` | Hibernatable websocket cold metadata and hot state |
| `_rivet_runtime` | Runtime singletons: last pushed alarm, inspector token, queue next id |
| `_rivet_queue` | Queue messages |
| `_rivet_wf_kv` | TypeScript workflow storage with verbatim packed keys |
| `_rivet_user_kv` | Deprecated user `c.kv` compatibility storage |

## Legacy KV import

The first wake on the migrated runtime runs `actor/migrate_kv_to_sqlite/` before actor readiness. The importer live-scans the legacy actor KV keyspace, copies recognized records into SQLite, marks `_rivet_meta.kv_import_state = done`, and leaves legacy KV bytes untouched as a frozen downgrade snapshot.

Legacy traces under `[7, 1, ...]` are intentionally skipped and are not copied to `_rivet_user_kv`. The only runtime KV write after migration is the inspector token mirror at `[3]`, kept until the dashboard stops fetching the token through the public actor-KV endpoint.

## State persistence flow

- `request_save` uses `RequestSaveOpts { immediate, max_wait_ms }`. NAPI callers use `ctx.requestSave({ immediate, maxWaitMs })`. Do not use a boolean `requestSave` or `requestSaveWithin`.
- Receive-loop persistence routes deferred saves through `ActorContext::request_save(...)` + `ActorEvent::SerializeState { reason: Save, .. }`.
- Shutdown adapters persist explicitly with `ActorContext::save_state(Vec<StateDelta>)` because `Sleep`/`Destroy` replies are unit-only. Direct durability must still clear pending save-request flags after a successful write.
- Actor state is post-boot delta-only. Use `request_save` / `save_state(Vec<StateDelta>)`. Do not reintroduce `set_state` / `mutate_state`.
- Schedule mutations write point rows in `_rivet_schedule_events`, then resync the envoy alarm to the earliest event.
- State mutations from inside `on_state_change` callbacks fail with `actor/state_mutation_reentrant`. Use vars or another non-state side channel for callback-run counters.

## Inspector wiring

- Live inspector state rides `ActorContext::inspector_attach()` returning an `InspectorAttachGuard` plus `subscribe_inspector()`. Hold the guard for the websocket lifetime so `ActorTask` can debounce `SerializeState { reason: Inspector, .. }` off request-save hooks.
- Cross-cutting inspector hooks stay anchored on `ActorContext`. Queue-specific callbacks carry the current size; connection updates read the context connection count so unconfigured inspectors stay cheap no-ops.

## Schedule + alarms

- `Schedule` alarm sync is guarded by `dirty_since_push`. Fresh schedules start dirty, mutations set dirty, and unchanged shutdown syncs must not re-push identical envoy alarms.
- Persisted driver-alarm dedup stores the last pushed `Option<i64>` in `_rivet_runtime.last_pushed_alarm`. Startup loads it from SQLite and skips identical future alarm pushes.

## Transport helpers

- HTTP and WebSocket staging helpers keep transport failures at the boundary. `on_request` errors become HTTP 500 responses; `on_websocket` errors become logged 1011 closes. `ConnHandle` and `WebSocket` wrappers surface explicit configuration errors through internal `try_*` helpers.
- Bulk transport disconnect helpers sweep every matching connection, remove the successful disconnects, update connection/sleep bookkeeping, then aggregate any per-connection failures into the returned error.
- Receive-loop `ActorEvent::Action` dispatch uses `conn: None` for alarm-originated work and `Some(ConnHandle)` for real client connections. Do not synthesize placeholder connections for scheduled actions.
- Sleep readiness stays centralized in `ActorContext` sleep state. Queue waits, scheduled internal work, disconnect callbacks, and websocket callbacks report activity through `ActorContext` hooks so the idle timer stays accurate.
- User-facing `onDisconnect` work runs inside `ActorContext::with_disconnect_callback(...)` so `pending_disconnect_count` gates sleep until the async callback finishes.

## Registry + dispatch

- Registry startup builds configured `ActorContext`s with `ActorContext::build(...)` so state, queue, and connection managers inherit the actor config before lifecycle startup runs. `ActorContext::build(...)` must seed owned queue, connection, and sleep config storage from its `ActorConfig`; do not initialize those fields with `ActorConfig::default()`.
- Registry actor task handles live in one `actor_instances: SccHashMap<String, ActorInstanceState>`. Use `entry_async` for Active/Stopping transitions.
- `RegistryDispatcher::handle_fetch` owns framework HTTP routes `/metrics`, `/inspector/*`, `/action/*`, and `/queue/*`. TypeScript NAPI callbacks keep action/queue schema validation and queue `canPublish`.
- Raw `onRequest` HTTP fetches bypass `maxIncomingMessageSize` / `maxOutgoingMessageSize`. Those message-size guards apply only to `/action/*` and `/queue/*` framework routes, not unmatched user `onRequest` paths.
- Framework HTTP error payloads omit absent `metadata` for JSON/CBOR responses so missing metadata stays `undefined`. Only explicit metadata `null` serializes as `null`.

## Startup sequence

1. Ensure the internal SQLite schema and import legacy KV to SQLite if needed.
2. Load actor startup data from internal SQLite into `ActorContext` before factory creation.
3. Persist `has_initialized` immediately.
4. Resync persisted alarms and restore hibernatable connections.
5. Set `ready` before the driver hook.
6. Reset the sleep timer.
7. Spawn `run` in a detached panic-catching task.
8. Drain overdue scheduled events after `started`.
9. Set `started` after the driver hook completes.

## Shutdown sequences

### Sleep

Two-phase:

- `SleepGrace` fires `onSleep` immediately and keeps dispatch/save timers live.
- `SleepFinalize` gates dispatch, suspends alarms, and runs teardown.

Sleep grace fires the actor abort signal on entry and waits for the run handler to exit before finalize.

Finalize:

1. Wait for the tracked `run` task.
2. Poll `ActorContext` sleep state for the idle window and shutdown-task drains.
3. Wait for `ActorContext::wait_for_on_state_change_idle(...)` before sending final save events so async `onStateChange` work cannot race durability.
4. Persist hibernatable connections.
5. Disconnect non-hibernatable connections.
6. Immediate state save.

### Destroy

- Skip the idle-window wait.
- Use the unified `sleep_grace_period` budget for the destroy phase.
- Wait for `wait_for_on_state_change_idle(...)` before final saves.
- Disconnect every connection.
- Immediate state save + SQLite cleanup.

### Stop

Persistence order:

1. Immediate state save.
2. Pending state write wait.
3. Sync the driver alarm for sleep, or cancel it without querying SQLite for destroy.
4. Alarm write wait.
5. SQLite cleanup.

## ActorConfig

- `sleep_grace_period_overridden` distinguishes an explicit `sleep_grace_period` from runtime override defaults.

## envoy-client interop

- Graceful actor teardown flows through `EnvoyCallbacks::on_actor_stop_with_completion`. The default implementation preserves the old immediate `on_actor_stop` behavior by auto-completing the stop handle after the callback returns.
- Sync `EnvoyHandle` lookups for live actor state read the shared `SharedContext.actors` mirror keyed by actor id/generation. Blocking back through the envoy task can panic on current-thread Tokio runtimes.

## Callbacks

- Boxed callback APIs use `futures::future::BoxFuture<'static, ...>` plus the shared `actor::callbacks::Request` and `Response` wrappers so config and HTTP parsing helpers stay in core for future runtimes.

## Test isolation

- Process-global `ActorTask` test hooks (`install_shutdown_cleanup_hook`, lifecycle-event/reply hooks) must be actor-scoped and serialized in tests. Parallel `cargo test` runs will otherwise cross-wire unrelated actors.

## High-level wrapper (`rivetkit`) interop

- Typed `Ctx<A>` stays a stateless wrapper over `rivetkit-core::ActorContext`. Actor state lives in the user receive loop. There is no typed vars field. CBOR encode/decode stays at wrapper method boundaries like `broadcast` and `ConnCtx`.
- Typed `Ctx<A>::client()` builds and caches `rivetkit-client` from core Envoy client accessors. Keep actor-to-actor client construction in the wrapper, not core.
- Typed `Start<A>` wrappers rehydrate each `ActorStart.hibernated` state blob back onto the `ConnHandle` before exposing `ConnCtx`, or `conn.state()` stops matching the wake snapshot.
- `rivetkit-rust/packages/rivetkit/src/persist.rs` owns typed actor-state `StateDelta` builders. `SerializeState`/`Sleep`/`Destroy` in `src/event.rs` stay thin reply helpers that reuse those builders instead of open-coding persistence bytes per wrapper.
