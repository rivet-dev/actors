# CLAUDE.md

Design constraints, invariants, and reference commands for the Rivet monorepo. For implementation details, wiring, and procedural gotchas, follow the links under [Reference Docs](#reference-docs).

## Terminology

**Always spell the product name `agentOS`, never `AgentOS`; do not alter type identifiers such as `AgentOSActorConfig`.**

**ALWAYS use `rivet.dev` - NEVER use `rivet.gg`**

- API endpoint: `https://api.rivet.dev`
- Cloud API endpoint: `https://cloud-api.rivet.dev`
- Dashboard: `https://dashboard.rivet.dev`
- Documentation: `https://rivet.dev/docs`

**Use "sandbox mounting" when referring to the agentOS sandbox integration.** Do not use "sandbox extension" or "sandbox escalation." The feature mounts a sandbox as a filesystem inside the VM.

**ALWAYS use `github.com/rivet-dev/rivet` - NEVER use `rivet-dev/rivetkit` or `rivet-gg/*`**

**Never modify an existing published `*.bare` runner protocol version unless explicitly asked to do so.**

- Add a new versioned schema instead, then migrate `versioned.rs` and related compatibility code to bridge old versions forward.
- When bumping the protocol version, update `PROTOCOL_MK2_VERSION` in `engine/packages/runner-protocol/src/lib.rs` and `PROTOCOL_VERSION` in `rivetkit-typescript/packages/engine-runner/src/mod.ts` together. Both must match the latest schema version.

**Always use versioned BARE (`vbare`) instead of raw `serde_bare` for any persisted or wire-format encoding unless explicitly told otherwise.** Raw `serde_bare::to_vec` / `from_slice` has no version header, so any future schema change forces hand-rolled `LegacyXxx` fallback structs. `vbare::OwnedVersionedData` plus a versioned `*.bare` schema is the standard pattern. Acceptable raw-bare exceptions: ephemeral in-memory encodings that never cross a process boundary or hit disk, and wire formats whose protocol version is coordinated out-of-band (e.g. an HTTP path like `/v{PROTOCOL_VERSION}/...` or another channel that pins both peers to one schema per call).

- Avoid raw `f64` fields in vbare protocol schemas that use hashable maps; generated Rust derives `Eq`/`Hash`, so encode floats as fixed bytes or an ordered wrapper.
- Version converters must manually map fields between versions; never use serialize-deserialize round trips such as `transcode_version` or `serde_bare::to_vec` plus `from_slice`.
- RivetKit client/server protocol compatibility assumes the server/runtime is newer than the client; clients send their latest request protocol version, and servers handle older-client compatibility and negotiation.

When talking about "Rivet Actors" make sure to capitalize "Rivet Actor" as a proper noun and lowercase "actor" as a generic noun.

**Actor Runtime Socket** is the product name for the generic actor-local protocol. SQLite is its first capability and Unix sockets are its current transport; do not use "Actor SQLite UDS API" or "SQLite UDS" as the product name.

## Commands

### Build + test

```bash
# Check a specific package without producing artifacts (preferred for verification)
cargo check -p package-name

# Build
cargo build
cargo build -p package-name
cargo build --release

# Test
cargo test
cargo test -p package-name
cargo test test_name
cargo test -- --nocapture
```

### Development

```bash
# Run linter (but see "Development warnings" below)
./scripts/cargo/fix.sh

# Check for linting issues
cargo clippy -- -W warnings
```

- Agents may run `node scripts/format/agent-format.mjs` to format changed Biome and Rust files. Do not run broad `cargo fmt` or `cargo fmt --all` manually.
- Do not run `./scripts/cargo/fix.sh` or other broad formatter/fixer commands yourself.
- Ensure lefthook is installed and enabled for git hooks (`lefthook install`).

### Docker dev environment

```bash
cd self-host/compose/dev
docker-compose up -d
```

- Do not edit `self-host/compose/dev*` configs directly. Edit the template in `self-host/compose/template/` and rerun (`cd self-host/compose/template && pnpm start`) to regenerate.
- Rebuild publish base images with `scripts/docker-builder-base/build-push.sh <base-name|all> --push`. Update `BASE_TAG` when rebuilding shared builder bases; engine bases are published per commit in `publish.yaml`.

### Version control (jj)

- This repo uses jj (Jujutsu) on top of git. **jj's workflow is inverted from git:** the working copy is itself a revision that auto-tracks edits, so you create a new revision *before* making changes (with `jj new`) rather than committing *after* (`git commit`). The description is set separately via `jj describe`. There is no staging step.
- Before making changes, check whether jj is initialized by running `jj status`. If it fails (e.g. "There is no jj repo in '.'"), run `jj git init --colocate` from the repo root so jj lives alongside the existing `.git` directory. Do NOT run `jj git init` without `--colocate` — that creates a standalone jj repo and breaks the git workflow.
- **MUST run `jj new` before making any file edits for a new task.** This is the first step of any task that touches files. Run it before reading, before planning, before editing. The only exception is when you are directly fixing or finishing the change at `@` that you just made in this same session. In that case use `jj squash --into <rev>` or `jj edit <rev>`. If you already started editing without running `jj new`, stop and split the changes with `JJ_EDITOR=true jj split <paths>` before continuing. Each revision must be one self-contained change reviewable on its own. Never mix unrelated work into one revision.
- Set the revision description with `jj describe -m "{conventional commit message}"` — a single-line conventional commit (`feat`, `fix`, `chore`, `docs`, `refactor`, etc.) with an optional scope. Examples: `feat(metrics): record depot sqlite phase timings`, `fix(pegboard): handle empty ack batch`.
- **Commit titles and PR titles are pure conventional commits.** Never indicate that a change was written by a coding agent: no model name, no agent name, no `[SLOP(...)]` prefix in the title, body, or PR text, and no `Co-Authored-By:` or `Generated with` trailer. The title must read exactly as a human-authored conventional commit. jj descriptions stay single-line.
- **PR descriptions are a simple, high-level bullet list of what changed.** One bullet per meaningful change in plain language. No per-file or line-by-line detail, no implementation narration, and no mention of an agent.
- **Never push to `main` unless explicitly specified by the user.**
- **Safety:** Never run destructive jj or git commands (`jj git push`, `jj abandon`, `jj squash` into a non-current revision, `jj op restore`, `jj op undo` past your own work, `jj rebase -d main`, `git push --force`, `git reset --hard`) unless the user explicitly requests it.


### Releases

- **Cut every release as a patch.** The version number is only incidentally semver-compatible; it is not semver-driven. Major and minor bumps are marketing decisions the user makes deliberately, never inferred from what changed in the code.
- Always pass `--version <x.y.z> --latest` explicitly to `pnpm --filter=publish release`. The repo still carries pre-2.x calendar tags (`v25.8.0`) that sort above the current line, so the `--major/--minor/--patch` flags bump from the wrong version and the `latest` dist-tag silently resolves to false.

## Assets

- Large public dashboard and website media belongs in the `rivet-assets` R2 bucket, not Git.
- Use object keys shaped like `dashboard/{group}/{asset-name}` for dashboard media and `website/blog/{post}/{asset-name}` for blog or changelog post media.
- Upload with `op://Engineering/rivet-assets R2 Upload/{username,password}` and `aws s3 cp <file> s3://rivet-assets/<key> --endpoint-url https://2a94c6a0ced8d35ea63cddc86c2681e7.r2.cloudflarestorage.com`.
- For blog or changelog hero media, upload the file to R2 as `website/blog/{post-slug}/image.{ext}` and set frontmatter `image: true` (or `image: { format: "gif" }` for a non-png). The URL is derived from the slug and dimensions are fixed at a 2:1 ratio, so do not write the absolute URL, width, or height. Resolved by `website/src/lib/postImage.ts`.
- Do not use Git LFS in this repo; jj can snapshot raw working-tree bytes.

## Frontend Visual Changes

- For any frontend visual change, use the `agent-browser` skill to view the result in a browser instead of working blind. If it is not installed, prompt the user to install it.
- The frontend dashboard dev server always runs at `http://localhost:43708/`. Check that it is already serving there before starting a new one.
- The same change can render differently per deployment flavor. Verify visual changes in OSS and cloud (at minimum), not just whichever flavor the dev server defaults to. See [Frontend Feature Flags](#frontend-feature-flags).

## Frontend Forms

- Manage all user-submitted form state with `react-hook-form`. Do not hand-roll form state with `useState`-driven controlled inputs. Wrapping a controlled UI-library input (e.g. a shadcn `Select`) in RHF's `<Controller>` is still react-hook-form owning the state and is fine.

## Frontend Feature Flags

- The dashboard serves multiple deployment flavors (cloud / OSS / enterprise) from one build via flags in `frontend/src/lib/features.ts`; consume them through `import { features } from "@/lib/features"`, never by reading `VITE_FEATURE_FLAGS` directly.
- Ship a new pack of features behind a feature flag whenever it is significant (a whole panel/page/subsystem) or not universally available across flavors, so each flavor can freely enable or disable it. Do not gate small universal changes, and do not make flags for everything.
- If unsure whether a feature needs a flag, confirm the need with the user before adding or omitting one.
- Test frontend changes across flavors before calling them done, especially OSS (it turns the most off and regresses most often). Switch flavors in dev by setting `localStorage.FEATURE_FLAGS` and reloading. See [Feature flags](.claude/reference/feature-flags.md) for the flag list, flavor mapping, the per-flavor flag sets, and consistency rules.

## Frontend Routing (TanStack Router)

### Route context vs loader data
- `context()` runs at match creation time — its return value is part of `match.context` and readable via `useRouteContext`. Use it for synchronous context setup (e.g. creating a data provider from params).
- `beforeLoad()` return value goes into `match.__beforeLoadContext` and is **never merged back into `match.context`**. `useRouteContext` will not see it — components reading via `useRouteContext` get a stale snapshot from before `beforeLoad` ran.
- For async-computed values (e.g. a data provider that depends on a fetched namespace), return the value from `loader()` instead and read it in components via `useLoaderData`. The loader receives the full merged context including `beforeLoad` results as a function argument, so it can re-export the computed value into `match.loaderData`.
- Rule of thumb: sync setup → `context()` + `useRouteContext`. Async setup → `beforeLoad` (for child route access) + `loader` return + `useLoaderData` (for component access).

### Data providers (convention)
- Every route that owns a data provider sets it up in `context()` (sync) or `beforeLoad` (async) AND re-exports it from `loader` as `{ dataProvider: context.dataProvider }`. All consumer hooks in `src/components/actors/data-provider.tsx` read via `useLoaderData`. Do not read data providers via `useRouteContext` — `match.context` is a snapshot taken at match creation time and does not include `beforeLoad` results.

## Dependency Management

- Integrations must default to RivetKit's standard behavior and configuration resolution. Do not duplicate Rivet endpoint, namespace, token, pool, local Engine, runtime, or readiness defaults inside an integration; pass configuration through to RivetKit and add integration-specific behavior only when the external protocol requires it.

- Prefer the Tokio-shaped APIs from `antiox` (`antiox/sync/mpsc`, `antiox/task`, etc.) over ad hoc Promise queues, custom channel wrappers, or event-emitter coordination.
- `rivet-envoy-client` transport features are mutually exclusive; native builds use the default `native-transport`, while wasm builds must set `default-features = false` and enable `wasm-transport`.
- `rivet-envoy-client` wasm WebSocket code lives behind `target_arch = "wasm32"` with a native-host `wasm-transport` stub so feature checks do not compile browser APIs on developer machines.
- `rivetkit-core` wasm builds use `--no-default-features --features wasm-runtime,sqlite-remote`; keep native process and runner-config HTTP code behind `native-runtime`.
- Core-owned lifecycle tasks in `rivetkit-core` should spawn through `RuntimeSpawner` so native builds use Send-capable tasks and wasm builds use local tasks.
- `rivet-envoy-client::async_counter::AsyncCounter` is the shared HTTP request counter type consumed by core sleep logic; do not pull `rivet-util` into core for that counter.
- For `wasm32-unknown-unknown` Rust checks, use target-specific minimal Tokio plus `getrandom/js` and `uuid/js`; scan production dependencies with `cargo tree -e normal` so dev-dependencies do not create false native-dependency hits.
- Use `scripts/cargo/check-rivetkit-core-wasm.sh` as the canonical wasm gate for `rivetkit-core`; it checks the wasm build, scans native dependency leaks, and verifies native transport/runtime features fail on wasm.
- The high-level `rivetkit` crate stays a thin typed wrapper over `rivetkit-core` and re-exports shared transport/config types instead of redefining them.
- When `rivetkit` needs ergonomic helpers on a `rivetkit-core` type it re-exports, prefer an extension trait plus `prelude` re-export instead of wrapping and replacing the core type.
- RivetKit action and event protocol `args` must always be array-shaped before crossing the client protocol boundary. Normalize at the server/source side, not in client delivery code: named structs/objects become `[object]`, tuples/arrays stay positional, scalars become `[scalar]`, and unit/null becomes `[]`.
- `engine/sdks/*/api-*` are auto-generated SDK outputs; update the source API schema and regenerate them instead of editing them by hand.

### RivetKit Test Fixtures

- Core tests that touch the `_RIVET_TEST_INSPECTOR_TOKEN` env override must share a process-wide lock with startup tests that assert inspector-token initialization side effects; otherwise parallel `cargo test` runs can flip `init_inspector_token(...)` between the env-override no-op path and the stored-token initialization path.
- Every new or modified internal SQLite `SELECT`, `UPDATE`, or `DELETE` in `rivetkit-core` must add or update a query-efficiency test using the production SQL. Use representative cardinality to reject full scans, temporary sorts, and automatic indexes where indexed access is expected; allowlist intentional scans in the catalog with a concrete reason and production bound. Simple inserts and schema DDL need correctness or migration coverage but no plan assertion. Assert semantic plan properties rather than exact `EXPLAIN QUERY PLAN` text, and run `cargo test -p rivetkit-core sql_efficiency --lib` after internal schema, index, or query changes. This policy excludes user-provided SQL.
- For the fast static/http/bare driver verifier, pass only the files listed under `## Fast Tests` in `~/.agents/notes/driver-test-progress.md`; `tests/driver/*.test.ts` also pulls in slow-suite files and gives bogus gate failures.
- Wasm host smoke tests can drive `buildNativeFactory` through `WasmCoreRuntime` fake bindings to cover actor callbacks, KV, state serialization, remote SQLite routing, and NAPI import boundaries without checked-in wasm-pack output.
- When moving Rust inline tests out of `src/`, keep a tiny source-owned `#[cfg(test)] #[path = "..."] mod tests;` shim so the moved file still has private module access without widening runtime visibility. Prefer a dedicated moved-test file per source module; reusing stale shared `tests/modules/*.rs` files can silently rot against private APIs and explode once you wire them back in.
- Tracing assertions on spawned Rust futures should bind an explicit `tracing::Dispatch` with `.with_subscriber(...)` on the spawned future; thread-local `set_default(...)` can miss the real logs in full async suite runs.

### SQLite Package

- Depot client owns native SQLite VFS and query execution in `engine/packages/depot-client/`; core owns lifecycle, and NAPI only marshals JS types.
- SQLite VFS direct tests should record workflow compaction wakes through `CompactionSignaler`, not call legacy `compact_default_batch`.
- SQLite VFS correctness tests should use `DirectStorage`; do not reintroduce mock or envoy transport variants for those tests.
- SQLite VFS `xSync` durability depends on depot's `sqlite_commit` reply waiting for the FDB transaction commit.
- SQLite VFS process-global registrations must be owned by a Drop guard so panics unwind through `sqlite3_vfs_unregister`.
- `NativeDatabase::Drop` must bound dirty-page flushes with a short timeout and return after logging if the commit future never resolves.
- Actor2 workflows and envoy actors always use the SQLite v2 storage format; only old actor v1 workflows and pegboard runners use the v1 storage format. ("v2" here refers to the on-disk storage format, not envoy-protocol v2.)
- Native SQLite VFS recent-page preload hints are actor-side Rust state surfaced by `NativeDatabase::snapshot_preload_hints()`; persist and consume them through runtime/envoy wiring, not JS APIs.
- SQLite VFS file handles must enforce their reader or writer role; reader-owned handles fail closed on mutating callbacks.
- Native SQLite single-statement work should route through the native execute path; keep `exec` as the multi-statement compatibility path.
- Native SQLite opens should use `depot_client::database::open_database_from_envoy` instead of direct `rusqlite` calls so native routing policy stays shared.
- Pegboard-envoy remote SQL executor caches should use `Arc<OnceCell<NativeDatabaseHandle>>` values so first-use initialization stays lazy and single-flight per `(actor_id, sqlite_generation)`.
- Sent remote SQL requests must fail with `sqlite.remote_indeterminate_result` on WebSocket disconnect; only unsent remote SQL may be sent after reconnect.
- Native SQLite manual transactions keep an idle writer open until autocommit returns; route subsequent work through the writer instead of reader classification.
- Native SQLite read mode may hold multiple read-only connections, while write mode must hold exactly one writable connection and no readers; TypeScript must not be the routing policy boundary.
- For NAPI bridge wiring (TSF callback layout, cancellation tokens, `#[napi(object)]` rules), see `docs-internal/engine/napi-bridge.md`.

## Agent Working Directory

All agent working files live user-scoped in `~/.agents/`, never inside the repo. Override the location with the `AGENTS_DIR` env var. Run `scripts/setup/agents-dir.sh` once to create the subdirs. These files are not committed; `.agent/` is gitignored as a safety net.

- **Specs**: `~/.agents/specs/` — design specs and interface definitions for planned work.
- **Research**: `~/.agents/research/` — research documents on external systems, prior art, and design analysis.
- **Todo**: `~/.agents/todo/*.md` — deferred work items with context on what needs to be done and why.
- **Notes**: `~/.agents/notes/` — general notes and tracking.
- **Benchmarks**: `~/.agents/benchmarks/` — benchmark result artifacts.

When the user asks to track something in a note, store it in `~/.agents/notes/` by default. When something is identified as "do later", add it to `~/.agents/todo/`. Design documents and interface specs go in `~/.agents/specs/`.

## RivetKit Layer Architecture

- **rivetkit-core** is the source of truth for all load-bearing functionality. **rivetkit (TypeScript)** is the flagship user-facing implementation. **rivetkit (Rust)** is a preview API that should be kept up to date with rivetkit-typescript on a best-effort basis; it may lag behind, but new user-facing capabilities added to rivetkit-typescript should be mirrored where practical.

- **Engine** (`packages/core/engine/`, includes Pegboard + Pegboard Envoy) — Orchestration. Manages actor lifecycle, routing, KV, SQLite, alarms. In local dev, the engine is spawned alongside RivetKit.
- **envoy-client** (`engine/sdks/rust/envoy-client/`) — Wire protocol between actors and the engine. BARE serialization, WebSocket transport, KV request/response matching, SQLite protocol dispatch, tunnel routing.
- **rivetkit-core** (`rivetkit-rust/packages/rivetkit-core/`) — Core RivetKit logic in Rust, language-agnostic. Lifecycle state machine, sleep logic, shutdown sequencing, state persistence, action dispatch, event broadcast, queue management, schedule system, inspector, metrics. All callbacks are dynamic closures with opaque bytes. All load-bearing logic must live here. Config conversion helpers and HTTP request/response parsing for foreign runtimes belong here.
- **rivetkit (Rust)** (`rivetkit-rust/packages/rivetkit/`) — Rust-friendly typed API. `Actor` trait, `Ctx<A>`, `Registry` builder, CBOR serde at boundaries. Thin wrapper over rivetkit-core. No load-bearing logic.
- **rivetkit-napi** (`rivetkit-typescript/packages/rivetkit-napi/`) — NAPI bindings only. ThreadsafeFunction wrappers, JS object construction, Promise-to-Future conversion. No load-bearing logic. Must only translate between JS types and rivetkit-core types. Only consumed by `rivetkit-typescript/packages/rivetkit/`.
- **rivetkit (TypeScript)** (`rivetkit-typescript/packages/rivetkit/`) — TypeScript-friendly API. Calls into rivetkit-core via NAPI for lifecycle logic. Owns workflow engine, agent-os, and client library. Zod validation for user-provided schemas runs here.

### Layer constraints

- All actor-runtime lifecycle logic, state persistence, sleep/shutdown, action dispatch, event broadcast, queue management, schedule, inspector, and metrics must live in rivetkit-core. No actor-runtime lifecycle logic in TS or NAPI.
- The rivetkit (TypeScript) **client** (`rivetkit-typescript/packages/rivetkit/src/client/`) is exempt from the core-only rule. Client-side dispatch retry, stale-handle resolution, lifecycle-error classification, and reconnection logic stay in TypeScript and are not duplicated in rivetkit-core. The client runs in the user's process, not on the actor host.
- rivetkit-napi must be pure bindings. If code would be duplicated by a future V8 runtime, it belongs in rivetkit-core instead.
- rivetkit-napi serves through `CoreRegistry` + `NapiActorFactory`; do not reintroduce the deleted `BridgeCallbacks` JSON-envelope envoy path or `startEnvoy*Js` exports.
- NAPI `ActorContext.sql()` returns `JsNativeDatabase` directly; do not reintroduce a standalone `SqliteDb` wrapper export.
- TypeScript runtime adapters expose `CoreRuntime` from `rivetkit/src/registry/runtime.ts`; keep raw `@rivetkit/rivetkit-napi` and future `@rivetkit/rivetkit-wasm` imports inside `src/registry/*-runtime.ts`.
- rivetkit (Rust) is a thin typed wrapper. If it does more than deserialize, delegate to core, and serialize, the logic should move to rivetkit-core.
- rivetkit (TypeScript) owns only: workflow engine, agent-os, client library, Zod schema validation for user-defined types, and actor definition types.
- Errors use universal `RivetError` (group/code/message/metadata) at all boundaries. No custom error classes in TS.
- CBOR serialization at all cross-language boundaries. JSON only for HTTP inspector endpoints.
- Pegboard orchestrates actor exclusivity: at most one actor instance for a given actor id may be running or accessing that actor's storage at a time. This is the actor single-writer invariant: a Rivet Actor is the single writer for both KV and SQLite. `pegboard-envoy`, `envoy-client`, and remote/wasm SQLite may rely on this invariant and must not add envoy-protocol lease keys, engine-side transaction ownership, or separate same-actor concurrency fences. Coordinate LocalNative and remote/wasm transactions through the same actor-local `rivetkit-core` coordinator before backend dispatch. Actor Runtime Socket `leaseKey` values identify connection-local transaction handles and never cross depot, envoy, or engine boundaries. The lost-timeout + ping protocol is responsible for making overlapping actor generations impossible.

### Monorepo orientation

- **Core Engine** (`packages/core/engine/`) — main orchestration service.
- **Workflow Engine** (`packages/common/gasoline/`) — multi-step operations with reliability + observability.
- **Pegboard** (`packages/core/pegboard/`) — actor/server lifecycle management.
- **Pegboard Envoy** (`engine/packages/pegboard-envoy/`) — active actor-to-engine bridge (successor to pegboard-runner).
- **Common packages** (`packages/common/`) — foundation utilities, DB pools, caching, metrics, logging, health, gasoline core.
- **Core packages** (`packages/core/`) — engine executable, pegboard orchestration, workflow workers.
- **Shared libraries** (`shared/{language}/{package}/`) — shared between engine and rivetkit (e.g., `shared/typescript/virtual-websocket/`).
- **Databases**: UniversalDB (distributed state), ClickHouse (analytics/time-series). Connection pooling via `packages/common/pools/`.
- Services communicate via NATS with service discovery.

### Deprecated paths

- `engine/packages/pegboard-runner/`, `engine/sdks/typescript/runner`, `rivetkit-typescript/packages/engine-runner/`, and associated runner workflows are deprecated. All new actor hosting work targets `engine/packages/pegboard-envoy/` exclusively. Do not add features to or fix bugs in the deprecated runner path.

### Engine runner parity

- Keep `engine/sdks/typescript/runner` and `engine/sdks/rust/engine-runner` at feature parity.
- Any behavior, protocol handling, or test coverage added to one runner should be mirrored in the other runner in the same change whenever possible.
- When parity cannot be completed in the same change, explicitly document the gap and add a follow-up task.

## Trust Boundaries

- Treat `client <-> engine` as untrusted.
- Treat `envoy <-> pegboard-envoy` as untrusted.
- Treat traffic inside the engine over `nats`, `fdb`, and other internal backends as trusted.
- Treat `gateway`, `api`, `pegboard-envoy`, `nats`, `fdb`, and similar engine-internal services as one trusted internal boundary once traffic is inside the engine.
- Treat a client connected to an actor-local Unix socket as a trusted, application-local peer, not an adversarial security boundary. Socket ownership, permissions, and per-generation lifetime provide isolation. Continue enforcing framing, resource bounds, cancellation, and shutdown behavior for correctness and faulty clients, but do not assign security severity based on malicious local traffic unless this trust model changes.
- Validate and authorize all client-originated data at the engine edge before it reaches trusted internal systems.
- Validate and authorize all envoy-originated data at `pegboard-envoy` before it reaches trusted internal systems.

## WebSocket Rejection

- Reject WebSocket connections (auth failures, routing errors, any rejection reason) by accepting the upgrade and sending a close frame with a meaningful close code and `<group>.<code>` reason. Do not reject with an HTTP status before the upgrade. Browser clients cannot surface HTTP status on a failed upgrade; they only see `CloseEvent.code` / `.reason`, so pre-upgrade rejection leaves them with no diagnostic. Use close code `1008` (policy violation) for auth failures, matching the `inspector.unauthorized` convention.
- HTTP responses intended to surface as RivetKit client errors must use the canonical `HttpResponseError` body with a matching `Content-Type` and `x-rivet-error`; plain-text error bodies are not parsed as structured client errors.

## Fail-By-Default Runtime

- Avoid silent no-ops for required runtime behavior. If a capability is required, validate it and throw an explicit error with actionable context instead of returning early.
- Do not use optional chaining for required lifecycle and bridge operations (for example sleep, destroy, alarm dispatch, ack, and websocket dispatch paths).
- Optional chaining is acceptable only for best-effort diagnostics and cleanup paths (for example logging hooks and dispose/release cleanup).
- Keep scaffolded `rivetkit-core` wrappers `Default`-constructible, but return explicit configuration errors until a real `EnvoyHandle` is wired in.
- Keep foreign-runtime-only `ActorContext` helpers present on the public surface even before NAPI or V8 wires them. Make them fail with explicit configuration errors instead of silently disappearing.
- In `rivetkit-core` `ActorTask::run`, bind inbox `recv()` calls as raw `Option`s and log the closed channel before terminating. `Some(...) = recv()` plus `else => break` hides which inbox died.
- In `rivetkit-typescript/packages/rivetkit/src/common/utils.ts::deconstructError`, only passthrough canonical structured errors (`instanceof RivetError` or tagged `__type: "RivetError"` with full fields). Plain-object lookalikes must still be classified and sanitized.
- Actor-owned lifecycle / dispatch / lifecycle-event inbox producers use `try_reserve` helpers and return `actor.overloaded`. Do not await bounded `mpsc::Sender::send`.

## Performance

- Use `rivet_perf::{perf_start, perf_finish}` for latency-sensitive async phases, shared I/O wrappers, and suspected backpressure points where slow-tail spans are useful.
- Keep `perf_start!` labels bounded for metrics; put high-cardinality values such as IDs, request paths, and byte counts in span fields instead.
- Every `PerfMeasure` must end with `perf_finish!` or `perf_abandon!` before leaving scope.
- Never use `Mutex<HashMap<...>>` or `RwLock<HashMap<...>>`. Use `scc::HashMap` (preferred), `moka::Cache` (for TTL/bounded), or `DashMap` for concurrent maps.
- Use `scc::HashSet` instead of `Mutex<HashSet<...>>` for concurrent sets.
- `scc` async methods do not hold locks across `.await` points. Use `entry_async` for atomic read-then-write.
- Hold lock guards for as short as possible, including `scc` guards from `get_async` and related methods. Clone/copy needed data and `drop(...)` before async work, as in `send_and_check_ping` in `engine/packages/pegboard-gateway2/src/shared_state.rs`.
- Never poll a shared-state counter with `loop { if ready; sleep(Nms).await; }`. Pair the counter with a `tokio::sync::Notify` (or `watch::channel`) that every decrement-to-zero site pings, and wait with `AsyncCounter::wait_zero(deadline)` or an equivalent `notify.notified()` + re-check guard that arms the permit before the check.
- Every shared counter with an awaiter must have a paired `Notify`, `watch`, or permit. Waiters must arm the notification before re-checking the counter so decrement-to-zero cannot race past them.
- Reserve `tokio::time::sleep` for: per-call timeouts via `tokio::select!`, retry/reconnect backoff, deliberate debounce windows, or `sleep_until(deadline)` arms in an event-select loop. If it is inside a `loop { check; sleep }` body, it is polling and should be event-driven instead.
- Never add unexplained wall-clock defers like `sleep(1ms)` to decouple a spawn from its caller. Use `tokio::task::yield_now().await` or rely on the spawn itself.

## Memory Leaks

- Do not introduce intentional leaks (`Box::leak`, `std::mem::forget`, `*_into_raw` without matching cleanup) unless an upstream API makes ownership impossible to express safely.
- Never call `Box::leak` inside a per-request, per-error, or per-call code path; if a `'static` reference is required, use a compile-time `static`/`const` or intern it through a process-global map keyed by identity.
- Interned leaks must be bounded by unique schema/config identity and must not include unbounded user input such as raw error messages, SQL, actor keys, request paths, or headers.
- `std::mem::forget` is only acceptable when an FFI handle cannot be dropped in the current context; document the constraint inline, prove the leak is bounded, and prefer routing cleanup through an Env-bearing owner.
- Spawned futures that capture JS callbacks or other heavy resources must have a guaranteed completion path (e.g. a `CancellationToken` whose clones are guaranteed to drop). A `spawn_local(async move { token.cancelled().await; ... })` only drains if every clone of the token is dropped or cancelled.

## Async Rust Locks

- Async Rust code defaults to `tokio::sync::Mutex` / `tokio::sync::RwLock`. Do not use `std::sync::Mutex` / `std::sync::RwLock`.
- Use `parking_lot::Mutex` / `parking_lot::RwLock` only when sync is mandated by the call context: `Drop`, sync traits, FFI/SQLite VFS callbacks, or sync `&self` accessors.
- `rivetkit-napi` sync N-API methods, TSF callback slots, and test `MakeWriter` captures are forced-sync contexts. Use `parking_lot` there and keep guards out of awaits.
- `rivetkit-napi` test-only global serialization should use a real `parking_lot` guard instead of `AtomicBool` spin loops.
- If an external dependency's struct requires `std::sync::Mutex`, keep it at the construction boundary with an explicit forced-std-sync comment.
- Prefer async locks because sync guards can be silently held across `.await`, poisoning creates `.expect("lock poisoned")` boilerplate, and the tiny uncontended-lock win is dwarfed by actor I/O latency.

## Error Handling

- Custom error system at `packages/common/error/` using `#[derive(RivetError)]` on struct definitions. For the full derive example and conventions, see `.claude/reference/error-system.md`.
- Always return anyhow errors from failable functions. Do not glob-import from anyhow. Prefer `.context()` over the `anyhow!` macro.
- `rivetkit-core` should convert callback/action `anyhow::Error` values into transport-safe `group/code/message` payloads with `rivet_error::RivetError::extract` before returning them across runtime boundaries.
- `rivetkit-core` is the single source of truth for cross-boundary error sanitization. The TS bridge must NOT pre-wrap non-structured JS errors into a canonical `RivetError` before bridge-encoding. Pass raw `Error` values through the bridge as unstructured strings so core's `RivetError::extract` hits `build_internal` and produces the sanitized `INTERNAL_ERROR` payload. Only TS errors that never cross into core (HTTP router parsing, Hono middleware) should be sanitized by `common/utils.ts::deconstructError`. The dev-mode toggle that exposes raw messages lives in core (reads env at `build_internal`), not in the TS bridge.
- `envoy-client` actor-scoped HTTP fetch work should stay in a `JoinSet` plus an `Arc<AtomicUsize>` counter so sleep checks can read in-flight request count and shutdown can abort and join the tasks before sending `Stopped`.

## Logging

- Use tracing. Never use `eprintln!` or `println!` for logging in Rust code. Always use `tracing::info!`, `tracing::warn!`, `tracing::error!`, etc.
- Do not format parameters into the main message. Use structured fields: `tracing::info!(?x, "foo")` instead of `tracing::info!("foo {x}")`.
- Log messages should be lowercase unless mentioning specific code symbols. `tracing::info!("inserted UserRow")` instead of `tracing::info!("Inserted UserRow")`.
- `rivetkit-core` runtime logs should include `actor_id` and stable structured fields such as `reason`, `kind`, `delta_count`, byte counts, and timestamp fields instead of payload debug dumps.

## Metrics

- **Always include `namespace_id` as a metric label** when applicable. Bounded cardinality, useful for per-namespace dashboards.
- **Never use unbounded labels.** No `actor_id`, `actor_key`, `envoy_key`, `runner_id`, `workflow_id`, request_id, etc. Each unbounded label creates a new time series per value; under load this produces millions of series and OOMs the metrics pipeline.
- Other safe labels: `pool_name`, `runner_name`, `workflow_name`, `activity_name`, `operation_name`, `state`, `result`, `reason`, `error`, `status` — all bounded by code-defined enums or by namespace count.
- Standard label names (match existing conventions):
  - HTTP error class → `error` (not `error_kind` or `error_type`)
  - HTTP status code → `status` (not `status_code` or `http_status`)
  - Operation success/failure → `result` (not `outcome`)
  - State-transition cause → `reason`
- Never emit metrics from inside a `#[workflow]` function body. Workflow functions are replayed deterministically by gasoline; metric increments inside them fire on every replay. Move the increment into an `#[activity]` or `#[operation]` that the workflow calls.
- Gauges of integer-valued state use `IntGauge` / `IntGaugeVec`, not `Gauge` / `GaugeVec`.
- Reuse existing histogram bucket constants from `engine/packages/metrics/src/buckets.rs` (`BUCKETS`, `MICRO_BUCKETS`, `LIFETIME_BUCKETS`, etc.) rather than inventing new bucket arrays per metric. Add a new constant to `buckets.rs` only if no existing constant covers the value range.

## Testing

- **Never use `vi.mock`, `jest.mock`, or module-level mocking.** Write tests against real infrastructure (Docker containers, real databases, real filesystems). For LLM calls, use `@copilotkit/llmock` to run a mock LLM server. For protocol-level test doubles (e.g., ACP adapters), write hand-written scripts that run as real processes. `vi.fn()` for simple callback tracking is acceptable.
- Driver tests that wait for actor sleep must not poll actor actions while waiting; each action counts as activity and can reset the sleep deadline.
- **Never paper over flakes with retry loops or bumped waits.** When a test flakes, (1) root-cause the race, (2) write a deterministic repro using `vi.useFakeTimers()` or event-ordered `Promise` resolution, (3) fix the underlying ordering in core/napi/typescript, (4) delete any flake-workaround note. `vi.waitFor` is acceptable for legitimate "wait for an async event" coordination but never as a retry-until-success masking layer. Every `vi.waitFor` call must have a one-line comment explaining why polling rather than direct awaiting is necessary.
- In `rivetkit-typescript/packages/rivetkit/tests/`, put the `vi.waitFor(...)` justification on the immediately preceding `//` line. `pnpm run check:wait-for-comments` enforces the adjacent comment.
- **Rust tests live under `tests/`, not inline `#[cfg(test)] mod tests` in `src/`.** Move every inline test module in Rust crates to the crate's `tests/` directory. Exceptions must be justified (e.g., testing a private internal that can't be reached from an integration test).
- For running RivetKit tests, Vitest filter gotchas, the driver-test parity workflow, and Rust test layout rules, see `.claude/reference/testing.md`.

## Traces Package

- Keep `@rivetkit/traces` chunk writes under the 128 KiB actor KV value limit. Use 96 KiB chunks unless a multipart reader/writer replaces the single-value format.

## Naming + Data Conventions

- Data structures often include:
  - `id` (uuid)
  - `name` (machine-readable name, must be valid DNS subdomain, convention is using kebab case)
  - `description` (human-readable, if applicable)
- Use UUID (v4) for generating unique identifiers.
- Store dates as i64 epoch timestamps in milliseconds for precise time tracking.
- Timestamps use `*_at` naming with past-tense verbs. For example, `created_at`, `destroyed_at`.

## Code Style

- Hard tabs for Rust formatting (see `rustfmt.toml`).
- Follow existing patterns in neighboring files.
- Always check existing imports and dependencies before adding new ones.
- **Always add imports at the top of the file instead of inline within a function.**
- Prefer explicit `if` or `match` statements over calling `.then(...)` on booleans.
- Warning and error logs should briefly identify the likely operational cause or direction when known.

### Comments

- Write comments as normal, complete sentences. Avoid fragmented structures with parentheticals and dashes like `// Spawn engine (if configured) - regardless of start kind`. Instead, write `// Spawn the engine if configured`. Especially avoid dashes (hyphens are OK).
- Never use em dashes (—) in any plain-English writing (docs, comments, PR descriptions, prose). Use periods to separate sentences instead.
- Documenting deltas is not important or useful. A developer who has never worked on the project will not gain extra information if you add a comment stating that something was removed or changed because they don't know what was there before. The only time you would be adding a comment for something NOT being there is if its unintuitive for why its not there in the first place.

### Match statements

- Never use a `_ =>` fall-through arm when matching on a Rust enum (or a TypeScript discriminated union). Enumerate every variant explicitly so adding a new variant later is a compile error instead of a silent behavior change. `_` is fine for `Result`, `Option`, integers, strings, and other open value spaces. `_ => unreachable!()` / `_ => panic!()` are explicit asserts and acceptable.

## Documentation

- If you need to look at the documentation for a package, visit `https://docs.rs/{package-name}`. For example, serde docs live at `https://docs.rs/serde/`.
- When adding new docs pages, update `website/src/sitemap/mod.ts` so the page appears in the sidebar.
- For the full docs-sync table (limits, config, actor errors, statuses, k8s, landing, sandbox providers, inspector), see `.claude/reference/docs-sync.md`.

## CLAUDE.md conventions

- When adding entries to any CLAUDE.md file, keep them concise. Ideally a single bullet point or minimal bullet points. Do not write paragraphs.
- Only add design constraints, invariants, and non-obvious rules that shape how new code should be written. Do not add general trivia, current implementation wiring, KV-key layouts, module organization, API signatures, ephemeral migration state, or anything a reader can learn by reading the code. That content belongs in module doc-comments, `docs-internal/`, or `.claude/reference/`.
- When the user asks to update any `CLAUDE.md`, add one-line bullet points only, or add a new section containing one-line bullet points.
- Architectural internals and runtime wiring belong in `docs-internal/engine/`. Agent-procedural guides (test-harness gotchas, build troubleshooting, docs-sync tables) belong in `.claude/reference/`. Link them from the [Reference Docs](#reference-docs) index below instead of inlining.
- Every directory that has a `CLAUDE.md` must also have an `AGENTS.md` symlink pointing to it (`ln -s CLAUDE.md AGENTS.md` from the same directory). When creating a new `CLAUDE.md`, create the symlink in the same change.

## Reference Docs

Load these only when the task touches the topic.

### Architecture (`docs-internal/engine/`)

- **[rivetkit-core internals](docs-internal/engine/rivetkit-core-internals.md)** — KV-key layout, storage organization on `ActorContextInner`, startup/shutdown sequences, inspector attach plumbing, schedule dirty-flag, registry dispatch. Read before changing state persistence, lifecycle, or registry wiring.
- **[rivetkit-core state management](docs-internal/engine/rivetkit-core-state-management.md)** — `request_save` / `save_state` / `persist_state` / `set_state_initial` semantics. Keep in sync when changing state APIs.
- **[ActorTask dispatch](docs-internal/engine/actor-task-dispatch.md)** — `DispatchCommand::Action`/`Http`/`OpenWebSocket`, `UserTaskKind` children, `ActorTask` migration status. Read before changing actor task routing.
- **[Inspector protocol](docs-internal/engine/inspector-protocol.md)** — HTTP↔WebSocket mirroring rules, wire-version negotiation, `inspector.*_dropped` downgrades, workflow inspector inference. Read before touching inspector endpoints.
- **[NAPI bridge](docs-internal/engine/napi-bridge.md)** — TSF callback slots, `ActorContextShared` cache reset, `#[napi(object)]` payload rules, cancellation token bridging, error prefix encoding. Read before touching `rivetkit-napi`.
- **[OSS to EE upgrade](docs-internal/engine/oss-to-ee-upgrade.md)** — what survives the switch from an OSS build to an EE build. Read before removing cold-storage schema as dead code, or before changing a compaction workflow's durable steps.
- **[Envoy load balancing](docs-internal/engine/envoy-load-balancing.md)** — Hash-ring layout, virtual nodes, allocator flow, stale-envoy expiry, and tuning. Read before touching pegboard envoy allocation.
- **[BARE protocol crates](docs-internal/engine/bare-protocol-crates.md)** — vbare schema ordering, identity converters, `build.rs` TS codec generation pattern. Read before adding/changing protocol crates.
- **[SQLite VFS parity](docs-internal/engine/sqlite-vfs.md)** — native Rust VFS ↔ WASM TypeScript VFS 1:1 parity rule, v2 storage keys, chunk layout, delete/truncate strategy. Read before touching either VFS.
- **[SQLite optimizations](docs-internal/engine/SQLITE_OPTIMIZATIONS.md)** — brief tracker for SQLite cold-read, VFS, storage, preload, and benchmark optimization ideas.
- **[TLS trust roots](docs-internal/engine/tls-trust-roots.md)** — rustls native+webpki union rationale, which clients use which backend.
- **[RivetKit telemetry](docs-internal/engine/rivetkit-telemetry.md)** — Core-owned invocation and SQLite spans, ray semantics, schedule trace origins in `_rivet_meta`, native OTLP export. Read before touching actor tracing, metrics, or log correlation.
- **[Sleep sequence](docs-internal/engine/sleep-sequence.md)** — engine lifecycle authority, `keepAwake` vs `waitUntil` semantics, grace deadline shutdown-token abort, `can_arm_sleep_timer` vs `can_finalize_sleep` predicates. Read before touching sleep/destroy lifecycle.

### Agent procedural (`.claude/reference/`)

- **[Testing](.claude/reference/testing.md)** — running RivetKit tests, Vitest filter gotchas, driver-test parity workflow, Rust test layout.
- **[Feature flags](.claude/reference/feature-flags.md)** — frontend `features.*` system, deployment-flavor mapping, when to add a flag, consistency rules.
- **[Build troubleshooting](.claude/reference/build-troubleshooting.md)** — DTS failures, NAPI rebuild, `JsActorConfig` field churn, tsup stale exports.
- **[Docs sync](.claude/reference/docs-sync.md)** — full table of "when you change X, update docs Y". Consult before finishing a change.
- **[Content frontmatter](.claude/reference/content-frontmatter.md)** — required frontmatter schemas for docs + blog/changelog.
- **[Examples + Vercel](.claude/reference/examples.md)** — example templates, Vercel mirror regen, common errors.
- **[RivetError system](.claude/reference/error-system.md)** — full derive example, artifact commit rule, anyhow usage.
- **[Dependencies](.claude/reference/dependencies.md)** — pnpm resolutions, Rust workspace deps, dynamic imports, version bumps, reqwest pool.
- When a `utoipa` / OpenAPI enum grows a new API variant, update the checked-in SDK unions under `engine/sdks/{typescript,rust,go}/api-full` in the same change. `engine/artifacts/openapi.json` can be ahead of the generated clients.
