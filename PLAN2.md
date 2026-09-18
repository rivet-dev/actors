# Plan 2 — agent-os integration

NAPI-only Rust-backed agent-os actor for rivetkit. Written after a first attempt overcomplicated the architecture; reviewed by 5 subagents (architecture, feasibility, test strategy, risks, adversarial) and corrected based on findings.

---

## Scope

**In:** NAPI-only agent-os actor. JS users on the native runtime call `agentOs(config)` and get a working Rust-backed actor.

**Out:** wasm runtime (agent-os-client uses `tokio::process`, native-only). Rust-direct registration (no caller). Future actor kinds beyond agent-os.

---

## Architectural principles

1. **One NAPI class, one register method.** No separate `NapiAgentOsDefinition`. agent-os produces a `NapiActorFactory` via a static constructor and registers through the existing `register(name, &NapiActorFactory)` path.
2. **Discriminator via marker field, not class hierarchy.** `ActorDefinition` grows an optional `nativeFactoryBuilder?: (runtime: CoreRuntime) => ActorFactoryHandle` field. Two call sites (registry loop, engine driver ladder) check that one field.
3. **Validation drives ordering.** Get one test through the driver suite (the real end-to-end gate) before building anything else. No structural scaffolding before signal.
4. **No premature abstractions.** No `HostToolInvoker` trait. No `entry_fn` + `actor_config` + `build_core_factory` three-way API — just `build_core_factory(config) -> CoreActorFactory`. No event broadcast constants without feeders. Wire DTOs land in the same PR as their first feeder action, never alone.
5. **User config is load-bearing from Day 1 for the serializable subset.** `software`, `loopbackExemptPorts`, `allowedNodeBuiltins`, `moduleAccessCwd`, `additionalInstructions`, `permissions`, `rootFilesystem`, `sidecar.Shared`, plain `mounts` flow through. Non-serializable fields (`mounts[].driver` callbacks, `scheduleDriver`, `sidecar.Explicit`, `onBeforeConnect`/`onSessionEvent`/`onPermissionRequest` callbacks) **fail loud** with a clear "not yet supported" error. Never silently drop user config.
6. **TypeScript is the source of truth for the wire protocol.** Byte payloads use the rivetkit convention `["$Uint8Array", base64]` because that's what TS emits and expects. The Rust framework must mirror it on both encode and decode sides (see RIVETKIT_RUST_FIX.md — this is a framework fix, not agent-os work).

---

## Architecture

### Rust crate `rivetkit-agent-os` (native-only)

```
src/
  lib.rs           — pub fn build_core_factory(config: AgentOsActorConfig)
                     -> CoreActorFactory
  actor.rs         — AgentOsActor marker (impl Actor)
  config.rs        — AgentOsActorConfig (closure factory for AgentOsConfig
                     so we can rebuild it across sleep/wake cycles)
  run.rs           — event loop: ensure_vm, shutdown_vm, dispatch
  actions/
    mod.rs         — pub async fn dispatch(ctx, vm, action)
    filesystem.rs  — read_file, write_file, ...
    session.rs     — create_session, send_prompt, ...
    process.rs     — exec, spawn, ...
    shell.rs       — open_shell, ...
    cron.rs        — schedule_cron, ...
    network.rs     — vm_fetch
    preview.rs     — create_signed_preview_url, expire_signed_preview_url
  events.rs        — Subscriptions: per-VM cron broadcast + per-entity
                     session/process/shell broadcasts wired from
                     dispatcher arms when entities are created
  persistence.rs   — MIGRATION_SQL + migrate(sql)
  preview_http.rs  — handle(ctx, vm, http) for /fetch/{token}/path
  state.rs         — RunState counters (populated by dispatcher arms)
```

One public function: `build_core_factory(config) -> CoreActorFactory`. No separate `AgentOsActorDefinition` struct, no `entry_fn`/`actor_config` accessors.

### NAPI binding `rivetkit-napi`

Add a static constructor to the existing `NapiActorFactory`:

```rust
#[napi]
impl NapiActorFactory {
    /// Existing constructor for JS-callback actors.
    #[napi(constructor)]
    pub fn constructor(callbacks: JsObject, config: Option<JsActorConfig>)
        -> napi::Result<Self> { ... }

    /// New: build a NapiActorFactory backed by rivetkit-agent-os.
    /// Walks `tool_callbacks` JsObject to extract execute fns,
    /// builds AgentOsActorConfig from `options`, calls
    /// `rivetkit_agent_os::build_core_factory(config)`.
    #[napi(factory)]
    pub fn from_agent_os(
        options: NapiAgentOsOptions,
        tool_callbacks: Option<JsObject>,
    ) -> napi::Result<Self> { ... }
}
```

Zero changes to the existing `register` method.

### TypeScript `rivetkit`

Three small changes:

