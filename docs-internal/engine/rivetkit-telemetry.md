# RivetKit telemetry

Architecture and operational invariants for RivetKit traces, invocation metrics, and log correlation. Core owns invocation telemetry; adapters only bridge the current Core context to their host runtime. Pair with `napi-bridge.md` for the adapter boundary and `rivetkit-core-internals.md` for surrounding lifecycle context.

## What it produces

Three span shapes, all on the `rivetkit::telemetry` tracing target:

| Span | When | Kind | Parent |
| --- | --- | --- | --- |
| `{actor}/{action}` | an action runs | `server` | remote `traceparent` if valid, else root |
| `{actor}/{action}` | a schedule or cron fires | `internal` | fresh root, plus one link to the defining invocation |
| `rivet.sqlite.{operation}` | any `c.db` call | `internal` | the current invocation |

Invocation spans carry `rivet.invocation.type`, `rivet.actor.id`, `rivet.actor.name`, `rivet.actor.key`, `rivet.action.name`, `rivet.ray.id`, `otel.status_code`, and `error.type` on failure. SQLite spans carry the same actor identity plus `rivet.operation.system` and `rivet.operation.name`.

Two metrics, labelled by actor name, action name, invocation type, and status:

- `rivetkit_actor_invocations_total`
- `rivetkit_actor_invocation_duration_seconds`

Pino binds actor ID, name, key, and ray ID on every actor log line. Sampled work also binds `trace_id` and `span_id`, in snake case because that is what OTel log-correlation tooling looks for.

### Why action names are bounded

Action names arrive from the caller on the URL path and are never validated against the registry before dispatch. An undeclared name becomes `_OTHER`, following the OpenTelemetry semantic convention for unknown caller-supplied values (`http.request.method` uses the same fallback). This applies to the metric label **and** to the span name, because a backend that derives metrics from span names turns an unbounded name into a new series exactly as a label would. `ActorMetrics::label_action_name` is the single source for both.

### Why SQLite spans parent to the invocation, not the surrounding application span

Core creates these spans in Rust and cannot see the JavaScript span stack. A `c.db` call made while an application span is active therefore appears as a sibling of that span under the invocation, not as its child. This is the visible consequence of the two-pipeline model below, and it was accepted deliberately: reading the JS context from Rust on every query would mean a context lookup across the NAPI boundary per call.

## Turning it on

Set these on the process that loads RivetKit, which is the actor runner. On Rivet Cloud that is the same machine and the same environment block as `RIVET_ENDPOINT`; the dashboard only displays that value for you to copy, so both go wherever your platform keeps environment variables.

```sh
OTEL_SERVICE_NAME=internal-api
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://collector:4318/v1/traces
OTEL_EXPORTER_OTLP_TRACES_PROTOCOL=http/protobuf
```

The endpoint is illustrative; use the team's receiver and check which format and authentication it expects. `grpc` targets the collector's 4317 port rather than 4318. `OTEL_EXPORTER_OTLP_HEADERS` and `OTEL_EXPORTER_OTLP_TRACES_HEADERS` are both honored, which is what makes hosted backends work.

Application spans come from the application's own OpenTelemetry SDK pointed at the same collector. The two pipelines share only trace context, and that is sufficient: Core invocation and SQLite spans arrive from the Rust exporter, application spans arrive from the JavaScript SDK, and the collector joins them on trace ID. No custom JavaScript exporter or span bridge is involved. Bridging the two into one pipeline was considered and deferred; it buys a single export path and richer nesting, at the cost of RivetKit owning span export on behalf of the application.

### Two ways this fails silently

Both look like broken RivetKit tracing rather than application misconfiguration, so they belong in any user-facing doc.

- **Configuring `NodeSDK` in code and nothing else.** That configures the JavaScript pipeline only. The Rust side reads the environment, so the result is application spans with no RivetKit spans and no error anywhere. Configure both through the same environment variables; `NodeSDK` reads them too.
- **No context manager registered.** `runWithActorInvocationSpan` activates the Core span with `context.with(...)` from `@opentelemetry/api`, which delegates to the globally registered context manager. With none registered the API uses a no-op manager that invokes the callback and stores nothing, so `context.active()` inside the action returns the root context and every application span becomes its own trace root. `NodeSDK.start()` and `NodeTracerProvider.register()` register one; a bare `BasicTracerProvider` plus `trace.setGlobalTracerProvider(...)` does not.

