# TODOLIST for PLAN2

Phase-by-phase task list. Each phase ends with an **e2e gate** that must pass before moving on, and a **STOP — discuss** marker.

**E2E maintained at every STOP.** A regression in an earlier phase blocks progress in the current phase. **Feature debt is OK; e2e debt is not.**

Reference: PLAN2.md (design), RIVETKIT_RUST_FIX.md (framework precondition).

---

## Phase 0 — Prerequisites + smoke test target

### Tasks

- [ ] Verify prereqs all build:
  - [ ] `cargo check -p rivetkit`
  - [ ] `cargo check -p rivetkit-core`
  - [ ] `cargo build -p agent-os-sidecar`
  - [ ] `cargo build -p rivet-engine`
  - [ ] `pnpm install` at root
  - [ ] `pnpm -r --filter @rivetkit/workflow-engine --filter @rivetkit/virtual-websocket --filter @rivetkit/engine-envoy-protocol build`
- [ ] Confirm test fixture `rivetkit-typescript/packages/rivetkit/fixtures/driver-test-suite/agent-os.ts` exists (it's just `agentOs({ options: { software: [common] } })` against legacy code).
- [ ] Confirm `createWasmDriverTestConfig` defaults `skip.agentOs = true` (sensible default; add if missing).
- [ ] Write the driver-suite smoke test target as a placeholder in `rivetkit-typescript/packages/rivetkit/tests/driver/actor-agent-os.test.ts`:
  ```ts
  test("writeFile and readFile round-trip", async (c) => {
      const { client } = await setupDriverTest(c, {
          ...driverTestConfig,
          useRealTimers: true,
      });
      const actor = client.agentOsTestActor.getOrCreate([crypto.randomUUID()]);
      await actor.writeFile("/home/user/hello.txt", "hello world");
      const data = await actor.readFile("/home/user/hello.txt");
      expect(new TextDecoder().decode(data)).toBe("hello world");
  }, 60_000);
  ```

### E2E gate

The smoke test fails because no implementation exists yet — that's expected. The test framework must be operational (failure mode is "actor not registered" or "no agentOs implementation," not "test runner broken").

### STOP — discuss

Confirm prereqs all build. Confirm the smoke test target is runnable (even if failing). Decide if anything in the prereq gate failed and how to handle.

---

## RIVETKIT_RUST_FIX — precondition

Land before Phase 1. Three parts; Parts 1+2 essential, Part 3 follow-up.

### Tasks — Part 1 (encode)

- [ ] Create `rivetkit-rust/packages/rivetkit/src/encoding.rs` with `JsonCompatAdapter` serializer that intercepts `serialize_bytes` and emits `["$Uint8Array", base64]`. Only the Uint8Array case.
- [ ] Add `JSON_COMPAT_UINT8_ARRAY = "$Uint8Array"` const (capital U).
- [ ] Add `pub mod encoding;` to `rivetkit-rust/packages/rivetkit/src/lib.rs`.
- [ ] Swap `encode_cbor` for `encode_json_compat` in `Action::ok` (`event.rs:196`).
- [ ] Audit other callers of `encode_cbor` in `event.rs` (`WfHistory::reply`, `WfReplay::reply`). Swap if they forward to JS clients.
- [ ] Add doc-comment to `lib.rs` referencing the TS source-of-truth convention.

### Tasks — Part 2 (decode)

- [ ] Create `rivetkit-rust/packages/client/src/encoding.rs` with `revive_json_compat` walker. Detect `["$Uint8Array", base64]` tagged arrays; recurse; pass through everything else.
- [ ] Hook into the action-response decode site. Find via grep for action result deserialization in `rivetkit-rust/packages/client/`.

### Tasks — Tests

- [ ] `rivetkit-rust/packages/rivetkit/tests/encoding.rs`:
  - [ ] `byte_buf_wraps_as_json_compat_uint8_array`
  - [ ] `nested_byte_field_in_struct_wraps`
  - [ ] `plain_vec_u8_stays_as_array`
  - [ ] `non_byte_types_pass_through_unchanged`
- [ ] `rivetkit-rust/packages/client/tests/encoding.rs`:
  - [ ] `json_compat_uint8_array_revives_to_bytes`
  - [ ] `nested_byte_field_revives_inside_struct`
  - [ ] `non_byte_arrays_pass_through`
  - [ ] `unrelated_tagged_arrays_pass_through`
- [ ] Round-trip test (encode then decode) asserting original bytes recovered.
- [ ] Cross-language parity test:
  - [ ] Rust test writes fixture file.
  - [ ] Vitest test reads fixture, asserts TS `encodeJsonCompatValue` matches and TS `reviveJsonCompatValue` revives correctly.

### E2E gate

- [ ] All Rust encoding tests pass: `cargo test -p rivetkit --test encoding`.
- [ ] All Rust client decoding tests pass: `cargo test -p rivetkit-client --test encoding`.
- [ ] Cross-language fixture parity test passes.

### STOP — discuss

Framework fix is solid. Confirm before adding agent-os on top.

---

## Phase 1a — Rust crate + one action, dispatcher-level e2e

### Tasks

- [ ] Create new crate at `rivetkit-rust/packages/rivetkit-agent-os/`:
  - [ ] `Cargo.toml` with deps on `rivetkit`, `rivetkit-core`, `agent-os-client`, `anyhow`, `tokio`, `tracing`, `serde`, `serde_bytes`, `ciborium`, `futures`.
  - [ ] Add to root workspace members in `Cargo.toml`.
  - [ ] May need to re-pin hickory-resolver to `0.26.0-beta.3` via `cargo update --precise` if the lockfile resolves to a newer beta.
- [ ] `src/actor.rs`: `AgentOsActor` marker struct implementing `Actor` with `Input=()`, `ConnParams=serde_json::Value`, `ConnState=()`, `Action=Raw`.
- [ ] `src/config.rs`: `AgentOsActorConfig` with `build_options: Arc<dyn Fn() -> AgentOsConfig + Send + Sync>` closure factory. Carries `SoftwareInput` with `command_dir: Option<String>` field on the Rust DTO.
- [ ] `src/lib.rs`: `pub fn build_core_factory(config: AgentOsActorConfig) -> CoreActorFactory`. ONE public function.
- [ ] `src/run.rs`: event loop with `ensure_vm` (lazy bring-up, broadcasts `vmBooted`) and `shutdown_vm` (on Sleep/Destroy, broadcasts `vmShutdown`).
- [ ] `src/actions/mod.rs`: `pub async fn dispatch(ctx, vm, action)` with ONE arm: `readFile`. Uses `action.ok(&bytes)` which auto-wraps via the RIVETKIT_RUST_FIX adapter.

### E2E gate — `rivetkit-agent-os/tests/dispatcher_e2e.rs`

Gated on `AGENT_OS_SIDECAR_BIN` env var (skip if missing).

- [ ] Build VM via `AgentOs::create(AgentOsConfig::default())`.
- [ ] Seed a known file via `vm.write_file(...)` directly (bypass dispatcher).
- [ ] Build synthetic `Action` event for `"readFile"`.
- [ ] Call `actions::dispatch(ctx, vm, action)`.
- [ ] Decode reply, assert bytes match what was written.

Run with:
```sh
cargo build -p agent-os-sidecar
AGENT_OS_SIDECAR_BIN=$(pwd)/target/debug/agent-os-sidecar \
    cargo test -p rivetkit-agent-os --test dispatcher_e2e
```

Must pass.

### STOP — discuss

The Rust crate works against a real sidecar at the dispatcher layer. Confirm before adding NAPI.

---

## Phase 1b — NAPI binding, NAPI-level e2e

### Tasks

- [ ] `NapiAgentOsOptions` `#[napi(object)]` in `rivetkit-typescript/packages/rivetkit-napi/src/actor_factory.rs` (or new module) carrying plain-data fields. JSON-envelope fields for nested shapes:
  - `software_json: Option<String>` (JSON-encoded `SoftwareInput[]`)
  - `loopback_exempt_ports: Option<Vec<u32>>`
  - `allowed_node_builtins: Option<Vec<String>>`
  - `module_access_cwd: Option<String>`
  - `additional_instructions: Option<String>`
  - `permissions_json: Option<String>`
  - `mounts_json: Option<String>` (plain-data subset only)
  - `root_filesystem_json: Option<String>`
  - `sidecar_json: Option<String>`
- [ ] `NapiActorFactory::from_agent_os(options, tool_callbacks)` static `#[napi(factory)]` method:
  - Walk `tool_callbacks` JsObject if provided to wrap `execute` fns as TSFs (or stub for now — Phase 5 fully wires this).
  - Build `AgentOsActorConfig` from options.
  - Call `rivetkit_agent_os::build_core_factory(config)`.
  - Store `Arc<CoreActorFactory>` on `inner`.
- [ ] Fail-loud detection: if `options` contains markers for non-serializable fields (`mounts[].driver`, `scheduleDriver`, `sidecar.Explicit`, the three user callbacks), throw a clear "not yet supported on the Rust path" error.
- [ ] Rebuild NAPI: `pnpm --filter @rivetkit/rivetkit-napi build:force`. Verify `from_agent_os` appears in `index.d.ts` as a static method.

### E2E gate — `rivetkit-typescript/packages/rivetkit-napi/tests/agent-os-factory.test.ts`

- [ ] Import `NapiActorFactory` from `@rivetkit/rivetkit-napi`.
- [ ] Construct via `NapiActorFactory.fromAgentOs({ software_json: JSON.stringify([{ package: "@rivet-dev/agent-os-common", commandDir: "/path" }]) }, undefined)`. Assert non-null handle.
- [ ] Construct with deliberately bad config (non-data driver field). Assert throws fail-loud error.

Plus: **Phase 1a's `dispatcher_e2e.rs` must still pass.** No regressions.

### STOP — discuss

NAPI binding works. The Rust crate is reachable from JS. Confirm before adding JS shim layer.

---

## Phase 1c — JS shim + engine driver branches, full driver-suite e2e

### Tasks

- [ ] Add optional `nativeFactoryBuilder?: (runtime: CoreRuntime) => ActorFactoryHandle` to `AnyActorDefinition` interface in `actor/definition.ts`, and to `ActorDefinition` class as defaultable instance property.
- [ ] Rewrite `agentOs(config)` in `src/agent-os/actor/index.ts`:
  - Parse config via existing `agentOsActorConfigSchema`.
  - Build `nativeFactoryBuilder` lazy closure:
    - Reject if `runtime.kind !== "napi"` with clear error.
    - Build `NapiAgentOsOptions` from parsed config (thread `software` with `commandDir`).
    - Extract tool callbacks from `parsed.options.toolKits` (stub for Phase 1c — pass `undefined`).
    - Call `runtime.createAgentOsFactory(options, toolCallbacks)`, cast to `ActorFactoryHandle`.
  - Return `new ActorDefinition({} as any)` with `nativeFactoryBuilder` set.
- [ ] Delete the legacy `agentOs()` body and the `buildXActions` imports in the same file.
- [ ] Delete `src/agent-os/actor/{cron,db,filesystem,network,preview,process,session,shell}.ts`.
- [ ] Update `src/agent-os/index.ts` barrel to drop the deleted exports.
- [ ] Add `createAgentOsFactory?(options, toolCallbacks): ActorFactoryHandle` to `CoreRuntime` interface in `registry/runtime.ts`.
- [ ] `napi-runtime.ts::NapiCoreRuntime`:
  - [ ] Implement `createAgentOsFactory(options, toolCallbacks)` calling `NapiActorFactory.fromAgentOs(...)` and returning the handle.
  - [ ] **Widen `registerActor` signature** to take `definition: AnyActorDefinition` + `registryConfig: RegistryConfig`. Inside, do the dispatch: `if (definition.nativeFactoryBuilder) → call it; else → buildNativeFactory`.
- [ ] `wasm-runtime.ts::WasmCoreRuntime`:
  - [ ] Mirror the widened `registerActor` signature. For agent-os actors (`nativeFactoryBuilder` set), throw "not supported."
- [ ] `registry/native.ts::buildConfiguredRegistry`: loop becomes `runtime.registerActor(registry, name, definition, config)`.
- [ ] `drivers/engine/actor-driver.ts`: add branch between dynamic and static:
  ```ts
  } else if ((definition as any).nativeFactoryBuilder) {
      // Rust-backed; CoreActorFactory handles everything.
      logger().debug({ msg: "engine actor started (rust-native)", actorId, name, key });
  }
  ```
- [ ] Add `writeFile` arm to `actions::dispatch` in the Rust crate (smoke test needs both).

### E2E gate — Phase 0's smoke test passes for bare encoding

```sh
AGENT_OS_SIDECAR_BIN=$(pwd)/target/debug/agent-os-sidecar \
RIVET_ENGINE_BINARY=$(pwd)/target/debug/rivet-engine \
    pnpm vitest run tests/driver/actor-agent-os.test.ts \
    --bail=1 -t "writeFile and readFile round-trip"
```

Bare encoding cell must pass.

Plus: **Phase 1a tests AND Phase 1b test must still pass.**

### STOP — discuss

End-to-end works. JS → engine → NAPI → Rust → sidecar → VM → bytes back. **This is the milestone that means the architecture is real.** Confirm before expanding.

---

## Phase 2 — Cross-encoding parity

### Tasks

- [ ] Verify Phase 0's smoke test passes for cbor encoding (should "just work" because RIVETKIT_RUST_FIX wraps bytes at the framework layer).
- [ ] Verify the same for json encoding.
- [ ] If either fails, narrow: was RIVETKIT_RUST_FIX applied to every relevant `encode_cbor` call? Did the dispatcher arm use `action.ok(&bytes)` correctly?
- [ ] Add one structured-object action (e.g. `stat`) to validate non-byte shapes round-trip across all three encodings.

### E2E gate

- [ ] `writeFile and readFile round-trip` passes for bare + cbor + json.
- [ ] `stat returns file metadata` passes for bare + cbor + json.
- [ ] All Phase 1 e2e gates still pass.

### STOP — discuss

Cross-encoding works. Confirm before bulk action buildout.

---

## Phase 3 — Action surface buildout

Build out the remaining ~49 actions. After each category, run the full driver suite **no-bail** to catch independent failures.

### Filesystem

- [ ] Add arms: `mkdir`, `readdir`, `stat`, `exists`, `move`, `deleteFile`, `readFiles`, `writeFiles`, `readdirRecursive`.
- [ ] Wire DTOs land with their first feeder arm.
- [ ] **Driver-suite gate for filesystem category (no `--bail`).**

### Process

- [ ] Add arms: `exec`, `spawn`, `waitProcess`, `killProcess`, `stopProcess`, `listProcesses`, `allProcesses`, `processTree`, `getProcess`, `writeProcessStdin`, `closeProcessStdin`.
- [ ] Wire `processOutput` + `processExit` broadcasts inside `spawn` / `exec` arms.
- [ ] Increment `RunState.active_processes` on spawn, decrement on exit.
- [ ] **Driver-suite gate for process category.**

### Shell

- [ ] Add arms: `openShell`, `writeShell`, `resizeShell`, `closeShell`.
- [ ] Wire `shellData` broadcast inside `openShell` arm.
- [ ] Increment/decrement `RunState.active_shells`.
- [ ] **Driver-suite gate for shell category.**

### Session

- [ ] Add arms: `createSession`, `sendPrompt`, `cancelPrompt`, `respondPermission`, `closeSession`, `destroySession`, `resumeSession`, `listSessions`, `getSession`, `setMode`, `getModes`, `setModel`, `setThoughtLevel`, `getConfigOptions`, `getEvents`, `getSequencedEvents`, `rawSend`, `listAgents`.
- [ ] Wire `sessionEvent` + `permissionRequest` broadcasts inside `createSession`.
- [ ] Increment/decrement `RunState.active_sessions`.
- [ ] **Driver-suite gate for session category.**

### Cron

- [ ] Add arms: `scheduleCron`, `listCronJobs`, `cancelCronJob`.
- [ ] Wire `cronEvent` broadcast (always-on per VM).
- [ ] **Driver-suite gate for cron category.**

### Network

- [ ] Add arm: `vmFetch`.
- [ ] **Driver-suite gate for network category.**

### Misc session bookkeeping (persistence-backed)

- [ ] `listPersistedSessions` querying `agent_os_sessions`.
- [ ] `getSessionEvents` querying `agent_os_session_events`.
- [ ] **Driver-suite gate.**

### E2E gate (phase boundary)

Full driver suite no-bail. All categories green except preview (Phase 4) and tool actions (Phase 5).

Plus all earlier e2e gates still pass.

### STOP — discuss

Action surface built out and validated. Confirm before preview + toolkits.

---

## Phase 4 — Persistence + preview

### Tasks

- [ ] `src/persistence.rs`: `MIGRATION_SQL` const with 4 agent-os tables + indexes (port from TS `actor/db.ts`). `pub const MIGRATION_SQL: &str`.
- [ ] `pub async fn migrate_actor(ctx: &Ctx<AgentOsActor>) -> Result<()>` — calls `sql.exec(MIGRATION_SQL)` if SQLite enabled, idempotent.
- [ ] Call `migrate_actor` at the top of `run::run` before the event loop.
- [ ] `tests/persistence.rs` rusqlite unit tests covering migration validity.
- [ ] `src/actions/preview.rs`: `generate_token` + `create_signed_preview_url` + `expire_signed_preview_url`. SQLite-backed via `ctx.sql()`.
- [ ] `src/preview_http.rs`: `parse_fetch_path` + `handle(ctx, vm, http)` for `/fetch/{token}/path` URLs. **Forward all request headers** to the VM fetch (don't drop them).
- [ ] Wire preview handler into `run::run`'s `Event::Http` arm.
- [ ] `tests/preview_http.rs` unit tests for path parser + token generator.

### E2E gate

- [ ] `tests/persistence.rs` passes.
- [ ] `tests/preview_http.rs` passes.
- [ ] Driver-suite preview tests pass (`createSignedPreviewUrl`, token round-trip via `/fetch/{token}/path`, `expireSignedPreviewUrl`).
- [ ] All earlier e2e gates still pass.

### STOP — discuss

Persistence + preview works. Confirm before toolkit callbacks.

---

## Phase 5 — Toolkit callbacks

### Tasks

- [ ] In `NapiActorFactory::from_agent_os`, walk `tool_callbacks` JsObject. Extract `execute: JsFunction` for each `"<toolkit>:<tool>"` key. Wrap as `ThreadsafeFunction<ToolInvocationPayload>`.
- [ ] Plug TSF callbacks into `AgentOsConfig::tool_kits` via a `ToolCallback` Arc closure that dispatches through the TSF.
- [ ] In JS `agentOs()` shim's `nativeFactoryBuilder`, walk `parsed.options.toolKits[*].tools[*]`, build the `{"<toolkit>:<tool>": executeFn}` JsObject, pass to `runtime.createAgentOsFactory`.
- [ ] Driver-suite test: register an actor with a host tool, send a prompt that triggers the tool, assert JS `execute` ran and result reached the session.

### Risk callout

Highest-risk phase. TSF lifetime + cancellation token bridging across `napi_actor_events.rs` is a known minefield. If lifetime bugs surface, the rollback is to ship Phases 0–4 without host-tool support and merge.

### E2E gate

- [ ] Driver-suite host-tool round-trip test passes.
- [ ] All earlier e2e gates still pass.

### STOP — discuss

Toolkits work end-to-end. Confirm before cleanup.

---

## Phase 6 — Cleanup + docs

### Tasks — final cleanup pass

- [ ] Grep for any orphan code (unused helpers, dead constants, unused wire DTOs from earlier phases).
- [ ] Grep for diagnostic logs: `grep -rE "console\.error|tracing::debug" rivetkit-typescript/packages/rivetkit/src/agent-os rivetkit-rust/packages/rivetkit-agent-os/src` — clean up.
- [ ] Confirm `RunState` counters are wired throughout (no unincremented counter sets).
- [ ] Confirm every wire DTO has a feeder.
- [ ] Confirm every event broadcast constant has a `Subscriptions::spawn_*` feeder.

### Tasks — docs

- [ ] Add `rivetkit-rust/packages/rivetkit-agent-os/CLAUDE.md`.
- [ ] Add `AGENTS.md` symlink (`ln -s CLAUDE.md AGENTS.md` from the package dir).
- [ ] Add bullet to root `CLAUDE.md` under "RivetKit Layer Architecture" describing the new crate.
- [ ] Update website docs for the new agent-os actor surface.

### Tasks — lint + format

- [ ] `cargo clippy -p rivetkit-agent-os -- -W warnings` clean.
- [ ] `pnpm biome check` clean on changed files.
- [ ] `pnpm tsc --noEmit` baseline clean (modulo pre-existing workspace issues).

### Tasks — final regression

- [ ] Full driver suite no-bail. Every cell green (modulo intentional skips).
- [ ] `cargo test -p rivetkit-agent-os` clean.
- [ ] Rebuild NAPI: `pnpm --filter @rivetkit/rivetkit-napi build:force`.
- [ ] Quickstart smoke (`ext/agent-os/examples/quickstart/`) runs against the new path.

### E2E gate

Final driver suite no-bail run. All cells green. Bytes round-trip on all three encodings.

### STOP — discuss

Ready to land. Confirm before merging.

---

## Review checklist (apply at every PR within these phases)

Pulled from PLAN2.md "Review checklist." Applies to **every PR**, not just phase boundaries:

- [ ] No DTO added without a feeder in the same PR.
- [ ] No event broadcast constant without a `Subscriptions::spawn_*` feeder.
- [ ] No `RunState` counter introduced without dispatcher-arm increment + decrement.
- [ ] No abstraction (trait, generic wrapper) with one impl.
- [ ] No orphan method (grep the workspace before merge).
- [ ] User config flows or fails loud — never silently dropped.
- [ ] Byte payloads go through `Action::ok` wrapping (post-RIVETKIT_RUST_FIX).
- [ ] No legacy paths kept as fallback after a cutover.
- [ ] No diagnostic logs left in.
- [ ] Commit messages don't say "verified" without naming the test that ran.
- [ ] All prior sub-phase e2e tests still pass.
- [ ] NAPI artifact rebuilt if Rust touched.

---

## Notes

- **E2E maintained:** at every STOP, all prior phase e2e tests must still pass.
- **Stop and discuss:** don't move past a STOP without confirming the gate.
- **Feature debt is OK; e2e debt is not.** A missing action gets added in Phase 3. A failing driver-suite cell means the architecture has a leak that more code will only obscure.