**Change 1: `AnyActorDefinition` and `ActorDefinition` grow an optional field.**
```ts
export interface AnyActorDefinition {
    readonly config: any;
    readonly nativeFactoryBuilder?: (runtime: CoreRuntime) => ActorFactoryHandle;
}
```

The `ActorDefinition` class gets the field as a defaultable instance property so `instanceof ActorDefinition` still works downstream.

**Change 2: `agentOs(config)` returns a real `ActorDefinition` instance with `nativeFactoryBuilder` set.**

Lazy — `agentOs()` runs at module load time before the runtime is selected. The builder runs at registration time when the runtime is known. Mirrors the Cloudflare Workers global-scope rule.

```ts
// src/agent-os/actor/index.ts
export function agentOs(config): ActorDefinition<...> {
    const parsed = agentOsActorConfigSchema.parse(config);
    const nativeFactoryBuilder = (runtime: CoreRuntime): ActorFactoryHandle => {
        if (runtime.kind !== "napi") {
            throw new Error(
                "agentOs() requires the NAPI runtime; the wasm runtime does not support agent-os actors",
            );
        }
        const options = toNapiAgentOsOptions(parsed);
        const toolCallbacks = extractToolCallbacks(parsed);
        return runtime.createAgentOsFactory!(options, toolCallbacks);
    };
    const definition = new ActorDefinition({} as any);
    (definition as any).nativeFactoryBuilder = nativeFactoryBuilder;
    return definition;
}
```

**Change 3: Dispatch lives inside `runtime.registerActor`. The registry loop is a one-liner.**

```ts
// registry/native.ts::buildConfiguredRegistry
for (const [name, definition] of Object.entries(config.use)) {
    runtime.registerActor(registry, name, definition, config);
}
```

```ts
// registry/napi-runtime.ts::NapiCoreRuntime
registerActor(
    registry: RegistryHandle,
    name: string,
    definition: AnyActorDefinition,
    registryConfig: RegistryConfig,
): void {
    const factory = (definition as any).nativeFactoryBuilder
        ? (definition as any).nativeFactoryBuilder(this)
        : buildNativeFactory(this, registryConfig, definition);
    asNativeRegistry(registry).register(name, asNativeFactory(factory));
}
```

`registerActor`'s signature widens to take an `AnyActorDefinition` + the `RegistryConfig`. Wasm-runtime mirrors the same dispatch with its own fallback (or throws for the agent-os case).

**Engine driver ladder** (`drivers/engine/actor-driver.ts`) — between dynamic and static branches:
```ts
} else if ((definition as any).nativeFactoryBuilder) {
    // Rust-backed actor; CoreActorFactory handles everything.
    // handler.actor stays undefined.
} else if (isStaticActorDefinition(definition)) {
    ...
}
```

`handler.actor` undefined is safe because the only blocking accessor (`#loadActorHandler`) is called from hibernating-WS code that agent-os doesn't use.

**Keep on `CoreRuntime`:**
- `createAgentOsFactory?(options, toolCallbacks): ActorFactoryHandle` — named capability seam per `rivetkit-typescript/CLAUDE.md` "Runtime Boundary" rule. Wasm throws "not supported."

**Delete:**
- Legacy `agentOs()` body (the `actor({...})` callback bag).
- `src/agent-os/actor/{cron,db,filesystem,network,preview,process,session,shell}.ts` (the buildXActions modules).

---

## Implementation order

Each phase has its own e2e gate. See TODOLIST.md for the concrete checklist.

**Phase 0** — prerequisites + smoke test target.

**RIVETKIT_RUST_FIX** — precondition. Framework byte-encoding fix (see RIVETKIT_RUST_FIX.md).

**Phase 1a** — Rust crate skeleton + ONE action (`readFile`) + dispatcher e2e against real sidecar. No NAPI, no engine, no JS.

**Phase 1b** — NAPI `NapiActorFactory::from_agent_os` + focused Vitest. 1a still passes.

**Phase 1c** — JS shim + ladder branches + first driver-suite cell green (bare encoding). 1a + 1b still pass. *This is the "architecture is real" milestone.*

**Phase 2** — cross-encoding parity (cbor + json work without per-action fixes thanks to RIVETKIT_RUST_FIX). Test one structured-object action across all three encodings.

**Phase 3** — action surface buildout, category by category. Driver suite no-bail at each category.

**Phase 4** — persistence + preview HTTP handler.

**Phase 5** — toolkit callbacks (highest risk).

**Phase 6** — cleanup + docs.

---

## Tests

