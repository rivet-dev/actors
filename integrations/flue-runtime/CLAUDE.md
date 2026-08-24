# @rivet-dev/flue

The Flue deployment target and Rivet runtime adapter. It plugs Rivet Actors
into Flue's runtime, exposing agents and workflows as deployable Rivet actors.

## Reference adapter: Cloudflare Durable Objects

This adapter is a **1:1 port of the Cloudflare Durable Object adapter** in the Flue runtime
(`@flue/runtime`, `packages/runtime/src/cloudflare/` in the flue monorepo). Both adapters implement
the same durable agent/workflow model (per-instance storage, the submission recovery model, the
run-record + central run-index topology) against a single-instance, per-actor SQLite backend.

**When in doubt about correct behavior, mimic the Cloudflare Durable Objects adapter.** It is the
reference implementation and the source of truth for the intended contract. Before changing or adding
behavior here, find the corresponding code in `src/cloudflare/` and match its semantics unless there is
a Rivet-specific reason to diverge — and when you must diverge, say so explicitly in a comment that
names the Cloudflare behavior you are departing from and why (e.g. Rivet has no durable resumable
fibers, so it omits the attempt-marker reconcile that Cloudflare needs).

Canonical examples of matching Cloudflare:
- **Run index is best-effort.** `createRun`/`endRun` write the authoritative per-actor record first,
  then mirror a pointer into the central registry actor. An index-write fault must be swallowed + logged,
  never propagated — it must not fail run admission or finalization. This matches Cloudflare's
  `safeIndexWrite` (`src/cloudflare/run-store.ts`). See `src/run-store-composite.ts`.
- **Recovery model.** Interrupted `running` submissions are reclaimed by reconcile + `claimSubmission`
  CAS, exactly as in `src/cloudflare/agent-coordinator.ts`.

Deliberate, documented divergences from Cloudflare:
- **No attempt markers in reconcile.** Cloudflare uses persisted attempt markers to avoid terminalizing
  a fiber that its SDK is auto-resuming after eviction. Rivet has no fiber resume (turns run via plain
  `keepAwake`), so every post-restart `running` submission is genuinely interrupted and should be
  reconciled. Wiring markers in would wrongly suppress legitimate recovery. (See `agent-coordinator.ts`.)
- **Transactions use the Rivet database transaction API.** Rivet wraps actor work in its own
  lifecycle, so adapter stores must not issue raw `BEGIN` statements. The `SAVEPOINT` fallback in
  `rivet-db.ts` exists only for the Node SQLite contract harness.
- **Response forwarding** crosses the actor RPC boundary (Cloudflare uses native DO RPC), so request/
  response (de)serialization lives here with no Cloudflare equivalent.

## Conventions

- TypeScript for all source/tests/fixtures/config; do not add `.mjs` files.
- Tests live in `test/`; design them from observable contracts, name them `it('X when Y')`.
- The store contract tests (shared with the runtime) must stay green; run them against the real
  `createRivetAsyncSqlDb` adapter, not a stronger double.
