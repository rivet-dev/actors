---
name: driver-test-runner
description: Methodically run the RivetKit driver test suite file by file across the native (NAPI) and wasm runtimes, tracking progress in ~/.agents/notes/driver-test-progress.md. Use when you need to validate the driver test suite after changes, bring up a new driver, or debug test failures systematically.
allowed-tools: Bash, Read, Write, Edit, Grep, Glob, Agent, TaskCreate, TaskUpdate
---

# Driver Test Suite Runner

Methodically run the RivetKit driver test suite one file group at a time across the native (NAPI) and wasm runtimes, tracking progress in `~/.agents/notes/driver-test-progress.md`.

## Arguments

The skill accepts optional arguments:

- **`reset`** — Clear progress and start from the beginning.
- **`resume`** — Pick up from where we left off (default behavior).
- **`only <file>`** — Run only a specific test file group (e.g., `only actor-conn`).
- **`from <file>`** — Start from a specific file group, skipping earlier ones.
- **`runtime <native|wasm|both>`** — Which runtime(s) to run (default: `both`).
- **`encoding <bare|cbor|json>`** — Override encoding (default: `bare`).
- **`registry <static>`** — Override registry type (default: `static`).

## Runtime Matrix

The driver suite runs over a runtime × SQLite-backend × encoding matrix defined in `rivetkit-typescript/packages/rivetkit/tests/driver/shared-matrix.ts`. The runtime dimension has two values:

- **`native`** — NAPI bindings (`@rivetkit/rivetkit-napi`). Pairs with `sqlite=local` for the primary native driver pass.
- **`wasm`** — WebAssembly bindings (`@rivetkit/rivetkit-wasm`). Wasm **cannot** use local SQLite; it must pair with `sqlite=remote` (executes SQL through the engine over the wire).

The skill defaults to running each test file twice: once on `native/local` and once on `wasm/remote`, each at `encoding=bare`. A file is checked off only when both runtimes pass.

The test harness does not read environment variables for matrix selection. Always select matrix cells with Vitest `-t` using the full inner describe name: `runtime (<runtime>) / sqlite (<backend>) / encoding (<encoding>)`.

## How It Works

### 0. Anchor the reference before fixing parity bugs

If a RivetKit driver test fails because native or Rust behavior diverges from the old runtime, do this before inventing a separate debugging workflow:

1. Treat `rivetkit-typescript/packages/rivetkit` driver tests as the primary oracle.
2. Compare the failing behavior against the original TypeScript implementation at ref `feat/sqlite-vfs-v2` using `git show` or `git diff`.
3. Patch native/Rust to match the original TypeScript behavior.
4. Rerun the same TypeScript driver test before adding any lower-level native tests.

If a test passes on `native` but fails on `wasm` (or vice versa), the divergence is in the runtime adapter (`packages/rivetkit/src/registry/wasm-runtime.ts` or `napi-runtime.ts`) or in `rivetkit-core`'s wasm/native feature gates — not in user-facing actor code.

Native unit tests are allowed only after the failing TypeScript driver test has reproduced the bug and after the fix is validated against that same TypeScript driver test.

### 1. Ensure runtime artifacts are built

Both runtime adapters need their build outputs on disk before the suite can load them. A fresh checkout, a Rust edit under `packages/rivetkit-napi` / `packages/sqlite-native`, or any change under `packages/rivetkit-wasm` invalidates these.

**NAPI (`@rivetkit/rivetkit-napi`)** — produces a platform-specific `.node` next to `package.json`:

```bash
ls rivetkit-typescript/packages/rivetkit-napi/*.node 2>/dev/null
# missing? rebuild:
pnpm --filter @rivetkit/rivetkit-napi run build:force
```

After Rust changes, always use `build:force` (per `rivetkit-typescript/CLAUDE.md`); the non-`:force` variant can skip the rebuild and leave the suite running against a stale `.node`.