| Layer | Test file | Purpose |
|---|---|---|
| Unit | `rivetkit-agent-os/tests/persistence.rs` | MIGRATION_SQL valid via rusqlite |
| Unit | `rivetkit-agent-os/tests/preview_http.rs` | path parser + token generator |
| Helper-e2e | `rivetkit-agent-os/tests/end_to_end.rs` | helpers against real sidecar (gated) |
| Dispatcher-e2e | `rivetkit-agent-os/tests/dispatcher_e2e.rs` | dispatch arms against real sidecar (gated) |
| Cutover | `rivetkit-typescript/.../tests/agent-os-cutover.test.ts` | `agentOs()` returns expected shape |
| End-to-end | `rivetkit-typescript/.../tests/driver/actor-agent-os.test.ts` | full chain via engine + sidecar |
| Sleep/wake | `rivetkit-typescript/.../tests/driver/actor-agent-os-sleep.test.ts` | VM recreates with user config on wake; catches the "default VM after wake" regression |

**Inner loop:** `dispatcher_e2e.rs` is the per-save gate. ~2-second feedback against a real sidecar. No engine.

**Boundary gate:** the driver suite at phase boundaries. **No `--bail`** at boundaries — the full suite must be green. `--bail=1` is for local fix-and-retry only.

---

## Review checklist (every PR)

**No premature scaffolding:**
- [ ] Every wire DTO receives a real value from a real caller in this PR. No DTOs added "for the next action."
- [ ] Every event broadcast name constant is fed by an actual `Subscriptions::spawn_*` call.
- [ ] Every `RunState` counter is incremented in the dispatcher arm that creates the entity AND decremented in the arm that destroys it.
- [ ] No abstraction (trait, generic wrapper) with one concrete impl.
- [ ] No orphan methods left after refactors (grep the workspace).

**Full data flow, no stubs:**
- [ ] User config flows from `agentOs(config)` → `NapiAgentOsOptions` → `AgentOsActorConfig` → `AgentOsConfig` for every plain-data field, OR an explicit fail-loud error is raised for non-serializable fields.

**Wire format hygiene:**
- [ ] Byte payloads (top-level and nested) go through the RIVETKIT_RUST_FIX `Action::ok` wrapping. Never raw `serde_bytes::ByteBuf` or `Vec<u8>` to client.

**Cleanup discipline:**
- [ ] No legacy code paths left as "fallback" — if the new path is the path, the old path is deleted in the same PR.
- [ ] No diagnostic `console.error` / `tracing::debug!` added during debugging that survives commit.

**Verified, not just compiled:**
- [ ] No commit message says "verified" without naming the exact test (file + name) that was run and passed.
- [ ] `cargo check` and `pnpm tsc --noEmit` are baselines, not gates. The actual gate is the e2e test from the relevant sub-phase.

**Sub-phase regression check:**
- [ ] All prior sub-phase e2e tests still pass after each subsequent change.

**Build hygiene:**
- [ ] If Rust touched, NAPI artifact rebuilt (`pnpm --filter @rivetkit/rivetkit-napi build:force`).
- [ ] Driver suite ran at phase boundaries without `--bail`.

Note on `pnpm build` for `rivetkit/`: not required for src edits — `vitest.config.ts` uses `vite-tsconfig-paths` so `rivetkit/agent-os` resolves to `src/agent-os/index.ts` during test runs. `dist/` build matters only for external consumers.

---

## Resolved questions

1. **`nativeFactoryBuilder` typing:** `ActorFactoryHandle` opaque. The engine driver passes it through to `runtime.registerActor` and never inspects.
2. **Construction timing:** lazy builder. `agentOs(config)` runs at module load before runtime selection. The registration loop calls the builder once the runtime is known.
3. **Wire DTOs location:** inline in each action module.
4. **`skip.agentOs` on wasm:** `createWasmDriverTestConfig` defaults it to `true`.

## Open question to resolve at Phase 1 start

**Fail-loud strategy for non-serializable config fields.** Decide which fields throw vs. silently drop with a warning. The conservative call is fail-loud for everything that can't round-trip cleanly. Decide before Phase 1b's `NapiAgentOsOptions` shape lands.

---

## Slip risk

| Phase | Slip risk |
|---|---|
| 0 | Low — clean state |
| RIVETKIT_RUST_FIX | Medium — custom serializer adapter; well-defined |
| 1a | Low — `dispatcher_e2e.rs` is a focused gate |
| 1b | Low — `#[napi(factory)]` is established |
| 1c | Low if discovered; Medium if a new layer-mismatch surfaces |
| 2 | Low — Parts 1+2 of the fix handle byte encoding |
| 3 | Low — pattern repeats per arm |
| 4 | Low |
| 5 | **High** — TSF lifetime + cancellation token bridging across `napi_actor_events.rs` is a known minefield |
| 6 | Low |

**Rollback for high-risk phases:**
- RIVETKIT_RUST_FIX: ship Phase 3 with `bare`-encoding-only and document.
- Phase 5: ship Phases 0–4 without host-tool support, document, merge. The other 50+ actions are usable without tools.
