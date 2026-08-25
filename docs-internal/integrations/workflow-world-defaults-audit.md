# Workflow World defaults audit

`@rivet-dev/workflow-world` should implement Workflow World semantics while
delegating Rivet connection and runtime behavior to RivetKit. It must not copy
RivetKit's environment resolution or invent integration-specific defaults.

This audit is scoped only to `@rivet-dev/workflow-world`, including its package
tests, examples, and documentation. References to Eve and RivetKit below define
the external contracts the World should use; they are not audits of those
projects.

## Startup readiness plan

Status: direction accepted with revisions.

The lazy Engine proposal in `/home/nathan/tmp/eve-rivet-lazy-engine-spec.md`
requires an explicit readiness primitive and a single combined application
registry. `RivetClientWorld.start()` intentionally performs no actor request,
so readiness is required immediately before the first real World operation.

Eve can receive a cold request before eager application startup completes, so
the World operation itself must enforce the RivetKit readiness barrier.

Add `registry.startAndWait()` as the public RivetKit method with these semantics:

- It starts the registry if needed and is safe to call repeatedly.
- It resolves only after the envoy is registered and can serve actor requests;
  local HTTP health alone is insufficient.
- It shares one in-flight readiness promise across callers, uses a bounded
  timeout, and surfaces startup failures. Retry is allowed only when the
  underlying lifecycle can safely retry without duplicating resources.
- It uses a readiness signal from the RivetKit/core lifecycle rather than
  integration-owned actor probes or polling.
- It follows the registry's normal local-versus-remote Engine configuration and
  never independently spawns an Engine.

The lazy World integration must receive the application's combined registry,
containing both `workflowWorldActors` and optional application actors such as
agentOS `vm`, and call `startAndWait()` before its first outgoing actor request.
The Workflow World package remains independent of agentOS and must not create a
second private registry. Keep `registry.start()` as the fire-and-forget
compatibility API; a separate public `waitForReady()` is unnecessary without a
concrete wait-without-start caller.

The World must not implement its own `no_envoys` retry or readiness protocol. It
should consume the RivetKit readiness contract described above.

Acceptance coverage should include cold single-process startup, repeated and
concurrent `startAndWait()` calls, startup failure behavior, remote Engine mode,
and shutdown without an orphan Engine. The Eve end-to-end test should confirm
the first World operation succeeds from a cold `eve dev` process.

## Connection configuration

Status: fix required.

The World currently resolves `RIVET_ENDPOINT`, `RIVET_NAMESPACE`,
`RIVET_POOL_NAME`, and `RIVET_TOKEN` itself. This has already drifted from
RivetKit: RivetKit reads `RIVET_POOL`, and its client owns endpoint URL parsing
and metadata-based client configuration.

Construct the normal World client through RivetKit without duplicating those
settings:

```ts
createClient<typeof registry>();
```

Keep prebuilt client injection for tests and advanced composition. If a
programmatic configuration escape hatch is needed, accept RivetKit's client
configuration as one pass-through value rather than defining another endpoint,
namespace, pool, and token API.

Remove the integration's local endpoint resolver, required-value checks for
Rivet connection settings, `RIVET_POOL_NAME`, and hardcoded `default` values.
Also remove `disableMetadataLookup: true` unless a failing test demonstrates a
specific incompatibility; metadata lookup is normal RivetKit client behavior.

Update the package README, website guide, example environment file, and tests to
use RivetKit's canonical variables. In particular, document `RIVET_POOL`, not
`RIVET_POOL_NAME`.

## Local authentication

Status: fix required.

Remove the World's `"dev"` token fallback. RivetKit's default token is
`undefined`, and the unauthenticated local Engine accepts requests without an
`x-rivet-token` header. Authentication should come from endpoint URL credentials
or `RIVET_TOKEN` through RivetKit's client configuration.

The integration-test runtime may explicitly configure credentials when testing
authenticated behavior, but a fixture token must not be represented as the
local Engine or public package default.

## Deployment pinning

Status: known limitation; add an inline code comment.

The World currently returns a constant deployment ID and dispatches wakes to the
current `WORKFLOW_RUNTIME_URL`. It therefore cannot guarantee the Workflow SDK's
immutable-deployment behavior: an unfinished run may resume against newer code.
`RIVET_ENVOY_VERSION` supports runner upgrades, but it does not make a historical
version addressable or route an HTTP request to an exact version.

Add a concise comment beside `getDeploymentId()` explaining this limitation and
why `resolveLatestDeploymentId()` is intentionally not implemented. Full support
requires either an immutable runtime deployment address retained per run or a
versioned workflow bundle loader. Do not imply that `RIVET_ENVOY_VERSION` alone
provides the Workflow SDK deployment pinning.

## Additional audit findings

The complete World-package audit of places that override or duplicate default
Rivet behavior is:

| Current behavior | Required disposition |
| --- | --- |
| World-specific endpoint/namespace/pool/token config fields | Remove or replace with a single RivetKit client/config pass-through |
| Custom local endpoint construction from Engine host and port | Remove; RivetKit owns local Engine endpoint resolution |
| Required checks for endpoint, namespace, and pool | Remove; RivetKit supplies and validates its defaults |
| `RIVET_POOL_NAME` | Remove; RivetKit uses `RIVET_POOL` |
| Hardcoded namespace and pool value `"default"` | Remove from the World; RivetKit owns these defaults |
| Hardcoded token `"dev"` | Remove; an unauthenticated local Engine uses no token |
| `disableMetadataLookup: true` | Remove unless a concrete incompatibility proves it necessary |
| `startRegistry()` HTTP-health polling | Replace with `registry.startAndWait()` using RivetKit's actual envoy-readiness signal |
| Rivet-specific workflow runtime URL aliases | Remove; retain the canonical World / Workflow SDK setting |
| Native runtime, local SQLite, and external Engine overrides in test runtime | Keep only as explicit test-harness controls; never document as public defaults |

### Registry readiness

Status: replace with `registry.startAndWait()`.

`startRegistry()` polls `registry.routes.health()`. That proves the local HTTP
runtime responds, not that its envoy has registered with the Engine. It should
be removed in favor of the idempotent RivetKit readiness API; do not grow another
integration-owned polling protocol.

### Workflow runtime URL

Status: simplify and document intentional behavior.

The World owns the callback URL because it must deliver the Workflow SDK queue
messages to the application. Keep one explicit `runtimeUrl` option and the
canonical `WORKFLOW_RUNTIME_URL`. Remove Rivet-specific aliases such as
`RIVET_WORKFLOW_BASE_URL`. Retain an upstream Workflow SDK alias or local port
fallback only if its documented behavior and tests require it.

### Test runtime overrides

Status: valid test scope, cleanup required.

The integration runtime's native runtime, local SQLite, and externally managed
Engine settings are deliberate test-harness controls. They are not public
defaults. Update its pool variable and remove its `"dev"` token fallback so the
tests exercise the same RivetKit resolution rules as applications unless a test
explicitly overrides them.

### World-owned settings

Status: keep.

`WORKFLOW_QUEUE_NAMESPACE`, `WORKFLOW_RUNTIME_URL`, dispatcher retry policy,
stream retention, and `RIVET_WORKFLOW_SECRET` represent Workflow World behavior
rather than RivetKit connection defaults. These may remain in the integration,
with test-only fault-injection variables kept clearly internal.
