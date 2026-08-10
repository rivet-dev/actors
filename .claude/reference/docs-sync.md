# Docs sync table

When making engine or RivetKit changes, keep documentation in sync. Check this table before finishing a change.

## Sitemap

- When adding new docs pages, update `website/src/sitemap/mod.ts` so the page appears in the sidebar.

## Code blocks in docs

- All TypeScript code blocks in docs are typechecked during the website build. They must be valid, compilable TypeScript.
- Use `<CodeGroup workspace>` only when showing multiple related files together (e.g., `actors.ts` + `client.ts`). For a single file, use a standalone fenced code block.
- Code blocks are extracted and typechecked via `website/src/integrations/typecheck-code-blocks.ts`. Add `@nocheck` to the code fence to skip typechecking for a block.

## Sync rules

| Change | Update |
|---|---|
| **Limits** (max message sizes, timeouts, KV/queue/SQLite/WebSocket/HTTP limits) | `website/src/content/docs/actors/limits.mdx` |
| **Engine config options** (`engine/packages/config/`) | `website/src/content/docs/self-hosting/configuration.mdx` |
| **RivetKit config** (`rivetkit-typescript/packages/rivetkit/src/registry/config/index.ts`, `rivetkit-typescript/packages/rivetkit/src/actor/config.ts`) | `website/src/content/docs/actors/limits.mdx` if they affect limits/timeouts |
| **Actor errors** (`ActorError` in `engine/packages/types/src/actor/error.rs`, `RunnerPoolError`) | `website/src/content/docs/actors/troubleshooting.mdx` — each error should document the dashboard message (from `frontend/src/components/actors/actor-status-label.tsx`) and the API JSON shape |
| **Actor statuses** (`frontend/src/components/actors/queries/index.ts` derivation) | `website/src/content/docs/actors/statuses.mdx` + tests in `frontend/src/components/actors/queries/index.test.ts` |
| **Kubernetes manifests** (`self-host/control-plane/kubernetes/`) | `website/src/content/docs/self-hosting/kubernetes.mdx`, `self-host/control-plane/kubernetes/README.md`, and `scripts/run/k8s/engine.sh` if file names or deployment steps change |
| **Landing page** (`website/src/pages/index.astro` + section components in `website/src/components/marketing/sections/`) | `README.md` — reflect the same headlines, features, benchmarks, and talking points where applicable |
| **Sandbox providers** (`rivetkit-typescript/packages/rivetkit/src/sandbox/providers/`) | `website/src/content/docs/actors/sandbox.mdx` — provider docs, option tables, custom provider guidance |
| **Inspector endpoints** | `docs/content/docs/debugging.mdx` — AI skills are generated from the docs, so there is no separate skill file to update |
| **rivetkit-core state management** (`request_save`, `save_state`, `persist_state`, `set_state_initial` semantics) | `docs-internal/engine/rivetkit-core-state-management.md` |