**Wasm (`@rivetkit/rivetkit-wasm`)** — produces `packages/rivetkit-wasm/pkg/rivetkit_wasm.{js,wasm,d.ts}`:

```bash
ls rivetkit-typescript/packages/rivetkit-wasm/pkg/rivetkit_wasm.wasm 2>/dev/null
# missing? rebuild (uses the package-pinned wasm-pack, do not use npx):
pnpm --filter @rivetkit/rivetkit-wasm run build
```

Skip the wasm build only if `runtime` is `native` and you're certain the wasm fixture path won't be loaded. With the default `runtime=both`, the wasm build is always required.

### 2. Ensure the engine is running

Before running any tests, check if the RocksDB engine is already running:

```bash
curl -sf http://127.0.0.1:6420/health || echo "NOT RUNNING"
```

If it's not running, start it:

```bash
./scripts/run/engine-rocksdb.sh >/tmp/rivet-engine-startup.log 2>&1 &
```

Wait for health check to pass (poll every 2 seconds, up to 60 seconds).

### 3. Initialize or load progress file

The progress file lives at `~/.agents/notes/driver-test-progress.md`. If it doesn't exist or `reset` was passed, create it with the template below. If it exists and `resume` was passed, read it and pick up from the first file with an unchecked runtime box.

Each file row gets two checkboxes — one for each runtime. Check off a runtime independently as soon as it passes, and only advance to the next file when both runtimes for the current file are checked.

Progress file template:

```markdown
# Driver Test Suite Progress

Started: <timestamp>
Config: registry (static), encoding (bare), runtimes (native, wasm)

Each row: `[native] [wasm] <file> | <suite description>`

## Fast Tests

- [ ] [ ] manager-driver | Manager Driver Tests
- [ ] [ ] actor-conn | Actor Connection Tests
- [ ] [ ] actor-conn-state | Actor Connection State Tests
- [ ] [ ] conn-error-serialization | Connection Error Serialization Tests
- [ ] [ ] actor-destroy | Actor Destroy Tests
- [ ] [ ] request-access | Request Access in Lifecycle Hooks
- [ ] [ ] actor-handle | Actor Handle Tests
- [ ] [ ] action-features | Action Features Tests
- [ ] [ ] access-control | access control
- [ ] [ ] actor-vars | Actor Variables
- [ ] [ ] actor-metadata | Actor Metadata Tests
- [ ] [ ] actor-telemetry | Actor Telemetry
- [ ] [ ] actor-onstatechange | Actor State Change Tests
- [ ] [ ] actor-db | Actor Database
- [ ] [ ] actor-db-raw | Actor Database Raw Tests
- [ ] [ ] actor-db-init-order | Actor Db Init Order
- [ ] [ ] actor-workflow | Actor Workflow Tests
- [ ] [ ] actor-error-handling | Actor Error Handling Tests
- [ ] [ ] actor-queue | Actor Queue Tests
- [ ] [ ] actor-kv | Actor KV Tests
- [ ] [ ] actor-stateless | Actor Stateless Tests
- [ ] [ ] raw-http | raw http
- [ ] [ ] raw-http-request-properties | raw http request properties
- [ ] [ ] raw-websocket | raw websocket
- [ ] [ ] actor-inspector | Actor Inspector Tests
- [ ] [ ] gateway-query-url | Gateway Query URL Tests
- [ ] [ ] actor-db-pragma-migration | Actor Database Pragma Migration
- [ ] [ ] actor-state-zod-coercion | Actor State Zod Coercion
- [ ] [ ] actor-conn-status | Connection Status Changes
- [ ] [ ] gateway-routing | Gateway Routing
- [ ] [ ] lifecycle-hooks | Lifecycle Hooks
- [ ] [ ] serverless-handler | Serverless Handler Tests

## Slow Tests

- [ ] [ ] actor-state | Actor State Tests
- [ ] [ ] actor-save-state | Actor Save State Tests
- [ ] [ ] actor-schedule | Actor Schedule Tests
- [ ] [ ] actor-sleep | Actor Sleep Tests
- [ ] [ ] actor-sleep-db | Actor Sleep Database Tests
- [ ] [ ] actor-lifecycle | Actor Lifecycle Tests
- [ ] [ ] actor-conn-hibernation | Actor Connection Hibernation Tests
- [ ] [ ] actor-run | Actor Run Tests
- [ ] [ ] hibernatable-websocket-protocol | hibernatable websocket protocol
- [ ] [ ] actor-db-stress | Actor Database Stress Tests

## Excluded

- [ ] [ ] actor-agent-os | Actor agentOS Tests (skip unless explicitly requested)

## Log
```

