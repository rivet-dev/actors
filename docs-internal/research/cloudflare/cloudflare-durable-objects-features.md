# Cloudflare Durable Objects: Comprehensive Feature Reference

> **As of:** 2026-03-06
>
> **Cloudflare basis:** Official Durable Objects docs accessed 2026-03-06. Relevant Cloudflare version marker: RPC guidance depends on Workers compatibility date `>= 2024-04-03`.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Migration framing:** Durable Objects are the closest Cloudflare primitive to Rivet Actors, but the biggest semantic gap is concurrency. Cloudflare serializes DO event handling by default; Rivet actions run in parallel by default and must use queues/workflows when you need serialized mutation paths.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Defining a Durable Object Class | Extend `DurableObject` base class to define stateful objects | native | high | [Actors Index](https://rivet.dev/docs/actors), [Lifecycle](https://rivet.dev/docs/actors/lifecycle) | [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts) | Low | `actor({...})` is the direct analog. |
| Durable Object Namespace (`DurableObjectNamespace`) | ID generation, stubs, jurisdictions | partial | high | [Actor Keys](https://rivet.dev/docs/actors/keys), [Metadata](https://rivet.dev/docs/actors/metadata) | [actor-handle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-handle.ts) | Medium | Deterministic addressing maps well via keys. Jurisdiction subnamespaces do not. |
| Durable Object ID (`DurableObjectId`) | Unique identifiers via `newUniqueId`, `idFromName`, `idFromString` | partial | high | [Metadata](https://rivet.dev/docs/actors/metadata), [Actor Keys](https://rivet.dev/docs/actors/keys) | [actor-metadata.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-metadata.ts) | Medium | Rivet exposes `actorId`, but ID-generation APIs are not a 1:1 match. Prefer keys for stable addressing. |
| Durable Object Stub (`DurableObjectStub`) | Proxy for communicating with a Durable Object instance | native | high | [JavaScript Client](https://rivet.dev/docs/clients/javascript) | [actor-handle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-handle.ts) | Low | Actor handles from `createClient()` are the equivalent. |
| RPC (Remote Procedure Calls) | Public methods auto-exposed as remote endpoints | native | high | [Actions](https://rivet.dev/docs/actors/actions) | [examples/cloudflare-workers/src/actors.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/actors.ts) | Low | DO RPC methods map to actor actions. |
| Fetch Handler (HTTP Request/Response) | HTTP `Request`/`Response` interface | native | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Low | `onRequest` is a direct match for low-level HTTP handling. |
| Durable Object State (`DurableObjectState` / `ctx`) | Access to storage, ID, `blockConcurrencyWhile`, and `acceptWebSocket` | partial | high | [Lifecycle](https://rivet.dev/docs/actors/lifecycle), [Metadata](https://rivet.dev/docs/actors/metadata), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts) | High | Most fields map, but `blockConcurrencyWhile` has no direct equivalent. Use lifecycle/setup plus queues/workflows for serialization. |
| SQLite Storage API | Embedded per-object relational database | native | high | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Low | Strong direct fit. |
| Synchronous KV API (SQLite-backed only) | Sync key-value on SQLite backend | partial | medium | [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | [actor-kv.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-kv.ts) | Medium | Rivet KV is async; there is no sync KV API. |
| Asynchronous KV API | Async key-value on both backends | native | high | [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | [actor-kv.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-kv.ts) | Low | Direct fit. |
| Transactional Storage | Atomic transactions (sync and async) | partial | high | [SQLite](https://rivet.dev/docs/actors/sqlite), [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | High | SQLite transactions are native. KV/state transactions are not a 1:1 DO storage transaction surface. |
| Point In Time Recovery (PITR) | Restore to any point in last 30 days | unsupported | high | [SQLite](https://rivet.dev/docs/actors/sqlite) | Gap | High | No PITR equivalent is documented for actor-local storage. |
| Alarms API | Schedule future execution, at-least-once | native | high | [Actor Scheduling](https://rivet.dev/docs/actors/schedule) | [actor-schedule.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-schedule.ts) | Medium | Good fit; validate exact delivery semantics when alarm idempotency matters. |
| WebSocket Support (Standard API) | Bidirectional real-time connections | native | high | [Connections](https://rivet.dev/docs/actors/connections), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) | Low | Both high-level and raw paths exist. |
| WebSocket Hibernation API | Connections survive object sleep | partial | high | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler), [Limits](https://rivet.dev/docs/actors/limits) | [actor-conn-hibernation.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-hibernation.ts) | High | Rivet supports hibernation, but low-level raw WebSocket hibernation is documented as experimental. |
| In-Memory State | Fast instance-local caching | native | high | [State](https://rivet.dev/docs/actors/state), [Ephemeral Variables](https://rivet.dev/docs/actors/ephemeral-variables) | [actor-vars.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-vars.ts) | Low | Use `state` for persisted data and `vars` for ephemeral data. |
| Durable Object Lifecycle | Creation, caching, eviction behavior | native | high | [Lifecycle](https://rivet.dev/docs/actors/lifecycle) | [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts), [actor-sleep.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-sleep.ts) | Medium | Strong conceptual fit. |
| Input/Output Gates and Concurrency | Automatic concurrency safety | partial | high | [Actions](https://rivet.dev/docs/actors/actions), [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Rivet actions run in parallel by default. Port write-heavy DO code to queues/workflows if it depended on DO serialization. |
| Configuration (`wrangler.toml` / `wrangler.jsonc`) | Binding, migration, and class configuration | pattern | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers/wrangler.json](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/wrangler.json) | Low | When running on Cloudflare, Wrangler config still exists, but it configures the Rivet driver rather than DO classes directly. |
| Migrations (Create, Rename, Delete, Transfer) | Create/rename/delete/transfer classes | pattern | medium | [Versions](https://rivet.dev/docs/actors/versions), [Destroy](https://rivet.dev/docs/actors/destroy) | Docs-only | Medium | Code/version rollout exists, but DO-class migration primitives do not have a direct analogue. |
| Data Location (Jurisdictions and Location Hints) | Restrict data to EU/FedRAMP, influence initial placement | partial | medium | [Metadata](https://rivet.dev/docs/actors/metadata), [Multi-Region](https://rivet.dev/docs/self-hosting/multi-region) | Docs-only | High | Region awareness exists, jurisdiction-restricted namespaces do not. |
| Environments | Staging/production isolation | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/cloudflare-workers) | Low | Use separate namespaces/deployments. |
| Error Handling | Custom error responses and exception patterns | native | high | [Errors](https://rivet.dev/docs/actors/errors), [Actions](https://rivet.dev/docs/actors/actions) | [actor-error-handling.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-error-handling.ts) | Low | `UserError` and custom error handling cover the main cases. |
| TTL (Time To Live) Pattern | Auto-delete via alarms for ephemeral data | native | high | [Actor Scheduling](https://rivet.dev/docs/actors/schedule), [Destroy](https://rivet.dev/docs/actors/destroy) | [actor-destroy.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-destroy.ts) | Low | `c.schedule` plus `c.destroy()` fits well. |
| Counter Example (Read-Modify-Write) | Atomic read-modify-write pattern | pattern | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Do not port write logic directly into parallel actions if DO serialization mattered. |
| ReadableStream Support | Stream large responses from Durable Objects | unsupported | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [request-handler.mdx](https://github.com/rivet-dev/rivet/blob/ba46891b1/website/src/content/docs/actors/request-handler.mdx) | High | Streaming responses and SSE are explicitly unsupported today. |
| Container API | Start/stop/interact with containers | unsupported | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | High | No container runtime equivalent in Rivet Actors. |
| WebGPU API | GPU compute (local dev only) | unsupported | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | High | No WebGPU equivalent in Rivet Actor docs/runtime. |
| Testing | Isolated test support via Vitest | native | high | [Testing](https://rivet.dev/docs/actors/testing) | [rivetkit test suite](https://github.com/rivet-dev/rivet/tree/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests) | Low | Strong test coverage exists. |
| Observability (Metrics and Analytics) | Dashboard + GraphQL API | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Medium | Inspector/logging are solid, but not the same managed metrics surface. |
| Data Security | AES-256 at rest, TLS in transit | partial | medium | [Authentication](https://rivet.dev/docs/actors/authentication) | Docs-only | Medium | App-facing auth is documented; storage/compliance details differ from Cloudflare's platform claims. |
| Pricing | Per-request, duration, and storage billing | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Docs-only | Low | Commercial comparison, not runtime parity. |
| Limits | Object size, request size, and rate limits | partial | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | Medium | Limits differ materially and must be re-audited. |
| Gradual Deployments | Incremental code rollout | partial | medium | [Versions](https://rivet.dev/docs/actors/versions) | Docs-only | Medium | Versioning exists, but not the same Cloudflare progressive rollout surface. |
| Troubleshooting | Common errors and debugging guidance | partial | high | [Troubleshooting](https://rivet.dev/docs/actors/troubleshooting), [Debugging](https://rivet.dev/docs/actors/debugging) | Docs-only | Low | Rivet has troubleshooting and debugging docs. |
| Known Issues | Platform-level known issues and workarounds | partial | low | [Troubleshooting](https://rivet.dev/docs/actors/troubleshooting) | Gap | Low | No centralized "known issues" page equivalent was found. |
| Sharding and Design Patterns | Fan-out and coordination patterns | native | high | [Design Patterns](https://rivet.dev/docs/actors/design-patterns), [Scaling](https://rivet.dev/docs/actors/scaling) | Docs-only | Low | Strong fit. |
| `RpcTarget` Class for Durable Object Metadata | Return metadata via `RpcTarget` stubs | native | high | [Metadata](https://rivet.dev/docs/actors/metadata) | [actor-metadata.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-metadata.ts) | Low | Metadata API maps cleanly. |
| REST API | Programmatic management | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging), [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts), [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Medium | Management and gateway APIs exist, but they are not the same DO management API. |
| Legacy KV Storage Backend | Original key-value backend (deprecated) | out-of-scope | high | [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | Docs-only | Low | No equivalent legacy mode matters for migration planning. |
| Rust API (`workers-rs`) | Rust bindings for Durable Objects | unsupported | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | Medium | This migration guide is for Rivet Actors/RivetKit, not a Rust DO-compatible runtime. |
| Storage Options Summary | Comparison of SQLite vs KV backends | native | high | [Persistence](https://rivet.dev/docs/actors/persistence), [SQLite](https://rivet.dev/docs/actors/sqlite), [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts), [actor-kv.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-kv.ts) | Low | Rivet explicitly offers state, KV, and SQLite as separate storage layers. |

## High-Risk Behavioral Deltas

- **Concurrency is the main migration hazard.** Durable Objects serialize requests/events for one object. Rivet actions run in parallel unless you funnel mutations through queues or workflows.
- **Storage semantics split across layers.** DO `ctx.storage` combines addressing and durability. Rivet splits small persisted state, KV, and SQLite into separate tools. Choose intentionally.
- **Raw WebSocket hibernation needs targeted validation.** Rivet supports hibernation, but if your DO used the hibernation API heavily with raw sockets, validate that path because Rivet marks `canHibernateWebSocket` as experimental.
- **Readable streaming is a real gap.** Large streaming HTTP responses and SSE are explicitly unsupported today.
- **Jurisdiction and platform ops are not 1:1.** Data-placement controls, class migrations, and DO operational tooling need separate redesign decisions.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Actor/DO lifecycle ports cleanly | Create, wake, sleep, destroy hooks behave predictably | Pass: [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts), [actor-sleep.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-sleep.ts) |
| SQLite and KV replace DO storage correctly | CRUD and persistence work for the chosen storage layer | Pass: [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts), [actor-kv.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-kv.ts) |
| Serialized mutation path is explicit | Any read-modify-write code that relied on DO serialization is moved to queues/workflows | Gap: review [Queues & Run Loops](https://rivet.dev/docs/actors/queues) and [Workflows](https://rivet.dev/docs/actors/workflows) for each migrated mutation path |
| WebSocket hibernation preserves logical sessions | Connections survive sleep with expected hook behavior | Pass: [actor-conn-hibernation.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-hibernation.ts) |
| HTTP handler parity exists | `fetch()`-style request handling ports to `onRequest` | Pass: [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) |
| Streaming response dependence is removed or replaced | No migrated path requires SSE/streaming HTTP | Fail: [request-handler.mdx](https://github.com/rivet-dev/rivet/blob/ba46891b1/website/src/content/docs/actors/request-handler.mdx) documents streaming as unsupported |
| Jurisdiction and rollout assumptions are documented | Migration plan covers region placement and upgrade strategy explicitly | Gap: review [Multi-Region](https://rivet.dev/docs/self-hosting/multi-region) and [Versions](https://rivet.dev/docs/actors/versions) during ops planning |

---

This document catalogs every feature of Cloudflare Durable Objects with descriptions, documentation links, and code snippets taken directly from the official documentation. Its purpose is to enable mapping each Cloudflare feature to its Rivet Actor equivalent.

---

## Defining a Durable Object Class

**Docs:** https://developers.cloudflare.com/durable-objects/get-started/
**Docs:** https://developers.cloudflare.com/durable-objects/api/base/

A Durable Object is a JavaScript/TypeScript/Python class that extends the `DurableObject` base class. Each class defines a namespace; each instance within that namespace has a globally unique ID and its own private storage. Public methods are automatically exposed as RPC endpoints.

The `DurableObject` base class is an abstract class which all Durable Objects inherit from. It provides `ctx` (DurableObjectState) and `env` (bindings) as class properties.

```typescript
import { DurableObject } from "cloudflare:workers";

export class MyDurableObject extends DurableObject<Env> {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  async sayHello(): Promise<string> {
    let result = this.ctx.storage.sql
      .exec("SELECT 'Hello, World!' as greeting")
      .one();
    return result.greeting;
  }
}
```

---

## Durable Object Namespace (DurableObjectNamespace)

**Docs:** https://developers.cloudflare.com/durable-objects/api/namespace/

A namespace represents a set of Durable Objects backed by the same class. There is exactly one namespace per class, containing unlimited instances. The namespace is accessed through the Worker's `env` parameter via bindings configured in wrangler.

**Key methods:**

- `idFromName(name: string)` - Creates a unique `DurableObjectId` from a deterministic string identifier. The same name always produces the same ID.
- `newUniqueId(options?)` - Generates a random, unique `DurableObjectId`. Offers lower first-request latency by skipping world-wide consistency checks. Accepts optional `jurisdiction` parameter.
- `idFromString(hexId: string)` - Reconstructs a `DurableObjectId` from a previously stringified hex ID.
- `get(id: DurableObjectId, options?)` - Obtains a `DurableObjectStub` from a `DurableObjectId`. Accepts optional `locationHint`.
- `getByName(name: string)` - Obtains a `DurableObjectStub` directly from a name string (convenience method combining `idFromName` + `get`).
- `jurisdiction(jurisdiction: string)` - Creates a jurisdiction-restricted subnamespace (e.g., `"eu"`, `"fedramp"`).

Creating an ID does not create the Durable Object. The object is created lazily upon the first actual access.

```typescript
// Get by deterministic name
const stub = env.MY_DURABLE_OBJECT.getByName("foo");

// Or manually: create ID from name, then get stub
const id = env.MY_DURABLE_OBJECT.idFromName("foo");
const stub = env.MY_DURABLE_OBJECT.get(id);

// Generate a random unique ID
const uniqueId = env.MY_DURABLE_OBJECT.newUniqueId();
const stub = env.MY_DURABLE_OBJECT.get(uniqueId);

// Reconstruct from string
const id = env.MY_DURABLE_OBJECT.idFromString(hexString);
const stub = env.MY_DURABLE_OBJECT.get(id);
```

---

## Durable Object ID (DurableObjectId)

**Docs:** https://developers.cloudflare.com/durable-objects/api/id/

A Durable Object ID is a 64-digit hexadecimal number that uniquely identifies a Durable Object instance. IDs are constructed indirectly via the `DurableObjectNamespace` interface.

**Methods:**

- `toString()` - Converts the ID to a 64-digit hex string for logging or storage (e.g., session cookies). Can be reconstructed via `idFromString`.
- `equals(other: DurableObjectId)` - Compares equality between two IDs. Returns boolean.

**Properties:**

- `name` - Optional property returning the name from `idFromName()` creation. Returns `undefined` for IDs created via `newUniqueId()`.

```javascript
// toString() - convert ID to string for storage (e.g., session cookie)
const id = env.MY_DURABLE_OBJECT.newUniqueId();
const session_id = id.toString();

// Recreate the ID from the string
const id = env.MY_DURABLE_OBJECT.idFromString(session_id);

// equals() - compare two IDs
const id1 = env.MY_DURABLE_OBJECT.newUniqueId();
const id2 = env.MY_DURABLE_OBJECT.newUniqueId();
console.assert(!id1.equals(id2), "Different unique ids should never be equal.");

// name property
const uniqueId = env.MY_DURABLE_OBJECT.newUniqueId();
const fromNameId = env.MY_DURABLE_OBJECT.idFromName("foo");

console.assert(uniqueId.name === undefined, "unique ids have no name");
console.assert(
  fromNameId.name === "foo",
  "name matches parameter to idFromName",
);
```

---

## Durable Object Stub (DurableObjectStub)

**Docs:** https://developers.cloudflare.com/durable-objects/api/stub/

A `DurableObjectStub` is the client interface for invoking methods on a remote Durable Object. It supports RPC method invocation with E-order semantics (Cap'n Proto RPC protocol).

**Key guarantees:**

- When you make multiple calls to the same Durable Object via the same stub, calls are delivered in order.
- If a stub throws an exception, all in-flight calls and future calls will fail. You must recreate the stub.
- Different stubs to the same object have no ordering guarantees between them.

**Properties:**

- `id` - Returns the `DurableObjectId` used to create the stub.
- `name` - Optional property returning the name provided during stub creation via `getByName()` or `idFromName()`. Returns `undefined` for stubs created with `newUniqueId()`.

```javascript
// Using the id property
const id = env.MY_DURABLE_OBJECT.newUniqueId();
const stub = env.MY_DURABLE_OBJECT.get(id);
console.assert(id.equals(stub.id), "This should always be true");

// Using the name property
const stub = env.MY_DURABLE_OBJECT.getByName("foo");
console.assert(stub.name === "foo", "This should always be true");
```

---

## RPC (Remote Procedure Calls)

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/create-durable-object-stubs-and-send-requests/

All new projects should prefer RPC methods (requires compatibility date >= 2024-04-03). By extending `DurableObject`, public methods are automatically exposed as RPC endpoints callable from Workers via stubs. All calls are asynchronous, accept and return serializable types, and propagate exceptions to the caller.

```typescript
import { DurableObject } from "cloudflare:workers";

export interface Env {
  MY_DURABLE_OBJECT: DurableObjectNamespace<MyDurableObject>;
}

export class MyDurableObject extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  async sayHello(): Promise<string> {
    return "Hello, World!";
  }
}

export default {
  async fetch(request, env) {
    const stub = env.MY_DURABLE_OBJECT.getByName("foo");
    const rpcResponse = await stub.sayHello();
    return new Response(rpcResponse);
  },
} satisfies ExportedHandler<Env>;
```

---

## Fetch Handler (HTTP Request/Response)

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/create-durable-object-stubs-and-send-requests/

For projects with compatibility dates before 2024-04-03, or for HTTP Request/Response flows, the `fetch()` handler accepts an HTTP Request and returns a Response. The URL does not have to be a publicly-resolvable hostname.

```typescript
export class MyDurableObject extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  async fetch(request: Request): Promise<Response> {
    return new Response("Hello, World!");
  }
}

export default {
  async fetch(request, env) {
    const stub = env.MY_DURABLE_OBJECT.getByName("foo");
    const response = await stub.fetch(request);
    return response;
  },
} satisfies ExportedHandler<Env>;
```

Legacy pattern using URL-based routing within `fetch()`:

```typescript
export class MyDurableObject extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  private hello(name: string) {
    return new Response(`Hello, ${name}!`);
  }

  private goodbye(name: string) {
    return new Response(`Goodbye, ${name}!`);
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    let name = url.searchParams.get("name");
    if (!name) {
      name = "World";
    }

    switch (url.pathname) {
      case "/hello":
        return this.hello(name);
      case "/goodbye":
        return this.goodbye(name);
      default:
        return new Response("Bad Request", { status: 400 });
    }
  }
}

export default {
  async fetch(_request, env, _ctx) {
    const stub = env.MY_DURABLE_OBJECT.getByName("foo");
    let response = await stub.fetch("http://do/hello?name=World");
    return response;
  },
} satisfies ExportedHandler<Env>;
```

---

## Durable Object State (DurableObjectState / ctx)

**Docs:** https://developers.cloudflare.com/durable-objects/api/state/

The `DurableObjectState` interface manages a Durable Object's lifecycle and WebSocket connections. Accessed via `this.ctx` on Durable Object instances.

**Properties:**

- `id` - Read-only `DurableObjectId` property.
- `storage` - Read-only property providing access to `DurableObjectStorage`.
- `exports` - Contains loopback bindings to Worker's top-level exports.

**Key methods:**

- `blockConcurrencyWhile(callback)` - Executes an async callback while blocking other events. Has a 30-second timeout. Commonly used in constructors for initialization.
- `waitUntil(promise)` - Present for API compatibility but has no effect in Durable Objects.
- `abort(reason?)` - Forcibly resets a Durable Object and logs an error message. Not available in local development.

**WebSocket methods (Hibernation API):**

- `acceptWebSocket(ws, tags?)` - Accepts a WebSocket connection for hibernation management. Supports up to 32,768 connections per object. Tags limited to 256 characters, max 10 tags per WebSocket.
- `getWebSockets(tag?)` - Returns accepted WebSocket connections, optionally filtered by tag.
- `setWebSocketAutoResponse(response?)` - Sets an automatic response for specific messages without waking the object.
- `getWebSocketAutoResponse()` - Gets the currently configured auto-response.
- `getWebSocketAutoResponseTimestamp(ws)` - Gets the timestamp of the last auto-response.
- `setHibernatableWebSocketEventTimeout(timeout)` - Sets timeout for hibernatable WebSocket events.
- `getTags(ws)` - Gets tags associated with a WebSocket connection.

```typescript
import { DurableObject } from "cloudflare:workers";

export class MyDurableObject extends DurableObject {
  initialized = false;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);

    // blockConcurrencyWhile will ensure that initialized will always be true
    this.ctx.blockConcurrencyWhile(async () => {
      this.initialized = true;
    });
  }

  async sayHello() {
    // Error: Hello, World! will be logged
    this.ctx.abort("Hello, World!");
  }
}
```

---

## SQLite Storage API

**Docs:** https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/
**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/access-durable-objects-storage/

The recommended storage backend for all new Durable Objects. Each object gets a private, embedded SQLite database with up to 10 GB capacity. Provides transactional, strongly consistent, and serializable storage.

### SQL API

Access via `ctx.storage.sql`:

```typescript
export class MyDurableObject extends DurableObject {
  sql: SqlStorage;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;

    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS artist(
        artistid    INTEGER PRIMARY KEY,
        artistname  TEXT
      );
      INSERT INTO artist (artistid, artistname) VALUES
        (123, 'Alice'),
        (456, 'Bob'),
        (789, 'Charlie');
    `);
  }
}
```

**`exec(query: string, ...bindings: any[]): SqlStorageCursor`** - Executes SQL queries with optional `?` placeholder bindings. Returns a cursor supporting:

- `next()` - Returns object with `done` and `value` properties.
- `toArray()` - Returns array of row objects.
- `one()` - Returns single row or throws if results != 1.
- `raw()` - Returns Iterator of column value arrays.
- `columnNames` - string[] of column names in order.
- `rowsRead` - number of rows read.
- `rowsWritten` - number of rows written.

```typescript
// Iterate over results
let cursor = this.sql.exec("SELECT * FROM artist;");
for (let row of cursor) {
  // Process row
}

// Convert to array
let resultsArray = this.sql.exec("SELECT * FROM artist;").toArray();

// Get raw arrays
let rawResults = this.sql.exec("SELECT * FROM artist;").raw().toArray();

// Get single row
let oneRow = this.sql.exec("SELECT * FROM artist WHERE artistname = ?;", "Alice").one();

// Check rowsRead
let cursor = this.sql.exec("SELECT * FROM artist;");
cursor.next();
console.log(cursor.rowsRead); // prints 1
cursor.toArray();
console.log(cursor.rowsRead); // prints 3
```

**`databaseSize: number`** - Returns current SQLite database size in bytes:

```typescript
let size = ctx.storage.sql.databaseSize;
```

**Typed queries:**

```typescript
type User = {
  id: string;
  name: string;
  email_address: string;
  version: number;
};

const result = this.ctx.storage.sql
  .exec<User>(
    "SELECT id, name, email_address, version FROM users WHERE id = ?",
    user_id,
  )
  .one();
```

**Supported SQLite extensions:** FTS5 (full-text search), JSON extension, Math functions.

### SQL Storage Limits

| Feature | Limit |
|---------|-------|
| Maximum columns per table | 100 |
| Maximum rows per table | Unlimited (within 10 GB per-object limit) |
| Maximum string, BLOB, or table row size | 2 MB |
| Maximum SQL statement length | 100 KB |
| Maximum bound parameters per query | 100 |
| Maximum arguments per SQL function | 32 |
| Maximum characters in LIKE/GLOB pattern | 50 bytes |

---

## Synchronous KV API (SQLite-backed only)

**Docs:** https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/

Available only on SQLite-backed Durable Objects. Provides synchronous key-value operations.

- `get(key: string): any | undefined` - Retrieves value or undefined.
- `put(key: string, value: any): void` - Stores value (must support structured clone).
- `delete(key: string): boolean` - Deletes key, returns true if it existed.
- `list(options?): Iterable<string, any>` - Returns all keys/values in ascending UTF-8 order.

**List options:** `start`, `startAfter` (mutually exclusive with `start`), `end`, `prefix`, `reverse`, `limit`.

---

## Asynchronous KV API

**Docs:** https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/
**Docs:** https://developers.cloudflare.com/durable-objects/api/legacy-kv-storage-api/

Available on both SQLite-backed and legacy KV-backed Durable Objects.

- `get(key | keys[], options?)` - Retrieves single value or Map of values (up to 128 keys). Options: `allowConcurrency`, `noCache`.
- `put(key, value | entries, options?)` - Stores single or multiple key-value pairs (up to 128). Options: `allowUnconfirmed`, `noCache`. Multiple `put()` calls without `await` combine atomically (write coalescing).
- `delete(key | keys[], options?)` - Deletes single key (returns boolean) or multiple keys (returns count, up to 128). Options: `allowUnconfirmed`, `noCache`.
- `list(options?)` - Returns all keys/values in ascending UTF-8 order. Same options as sync version plus `allowConcurrency` and `noCache`.

---

## Transactional Storage

**Docs:** https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/

### `transactionSync(callback): any` (SQLite-only)

Wraps synchronous callback in a SQLite transaction. Rolls back on exception.

```typescript
this.ctx.storage.transactionSync(() => {
  // Synchronous operations only
});
```

### `transaction(closureFunction(txn)): Promise`

Wraps storage operations in an atomic transaction. The `txn` object provides `put()`, `get()`, `delete()`, `list()` methods, plus a `rollback()` function.

For SQLite-backed objects, operations on `ctx.storage` are automatically transactional (within a single synchronous execution block).

### `sync(): Promise`

Synchronizes pending writes to disk. Resolves when complete or immediately if no pending writes.

### `deleteAll(options?): Promise`

Deletes all stored data. For SQLite backend, atomic deletion of entire database including SQL and KV data. With compatibility date 2026-02-24+, also deletes active alarms.

---

## Point In Time Recovery (PITR)

**Docs:** https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/

Available only for SQLite-backed Durable Objects. Allows restoring the database to any point within the last 30 days using bookmarks (alphanumeric strings comparable lexically).

- `getCurrentBookmark(): Promise<string>` - Returns bookmark for current point in time.
- `getBookmarkForTime(timestamp: number | Date): Promise<string>` - Returns bookmark for specified time (must be within 30 days).
- `onNextSessionRestoreBookmark(bookmark: string): Promise<string>` - Configures restoration on next restart. Returns special bookmark to undo recovery.

```typescript
const DAY_MS = 24 * 60 * 60 * 1000;
let bookmark = ctx.storage.getBookmarkForTime(Date.now() - 2 * DAY_MS);
ctx.storage.onNextSessionRestoreBookmark(bookmark);
// Then call ctx.abort() to restart
```

---

## Alarms API

**Docs:** https://developers.cloudflare.com/durable-objects/api/alarms/

Alarms enable scheduling a Durable Object to activate at a future time. Each Durable Object can schedule a single alarm at a time. Alarms have guaranteed at-least-once execution and are retried automatically with exponential backoff (2-second initial delay, up to 6 retries).

**Storage methods:**

- `getAlarm(options?): Promise<number | null>` - Returns milliseconds since UNIX epoch if alarm exists, otherwise null.
- `setAlarm(scheduledTime: Date | number, options?): Promise` - Sets alarm time. Overrides existing alarms. If time <= now, executes immediately.
- `deleteAlarm(options?): Promise` - Removes currently set alarm.

**Handler method:**

- `alarm(alarmInfo?: AlarmInvocationInfo)` - System-invoked when scheduled time arrives. `alarmInfo` contains `retryCount` (number) and `isRetry` (boolean).

**Alarms vs Cron Triggers:** A Worker supports up to 3 Cron Triggers, but unlimited Durable Objects each with their own alarm. Alarms are set programmatically; Cron Triggers require dashboard/API configuration.

```javascript
import { DurableObject } from "cloudflare:workers";

export class AgentServer extends DurableObject {
  async scheduleEvent(id, runAt, repeatMs = null) {
    await this.ctx.storage.put(`event:${id}`, { id, runAt, repeatMs });
    const currentAlarm = await this.ctx.storage.getAlarm();
    if (!currentAlarm || runAt < currentAlarm) {
      await this.ctx.storage.setAlarm(runAt);
    }
  }

  async alarm() {
    const now = Date.now();
    const events = await this.ctx.storage.list({ prefix: "event:" });
    let nextAlarm = null;

    for (const [key, event] of events) {
      if (event.runAt <= now) {
        await this.processEvent(event);
        if (event.repeatMs) {
          event.runAt = now + event.repeatMs;
          await this.ctx.storage.put(key, event);
        } else {
          await this.ctx.storage.delete(key);
        }
      }
      if (event.runAt > now && (!nextAlarm || event.runAt < nextAlarm)) {
        nextAlarm = event.runAt;
      }
    }
    if (nextAlarm) await this.ctx.storage.setAlarm(nextAlarm);
  }

  async processEvent(event) {
    // Event handling logic
  }
}
```

Basic alarm with retry tracking:

```javascript
class MyDurableObject extends DurableObject {
  async alarm(alarmInfo) {
    if (alarmInfo?.retryCount != 0) {
      console.log(
        "This alarm event has been attempted ${alarmInfo?.retryCount} times before.",
      );
    }
  }
}
```

**Important:** Alarms do not repeat automatically. You must schedule the next alarm explicitly. Only one alarm per object at a time. Because alarms are retried only up to 6 times, catch exceptions and reschedule if you need indefinite retries.

---

## WebSocket Support (Standard API)

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/websockets/
**Docs:** https://developers.cloudflare.com/durable-objects/examples/websocket-server/

Durable Objects can act as WebSocket servers using the standard Web API. WebSocket connections are created using `WebSocketPair`, where one end is returned to the client and the other is accepted by the Durable Object.

**Key limitation:** WebSocket connections pin your Durable Object to memory, so duration charges accrue as long as the WebSocket is connected (regardless of activity). The WebSocket Hibernation API is recommended instead.

Code updates disconnect all WebSockets. Deploying a new version restarts every Durable Object, which disconnects any existing connections.

```typescript
import { DurableObject } from 'cloudflare:workers';

export class WebSocketServer extends DurableObject {
  currentlyConnectedWebSockets: number;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.currentlyConnectedWebSockets = 0;
  }

  async fetch(request: Request): Promise<Response> {
    const webSocketPair = new WebSocketPair();
    const [client, server] = Object.values(webSocketPair);

    server.accept();
    this.currentlyConnectedWebSockets += 1;

    server.addEventListener("message", (event: MessageEvent) => {
      server.send(
        `[Durable Object] currentlyConnectedWebSockets: ${this.currentlyConnectedWebSockets}`,
      );
    });

    server.addEventListener("close", (cls: CloseEvent) => {
      this.currentlyConnectedWebSockets -= 1;
      server.close(cls.code, "Durable Object is closing WebSocket");
    });

    return new Response(null, {
      status: 101,
      webSocket: client,
    });
  }
}
```

---

## WebSocket Hibernation API

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/websockets/
**Docs:** https://developers.cloudflare.com/durable-objects/examples/websocket-hibernation-server/

The recommended WebSocket approach. Allows Durable Objects to enter a dormant state during inactivity while keeping client WebSocket connections alive. Billable Duration (GB-s) charges do not accrue during hibernation.

**How it works:** When a Durable Object receives no events for ~10 seconds and meets hibernation criteria, it is evicted from memory. WebSocket clients stay connected to Cloudflare's network. Upon an incoming message, the Durable Object reinitializes (constructor runs) and the handler executes.

**Hibernation criteria (ALL must be met):**
- No `setTimeout`/`setInterval` callbacks scheduled
- No in-progress awaited `fetch()` calls
- No WebSocket standard API usage (must use Hibernation API)
- No request/event still processing

**Core methods on `ctx`:**
- `acceptWebSocket(ws, tags?)` - Accepts WebSocket for hibernation management. Up to 32,768 connections per object.
- `getWebSockets(tag?)` - Returns accepted WebSocket connections, optionally filtered by tag.
- `setWebSocketAutoResponse(response?)` - Automatic response without waking the object.

**Handler methods on the Durable Object class:**
- `webSocketMessage(ws, message)` - Called when a message arrives. Does not trigger for control frames.
- `webSocketClose(ws, code, reason, wasClean)` - Called on connection closure. You must call `ws.close(code, reason)` inside this handler to complete the close handshake.
- `webSocketError(ws, error)` - Called for non-disconnection errors.

**Per-connection state persistence through hibernation:**
- `ws.serializeAttachment(value)` - Persists connection state (up to 2,048 bytes per connection).
- `ws.deserializeAttachment()` - Retrieves previously serialized attachment data.

**Hibernation is only supported when the Durable Object acts as a server (not for outgoing WebSocket use cases).**

```typescript
import { DurableObject } from "cloudflare:workers";

export class WebSocketHibernationServer extends DurableObject {
  async fetch(request: Request): Promise<Response> {
    const webSocketPair = new WebSocketPair();
    const [client, server] = Object.values(webSocketPair);

    this.ctx.acceptWebSocket(server);

    return new Response(null, {
      status: 101,
      webSocket: client,
    });
  }

  async webSocketMessage(ws: WebSocket, message: ArrayBuffer | string) {
    ws.send(
      `[Durable Object] message: ${message}, connections: ${this.ctx.getWebSockets().length}`,
    );
  }

  async webSocketClose(
    ws: WebSocket,
    code: number,
    reason: string,
    wasClean: boolean,
  ) {
    ws.close(code, reason);
  }
}
```

Per-connection state persistence through hibernation using attachments:

```typescript
import { DurableObject } from "cloudflare:workers";

interface ConnectionState {
  orderId: string;
  joinedAt: number;
}

export class WebSocketServer extends DurableObject<Env> {
  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const orderId = url.searchParams.get("orderId") ?? "anonymous";

    const webSocketPair = new WebSocketPair();
    const [client, server] = Object.values(webSocketPair);

    this.ctx.acceptWebSocket(server);

    const state: ConnectionState = {
      orderId,
      joinedAt: Date.now(),
    };
    server.serializeAttachment(state);

    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer) {
    const state = ws.deserializeAttachment() as ConnectionState;
    ws.send(`Hello ${state.orderId}, you joined at ${state.joinedAt}`);
  }

  async webSocketClose(ws: WebSocket, code: number, reason: string, wasClean: boolean) {
    const state = ws.deserializeAttachment() as ConnectionState;
    console.log(`${state.orderId} disconnected`);
    ws.close(code, reason);
  }
}
```

---

## In-Memory State

**Docs:** https://developers.cloudflare.com/durable-objects/reference/in-memory-state/

Each Durable Object has one active instance at any time. All requests to that object are handled by that same instance. Instance variables maintain their state as long as the object remains in memory before eviction.

Common pattern: initialize from persistent storage during first access, then cache in instance variables.

```javascript
import { DurableObject } from "cloudflare:workers";

export class Counter extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    // `blockConcurrencyWhile()` ensures no requests are delivered until
    // initialization completes.
    this.ctx.blockConcurrencyWhile(async () => {
      let stored = await this.ctx.storage.get("value");
      // After initialization, future reads do not need to access storage.
      this.value = stored || 0;
    });
  }

  // Handle HTTP requests from clients.
  async fetch(request) {
    // use this.value rather than storage
  }
}
```

**Important:** Each Durable Object instance maintains separate memory for instance variables (`this.value`), but may share global memory with other instances, making global variables problematic. The storage API includes automatic in-memory caching, so recently accessed values return instantly.

---

## Durable Object Lifecycle

**Docs:** https://developers.cloudflare.com/durable-objects/concepts/durable-object-lifecycle/

Durable Objects transition through five states:

1. **Active, in-memory** - Running and processing requests.
2. **Idle, in-memory non-hibernateable** - Waiting for requests but cannot hibernate (e.g., has active timers).
3. **Idle, in-memory hibernateable** - Waiting and qualifies for hibernation. After ~10 seconds of inactivity, transitions to hibernated.
4. **Hibernated** - Removed from memory; WebSocket connections remain active. In-memory state is discarded.
5. **Inactive** - Completely removed from host process. Requires cold start on reactivation.

**Billing:** Charges only when actively running in-memory, or idle in-memory and non-hibernateable.

**Eviction timing:** If hibernation conditions are not met, the object is evicted entirely from memory after 70-140 seconds of inactivity.

**Shutdown:** HTTP requests receive up to 30 seconds to complete. WebSocket connections terminate automatically. No shutdown hooks are provided. Write state incrementally to storage rather than relying on shutdown callbacks.

---

## Input/Output Gates and Concurrency

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/
**Docs:** https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/

JavaScript is single-threaded and event-driven. Durable Objects uses input/output gates to prevent concurrency bugs:

- **Input gates** block new events during synchronous JavaScript execution.
- **Output gates** hold responses until pending storage writes complete.
- **Write coalescing** batches multiple storage writes without intervening awaits into atomic transactions.

`fetch()` and other non-storage I/O open input gates, allowing request interleaving. For atomic read-modify-write patterns, prefer `transaction()` or `transactionSync()`.

`blockConcurrencyWhile(callback)` blocks all concurrency unconditionally. Reserve for initialization only since it limits throughput to ~200 req/sec if each operation takes 5ms.

---

## Configuration (wrangler.toml / wrangler.jsonc)

**Docs:** https://developers.cloudflare.com/durable-objects/get-started/

### Durable Object Bindings

```json
{
  "durable_objects": {
    "bindings": [
      {
        "name": "MY_DURABLE_OBJECT",
        "class_name": "MyDurableObject"
      }
    ]
  }
}
```

```toml
[[durable_objects.bindings]]
name = "MY_DURABLE_OBJECT"
class_name = "MyDurableObject"
```

Fields: `name` (binding name in Worker), `class_name` (the class to bind), optional `script_name` (defaults to current Worker).

### Migrations (SQLite)

```json
{
  "migrations": [
    {
      "tag": "v1",
      "new_sqlite_classes": ["MyDurableObject"]
    }
  ]
}
```

```toml
[[migrations]]
tag = "v1"
new_sqlite_classes = [ "MyDurableObject" ]
```

### Migrations (Legacy KV)

```json
{
  "migrations": [
    {
      "tag": "v1",
      "new_classes": ["MyDurableObject"]
    }
  ]
}
```

```toml
[[migrations]]
tag = "v1"
new_classes = [ "MyDurableObject" ]
```

Each migration requires a unique `tag`. Tags are used to determine which migrations have already been applied.

---

## Migrations (Create, Rename, Delete, Transfer)

**Docs:** https://developers.cloudflare.com/durable-objects/reference/durable-objects-migrations/

Migrations map class names to runtime states. Required when creating, renaming, deleting, or transferring Durable Object classes. NOT required when updating code for an existing class.

### Create Migration

```json
{
  "migrations": [
    {
      "tag": "v1",
      "new_sqlite_classes": ["DurableObjectAClass"]
    }
  ]
}
```

### Delete Migration

Deletes all Durable Objects and their stored data for the class.

```json
{
  "migrations": [
    {
      "tag": "v3",
      "deleted_classes": ["DeprecatedObjectClass"]
    }
  ]
}
```

### Rename Migration

Transfers stored data between two classes in the same Worker.

```json
{
  "durable_objects": {
    "bindings": [
      {
        "name": "MY_DURABLE_OBJECT",
        "class_name": "UpdatedName"
      }
    ]
  },
  "migrations": [
    {
      "tag": "v3",
      "renamed_classes": [
        {
          "from": "OldName",
          "to": "UpdatedName"
        }
      ]
    }
  ]
}
```

### Transfer Migration

Transfers stored data between classes in different Worker scripts.

```json
{
  "durable_objects": {
    "bindings": [
      {
        "name": "MY_DURABLE_OBJECT",
        "class_name": "TransferredClass"
      }
    ]
  },
  "migrations": [
    {
      "tag": "v4",
      "transferred_classes": [
        {
          "from": "DurableObjectExample",
          "from_script": "OldWorkerScript",
          "to": "TransferredClass"
        }
      ]
    }
  ]
}
```

**Key rules:**
- Migration tags are unique per environment.
- Migrations are applied at deployment, each only once per environment.
- Environment-level migrations override top-level migrations.
- Cannot enable SQLite backend on existing deployed classes.
- Durable Object migrations are atomic and cannot be gradually deployed.
- After rename/transfer, existing bindings from other Workers automatically forward to the updated destination class.

---

## Data Location (Jurisdictions and Location Hints)

**Docs:** https://developers.cloudflare.com/durable-objects/reference/data-location/

### Jurisdictions

Restrict Durable Objects to specific jurisdictions to comply with regulations (GDPR, FedRAMP). Workers can access jurisdiction-constrained objects globally, but the objects only execute and store data within their designated region.

Supported jurisdictions: `eu` (European Union), `fedramp` (FedRAMP-compliant data centers).

```javascript
const euSubnamespace = env.MY_DURABLE_OBJECT.jurisdiction("eu");
const euId = euSubnamespace.newUniqueId();
```

The same name generates different IDs across jurisdictions.

### Location Hints

Guide initial Durable Object placement (not guaranteed). Supported hints: `wnam`, `enam`, `sam`, `weur`, `eeur`, `apac`, `oc`, `afr`, `me`.

```javascript
let durableObjectStub = OBJECT_NAMESPACE.get(id, { locationHint: "enam" });
```

---

## Environments

**Docs:** https://developers.cloudflare.com/durable-objects/reference/environments/

Wrangler enables deploying the same Worker with different configurations per environment. Durable Object bindings must be specified individually for each environment (not inherited from top-level).

```json
{
  "env": {
    "staging": {
      "durable_objects": {
        "bindings": [
          {
            "name": "EXAMPLE_CLASS",
            "class_name": "DurableObjectExample"
          }
        ]
      }
    }
  }
}
```

When Wrangler appends the environment name to the Worker name (e.g., `worker-name-staging`), the binding accesses different Durable Objects than the top-level binding.

To access top-level objects from environment-specific bindings, use `script_name`:

```json
{
  "env": {
    "another": {
      "durable_objects": {
        "bindings": [
          {
            "name": "EXAMPLE_CLASS",
            "class_name": "DurableObjectExample",
            "script_name": "worker-name"
          }
        ]
      }
    }
  }
}
```

---

## Error Handling

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/error-handling/

When exceptions occur, they propagate to the client's callsite. Key exception properties:

- `.retryable` (boolean) - Indicates transient failures suitable for retry (with exponential backoff).
- `.overloaded` (boolean) - Signals resource constraints. Do NOT retry; retrying worsens the overload.
- `.remote` (boolean) - Indicates whether the error came from user code or infrastructure.

After exceptions, avoid reusing stubs. Create a new stub for subsequent requests.

```typescript
import { DurableObject } from "cloudflare:workers";

export interface Env {
  ErrorThrowingObject: DurableObjectNamespace;
}

export default {
  async fetch(request, env, ctx) {
    let userId = new URL(request.url).searchParams.get("userId") || "";

    // Retry behavior can be adjusted to fit your application.
    let maxAttempts = 3;
    let baseBackoffMs = 100;
    let maxBackoffMs = 20000;

    let attempt = 0;
    while (true) {
      // Try sending the request
      try {
        // Create a Durable Object stub for each attempt, because certain types of
        // errors will break the Durable Object stub.
        const doStub = env.ErrorThrowingObject.getByName(userId);
        const resp = await doStub.fetch("http://your-do/");

        return Response.json(resp);
      } catch (e: any) {
        if (!e.retryable) {
          // Failure was not a transient internal error, so don't retry.
          break;
        }
      }

      let backoffMs = Math.min(
        maxBackoffMs,
        baseBackoffMs * Math.random() * Math.pow(2, attempt),
      );

      attempt += 1;
      if (attempt >= maxAttempts) {
        // Reached max attempts, so don't retry.
        break;
      }

      await scheduler.wait(backoffMs);
    }

    return new Response("server error", { status: 500 });
  },
} satisfies ExportedHandler<Env>;

export class ErrorThrowingObject extends DurableObject {
  constructor(state: DurableObjectState, env: Env) {
    super(state, env);

    // Any exceptions that are raised in your constructor will also set the
    // .remote property to True
    throw new Error("no good");
  }

  async fetch(req: Request) {
    // Generate an uncaught exception
    // A .remote property will be added to the exception propagated to the caller
    // and will be set to True
    throw new Error("example error");

    // We never reach this
    return Response.json({});
  }
}
```

---

## TTL (Time To Live) Pattern

**Docs:** https://developers.cloudflare.com/durable-objects/examples/durable-object-ttl/

Use the Alarms API to implement TTL on Durable Objects. Reset the alarm on each request; when the alarm fires, clean up with `deleteAll()`.

Pattern:
1. Define a TTL duration (e.g., 60 seconds).
2. In `fetch()` or RPC methods, call `setAlarm(Date.now() + TTL)` to reset the timer.
3. In `alarm()`, call `deleteAll()` to clean up.

```typescript
import { DurableObject } from "cloudflare:workers";

export interface Env {
  MY_DURABLE_OBJECT: DurableObjectNamespace<MyDurableObject>;
}

// Durable Object
export class MyDurableObject extends DurableObject {
  // Time To Live (TTL) in milliseconds
  timeToLiveMs = 1000;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  async fetch(_request: Request) {
    // Extend the TTL immediately following every fetch request to a Durable Object.
    await this.ctx.storage.setAlarm(Date.now() + this.timeToLiveMs);
    ...
   }

  async alarm() {
    await this.ctx.storage.deleteAll();
  }
}

// Worker
export default {
  async fetch(request, env) {
    const stub = env.MY_DURABLE_OBJECT.getByName("foo");
    return await stub.fetch(request);
  },
} satisfies ExportedHandler<Env>;
```

---

## Counter Example (Read-Modify-Write)

**Docs:** https://developers.cloudflare.com/durable-objects/examples/build-a-counter/

Demonstrates safe read-modify-write operations using input gates (no explicit locking needed):

```typescript
export class Counter extends DurableObject {
  async increment(amount = 1) {
    let value = (await this.ctx.storage.get("value")) || 0;
    value += amount;
    await this.ctx.storage.put("value", value);
    return value;
  }
}
```

---

## ReadableStream Support

**Docs:** https://developers.cloudflare.com/durable-objects/examples/readable-stream/

Durable Objects can stream ReadableStream data to Workers. If the Worker cancels the Durable Object's readable stream, the cancellation propagates to the Durable Object.

```typescript
import { DurableObject } from 'cloudflare:workers';

// Send incremented counter value every second
async function* dataSource(signal: AbortSignal) {
    let counter = 0;
    while (!signal.aborted) {
        yield counter++;
        await new Promise((resolve) => setTimeout(resolve, 1_000));
    }
    console.log('Data source cancelled');
}

export class MyDurableObject extends DurableObject<Env> {
    async fetch(request: Request): Promise<Response> {
        const abortController = new AbortController();
        const stream = new ReadableStream({
            async start(controller) {
                if (request.signal.aborted) {
                    controller.close();
                    abortController.abort();
                    return;
                }
                for await (const value of dataSource(abortController.signal)) {
                    controller.enqueue(new TextEncoder().encode(String(value)));
                }
            },
            cancel() {
                console.log('Stream cancelled');
                abortController.abort();
            },
        });
        const headers = new Headers({
            'Content-Type': 'application/octet-stream',
        });
        return new Response(stream, { headers });
    }
}

export default {
    async fetch(request, env, ctx): Promise<Response> {
        const stub = env.MY_DURABLE_OBJECT.getByName("foo");
        const response = await stub.fetch(request, { ...request });
        if (!response.ok || !response.body) {
            return new Response('Invalid response', { status: 500 });
        }
        const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
        let data = [] as string[];
        let i = 0;
        while (true) {
            if (i > 5) {
                reader.cancel();
                break;
            }
            const { value, done } = await reader.read();
            if (value) {
                console.log(`Got value ${value}`);
                data = [...data, value];
            }
            if (done) {
                break;
            }
            i++;
        }
        return Response.json(data);
    },
} satisfies ExportedHandler<Env>;
```

---

## Container API

**Docs:** https://developers.cloudflare.com/durable-objects/api/container/

Durable Objects can start, stop, and interact with an associated container through `ctx.container`.

**Key methods/properties:**
- `running` - Indicates whether a container is currently operational (does not ensure full readiness).
- `start(options?)` - Initiates container boot with optional environment variables, entrypoint commands, and internet access settings.
- `destroy(reason?)` - Halts the container, can return custom error messages.
- `signal(signal)` - Sends IPC signals (SIGTERM, SIGKILL) for graceful/forceful termination.
- `getTcpPort(port)` - Retrieves a TCP port for container communication (HTTP or direct TCP).
- `monitor()` - Returns a promise that resolves on container exit or rejects on errors.

```typescript
export class MyDurableObject extends DurableObject {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.ctx.blockConcurrencyWhile(async () => {
      this.ctx.container.start();
    });
  }
}
```

```javascript
// Start with options
this.ctx.container.start({
  env: {
    FOO: "bar",
  },
  enableInternet: false,
  entrypoint: ["node", "server.js"],
});

// Destroy a container
this.ctx.container.destroy("Manually Destroyed");

// Send a signal
const SIGTERM = 15;
this.ctx.container.signal(SIGTERM);

// Fetch from a container TCP port
const port = this.ctx.container.getTcpPort(8080);
const res = await port.fetch("http://container/set-state", {
  body: initialState,
  method: "POST",
});
```

```javascript
// Monitor container lifecycle
class MyContainer extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    function onContainerExit() {
      console.log("Container exited");
    }
    async function onContainerError(err) {
      console.log("Container errored", err);
    }
    this.ctx.container.start();
    this.ctx.container.monitor().then(onContainerExit).catch(onContainerError);
  }
}
```

---

## WebGPU API

**Docs:** https://developers.cloudflare.com/durable-objects/api/webgpu/

WebGPU is available only in Durable Objects (not Workers) and only in local development. Cannot be deployed to production. Supports compute pipelines but not rendering pipelines.

Configuration:
```
compatibility_flags = ["experimental", "webgpu"]
```

---

## Testing

**Docs:** https://developers.cloudflare.com/durable-objects/examples/testing-with-durable-objects/

Use `@cloudflare/vitest-pool-workers` with Vitest for testing. Each test gets isolated storage. Key testing utilities from `cloudflare:test`:

- `env` - Access to environment bindings including Durable Object namespaces.
- `SELF` - For integration tests making HTTP requests.
- `runInDurableObject(stub, callback)` - Access instance internals and storage directly.
- `listDurableObjectIds(namespace)` - List all Durable Object IDs in a namespace.
- `runDurableObjectAlarm(stub)` - Trigger alarms immediately in tests.

```typescript
import { defineWorkersConfig } from "@cloudflare/vitest-pool-workers/config";

export default defineWorkersConfig({
  test: {
    poolOptions: {
      workers: {
        wrangler: { configPath: "./wrangler.jsonc" },
      },
    },
  },
});
```

Unit test example:

```javascript
import { env } from "cloudflare:test";
import { describe, it, expect, beforeEach } from "vitest";

describe("Counter Durable Object", () => {
  it("should increment the counter", async () => {
    const id = env.COUNTER.idFromName("test-counter");
    const stub = env.COUNTER.get(id);

    const count1 = await stub.increment();
    expect(count1).toBe(1);

    const count2 = await stub.increment();
    expect(count2).toBe(2);
  });

  it("should isolate different Durable Object instances", async () => {
    const id1 = env.COUNTER.idFromName("counter-1");
    const id2 = env.COUNTER.idFromName("counter-2");

    const stub1 = env.COUNTER.get(id1);
    const stub2 = env.COUNTER.get(id2);

    await stub1.increment();
    await stub1.increment();
    await stub2.increment();

    expect(await stub1.getCount()).toBe(2);
    expect(await stub2.getCount()).toBe(1);
  });
});
```

Direct access to internals:

```javascript
import { env, runInDurableObject, listDurableObjectIds } from "cloudflare:test";

await runInDurableObject(stub, async (instance, state) => {
  expect(instance).toBeInstanceOf(Counter);

  const result = state.storage.sql
    .exec("SELECT value FROM counters WHERE name = ?", "default")
    .one();
  expect(result.value).toBe(2);
});
```

Testing alarms:

```javascript
import { env, runInDurableObject, runDurableObjectAlarm } from "cloudflare:test";

await runInDurableObject(stub, async (instance, state) => {
  await state.storage.setAlarm(Date.now() + 60_000);
});

const alarmRan = await runDurableObjectAlarm(stub);
expect(alarmRan).toBe(true);
```

---

## Observability (Metrics and Analytics)

**Docs:** https://developers.cloudflare.com/durable-objects/observability/metrics-and-analytics/

Durable Objects expose namespace-level and request-level metrics via the Cloudflare dashboard and GraphQL Analytics API.

**GraphQL datasets:**
- `durableObjectsInvocationsAdaptiveGroups`
- `durableObjectsPeriodicGroups`
- `durableObjectsStorageGroups`
- `durableObjectsSubrequestsAdaptiveGroups`

**Logging:** Enable observability in wrangler config. The field `$workers.durableObjectId` identifies the specific instance generating a log entry.

---

## Data Security

**Docs:** https://developers.cloudflare.com/durable-objects/reference/data-security/

- **Encryption at rest:** All data and metadata encrypted using AES-256 via LUKS. No user configuration required.
- **Encryption in transit:** TLS/SSL for all data movement between Workers and Durable Objects and between internal network nodes.

---

## Pricing

**Docs:** https://developers.cloudflare.com/durable-objects/platform/pricing/

Durable Objects incur two types of billing: compute and storage. Available on both Free and Paid plans.

### Compute

| Metric | Free Plan | Paid Plan |
|--------|-----------|-----------|
| Requests | 100,000/day | 1M included + $0.15/million |
| Duration | 13,000 GB-s/day | 400,000 GB-s included + $12.50/million GB-s |

Requests include HTTP requests, RPC sessions, WebSocket messages, and alarm invocations.

### Storage (SQLite)

| Metric | Free Plan | Paid Plan |
|--------|-----------|-----------|
| Rows read | 5M/day | 25B/month included + $0.001/million |
| Rows written | 100K/day | 50M/month included + $1.00/million |
| Stored data | 5 GB total | 5 GB-month included + $0.20/GB-month |

### Storage (Legacy KV, Paid Only)

| Metric | Included | Overage |
|--------|----------|---------|
| Read units | 1M | $0.20/million |
| Write units | 1M | $1.00/million |
| Delete requests | 1M | $1.00/million |
| Stored data | 1 GB | $0.20/GB-month |

---

## Limits

**Docs:** https://developers.cloudflare.com/durable-objects/platform/limits/

### SQLite-backed Durable Objects

| Feature | Limit |
|---------|-------|
| Number of Objects | Unlimited |
| Max classes per account | 500 (Paid) / 100 (Free) |
| Storage per account | Unlimited (Paid) / 5 GB (Free) |
| Storage per Durable Object | 10 GB |
| Key + value combined size | 2 MB |
| WebSocket message size | 32 MiB (received only) |
| CPU per request | 30s default, configurable to 5 min |

### KV-backed Durable Objects

| Feature | Limit |
|---------|-------|
| Number of Objects | Unlimited |
| Storage per account | 50 GB (can be raised) |
| Storage per Durable Object | Unlimited |
| Key size | 2 KiB (2048 bytes) |
| Value size | 128 KiB (131072 bytes) |
| WebSocket message size | 32 MiB (received only) |
| CPU per request | 30s |

### Wall Time Limits

| Invocation Type | Wall Time Limit |
|-----------------|-----------------|
| Incoming HTTP request | Unlimited (while client connected) |
| Cron Triggers | 15 minutes |
| Queue consumers | 15 minutes |
| Alarm handlers | 15 minutes |
| RPC / HTTP from DO | Unlimited (while caller connected) |

### Throughput

A single Durable Object handles approximately 500-1,000 requests per second depending on operation complexity:
- Simple operations: ~1,000 req/sec
- Moderate processing: ~500-750 req/sec
- Complex operations: ~200-500 req/sec

---

## Gradual Deployments

**Docs:** https://developers.cloudflare.com/durable-objects/reference/durable-object-gradual-deployments/

Supports gradually deploying changes to Durable Objects code. Note that Durable Object migrations themselves are atomic and cannot be gradually deployed, but code changes can be.

---

## Troubleshooting

**Docs:** https://developers.cloudflare.com/durable-objects/observability/troubleshooting/

### Debugging Tools

- `wrangler dev --remote` - Tunnel from local to Cloudflare network for testing.
- `wrangler tail` - Live feed of console and exception logs.

### Common Errors

- **Overload errors:** "Too many requests queued", "Too much data queued", "Requests queued for too long", "Too many requests within 10 second window". Solutions: reduce work per request or shard across more DO instances.
- **"Your account is generating too much load"** - Rate limit on stub creation. Retry after brief delays.
- **"Durable Object reset because its code was updated"** - In-memory state lost; persisted data unaffected.
- **Storage timeout** - `deleteAll()` may timeout with large datasets. Safe to retry (makes progress each call).
- **"Too many concurrent storage operations"** - Use batch `get(keys[])` instead of individual calls.

---

## Known Issues

**Docs:** https://developers.cloudflare.com/durable-objects/platform/known-issues/

1. **Global uniqueness gap** - If an event takes time and never accesses storage, the object may no longer be current. Storage access then causes an exception.
2. **Code update race** - Deployments roll out gradually. Newer Worker versions may communicate with older DO versions. Ensure API compatibility.
3. **Development tool limitations:**
   - `wrangler tail` WebSocket request logs delayed until connection closes.
   - Dashboard editor cannot access DOs exported by other Workers.
   - `wrangler dev` keeps writes in memory (not persisted) unless `script_name` specified.
4. **Alarm hot reload issue** - Alarm methods may fail after hot reload in local dev. Restart `wrangler dev` as workaround.

---

## Sharding and Design Patterns

**Docs:** https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/

### The "Atom of Coordination"

Model each Durable Object around a logical unit needing coordination (chat room, game session, document, user workspace). Never create a single global Durable Object (anti-pattern / bottleneck).

### Sharding Formula

Required DOs = (Total requests/second) / (Requests per DO capacity)

### Parent-Child Pattern

Create separate child Durable Objects for hierarchical data. Parents coordinate; children maintain independent state.

### ID Strategies

- `idFromName(name)` - Deterministic, consistent routing. Preferred for most use cases.
- `newUniqueId()` - Random, requires storing ID mapping externally (e.g., in D1).

### Schema Migrations with blockConcurrencyWhile

Track schema versions manually (PRAGMA user_version unsupported). Use a `_sql_schema_migrations` table.

### Idempotent Operations

Alarms may fire multiple times. Always check state before performing actions to avoid duplicates. Write state incrementally. Design for unexpected shutdowns since there are no shutdown hooks.

---

## RpcTarget Class for Durable Object Metadata

**Docs:** https://developers.cloudflare.com/durable-objects/examples/reference-do-name-using-init/

A Durable Object cannot directly access its own name via `this.ctx.id.name`. The solution uses an `RpcTarget` class that automatically carries the name with each method call. Two approaches: non-persistent (metadata stored temporarily) and persistent (stored via `ctx.storage.put()`).

Non-persistent metadata example:

```typescript
import { DurableObject, RpcTarget } from "cloudflare:workers";

export class RpcDO extends RpcTarget {
  constructor(
    private mainDo: MyDurableObject,
    private doIdentifier: string,
  ) {
    super();
  }

  async computeMessage(userName: string): Promise<string> {
    return this.mainDo.computeMessage(userName, this.doIdentifier);
  }

  async simpleGreeting(userName: string) {
    return this.mainDo.simpleGreeting(userName);
  }
}

export class MyDurableObject extends DurableObject<Env> {
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
  }

  async setMetaData(doIdentifier: string) {
    return new RpcDO(this, doIdentifier);
  }

  async computeMessage(
    userName: string,
    doIdentifier: string,
  ): Promise<string> {
    console.log({
      userName: userName,
      durableObjectIdentifier: doIdentifier,
    });
    return `Hello, ${userName}! The identifier of this DO is ${doIdentifier}`;
  }

  private async notInRpcTarget() {
    return "This is not in the RpcTarget";
  }

  async simpleGreeting(userName: string) {
    console.log(this.notInRpcTarget());
    return `Hello, ${userName}! This doesn't use the DO identifier.`;
  }
}

export default {
  async fetch(request, env, ctx): Promise<Response> {
    let id: DurableObjectId = env.MY_DURABLE_OBJECT.idFromName(
      new URL(request.url).pathname,
    );
    let stub = env.MY_DURABLE_OBJECT.get(id);

    const rpcTarget = stub.setMetaData(id.name ?? "default");

    const greeting = await rpcTarget.computeMessage("world");

    const simpleGreeting = await rpcTarget.simpleGreeting("world");

    try {
      (await rpcTarget)[Symbol.dispose]?.();
      console.log("RpcTarget cleaned up.");
    } catch (e) {
      console.error({
        message: "RpcTarget could not be cleaned up.",
        error: String(e),
        errorProperties: e,
      });
    }

    return new Response(greeting, { status: 200 });
  },
} satisfies ExportedHandler<Env>;
```

---

## REST API

**Docs:** https://developers.cloudflare.com/durable-objects/durable-objects-rest-api/

Cloudflare provides REST API endpoints for managing Durable Objects programmatically outside of Worker code.

---

## Legacy KV Storage Backend

**Docs:** https://developers.cloudflare.com/durable-objects/api/legacy-kv-storage-api/

The original storage backend. Only supports Asynchronous KV API and Alarms API. Does NOT support SQL API, PITR API, or Synchronous KV API. Only available on Workers Paid plan. Key size limit: 2 KiB. Value size limit: 128 KiB.

New projects should use SQLite-backed storage instead.

---

## Rust API (workers-rs)

**Docs:** https://developers.cloudflare.com/durable-objects/api/workers-rs/

Durable Objects can be implemented in Rust using the `workers-rs` crate, providing Rust-native bindings to the Workers runtime.

---

## Storage Options Summary

**Docs:** https://developers.cloudflare.com/durable-objects/platform/storage-options/

Two storage backends:

1. **SQLite** (recommended for all new projects) - SQL API, Sync KV API, Async KV API, PITR, Alarms. Free + Paid plans. 10 GB per object.
2. **KV** (legacy) - Async KV API, Alarms only. Paid plan only. 128 KiB value limit.
