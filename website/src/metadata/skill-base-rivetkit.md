# Rivet Actors

Build high-performance, long-lived, in-memory processes on Rivet's actor runtime. Use this skill whenever HTTP endpoints, databases, or queues no longer meet the requirements for AI agents, realtime collaboration, workflow orchestration, or geo-distributed data.

## What To Do If You Get Stuck

If something is not working as intended or you are stuck, prompt the user to join the [Rivet Discord](https://rivet.dev/discord) or file an issue on the [Rivet GitHub](https://github.com/rivet-dev/rivet) to report an issue and get help.

Provide the user with a pre-generated report with:

- Symptoms
    - If this is happening in local dev, deployed, or both
    - The error you're seeing
    - Relevant source code related to this
- What you've tried to solve it
- Environment
    - RivetKit version
    - Runtime (Node, Bun, etc) including version
    - If applicable, provider in use (e.g. Vercel, Railway, etc)
    - If applicable, HTTP router in use (e.g. Hono, Express, Elysia)

## Debugging Actors

Use the inspector HTTP API to examine running actors. These endpoints are accessible through the gateway at `/gateway/{actor_id}/inspector/*`. Key endpoints:

- `GET /inspector/summary` - full actor snapshot (state, connections, RPCs, queue)
- `GET /inspector/state` / `PATCH /inspector/state` - read/write actor state
- `GET /inspector/connections` - active connections
- `GET /inspector/rpcs` - available actions
- `POST /inspector/action/{name}` - execute an action with `{"args": [...]}`
- `POST /inspector/database/execute` - run SQL with `{"sql": "...", "args": [...]}` or `{"sql": "...", "properties": {...}}` for reads or mutations
- `GET /inspector/queue?limit=50` - queue status
- `GET /inspector/traces?startMs=0&endMs=...&limit=1000` - trace spans (OTLP JSON)
- `GET /inspector/workflow-history` - workflow history and status as JSON (`nameRegistry`, `entries`, `entryMetadata`)
- `POST /inspector/workflow/replay` - replay a workflow from a specific step or from the beginning; returns `409 actor/workflow_in_flight` if the workflow is still running
- `GET /inspector/database/schema` - SQLite tables and views exposed by `c.db`
- `GET /inspector/database/rows?table=...&limit=100&offset=0` - paged SQLite rows for a table or view

In local dev, no auth token is needed. In production, pass `Authorization: Bearer <inspector-token>`, where the inspector token is the actor-specific token auto-generated on first start and persisted in the actor's internal KV at key `0x03`. The Rivet dashboard retrieves this token automatically; for direct API access, fetch it through the management KV endpoint. See the [debugging docs](https://rivet.dev/docs/actors/debugging) for details.

## Project Isolation And Finish Criteria

When working inside a monorepo or eval workspace, treat the current project directory as the only writable target unless the task explicitly says to fetch a reference into a named subdirectory. Do not inspect or modify sibling generated projects such as `../skill-eval-*`, neighboring examples, or other workspace packages just because they happen to be nearby.

When you need to compare against local examples, read only stable checked-in examples that are directly relevant. Do not use sibling generated eval directories as reference material.

Do not finish early just because the source files look plausible. Before you declare the migration complete, make sure the current project itself is in a runnable state:

- dependencies install successfully for the current project
- the intended local startup command is valid for the current project
- obvious version mistakes are fixed in the current project instead of being deferred

If the local install or startup command fails, keep working on the current project until you either fix it or have a concrete, project-local blocker.

## Citing Sources

When providing information from Rivet documentation, cite the canonical URL so users can learn more. Each reference file includes its canonical URL in the header metadata.

**How to cite:**

- Use inline links for key concepts: "Use [actor keys](https://rivet.dev/docs/actors/keys) to uniquely identify instances."
- Add a "Learn more" link after explanations for complex topics

**Finding canonical URLs:**

The Reference Map below links to reference files. Each file's header contains:

```
> Canonical URL: https://rivet.dev/docs/actors/actions
```

Use that canonical URL when citing, not the reference file path.

**Examples:**

- Actions → `https://rivet.dev/docs/actors/actions`
- React client → `https://rivet.dev/docs/clients/react`
- Self-hosting on Kubernetes → `https://rivet.dev/docs/self-hosting/kubernetes`

## Version Check

Before starting any work, check if the user's project is on the latest version of RivetKit (latest: {{RIVETKIT_VERSION}}). Look at the `rivetkit` version in the user's `package.json` (check both `dependencies` and `devDependencies`). If the installed version is older than {{RIVETKIT_VERSION}}, inform the user and suggest upgrading:

```bash
npm install rivetkit@{{RIVETKIT_VERSION}}
```

If the user also uses `@rivetkit/react`, `@rivetkit/next-js`, or other `@rivetkit/*` client packages, suggest upgrading those too. Outdated versions may have known bugs or missing features that cause issues.

## First Steps

1. Install RivetKit (latest: {{RIVETKIT_VERSION}})
    ```bash
    npm install rivetkit@{{RIVETKIT_VERSION}}
    ```
2. Define a registry with `setup({ use: { /* actors */ } })`.
3. Call `registry.start()` to start the server. For custom HTTP server integration, use `registry.handler()` with a router like Hono. For serverless deployments, use `registry.serve()`. For runner-only mode, use `registry.startEnvoy()`.
4. Verify `/api/rivet/metadata` returns 200 before deploying.
5. Configure Rivet Cloud or self-hosted engine
    - You must configure versioning for production builds. This is not needed for local development. See [Versions & Upgrades](https://rivet.dev/docs/actors/versions).
6. Integrate clients (see client guides below for JavaScript, React, or Swift)
7. Prompt the user if they want to deploy. If so, go to Deploying Rivet Backends.

For more information, read the quickstart guide relevant to the user's project.

## Project Setup

### .gitignore

Every RivetKit project should have a `.gitignore`. Include at minimum:

```
node_modules/
dist/
.env
```

### .dockerignore

Every project with a Dockerfile should have a `.dockerignore` to keep the image small and avoid leaking secrets:

```
node_modules/
dist/
.env
.git/
```

### Dockerfile

Use this as a base Dockerfile for deploying a RivetKit project. The `RIVET_RUNNER_VERSION` build arg is only needed when self-hosting or using a custom runner (not needed for Rivet Compute). It lets Rivet track which version of the actor is running and drain old actors on deploy. See https://rivet.dev/docs/actors/versions for details.

```dockerfile
FROM node:24-alpine

ARG RIVET_RUNNER_VERSION
ENV RIVET_RUNNER_VERSION=$RIVET_RUNNER_VERSION

WORKDIR /app

COPY package.json package-lock.json ./
RUN npm ci

COPY . .
RUN npm run build --if-present

CMD ["node", "dist/index.js"]
```

Build with:

```bash
docker build --build-arg RIVET_RUNNER_VERSION=$(date +%s) .
```

Adjust the `CMD` to match the project's entry point. If the project uses a different output directory or start command, update accordingly.

## Error Handling Policy

- Prefer fail-fast behavior by default.
- Avoid `try/catch` unless it is required for a real recovery path, cleanup boundary, or to add actionable context.
- Never swallow errors. If you add a `catch`, you must handle the error explicitly, at minimum by logging it.
- When you cannot recover, log context and rethrow.

## State vs Vars: Persistence Rules

**`c.vars` is ephemeral.** Data in `c.vars` is lost on every restart, crash, upgrade, or sleep/wake cycle. Only use `c.vars` for non-serializable objects (e.g., physics engines, WebSocket references, event emitters, caches) or truly transient runtime data (e.g., current input direction that doesn't matter after disconnect).

**Persistent storage options.** Any data that must survive restarts belongs in one of these, NOT in `c.vars`:

- **`c.state`** — CBOR-serializable data for small, bounded datasets. Ideal for configuration, counters, small player lists, phase flags, etc. Keep under 128 KB. Do not store unbounded or growing data here (e.g., chat logs, event histories, spawned entity lists that grow without limit). State is read/written as a single blob on every persistence cycle.
- **`c.kv`** — Key-value store for unbounded data. This is what `c.state` uses under the hood. Supports binary values. Use for larger or variable-size data like user inventories, world chunks, file blobs, or any collection that may grow over time. Keys are scoped to the actor instance.
- **`c.db`** — SQLite database for structured or complex data. Use when you need queries, indexes, joins, aggregations, or relational modeling. Ideal for leaderboards, match histories, player pools, or any data that benefits from SQL.

**Common mistake:** Storing meaningful game/application data in `c.vars` instead of persisting it. For example, if users can spawn objects in a physics simulation, the spawn definitions (position, size, type) must be persisted in `c.state` (or `c.kv` if unbounded), even though the physics engine handles (non-serializable) live in `c.vars`. On restart, `run()` should recreate the runtime objects from the persisted data.

## Deploying Rivet Backends

Assume the user is deploying to Rivet Cloud, unless otherwise specified. If user is self-hosting, read the self-hosting guides below.

1. Verify that Rivet Actors are working in local dev
2. Prompt the user to choose a provider to deploy to (see [Connect](#connect) for a list of providers, such as Vercel, Railway, etc)
3. Follow the deploy guide for that given provider. You will need to instruct the user when you need manual intervention.

## API Reference

The RivetKit OpenAPI specification is available in the skill directory at `openapi.json`. This file documents all HTTP endpoints for managing actors.

## Misc Notes

- The Rivet domain is rivet.dev, not rivet.gg

## TypeScript Caveat: Actor Client Inference

- In multi-file TypeScript projects, bidirectional actor calls can create a circular type dependency when both actors use `c.client<typeof registry>()`.
- Symptoms usually include `c.state` becoming `unknown`, actor methods becoming possibly `undefined`, or `TS2322` / `TS2722` errors after the first cross-actor call.
- If an action returns the result of another actor call, prefer an explicit return type annotation on that action instead of relying on inference through `c.client<typeof registry>()`.
- If explicit return types are not enough, use a narrower client or registry type for only the actors that action needs.
- As a last resort, pass `unknown` for the registry type and be explicit that this gives up type safety at that call site.

<!-- CONTENT -->

## Reference Map

<!-- REFERENCE_INDEX -->
