# Cloudflare Queues: Comprehensive Feature Reference

> **As of:** 2026-03-06
>
> **Cloudflare basis:** Official Queues docs accessed 2026-03-06. Relevant Cloudflare version marker: default content type semantics changed after Workers compatibility date `2024-03-18`.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Migration framing:** Cloudflare Queues is a managed, shared queue service. Rivet queues are **actor-local durable mailboxes**. That makes Rivet a strong fit for per-entity serialized work, but not a drop-in replacement for a global broker without an actor sharding design.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Queue Creation and Management | Create, update, delete, list, and inspect queues via CLI or dashboard | pattern | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Actor Keys](https://rivet.dev/docs/actors/keys) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Queue identity is usually an actor key or actor type, not a separate managed queue resource. |
| Producer Bindings (Wrangler Configuration) | Configure Workers as queue producers via Wrangler binding declarations | pattern | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [JavaScript Client](https://rivet.dev/docs/clients/javascript) | [examples/cloudflare-workers/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/index.ts) | Medium | Producers call actors via `createClient()` or server-side handles; there is no separate producer binding primitive. |
| Producing Messages (`send` / `sendBatch`) | Send single or batched messages with delay and content type options | native | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | Medium | `handle.send(...)`, `c.queue.send(...)`, `nextBatch`, and `iter()` cover the core flow. |
| Content Types | Four supported message formats: JSON, text, bytes, and V8 serialization | partial | medium | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | Medium | JSON-style payloads fit well. Treat binary/engine-specific formats as a migration spike. |
| Consumer Bindings (Wrangler Configuration) | Configure consumer Workers with batch size, retries, and dead letter settings | pattern | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Consumer behavior lives in actor code and actor options, not in a separate binding block. |
| Consuming Messages (Push-Based / Worker Consumer) | Automatic Worker invocation via queue handler when messages are available | partial | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Rivet run loops are naturally push-like for a live actor, but the queue is still local to that actor rather than a platform-wide service. |
| Message Batching | Configurable batch size and timeout to reduce invocation frequency | native | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | Low | `nextBatch` and `tryNextBatch` provide batching. |
| Explicit Acknowledgment and Retries | Per-message and batch-level ack/retry with precedence rules | partial | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Completion-based retries are native, but retry counts/backoff policies are not Cloudflare-Queues-shaped primitives. |
| Message Delays | Configurable delivery delays up to 12 hours on send or retry | pattern | high | [Actor Scheduling](https://rivet.dev/docs/actors/schedule), [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-schedule.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-schedule.ts) | Medium | Delay by scheduling a future enqueue or a future action. |
| Dead Letter Queues | Route failed messages to a separate queue after retry exhaustion | pattern | medium | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Design Patterns](https://rivet.dev/docs/actors/design-patterns) | Gap | High | Build an explicit dead-letter actor or persistent failure table. |
| Consumer Concurrency (Autoscaling) | Automatic horizontal scaling of consumer Workers based on backlog | pattern | high | [Scaling](https://rivet.dev/docs/actors/scaling), [Design Patterns](https://rivet.dev/docs/actors/design-patterns) | Docs-only | High | Scale by sharding queue ownership across actors. |
| Pull-Based Consumers (HTTP Pull) | Retrieve messages via HTTP from any language or infrastructure | pattern | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [Debugging](https://rivet.dev/docs/actors/debugging) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Medium | Expose a custom pull endpoint if needed. |
| Publishing via HTTP (REST API) | Publish messages to queues directly via Cloudflare REST API | pattern | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Medium | Straightforward with `onRequest` or your gateway layer. |
| Publishing via Workers | Send messages from Worker fetch handlers with error handling | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [JavaScript Client](https://rivet.dev/docs/clients/javascript) | [examples/cloudflare-workers/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/index.ts) | Low | Good fit for Cloudflare-hosted Rivet backends. |
| Queues with Durable Objects | Publish messages from within Durable Object instances | native | medium | [Communicating Between Actors](https://rivet.dev/docs/actors/communicating-between-actors), [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | Low | Actor-to-actor queueing is native. |
| Queues with R2 (Batch Error Storage) | Pattern for batching errors and storing them in R2 object storage | external | low | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | Gap | Medium | Use external object storage if this pattern is required. |
| R2 Event Notifications (Event Subscriptions) | Trigger queue messages on R2 bucket object create or delete events | external | low | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler) | Gap | Medium | Requires external object-storage eventing. |
| Delivery Guarantees | At-least-once delivery with idempotency key recommendations | partial | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | High | Rivet durable queues retry uncompleted messages, but guarantees are actor-local and completion-based, not a managed broker SLA. |
| Pause and Resume Delivery | Halt or restart message delivery to consumers via CLI | pattern | medium | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [State](https://rivet.dev/docs/actors/state) | Gap | Medium | Implement as an application-level paused flag plus guarded consumers. |
| Purge Queue | Remove all messages from a queue with a single CLI command | pattern | medium | [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | Gap | Medium | Possible via custom admin logic, not a built-in control-plane command. |
| Consumer Management (CLI) | Add and remove Worker and HTTP pull consumers via Wrangler | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | No equivalent managed consumer CLI exists. |
| Local Development | Local queue simulation via Wrangler and Miniflare | native | high | [Testing](https://rivet.dev/docs/actors/testing), [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) | Low | Strong local coverage exists. |
| Metrics and Observability | Backlog, concurrency, and message operation metrics via GraphQL API | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Medium | Inspector/logging exist, but not Cloudflare Queues metrics. |
| Error Codes | Client, rate limit, and server error codes with descriptions | native | high | [Errors](https://rivet.dev/docs/actors/errors) | [actor-error-handling.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-error-handling.ts) | Low | Error handling is well covered. |
| Limits | Message size, throughput, retention, and concurrency constraints | partial | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | High | Limits differ substantially and must be re-planned. |
| Pricing | Operation-based pricing with free and paid tiers | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Docs-only | Low | Commercial comparison, not a runtime feature. |
| Rate Limit Handling Pattern | Queue-based pattern for respecting external API rate limits | native | high | [Design Patterns](https://rivet.dev/docs/actors/design-patterns), [Actor Scheduling](https://rivet.dev/docs/actors/schedule) | Docs-only | Low | Actor-local queues plus timers are a strong fit. |
| How Queues Works (Architecture) | Core concepts, architectural properties, and consumer types | partial | high | [Queues & Run Loops](https://rivet.dev/docs/actors/queues), [Scaling](https://rivet.dev/docs/actors/scaling) | Docs-only | Medium | Architectural concepts map, but only after accounting for actor-local ownership. |
| Dashboard Operations | UI for fetching, acknowledging, and sending messages | out-of-scope | high | [Debugging](https://rivet.dev/docs/actors/debugging) | Gap | Low | No Cloudflare Queues-style dashboard ops surface is documented. |
| Wrangler Global Flags | Global CLI flags available for all Wrangler queues commands | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | No equivalent control-plane CLI. |

## High-Risk Behavioral Deltas

- **This is not a global broker-to-broker migration.** Cloudflare Queues is a managed shared service; Rivet queues are local to an actor instance.
- **Ordering is per actor queue, not cross-shard.** If the current system relies on one global FIFO, redesign around ownership or introduce an external queue.
- **Retry semantics are completion-based.** Rivet retries durable messages that are not completed, but retry policy, backoff, DLQ routing, and operational controls are application patterns.
- **Delays and scheduling are separate concerns.** Cloudflare puts delay on the queue send itself. In Rivet, delayed enqueue is usually implemented with `c.schedule`.
- **Autoscaling becomes a sharding problem.** To increase consumer concurrency, distribute work across more actor keys or actor types.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Queue send/receive works | Actor queue can durably accept and consume messages | Pass: [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) |
| Completable/wait send paths are acceptable | Sender can wait for completion and receive timeout/completed status | Pass: [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) |
| Retry behavior is explicit | Uncompleted messages retry and app code is idempotent | Pass: [actor-queue.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-queue.ts) |
| Delay scheduling works | Delayed tasks fire in timestamp order | Pass: [actor-schedule.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-schedule.ts) |
| Global queue topology is redesigned | Migration plan states shard key and actor ownership model | Gap: review [Scaling](https://rivet.dev/docs/actors/scaling) and [Design Patterns](https://rivet.dev/docs/actors/design-patterns) during migration design |
| DLQ path is defined | Failure routing exists after retry exhaustion | Gap: build on [Queues & Run Loops](https://rivet.dev/docs/actors/queues); no first-class DLQ proof exists today |
| Ops observability is sufficient | Team has alternative dashboards/alerts for queue backlog and failures | Gap: [Debugging](https://rivet.dev/docs/actors/debugging) exists, but no Cloudflare Queues-style backlog metrics surface is documented |

---

This document catalogs every feature of Cloudflare Queues with descriptions, documentation links, and code snippets directly from the official documentation. The purpose is to enable mapping each Cloudflare Queues feature to its Rivet Actor equivalent.

---

## Queue Creation and Management

**Docs:** https://developers.cloudflare.com/queues/get-started/
**Docs:** https://developers.cloudflare.com/queues/reference/wrangler-commands/

A queue is a buffer or list that automatically scales as messages are written to it, and allows a consumer Worker to pull messages from that same queue. Queues are created and managed via the Wrangler CLI or Cloudflare Dashboard.

**Create a queue:**
```bash
npx wrangler queues create <MY-QUEUE-NAME>
```

Queue names must be 1-63 characters, start/end with alphanumeric characters, and can only contain dashes as special characters.

**Create with options:**
```bash
npx wrangler queues create <QUEUE-NAME> --delivery-delay-secs 60 --message-retention-period-secs 3000
```

- `--delivery-delay-secs`: Delays message delivery (0-43200 seconds; defaults to 0)
- `--message-retention-period-secs`: Retention duration (60-1209600 seconds; defaults to 345600/4 days)

**Update a queue:**
```bash
npx wrangler queues update <QUEUE-NAME> --delivery-delay-secs 60 --message-retention-period-secs 3000
```

**Delete a queue:**
```bash
npx wrangler queues delete <QUEUE-NAME>
```

**List all queues:**
```bash
npx wrangler queues list
```

**Get queue info:**
```bash
npx wrangler queues info <QUEUE-NAME>
```

**Pause/resume delivery:**
```bash
npx wrangler queues pause-delivery <QUEUE-NAME>
npx wrangler queues resume-delivery <QUEUE-NAME>
```

**Purge all messages:**
```bash
npx wrangler queues purge <QUEUE-NAME> --force
```

**Limits:**
- 10,000 queues per account
- Per-queue message throughput: 5,000 messages/second
- Per-queue backlog size: 25GB

---

## Producer Bindings (Wrangler Configuration)

**Docs:** https://developers.cloudflare.com/queues/configuration/configure-queues/

A producer is a client that publishes or produces messages onto a queue. Producer bindings are configured in wrangler configuration files.

**wrangler.jsonc:**
```json
{
  "queues": {
    "producers": [
      {
        "queue": "my-queue",
        "binding": "MY_QUEUE"
      }
    ]
  }
}
```

**wrangler.toml:**
```toml
[[queues.producers]]
queue = "my-queue"
binding = "MY_QUEUE"
```

Multiple producer Workers can write to a single queue without limit.

---

## Producing Messages (send / sendBatch)

**Docs:** https://developers.cloudflare.com/queues/configuration/javascript-apis/

Producers send messages to queues via the `send()` and `sendBatch()` methods on the queue binding.

### Queue Interface

```typescript
interface Queue<Body = unknown> {
  send(body: Body, options?: QueueSendOptions): Promise<void>;
  sendBatch(messages: Iterable<MessageSendRequest<Body>>, options?: QueueSendBatchOptions): Promise<void>;
}
```

### QueueSendOptions

```typescript
interface QueueSendOptions {
  contentType?: QueuesContentType;
  delaySeconds?: number;
}
```

### QueueSendBatchOptions

```typescript
interface QueueSendBatchOptions {
  delaySeconds?: number;
}
```

### MessageSendRequest

```typescript
interface MessageSendRequest<Body = unknown> {
  body: Body;
  contentType?: QueueContentType;
  delaySeconds?: number;
}
```

- `delaySeconds`: integer between 0 and 43200 (12 hours)

### Single Message Example

```javascript
export default {
  async fetch(req, env, ctx) {
    await env.MY_QUEUE.send({
      url: req.url,
      method: req.method,
      headers: Object.fromEntries(req.headers),
    });
    return new Response("Sent!");
  },
};
```

### Batch Message Example

```javascript
const sendResultsToQueue = async (results, env) => {
  const batch = results.map((value) => ({
    body: value,
  }));
  await env.MY_QUEUE.sendBatch(batch);
};
```

**Key constraints:**
- Message body: max 128 KB
- Batch messages: up to 100 per batch, 128 KB each, 256 KB total
- When the promise resolves, the message is confirmed to be written to disk.

---

## Content Types

**Docs:** https://developers.cloudflare.com/queues/configuration/javascript-apis/

Messages support four content types:

```typescript
type QueuesContentType = "text" | "bytes" | "json" | "v8";
```

- `"json"`: JSON-serializable JavaScript objects (default for compatibility date after 2024-03-18)
- `"text"`: String values
- `"bytes"`: ArrayBuffer values
- `"v8"`: Non-JSON-serializable objects (Date, Map, etc.) -- specific to Workers runtime

The default content type changed from `v8` to `json` to improve compatibility with pull-based consumers for any Workers with a compatibility date after `2024-03-18`.

Pull-based consumers cannot decode the `v8` content type as it is specific to the Workers runtime. Use only `text`, `bytes`, or `json` types for pull consumers. The `json` and `bytes` types arrive base64-encoded and require decoding.

---

## Consumer Bindings (Wrangler Configuration)

**Docs:** https://developers.cloudflare.com/queues/configuration/configure-queues/

A consumer is a client that subscribes to or consumes messages from a queue. Consumer bindings are configured in wrangler configuration files.

**wrangler.jsonc:**
```json
{
  "queues": {
    "consumers": [
      {
        "queue": "my-queue",
        "max_batch_size": 10,
        "max_batch_timeout": 30,
        "max_retries": 10,
        "dead_letter_queue": "my-queue-dlq"
      }
    ]
  }
}
```

**wrangler.toml:**
```toml
[[queues.consumers]]
queue = "my-queue"
max_batch_size = 10
max_batch_timeout = 30
max_retries = 10
dead_letter_queue = "my-queue-dlq"
```

**Consumer configuration parameters:**
- `queue`: Queue name (required)
- `max_batch_size`: Maximum messages per batch (defaults to 10, max 100)
- `max_batch_timeout`: Maximum wait time in seconds before delivering batch (defaults to 5, max 60)
- `max_retries`: Retry attempts on failure (defaults to 3, max 100)
- `dead_letter_queue`: Destination queue for messages that exhaust retries
- `max_concurrency`: Maximum concurrent consumer invocations (optional, max 250)

Each queue can only have one active consumer.

---

## Consuming Messages (Push-Based / Worker Consumer)

**Docs:** https://developers.cloudflare.com/queues/configuration/javascript-apis/
**Docs:** https://developers.cloudflare.com/queues/get-started/

Push-based consumers use a `queue()` handler in a Worker that is automatically invoked when messages are available. Workers activate when the queue contains messages. Empty queues do not trigger handler invocations.

### Consumer Handler Example

```javascript
export default {
  async queue(batch, env, ctx) {
    for (const message of batch.messages) {
      console.log("Received", message.body);
    }
  },
};
```

### Full Producer + Consumer Example

```typescript
export default {
  async fetch(request, env, ctx): Promise<Response> {
    const log = {
      url: request.url,
      method: request.method,
      headers: Object.fromEntries(request.headers),
    };

    await env.<MY_QUEUE>.send(log);

    return new Response("Success!");
  },
  async queue(batch, env, ctx): Promise<void> {
    for (const message of batch.messages) {
      console.log("consumed from our queue:", JSON.stringify(message.body));
    }
  },
} satisfies ExportedHandler<Env>;
```

### MessageBatch Interface

```typescript
interface MessageBatch<Body = unknown> {
  readonly queue: string;
  readonly messages: readonly Message<Body>[];
  ackAll(): void;
  retryAll(options?: QueueRetryOptions): void;
}
```

### Message Interface

```typescript
interface Message<Body = unknown> {
  readonly id: string;
  readonly timestamp: Date;
  readonly body: Body;
  readonly attempts: number;
  ack(): void;
  retry(options?: QueueRetryOptions): void;
}
```

- `attempts`: Starts at 1
- `id`: System-generated unique identifier

### Default Acknowledgment Behavior

By default, all messages in the batch will be acknowledged as soon as all of the following conditions are met:
1. The `queue()` function has returned.
2. If the `queue()` function returned a promise, the promise has resolved.
3. Any promises passed to `waitUntil()` have resolved.

If the `queue()` function throws, or the promise returned by it or any of the promises passed to `waitUntil()` were rejected, then the entire batch will be considered a failure and will be retried according to the consumer's retry settings.

---

## Message Batching

**Docs:** https://developers.cloudflare.com/queues/configuration/batching-retries/

Message batching reduces invocation frequency and costs. Two configuration parameters control batching behavior:

- `max_batch_size` (default: 10, range: 1-100 messages) -- maximum messages per batch
- `max_batch_timeout` (default: 5 seconds, range: 0-60 seconds) -- maximum wait time before delivery

Whichever limit is reached first will trigger the delivery of a batch. Empty queues do not trigger handler invocations for push-based consumers, avoiding unnecessary reads.

---

## Explicit Acknowledgment and Retries

**Docs:** https://developers.cloudflare.com/queues/configuration/batching-retries/

Individual messages can be acknowledged or retried explicitly, providing fine-grained control over message processing.

### Per-Message Operations

- `msg.ack()`: Acknowledges a single message, preventing redelivery even if subsequent batch processing fails. Valuable for non-idempotent operations like API calls or database writes.
- `msg.retry()`: Forces redelivery of a single message in a subsequent batch without failing the entire batch.

### Batch-Level Operations

- `batch.ackAll()`: Acknowledges all messages in the batch.
- `batch.retryAll(options?)`: Retries all messages in the batch.

### QueueRetryOptions

```typescript
interface QueueRetryOptions {
  delaySeconds?: number;
}
```

### Precedence Rules

- The initial `ack()` or `retry()` call on a message takes precedence over subsequent calls.
- Single-message calls (`msg.ack()`, `msg.retry()`) override batch-level calls (`ackAll()`, `retryAll()`) for that specific message.

### Explicit Ack Example

```typescript
export default {
  async queue(batch, env, ctx): Promise<void> {
    for (const msg of batch.messages) {
      // TODO: do something with the message
      // Explicitly acknowledge the message as delivered
      msg.ack();
    }
  },
} satisfies ExportedHandler<Env>;
```

### Explicit Retry Example

```typescript
export default {
  async queue(batch, env, ctx): Promise<void> {
    for (const msg of batch.messages) {
      // TODO: do something with the message that fails
      msg.retry();
    }
  },
} satisfies ExportedHandler<Env>;
```

### Batch Retry with Delay Example

```typescript
export default {
  async queue(batch, env, ctx): Promise<void> {
    // Mark for retry and delay a batch of messages
    // by 600 seconds (10 minutes)
    batch.retryAll({ delaySeconds: 600 });
  },
} satisfies ExportedHandler<Env>;
```

### Delivery Failure Behavior

Default retry limit is 3 attempts before deletion or dead-letter queue routing. When a single message within a batch fails to be delivered, the entire batch is retried, unless you have explicitly acknowledged a message (or messages) within that batch.

---

## Message Delays

**Docs:** https://developers.cloudflare.com/queues/configuration/batching-retries/

Messages support delays up to 12 hours (43,200 seconds), applicable during send or retry operations.

### Delay on Send

Use `delaySeconds` parameter with `send()` or `sendBatch()` methods:

```javascript
await env.MY_QUEUE.send(message, { delaySeconds: 60 });
```

### Delay on Retry

Pass `delaySeconds` to `msg.retry()` or `batch.retryAll()`:

```javascript
msg.retry({ delaySeconds: 30 });
batch.retryAll({ delaySeconds: 60 });
```

### Queue-Level Default Delays

Configure via `--delivery-delay-secs` during queue creation or via `delivery_delay`/`retry_delay` in Wrangler configuration files.

### Precedence

Message-level settings override queue defaults. Setting `delaySeconds: 0` bypasses queue-level delays.

### Exponential Backoff Pattern

Messages include an `attempts` property for tracking delivery attempts. Exponential backoff can be implemented by calculating `baseDelaySeconds ** attempts` and passing the result to `retry()`.

---

## Dead Letter Queues

**Docs:** https://developers.cloudflare.com/queues/configuration/dead-letter-queues/

A Dead Letter Queue (DLQ) receives messages when consumer delivery fails after exhausting retry attempts. A DLQ is like any other queue, and can be produced to and consumed from independently.

Without a DLQ specified, failed messages are permanently deleted after retries are exhausted.

### Configuration

**wrangler.jsonc:**
```json
{
  "queues": {
    "consumers": [
      {
        "queue": "my-queue",
        "dead_letter_queue": "my-other-queue"
      }
    ]
  }
}
```

**CLI setup:**
```bash
wrangler queues consumer add $QUEUE_NAME $SCRIPT_NAME --dead-letter-queue=$NAME_OF_OTHER_QUEUE
```

### Processing DLQ Messages

To handle messages in a DLQ, you must configure a consumer for that queue like any standard queue.

Messages delivered to a DLQ without an active consumer will persist for four (4) days before being deleted from the queue.

---

## Consumer Concurrency (Autoscaling)

**Docs:** https://developers.cloudflare.com/queues/configuration/consumer-concurrency/

Consumer concurrency allows a consumer Worker processing messages from a queue to automatically scale out horizontally to keep up with the rate that messages are being written to a queue.

### How It Works

The system activates by default, with Workers automatically adjusting up to maximum concurrent invocations based on:
- Queue backlog and growth rate
- Failed invocation ratio (uncaught exceptions)
- The `max_concurrency` setting

The system prioritizes preventing exponential backlog growth to avoid hitting message retention limits.

### Configuration

Set maximum concurrent invocations via:
1. Cloudflare Dashboard (range: 1-250)
2. Wrangler configuration file (requires version 2.13.0+)

**wrangler.jsonc:**
```json
{
  "queues": {
    "consumers": [
      {
        "queue": "my-queue",
        "max_concurrency": 1
      }
    ]
  }
}
```

**wrangler.toml:**
```toml
[[queues.consumers]]
queue = "my-queue"
max_concurrency = 1
```

### Recommendation

Cloudflare advises leaving concurrency unset to enable maximum scaling, as setting a fixed number means that your consumer will only ever scale up to that maximum, even as Queues increases the maximum supported invocations over time.

### Billing Impact

Concurrent invocations incur standard CPU time costs, but overall expenses remain identical whether processing messages concurrently or sequentially -- concurrency simply accelerates completion and prevents message expiration.

---

## Pull-Based Consumers (HTTP Pull)

**Docs:** https://developers.cloudflare.com/queues/configuration/pull-consumers/

Pull-based consumers enable message retrieval from a queue via HTTP requests from environments outside Cloudflare Workers. This approach works with any programming language and offers control over consumption rates.

### When to Use Pull vs Push

Push-based consumers are the easiest way to get started since they automatically scale on Workers infrastructure. Pull-based consumers are preferable when you need to consume messages from existing infrastructure outside of Cloudflare Workers, and/or where you need to carefully control how fast messages are consumed.

### Setup

**1. Authentication Token**

Create an API token with `queues#read` and `queues#write` permissions. Both are required because a consumer must be able to write to a queue to acknowledge messages.

**2. Enable HTTP Pull in Wrangler**

```toml
[[queues.consumers]]
queue = "QUEUE-NAME"
type = "http_pull"
visibility_timeout_ms = 5_000
max_retries = 5
dead_letter_queue = "SOME-OTHER-QUEUE"
```

**Or via CLI:**
```bash
npx wrangler queues consumer http add $QUEUE-NAME
```

### Pulling Messages

```javascript
let resp = await fetch(
  `https://api.cloudflare.com/client/v4/accounts/${CF_ACCOUNT_ID}/queues/${QUEUE_ID}/messages/pull`,
  {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${QUEUES_API_TOKEN}`,
    },
    body: JSON.stringify({ visibility_timeout_ms: 6000, batch_size: 50 }),
  },
);
```

**Pull parameters:**
- `batch_size` (default: 5; max: 100)
- `visibility_timeout` (default: 30 seconds; max: 12 hours)

### Acknowledging Messages

```javascript
let resp = await fetch(
  `https://api.cloudflare.com/client/v4/accounts/${CF_ACCOUNT_ID}/queues/${QUEUE_ID}/messages/ack`,
  {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${QUEUES_API_TOKEN}`,
    },
    body: JSON.stringify({
      acks: [
        { lease_id: "lease_id1" },
        { lease_id: "lease_id2" },
      ],
      retries: [{ lease_id: "lease_id4", delay_seconds: 600 }],
    }),
  },
);
```

### Message Structure (Pull Response)

Each message contains:
- `body` -- may be base64 encoded
- `id` -- unique ephemeral identifier
- `timestamp_ms` -- Unix epoch publication time
- `attempts` -- delivery attempt count
- `lease_id` -- used for acknowledgment or retry

---

## Publishing via HTTP (REST API)

**Docs:** https://developers.cloudflare.com/queues/examples/publish-to-a-queue-via-http/

Messages can be published to a queue directly via HTTP requests from any service or programming language, including Go, Rust, Python, or even a Bash script.

**Requirements:**
- A queue created via the Cloudflare dashboard or wrangler CLI
- A Cloudflare API token with the `Queues Edit` permission

**Curl example:**
```bash
curl -XPOST -H "Authorization: Bearer <paste-your-api-token-here>" \
  "https://api.cloudflare.com/client/v4/accounts/<paste-your-account-id-here>/queues/<paste-your-queue-id-here>/messages" \
  --data '{ "body": { "greeting": "hello" } }'
```

A successful request returns HTTP 200 with the response: `{"success":true}`

If you receive an HTTP 403 error, your API token is invalid or does not have the `Queues Edit` permission.

---

## Publishing via Workers

**Docs:** https://developers.cloudflare.com/queues/examples/publish-to-a-queue-via-workers/

```typescript
interface Env {
  YOUR_QUEUE: Queue;
}

export default {
  async fetch(req, env, ctx): Promise<Response> {
    let messages;
    try {
      messages = await req.json();
    } catch {
      return Response.json({ error: "payload not valid JSON" }, { status: 400 });
    }

    try {
      await env.YOUR_QUEUE.send(messages);
    } catch (e) {
      const message = e instanceof Error ? e.message : "Unknown error";
      console.error(`failed to send to the queue: ${message}`);
      return Response.json({ error: message }, { status: 500 });
    }

    return Response.json({ success: true });
  },
} satisfies ExportedHandler<Env>;
```

Deploy with:
```bash
npx wrangler deploy
```

Test with curl:
```bash
curl -XPOST "https://YOUR_WORKER.YOUR_ACCOUNT.workers.dev" \
  --data '{"messages": [{"msg":"hello world"}]}'
```

---

## Queues with Durable Objects

**Docs:** https://developers.cloudflare.com/queues/examples/use-queues-with-durable-objects/

Messages can be published to a queue from within a Durable Object.

**wrangler.jsonc configuration:**
```json
{
  "$schema": "./node_modules/wrangler/config-schema.json",
  "name": "my-worker",
  "queues": {
    "producers": [
      {
        "queue": "my-queue",
        "binding": "YOUR_QUEUE"
      }
    ]
  },
  "durable_objects": {
    "bindings": [
      {
        "name": "YOUR_DO_CLASS",
        "class_name": "YourDurableObject"
      }
    ]
  },
  "migrations": [
    {
      "tag": "v1",
      "new_sqlite_classes": [
        "YourDurableObject"
      ]
    }
  ]
}
```

**Implementation:**
```typescript
import { DurableObject } from "cloudflare:workers";

interface Env {
  YOUR_QUEUE: Queue;
  YOUR_DO_CLASS: DurableObjectNamespace<YourDurableObject>;
}

export default {
  async fetch(req, env, ctx): Promise<Response> {
    const url = new URL(req.url);
    const userIdParam = url.searchParams.get("userId");

    if (userIdParam) {
      const durableObjectStub = env.YOUR_DO_CLASS.getByName(userIdParam);
      const response = await durableObjectStub.fetch(req);
      return response;
    }

    return new Response("userId must be provided", { status: 400 });
  },
} satisfies ExportedHandler<Env>;

export class YourDurableObject extends DurableObject<Env> {
  async fetch(req: Request): Promise<Response> {
    await this.env.YOUR_QUEUE.send({
      id: this.ctx.id.toString(),
    });

    return new Response("wrote to queue");
  }
}
```

---

## Queues with R2 (Batch Error Storage)

**Docs:** https://developers.cloudflare.com/queues/examples/send-errors-to-r2/

This pattern catches JavaScript errors and sends them to a queue, where a consumer batches them and stores them in R2 object storage.

```typescript
interface ErrorMessage {
  message: string;
  stack?: string;
}

interface Env {
  readonly ERROR_QUEUE: Queue<ErrorMessage>;
  readonly ERROR_BUCKET: R2Bucket;
}

export default {
  async fetch(req, env, ctx): Promise<Response> {
    try {
      return doRequest(req);
    } catch (e) {
      const error: ErrorMessage = {
        message: e instanceof Error ? e.message : String(e),
        stack: e instanceof Error ? e.stack : undefined,
      };
      await env.ERROR_QUEUE.send(error);
      return new Response(error.message, { status: 500 });
    }
  },
  async queue(batch, env, ctx): Promise<void> {
    let file = "";
    for (const message of batch.messages) {
      const error = message.body;
      file += error.stack ?? error.message;
      file += "\r\n";
    }
    await env.ERROR_BUCKET.put(`errors/${Date.now()}.log`, file);
  },
} satisfies ExportedHandler<Env, ErrorMessage>;

function doRequest(request: Request): Response {
  if (Math.random() > 0.5) {
    return new Response("Success!");
  }
  throw new Error("Failed!");
}
```

---

## R2 Event Notifications (Event Subscriptions)

**Docs:** https://developers.cloudflare.com/r2/buckets/event-notifications/

Event notifications send messages to your queue when data in your R2 bucket changes. These messages can be consumed via a consumer Worker or HTTP pull requests.

### Event Types

- `object-create`: Triggered when new objects are created or existing objects are overwritten (PutObject, CopyObject, CompleteMultipartUpload)
- `object-delete`: Triggered when an object is explicitly removed from the bucket (DeleteObject, LifecycleDeletion)

### Configuration via CLI

```bash
npx wrangler r2 bucket notification create <BUCKET_NAME> \
  --event-type <EVENT_TYPE> \
  --queue <QUEUE_NAME> \
  --prefix <OPTIONAL_PREFIX> \
  --suffix <OPTIONAL_SUFFIX>
```

### Event Subscription Wrangler Commands

```bash
npx wrangler queues subscription create <QUEUE> --source <SOURCE> --events <EVENTS>
npx wrangler queues subscription list <QUEUE>
npx wrangler queues subscription get <QUEUE> --id <SUBSCRIPTION_ID>
npx wrangler queues subscription update <QUEUE> --id <ID> --name <NAME> --events <EVENTS> --enabled
npx wrangler queues subscription delete <QUEUE> --id <SUBSCRIPTION_ID>
```

### Consumer Worker Example

```typescript
export interface Env {
  LOG_SINK: R2Bucket;
}

export default {
  async queue(batch, env): Promise<void> {
    const batchId = new Date().toISOString().replace(/[:.]/g, "-");
    const fileName = `upload-logs-${batchId}.json`;

    // Serialize the entire batch of messages to JSON
    const fileContent = new TextEncoder().encode(
      JSON.stringify(batch.messages),
    );

    // Write the batch of messages to R2
    await env.LOG_SINK.put(fileName, fileContent, {
      httpMetadata: {
        contentType: "application/json",
      },
    });
  },
} satisfies ExportedHandler<Env>;
```

### Message Structure

Event notification messages include properties like: account ID, action type, bucket name, object details (key, size, eTag), event timestamp, and copySource information (for copy operations only).

### Constraints

- Up to 100 notification rules per bucket
- Queue throughput is limited to 5,000 messages per second
- Overlapping rules triggering multiple notifications for single events are prohibited

---

## Delivery Guarantees

**Docs:** https://developers.cloudflare.com/queues/reference/delivery-guarantees/

Cloudflare Queues implements **at-least-once** delivery by default, prioritizing reliability over speed.

Messages are guaranteed to arrive at least once, though rare duplicate deliveries may occur. Exactly-once delivery requires additional system overhead.

### Handling Duplicates

When duplicate processing would cause problems, developers should:
- Generate unique IDs when writing messages
- Use these IDs as database primary keys or idempotency keys for de-duplication
- Apply idempotency keys with external services (email APIs, payment APIs) to leverage their built-in duplicate rejection

### Message Ordering

Queues does not guarantee that messages will be delivered to a consumer in the same order in which they are published.

---

## Pause and Resume Delivery

**Docs:** https://developers.cloudflare.com/queues/reference/wrangler-commands/

Queue delivery can be paused and resumed, which halts or restarts message delivery to consumers.

```bash
npx wrangler queues pause-delivery <QUEUE-NAME>
npx wrangler queues resume-delivery <QUEUE-NAME>
```

When a queue is paused (disabled), attempting to send messages returns error code 10252 (`QueueDisabled`). You must unpause/resume the queue to restore delivery.

---

## Purge Queue

**Docs:** https://developers.cloudflare.com/queues/reference/wrangler-commands/

Remove all messages from a queue:

```bash
npx wrangler queues purge <QUEUE-NAME> --force
```

The `--force` flag skips the confirmation prompt.

---

## Consumer Management (CLI)

**Docs:** https://developers.cloudflare.com/queues/reference/wrangler-commands/

### Worker Consumers

```bash
npx wrangler queues consumer add <QUEUE-NAME> <SCRIPT-NAME>
npx wrangler queues consumer worker add <QUEUE-NAME> <SCRIPT-NAME>
npx wrangler queues consumer worker remove <QUEUE-NAME> <SCRIPT-NAME>
npx wrangler queues consumer remove <QUEUE-NAME> <SCRIPT-NAME>
```

Worker consumer options: `--batch-size`, `--batch-timeout`, `--message-retries`, `--dead-letter-queue`, `--max-concurrency`, `--retry-delay-secs`

### HTTP Pull Consumers

```bash
npx wrangler queues consumer http add <QUEUE-NAME>
npx wrangler queues consumer http remove <QUEUE-NAME>
```

HTTP consumer options: `--batch-size`, `--message-retries`, `--dead-letter-queue`, `--visibility-timeout-secs`, `--retry-delay-secs`

---

## Local Development

**Docs:** https://developers.cloudflare.com/queues/configuration/local-development/

Queues support local development workflows using Wrangler via Miniflare, which creates a standalone, local-only environment that mirrors the production environment.

**Requirements:**
- Wrangler v3.1.0 or later
- Node.js 18.0.0 or later

**Start local development:**
```bash
npx wrangler@latest dev
```

### Multiple Workers (Producer + Consumer)

For architectures with separate producer and consumer Workers, run both simultaneously:

```
producer-worker/
  wrangler.jsonc
  index.ts
  consumer-worker/
    wrangler.jsonc
    index.ts
```

```bash
npx wrangler@latest dev -c wrangler.jsonc -c consumer-worker/wrangler.jsonc --persist-to .wrangler/state
```

When the producer sends messages, the consumer Worker will automatically be invoked to handle them.

### Limitations

- Consumer concurrency is not supported while running locally
- Queues does not support Wrangler remote mode (`wrangler dev --remote`)

---

## Metrics and Observability

**Docs:** https://developers.cloudflare.com/queues/observability/metrics/

Cloudflare Queues exposes three categories of metrics accessible through the GraphQL Analytics API and the dashboard.

### Backlog Metrics (`queuesBacklogAdaptiveGroups` dataset)

- Average size of the backlog, in bytes
- Average size of the backlog, in number of messages

Filterable by: queueID, datetime, date, datetimeHour, datetimeMinute

### Consumer Concurrency (`queueConsumerMetricsAdaptiveGroups` dataset)

- Average number of concurrent consumers over the period

### Message Operations (`queueMessageOperationsAdaptiveGroups` dataset)

- Total billable operations (writes, reads, deletes)
- Sum of bytes read, written, and deleted from the queue
- Average lag time in milliseconds between when the message was written and the operation to consume the message
- Average number of retries per message
- Maximum message size over the specified period

Filterable by: queueID, actionType (WriteMessage, ReadMessage, DeleteMessage), consumerType (worker or http), outcome (success, dlq, fail), and time dimensions.

### Example GraphQL Queries

**Queue Backlog:**
```graphql
query QueueBacklog(
  $accountTag: string!
  $queueId: string!
  $datetimeStart: Time!
  $datetimeEnd: Time!
) {
  viewer {
    accounts(filter: { accountTag: $accountTag }) {
      queueBacklogAdaptiveGroups(
        limit: 10000
        filter: {
          queueId: $queueId
          datetime_geq: $datetimeStart
          datetime_leq: $datetimeEnd
        }
      ) {
        avg {
          messages
          bytes
        }
      }
    }
  }
}
```

**Consumer Concurrency by Hour:**
```graphql
query QueueConcurrencyByHour(
  $accountTag: string!
  $queueId: string!
  $datetimeStart: Time!
  $datetimeEnd: Time!
) {
  viewer {
    accounts(filter: { accountTag: $accountTag }) {
      queueConsumerMetricsAdaptiveGroups(
        limit: 10000
        filter: {
          queueId: $queueId
          datetime_geq: $datetimeStart
          datetime_leq: $datetimeEnd
        }
        orderBy: [datetimeHour_DESC]
      ) {
        avg {
          concurrency
        }
        dimensions {
          datetimeHour
        }
      }
    }
  }
}
```

**Message Operations by Minute:**
```graphql
query QueueMessageOperationsByMinute(
  $accountTag: string!
  $queueId: string!
  $datetimeStart: Date!
  $datetimeEnd: Date!
) {
  viewer {
    accounts(filter: { accountTag: $accountTag }) {
      queueMessageOperationsAdaptiveGroups(
        limit: 10000
        filter: {
          queueId: $queueId
          datetime_geq: $datetimeStart
          datetime_leq: $datetimeEnd
        }
        orderBy: [datetimeMinute_DESC]
      ) {
        count
        sum {
          bytes
        }
        dimensions {
          datetimeMinute
        }
      }
    }
  }
}
```

---

## Error Codes

**Docs:** https://developers.cloudflare.com/queues/reference/error-codes/

### Client Side Errors

| Code | Error | Details |
|------|-------|---------|
| 10104 | QueueNotFound | Queue does not exist |
| 10106 | Unauthorized | Unauthorized request |
| 10107 | QueueIDMalformed | Queue ID contains invalid characters |
| 10201 | ClientDisconnected | Client disconnected during processing |
| 10202 | BatchDelayInvalid | Invalid batch delay (must be 1-86400 seconds) |
| 10203 | MessageMetadataInvalid | Invalid content type or delay |
| 10204 | MessageSizeOutOfBounds | Message size out of bounds (0-128 KB) |
| 10205 | BatchSizeOutOfBounds | Batch size out of bounds (0-256 KB) |
| 10206 | BatchCountOutOfBounds | Batch count out of bounds (0-100 messages) |
| 10207 | JSONRequestBodyInvalid | Schema mismatch |
| 10208 | JSONRequestBodyMalformed | Invalid JSON syntax |

### Rate Limit / Overload Errors (429-type)

| Code | Error | Details |
|------|-------|---------|
| 10250 | QueueOverloaded | Queue is overloaded; temporarily reduce message sending |
| 10251 | QueueStorageLimitExceeded | Storage limit reached; purge queue or allow backlog processing |
| 10252 | QueueDisabled | Queue paused; unpause delivery |
| 10253 | FreeTierLimitExceeded | Free tier exceeded; upgrade to Workers Paid plan |

### Server Errors (500-type)

| Code | Error | Details |
|------|-------|---------|
| 15000 | UnknownInternalError | Unknown internal error |

---

## Limits

**Docs:** https://developers.cloudflare.com/queues/platform/limits/

| Feature | Limit |
|---------|-------|
| Queues per account | 10,000 |
| Message size | 128 KB |
| Message retries | 100 |
| Maximum consumer batch size | 100 messages |
| Maximum messages per sendBatch call | 100 (or 256 KB total) |
| Maximum batch wait time | 60 seconds |
| Per-queue message throughput | 5,000 messages/second |
| Message retention period | Configurable up to 14 days (Free: 24 hours fixed) |
| Per-queue backlog size | 25 GB |
| Concurrent consumer invocations | 250 (push-based only) |
| Consumer duration (wall clock) | 15 minutes |
| Consumer CPU time | Configurable to 5 minutes (default: 30 seconds via `limits.cpu_ms`) |
| visibilityTimeout (pull-based) | 12 hours |
| delaySeconds | 24 hours |

Notes:
- 1 KB is measured as 1000 bytes
- Messages can include up to ~100 bytes of internal metadata that counts towards total message limits
- Queue consumers have a maximum wall time of 15 minutes per invocation, distinct from CPU time

---

## Pricing

**Docs:** https://developers.cloudflare.com/queues/platform/pricing/

Cloudflare Queues charges for the total number of operations against each of your queues during a given month.

### What Counts as an Operation

- Every 64 KB of written, read, or deleted data = 1 operation
- Messages exceeding 64 KB are charged proportionally (e.g., a 127 KB message = 2 operations)
- 1 KB = 1,000 bytes; messages include ~100 bytes internal metadata
- Charged per-message, not per-batch

### Pricing Tiers

| Plan | Included | Overage Rate |
|------|----------|--------------|
| Workers Free | 10,000 ops/day | N/A |
| Workers Paid | 1,000,000 ops/month | $0.40/million |

### Message Retention

- Free: 24 hours (fixed)
- Paid: 4 days default, up to 14 days configurable

### Additional Operational Charges

- Each retry = 1 read operation
- Dead Letter Queue writes = 1 write operation per 64 KB
- Expired (unread) messages = 1 write + 1 delete operation

### No Egress/Bandwidth Charges

No charges for egress bandwidth.

---

## Rate Limit Handling Pattern

**Docs:** https://developers.cloudflare.com/queues/tutorials/handle-rate-limits/

This pattern uses Queues to respect external API rate limits (e.g., Resend's 2 requests/second) by batching messages accordingly.

**Key approach:**
- Set `max_batch_size` to match the external API rate limit (e.g., 2)
- Queue messages with a 1-second delay to spread them out
- Consumer handler processes batches and calls external API
- Failed sends trigger retries after configurable delays (e.g., 5 seconds)

```typescript
import { Resend } from "resend";

interface Message {
  email: string;
}

export default {
  async fetch(req, env, ctx): Promise<Response> {
    try {
      await env.EMAIL_QUEUE.send(
        { email: await req.text() },
        { delaySeconds: 1 },
      );
      return new Response("Success!");
    } catch (e) {
      return new Response("Error!", { status: 500 });
    }
  },
  async queue(batch, env, ctx): Promise<void> {
    const resend = new Resend(env.RESEND_API_KEY);
    for (const message of batch.messages) {
      try {
        const sendEmail = await resend.emails.send({
          from: "onboarding@resend.dev",
          to: [message.body.email],
          subject: "Hello World",
          html: "<strong>Sending an email from Worker!</strong>",
        });

        if (sendEmail.error) {
          console.error(sendEmail.error);
          message.retry({ delaySeconds: 5 });
        } else {
          message.ack();
        }
      } catch (e) {
        console.error(e);
        message.retry({ delaySeconds: 5 });
      }
    }
  },
} satisfies ExportedHandler<Env, Message>;
```

---

## How Queues Works (Architecture)

**Docs:** https://developers.cloudflare.com/queues/reference/how-queues-works/

### Core Concepts

- **Queue**: An automatically-scaling buffer that stores messages until a consumer retrieves them.
- **Producer**: A Worker that publishes messages to a queue via `send()`.
- **Consumer**: A Worker (push-based) or external service (pull-based) that processes messages.
- **Message**: Any JSON serializable object.

### Key Architectural Properties

- Messages are guaranteed not to be lost once successfully written.
- Messages will not be deleted until the consumer completes processing.
- Messages are NOT guaranteed to be delivered in the same order as published.
- Multiple producer Workers can write to a single queue without limit.
- Each queue can only have one active consumer.
- A single consumer Worker can process multiple queues by switching on the queue name.
- Messages support two primary content types: JSON (default) and text.
- The system batches messages during delivery, treating batches as atomic units for retry logic.

### Consumer Types

1. **Worker consumers (push-based)**: Workers activate when the queue contains messages. Automatically scale horizontally.
2. **HTTP pull consumers (pull-based)**: External services retrieve messages via HTTP endpoints. Provide explicit control over consumption rate.

---

## Dashboard Operations

**Docs:** https://developers.cloudflare.com/queues/examples/list-messages-from-dash/
**Docs:** https://developers.cloudflare.com/queues/examples/send-messages-from-dash/

The Cloudflare Dashboard provides UI for:
- Fetching and acknowledging messages currently in a queue
- Sending messages to a queue directly from the dashboard

---

## Wrangler Global Flags

**Docs:** https://developers.cloudflare.com/queues/reference/wrangler-commands/

Available for all wrangler queues commands:

- `--v` / `--version`: Display version
- `--cwd`: Specify working directory
- `--config` / `--c`: Configure file path
- `--env` / `--e`: Environment selection
- `--env-file`: Load environment variables
- `--experimental-provision` / `--x-provision` (default: true)
- `--experimental-auto-create` / `--x-auto-create` (default: true)
