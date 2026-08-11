# PartyKit Feature Reference

> **As of:** 2026-03-06
>
> **PartyKit basis:** docs.partykit.io accessed 2026-03-06. PartyKit docs are versionless at the documentation layer.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Migration framing:** PartyKit and Rivet Actors both center on long-lived room-like stateful processes. The biggest gaps are storage semantics, client protocol ergonomics, and any PartyKit-specific ecosystem package such as `y-partykit` or `partykit-ai`.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Party Server Definition | TypeScript classes implementing `Party.Server` with a `Party.Room` instance | native | high | [Actors Index](https://rivet.dev/docs/actors), [Actor Keys](https://rivet.dev/docs/actors/keys) | [examples/raw-websocket-handler/src/actors.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/raw-websocket-handler/src/actors.ts) | Low | Rooms map naturally to actor keys. |
| WebSocket Connections | Lifecycle handlers for connect, message, close, and error events | native | high | [Connections](https://rivet.dev/docs/actors/connections), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts), [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | Low | High-level and raw WS paths both exist. |
| WebSocket Messaging and Broadcasting | Broadcast to all connections or send to individual clients | native | high | [Realtime](https://rivet.dev/docs/actors/events), [Connections](https://rivet.dev/docs/actors/connections) | [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | Low | Strong fit. |
| `Party.Connection` Properties | Per-connection id, uri, state, `setState`, `send`, and `close` methods | partial | high | [Connections](https://rivet.dev/docs/actors/connections), [Metadata](https://rivet.dev/docs/actors/metadata) | [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | Medium | Connection state maps well, but URI/request metadata lives on request context rather than a PartyKit-shaped object. |
| Connection Tags (Metadata and Filtering) | Attach metadata tags to connections and filter connections by tag | pattern | high | [Connections](https://rivet.dev/docs/actors/connections) | [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | Medium | Store tags in `connState` and filter `c.conns` yourself. |
| Room and Party Management | `Party.Room` properties including id, env, storage, and on-demand room creation | native | high | [Actor Keys](https://rivet.dev/docs/actors/keys), [Metadata](https://rivet.dev/docs/actors/metadata) | [actor-metadata.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-metadata.ts) | Low | Strong fit. |
| State and Storage | Transactional key-value storage API for persisting room data across restarts | partial | high | [State](https://rivet.dev/docs/actors/state), [Low-Level KV Storage](https://rivet.dev/docs/actors/kv), [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-kv.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-kv.ts), [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | High | Rivet splits state/KV/SQLite; PartyKit's transactional room storage does not map 1:1. |
| In-Memory State | Instance variables that persist across messages within a single lifecycle | native | high | [Ephemeral Variables](https://rivet.dev/docs/actors/ephemeral-variables) | [actor-vars.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-vars.ts) | Low | Direct fit via `vars`. |
| HTTP Request Handlers | Handle HTTP requests to room URLs via `onRequest` with full room access | native | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Low | Direct fit. |
| Edge Middleware (Static Handlers) | Static methods running at the edge for auth and request interception | pattern | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [Authentication](https://rivet.dev/docs/actors/authentication) | [examples/cloudflare-workers-hono/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers-hono/src/index.ts) | Medium | Implement at the host router/entrypoint before the actor call. |
| Multi-Party Communication | Configure multiple parties per project with inter-party HTTP and WebSocket calls | native | high | [Communicating Between Actors](https://rivet.dev/docs/actors/communicating-between-actors), [Design Patterns](https://rivet.dev/docs/actors/design-patterns) | Docs-only | Low | Multiple actor types plus actor-to-actor calls fit well. |
| Cron Jobs / Scheduled Tasks | Project-level scheduled tasks via cron expressions in `partykit.json` | pattern | medium | [Actor Scheduling](https://rivet.dev/docs/actors/schedule) | [actor-schedule.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-schedule.ts) | Medium | Timers are native, cron expressions need a host-platform or coordinator pattern. |
| Alarms (Room-Level Scheduled Tasks) | Per-room scheduled tasks with one active alarm per room at a time | native | high | [Actor Scheduling](https://rivet.dev/docs/actors/schedule) | [actor-schedule.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-schedule.ts) | Low | Good fit. |
| Hibernation API | Unload server from memory between messages while keeping WebSocket connections alive | partial | high | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler), [Limits](https://rivet.dev/docs/actors/limits) | [actor-conn-hibernation.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-hibernation.ts) | High | Rivet supports hibernation, but raw `onWebSocket` hibernation is still documented as experimental. |
| Server Lifecycle (`onStart`) | Async initialization method that runs before any connect or request handlers | native | high | [Lifecycle](https://rivet.dev/docs/actors/lifecycle) | [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts) | Low | Use `onWake` plus `onCreate` as needed. |
| Configuration (`partykit.json`) | Project configuration for parties, static assets, cron schedules, and build settings | pattern | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers/wrangler.json](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/wrangler.json) | Medium | Config is split between Rivet registry and your host platform config. |
| Client Library (PartySocket) | Auto-reconnecting WebSocket client with message buffering and multi-platform support | partial | medium | [JavaScript Client](https://rivet.dev/docs/clients/javascript), [Connections](https://rivet.dev/docs/actors/connections) | [examples/cursors-raw-websocket/frontend/App.tsx](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cursors-raw-websocket/frontend/App.tsx) | Medium | Rivet clients are strong, but PartySocket-specific reconnect/buffering semantics should be validated before assuming parity. |
| React Hook (`usePartySocket`) | React hook for managing PartySocket lifecycle with component mounting | partial | high | [React](https://rivet.dev/docs/clients/react) | [clients/react.mdx](https://github.com/rivet-dev/rivet/blob/ba46891b1/website/src/content/docs/clients/react.mdx) | Low | `useActor` is the normal replacement. |
| Y.js / CRDT Support (`y-partykit`) | Addon library for hosting Yjs collaborative document backends | pattern | low | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler), [Design Patterns](https://rivet.dev/docs/actors/design-patterns) | Gap | High | Rivet has no first-class `y-partykit` equivalent. Build or keep a custom CRDT transport layer. |
| AI Integration (`partykit-ai`) | Cloudflare AI model access and Vectorize vector database integration | external | low | [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors) | [examples/sandbox/src/actors/ai/ai-agent.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/ai/ai-agent.ts) | Medium | AI/vector features are external integrations orchestrated by Rivet. |
| Authentication | WebSocket and HTTP authentication via edge middleware and token validation | native | high | [Authentication](https://rivet.dev/docs/actors/authentication), [Connections](https://rivet.dev/docs/actors/connections) | [access-control.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/access-control.ts) | Low | Strong fit. |
| Binary Message Handling | Support for `ArrayBufferLike` messages with MessagePack and mixed encoding | native | high | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler), [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) | Low | Good fit. |
| Rate Limiting | Per-connection and per-room message rate limiting with back-off strategies | pattern | high | [Design Patterns](https://rivet.dev/docs/actors/design-patterns), [Actor Scheduling](https://rivet.dev/docs/actors/schedule) | Docs-only | Medium | Actor-local rate limiters are a good fit, but you must build them explicitly. |
| Input Validation | Schema-based validation of WebSocket and HTTP inputs using Zod | native | high | [Actions](https://rivet.dev/docs/actors/actions), [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [examples/sandbox/src/actors/actions/action-inputs.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/actions/action-inputs.ts) | Low | Straightforward app-layer validation. |
| Static Asset Serving | Serve static files with caching, SPA mode, and build integration | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | Low | This belongs in the web host/platform, not the actor runtime. |
| Environment Variables | Persistent secrets and per-deployment variables accessible via `room.env` | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/index.ts) | Low | Good fit. |
| Preview Environments | Deploy to custom preview URLs for testing before production | external | medium | [Deploying to Cloudflare Workers](https://rivet.dev/docs/connect/cloudflare-workers) | Docs-only | Medium | Depends on host-platform deploy tooling, not the actor runtime. |
| Deployment and Hosting | Deploy to PartyKit platform or own Cloudflare account with custom domains | partial | high | [Deploying to Cloudflare Workers](https://rivet.dev/docs/connect/cloudflare-workers) | [examples/cloudflare-workers/README.md](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/README.md) | Medium | Rivet deploys well to Cloudflare Workers, but is not PartyKit hosting. |
| CI/CD with GitHub Actions | Automated deployment via GitHub Actions with token-based authentication | pattern | high | [Versions](https://rivet.dev/docs/actors/versions) | Docs-only | Low | Standard CI/CD applies. |
| CLI Commands | Project management, deployment, environment variable, and AI commands | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | No PartyKit-style CLI surface exists. |
| Debugging | Console logging, VS Code breakpoints, DevTools, and live log tailing | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging), [Testing](https://rivet.dev/docs/actors/testing) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Low | Strong debugging exists, but tooling ergonomics differ. |
| Global Distribution and Architecture | Runs on Cloudflare workerd runtime with Durable Object-backed global distribution | partial | medium | [Scaling](https://rivet.dev/docs/actors/scaling), [Metadata](https://rivet.dev/docs/actors/metadata) | Docs-only | Medium | Similar actor distribution goals, different runtime/network model. |
| `Party.Request` | Cloudflare Worker `Request` wrapper with additional CF metadata | partial | medium | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [raw-http-request-properties.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http-request-properties.ts) | Medium | Standard `Request` is available, but PartyKit/Cloudflare-specific request extensions are not guaranteed. |
| `Party.FetchSocket` | WebSocket wrapper for non-Party endpoint connections in `onSocket` handler | partial | medium | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) | Medium | Low-level WebSockets cover most needs, but the exact wrapper type differs. |
| Compatibility and Runtime | Cloudflare Workers API compatibility date and flags configuration | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Docs-only | Low | Host-platform configuration issue, not actor feature parity. |

## High-Risk Behavioral Deltas

- **Storage needs an explicit redesign.** PartyKit's room storage is a single async storage surface; Rivet makes you choose between `state`, `kv`, and SQLite.
- **Client ergonomics differ.** `PartySocket` and `usePartySocket` do not map 1:1 to Rivet client connection semantics or buffering behavior.
- **Hibernation is usable but needs validation.** Especially for raw websocket-heavy apps, validate the exact wake/sleep behavior you rely on.
- **Connection tags are app-level data, not a built-in index.** If the PartyKit app depends on heavy tag-based fanout, implement and benchmark filtering logic explicitly.
- **PartyKit ecosystem packages do not carry over.** `y-partykit`, `partykit-ai`, static hosting, and preview tooling need their own migration choices.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Room identity maps cleanly to actor keys | One room ID consistently resolves to one actor instance | Pass: [actor-handle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-handle.ts) |
| Connection state and fanout work | Per-connection metadata and broadcast flows behave correctly | Pass: [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) |
| Raw websocket protocol paths work | Binary/text WS handling and path/query routing succeed | Pass: [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) |
| Room storage choice is explicit | Migration states whether each data set lives in `state`, `kv`, or SQLite | Gap: review [Persistence](https://rivet.dev/docs/actors/persistence) during migration design |
| Tag-based routing is benchmarked | Manual `connState` filtering meets fanout/latency needs | Gap: only [Connections](https://rivet.dev/docs/actors/connections) primitives are documented; add a benchmark/prototype for heavy fanout |
| PartySocket reconnect assumptions are validated | Client reconnect/buffer semantics are acceptable in the new client stack | Gap: compare against [JavaScript Client](https://rivet.dev/docs/clients/javascript) and [React](https://rivet.dev/docs/clients/react) in an app-specific spike |
| CRDT/Yjs migration path is chosen | Team either keeps existing CRDT layer or builds a new Rivet-backed transport | Gap: only [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) primitives exist; no first-class Yjs package equivalent is documented |

---

Comprehensive listing of every PartyKit feature, sourced directly from [docs.partykit.io](https://docs.partykit.io/). This document is intended for mapping each PartyKit feature to its Rivet Actor equivalent.

---

## Party Server Definition

**Docs:** [Party.Server API](https://docs.partykit.io/reference/partyserver-api/)

PartyKit servers are defined as TypeScript classes implementing the `Party.Server` interface. Each server receives a `Party.Room` instance in its constructor, which provides access to room state, storage, connections, and context.

```typescript
import type * as Party from "partykit/server";

export default class Server implements Party.Server {
  constructor(readonly room: Party.Room) {}
}
```

The `Party.Worker` interface documents static methods (edge handlers). The `satisfies` keyword is used for type-checking:

```typescript
Server satisfies Party.Worker;
```

**Key concepts:**
- Each server class is a "Party" -- a single Durable Object instance
- A "Room" is an instance of a party identified by a unique ID
- Multiple rooms can exist within the same party type
- Rooms are accessed at `/parties/:party/:room-id`
- The runtime is Cloudflare's `workerd`, supporting JavaScript, TypeScript, npm packages, and WebAssembly

---

## WebSocket Connections

**Docs:** [Party.Server API - onConnect](https://docs.partykit.io/reference/partyserver-api/)

### onConnect

Triggered when a WebSocket client connects. Receives the connection object and a `ConnectionContext` containing the initial HTTP request metadata.

```typescript
async onConnect(connection: Party.Connection, ctx: Party.ConnectionContext) {
  // ctx.request contains the initial HTTP request
}
```

### onMessage

Called when a WebSocket message is received. Messages can be `string` or `ArrayBufferLike`.

```typescript
onMessage(message: string | ArrayBufferLike, sender: Party.Connection) {
  // Process incoming message from sender
}
```

### onClose

Fires when a client closes their connection. By the time `onClose` is called, the connection is already closed and can no longer receive messages.

```typescript
onClose(connection: Party.Connection) {
  // Connection is already closed
}
```

### onError

Triggered when a connection encounters an error.

```typescript
onError(connection: Party.Connection, error: Error) {
  // Handle connection error
}
```

---

## WebSocket Messaging and Broadcasting

**Docs:** [Party.Server API - Room.broadcast](https://docs.partykit.io/reference/partyserver-api/)

### Broadcasting to All Connections

Send a message to all connected clients, with an optional exclusion list.

```typescript
this.room.broadcast(message, [sender.id]);
```

The second parameter is an array of connection IDs to exclude from the broadcast.

### Sending to Individual Connections

```typescript
connection.send("Good-bye!");
connection.close();
```

### Getting Connection Count

```typescript
const playerCount = [...this.room.getConnections()].length;
```

---

## Party.Connection Properties

**Docs:** [Party.Server API - Party.Connection](https://docs.partykit.io/reference/partyserver-api/)

Each WebSocket connection has the following properties and methods:

- `id` -- Unique connection identifier (auto-generated GUID or client-specified)
- `uri` -- Original connection request URI
- `state` -- Retrieves data stored via `setState()`
- `setState(data)` -- Stores up to 2KB of connection-specific data (not persisted across hibernation)
- `send(message)` -- Transmits a message to the client
- `close()` -- Closes the connection

```typescript
connection.setState({ username: "jani" }); // Max 2KB
const user = connection.state?.username;
connection.send("Hello!");
connection.close();
```

---

## Connection Tags (Metadata and Filtering)

**Docs:** [Party.Server API - getConnectionTags](https://docs.partykit.io/reference/partyserver-api/)

Attach metadata to connections via tags, then filter connections by tag. Tags are set during connection establishment and used for targeted messaging.

```typescript
import type * as Party from "partykit/server";

export default class Server implements Party.Server {
  getConnectionTags(
    connection: Party.Connection,
    ctx: Party.ConnectionContext
  ) {
    const country = (ctx.request.cf?.country as string) ?? "unknown";
    return [country];
  }

  async onMessage(message: string) {
    for (const british of this.room.getConnections("GB")) {
      british.send(`Pip-pip!`);
    }
  }
}
```

- `getConnectionTags` returns an array of string tags
- `room.getConnections(tag)` filters connections by a single tag
- `room.getConnections()` returns all connections (no filter)
- `room.getConnection(id)` retrieves a single connection by ID

---

## Room and Party Management

**Docs:** [Party.Server API - Party.Room](https://docs.partykit.io/reference/partyserver-api/), [Glossary](https://docs.partykit.io/glossary/)

### Party.Room Properties

- `room.id` -- Room identifier extracted from the Party URL path (`/parties/:name/:id`)
- `room.internalID` -- Platform-assigned internal identifier (use `id` instead)
- `room.env` -- Project environment variables
- `room.storage` -- Asynchronous key-value storage API
- `room.context.parties` -- Access to other parties in the project
- `room.context.ai` -- AI binding access
- `room.context.vectorize` -- Vectorize Index access

### On-Demand Room Creation

Parties are created dynamically based on unique identifiers. The same identifier always routes to the same server instance, while new identifiers trigger new instance creation with minimal startup time. There is no explicit "create room" API -- rooms are created on first access.

### Glossary

- **Party** -- A single server instance (one Durable Object)
- **PartyServer** -- The instance code definition (your class)
- **PartyWorker** -- Static code definition running in a separate edge worker
- **Room** -- An instance of a party identified by a unique ID
- **Server** -- Synonymous with "Party"
- **Durable Object** -- Cloudflare primitive: code running at the edge with persistent state

---

## State and Storage

**Docs:** [Persisting State into Storage](https://docs.partykit.io/guides/persisting-state-into-storage/), [Party.Server API](https://docs.partykit.io/reference/partyserver-api/)

PartyKit provides a transactional key-value Storage API for persisting room data. State survives server restarts caused by redeployment, hibernation, errors, or lifetime limits.

### Storage API Methods

**Reading data:**
```javascript
const data = await this.room.storage.get<OptionalTypeDefinition>("key");
```

**Writing data:**
```javascript
await this.room.storage.put("key", value);
```

**Deleting data:**
```javascript
await this.room.storage.delete("key");
```

**Listing all items:**
```javascript
const items = await this.room.storage.list();
for (const [key, value] of items) {
  console.log(key, value);
}
```

### Data Constraints

- **Keys:** Maximum 2,048 bytes (must be strings)
- **Individual values:** Up to 128 KiB (131,072 bytes)
- **Supported types:** Any format compatible with the structured clone algorithm
- **Total room capacity:** 128 MiB RAM

### Access Patterns

**Pattern 1 -- Load data upfront in onStart:**
Ideal for frequent reads and infrequent writes. Load all data into memory during initialization.

**Pattern 2 -- Read data when needed:**
Ideal for frequent writes or balanced read-write ratios. Query storage on-demand; the Storage API includes built-in caching for repeated key access.

### Key Sharding for Large Datasets

```javascript
// Store items individually
this.room.storage.put(`item:${event.id}`, event.data);

// Retrieve and update
const item = await this.room.storage.get(`item:${event.id}`);
```

---

## In-Memory State

**Docs:** [How PartyKit Works](https://docs.partykit.io/how-partykit-works/)

Unlike serverless functions, each Party maintains in-memory state between messages. The isolation of Durable Objects combined with ID-based routing enables developers to treat each Party as a single-tenant application. Instance variables on the server class persist across messages (but not across hibernation/restarts unless stored in `room.storage`).

```typescript
export default class Server implements Party.Server {
  messages: string[] = [];
  onMessage(message: string) {
    // keep track of messages in-memory
    this.messages.push(message);
    // send them to all connected clients
    this.room.broadcast(JSON.stringify({ messages: [message] }));
  }
  onConnect(connection: Party.Connection) {
    // when a new client connects, send them the full message history
    connection.send(JSON.stringify({ messages: this.messages }));
  }
}
```

---

## HTTP Request Handlers

**Docs:** [Responding to HTTP Requests](https://docs.partykit.io/guides/responding-to-http-requests/), [Party.Server API](https://docs.partykit.io/reference/partyserver-api/)

### onRequest

Handles HTTP requests to the room URL (`/parties/:party/:roomId`). Must return a Fetch API `Response`. Has full access to room resources including connected WebSocket clients.

```javascript
async onRequest(request: Party.Request) {
  if (request.method === "POST") {
    const payload = await request.json<{ message: string }>();
    this.messages.push(payload.message);
    this.room.broadcast(payload.message);
    return new Response("OK");
  }
  if (request.method === "GET") {
    return new Response(JSON.stringify(this.messages));
  }
  return new Response("Method not allowed", { status: 405 });
}
```

### Making HTTP Requests to a Room (Client-Side)

```javascript
// Manual URL construction
const protocol = PARTYKIT_HOST.startsWith("localhost") ? "http" : "https";
fetch(`${protocol}://${PARTYKIT_HOST}/parties/main/${roomId}`, {
  method: "POST",
  body: JSON.stringify({ message: "Hello!" })
});

// Using PartySocket utility
PartySocket.fetch(
  { host: PARTYKIT_HOST, room: roomId },
  { method: "POST", body: JSON.stringify({ message: "Hello!" }) }
);
```

### Key Advantage

The `onRequest` method has access to all of the room's resources, including connected WebSocket clients. This enables:
- Integration with third-party systems and webhooks
- Server-side rendering of room state
- Cross-party messaging via HTTP
- Push/pull pattern (HTTP write, WebSocket broadcast)

---

## Edge Middleware (Static Handlers)

**Docs:** [Party.Server API](https://docs.partykit.io/reference/partyserver-api/)

Static methods run in an edge worker near the user, not in the room. They do not have access to room resources like storage. Instead, they receive a `Party.Lobby` or `Party.FetchLobby` object.

### onBeforeConnect

Runs before any WebSocket connection is made to the party. Can modify the request or deny access by returning a Response.

```typescript
static async onBeforeConnect(
  req: Party.Request,
  lobby: Party.Lobby,
  ctx: Party.ExecutionContext
) {
  try {
    const token = new URL(req.url).searchParams.get("token") ?? "";
    const session = await verifyToken(token, { issuer });
    req.headers.set("X-User-ID", session.sub);
    return req; // Allow connection with modified request
  } catch (e) {
    return new Response("Unauthorized", { status: 401 });
  }
}
```

### onBeforeRequest

Runs before any HTTP request is made to the party. Same capabilities as `onBeforeConnect`.

```typescript
static async onBeforeRequest(
  req: Party.Request,
  lobby: Party.Lobby,
  ctx: Party.ExecutionContext
) {
  try {
    const token = req.headers.get("Authorization") ?? "";
    await verifyToken(token, { issuer });
    return req;
  } catch (e) {
    return new Response("Unauthorized", { status: 401 });
  }
}
```

### onFetch

Handles HTTP requests that do not match any Party URLs or static assets. Only one `onFetch` handler per project (defined in the main party; other parties' handlers are ignored).

```typescript
static async onFetch(
  req: Party.Request,
  lobby: Party.FetchLobby,
  ctx: Party.ExecutionContext
) {
  return new Response(req.url, { status: 403 });
}
Server satisfies Party.Worker;
```

### onSocket

Handles WebSocket connections outside Party URL patterns. Only one handler per project.

```typescript
static async onSocket(
  socket: Party.FetchSocket,
  lobby: Party.FetchLobby,
  ctx: Party.ExecutionContext
) {
  socket.send("Hello!");
}
Server satisfies Party.Worker;
```

### Lobby Objects

- `Party.Lobby` -- Available in `onBeforeConnect`/`onBeforeRequest` with `id`, `env`, `parties`, `ai`, `vectorize`
- `Party.FetchLobby` -- Available in `onFetch`/`onSocket` with `env`, `parties`, `ai`, `vectorize`

---

## Multi-Party Communication

**Docs:** [Using Multiple Parties per Project](https://docs.partykit.io/guides/using-multiple-parties-per-project/)

### Configuration

Define multiple parties in `partykit.json`:

```json
{
  "name": "multiparty",
  "main": "src/server.ts",
  "parties": {
    "user": "src/user.ts",
    "connections": "src/connections.ts"
  }
}
```

### Client Connection to Specific Party

```javascript
const partySocket = new PartySocket({
  host: PARTYKIT_HOST,
  room: "room-id",
  party: "connections"
});
```

### Inter-Party Communication (Server-to-Server)

Parties can access other parties through `this.room.context.parties`, enabling both HTTP requests and WebSocket connections between parties:

```javascript
const userParty = this.room.context.parties.user;
const userRoom = userParty.get(userId);

// HTTP request to another party's room
const res = await userRoom.fetch({ method: "GET" });

// WebSocket connection to another party's room
const socket = await userRoom.socket();
```

### Pattern: Connection Tracking

A dedicated `connections` party tracks active connections across rooms. Other parties notify this central tracker when connections open or close via POST requests, enabling system-wide connection monitoring.

---

## Cron Jobs / Scheduled Tasks

**Docs:** [Party.Server API - onCron](https://docs.partykit.io/reference/partyserver-api/)

### Configuration

Define cron schedules in `partykit.json`:

```json
{
  "crons": {
    "every-minute": "*/1 * * * *",
    "every-hour": "0 * * * *",
    "every-day": "0 0 * * *"
  }
}
```

### Handler

```typescript
import type * as Party from "partykit/server";

export default class Server implements Party.Server {
  static async onCron(
    cron: Party.Cron,
    lobby: Party.CronLobby,
    ctx: Party.ExecutionContext
  ) {
    console.log(`Running cron ${cron.name} at ${cron.scheduledTime}`);
  }
}
Server satisfies Party.Worker;
```

### Party.Cron Properties

- `name` -- The cron job name (key from the `crons` config)
- `definition` -- The cron expression string
- `scheduledTime` -- When the job was scheduled to run

### Party.CronLobby Properties

- `env` -- Environment variables
- `parties` -- Access to other parties
- `ai` -- AI binding access
- `vectorize` -- Vectorize Index access

### Local Testing

Test cron jobs locally by visiting: `http://localhost:1999/__scheduled__?cron=cron-name`

**Important:** `onCron` is a static method that runs on the edge worker, not inside a room. It does not have access to room storage or connections directly. To interact with rooms, use `lobby.parties` to make HTTP/WebSocket requests.

---

## Alarms (Room-Level Scheduled Tasks)

**Docs:** [Scheduling Tasks with Alarms](https://docs.partykit.io/guides/scheduling-tasks-with-alarms/)

Alarms are per-room scheduled tasks, distinct from project-level cron jobs. Only one alarm can be active per room at a time.

### Setting an Alarm

```javascript
this.room.storage.setAlarm(Date.now() + 5 * 60 * 1000); // 5 minutes from now
```

### Responding to Alarms

```javascript
onAlarm() {
  this.refreshDataFromExternalDatabase();
}
```

### Recurring Alarms

Reschedule within the callback:

```javascript
onAlarm() {
  // Do work...
  this.room.storage.setAlarm(Date.now() + 60 * 1000); // Reschedule in 1 minute
}
```

### Checking Existing Alarms

```javascript
const existingAlarm = await this.room.storage.getAlarm();
```

### Storage Expiration Pattern

```javascript
await this.room.storage.setAlarm(Date.now() + EXPIRY_PERIOD_MILLISECONDS);
```

### Limitations

- **One alarm per room maximum** -- Setting a new alarm cancels the previous one
- **`room.id` unavailable** -- The room identifier is not accessible in `onAlarm`. Workaround: store the ID in room storage during `onStart`
- **No inter-party context** -- `room.context.parties` is not available in alarm callbacks. Workaround: use public HTTP fetch requests
- Active rooms execute `onAlarm` immediately; dormant rooms are loaded into memory first (constructor + `onStart` run before `onAlarm`)

---

## Hibernation API

**Docs:** [Scaling PartyKit Servers with Hibernation](https://docs.partykit.io/guides/scaling-partykit-servers-with-hibernation/)

Hibernation allows the platform to remove the server from memory between messages while keeping WebSocket connections alive. This dramatically increases connection capacity.

### Configuration

```javascript
export default class Server implements Party.Server {
  options: Party.ServerOptions = {
    hibernate: true,
  };
}
```

### Connection Capacity

- **Without Hibernation:** Up to 100 connections per room
- **With Hibernation:** Up to 32,000 connections per room
- Practical maximum depends on memory usage (128 MB limit per party)

### How It Works

1. When no messages are being processed, the `Party.Server` instance is unloaded from memory
2. WebSocket connections are maintained by the platform
3. Upon receiving a message, a new server instance is created
4. Constructor and `onStart` callbacks execute before message handlers
5. This cycle can occur frequently during quiet periods

### Best Use Cases

- More than 100 simultaneous connected clients
- Infrequent writes (clients rarely send messages)
- HTTP-only writes with WebSockets for broadcasting
- Minimal in-memory state requirements

### Unsuitable Scenarios

- Message handling depends on expensive, hard-to-recreate state (external API calls)
- Using Yjs/y-partykit (currently unsupported with hibernation)

### Key Limitations

- Manual event handlers attached in `onConnect` do not persist after hibernation. Use `onMessage` and `onClose` class methods instead
- Local development does not simulate hibernation behavior

### Recommended Pattern: Partial State Loading

```javascript
async onMessage(websocketMessage: string) {
  const event = JSON.parse(websocketMessage);
  if (event.type === "update") {
    const item = await this.room.storage.get(`item:${event.id}`);
    const updated = { ...item, ...event.data };
    this.room.storage.put(`item:${event.id}`, updated);
  }
}
```

---

## Server Lifecycle (onStart)

**Docs:** [Party.Server API - onStart](https://docs.partykit.io/reference/partyserver-api/)

The `onStart` method executes when the server initializes or wakes from hibernation, before any `onConnect` or `onRequest` calls. Use it for asynchronous initialization such as loading data from storage.

```typescript
async onStart() {
  // Load data from storage, fetch external config, etc.
}
```

---

## Configuration (partykit.json)

**Docs:** [PartyKit Configuration](https://docs.partykit.io/reference/partykit-configuration/)

### Project Configuration

```json
{
  "name": "my-project",
  "main": "src/server.ts",
  "parties": {
    "other": "src/other.ts",
    "another": "src/another.ts"
  }
}
```

- `name` (string) -- Project identifier; generates URL: `https://<name>.<user>.partykit.dev`
- `main` (string) -- Entry point file for the default party
- `parties` (object) -- Map of party names to file paths for additional parties

### Static Asset Serving

```json
{
  "serve": {
    "path": "path/to/assets",
    "browserTTL": 31536000000,
    "edgeTTL": 31536000000,
    "singlePageApp": true,
    "exclude": ["**/*.map"],
    "include": ["**/*.map"],
    "build": {
      "entry": "path/to/entry.ts",
      "bundle": true,
      "splitting": true,
      "outdir": "path/to/outdir",
      "minify": true,
      "format": "esm",
      "sourcemap": true,
      "define": {"process.env.xyz": "123"},
      "external": ["react", "react-dom"],
      "loader": {".png": "file"}
    }
  }
}
```

### Cron Schedules

```json
{
  "crons": {
    "every-minute": "*/1 * * * *",
    "every-hour": "0 * * * *"
  }
}
```

### Dev Server

- `port` (number, default: 1999) -- Development server port
- `persist` (string | boolean, default: `.partykit/state`) -- Storage persistence path

### Build Configuration

```json
{
  "build": {
    "command": "npm run build",
    "watch": "src",
    "cwd": "."
  }
}
```

### Compilation

- `define` (object) -- Global constants substituted during compilation
- `minify` (boolean, default: true) -- JavaScript minification
- `compatibilityDate` (string) -- Cloudflare Workers API compatibility date
- `compatibilityFlags` (string[]) -- Additional Cloudflare Workers compatibility flags (e.g., `["web_socket_compression"]`)

### Vectorize Configuration

```json
{
  "vectorize": {
    "myIndex": {
      "index_name": "my-index"
    }
  }
}
```

---

## Client Library (PartySocket)

**Docs:** [PartySocket API](https://docs.partykit.io/reference/partysocket-api/)

### Installation

```
npm install partysocket@latest
```

### Basic Connection

```javascript
import PartySocket from "partysocket";

const ws = new PartySocket({
  host: "project-name.username.partykit.dev",
  room: "my-room",
  id: "some-connection-id",
  party: "main",
  query: async () => ({ token: await getAuthToken() })
});
```

### Key Features

- **WebSocket compatibility** -- Implements standard browser WebSocket interface (Level0 and Level2 event model)
- **Auto-reconnection** -- Automatically restores closed connections
- **Multi-platform** -- Works on Web, ServiceWorkers, Node.js, and React Native
- **Zero dependencies** -- No reliance on Window, DOM, or EventEmitter libraries
- **Message buffering** -- Queues messages until connection opens
- **Connection timeouts** -- Built-in timeout handling
- **Dynamic URLs** -- Change server addresses between reconnection attempts

### Update Connection Properties

```javascript
ws.updateProperties({
  host: "another-project.username.partykit.dev",
  room: "my-new-room"
});
ws.reconnect();
```

### Configuration Options

```javascript
type Options = {
  WebSocket?: any;                      // Custom WebSocket implementation
  maxReconnectionDelay?: number;        // Default: 10,000 ms
  minReconnectionDelay?: number;        // Default: 1,000 + Math.random() * 4,000 ms
  reconnectionDelayGrowFactor?: number; // Default: 1.3
  minUptime?: number;                   // Default: 5,000 ms
  connectionTimeout?: number;           // Default: 4,000 ms
  maxRetries?: number;                  // Default: Infinity
  maxEnqueuedMessages?: number;         // Default: Infinity
  startClosed?: boolean;                // Default: false
  debug?: boolean;                      // Default: false
  debugLogger?: (...args: any[]) => void;
};
```

### API Methods

| Method | Signature |
|--------|-----------|
| `close` | `close(code?: number, reason?: string)` |
| `reconnect` | `reconnect(code?: number, reason?: string)` |
| `send` | `send(data: string \| ArrayBuffer \| Blob \| ArrayBufferView)` |
| `addEventListener` | `addEventListener(type: 'open' \| 'close' \| 'message' \| 'error', listener: EventListener)` |
| `removeEventListener` | `removeEventListener(type: 'open' \| 'close' \| 'message' \| 'error', listener: EventListener)` |

### ReadyState Constants

| Constant | Value |
|----------|-------|
| CONNECTING | 0 |
| OPEN | 1 |
| CLOSING | 2 |
| CLOSED | 3 |

### Generic WebSocket Usage (Non-PartyKit Servers)

PartySocket can be used with any WebSocket server:

```javascript
import { WebSocket } from "partysocket";
const ws = new WebSocket("wss://my.site.com");
ws.addEventListener("open", () => ws.send("hello!"));
```

### Dynamic URL Provider

```javascript
const urls = ["wss://my.site.com", "wss://your.site.com", "wss://their.site.com"];
let urlIndex = 0;
const urlProvider = () => urls[urlIndex++ % urls.length];
const ws = new WebSocket(urlProvider);
```

### Async URL Provider

```javascript
const urlProvider = async () => {
  const token = await getSessionToken();
  return `wss://my.site.com/${token}`;
};
const ws = new WebSocket(urlProvider);
```

### Static Fetch Utility

```javascript
PartySocket.fetch(
  { host: PARTYKIT_HOST, room: roomId },
  { method: "POST", body: JSON.stringify({ message: "Hello!" }) }
);
```

---

## React Hook (usePartySocket)

**Docs:** [PartySocket API - React](https://docs.partykit.io/reference/partysocket-api/)

```javascript
import usePartySocket from "partysocket/react";

const Component = () => {
  const ws = usePartySocket({
    host: "project-name.username.partykit.dev",
    room: "my-room",
    onOpen() { console.log("connected"); },
    onMessage(e) { console.log("message", e.data); },
    onClose() { console.log("closed"); },
    onError(e) { console.log("error"); }
  });
};
```

The hook manages the WebSocket lifecycle automatically with React component mounting/unmounting.

---

## Y.js / CRDT Support (y-partykit)

**Docs:** [Y-PartyKit API](https://docs.partykit.io/reference/y-partykit-api/)

Y-PartyKit is an addon library enabling PartyKit to host backends for Yjs, a high-performance library of data structures for building collaborative software.

### Server Setup

```typescript
import type * as Party from "partykit/server";
import { onConnect } from "y-partykit";

export default class YjsServer implements Party.Server {
  constructor(public party: Party.Room) {}
  onConnect(conn: Party.Connection) {
    return onConnect(conn, this.party, { /* options */ });
  }
}
```

### Client Connection

```typescript
import YPartyKitProvider from "y-partykit/provider";
import * as Y from "yjs";

const yDoc = new Y.Doc();
const provider = new YPartyKitProvider(
  "localhost:1999",
  "my-document-name",
  yDoc
);
```

### Provider Options

- `connect: false` -- Delay connection until explicitly triggered
- `awareness` -- Custom Yjs awareness instance
- `params` -- Async function returning query parameters (e.g., for auth tokens)

### React Integration

```typescript
import useYProvider from "y-partykit/react";

const provider = useYProvider({
  host: "localhost:1999",
  room: "my-document-name",
  doc: yDoc
});
```

### Persistence Modes

**Snapshot mode (recommended):**
```typescript
persist: { mode: "snapshot" }
```
Stores the latest document state. Merges updates when sessions end.

**History mode (advanced):**
```typescript
persist: { mode: "history", maxBytes: 10_000_000, maxUpdates: 10_000 }
```
Maintains full edit history with configurable size limits.

### Additional Server Options

- `readOnly: true` -- Disables editing
- `load()` -- Async function to load documents from external storage
- `callback.handler()` -- Async function called periodically after edits
- `debounceWait: 2000` -- Delay before invoking handler
- `debounceMaxWait: 10000` -- Maximum interval between handler calls

---

## AI Integration (partykit-ai)

**Docs:** [PartyKit AI](https://docs.partykit.io/reference/partykit-ai/)

PartyKit AI integrates Cloudflare AI for model access and Cloudflare Vectorize for vector database functionality. Currently in Open Beta.

### Installation and Setup

```
npm install partykit-ai
```

```javascript
import { Ai } from "partykit-ai";
const ai = new Ai(room.context.ai);
```

Works within `Party.Server` classes and static handlers like `onFetch`, `onSocket`, and `onCron`.

### Text Generation Example

```javascript
static async onFetch(request, lobby) {
  const ai = new Ai(lobby.ai);
  const result = await ai.run("@cf/meta/llama-2-7b-chat-int8", {
    messages: [
      { role: "system", content: "You are a friendly assistant" },
      { role: "user", content: "What is Hello, World?" }
    ],
    stream: true
  });
  return new Response(result, {
    headers: { "content-type": "text/event-stream" }
  });
}
```

### Available Model Types

- Text generation (e.g., Llama 2)
- Image generation and processing
- Translation services
- Text-to-speech conversion

List all models: `npx partykit ai models`

### Vectorize (Vector Database)

**Configuration in partykit.json:**
```json
{
  "vectorize": {
    "myIndex": {
      "index_name": "my-index"
    }
  }
}
```

**Access:** `this.room.context.vectorize.myIndex`

**CLI Commands:**
```
npx partykit vectorize create my-index --dimensions <number> --metric <type>
npx partykit vectorize delete <name>
npx partykit vectorize get <name>
npx partykit vectorize list
npx partykit vectorize insert <name> --file <filename>
```

**Core API Methods:**

| Method | Purpose |
|--------|---------|
| `insert()` | Add vectors to index |
| `upsert()` | Insert or update vectors |
| `query()` | Search with filtering |
| `getByIds()` | Retrieve specific vectors |
| `deleteByIds()` | Remove vectors |
| `describe()` | Get index configuration |

**Vector Structure:**

```javascript
{
  id: "unique-identifier",
  values: [32.4, 6.55, 11.2],
  namespace: "optional-partition",
  metadata: { key: "value" }
}
```

**Query with Metadata Filtering:**

```javascript
await myIndex.query([1, 2, 3], {
  topK: 15,
  filter: { streaming_platform: "netflix" },
  returnMetadata: true
});
```

Supported filter operators: `$eq` (equals), `$ne` (not equals).

---

## Authentication

**Docs:** [Authentication Guide](https://docs.partykit.io/guides/authentication/)

### WebSocket Connection Authentication

Client passes token as query parameter:

```javascript
const partySocket = new PartySocket({
  host: PARTYKIT_HOST,
  room: "room-id",
  query: async () => ({
    token: await getToken()
  })
});
```

Server validates in `onBeforeConnect`:

```javascript
static async onBeforeConnect(request: Party.Request, lobby: Party.Lobby) {
  try {
    const issuer = lobby.env.CLERK_ENDPOINT || DEFAULT_CLERK_ENDPOINT;
    const token = new URL(request.url).searchParams.get("token") ?? "";
    const session = await verifyToken(token, { issuer });
    request.headers.set("X-User-ID", session.sub);
    return request;
  } catch (e) {
    return new Response("Unauthorized", { status: 401 });
  }
}
```

### HTTP Request Authentication

Client passes token via Authorization header:

```javascript
fetch(`https://${PARTYKIT_HOST}/party/${roomId}`, {
  headers: { Authorization: getToken() }
});
```

Server validates in `onBeforeRequest`:

```javascript
static async onBeforeRequest(request: Party.Request) {
  try {
    const token = request.headers.get("Authorization") ?? "";
    await verifyToken(token, { issuer });
    return request;
  } catch (e) {
    return new Response("Unauthorized", { status: 401 });
  }
}
```

### Supported Authentication Methods

- JWT verification (via `cloudflare-worker-jwt`)
- Server-to-server shared secrets via environment variables
- Session tokens verified against services (e.g., NextAuth.js, Clerk)

---

## Binary Message Handling

**Docs:** [Handling Binary Messages](https://docs.partykit.io/guides/handling-binary-messages/)

WebSocket messages can be `string` or `ArrayBufferLike`. Binary formats are more efficient for non-textual data (images, video, WASM).

### MessagePack Encoding (Server)

```javascript
import { pack, unpack } from "msgpackr";

class Server implements Party.Server {
  onConnect(connection: Party.Connection) {
    const data = { type: "join", id: connection.id };
    const message = pack(data);
    this.room.broadcast(message);
  }

  onMessage(message: string | ArrayBufferLike) {
    if (typeof message !== "string") {
      const data = unpack(message);
    }
  }
}
```

### MessagePack Decoding (Client)

```javascript
import { unpack } from "msgpackr/unpack";

socket.addEventListener("message", (event) => {
  const message = unpack(event.data);
});
```

### Mixed Encoding Strategy

```javascript
onMessage(message: string | ArrayBuffer) {
  const data = typeof message !== "string"
    ? unpack(message)
    : JSON.parse(message);
}
```

---

## Rate Limiting

**Docs:** [Rate Limiting Messages](https://docs.partykit.io/guides/rate-limiting-messages/)

PartyKit processes hundreds of messages per second from a single connection and thousands per room.

### Basic Rate Limiting via Connection State

```javascript
onMessage(message: string, sender: Party.Connection<{ lastMessageTime?: number }>) {
  const now = Date.now();
  const prev = sender.state?.lastMessageTime;
  if (prev && now < (prev + 1000)) {
    sender.close();
  } else {
    sender.setState({ lastMessageTime: now });
  }
}
```

### Incremental Back-Off Rate Limiter

```javascript
onMessage(message: string, sender: Party.Connection) {
  rateLimit(sender, 100, () => {
    // Process message (rate limited to every 100ms)
  });
}
```

### Strategies

- Disconnect misbehaving clients
- Shadow-banning (close connection without broadcasting their messages)
- Use `rate-limiter-flexible` npm package for advanced algorithms

---

## Input Validation

**Docs:** [Validating Client Inputs](https://docs.partykit.io/guides/validating-client-inputs/)

### Schema Definition with Zod

```javascript
const AddMessage = z.object({
  type: z.literal("add"),
  id: z.string(),
  item: z.string()
});
const RemoveMessage = z.object({
  type: z.literal("remove"),
  id: z.number()
});
const Message = z.union([AddMessage, RemoveMessage]);
```

### WebSocket Message Validation

```javascript
onMessage(message: string) {
  const result = Message.safeParse(JSON.parse(message));
  if (result.success === true) {
    const data = result.data;
    // Handle based on data.type
  }
}
```

### HTTP Request Validation

```javascript
async onRequest(req: Party.Request) {
  const body = await req.json();
  const result = Message.safeParse(body);
  if (result.success) {
    // Process valid data
  } else {
    return new Response(result.error.message, { status: 400 });
  }
}
```

---

## Static Asset Serving

**Docs:** [Serving Static Assets](https://docs.partykit.io/guides/serving-static-assets/)

### Basic Configuration

```json
{
  "serve": "path/to/assets"
}
```

### Advanced Configuration

```json
{
  "serve": {
    "path": "path/to/assets",
    "browserTTL": 31536000000,
    "edgeTTL": 31536000000,
    "singlePageApp": true,
    "exclude": ["**/*.map"],
    "include": ["**/*.map"]
  }
}
```

### Build Integration

```json
{
  "serve": {
    "path": "path/to/assets",
    "build": "path/to/entry.ts"
  }
}
```

PartyKit's bundler via `serve.build` automatically defines `PARTYKIT_HOST` correctly for both development and deployment environments.

---

## Environment Variables

**Docs:** [Managing Environment Variables](https://docs.partykit.io/guides/managing-environment-variables/)

### Method 1: Persistent Secrets

```
npx partykit env add API_KEY
npx partykit deploy
```

### Method 2: Per-Deployment Secrets

Via `.env` file:
```
npx partykit deploy --with-vars
```

Via command line:
```
npx partykit deploy --var API_KEY=$API_KEY --var HOST=$HOST
```

### Accessing Variables in Code

Variables are accessible via `room.env`:

```javascript
const apiKey = this.room.env.API_KEY;
```

Also available in lobby objects (`lobby.env`) in static handlers.

---

## Preview Environments

**Docs:** [Preview Environments](https://docs.partykit.io/guides/preview-environments/)

Deploy to custom preview URLs for testing:

```
partykit deploy --preview my-preview
```

Results in deployment to: `https://my-preview.my-project.alice.partykit.dev`

Delete previews:
```
partykit delete --preview my-preview
```

With custom domains:
```
partykit deploy --domain mydomain.com --preview my-preview
```
Results in: `https://my-preview.mydomain.com`

---

## Deployment and Hosting

**Docs:** [Deploying Your PartyKit Server](https://docs.partykit.io/guides/deploying-your-partykit-server/)

### Standard Deployment

```
npx partykit deploy
```

Deploys to `[project-name].[github-username].partykit.dev`. Domain provisioning takes up to two minutes.

### Cloud-Prem (Own Cloudflare Account)

```
CLOUDFLARE_ACCOUNT_ID=<id> CLOUDFLARE_API_TOKEN=<token> npx partykit deploy --domain partykit.domain.com
```

### Live Log Tailing

```
npx partykit tail
```

---

## CI/CD with GitHub Actions

**Docs:** [Setting Up CI/CD with GitHub Actions](https://docs.partykit.io/guides/setting-up-ci-cd-with-github-actions/)

### Generate Token

```
npx partykit@latest token generate
```

### GitHub Actions Workflow

```yaml
name: Deploy
on:
  push:
    branches:
      - main
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: actions/setup-node@v4
        with:
          node-version: 18
          cache: "npm"
      - run: npm ci
      - run: npx partykit deploy
        env:
          PARTYKIT_TOKEN: ${{ secrets.PARTYKIT_TOKEN }}
          PARTYKIT_LOGIN: ${{ secrets.PARTYKIT_LOGIN }}
```

---

## CLI Commands

**Docs:** [PartyKit CLI](https://docs.partykit.io/reference/partykit-cli/)

### Project Commands

| Command | Description |
|---------|-------------|
| `npx partykit init` | Add PartyKit to an existing npm project |
| `npx partykit dev` | Start local development server with file watching |
| `npx partykit dev src/server.ts` | Dev server with custom entry point |
| `npx partykit deploy` | Deploy code to PartyKit platform |
| `npx partykit deploy src/server.ts --name my-project` | Deploy with custom entry and name |
| `npx partykit tail` | Tail live logs for your project |
| `npx partykit list` | List all published projects |
| `npx partykit delete` | Remove published project |

### Environment Variable Commands

| Command | Description |
|---------|-------------|
| `npx partykit env list` | Display all configured env variable keys |
| `npx partykit env add <key>` | Create or update environment variable |
| `npx partykit env remove <key>` | Delete environment variable |
| `npx partykit env pull [filename]` | Export variables to JSON file |
| `npx partykit env push` | Upload variables from partykit.json |

### Authentication Commands

| Command | Description |
|---------|-------------|
| `npx partykit login` | Authenticate with PartyKit |
| `npx partykit logout` | Log out |
| `npx partykit whoami` | Display current user |
| `npx partykit token generate` | Create OAuth token for CI/CD |

### AI Commands

| Command | Description |
|---------|-------------|
| `npx partykit ai models` | List all available AI models |
| `npx partykit vectorize create <name> --dimensions <n> --metric <type>` | Create vector index |
| `npx partykit vectorize delete <name>` | Remove vector index |
| `npx partykit vectorize get <name>` | Get index details |
| `npx partykit vectorize list` | List all vector indexes |
| `npx partykit vectorize insert <name> --file <filename>` | Add vectors |

---

## Debugging

**Docs:** [Debugging Guide](https://docs.partykit.io/guides/debugging/)

PartyKit supports the same debugging techniques as Cloudflare Workers:

1. **Console Logging** -- `console.log()` output visible in dev server and via `npx partykit tail`
2. **VS Code Breakpoints** -- Set breakpoints in VS Code for local debugging
3. **JetBrains IDE Breakpoints** -- IntelliJ and other JetBrains IDEs supported
4. **DevTools** -- Browser DevTools for network inspection, CPU profiling, heap snapshots

---

## Global Distribution and Architecture

**Docs:** [How PartyKit Works](https://docs.partykit.io/how-partykit-works/)

- Runs on Cloudflare's `workerd` runtime (same as Cloudflare Workers)
- Each Party is backed by a Cloudflare Durable Object
- Distributed across Cloudflare's global edge network (within ~50ms of 95% of internet-connected population)
- Supports HTTP requests and WebSocket connections
- On-demand creation with minimal startup time
- Stateful (maintains in-memory state unlike serverless functions)
- Horizontally scalable across hundreds of data centers

---

## Party.Request

**Docs:** [Party.Server API](https://docs.partykit.io/reference/partyserver-api/)

`Party.Request` wraps the standard Cloudflare Worker `Request` object with additional CF metadata. It is used in `onRequest`, `onBeforeRequest`, `onBeforeConnect`, and `onFetch` handlers.

```typescript
async onRequest(req: Party.Request) {
  return new Response(req.cf.country, { status: 200 });
}
```

---

## Party.FetchSocket

**Docs:** [Party.Server API](https://docs.partykit.io/reference/partyserver-api/)

`Party.FetchSocket` is a WebSocket wrapper with a `request` property, available in the `onSocket` static handler for non-Party endpoint WebSocket connections.

---

## Compatibility and Runtime

**Docs:** [PartyKit Configuration](https://docs.partykit.io/reference/partykit-configuration/)

- `compatibilityDate` -- Controls which Cloudflare Workers API version is used
- `compatibilityFlags` -- Enable specific features like `web_socket_compression`
- Supports modern JavaScript, TypeScript, npm packages, and WebAssembly modules
- Node.js v17+ required for development tooling
