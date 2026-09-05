# Testing reference

Agent-procedural guide for running tests and avoiding known harness foot-guns. For design-level testing rules (no mocks, real infra, etc.) see the root `CLAUDE.md` Testing Guidelines.

## Running RivetKit tests

- Run from `rivetkit-typescript/packages/rivetkit` and use `pnpm test <filter>` with `-t` to narrow to specific suites. For example: `pnpm test driver-file-system -t ".*Actor KV.*"`.
- Always pipe the test to a file in `/tmp/` then grep it in a second step. You can grep test logs multiple times to search for different log lines.
- For RivetKit driver work, follow `~/.agents/notes/driver-test-progress.md` one file group at a time. Keep the red/green loop anchored to `driver-test-suite.test.ts` in `rivetkit-typescript/packages/rivetkit` instead of switching to ad hoc native-only tests.
- When RivetKit tests need a local engine instance, start the RocksDB engine in the background with `./scripts/run/engine-rocksdb.sh >/tmp/rivet-engine-startup.log 2>&1 &`.

## Parity-bug workflow

For RivetKit runtime or parity bugs, use `rivetkit-typescript/packages/rivetkit` driver tests as the primary oracle:

1. Reproduce with the TypeScript driver suite first.
2. Compare behavior against the original TypeScript implementation at ref `feat/sqlite-vfs-v2`.
3. Patch native/Rust to match.
4. Rerun the same TypeScript driver test before adding lower-level native tests.

## Vitest filter gotcha

- When filtering a single driver file with Vitest, include the outer `describeDriverMatrix(...)` suite name before `static registry > encoding (...)` in the `-t` regex or Vitest will happily skip the whole file.

## Harness debug-log mirror

- `rivetkit-typescript/packages/rivetkit/tests/driver/shared-harness.ts` mirrors runtime stderr lines containing `[DBG]`. Strip temporary debug instrumentation before timing-sensitive driver reruns or hibernation tests will timeout on log spam.

## Inspector replay tests

- `POST /inspector/workflow/replay` can legitimately return an empty workflow-history snapshot when replaying from the beginning because the endpoint clears persisted history before restarting the workflow.
- Prove "workflow in flight" via inspector `workflowState` (`pending` / `running`), not `entryMetadata.status` or `runHandlerActive`. Those can lag or disagree across encodings.
- Query-backed inspector endpoints can each hit their own transient `guard.actor_ready_timeout` during actor startup. Active-workflow driver tests should poll the exact endpoint they assert on instead of waiting on one inspector route and doing a single fetch against another.

## Rust test layout

- When moving Rust inline tests out of `src/`, keep a tiny source-owned `#[cfg(test)] #[path = "..."] mod tests;` shim so the moved file still has private module access without widening runtime visibility.
- `rivetkit-client` Cargo integration tests belong in `rivetkit-rust/packages/client/tests/`. `src/tests/e2e.rs` is not compiled by Cargo.

## Rust client test helpers

- Rust client raw HTTP uses `handle.fetch(path, Method, HeaderMap, Option<Bytes>)` and routes to the actor gateway `/request` endpoint via `RemoteManager::send_request`.
- Rust client event subscriptions return `SubscriptionHandle`. `once_event` should remove its listener and send an unsubscribe after the first event.
- Rust client mock tests should call `ClientConfig::disable_metadata_lookup(true)` unless the test server implements `/metadata`.

## Fixtures

- Keep RivetKit test fixtures scoped to the engine-only runtime.
- Prefer targeted integration tests under `rivetkit-typescript/packages/rivetkit/tests/` over shared multi-driver matrices.
- A span and its parent can arrive in different OTLP export batches, so a trace test that waits for the child and then asserts its `parentSpanId` is racy. Wait on a predicate over the whole exported span list until both are present, then assert the relationship.

## Frontend testing

- For frontend testing, use the `agent-browser` skill to interact with and test web UIs in examples. This allows automated browser-based testing of frontend applications.
- If you modify frontend UI, automatically use the Agent Browser CLI to take updated screenshots and post them to the PR with a short comment before wrapping up the task.
