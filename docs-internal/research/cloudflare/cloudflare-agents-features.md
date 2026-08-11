# Cloudflare Agents: Comprehensive Feature Reference

> **As of:** 2026-03-06
>
> **Cloudflare basis:** Official Agents docs accessed 2026-03-06. Cloudflare publishes this surface as live docs rather than semver'd product docs.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Agent Class and Lifecycle | Base class, lifecycle hooks, and core properties for agent instances | native | high | [Lifecycle](https://rivet.dev/docs/actors/lifecycle) | [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts) | Low | `actor({...})` plus lifecycle hooks is a direct fit. |
| State Management | Persistent, synchronized, bidirectional state with validation support | pattern | medium | [State](https://rivet.dev/docs/actors/state), [Connections](https://rivet.dev/docs/actors/connections), [Realtime](https://rivet.dev/docs/actors/events) | [actor-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-state.ts), [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | High | Rivet does not expose Agent-style client `setState()` with automatic bidirectional sync. Model writes as actions and outbound sync as events. |
| SQL Database (Embedded SQLite) | Per-instance embedded SQLite with zero-latency local access | native | high | [SQLite](https://rivet.dev/docs/actors/sqlite) | [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) | Medium | Enable SQLite explicitly per actor with `db: db(...)`; it is not implicit on every actor. |
| Callable Methods (RPC) | Decorator-based RPC methods invocable over WebSocket from clients | native | high | [Actions](https://rivet.dev/docs/actors/actions) | [examples/cloudflare-workers/src/actors.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/actors.ts) | Low | Use actor `actions`; transport can be HTTP or `.connect()`. |
| WebSocket Support | Bidirectional real-time connections with broadcasting and connection tags | native | high | [Connections](https://rivet.dev/docs/actors/connections), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts), [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | Medium | Choose high-level `.connect()` for action/event style APIs and `onWebSocket` for raw protocol control. |
| Hibernation | Sleep when inactive, wake on messages while preserving WebSocket connections | partial | high | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler), [Limits](https://rivet.dev/docs/actors/limits) | [actor-conn-hibernation.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-hibernation.ts) | High | Rivet supports connection hibernation and documents hibernating WebSockets, but raw `onWebSocket` hibernation is marked experimental and should be validated per migration. |
| HTTP and Server-Sent Events (SSE) | HTTP request handling and server-to-client streaming via SSE | partial | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | High | HTTP is native. SSE/streaming responses are explicitly not supported today in the request-handler docs. |
| Routing | URL-based routing to agent instances with automatic name conversion | native | high | [Actor Keys](https://rivet.dev/docs/actors/keys), [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [examples/cloudflare-workers-hono/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers-hono/src/index.ts) | Low | Map route params to actor keys in your app router. |
| Scheduling | Delayed, cron, and interval task scheduling persisted to SQLite | partial | high | [Actor Scheduling](https://rivet.dev/docs/actors/schedule) | [actor-schedule.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-schedule.ts) | Medium | Delayed and absolute timers are native. Cron requires an app/platform pattern, not a built-in cron DSL. |
| Queue Tasks | Sequential FIFO task queue with retries stored in SQLite | partial | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Rivet queues are actor-local mailboxes. Retry and completion semantics exist, but the topology is not a shared managed queue service. |
| AI Chat Agent | Persistent chat with resumable streaming and tool support via AIChatAgent | pattern | medium | [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors), [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/ai/ai-agent.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/ai/ai-agent.ts) | High | Build the orchestration yourself. There is no first-class AIChatAgent equivalent. |
| Tool Use / Function Calling | Server-side and client-side tool execution with approval workflows | pattern | high | [Actions](https://rivet.dev/docs/actors/actions), [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/ai/my-tools.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/ai/my-tools.ts) | Medium | Server-side tools map cleanly to actions/workflow steps. Client approval flows are app-defined. |
| AI Model Integration | Multi-provider AI model access including Workers AI and external APIs | external | medium | [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/ai/ai-agent.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/ai/ai-agent.ts) | Medium | Rivet can orchestrate model calls, but model access is through external SDKs/providers. |
| Workflows | Durable multi-step background processing with retries and state updates | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Medium | Rivet has a native workflow engine, but the API surface differs from Cloudflare Agents. |
| Human-in-the-Loop | Workflow approval and MCP elicitation for pausing on human input | pattern | high | [Workflows](https://rivet.dev/docs/actors/workflows), [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [examples/sandbox/src/actors/workflow/approval.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/approval.ts) | Medium | Model this as workflow waits on queue messages or HTTP/action resumes. |
| Client SDK | React hooks, vanilla JS client, and HTTP fetch for agent connectivity | native | high | [JavaScript Client](https://rivet.dev/docs/clients/javascript), [React](https://rivet.dev/docs/clients/react) | [examples/cloudflare-workers/scripts/client.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/scripts/client.ts) | Low | Rivet has first-party JS and React clients. |
| Email Routing | Inbound email processing with routing, replying, and forwarding | external | low | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | Gap | High | Use an external email ingress/provider and forward into Rivet via HTTP or queues. |
| Browse the Web (Headless Browser) | Web browsing via Puppeteer Browser Rendering API or Browserbase | external | low | [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors) | Gap | High | Rivet Actors can orchestrate a browser worker, but do not provide a browser runtime. |
| Model Context Protocol (MCP) | Open standard for connecting AI systems to external tool servers | external | low | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | Gap | Medium | You can host/proxy MCP endpoints yourself, but there is no first-class MCP runtime in Rivet docs. |
| Protocol Messages | Automatic JSON frames sent on WebSocket connect for identity and state | partial | medium | [Connections](https://rivet.dev/docs/actors/connections), [Realtime](https://rivet.dev/docs/actors/events) | [actor-conn-state.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-state.ts) | Medium | Rivet has its own client protocol; do not assume Cloudflare Agents frame shapes. |
| Observability | Structured event emission for logging, monitoring, and remote services | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Low | Rivet has inspector and structured logs, but not the same managed observability surface. |
| Webhooks | Inbound webhook routing with signature verification and deduplication | pattern | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [raw-http-request-properties.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http-request-properties.ts) | Medium | Verify signatures and idempotency in app code. |
| Configuration (Wrangler) | Wrangler-based config for bindings, migrations, secrets, and environments | pattern | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [Deploying to Cloudflare Workers](https://rivet.dev/docs/connect/cloudflare-workers) | [examples/cloudflare-workers/wrangler.json](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/wrangler.json) | Low | When deploying on Cloudflare Workers, Wrangler still exists, but it configures the Rivet Cloudflare driver rather than Agents SDK bindings. |
| x402 Payment Protocol | HTTP 402-based payment middleware for paywalled APIs and MCP tools | external | low | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | Gap | Medium | Implement at your API edge or gateway; not a Rivet Actor runtime feature. |
| Agent Patterns | Prompt chaining, routing, parallelization, and orchestrator-worker patterns | pattern | high | [Design Patterns](https://rivet.dev/docs/actors/design-patterns), [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/workflow/order.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/order.ts) | Low | Rivet patterns are expressive enough, but application structure must be redesigned rather than mechanically ported. |
| Platform Limits | Account-level constraints on agents, storage, compute, and connections | partial | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | Medium | Limits are different enough that migrations need a fresh capacity review. |
| Deployment | CLI-based deploy with custom domains, preview deployments, and rollbacks | partial | high | [Deploying to Cloudflare Workers](https://rivet.dev/docs/connect/cloudflare-workers) | [examples/cloudflare-workers/README.md](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/README.md) | Medium | Rivet supports Cloudflare Workers deployment, but preview/rollback ergonomics depend on the host platform, not Rivet itself. |
| Getting Started (Quick Start) | Starter template setup with create-cloudflare CLI | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/cloudflare-workers) | Low | Rivet has a direct Cloudflare Workers quickstart. |

## High-Risk Behavioral Deltas

- **State sync is not automatic.** Cloudflare Agents expose a built-in synchronized state plane; Rivet expects explicit action contracts for writes and explicit event/broadcast contracts for fanout.
- **Hibernation support exists, but the exact surface is different.** Rivet documents hibernating connections and hibernating WebSockets, but low-level `onWebSocket` hibernation is still marked experimental. Validate any migration that depends on Cloudflare's raw socket wakeup semantics.
- **Queues are local mailboxes, not a managed global queue product.** Order and retry semantics are per actor instance. If the Cloudflare design relied on a shared FIFO across many workers, redesign around sharded actors or use an external broker.
- **SSE is currently a hard gap.** The request-handler docs explicitly say streaming responses and SSE are not supported at the moment.
- **AI platform integrations are orchestration work, not runtime primitives.** Rivet can host the orchestration layer, but Workers AI, email routing, browser rendering, MCP, and x402 remain external integrations.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Lifecycle hooks survive sleep/wake | `onCreate`/`onWake`/`onSleep` semantics hold across actor restarts | Pass: [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts), [actor-sleep.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-sleep.ts) |
| Embedded per-instance SQLite works | CRUD, transactions, and persistence across sleep succeed | Pass: [actor-db.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-db.ts) |
| Connection hibernation preserves logical session state | Connection state survives sleep without extra connect/disconnect hooks | Pass: [actor-conn-hibernation.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-conn-hibernation.ts) |
| Raw HTTP routing works | Actor `onRequest` handles custom paths and bodies | Pass: [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) |
| Raw WebSocket routing works | Actor `onWebSocket` handles messages and path/query metadata | Pass: [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) |
| Queue retries behave acceptably for migrated task flows | Uncompleted messages retry and wait-completion paths return durable status | Pass: [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) |
| SSE replacement plan is explicit | Migration either avoids SSE or documents alternate client transport | Fail: [request-handler.mdx](https://github.com/rivet-dev/rivet/blob/ba46891b1/website/src/content/docs/actors/request-handler.mdx) documents SSE as unsupported |
| AIChatAgent parity is proven in app code | Tool loops, resumable chat state, and provider streaming are demonstrated in a migration spike | Gap: only orchestration examples exist today in [examples/sandbox/src/actors/ai](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/sandbox/src/actors/ai) |

---

This document catalogs every feature of Cloudflare Agents with descriptions, documentation links, and code snippets taken directly from the official documentation. Its purpose is to enable mapping each Cloudflare feature to its Rivet Actor equivalent.

---

## Agent Class and Lifecycle

**Docs:** https://developers.cloudflare.com/agents/api-reference/agents-api/

Agents extend a base `Agent` class. Each agent instance is a globally unique Durable Object with its own SQL database, WebSocket connections, and scheduling capabilities. You can have millions of instances, each operating independently for horizontal scaling without centralized session stores.

**Agent definition:**

```typescript
import { Agent } from "agents";

class MyAgent extends Agent<Env, State> {
  // Your agent logic
}

export default MyAgent;
```

**Lifecycle methods:**

| Method | Purpose |
|--------|---------|
| `onStart(props?)` | Runs when instance starts or wakes from hibernation |
| `onRequest(request)` | Handles each HTTP request |
| `onConnect(connection, ctx)` | Triggered on WebSocket connection |
| `onMessage(connection, message)` | Processes WebSocket messages |
| `onError(connection, error)` | Handles WebSocket errors |
| `onClose(connection, code, reason, wasClean)` | Called on connection closure |
| `onEmail(email)` | Routes incoming emails |
| `onStateChanged(state, source)` | Detects state modifications |

**Core properties:**

- `this.env` - Environment variables and bindings
- `this.ctx` - Execution context
- `this.state` - Current persisted state
- `this.sql` - SQL query function

**Basic counter agent example:**

```typescript
import { Agent, callable } from "agents";

export class CounterAgent extends Agent<Env, { count: number }> {
  initialState = { count: 0 };

  @callable()
  increment() {
    this.setState({ count: this.state.count + 1 });
    return this.state.count;
  }
}
```

---

## State Management

**Docs:** https://developers.cloudflare.com/agents/api-reference/store-and-sync-state/

Each agent instance has built-in state that is persistent (automatically saved to SQLite, survives restarts and hibernation), synchronized (changes broadcast to all connected WebSocket clients instantly), bidirectional (both server and clients can update state), and type-safe (full TypeScript support with generics).

**Defining initial state:**

```typescript
export class ChatAgent extends Agent {
  initialState = {
    messages: [],
    settings: { theme: "dark", notifications: true },
    lastActive: null,
  };
}
```

**Updating state:**

```typescript
// Replace entire state
this.setState({
  players: ["Alice", "Bob"],
  score: 0,
  status: "playing",
});

// Update specific fields
this.setState({
  ...this.state,
  score: this.state.score + 10,
});
```

State must be JSON-serializable (plain objects, arrays, primitives). Functions, classes, Dates (use ISO strings), Maps, Sets, and circular references are not supported.

**Responding to state changes:**

```typescript
onStateChanged(state: GameState, source: Connection | "server") {
  console.log("State updated:", state);
  console.log("Updated by:", source === "server" ? "server" : source.id);
}
```

The `source` parameter indicates the update origin: `"server"` means the agent called `setState()`, and a `Connection` means a client pushed state via WebSocket.

**Validating state changes before persistence:**

```typescript
validateStateChange(nextState: GameState, source: Connection | "server") {
  if (nextState.score < 0) {
    throw new Error("score cannot be negative");
  }

  if (this.state.status === "finished" && nextState.status !== "finished") {
    throw new Error("Cannot restart a finished game");
  }
}
```

Throwing an error from `validateStateChange` aborts the update before it is persisted or broadcast.

**Client-side state (React):**

```typescript
function GameUI() {
  const agent = useAgent({
    agent: "game-agent",
    name: "room-123",
    onStateChanged: (state, source) => {
      console.log("State updated:", state);
    }
  });

  const addPlayer = (name: string) => {
    agent.setState({
      ...agent.state,
      players: [...agent.state.players, name]
    });
  };

  return <div>Players: {agent.state?.players.join(", ")}</div>;
}
```

**Client-side state (vanilla JavaScript):**

```javascript
import { AgentClient } from "agents/client";

const client = new AgentClient({
  agent: "game-agent",
  name: "room-123",
  onStateChanged: (state) => {
    document.getElementById("score").textContent = state.score;
  },
});

client.setState({ ...client.state, score: 100 });
```

**Best practices -- keep state small, use SQL for large data:**

```typescript
// Bad - large arrays in state
initialState = { allMessages: [] };

// Good - light state with SQL queries
initialState = { messageCount: 0, lastMessageId: null };
async getMessages(limit = 50) {
  return this.sql`SELECT * FROM messages ORDER BY created_at DESC LIMIT ${limit}`;
}
```

---

## SQL Database (Embedded SQLite)

**Docs:** https://developers.cloudflare.com/agents/api-reference/store-and-sync-state/

Each agent instance has its own embedded SQLite database accessed via `this.sql` with tagged template literals. Queries execute with zero-latency local access (no network round-trips).

```typescript
// Create tables
this.sql`CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, name TEXT)`;

// Insert data
this.sql`INSERT INTO users (id, name) VALUES (${id}, ${name})`;

// Query data with type parameter
const users = this.sql<User>`SELECT * FROM users WHERE id = ${id}`;
```

**Typed results:**

```typescript
type User = {
  id: string;
  name: string;
  email: string;
};

const [user] = this.sql<User>`SELECT * FROM users WHERE id = ${userId}`;
```

Note: Type parameters do not validate results at runtime. Use libraries like Zod for validation.

**State vs SQL guidance:**

| Use State For | Use SQL For |
|---|---|
| UI state (loading, selected items) | Historical data |
| Real-time counters | Large collections |
| Active session data | Relationships |
| Configuration | Queryable data |

---

## Callable Methods (RPC)

**Docs:** https://developers.cloudflare.com/agents/api-reference/callable-methods/

Callable methods let clients invoke agent methods over WebSocket using RPC. Mark methods with `@callable()` to expose them to external clients like browsers, mobile apps, or other services.

```typescript
import { Agent, callable } from "agents";

export class MyAgent extends Agent {
  @callable()
  async greet(name: string): Promise<string> {
    return `Hello, ${name}!`;
  }
}
```

**Client invocation:**

```typescript
const result = await agent.stub.greet("World");
// "Hello, World!"
```

**When to use which approach:**

| Context | Approach |
|---------|----------|
| Browser/mobile calling agent | Use @callable() decorator |
| External service calling agent | Use @callable() decorator |
| Worker calling agent (same codebase) | Use Durable Object RPC directly |
| Agent-to-agent communication | Use Durable Object RPC via getAgentByName() |

Only JSON-serializable types work as arguments and return values (primitives, plain objects, arrays). Functions, Dates, Maps, and Sets cannot be serialized.

**Streaming callable methods:**

```typescript
@callable({ streaming: true })
async generateText(stream: StreamingResponse, prompt: string) {
  for await (const chunk of this.llm.stream(prompt)) {
    stream.send(chunk);
  }
  stream.end();
}
```

**Client-side streaming consumption:**

```typescript
await agent.call("generateText", [prompt], {
  stream: {
    onChunk: (chunk) => appendToOutput(chunk),
    onDone: (finalValue) => console.log("Complete", finalValue),
    onError: (error) => console.error("Error:", error),
  },
});
```

**TypeScript type safety:**

```typescript
const agent = useAgent<MyAgent>({
  agent: "MyAgent",
  name: "default",
});
```

Configuration note: Set `"target": "ES2021"` in tsconfig.json. Avoid `"experimentalDecorators": true` as it breaks TC39 standard decorators.

---

## WebSocket Support

**Docs:** https://developers.cloudflare.com/agents/api-reference/websockets/

Agents support WebSocket connections for real-time, bidirectional communication. Connections are automatically accepted unless explicitly rejected via `connection.close()`.

**Connection lifecycle hooks:**

| Hook | Trigger |
|------|---------|
| `onConnect(connection, ctx)` | New WebSocket connection established |
| `onMessage(connection, message)` | WebSocket message received |
| `onClose(connection, code, reason, wasClean)` | Connection closure |
| `onError(connection, error)` | WebSocket error occurrence |

**Connection object:**

| Property/Method | Type | Purpose |
|-----------------|------|---------|
| `id` | string | Unique connection identifier |
| `state` | State | Per-connection data storage |
| `setState(state)` | void | Update connection-specific state |
| `send(message)` | void | Send message to client |
| `close(code?, reason?)` | void | Terminate connection |

**Per-connection state persists across message exchanges and survives hibernation.**

**Broadcasting:**

```typescript
// Send to all connected clients
this.broadcast(JSON.stringify({ type: "update", data: someData }));

// Exclude specific connections
this.broadcast(message, [senderConnectionId]);
```

**Connection tags for filtered messaging:**

Override `getConnectionTags()` to assign metadata tags. Constraints: up to 9 tags per connection, maximum 256 characters each. Retrieve connections by tag using `getConnections(tag)`.

**Connection management methods:**

| Method | Signature | Purpose |
|--------|-----------|---------|
| `getConnections` | `(tag?: string) => Iterable<Connection>` | Retrieve all or filtered connections |
| `getConnection` | `(id: string) => Connection \| undefined` | Get specific connection by ID |
| `getConnectionTags` | `(connection, ctx) => string[]` | Assign tags to connection |
| `broadcast` | `(message, without?: string[]) => void` | Send to all/multiple connections |

**Binary data:** Messages can be strings or `ArrayBuffer`. Check type at runtime with `message instanceof ArrayBuffer`.

**Protocol messages:** Agents automatically send JSON text frames (`cf_agent_identity`, `cf_agent_state`, `cf_agent_mcp_servers`) to every connection. Suppress these for binary-only clients using `shouldSendProtocolMessages`.

---

## Hibernation

**Docs:** https://developers.cloudflare.com/agents/api-reference/websockets/

Agents can sleep when inactive and wake when messages arrive, conserving resources while maintaining open WebSocket connections (managed by Cloudflare).

**Configuration:**

```typescript
// Hibernation is enabled by default. Disable via:
static options = { hibernate: false };
```

**Proper state management across hibernation:**

```javascript
export class MyAgent extends Agent {
  initialState = { counter: 0 };

  // This will be lost after hibernation
  localCounter = 0;

  onMessage(connection, message) {
    // Persists across hibernation cycles
    this.setState({ counter: this.state.counter + 1 });

    // Lost after hibernation
    this.localCounter++;
  }
}
```

**What persists across hibernation:**
- `this.state` (agent state)
- `connection.state` (per-connection state)
- SQLite data (`this.sql`)
- Connection metadata

**What does NOT persist:**
- Class properties and in-memory variables
- Timers and intervals
- In-flight promises
- Local caches

Store critical data in `this.state` or SQLite, not in class properties.

---

## HTTP and Server-Sent Events (SSE)

**Docs:** https://developers.cloudflare.com/agents/api-reference/http-sse/

Agents handle HTTP requests via the `onRequest` method. This supports routing based on URL pathname, returning JSON responses, validating HTTP methods, and parsing request bodies.

**HTTP request handling:**

```typescript
import { Agent } from "agents";

export class APIAgent extends Agent {
  async onRequest(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname.endsWith("/status")) {
      return Response.json({ status: "ok", state: this.state });
    }

    if (url.pathname.endsWith("/action")) {
      if (request.method !== "POST") {
        return new Response("Method not allowed", { status: 405 });
      }
      const data = await request.json<{ action: string }>();
      await this.processAction(data.action);
      return Response.json({ success: true });
    }

    return new Response("Not found", { status: 404 });
  }

  async processAction(action: string) {
    // Handle the action
  }
}
```

**Manual SSE streaming:**

```typescript
export class StreamAgent extends Agent {
  async onRequest(request: Request): Promise<Response> {
    const encoder = new TextEncoder();

    const stream = new ReadableStream({
      async start(controller) {
        controller.enqueue(encoder.encode("data: Starting...\n\n"));

        for (let i = 1; i <= 5; i++) {
          await new Promise((r) => setTimeout(r, 500));
          controller.enqueue(encoder.encode(`data: Step ${i} complete\n\n`));
        }

        controller.enqueue(encoder.encode("data: Done!\n\n"));
        controller.close();
      },
    });

    return new Response(stream, {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      },
    });
  }
}
```

**SSE message format:**

```
data: your message here\n\n
```

Extended format with event types and IDs:

```
event: update\nid: 123\ndata: {"count": 42}\n\n
```

SSE streams are created using `ReadableStream` with headers: `Content-Type: text/event-stream`, `Cache-Control: no-cache`, `Connection: keep-alive`.

**WebSockets vs SSE comparison:**

| Aspect | WebSockets | SSE |
|--------|-----------|-----|
| Direction | Bi-directional | Server to Client only |
| Protocol | ws:// / wss:// | HTTP |
| Binary data | Supported | Text only |
| Reconnection | Manual | Automatic (browser) |
| Use case | Interactive apps, chat | Streaming responses, notifications |

---

## Routing

**Docs:** https://developers.cloudflare.com/agents/api-reference/agents-api/

Agents are accessed via URL pattern: `https://your-worker.workers.dev/agents/:agent-name/:instance-name`

Agent class names convert from camelCase to kebab-case in URLs: `ChatAgent` becomes `/agents/chat-agent/...`, `MyCustomAgent` becomes `/agents/my-custom-agent/...`.

**Use `routeAgentRequest()` for routing:**

```typescript
import { routeAgentRequest } from "agents";

export default {
  async fetch(request: Request, env: Env) {
    return (
      routeAgentRequest(request, env) ||
      new Response("Not found", { status: 404 })
    );
  },
} satisfies ExportedHandler<Env>;
```

Instance names default to `"default"` when omitted.

---

## Scheduling

**Docs:** https://developers.cloudflare.com/agents/api-reference/schedule-tasks/

The Agents SDK provides four scheduling modes. All scheduled tasks persist to SQLite and survive agent restarts.

**Delayed execution (seconds):**

```typescript
await this.schedule(60, "sendReminder", { message: "Check email" });
```

**Scheduled execution (specific date/time):**

```typescript
await this.schedule(new Date("2025-02-01T09:00:00Z"), "sendReminder", { message: "Monthly report due" });
```

**Recurring (cron expression):**

```typescript
await this.schedule("0 8 * * *", "dailyDigest", { type: "summary" });
```

**Fixed interval (seconds, sub-minute precision):**

```typescript
this.scheduleEvery(30, "poll", { source: "api" });
```

**Key methods:**

| Method | Description |
|--------|-------------|
| `schedule(when, callback, payload, options?)` | Creates a one-time or recurring task |
| `scheduleEvery(intervalSeconds, callback, payload, options?)` | Runs a task at fixed intervals |
| `getSchedule(id)` | Retrieves a single scheduled task by ID (synchronous) |
| `getSchedules(criteria?)` | Queries scheduled tasks with filters for type, id, or timeRange |
| `cancelSchedule(id)` | Removes a scheduled task; returns true if cancelled |

**Schedule object structure:**

- `id`: Unique identifier
- `callback`: Method name to invoke
- `payload`: JSON-serializable data
- `time`: Unix timestamp (seconds) of next execution
- `type`: `"scheduled"`, `"delayed"`, `"cron"`, or `"interval"`

**Error handling:** If a scheduled callback throws, the task fails for that execution but the schedule persists. For intervals, the next execution still occurs. Cron jobs reschedule for the next occurrence.

**Overlap prevention:** If an interval callback takes longer than the interval, the next execution is skipped with a warning logged.

**Limits:**
- Maximum tasks limited by SQLite storage (practical limit: tens of thousands per agent)
- Task size: up to 2MB per task including payload
- Minimum delay: 0 seconds
- Cron precision: minute-level only
- Interval precision: second-level

---

## Queue Tasks

**Docs:** https://developers.cloudflare.com/agents/api-reference/queue-tasks/

The Agents SDK includes an integrated queue system for asynchronous task execution. Tasks are stored in SQLite and processed sequentially in FIFO order.

**QueueItem type:**

```typescript
type QueueItem<T> = {
  id: string;           // Unique task identifier
  payload: T;           // Data passed to callback
  callback: keyof Agent; // Method name to invoke
  created_at: number;   // Task creation timestamp
};
```

**Core methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `queue()` | `async queue<T>(callback: keyof this, payload: T): Promise<string>` | Adds a task, returns task ID |
| `dequeue()` | `dequeue(id: string): void` | Removes a specific task by ID |
| `dequeueAll()` | `dequeueAll(): void` | Clears entire queue |
| `dequeueAllByCallback()` | `dequeueAllByCallback(callback: string): void` | Removes all tasks for a callback |
| `getQueue()` | `getQueue<T>(id: string): QueueItem<T> \| undefined` | Retrieves a single task by ID |
| `getQueues()` | `getQueues<T>(key: string, value: string): QueueItem<T>[]` | Retrieves tasks by payload key-value pair |

**Processing workflow:**
1. Validates callback method exists on agent
2. System attempts queue flush post-queueing
3. Processes in FIFO order by creation timestamp
4. Successfully executed tasks auto-remove
5. Errors are logged; tasks with missing callbacks are skipped
6. Stored in `cf_agents_queues` table; survives restarts

**Callback method signature:**

```typescript
async callbackMethod(payload: unknown, queueItem: QueueItem): Promise<void>
```

**Full usage example:**

```typescript
class MyAgent extends Agent {
  async processEmail(data: { email: string; subject: string }) {
    console.log(`Processing email: ${data.subject}`);
  }

  async onMessage(message: string) {
    const taskId = await this.queue("processEmail", {
      email: "user@example.com",
      subject: "Welcome!",
    });
    console.log(`Queued task with ID: ${taskId}`);
  }
}
```

**Batch operations:**

```typescript
class BatchProcessor extends Agent {
  async processBatch(data: { items: any[]; batchId: string }) {
    for (const item of data.items) {
      await this.processItem(item);
    }
    console.log(`Completed batch ${data.batchId}`);
  }

  async onLargeRequest(items: any[]) {
    const batchSize = 10;
    for (let i = 0; i < items.length; i += batchSize) {
      const batch = items.slice(i, i + batchSize);
      await this.queue("processBatch", {
        items: batch,
        batchId: `batch-${i / batchSize + 1}`,
      });
    }
  }
}
```

**Built-in retries:** Pass retry config as third argument: `{ retry: { maxAttempts, baseDelayMs, maxDelayMs } }`

**Limitations:**
- Tasks are processed sequentially, not in parallel
- FIFO only (no priority system)
- Queue processing happens during agent execution, not as separate background jobs

---

## AI Chat Agent

**Docs:** https://developers.cloudflare.com/agents/api-reference/chat-agent/

`AIChatAgent` provides automatic message persistence, resumable streaming, and tool support. Built on AI SDK and Durable Objects.

**Installation:**

```
npm install @cloudflare/ai-chat agents ai
```

**Server implementation:**

```typescript
import { AIChatAgent } from "@cloudflare/ai-chat";
import { createWorkersAI } from "workers-ai-provider";
import { streamText, convertToModelMessages } from "ai";

export class ChatAgent extends AIChatAgent {
  async onChatMessage() {
    const workersai = createWorkersAI({ binding: this.env.AI });
    const result = streamText({
      model: workersai("@cf/zai-org/glm-4.7-flash"),
      system: "You are a helpful assistant.",
      messages: await convertToModelMessages(this.messages),
    });
    return result.toUIMessageStreamResponse();
  }
}
```

**Client implementation (React):**

```typescript
function Chat() {
  const agent = useAgent({ agent: "ChatAgent" });
  const { messages, sendMessage, status } = useAgentChat({ agent });

  return (
    <div>
      {messages.map((msg) => (
        <div key={msg.id}>
          <strong>{msg.role}:</strong>
          {msg.parts.map((part, i) =>
            part.type === "text" ? <span key={i}>{part.text}</span> : null,
          )}
        </div>
      ))}
      <form onSubmit={(e) => {
        e.preventDefault();
        const input = e.currentTarget.elements.namedItem("input");
        sendMessage({ text: input.value });
        input.value = "";
      }}>
        <input name="input" placeholder="Type a message..." />
        <button type="submit" disabled={status === "streaming"}>
          Send
        </button>
      </form>
    </div>
  );
}
```

**Key features:**
- Messages automatically persist to SQLite and survive restarts
- Resumable streaming: disconnected clients resume mid-stream without data loss
- `this.messages` contains the full conversation history
- Status values: `"idle"`, `"submitted"`, `"streaming"`, `"error"`

**useAgentChat options:**

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| agent | ReturnType<typeof useAgent> | Required | Agent connection |
| onToolCall | callback | -- | Handle client-side tool execution |
| autoContinueAfterToolResult | boolean | true | Auto-continue after tool results |
| resume | boolean | true | Enable stream resumption |
| body | object or function | -- | Custom data sent with requests |

**useAgentChat return values:**
- `messages`: UIMessage[] (current conversation)
- `sendMessage`: send a message
- `clearHistory`: clear conversation
- `addToolOutput`: provide client-side tool output
- `addToolApprovalResponse`: approve/reject tools
- `setMessages`: set messages directly
- `status`: current status string

---

## Tool Use / Function Calling

**Docs:** https://developers.cloudflare.com/agents/api-reference/chat-agent/ and https://developers.cloudflare.com/agents/concepts/tools/

Tools enable AI systems to interact with external services and perform actions through structured APIs.

**Server-side tools (automatic execution):**

```typescript
tools: {
  getWeather: tool({
    description: "Get weather for a city",
    inputSchema: z.object({ city: z.string() }),
    execute: async ({ city }) => {
      const data = await fetchWeather(city);
      return { temperature: data.temp, condition: data.condition };
    },
  }),
}
```

**Client-side tools (via onToolCall callback):**

```typescript
const { messages, sendMessage } = useAgentChat({
  agent,
  onToolCall: async ({ toolCall, addToolOutput }) => {
    if (toolCall.toolName === "getLocation") {
      const pos = await new Promise((resolve, reject) =>
        navigator.geolocation.getCurrentPosition(resolve, reject),
      );
      addToolOutput({
        toolCallId: toolCall.toolCallId,
        output: { lat: pos.coords.latitude, lng: pos.coords.longitude },
      });
    }
  },
});
```

**Tool approval (human-in-the-loop):**

```typescript
tools: {
  processPayment: tool({
    description: "Process a payment",
    inputSchema: z.object({
      amount: z.number(),
      recipient: z.string(),
    }),
    needsApproval: async ({ amount }) => amount > 100,
    execute: async ({ amount, recipient }) => charge(amount, recipient),
  }),
}
```

---

## AI Model Integration

**Docs:** https://developers.cloudflare.com/agents/api-reference/using-ai-models/

Agents can use AI models from multiple providers. Workers AI is built-in without requiring API keys. OpenAI, Anthropic, Google Gemini, and any OpenAI-compatible service are also supported.

**Workers AI configuration:**

```jsonc
{
  "ai": { "binding": "AI" }
}
```

**Using Workers AI with the AI SDK:**

```
npm i ai workers-ai-provider
```

The AI SDK enables swapping providers seamlessly with identical code structure using `generateText()` or `streamText()`.

**Key capability:** Agents operate autonomously and handle extended responses lasting minutes or longer. If clients disconnect mid-stream, agents continue processing and can update reconnecting clients.

**AI Gateway:** Enables routing requests across providers based on availability, rate limits, or budgets via gateway configuration.

**Combining state and SQL with AI:**

```typescript
export class ReasoningAgent extends Agent<Env> {
  async callReasoningModel(prompt: Prompt) {
    let result = this.sql<History>`
      SELECT * FROM history
      WHERE user = ${prompt.userId}
      ORDER BY timestamp DESC LIMIT 1000
    `;

    let context = [];
    for (const row of result) {
      context.push(row.entry);
    }

    const systemPrompt = prompt.system || "You are a helpful assistant.";
    const userPrompt = `${prompt.user}\n\nUser history:\n${context.join("\n")}`;

    try {
      const response = await this.env.AI.run("@cf/zai-org/glm-4.7-flash", {
        messages: [
          { role: "system", content: systemPrompt },
          { role: "user", content: userPrompt },
        ],
      });

      this.sql`
        INSERT INTO history (timestamp, user, entry)
        VALUES (${new Date()}, ${prompt.userId}, ${response.response})
      `;

      return response.response;
    } catch (error) {
      console.error("Error calling reasoning model:", error);
      throw error;
    }
  }
}
```

---

## Workflows

**Docs:** https://developers.cloudflare.com/agents/api-reference/run-workflows/

Workflows integrate with agents for durable, multi-step background processing. Agents handle real-time communication and state management while workflows handle durable execution with automatic retries and failure recovery. Use them together for long-running tasks exceeding 30 seconds.

**AgentWorkflow class properties:**
- `agent`: Typed stub for calling Agent methods
- `instanceId`: Workflow instance ID
- `workflowName`: Workflow binding name
- `env`: Environment bindings

**Non-durable methods (may repeat on retry):**
- `reportProgress(progress)`: Report progress triggering `onWorkflowProgress`
- `broadcastToClients(message)`: Broadcast to WebSocket clients connected to the Agent
- `waitForApproval(step, options?)`: Wait for approval event

**Durable step methods (idempotent):**
- `step.reportComplete(result?)`: Report successful completion
- `step.reportError(error)`: Report an error
- `step.sendEvent(event)`: Send custom event to Agent
- `step.updateAgentState(state)`: Replace Agent state with broadcasts
- `step.mergeAgentState(partial)`: Merge into existing Agent state
- `step.resetAgentState()`: Reset to initialState

**Workflow state updates from within workflows:**

```typescript
class MyWorkflow extends Workflow<Env> {
  async run(event: AgentWorkflowEvent, step: AgentWorkflowStep) {
    // Replace entire state
    await step.updateAgentState({ status: "processing", progress: 0 });

    // Merge partial updates
    await step.mergeAgentState({ progress: 50 });

    // Reset to initialState
    await step.resetAgentState();

    return result;
  }
}
```

**Agent workflow management methods:**

| Method | Description |
|--------|-------------|
| `runWorkflow(workflowName, params, options?)` | Start and track workflow, returns instance ID |
| `sendWorkflowEvent(workflowName, instanceId, event)` | Send event to running workflow |
| `getWorkflowStatus(workflowName, instanceId)` | Get workflow status |
| `getWorkflow(instanceId)` | Retrieve tracked workflow by ID |
| `getWorkflows(criteria?)` | Query with cursor-based pagination |
| `terminateWorkflow(instanceId)` | Terminate immediately |
| `pauseWorkflow(instanceId)` | Pause for later resumption |
| `resumeWorkflow(instanceId)` | Resume paused workflow |
| `restartWorkflow(instanceId, options?)` | Restart from beginning |
| `approveWorkflow(instanceId, options?)` | Approve waiting workflow |
| `rejectWorkflow(instanceId, options?)` | Reject, triggering WorkflowRejectedError |
| `deleteWorkflow(instanceId)` | Delete tracking record |
| `deleteWorkflows(criteria?)` | Delete matching records |
| `migrateWorkflowBinding(oldName, newName)` | Migrate after renaming |

**Lifecycle callbacks (override on Agent):**
- `onWorkflowProgress`: Called when workflow reports progress
- `onWorkflowComplete`: Called when workflow completes
- `onWorkflowError`: Called when workflow errors
- `onWorkflowEvent`: Called when workflow sends event

**Workflow status values:** queued, running, paused, waiting, complete, errored, terminated

**Constraints:**
- Maximum 1,024 steps per workflow
- 10 MB state size limit
- 1-year maximum event wait time
- 30-minute maximum per step execution
- Workflows cannot open WebSocket connections directly

---

## Human-in-the-Loop

**Docs:** https://developers.cloudflare.com/agents/guides/human-in-the-loop/

Two primary patterns for pausing agent execution and awaiting human input.

### Workflow Approval

Uses `waitForApproval()` in Cloudflare Workflows. Workflows pause until human approval or rejection. Supports configurable timeouts ("7 days", "1 hour", "30 minutes").

**Workflow with approval step:**

```typescript
import { Agent } from "agents";
import { AgentWorkflow } from "agents/workflows";
import type { AgentWorkflowEvent, AgentWorkflowStep } from "agents/workflows";

type ExpenseParams = {
  amount: number;
  description: string;
  requestedBy: string;
};

export class ExpenseWorkflow extends AgentWorkflow<
  ExpenseAgent,
  ExpenseParams
> {
  async run(event: AgentWorkflowEvent<ExpenseParams>, step: AgentWorkflowStep) {
    const expense = event.payload;

    const validated = await step.do("validate", async () => {
      if (expense.amount <= 0) {
        throw new Error("Invalid expense amount");
      }
      return { ...expense, validatedAt: Date.now() };
    });

    await this.reportProgress({
      step: "approval",
      status: "pending",
      message: `Awaiting approval for $${expense.amount}`,
    });

    const approval = await this.waitForApproval<{ approvedBy: string }>(step, {
      timeout: "7 days",
    });

    console.log(`Approved by: ${approval?.approvedBy}`);

    const result = await step.do("process", async () => {
      return { expenseId: crypto.randomUUID(), ...validated };
    });

    await step.reportComplete(result);
    return result;
  }
}
```

**Agent approval and rejection methods:**

```typescript
import { Agent, callable } from "agents";

type PendingApproval = {
  workflowId: string;
  amount: number;
  description: string;
  requestedBy: string;
  requestedAt: number;
};

type ExpenseState = {
  pendingApprovals: PendingApproval[];
};

export class ExpenseAgent extends Agent<Env, ExpenseState> {
  initialState: ExpenseState = {
    pendingApprovals: [],
  };

  @callable()
  async approve(workflowId: string, approvedBy: string): Promise<void> {
    await this.approveWorkflow(workflowId, {
      reason: "Expense approved",
      metadata: { approvedBy, approvedAt: Date.now() },
    });

    this.setState({
      ...this.state,
      pendingApprovals: this.state.pendingApprovals.filter(
        (p) => p.workflowId !== workflowId,
      ),
    });
  }

  @callable()
  async reject(workflowId: string, reason: string): Promise<void> {
    await this.rejectWorkflow(workflowId, { reason });

    this.setState({
      ...this.state,
      pendingApprovals: this.state.pendingApprovals.filter(
        (p) => p.workflowId !== workflowId,
      ),
    });
  }
}
```

**Escalation with scheduling:** Use `schedule()` to set reminders after intervals (e.g., 4 hours) and escalation triggers (e.g., 24 hours).

**Audit trail:** Uses `this.sql` for immutable record-keeping with approval audit tables.

### MCP Elicitation

Within MCP server tool execution, call `this.server.server.elicitInput()` to request additional user input using JSON Schema-based forms. Occurs within immediate tool execution (not multi-step workflows).

**MCP elicitation example:**

```typescript
import { McpAgent } from "agents/mcp";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";

type State = { counter: number };

export class CounterMCP extends McpAgent<Env, State, {}> {
  server = new McpServer({
    name: "counter-server",
    version: "1.0.0",
  });

  initialState: State = { counter: 0 };

  async init() {
    this.server.tool(
      "increase-counter",
      "Increase the counter by a user-specified amount",
      { confirm: z.boolean().describe("Do you want to increase the counter?") },
      async ({ confirm }, extra) => {
        if (!confirm) {
          return { content: [{ type: "text", text: "Cancelled." }] };
        }

        const userInput = await this.server.server.elicitInput(
          {
            message: "By how much do you want to increase the counter?",
            requestedSchema: {
              type: "object",
              properties: {
                amount: {
                  type: "number",
                  title: "Amount",
                  description: "The amount to increase the counter by",
                },
              },
              required: ["amount"],
            },
          },
          { relatedRequestId: extra.requestId },
        );

        if (userInput.action !== "accept" || !userInput.content) {
          return { content: [{ type: "text", text: "Cancelled." }] };
        }

        const amount = Number(userInput.content.amount);
        this.setState({
          ...this.state,
          counter: this.state.counter + amount,
        });

        return {
          content: [
            {
              type: "text",
              text: `Counter increased by ${amount}, now at ${this.state.counter}`,
            },
          ],
        };
      },
    );
  }
}
```

**Multi-approver patterns:** Track required approvals count and current approvals list. Check threshold before executing.

---

## Client SDK

**Docs:** https://developers.cloudflare.com/agents/api-reference/client-sdk/

Three primary connection approaches, all delivering bidirectional state synchronization, RPC calls, streaming, and automatic reconnection with exponential backoff.

**React hook (`useAgent`):**

```typescript
import { useAgent } from "agents/react";

const agent = useAgent({
  agent: "ChatAgent",
  name: "room-123",
  onStateUpdate: (state) => console.log("New state:", state),
});

const sendMessage = async () => {
  const response = await agent.call("sendMessage", ["Hello!"]);
};
```

**Vanilla JavaScript (`AgentClient`):**

```javascript
import { AgentClient } from "agents/client";

const client = new AgentClient({
  agent: "ChatAgent",
  name: "room-123",
  host: "your-worker.your-subdomain.workers.dev",
  onStateUpdate: (state) => console.log("New state:", state),
});

const response = await client.call("sendMessage", ["Hello!"]);
```

**HTTP requests (`agentFetch`):**

```javascript
import { agentFetch } from "agents/client";

const response = await agentFetch({
  agent: "DataAgent",
  name: "instance-1",
  host: "my-worker.workers.dev",
});
```

**RPC via stub proxy (type-safe):**

```typescript
const user = await agent.stub.getUser("user-123");
const post = await agent.stub.createPost(title, content, tags);
```

**Connection options include:** `host`, `path`, `query` (static or async for authentication), event handlers (`onOpen`, `onClose`, `onError`), and `onMcpUpdate` for MCP server state changes.

**agentFetch vs WebSocket decision:** Use `agentFetch` for one-time requests, server-to-server calls, simple REST-style APIs. Use WebSocket clients for real-time updates, bidirectional communication, state synchronization, multiple RPC calls.

---

## Email Routing

**Docs:** https://developers.cloudflare.com/agents/api-reference/email/

Agents can process inbound emails via Cloudflare Email Routing. Supports routing, replying, forwarding, and rejecting.

**Basic implementation:**

```javascript
import { Agent, routeAgentEmail } from "agents";
import { createAddressBasedEmailResolver } from "agents/email";

export class EmailAgent extends Agent {
  async onEmail(email) {
    console.log("Received email from:", email.from);
    console.log("Subject:", email.headers.get("subject"));
    await this.replyToEmail(email, {
      fromName: "My Agent",
      body: "Thanks for your email!",
    });
  }
}

export default {
  async email(message, env) {
    await routeAgentEmail(message, env, {
      resolver: createAddressBasedEmailResolver("EmailAgent"),
    });
  },
};
```

**Email resolvers:**
- **Address-based:** Routes based on recipient address. Supports `agent+id@domain` format for routing to specific instances.
- **Secure reply:** Verifies incoming emails are authentic replies using HMAC signatures. Options: `maxAge`, `onInvalidSignature`.
- **Catch-all:** Routes all emails to a specific agent instance.
- **Combined resolvers:** Check secure replies first, then fall back to address-based.

**AgentEmail interface:**

```typescript
type AgentEmail = {
  from: string;
  to: string;
  headers: Headers;
  rawSize: number;
  getRaw(): Promise<Uint8Array>;
  reply(options): Promise<void>;
  forward(rcptTo, headers?): Promise<void>;
  setReject(reason): void;
};
```

**Email content parsing with postal-mime:**

```javascript
import PostalMime from "postal-mime";

class MyAgent extends Agent {
  async onEmail(email) {
    const raw = await email.getRaw();
    const parsed = await PostalMime.parse(raw);
    console.log("Subject:", parsed.subject);
    console.log("Text body:", parsed.text);
    console.log("HTML body:", parsed.html);
    console.log("Attachments:", parsed.attachments);
  }
}
```

**Auto-reply detection (prevents mail loops):**

```javascript
import { isAutoReplyEmail } from "agents/email";

class MyAgent extends Agent {
  async onEmail(email) {
    const raw = await email.getRaw();
    const parsed = await PostalMime.parse(raw);
    if (isAutoReplyEmail(parsed.headers)) {
      console.log("Skipping auto-reply email");
      return;
    }
  }
}
```

---

## Browse the Web (Headless Browser)

**Docs:** https://developers.cloudflare.com/agents/api-reference/browse-the-web/

Agents can browse the web using the Browser Rendering API (Puppeteer) or Browserbase.

**Browser Rendering API with Puppeteer:**

```typescript
export class MyAgent extends Agent<Env> {
  async browse(browserInstance: Fetcher, urls: string[]) {
    let responses = [];
    for (const url of urls) {
      const browser = await puppeteer.launch(browserInstance);
      const page = await browser.newPage();
      await page.goto(url);

      await page.waitForSelector("body");
      const bodyContent = await page.$eval(
        "body",
        (element) => element.innerHTML,
      );

      let resp = await this.env.AI.run("@cf/zai-org/glm-4.7-flash", {
        messages: [
          {
            role: "user",
            content: `Return JSON with product names, prices, URLs from: <content>${bodyContent}</content>`,
          },
        ],
      });

      responses.push(resp);
      await browser.close();
    }
    return responses;
  }
}
```

**Wrangler configuration:**

```jsonc
{
  "ai": { "binding": "AI" },
  "browser": { "binding": "MYBROWSER" },
}
```

**Browserbase:** Store API key as secret with `npx wrangler@latest secret put BROWSERBASE_API_KEY`, then use the Browserbase API directly.

---

## Model Context Protocol (MCP)

**Docs:** https://developers.cloudflare.com/agents/concepts/tools/

MCP is an open standard connecting AI systems with external applications, described as "a USB-C port for AI applications."

**MCP terminology:**
- **MCP Hosts:** AI assistants or agents needing external capabilities
- **MCP Clients:** Embedded clients connecting to MCP servers
- **MCP Servers:** Applications exposing tools, prompts, and resources

**Connection modes:**
- Remote: MCP clients connect over Internet via Streamable HTTP with OAuth
- Local: MCP clients on same machine using stdio

**Agent as MCP client -- server-side methods:**
- `addMcpServer()`: Connect to an MCP server
- `removeMcpServer()`: Disconnect from an MCP server
- `getMcpServers()`: List connected MCP servers

**Client-side MCP updates:**

```javascript
const agent = useAgent({
  agent: "AssistantAgent",
  name: "session-123",
  onMcpUpdate: (mcpServers) => {
    for (const [serverId, server] of Object.entries(mcpServers)) {
      console.log(`${serverId}: ${server.connectionState}`);
    }
  },
});
```

---

## Protocol Messages

**Docs:** https://developers.cloudflare.com/agents/api-reference/protocol-messages/

When WebSocket clients connect, the framework automatically sends three JSON text frames: `cf_agent_identity` (agent name/class), `cf_agent_state` (current state), and `cf_agent_mcp_servers` (MCP server list).

**Suppressing protocol messages for binary-only clients:**

```javascript
shouldSendProtocolMessages(connection, ctx) {
  const url = new URL(ctx.request.url);
  return url.searchParams.get("protocol") !== "false";
}
```

**Using WebSocket subprotocol to decide:**

```javascript
shouldSendProtocolMessages(connection, ctx) {
  const subprotocol = ctx.request.headers.get("Sec-WebSocket-Protocol");
  return subprotocol !== "mqtt";
}
```

**Checking protocol status:**

Use `isConnectionProtocolEnabled(connection)` to determine if a connection has protocol messages active. Safe to call after hibernation.

**What still works when suppressed:**

| Feature | Status |
|---------|--------|
| Protocol frames on connect/broadcast | No |
| Regular WebSocket messages | Yes |
| @callable() RPC methods | Yes |
| this.broadcast() messages | Yes |
| Binary data transmission | Yes |
| Agent state mutation via RPC | Yes |

**Readonly connections:**

```javascript
shouldConnectionBeReadonly(connection, ctx) {
  const url = new URL(ctx.request.url);
  return url.searchParams.get("type") === "sensor";
}
```

---

## Observability

**Docs:** https://developers.cloudflare.com/agents/api-reference/observability/

Agent instances emit internal events via the `observability` property for logging and monitoring.

**Default behavior:** Executes `console.log()` on event values with structured data including displayMessage, id, payload, timestamp, and type fields.

**Custom observability implementation:**

```javascript
import { Agent } from "agents";

const observability = {
  emit(event) {
    if (event.type === "connect") {
      console.log(event.timestamp, event.payload.connectionId);
    }
  },
};

class MyAgent extends Agent {
  observability = observability;
}
```

**Integration with external logging service:**

```javascript
import { Agent } from "agents";

const observability = {
  emit(event) {
    fetch("https://logging.example.com/ingest", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        service: "my-agent",
        level: event.type === "error" ? "error" : "info",
        message: event.displayMessage,
        metadata: {
          eventId: event.id,
          eventType: event.type,
          timestamp: event.timestamp,
          ...event.payload,
        },
      }),
    }).catch(console.error);
  },
};

class MyAgent extends Agent {
  observability = observability;
}
```

**Disable events:**

```javascript
class MyAgent extends Agent {
  observability = undefined;
}
```

**Available event types:**
- `connect`: WebSocket connection established
- `disconnect`: WebSocket connection closed
- `state:update`: Agent state modifications
- `message`: Client messages received
- `error`: Processing errors
- `schedule:execute`: Scheduled task execution
- `queue:process`: Queue task processing

**Event structure:** Each `ObservabilityEvent` contains: `id`, `type`, `displayMessage`, `timestamp` (Unix milliseconds), and `payload` (event-specific metadata).

Supports sending events to remote logging services via HTTP POST requests.

---

## Webhooks

**Docs:** https://developers.cloudflare.com/agents/guides/webhooks/

Agents can receive webhook events from external services and route them to dedicated agent instances using `getAgentByName()`.

**Webhook agent with signature verification:**

```javascript
import { Agent, getAgentByName, routeAgentRequest } from "agents";

export class WebhookAgent extends Agent {
  async onRequest(request) {
    if (request.method !== "POST") {
      return new Response("Method not allowed", { status: 405 });
    }

    const signature = request.headers.get("X-Hub-Signature-256");
    const body = await request.text();

    if (!(await this.verifySignature(body, signature, this.env.WEBHOOK_SECRET))) {
      return new Response("Invalid signature", { status: 401 });
    }

    const payload = JSON.parse(body);
    await this.processEvent(payload);
    return new Response("OK", { status: 200 });
  }

  async verifySignature(payload, signature, secret) {
    if (!signature) return false;

    const encoder = new TextEncoder();
    const key = await crypto.subtle.importKey(
      "raw",
      encoder.encode(secret),
      { name: "HMAC", hash: "SHA-256" },
      false,
      ["sign"],
    );

    const signatureBytes = await crypto.subtle.sign(
      "HMAC",
      key,
      encoder.encode(payload),
    );
    const expected = `sha256=${Array.from(new Uint8Array(signatureBytes))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("")}`;

    return signature === expected;
  }

  async processEvent(payload) {
    // Store event, update state, trigger actions...
  }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (url.pathname.startsWith("/webhooks/") && request.method === "POST") {
      const entityId = url.pathname.split("/")[2];
      const agent = await getAgentByName(env.WebhookAgent, entityId);
      return agent.fetch(request);
    }

    return (
      (await routeAgentRequest(request, env)) ||
      new Response("Not found", { status: 404 })
    );
  },
};
```

**Event deduplication:**

```javascript
class WebhookAgent extends Agent {
  async handleEvent(eventId, payload) {
    const existing = [
      ...this.sql`
      SELECT id FROM events WHERE id = ${eventId}
    `,
    ];

    if (existing.length > 0) {
      console.log(`Event ${eventId} already processed, skipping`);
      return;
    }

    await this.processPayload(payload);
    this.sql`INSERT INTO events (id, ...) VALUES (${eventId}, ...)`;
  }
}
```

**Async processing with queue:**

```javascript
class WebhookAgent extends Agent {
  async onRequest(request) {
    const payload = await request.json();

    if (!this.isValid(payload)) {
      return new Response("Invalid", { status: 400 });
    }

    await this.queue("processWebhook", payload);
    return new Response("Accepted", { status: 202 });
  }

  async processWebhook(payload) {
    await this.enrichData(payload);
    await this.notifyDownstream(payload);
    await this.updateAnalytics(payload);
  }
}
```

**Multi-provider routing:**

```javascript
export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === "POST") {
      if (url.pathname.startsWith("/webhooks/github/")) {
        const payload = await request.clone().json();
        const repoName = payload.repository?.full_name?.replace("/", "-");
        const agent = await getAgentByName(env.GitHubAgent, repoName);
        return agent.fetch(request);
      }

      if (url.pathname.startsWith("/webhooks/stripe/")) {
        const payload = await request.clone().json();
        const customerId = payload.data?.object?.customer;
        const agent = await getAgentByName(env.StripeAgent, customerId);
        return agent.fetch(request);
      }

      if (url.pathname === "/webhooks/slack") {
        const teamId = request.headers.get("X-Slack-Team-Id");
        const agent = await getAgentByName(env.SlackAgent, teamId);
        return agent.fetch(request);
      }
    }

    return (
      (await routeAgentRequest(request, env)) ??
      new Response("Not found", { status: 404 })
    );
  },
};
```

**Outbound webhooks:** Agents can send webhooks with HMAC-signed headers.

---

## Configuration (Wrangler)

**Docs:** https://developers.cloudflare.com/agents/api-reference/configuration/

Agents are configured via `wrangler.jsonc` or `wrangler.toml`.

**Key configuration fields:**

- `compatibility_flags`: Must include `"nodejs_compat"` for Node.js APIs
- `durable_objects.bindings`: Each agent requires `name` (env variable) and `class_name` (exported class)
- `migrations`: Manages Durable Object storage with tags (v1, v2, etc.) and `new_sqlite_classes`
- `assets`: Optional static file serving
- `ai`: Optional Workers AI binding
- `observability`: Recommended `"enabled": true`

**Example wrangler.jsonc:**

```jsonc
{
  "ai": { "binding": "AI" },
  "durable_objects": {
    "bindings": [{ "name": "ChatAgent", "class_name": "ChatAgent" }],
  },
  "migrations": [{ "tag": "v1", "new_sqlite_classes": ["ChatAgent"] }],
}
```

**Type generation:** Run `npx wrangler types` to auto-generate TypeScript definitions.

**Environment variables and secrets:**
- Local: `.env` file (add to `.gitignore`)
- Production: `npx wrangler secret put KEY_NAME`
- Non-secret variables: `vars` section in wrangler config (all values must be strings)
- Multi-environment: `env` sections (staging, production) with environment-specific overrides

**Local development:**
- `npx vite dev` (recommended for full-stack) or `npx wrangler dev`
- Durable Object state persists in `.wrangler/state/v3/d1/`
- Reset with `rm -rf .wrangler/state`
- Inspect SQLite directly: `sqlite3 .wrangler/state/v3/d1/miniflare-D1DatabaseObject/*.sqlite`

**Migrations for adding/renaming/deleting agents:**
- Adding: New migration tag with class in `new_sqlite_classes`
- Renaming: `renamed_classes` with `from`/`to` fields
- Deleting: `deleted_classes` to permanently remove stored data

---

## x402 Payment Protocol

**Docs:** https://developers.cloudflare.com/agents/ (x402 section)

x402 enables services to charge for API access using HTTP 402 Payment Required status code.

**Paywalled Worker with payment middleware:**

```typescript
import { Hono } from "hono";
import { Agent, getAgentByName } from "agents";
import { wrapFetchWithPayment } from "x402-fetch";
import { paymentMiddleware } from "x402-hono";
import { privateKeyToAccount } from "viem/accounts";

export class PayAgent extends Agent {
  fetchWithPay!: ReturnType<typeof wrapFetchWithPayment>;

  onStart() {
    const privateKey = process.env.CLIENT_TEST_PK as `0x${string}`;
    const account = privateKeyToAccount(privateKey);
    this.fetchWithPay = wrapFetchWithPayment(fetch, account);
  }

  async onRequest(req: Request) {
    const url = new URL(req.url);
    const paidUrl = new URL("/protected-route", url.origin).toString();
    return this.fetchWithPay(paidUrl, {});
  }
}

const app = new Hono<{ Bindings: Env }>();

app.use(
  paymentMiddleware(
    process.env.SERVER_ADDRESS as `0x${string}`,
    {
      "/protected-route": {
        price: "$0.10",
        network: "base-sepolia",
        config: { description: "Access to premium content" },
      },
    },
    { url: "https://x402.org/facilitator" },
  ),
);
```

**MCP Servers with paid tools:**

```typescript
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { McpAgent } from "agents/mcp";
import { withX402, type X402Config } from "agents/x402";
import { z } from "zod";

const X402_CONFIG: X402Config = {
  network: "base",
  recipient: env.MCP_ADDRESS,
  facilitator: { url: "https://x402.org/facilitator" },
};

export class PaidMCP extends McpAgent {
  server = withX402(
    new McpServer({ name: "PaidMCP", version: "1.0.0" }),
    X402_CONFIG,
  );

  async init() {
    this.server.paidTool(
      "square",
      "Squares a number",
      0.01,
      { number: z.number() },
      {},
      async ({ number }) => {
        return { content: [{ type: "text", text: String(number ** 2) }] };
      },
    );

    this.server.tool(
      "echo",
      "Echo a message",
      { message: z.string() },
      async ({ message }) => {
        return { content: [{ type: "text", text: message }] };
      },
    );
  }
}
```

---

## Agent Patterns

**Docs:** https://developers.cloudflare.com/agents/patterns/ (based on Anthropic research)

### Prompt Chaining
Decomposes tasks into sequential steps where each LLM call processes previous output. Example: generate marketing copy, evaluate quality, regenerate if needed.

### Routing
Classifies input and directs to specialized followup tasks. Example: classify customer query type, route to appropriate handler.

### Parallelization
Enables simultaneous task processing through sectioning or voting mechanisms. Example: multiple specialized code reviewers evaluating same code in parallel.

### Orchestrator-Workers
Central LLM breaks down tasks, delegates to Worker LLMs, synthesizes results. Example: senior architect plans implementation, workers execute planned changes.

### Evaluator-Optimizer
One LLM generates responses while another evaluates and provides feedback in a loop. Example: generate translation, evaluate quality, improve based on feedback.

---

## Platform Limits

**Docs:** https://developers.cloudflare.com/agents/platform/limits/

| Feature | Constraint |
|---------|-----------|
| Concurrent Agents per account | Tens of millions+ |
| Agent definitions per account | ~250,000+ |
| State storage per Agent | 1 GB maximum |
| Compute time per Agent | 30 seconds (refreshed per HTTP request / incoming WebSocket message) |
| Duration per step | Unlimited (allows waiting on external services) |
| Maximum deployed scripts | 500 per account |
| Script size (Workers Paid Plan) | 10 MB maximum |
| Workflow steps | Maximum 1,024 per workflow |
| Workflow state size | 10 MB |
| Workflow event wait time | 1 year maximum |
| Workflow step execution | 30 minutes maximum |
| Schedule task size | Up to 2MB including payload |
| Schedule cron precision | Minute-level only |
| Schedule interval precision | Second-level |
| Connection tags | Up to 9 per connection, 256 chars each |

The 30-second compute limit refreshes upon receiving HTTP requests, scheduled tasks, or WebSocket messages, permitting extended wall-clock durations.

---

## Deployment

**Docs:** https://developers.cloudflare.com/agents/api-reference/configuration/

**Basic deploy:** `npx wrangler deploy` bundles code, uploads to Cloudflare, applies migrations, and deploys to `*.workers.dev`.

**Custom domains:** Configure via `routes` with `pattern` and `zone_name`, or use `custom_domain: true`.

**Preview deployments:** Use `--dry-run`, `versions upload`, and `versions deploy` for gradual rollouts.

**Rollbacks:** `npx wrangler rollback`

**Multi-environment:** Define base config shared across environments, override with `env` sections. Each environment gets its own Durable Objects. Staging agents do not share state with production agents.

---

## Getting Started (Quick Start)

**Docs:** https://developers.cloudflare.com/agents/getting-started/

```bash
npx create-cloudflare@latest --template cloudflare/agents-starter
cd agents-starter && npm install
npm run dev
```

Each agent runs on a Durable Object with its own SQL database, WebSocket connections, and scheduling capabilities.
