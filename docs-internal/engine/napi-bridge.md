# NAPI bridge

Rules for `rivetkit-typescript/packages/rivetkit-napi/`. The bridge is pure plumbing — all load-bearing logic belongs in `rivetkit-core`. These notes capture current conventions and known foot-guns; they are not design principles. For the layer-boundary rule itself, see the root `CLAUDE.md`.

## Package boundaries

- The N-API addon lives at `@rivetkit/rivetkit-napi` in `rivetkit-typescript/packages/rivetkit-napi`. Keep Docker build targets, publish metadata, examples, and workspace package references in sync when renaming or moving it.
- TypeScript actor vars are JS-runtime-only in `registry/native.ts`. Do not reintroduce `ActorVars` in `rivetkit-core` or add `ActorContext.vars` / `setVars` to NAPI.

## Callback registry layout

- Keep the receive-loop adapter callback registry centralized in `rivetkit-typescript/packages/rivetkit-napi/src/actor_factory.rs`. Extend its TSF slots, payload builders, and bridge error helpers there instead of scattering ad hoc JS conversion logic across new dispatch code.
- Keep `rivetkit-typescript/packages/rivetkit-napi/src/napi_actor_events.rs` as the receive-loop execution boundary. `actor_factory.rs` stays focused on TSF binding setup and bridge helpers, not event-loop control flow.

## Actor context wrapping

- N-API actor-runtime wrappers expose `ActorContext` sub-objects as first-class classes, keep raw payloads as `Buffer`, and wrap queue messages as classes so completable receives can call `complete()` back into Rust.

## ThreadsafeFunction conventions

- NAPI callback bridges pass a single request object through `ThreadsafeFunction`. Promise results that cross back into Rust deserialize into `#[napi(object)]` structs instead of `JsObject` so the callback future stays `Send`.
- N-API `ThreadsafeFunction` callbacks using `ErrorStrategy::CalleeHandled` follow Node's error-first JS signature. Internal wrappers must accept `(err, payload)` and rethrow non-null errors explicitly.
- NAPI websocket async handlers hold one `WebSocketCallbackRegion` token per promise-returning handler so concurrent handlers cannot release each other's sleep guard.

## Payload + error conventions

- `#[napi(object)]` bridge payloads stay plain-data only. If TypeScript needs to cancel native work, use primitives or JS-side polling instead of trying to pass a `#[napi]` class instance through an object field.
- N-API structured errors cross the JS<->Rust boundary by prefix-encoding `{ group, code, message, metadata, rayId }` into `napi::Error.reason`, then normalizing that prefix back into a `RivetError` on the other side.
- N-API bridge debug logs use stable `kind` plus compact payload summaries, never raw buffers or full request bodies.

## Receive-loop state lifecycle

- `ActorContextShared` instances are cached by `actor_id`. Every fresh `run_adapter_loop` must call `reset_runtime_shared_state()` before reattaching abort/run/task hooks or sleep→wake cycles inherit stale `end_reason` / lifecycle flags and drop post-wake events.
- Receive-loop `SerializeState` handling stays inline in `napi_actor_events.rs`, reuses the shared `state_deltas_from_payload(...)` converter from `actor_context.rs`, and only cancels the adapter abort token on `Destroy` or final adapter teardown, not on `Sleep`.
- Receive-loop NAPI optional callbacks preserve the TypeScript runtime defaults: missing `onBeforeSubscribe` allows the subscription, missing workflow callbacks reply `None`, and missing connection lifecycle hooks still accept the connection while leaving the existing empty conn state untouched.

## Runtime-state reference cleanup

- `ActorContextShared::runtime_state` stores a N-API `Ref<()>` for the JS-only actor runtime state bag. `Ref::unref(env)` and reference deletion require an `Env`, but `reset_runtime_state()` runs from receive-loop worker paths and `Drop for ActorContextShared` may run without an active JS callback frame.
- The current `mem::forget` fallback in `actor_context.rs` keeps debug and release behavior aligned when no `Env` is available, but it leaks one JS object reference per actor wake cycle that created runtime state.
- The intended fix is to create an actor-shared cleanup `ThreadsafeFunction` the first time `runtime_state(env)` has an `Env`. Stale `Ref<()>` values should be wrapped in a payload whose `Drop` forgets the reference only if it was never successfully unreffed, then queued to that TSF from `reset_runtime_state()` and `Drop`.
- The TSF callback must run on the JS thread, call `ref.unref(ctx.env)`, and avoid invoking user callbacks. The TSF itself should be unreffed from the event loop so it does not keep Node alive.
- Shutdown is the hard edge: if the TSF is closing or Node supplies a null `Env` during addon teardown, the payload must fall back to the existing bounded process-lifetime leak instead of dropping a live `Ref<()>` and tripping napi-rs debug assertions.
- Before replacing the fallback, add a NAPI integration test that creates runtime state across many actor wake/destroy cycles, waits for the cleanup TSF to drain, and verifies native reference counts return to zero.

## Cancellation bridging

- For non-idempotent native waits like `queue.enqueueAndWait()`, bridge JS `AbortSignal` through a standalone native `CancellationToken`. Timeout-slicing is only safe for receive-style polling calls like `waitForNames()`.
- Native queue receive waits observe the actor abort token. `enqueue_and_wait` completion waits ignore actor abort and rely on the tracked user task for shutdown cancellation.
- Core queue receive waits need the `ActorContext`-owned abort `CancellationToken`, cancelled from `mark_destroy_requested()`. External JS cancel tokens alone will not make `c.queue.next()` abort during destroy.

## TypeScript-side bridge behavior

- In `rivetkit-typescript/packages/rivetkit/src/registry/native.ts`, late `registerTask(...)` calls during sleep/finalize teardown can legitimately hit `actor task registration is closed` / `not configured`. Swallow only that specific bridge error so workflow cleanup does not crash the runtime.
- Bare-workflow `no_envoys` failures should be investigated as possible runtime crashes before being chased as engine scheduling misses. Check actor stderr for late `registerTask(...)` / adapter panics first.
- Native actor runner settings in `rivetkit-typescript/packages/rivetkit/src/registry/native.ts` come from `definition.config.options`, not top-level actor config fields.