A hand-built JavaScript provider also needs its own resource. `OTEL_SERVICE_NAME` configures the Rust exporter only, so a provider without `service.name` reports `unknown_service:node` and files the two halves of one trace under different services.

### Why RivetKit does not register a context manager itself

Doing so would take ownership of global context away from the application, and it would not generalize beyond Node.

## Overhead characteristics

Absolute figures belong in a benchmark artifact, not here; they change with every build and host. Three properties of the design do not.

- **Sampling does not remove the latency cost.** The span is constructed in `tracing` before `tracing-opentelemetry` runs the sampler, so a sampled-out invocation pays nearly the same request latency as a fully traced one. Operators reaching for `OTEL_TRACES_SAMPLER` to cut latency will not get it; sampling reduces export volume, not construction.
- **Export cost lands in CPU, not latency.** The batch processor ships spans from a background thread, so enabling export raises worker CPU per invocation while leaving request latency close to the sampled-out case.
- **Spans are dropped, not queued indefinitely.** The batch processor holds `OTEL_BSP_MAX_QUEUE_SIZE` spans, 2,048 by default, and discards on overflow. Raising it absorbs bursts but only delays overflow when span production sustainably exceeds export throughput, and costs memory. Drops surface as `BatchSpanProcessor.SpanDroppingStarted` in the actor logs through the SDK log bridge, which is the only reason they are visible at all.

`rivetkit_actor_invocation_duration_seconds` uses `MICRO_BUCKETS`. Invocations land in the hundreds of microseconds, which the Prometheus default buckets, starting at 5 ms, collapse into a single bucket.

## Core lifecycle

- `ActorInvocation` owns one `rivet.actor.invoke` span, its timer, its metric labels, and exactly-once completion. Both action and schedule dispatch create it before enqueueing.
- Rejected enqueue attempts count as `status=error`. Unfinished invocations finish as `actor.dropped_reply`. Raw error messages are never recorded, only bounded `group.code` identity.
- A scheduled fire whose reply channel closes records that same `actor.dropped_reply` identity in `_rivet_schedule_history`, so the invocation span, the invocation metric, and what `cronHistory()` returns all agree.
- The span kind is derived from the invocation type rather than passed alongside it, so the two cannot disagree.
- Span and metric completion share `ActorInvocation`, so the action and schedule paths cannot drift when tracing is disabled or sampled out.

Export layers enable the `rivetkit::telemetry` target and log layers filter it off. That keeps diagnostic Rust spans out of application traces and keeps OTel spans from duplicating log context.

## Context propagation

Core accepts `x-rivetkit-ray-id`, `traceparent`, and `tracestate`. Rays must be 1 to 128 characters from `[A-Za-z0-9_-]`; absent or invalid rays become UUIDs. Invalid W3C context fails closed to a root span and never rejects an action.

Outbound actor calls resolve context at send time, preferring the active `@opentelemetry/api` span and falling back to the current Core invocation for an actor-owned client. An application span is the more specific parent when one is active, so the callee nests under the work that actually issued the call rather than under the whole invocation. Those headers replace static client telemetry headers, so configuration cannot pin stale context.

- `@opentelemetry/api` is a hard dependency at `^1.1.0`, the lowest minor that exports every symbol used, and it is inert without a provider. It is needed both to read outbound application spans and to activate inbound Core spans; making it optional would silently separate traces.
- Retained databases, clients, and schedule handles resolve the current invocation from `AsyncLocalStorage`, but only when it belongs to the same Core actor generation. Otherwise they use their creation context. Pointer identity through `Arc::ptr_eq`, rather than actor ID equality, is what isolates overlapping calls and restarted generations.
- KV and queue operations do not resolve the invocation, because Core attaches telemetry only to SQLite and schedule work.
- `c.log` stays creation-scoped because a Pino child is immutable correlation metadata, not an operation resolved on every write. Do not retain an action logger for later work.

### Why accepting caller trace context at an untrusted edge is acceptable

This follows the W3C Trace Context model: a caller can name any trace and can set the sampled flag, so an actor's traces are only as trustworthy as the callers that can reach it. Trace context carries no identity and no authorization, and every value derived from it is bounded before it reaches a span or a label. An operator who does not trust their callers should strip `traceparent`, `tracestate`, and `x-rivetkit-ray-id` at their own edge.