### 4. Run tests file by file

For each unchecked row in order, run the runtimes selected by the `runtime` arg (default `both`). For each runtime:

**a) Pick the runtime/sqlite pair:**

| Runtime | SQLite backend |
|---------|----------------|
| native  | local          |
| wasm    | remote         |

**b) Build the filter command:**

Each suite lives in its own file under `rivetkit-typescript/packages/rivetkit/tests/driver/<file>.test.ts`. The describe block nesting is:

```
<Outer Suite> > static registry > runtime (<runtime>) / sqlite (<backend>) / encoding (<encoding>) > <Suite Description>
```

Always use Vitest `-t` for driver matrix cells. Include runtime, SQLite backend, and encoding in the pattern so a partial match does not accidentally select another matrix cell.

Base command (native):

```bash
cd rivetkit-typescript/packages/rivetkit && \
  pnpm test tests/driver/<FILE>.test.ts \
    -t "runtime \(native\) / sqlite \(local\) / encoding \(bare\)" \
    > /tmp/driver-test-current.log 2>&1
echo "EXIT: $?"
```

Base command (wasm):

```bash
cd rivetkit-typescript/packages/rivetkit && \
  pnpm test tests/driver/<FILE>.test.ts \
    -t "runtime \(wasm\) / sqlite \(remote\) / encoding \(bare\)" \
    > /tmp/driver-test-current.log 2>&1
echo "EXIT: $?"
```

Replace `<FILE>` with the file name stem (part before the `|` in the progress file) and `<SUITE_DESCRIPTION>` with the suite description (part after the `|`). Escape parentheses in the description if present. Forward slashes inside the describe path do not need to be escaped.

**Important:** The suite description in the `-t` filter must match the inner `describe(...)` text in the test file exactly. Some mappings:

| File | Suite Description Text |
|------|----------------------|
| manager-driver | Manager Driver Tests |
| actor-conn | Actor Connection Tests |
| actor-conn-state | Actor Connection State Tests |
| conn-error-serialization | Connection Error Serialization Tests |
| actor-destroy | Actor Destroy Tests |
| request-access | Request Access in Lifecycle Hooks |
| actor-handle | Actor Handle Tests |
| action-features | Action Features Tests |
| access-control | access control |
| actor-vars | Actor Variables |
| actor-metadata | Actor Metadata Tests |
| actor-telemetry | Actor Telemetry |
| actor-onstatechange | Actor State Change Tests |
| actor-db | Actor Database |
| actor-db-raw | Actor Database Raw Tests |
| actor-db-init-order | Actor Db Init Order |
| actor-workflow | Actor Workflow Tests |
| actor-error-handling | Actor Error Handling Tests |
| actor-queue | Actor Queue Tests |
| actor-kv | Actor KV Tests |
| actor-stateless | Actor Stateless Tests |
| raw-http | raw http |
| raw-http-request-properties | raw http request properties |
| raw-websocket | raw websocket |
| actor-inspector | Actor Inspector Tests |
| gateway-query-url | Gateway Query URL Tests |
| actor-db-pragma-migration | Actor Database Pragma Migration |
| actor-state-zod-coercion | Actor State Zod Coercion |
| actor-conn-status | Connection Status Changes |
| gateway-routing | Gateway Routing |
| lifecycle-hooks | Lifecycle Hooks |
| serverless-handler | Serverless Handler Tests |
| actor-state | Actor State Tests |
| actor-save-state | Actor Save State Tests |
| actor-schedule | Actor Schedule Tests |
| actor-sleep | Actor Sleep Tests |
| actor-sleep-db | Actor Sleep Database Tests |
| actor-lifecycle | Actor Lifecycle Tests |
| actor-conn-hibernation | Actor Connection Hibernation Tests |
| actor-run | Actor Run Tests |
| hibernatable-websocket-protocol | hibernatable websocket protocol |
| actor-db-stress | Actor Database Stress Tests |
| actor-agent-os | Actor agentOS Tests |

