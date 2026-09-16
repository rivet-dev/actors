# RivetKit telemetry

Internal reference for actor traces and log correlation. Core owns telemetry behavior. Runtime adapters activate Core's context in the host language.

See [NAPI bridge](napi-bridge.md) for binding conventions and [Core internals](rivetkit-core-internals.md) for actor dispatch and lifecycle wiring.

## Ownership

Telemetry crosses two runtimes but keeps one trace:

```text
client
  headers: ray ID + W3C trace context
    → rivetkit-core
        invocation spans + native operation spans
          → NAPI
              activates the invocation context in TypeScript
                → application spans
```

- `rivetkit-core::telemetry` owns spans and completion
- NAPI only translates context between Core and TypeScript
- Rust and TypeScript export through their own OpenTelemetry SDKs

Core spans use the `rivetkit::telemetry` tracing target. Log layers exclude this target, and the export layer excludes unrelated diagnostic spans.

## Telemetry surfaces

| Work | Span | Kind | Relationship |
| --- | --- | --- | --- |
| Action | `{actor}/{action}` | `server` | Child of incoming context |
| Schedule | `{actor}/{action}` | `internal` | New trace linked to its origin |
| Raw HTTP | `{actor}/onRequest` | `server` | Child of incoming context |
| Queue send | `{actor}/queue.send` | `producer` | Child of incoming context |
| Queue receive | `{actor}/queue.receive` | `consumer` | Linked to the send origin |
| Actor call | `{callee}/{action}` | `client` | Child of application or invocation span |
| SQLite | `rivet.sqlite.{operation}` | `internal` | Child of application or invocation span |

Core records these attributes:

| Scope | Attributes |
| --- | --- |
| Actor | `rivet.actor.id`, `rivet.actor.name`, `rivet.actor.key` |
| Correlation | `rivet.ray.id` |
| Invocation | `rivet.invocation.type`, `otel.status_code`, `error.type` |
| Action | `rivet.action.name` |
| HTTP | `http.request.method`, `http.response.status_code` |
| Queue | `rivet.queue.name` |
| SQLite | `rivet.operation.system`, `rivet.operation.name` |

Raw HTTP spans use `onRequest`, never the request path. Handler errors use their `group.code` as `error.type`. A 5xx response uses the status code. Abandoned SQLite and actor-call tracking uses `actor.operation_abandoned` to represent an unknown outcome.

## Logs

Actor loggers include actor identity and ray ID. A valid span also adds `traceId` and `spanId`. Do not retain an invocation logger for unrelated work.

## Context propagation

```text
incoming headers
  x-rivet-ray-id ───────────────────────────────┐
  traceparent + tracestate ──→ invocation span ├─→ actor call / HTTP / queue
                                               │
application span ──────────────────────────────┘ preferred parent

schedule or queue send ── stores origin ── later execution links to origin
```

Core accepts correlation headers on actions, raw HTTP requests, and queue sends.

- Ray IDs match `[A-Za-z0-9_-]` and contain 1–128 characters
- RivetKit propagates ray IDs but does not create them
- Invalid W3C context starts a root span without rejecting the request
- Explicit HTTP trace headers override generated headers as one pair
- Per-call context overrides static client telemetry headers
- Application spans take precedence over the invocation span as outbound parents

The Engine gateway supplies its guard ray ID when the caller sends none. External clients read `rivet.ray.id` from OpenTelemetry baggage; a configured `x-rivet-ray-id` header is the fallback in both clients.

Rust owns ray ID validation in `rivetkit-client-protocol::telemetry_headers`. Core and the Rust client use `TraceContextPropagator`. TypeScript handles context in `common/otel-context.ts`.

Caller trace context provides correlation, not identity or authorization. Strip incoming correlation headers at an untrusted boundary when callers must not select these values.

## Invocation lifetime

```text
dispatch
  → start span and timer
  → run handler with context
  → send reply
  → wait for tracked waitUntil work
  → end span
```

`ActorInvocation` completes once. A reply dropped unsent finishes the span with `otel.status_code` `ERROR` and `error.type` `actor.dropped_reply`. A dispatch the actor task refuses is answered before any span exists. `c.keepAwake` remains part of the handler, while `waitUntil` may extend the span beyond the reply.

Schedules and queue messages store their ray ID and W3C origin in `origin_*` columns of their own rows, added by internal schema migration v2. Each schedule fire starts a new linked trace. Each queue receipt links to the send origin.

## Native export

The `native-runtime` feature enables Core's exporter. Hosts attach `telemetry::export::layer()` and call `shutdown_best_effort()` during shutdown.

- An OTLP endpoint enables export
- Protocol selection supports `grpc`, `http/protobuf`, and `http/json`
- Export uses a bounded background queue and never fails actor work
- NAPI forwards OpenTelemetry SDK warnings, such as dropped spans, to the JavaScript logger through `setTelemetryLogSink`. `createRegistry()` installs the sink and `shutdownTelemetry()` releases it. While a sink is installed the Rust log layers skip those warnings, and without one they print through the Rust log layers, so each warning is visible once

## Data policy

Record actor identity, invocation type, HTTP method and status, correlation IDs, operation names, and error identity. Do not record arguments, results, connection parameters, SQL text or bindings, actor state, arbitrary headers, or raw error messages.

## Gaps

- WebSocket handlers, lifecycle hooks, connection callbacks, KV, and actor-state operations have no dedicated spans
- WebSocket action messages and inspector actions do not inherit caller context
- Actor creation ray IDs do not reach the actor runtime
- Wasm does not export host spans or expose Core invocation context to TypeScript