## Scheduled work

- Creating or redefining a one-shot, interval, or cron schedule captures the current ray, `traceparent`, and `tracestate`. Recurring re-registration refreshes that context even when cadence is unchanged; preserving cadence must not preserve the identity of an older definer.
- Schedule context is a versioned BARE value in the existing `_rivet_meta` table, keyed by schedule ID, following the `run_wake_at` precedent. It is written in the same batch as the schedule row and removed in the same batch as the row delete, guarded so a delete that misses the row leaves the context alone. Only the due-schedule query reads it.
- Malformed values are logged and ignored, and their W3C fields fail closed through the same parser used for actions.

### Why this does not bump the internal schema version

Older runtimes ignore the metadata, so a schedule redefined by an older runtime keeps the previous definer's context until a newer runtime redefines it again. Telemetry degrades to best effort across that sequence without affecting schedule behavior.

### Why a scheduled fire starts a new trace

It keeps the defining invocation's ray, because a ray means the causal path of the work. Its OTel span is still a fresh trace root linked to the precise defining span: rays give broad correlation even when spans are absent or sampled out, while the span link records the standard durable asynchronous relationship. A cron running for a year would otherwise be one trace with millions of spans. Actor calls made by the fire propagate that same causal ray and the fire's new W3C context.

## Native export

NAPI owns the Rust `SdkTracerProvider`. Native export is enabled by standard `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` or `OTEL_EXPORTER_OTLP_ENDPOINT`, unless standard SDK disable or exporter controls turn it off. Sampling, resources, service name, and batching all use standard OTel environment variables. There is no RivetKit sampler, rate limiter, registry field, or exporter switch. Shutdown performs a bounded best-effort flush, and export failures cannot fail actor work.

- **Protocol selection is read here, not left to the exporter.** `opentelemetry-otlp` takes its default from a compile-time constant chosen by the enabled cargo features, and neither of its builders reads `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL` or `OTEL_EXPORTER_OTLP_PROTOCOL`. Enabling `http-json` would otherwise pin every deployment to JSON whatever an operator set. `configured_protocol` reads both variables and accepts all three OTLP values: `grpc`, `http/protobuf`, and `http/json`. Anything else errors naming the value. The default is `http/protobuf`, which the specification lists as a usual SDK default and which most collectors and hosted backends expect. gRPC matters because the Engine's own exporter speaks it, so a deployment running one collector on 4317 does not need a second receiver for RivetKit.
- **The SDK log bridge forwards the SDK's own diagnostics to the JavaScript logger.** A `tracing` layer filtered to `opentelemetry_sdk=warn` hands events to a `ThreadsafeFunction`, so dropped-span warnings appear in Pino alongside everything else the actor logs instead of on stdout in a different format. `internal-logs` must stay enabled on both `opentelemetry` and `opentelemetry_sdk`, or `otel_warn!` compiles to nothing and the bridge goes silent with no error.
- **That threadsafe function is unreferenced once created.** A referenced one counts as live work on the Node event loop, and `createRegistry` registers the sink on every registry, so any process that registered it would never exit on its own. Unreferencing keeps warnings flowing without making the sink a reason to keep running.
- **The addon export is called defensively.** `@rivetkit/rivetkit-napi` ships as its own per-platform package and can be older than the JavaScript calling it, so a missing sink logs a warning rather than failing registry construction. A logging hook must not be able to stop an actor from starting.

## Data policy

Allowed: actor ID, name, and key; declared action name; invocation type; bounded status; ray, trace, and span IDs; and error identity as `group.code`.

Never recorded: action arguments or results, connection parameters, SQL text or bindings, actor state, arbitrary headers, or raw error messages.

Event-category opt-in is a separate application-facing design. It is not inferred from sampling or from registry configuration.

## Not covered

- Dedicated spans for raw fetch and WebSocket handlers, lifecycle hooks, connection callbacks, KV, or actor-state operations.
- Connection and inspector actions inheriting the initiating connection trace.
- A public automatic-tracing category opt-in API.
- Engine-owned ray stamping, public invocation tokens, runtime-specific span lifecycles, or custom sampling controls.
- Wasm host span export. The Wasm adapter runs actions without invocation context and reports no trace context; only rays are retained.
- Integration entry points. There is no public API for attaching an application span to an outbound action, so the Effect bridge and `rivetkit/unstable/otel` are not part of this work.