**c) Pipe output to file and analyze:**

Always pipe test output to `/tmp/driver-test-current.log` so you can grep it afterward. Then analyze:

```bash
grep -E "Tests|FAIL|PASS|Error|✓|✗|×" /tmp/driver-test-current.log | tail -30
```

**d) If all tests pass for that runtime:** Check off only that runtime's box in the progress file and append to the log:

```
- <timestamp> <file> [<runtime>]: PASS (<N> tests, <duration>)
```

If both runtime boxes are now checked, the file is fully done; advance to the next file.

**e) If tests fail:**

1. Do NOT move to the next runtime or file.
2. Narrow down to the first failing test by adding enough test-name text to the same `-t` pattern.
3. Read the error output to understand the failure.
4. Append to the log:

```
- <timestamp> <file> [<runtime>]: FAIL - <brief description of failure>
```

5. Report the failure to the user with:
   - Which test file group failed and on which runtime
   - Which specific test(s) failed
   - The error message
   - Whether the failure is runtime-specific (e.g. fails on `wasm` but passes on `native`)
   - Suggested next steps

### 5. Narrowing scope on failure

If a file group fails, keep the full matrix selector and append enough test-name text to isolate the failing test:

```bash
cd rivetkit-typescript/packages/rivetkit && \
  pnpm test tests/driver/<FILE>.test.ts \
    -t "runtime \(<runtime>\) / sqlite \(<backend>\) / encoding \(bare\).*<test name>" \
    > /tmp/driver-test-narrow.log 2>&1
```

Do not use `-t` as a flake workaround. It is only for selecting the intended matrix cell and, when needed, a specific failing test.
If the bug only appears on one runtime, that's a strong signal — focus the diff hunt on the corresponding runtime adapter (`napi-runtime.ts` / `wasm-runtime.ts`) and any wasm-feature-gated code in `rivetkit-core` and `rivetkit-typescript/packages/rivetkit-wasm`.

### 6. Completion

When all rows are fully checked (both runtime boxes), append to the log:

```
- <timestamp> ALL TESTS COMPLETE
```

Report summary:
- Total files passing per runtime
- Total files failing per runtime (with names)
- Files where one runtime passes and the other fails (parity gaps)
- Total duration

## Rules

1. **One file at a time.** Never run the full suite. The whole point is methodical, scoped testing.
2. **Both runtimes per file before advancing** (when `runtime=both`). Run native, then wasm, on the same file. Check off each independently as it passes, but do not advance to the next file until both are checked.
3. **Fix before advancing.** Do not skip a failing runtime/file to test the next one (unless the user says to skip).
4. **Always pipe to file.** Never rely on inline terminal output for test results. Always write to `/tmp/driver-test-current.log` and grep afterward.
5. **Track everything.** Every run gets logged in the progress file with its runtime tag.
6. **Always use `-t` for matrix selection.** Include runtime, SQLite backend, and encoding in the selector. Do not scope the driver matrix with env vars.
7. **Never pair `wasm` with `local` SQLite.** The harness throws on this combination. If a wasm run somehow needs local SQLite to repro a bug, that's a bug in the matrix, not a workaround to apply.
8. **Respect timeouts.** Set a 600-second timeout for slow tests (sleep, lifecycle, stress). Use 120 seconds for fast tests. Wasm runs may be slower than native — extend timeouts proportionally if you see consistent timeouts on wasm only.
