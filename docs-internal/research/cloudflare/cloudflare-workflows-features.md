# Cloudflare Workflows -- Comprehensive Feature Reference

> **As of:** 2026-03-06
>
> **Cloudflare basis:** Official Workflows docs accessed 2026-03-06. Cloudflare documents this as a live platform surface rather than a semver'd package.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Migration framing:** Rivet has a native workflow engine inside actors. This is one of the strongest migration fits in the set, but the API surface differs: Rivet models durable steps, sleeps, queue waits, and replay history inside an actor rather than through `WorkflowEntrypoint` bindings and `WorkflowInstance` control APIs.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Workflow Definition (`WorkflowEntrypoint`) | Extend `WorkflowEntrypoint` class with a `run` method to define durable workflows | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Low | Use `run: workflow(async (ctx) => { ... })`. |
| `step.do()` -- Execute and Persist a Step | Execute an async callback and persist its return value as a retryable unit of work | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Low | `ctx.step(...)` is the direct analog. |
| `step.sleep()` -- Pause for a Relative Duration | Hibernate workflow execution for a specified duration without consuming CPU | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Low | Use `ctx.sleep(...)`. |
| `step.sleepUntil()` -- Pause Until a Fixed Date | Pause execution until a specific point in time via `Date` object or UNIX timestamp | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/workflow/timer.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/timer.ts) | Low | Use `ctx.sleepUntil(...)`. |
| `step.waitForEvent()` -- Wait for External Events | Block execution until a matching event is received or a timeout expires | partial | high | [Workflows](https://rivet.dev/docs/actors/workflows), [Queues & Run Loops](https://rivet.dev/docs/actors/queues) | [examples/sandbox/src/actors/workflow/approval.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/approval.ts) | Medium | Model waits via workflow queue receives or actor actions/HTTP handlers that feed the workflow. |
| Sending Events to Running Instances (`sendEvent`) | Deliver data to a paused workflow instance via `sendEvent` on `WorkflowInstance` | partial | medium | [Workflows](https://rivet.dev/docs/actors/workflows), [Communicating Between Actors](https://rivet.dev/docs/actors/communicating-between-actors) | [examples/sandbox/src/actors/workflow/approval.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/approval.ts) | Medium | Rivet's equivalent is usually queue send, actor action, or client message into the owning actor. |
| Retry Configuration (`WorkflowStepConfig`) | Configure per-step retry limits, delays, backoff strategies, and timeouts | native | medium | [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/workflow/history-examples.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/history-examples.ts) | Medium | Rivet workflows support step timeouts/retries, but validate backoff behavior if you relied on Cloudflare defaults exactly. |
| Error Handling (`NonRetryableError`) | Immediately fail a step without retrying using `NonRetryableError` | partial | medium | [Errors](https://rivet.dev/docs/actors/errors), [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-error-handling.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-error-handling.ts) | Medium | Rivet has rich errors, but there is no documented `NonRetryableError` exact analogue in the docs. |
| Triggering Workflows (Workers Binding) | Trigger workflows from Worker scripts via bindings in fetch, queue, cron, or DO handlers | native | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [JavaScript Client](https://rivet.dev/docs/clients/javascript) | [examples/cloudflare-workers/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/index.ts) | Low | Trigger via `createClient()` or normal HTTP entrypoints. |
| Passing Data to Workflows (Event Payloads) | Pass typed data to workflows via the `params` property of `create()` | native | high | [Actor Input](https://rivet.dev/docs/actors/input), [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/workflow/workflow-fixtures.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/workflow-fixtures.ts) | Low | Use actor input and workflow state. |
| Instance Management (`create`, `get`, `status`) | Create, retrieve, batch-create, and query status of workflow instances | partial | high | [JavaScript Client](https://rivet.dev/docs/clients/javascript), [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Medium | Handles and inspector history exist, but the control API is actor-centric rather than WorkflowInstance-centric. |
| Pause and Resume Instances | Explicitly pause and later resume running workflow instances | pattern | medium | [Workflows](https://rivet.dev/docs/actors/workflows), [State](https://rivet.dev/docs/actors/state) | [examples/sandbox/src/actors/workflow/approval.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/approval.ts) | Medium | Use a paused flag plus queue/action resume semantics. |
| Terminate and Restart Instances | Forcibly stop or restart workflow instances from the beginning | partial | medium | [Destroy](https://rivet.dev/docs/actors/destroy), [Versions](https://rivet.dev/docs/actors/versions) | Docs-only | Medium | Restart/cancel behavior exists, but not with the same product API surface. |
| Nested / Child Workflows | Trigger child workflows from a parent workflow without blocking | pattern | high | [Communicating Between Actors](https://rivet.dev/docs/actors/communicating-between-actors), [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/workflow/order.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/order.ts) | Medium | Use actor-to-actor orchestration or queue delegation. |
| State Passing Between Steps | Pass state between steps via persisted return values from `step.do()` | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Low | Strong fit. |
| Configuration (`wrangler.jsonc` / `wrangler.toml`) | Configure workflow bindings, names, and class mappings in wrangler config | pattern | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers/wrangler.json](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/wrangler.json) | Low | Config lives in Rivet actor registry plus any host-platform config. |
| Cross-Script Workflow Bindings | Bind to workflows defined in different Worker scripts using `script_name` | pattern | high | [JavaScript Client](https://rivet.dev/docs/clients/javascript), [Communicating Between Actors](https://rivet.dev/docs/actors/communicating-between-actors) | Docs-only | Low | Cross-service invocation is just client/server communication between Rivet-backed services. |
| Configuring CPU and Subrequest Limits | Increase CPU time and subrequest limits via wrangler configuration | partial | medium | [Limits](https://rivet.dev/docs/actors/limits), [Lifecycle](https://rivet.dev/docs/actors/lifecycle) | Docs-only | Medium | Rivet exposes timeouts and runtime limits, but not the same Workers CPU/subrequest model. |
| Rules and Best Practices | Guidelines for idempotency, granular steps, determinism, and state management | native | high | [Workflows](https://rivet.dev/docs/actors/workflows), [Design Patterns](https://rivet.dev/docs/actors/design-patterns) | [examples/sandbox/src/actors/workflow/history-examples.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/history-examples.ts) | Low | Strong fit. |
| Human-in-the-Loop Pattern | Combine `waitForEvent` with external approval for human-driven workflow steps | native | high | [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/workflow/approval.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/approval.ts) | Low | Approval flows map well. |
| Wrangler CLI Commands | CLI commands for managing workflows and instances via wrangler | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | No equivalent workflow control-plane CLI exists. |
| Limits | Plan-based limits for script size, step count, concurrency, and retention | partial | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | Medium | Limits differ and need new capacity review. |
| Pricing | Billing based on CPU time, requests/invocations, and storage | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Docs-only | Low | Commercial comparison, not runtime parity. |
| Event Subscriptions | Receive messages when workflow lifecycle events occur via queues | pattern | medium | [Realtime](https://rivet.dev/docs/actors/events), [Connections](https://rivet.dev/docs/actors/connections) | [examples/sandbox/src/actors/workflow/dashboard.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/dashboard.ts) | Medium | Use connection events, actor state, or explicit queue/event fanout. |
| Workflow Visualizer | Visual diagram of workflow steps, conditionals, and loops on the dashboard | unsupported | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Medium | Rivet exposes workflow history, not a Cloudflare-style visualizer. |
| Local Development | Local emulated workflow environment via `wrangler dev` | native | high | [Testing](https://rivet.dev/docs/actors/testing), [Workflows](https://rivet.dev/docs/actors/workflows) | [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) | Low | Strong fit. |
| Metrics and Analytics | GraphQL-based metrics for workflow and step-level event tracking | partial | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Medium | Inspector history/logging exist, but not Cloudflare workflow analytics. |
| Calling Workflows from Pages | Trigger workflows from Cloudflare Pages Functions via service bindings or HTTP | pattern | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [JavaScript Client](https://rivet.dev/docs/clients/javascript) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | Low | Any HTTP/backend caller can trigger the actor that owns the workflow. |
| Durable AI Agents | Combine workflows with Agents SDK for durable AI agent execution | pattern | medium | [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors), [Workflows](https://rivet.dev/docs/actors/workflows) | [examples/sandbox/src/actors/ai/ai-agent.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/ai/ai-agent.ts) | Medium | Rivet has the workflow substrate, but not an Agents SDK equivalent. |
| Glossary | Definitions for durable execution, event, instance, step, and workflow | out-of-scope | high | [Workflows](https://rivet.dev/docs/actors/workflows) | Docs-only | Low | Terminology mismatch is documentation-only. |

## High-Risk Behavioral Deltas

- **The core durability model maps well, but the API surface does not.** Port workflow logic, not control-plane assumptions about bindings, CLI, or visualizers.
- **External event waits are queue/action driven.** In Rivet, waiting for external signals is usually a queue receive or action-triggered resume inside the owning actor.
- **Inspector history is the main workflow observability surface.** There is no first-class visualizer equivalent today.
- **Step-only access rules matter.** Rivet workflow docs explicitly restrict `state`, `db`, and `client()` to step contexts; do not assume general actor context is always available everywhere in the workflow.
- **Termination/restart semantics need explicit migration design.** If the Cloudflare system relies on explicit pause/resume/cancel APIs, define the corresponding actor control actions up front.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Durable workflow steps replay safely | Step history survives restarts and replay | Pass: [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts) |
| Sleep and sleepUntil work | Workflow pauses and resumes on time | Pass: [actor-workflow.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-workflow.ts), [examples/sandbox/src/actors/workflow/timer.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/timer.ts) |
| External approval flow works | Workflow can pause waiting for outside input and continue correctly | Pass: [examples/sandbox/src/actors/workflow/approval.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/approval.ts) |
| Step retries/timeouts meet workload needs | Slow/flaky dependencies are retried with acceptable behavior | Partial: [examples/sandbox/src/actors/workflow/history-examples.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/workflow/history-examples.ts) |
| Workflow status is observable | Inspector exposes workflow history/status for operations and debugging | Pass: [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) |
| Pause/resume/cancel API is explicit | Migration defines actor control actions for operational control | Gap: use [Workflows](https://rivet.dev/docs/actors/workflows) plus app-defined actor control actions; no first-class workflow control API is documented |
| Dashboard visualizer dependence is removed | Team accepts inspector/history-based debugging instead of a native graph UI | Fail: [Debugging](https://rivet.dev/docs/actors/debugging) documents inspector/history endpoints, not a first-class workflow graph UI |

---

> Source: [Cloudflare Workflows Documentation](https://developers.cloudflare.com/workflows/)
>
> Purpose: Map each Cloudflare Workflows feature to its Rivet Actor equivalent.

---

## Workflow Definition (WorkflowEntrypoint)

**Docs:** https://developers.cloudflare.com/workflows/build/workers-api/

A Workflow is defined by extending the `WorkflowEntrypoint` class and implementing a `run` method. The `run` method receives a `WorkflowEvent` (with payload, timestamp, and instanceId) and a `WorkflowStep` object for defining durable steps. A Workflow must contain at least one `step` call to be valid.

```typescript
import { WorkflowEntrypoint, WorkflowStep } from "cloudflare:workers";
import type { WorkflowEvent } from "cloudflare:workers";

type Params = { name: string };

export class MyWorkflow extends WorkflowEntrypoint<Env, Params> {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    let someComputedState = await step.do("my step", async () => {});

    // Optional: return state from our run() method
    return someComputedState;
  }
}
```

The `WorkflowEvent` type is defined as:

```typescript
export type WorkflowEvent<T> = {
  payload: Readonly<T>;
  timestamp: Date;
  instanceId: string;
};
```

The event payload is immutable -- changes to `event.payload` are not persisted across steps. State should be stored by returning values from `step.do()` callbacks.

---

## step.do() -- Execute and Persist a Step

**Docs:** https://developers.cloudflare.com/workflows/build/workers-api/

`step.do()` executes an asynchronous callback and persists its serializable return value. Each step is a self-contained, individually retryable unit of work. If the Workflow engine restarts, already-completed steps are replayed from their persisted results (cache key is the step name).

```typescript
export class MyWorkflow extends WorkflowEntrypoint<Env, Params> {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const data = await step.do("fetch data", async () => {
      const response = await fetch("https://api.cloudflare.com/client/v4/ips");
      return await response.json<IPResponse>();
    });

    const result = await step.do(
      "process data",
      { retries: { limit: 3, delay: "5 seconds", backoff: "linear" } },
      async () => {
        return {
          name: event.payload.name,
          ipCount: data.result.ipv4_cidrs.length,
        };
      },
    );

    return result;
  }
}
```

`step.do()` accepts an optional `WorkflowStepConfig` as its second argument (before the callback) to configure retries and timeouts.

---

## step.sleep() -- Pause for a Relative Duration

**Docs:** https://developers.cloudflare.com/workflows/build/sleeping-and-retrying/

`step.sleep()` pauses Workflow execution for a specified duration. The Workflow hibernates during sleep and does not consume CPU. Accepts milliseconds (number) or a human-readable duration string.

```typescript
await step.sleep("sleep for a bit", "1 hour")
```

Supported human-readable duration units:

```
"second" | "minute" | "hour" | "day" | "week" | "month" | "year"
```

Maximum sleep duration is 365 days.

Sleep does not count toward the step limit (1,024 steps per Workflow).

---

## step.sleepUntil() -- Pause Until a Fixed Date

**Docs:** https://developers.cloudflare.com/workflows/build/sleeping-and-retrying/

`step.sleepUntil()` pauses execution until a specific point in time. Accepts a `Date` object or a UNIX timestamp in milliseconds.

```typescript
// sleepUntil accepts a Date object as its second argument
const workflowsLaunchDate = Date.parse("24 Oct 2024 13:00:00 UTC");
await step.sleepUntil("sleep until X times out", workflowsLaunchDate)
```

The system prioritizes resuming sleeping instances over newly queued ones to prevent blocking older Workflows.

---

## step.waitForEvent() -- Wait for External Events

**Docs:** https://developers.cloudflare.com/workflows/build/events-and-parameters/

`step.waitForEvent()` blocks execution until a matching event is received from an external source, or until a configurable timeout expires. This enables human-in-the-loop patterns, webhook-driven flows, and coordination between systems.

```typescript
export class MyWorkflow extends WorkflowEntrypoint<Env, Params> {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    let stripeEvent = await step.waitForEvent<IncomingStripeWebhook>(
      "receive invoice paid webhook from Stripe",
      { type: "stripe-webhook", timeout: "1 hour" },
    );
  }
}
```

Default timeout is 24 hours. Configurable between 1 second and 365 days.

The `type` parameter supports letters, digits, `-`, and `_` (periods are not supported and will produce `workflow.invalid_event_type` error).

When a timeout occurs, the Workflow throws an error unless wrapped in try-catch:

```typescript
try {
  const event = await step.waitForEvent("wait for approval", {
    type: "approval",
    timeout: "1 hour",
  });
} catch (e) {
  console.log("No approval received, proceeding with default action");
}
```

Instances in a `waiting` state do not count towards concurrency limits.

---

## Sending Events to Running Instances (sendEvent)

**Docs:** https://developers.cloudflare.com/workflows/build/events-and-parameters/

Events are sent to a running instance via the `sendEvent()` method on a `WorkflowInstance`. This pairs with `step.waitForEvent()` to deliver data to a paused Workflow.

```typescript
export default {
  async fetch(req: Request, env: Env) {
    const instanceId = new URL(req.url).searchParams.get("instanceId");
    const webhookPayload = await req.json<Payload>();

    let instance = await env.MY_WORKFLOW.get(instanceId);
    await instance.sendEvent({
      type: "stripe-webhook",
      payload: webhookPayload,
    });

    return Response.json({
      status: await instance.status(),
    });
  },
};
```

Events can also be sent via the Wrangler CLI:

```
npx wrangler workflows instances send-event <WORKFLOW_NAME> <INSTANCE_ID> --type <EVENT_TYPE> --payload <JSON>
```

---

## Retry Configuration (WorkflowStepConfig)

**Docs:** https://developers.cloudflare.com/workflows/build/sleeping-and-retrying/

Each `step.do()` call can be configured with retry policies and timeouts. If no config is provided, defaults apply.

```typescript
export type WorkflowStepConfig = {
  retries?: {
    limit: number;
    delay: string | number;
    backoff?: WorkflowBackoff;
  };
  timeout?: string | number;
};
```

Default configuration:

```typescript
const defaultConfig: WorkflowStepConfig = {
  retries: {
    limit: 5,
    delay: 10000,
    backoff: 'exponential',
  },
  timeout: '10 minutes',
};
```

Custom example:

```typescript
let someState = await step.do("call an API", {
  retries: {
    limit: 10, // The total number of attempts
    delay: "10 seconds", // Delay between each retry
    backoff: "exponential" // Any of "constant" | "linear" | "exponential";
  },
  timeout: "30 minutes",
}, async () => { /* Step code goes here */ })
```

- `retries.limit` supports `Infinity` for unlimited retries.
- `retries.delay` accepts milliseconds (number) or human-readable duration strings.
- `retries.backoff` can be `"constant"`, `"linear"`, or `"exponential"`.
- `timeout` is per-attempt, not total. Maximum recommended is 30 minutes. Use `step.waitForEvent()` for longer waits.

---

## Error Handling (NonRetryableError)

**Docs:** https://developers.cloudflare.com/workflows/build/sleeping-and-retrying/

`NonRetryableError` immediately fails a step without retrying. If not caught in a try-catch, the entire Workflow instance enters the "errored" state.

```typescript
import { WorkflowEntrypoint, WorkflowStep, WorkflowEvent } from 'cloudflare:workers';
import { NonRetryableError } from 'cloudflare:workflows';

export class MyWorkflow extends WorkflowEntrypoint<Env, Params> {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    await step.do("some step", async () => {
        if (!event.payload.data) {
          throw new NonRetryableError("event.payload.data did not contain the expected payload")
        }
      })
  }
}
```

Errors can be caught with try-catch to allow the Workflow to continue or run cleanup logic:

```typescript
await step.do('task', async () => {
  // work to be done
});

try {
    await step.do('non-retryable-task', async () => {
    // work not to be retried
        throw new NonRetryableError('oh no');
    });
} catch (e) {
    console.log(`Step failed: ${e.message}`);
    await step.do('clean-up-task', async () => {
      // Clean up code here
    });
}

// the Workflow will not fail and will continue its execution

await step.do('next-task', async() => {
  // more work to be done
});
```

Uncaught exceptions or exhausted retries cause the instance to enter the "Errored" state.

---

## Triggering Workflows (Workers Binding)

**Docs:** https://developers.cloudflare.com/workflows/build/trigger-workflows/

Workflows are triggered from Worker scripts using bindings. They can be triggered from: the `fetch` handler (HTTP), Queue consumers, Cron Triggers, or Durable Objects.

```typescript
interface Env {
  MY_WORKFLOW: Workflow;
}

export default {
  async fetch(req: Request, env: Env) {
    const instanceId = new URL(req.url).searchParams.get("instanceId");

    if (instanceId) {
      let instance = await env.MY_WORKFLOW.get(instanceId);
      return Response.json({
        status: await instance.status(),
      });
    }

    const newId = crypto.randomUUID();
    let instance = await env.MY_WORKFLOW.create({ id: newId });
    return Response.json({
      id: instance.id,
      details: await instance.status(),
    });
  },
};
```

---

## Passing Data to Workflows (Event Payloads)

**Docs:** https://developers.cloudflare.com/workflows/build/events-and-parameters/

Data is passed to a Workflow via the `params` property of `create()`. The data appears in the Workflow's `event.payload`.

```typescript
export default {
  async fetch(req: Request, env: Env) {
    let someEvent = { url: req.url, createdTimestamp: Date.now() };
    let instance = await env.MY_WORKFLOW.create({
      id: crypto.randomUUID(),
      params: someEvent,
    });
    return Response.json({
      id: instance.id,
      details: await instance.status(),
    });
  },
};
```

TypeScript type parameters allow typed payloads:

```typescript
interface User {
  email: string;
  createdTimestamp: number;
}

interface Env {
  MY_WORKFLOW: Workflow<User>;
}

export default {
  async fetch(request, env, ctx) {
    const user: User = {
      email: "user@example.com",
      createdTimestamp: Date.now()
    }

    let instance = await env.MY_WORKFLOW.create({
      params: user
    })

    return Response.json({
      id: instance.id,
      details: await instance.status(),
    });
  }
}
```

Maximum event payload size: 1 MiB.

---

## Instance Management (create, get, status)

**Docs:** https://developers.cloudflare.com/workflows/build/workers-api/

### create()

Creates a new Workflow instance with an optional custom ID and parameters.

```typescript
let instance = await env.MY_WORKFLOW.create({
  id: myIdDefinedFromOtherSystem,
  params: { hello: "world" },
});
return Response.json({
  id: instance.id,
  details: await instance.status(),
});
```

```typescript
interface WorkflowInstanceCreateOptions {
  id?: string;
  params?: unknown;
}
```

### createBatch()

Triggers up to 100 instances simultaneously for improved throughput.

```typescript
const listOfInstances = [
  { id: "id-abc123", params: { hello: "world-0" } },
  { id: "id-def456", params: { hello: "world-1" } },
  { id: "id-ghi789", params: { hello: "world-2" } },
];
let instances = await env.MY_WORKFLOW.createBatch(listOfInstances);
```

### get()

Retrieves a specific instance by ID.

```typescript
try {
  let instance = await env.MY_WORKFLOW.get(id);
  return Response.json({
    id: instance.id,
    details: await instance.status(),
  });
} catch (e: any) {
  const msg = `failed to get instance ${id}: ${e.message}`;
  console.error(msg);
  return Response.json({ error: msg }, { status: 400 });
}
```

### Instance Lifecycle Methods

```typescript
declare abstract class WorkflowInstance {
  public id: string;

  public pause(): Promise<void>;
  public resume(): Promise<void>;
  public terminate(): Promise<void>;
  public restart(): Promise<void>;
  public status(): Promise<InstanceStatus>;
}
```

### InstanceStatus

```typescript
type InstanceStatus = {
  status:
    | "queued"
    | "running"
    | "paused"
    | "errored"
    | "terminated"
    | "complete"
    | "waiting"
    | "waitingForPause"
    | "unknown";
  error?: {
    name: string,
    message: string
  };
  output?: unknown;
};
```

---

## Pause and Resume Instances

**Docs:** https://developers.cloudflare.com/workflows/build/trigger-workflows/

Instances can be explicitly paused and later resumed.

```typescript
let instance = await env.MY_WORKFLOW.get("abc-123");
await instance.pause();
```

```typescript
let instance = await env.MY_WORKFLOW.get("abc-123");
await instance.resume();
```

---

## Terminate and Restart Instances

**Docs:** https://developers.cloudflare.com/workflows/build/trigger-workflows/

Instances can be forcibly stopped or restarted from the beginning.

```typescript
let instance = await env.MY_WORKFLOW.get("abc-123");
await instance.terminate();
```

```typescript
let instance = await env.MY_WORKFLOW.get("abc-123");
await instance.restart();
```

---

## Nested / Child Workflows

**Docs:** https://developers.cloudflare.com/workflows/build/trigger-workflows/

A parent Workflow can trigger child Workflows. The parent continues execution immediately after the child instance is created -- it does not block waiting for the child to complete.

```typescript
export class ParentWorkflow extends WorkflowEntrypoint<Env, Params> {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const result = await step.do("initial processing", async () => {
      return { fileKey: "output.pdf" };
    });

    const childInstance = await step.do("trigger child workflow", async () => {
      return await this.env.CHILD_WORKFLOW.create({
        id: `child-${event.instanceId}`,
        params: { fileKey: result.fileKey },
      });
    });

    await step.do("continue with other work", async () => {
      console.log(`Started child workflow: ${childInstance.id}`);
    });
  }
}
```

---

## State Passing Between Steps

**Docs:** https://developers.cloudflare.com/workflows/build/rules-of-workflows/

State is passed between steps via return values from `step.do()`. The return value is persisted durably and available to subsequent steps. In-memory variables outside of steps are NOT reliable because the Workflow engine may hibernate between steps.

Correct pattern (return state from steps):

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const imageList: string[] = await Promise.all([
      step.do("get first cutest cat from KV", async () => {
        return await this.env.KV.get("cutest-http-cat-1");
      }),
      step.do("get second cutest cat from KV", async () => {
        return await this.env.KV.get("cutest-http-cat-2");
      }),
    ]);
    await step.sleep("wait", "3 hours");
    await step.do(
      "choose a random cat from the list and download it",
      async () => {
        const randomCat = imageList.at(getRandomInt(0, imageList.length));
        return await fetch(`https://http.cat/${randomCat}`);
      },
    );
  }
}
```

Incorrect pattern (in-memory state lost on hibernation):

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const imageList: string[] = [];
    await step.do("get first cutest cat from KV", async () => {
      const httpCat = await this.env.KV.get("cutest-http-cat-1");
      imageList.push(httpCat); // BAD: imageList is in-memory, will be lost
    });
    // ...
  }
}
```

Each step can persist up to 1 MiB of state. For larger data, store it externally (R2, KV) and return a reference:

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const dataRef = await step.do("fetch and store large dataset", async () => {
      const response = await fetch("https://api.example.com/large-dataset");
      const data = await response.json();
      await this.env.MY_BUCKET.put("dataset-123", JSON.stringify(data));
      return { key: "dataset-123" };
    });
    const data = await step.do("process dataset", async () => {
      const stored = await this.env.MY_BUCKET.get(dataRef.key);
      return processData(await stored.json());
    });
  }
}
```

---

## Configuration (wrangler.jsonc / wrangler.toml)

**Docs:** https://developers.cloudflare.com/workflows/get-started/guide/

Workflows are configured in `wrangler.jsonc` or `wrangler.toml`. Each Workflow binding maps a name, a JavaScript binding variable, and the exported class name.

**wrangler.jsonc:**

```json
{
  "$schema": "node_modules/wrangler/config-schema.json",
  "name": "my-workflow",
  "main": "src/index.ts",
  "compatibility_date": "2026-02-25",
  "observability": {
    "enabled": true
  },
  "workflows": [
    {
      "name": "my-workflow",
      "binding": "MY_WORKFLOW",
      "class_name": "MyWorkflow"
    }
  ]
}
```

**wrangler.toml:**

```toml
"$schema" = "node_modules/wrangler/config-schema.json"
name = "my-workflow"
main = "src/index.ts"
compatibility_date = "2026-02-25"

[observability]
enabled = true

[[workflows]]
name = "my-workflow"
binding = "MY_WORKFLOW"
class_name = "MyWorkflow"
```

The Workflow class must be exported from the Worker's entry point:

```typescript
export { MyWorkflow } from "./workflow";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // ...
  },
} satisfies ExportedHandler<Env>;
```

---

## Cross-Script Workflow Bindings

**Docs:** https://developers.cloudflare.com/workflows/build/workers-api/

A Worker can bind to a Workflow defined in a different Worker script using the `script_name` property. This enables one Worker to trigger Workflows defined in another.

```json
{
  "$schema": "./node_modules/wrangler/config-schema.json",
  "name": "web-api-worker",
  "main": "src/index.ts",
  "compatibility_date": "2026-02-25",
  "workflows": [
    {
      "name": "billing-workflow",
      "binding": "MY_WORKFLOW",
      "class_name": "MyWorkflow",
      "script_name": "billing-worker"
    }
  ]
}
```

```toml
[[workflows]]
name = "billing-workflow"
binding = "MY_WORKFLOW"
class_name = "MyWorkflow"
script_name = "billing-worker"
```

---

## Configuring CPU and Subrequest Limits

**Docs:** https://developers.cloudflare.com/workflows/reference/limits/

CPU time and subrequest limits can be increased via configuration.

**CPU limits (wrangler.jsonc):**

```json
{
  "limits": {
    "cpu_ms": 300000
  }
}
```

**CPU limits (wrangler.toml):**

```toml
[limits]
cpu_ms = 300_000
```

**Subrequest limits (wrangler.jsonc):**

```json
{
  "limits": {
    "subrequests": 10000000
  }
}
```

**Subrequest limits (wrangler.toml):**

```toml
[limits]
subrequests = 10_000_000
```

---

## Rules and Best Practices

**Docs:** https://developers.cloudflare.com/workflows/build/rules-of-workflows/

### 1. Ensure API/Binding calls are idempotent

Steps may retry, so operations should check if they have already completed before executing. Example: check if a customer has already been charged before charging again.

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const customer_id = 123456;
    await step.do(
      `charge ${customer_id} for its monthly subscription`,
      async () => {
        const subscription = await fetch(
          `https://payment.processor/subscriptions/${customer_id}`,
        ).then((res) => res.json());
        if (subscription.charged) {
          return;
        }
        return await fetch(
          `https://payment.processor/subscriptions/${customer_id}`,
          {
            method: "POST",
            body: JSON.stringify({ amount: 10.0 }),
          },
        );
      },
    );
  }
}
```

### 2. Make steps granular

Minimize the number of API/binding calls per step. Keep unrelated operations in separate steps to enable independent retry policies.

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const httpCat = await step.do("get cutest cat from KV", async () => {
      return await this.env.KV.get("cutest-http-cat");
    });
    const image = await step.do("fetch cat image from http.cat", async () => {
      return await fetch(`https://http.cat/${httpCat}`);
    });
  }
}
```

### 3. Do not rely on state outside of steps

Workflows may hibernate and lose all in-memory state. Store state exclusively through step return values.

### 4. Avoid side effects outside step.do

Logic outside of steps may be duplicated when the engine restarts. Non-serializable resources like database connections are exceptions.

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    // BAD: side effect outside step
    const badInstance = await this.env.ANOTHER_WORKFLOW.create();
    const badRandom = Math.random();

    // GOOD: side effect inside step
    const goodRandom = await step.do("create a random number", async () => {
      return Math.random();
    });

    // OK: non-serializable resource created outside steps
    const db = createDBConnection(this.env.DB_URL, this.env.DB_TOKEN);

    const goodInstance = await step.do(
      "good step that returns state",
      async () => {
        const instance = await this.env.ANOTHER_WORKFLOW.create();
        return instance;
      },
    );
  }
}
```

### 5. Do not mutate incoming events

The event payload is immutable. Changes are not persisted across steps.

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<MyEvent>, step: WorkflowStep) {
    // BAD: mutating event
    await step.do("bad step that mutates the incoming event", async () => {
      let userData = await this.env.KV.get(event.payload.user);
      event.payload = userData;
    });
    // GOOD: return state from step
    let userData = await step.do("good step that returns state", async () => {
      return await this.env.KV.get(event.payload.user);
    });
  }
}
```

### 6. Name steps deterministically

Step names act as cache keys. Non-deterministic names (e.g., including `Date.now()` or `Math.random()`) will prevent proper caching. Using step return values or event payload data in names is fine.

```typescript
// BAD: non-deterministic step name
await step.do(`step #1 running at: ${Date.now()}`, async () => { /* ... */ });

// GOOD: deterministic step name derived from data
let catList = await step.do("get cat list from KV", async () => {
  return await this.env.KV.get("cat-list");
});
for (const cat of catList) {
  await step.do(`get cat: ${cat}`, async () => {
    return await this.env.KV.get(cat);
  });
}
```

### 7. Use caution with Promise.race() and Promise.any()

Wrap these inside `step.do()` for deterministic caching:

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const race_return = await step.do("Promise step", async () => {
      return await Promise.race([
        step.do("Promise first race", async () => {
          await sleep(1000);
          return "first";
        }),
        step.do("Promise second race", async () => {
          return "second";
        }),
      ]);
    });
    await step.sleep("Sleep step", "2 hours");
    return await step.do("Another step", async () => {
      return race_return;
    });
  }
}
```

### 8. Instance IDs must be unique

Workflow instance IDs are unique per Workflow. Do not reuse IDs across different invocations. Use transaction IDs or composite IDs.

```typescript
export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    let userId = getUserId(req);

    // BAD: reusing userId as instance ID
    let badInstance = await env.MY_WORKFLOW.create({
      id: userId,
      params: payload,
    });

    // GOOD: composite unique ID
    let instanceId = `${getUserId(req)}-${crypto.randomUUID().slice(0, 6)}`;
    let { result } = await addNewInstanceToDB(userId, instanceId);
    let goodInstance = await env.MY_WORKFLOW.create({
      id: instanceId,
      params: payload,
    });

    return Response.json({
      id: goodInstance.id,
      details: await goodInstance.status(),
    });
  },
};
```

### 9. Always await steps

Unawaited step calls create dangling operations and race conditions.

```typescript
// BAD: missing await
const badIssues = step.do(`fetch issues from GitHub`, async () => {
  let issues = await getIssues(event.payload.repoName);
  return issues;
});

// GOOD: awaited
const goodIssues = await step.do(`fetch issues from GitHub`, async () => {
  let issues = await getIssues(event.payload.repoName);
  return issues;
});
```

### 10. Use deterministic conditional logic

Base conditions on event payloads or step outputs, not `Math.random()` or `Date.now()`.

```typescript
export class MyWorkflow extends WorkflowEntrypoint {
  async run(event: WorkflowEvent<Params>, step: WorkflowStep) {
    const config = await step.do("fetch config", async () => {
      return await this.env.KV.get("feature-flags", { type: "json" });
    });
    // GOOD: condition based on step output
    if (config.enableEmailNotifications) {
      await step.do("send email", async () => {});
    }
    // GOOD: condition based on event payload
    if (event.payload.userType === "premium") {
      await step.do("premium processing", async () => {});
    }
    // BAD: non-deterministic condition
    if (Math.random() > 0.5) {
      await step.do("maybe do something", async () => {});
    }
    // GOOD: random value captured in a step
    const shouldProcess = await step.do("decide randomly", async () => {
      return Math.random() > 0.5;
    });
    if (shouldProcess) {
      await step.do("conditionally do something", async () => {});
    }
  }
}
```

### 11. Batch multiple invocations

Use `createBatch` for improved throughput when creating many instances.

```typescript
export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    let instances = [
      { id: "user1", params: { name: "John" } },
      { id: "user2", params: { name: "Jane" } },
      { id: "user3", params: { name: "Alice" } },
      { id: "user4", params: { name: "Bob" } },
    ];

    // BAD: sequential creation
    for (let instance of instances) {
      await env.MY_WORKFLOW.create({
        id: instance.id,
        params: instance.params,
      });
    }

    // GOOD: batch creation
    let createdInstances = await env.MY_WORKFLOW.createBatch(instances);
    return Response.json({ instances: createdInstances });
  },
};
```

### 12. Limit step timeouts to 30 minutes

Use `step.waitForEvent()` for longer waits.

### 13. Keep step return values under 1 MiB

Store large data externally (R2 or KV) and return only references.

---

## Human-in-the-Loop Pattern

**Docs:** https://developers.cloudflare.com/workflows/examples/wait-for-event/

Combines `step.waitForEvent()` with external approval to pause a Workflow until a human takes action.

The example Workflow:

1. Saves image metadata to D1 database
2. Pauses via `waitForEvent()` awaiting approval signal (type: `'approval-for-ai-tagging'`, timeout: `'5 minute'`)
3. On approval, runs Workers AI to generate tags
4. Persists tags back to database

The approval event is sent from an external source (e.g., a Next.js frontend) via `instance.sendEvent()`.

```typescript
export class MyWorkflow extends WorkflowEntrypoint<Env, WorkflowParams> {

  private db!: DatabaseService;

  async run(event: WorkflowEvent<WorkflowParams>, step: WorkflowStep) {

    this.db = new DatabaseService(this.env.DB);

    const { imageKey } = event.payload;

    await step.do('Insert image name into database', async () => {
      await this.db.insertImage(imageKey, event.instanceId);
    });

    const waitForApproval = await step.waitForEvent('Wait for AI Image tagging approval', {
      type: 'approval-for-ai-tagging',
      timeout: '5 minute',
    });

    const approvalPayload = waitForApproval.payload as ApprovalRequest;

    if (approvalPayload?.approved) {
      const aiTags = await step.do('Generate AI tags', async () => {
        const image = await this.env.workflow_demo_bucket.get(imageKey);
        if (!image) throw new Error('Image not found');
        const arrayBuffer = await image.arrayBuffer();
        const uint8Array = new Uint8Array(arrayBuffer);
        const input = {
          image: Array.from(uint8Array),
          prompt: AI_CONFIG.PROMPT,
          max_tokens: AI_CONFIG.MAX_TOKENS,
        };
        const response = await this.env.AI.run(AI_CONFIG.MODEL, input);
        return response.description;
      });

      await step.do('Update DB with AI tags', async () => {
        await this.db.updateImageTags(event.instanceId, aiTags);
      });
    }
  }
}
```

Reference implementation: https://github.com/cloudflare/docs-examples/tree/main/workflows/waitForEvent

---

## Wrangler CLI Commands

**Docs:** https://developers.cloudflare.com/workflows/reference/wrangler-commands/

### Workflow Management

| Command | Description |
|---------|-------------|
| `workflows list` | List Workflows associated to account (supports `--page`, `--per-page`) |
| `workflows describe [NAME]` | Display details about a specific Workflow |
| `workflows delete [NAME]` | Remove a Workflow and its associated instances |
| `workflows trigger [NAME] [PARAMS]` | Trigger a Workflow, creating a new instance (optional `--params` JSON, `--id` custom instance ID) |

### Instance Management

| Command | Description |
|---------|-------------|
| `workflows instances list [NAME]` | List instances (filter by status: queued, running, paused, errored, terminated, complete) |
| `workflows instances describe [NAME] [ID]` | Describe instance logs, retries, and errors |
| `workflows instances send-event [NAME] [ID]` | Send event to running instance (`--type`, `--payload` JSON) |
| `workflows instances terminate [NAME] [ID]` | Stop a running instance |
| `workflows instances restart [NAME] [ID]` | Restart a paused or failed instance |
| `workflows instances pause [NAME] [ID]` | Pause an instance |
| `workflows instances resume [NAME] [ID]` | Resume a paused instance |

All commands support `--env`, `--config`, and `--cwd` global flags.

---

## Limits

**Docs:** https://developers.cloudflare.com/workflows/reference/limits/

| Limit | Free Plan | Paid Plan |
|-------|-----------|-----------|
| Max script size | 3 MB | 10 MB |
| Total scripts per account | 100 | 500 |
| Compute time per step | 10 ms | 30 seconds (default), configurable to 5 minutes |
| Duration (wall clock) per step | Unlimited | Unlimited |
| Max persisted state per step | 1 MiB (2^20 bytes) | 1 MiB (2^20 bytes) |
| Max event payload size | 1 MiB | 1 MiB |
| Max state per Workflow instance | 100 MB | 1 GB |
| Max step.sleep duration | 365 days | 365 days |
| Max steps per Workflow | 1,024 | 1,024 |
| Max Workflow executions | 100,000/day | Unlimited |
| Concurrent Workflow instances per account | 100 | 10,000 |
| Max instance creation rate | 100/second | 100/second |
| Max queued instances | 100,000 | 1,000,000 |
| Retention for completed instance state | 3 days | 30 days |
| Max Workflow name length | 64 characters | 64 characters |
| Max instance ID length | 100 characters | 100 characters |
| Subrequests per instance | 50 (external) / 1,000 (Cloudflare) | 10,000 (default), configurable to 10 million |

Instances in a `waiting` state (from `step.waitForEvent()`) do not count towards concurrency limits.

---

## Pricing

**Docs:** https://developers.cloudflare.com/workflows/reference/pricing/

Three billing dimensions:

1. **CPU time** -- total compute consumed (milliseconds) by a Workflow. Idle/sleeping Workflows do not consume CPU.
2. **Requests/Invocations** -- number of Workflow triggers. Subrequests do not incur extra costs.
3. **Storage** -- total storage persisted by Workflows (GB).

| Dimension | Free Plan | Paid Plan |
|-----------|-----------|-----------|
| Requests | 100,000/day (shared with Workers) | 10 million/month + $0.30/additional million |
| CPU time | 10 ms per invocation | 30 million ms/month + $0.02/additional million |
| Storage | 1 GB | 1 GB included + $0.20/GB-month |

- CPU limits can be extended to 5 minutes per instance via configuration.
- Storage uses GB-month calculation (average peak daily storage over 30 days).
- State retention: 3 days (Free) or 7 days (Paid) -- can delete instances to free storage.

---

## Event Subscriptions

**Docs:** https://developers.cloudflare.com/workflows/reference/event-subscriptions/

Event subscriptions allow receiving messages when Workflow events occur. Cloudflare products (KV, Workers AI, Workers) publish structured events to queues for consumption via Workers or HTTP pull consumers.

Six instance-level events are available:

| Event | Description |
|-------|-------------|
| `instance.queued` | Instance created and awaiting execution |
| `instance.started` | Instance begins or resumes execution |
| `instance.paused` | Instance pauses execution |
| `instance.errored` | Instance step throws an error |
| `instance.terminated` | Instance manually terminated |
| `instance.completed` | Instance successfully finishes |

Event structure:

```json
{
  "type": "cf.workflows.workflow.instance.queued",
  "source": { "workflow": "workflow-name" },
  "payload": { "versionId": "...", "instanceId": "..." },
  "metadata": {
    "accountId": "...",
    "eventSubscriptionId": "...",
    "schemaVersion": "...",
    "timestamp": "..."
  }
}
```

---

## Workflow Visualizer

**Docs:** https://developers.cloudflare.com/workflows/build/visualizer/

A visual representation of parsed Workflow code displayed as a diagram on the Cloudflare dashboard. Shows sequenced and parallel steps, conditionals, loops, and nested logic. Users can collapse/expand loops and conditionals.

Available in beta for TypeScript/JavaScript Workers. Accessible at: `dash.cloudflare.com/?to=/:account/workers/workflows`

Limitations: Non-default bundlers may display unexpected behavior. Python Workflows are not supported.

---

## Local Development

**Docs:** https://developers.cloudflare.com/workflows/build/local-development/

Workflows support local development using Wrangler. The local version provides an emulated environment that mirrors production.

Requirements: Wrangler v3.89.0+, Node.js 18.0.0+.

```bash
npx wrangler dev
```

Current limitations:
- Remote bindings and `wrangler dev --remote` not supported
- Wrangler Workflows commands targeting production API will not work locally
- `pause()`, `resume()`, `terminate()`, and `restart()` not implemented locally

---

## Metrics and Analytics

**Docs:** https://developers.cloudflare.com/workflows/observability/metrics-analytics/

Workflows expose metrics via the `workflowsAdaptiveGroups` GraphQL dataset. Metrics are retained for 31 days.

Filtering dimensions include: workflow name, instance ID, step name, event type, step count, and time-based groupings (5-minute to hourly intervals).

**Workflow-level event types:** queued, started, successful, failed, terminated.

**Step-level event types:** start, success, failure, sleep states, retry attempts.

Available via Cloudflare dashboard or GraphQL Analytics API.

Example: query Workflow invocations and wall time by hour:

```graphql
query WorkflowInvocationsExample(
  $accountTag: string!
  $datetimeStart: Time
  $datetimeEnd: Time
  $workflowName: string
) {
  viewer {
    accounts(filter: { accountTag: $accountTag }) {
      wallTime: workflowsAdaptiveGroups(
        limit: 10000
        filter: {
          datetimeHour_geq: $datetimeStart
          datetimeHour_leq: $datetimeEnd
          workflowName: $workflowName
        }
        orderBy: [count_DESC]
      ) {
        count
        sum {
          wallTime
        }
        dimensions {
          date: datetimeHour
        }
      }
    }
  }
}
```

Example: query raw event data for a specific instance:

```graphql
query WorkflowsAdaptiveExample(
  $accountTag: string!
  $datetimeStart: Time
  $datetimeEnd: Time
  $instanceId: string
) {
  viewer {
    accounts(filter: { accountTag: $accountTag }) {
      workflowsAdaptive(
        limit: 100
        filter: {
          datetime_geq: $datetimeStart
          datetime_leq: $datetimeEnd
          instanceId: $instanceId
        }
        orderBy: [datetime_ASC]
      ) {
        datetime
        eventType
        workflowName
        instanceId
        stepCount
        wallTime
      }
    }
  }
}
```

Example query variables:

```json
{
  "accountTag": "fedfa729a5b0ecfd623bca1f9000f0a22",
  "datetimeStart": "2024-10-20T00:00:00Z",
  "datetimeEnd": "2024-10-29T00:00:00Z",
  "workflowName": "shoppingCart",
  "instanceId": "ecc48200-11c4-22a3-b05f-88a3c1c1db81"
}
```

---

## Calling Workflows from Pages

**Docs:** https://developers.cloudflare.com/workflows/build/call-workflows-from-pages/

Workflows can be triggered from Cloudflare Pages Functions by deploying a separate Worker containing the Workflow definition and calling it via:

1. **Service Bindings (recommended):** Direct method invocation without public exposure, no HTTP overhead. Uses `WorkerEntrypoint` class.
2. **HTTP Fetch:** Standard `fetch()` calls to a publicly exposed Worker endpoint. Requires authentication (e.g., shared secret header).

### Service Binding Approach

Worker service (index.ts):

```typescript
import { WorkerEntrypoint } from "cloudflare:workers";

interface Env {
  MY_WORKFLOW: Workflow;
}

type Payload = {
  hello: string;
};

export default class WorkflowsService extends WorkerEntrypoint<Env> {
  async fetch() {
    return new Response(null, { status: 404 });
  }

  async createInstance(payload: Payload) {
    let instance = await this.env.MY_WORKFLOW.create({
      params: payload,
    });

    return Response.json({
      id: instance.id,
      details: await instance.status(),
    });
  }
}
```

Pages Function (functions/request.ts):

```typescript
interface Env {
  WORKFLOW_SERVICE: Service;
}

export const onRequest: PagesFunction<Env> = async (context) => {
  let payload = { hello: "world" };
  return context.env.WORKFLOW_SERVICE.createInstance(payload);
};
```

### HTTP Fetch Approach

Worker (index.ts):

```typescript
export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    let instance = await env.MY_WORKFLOW.create({
      params: payload,
    });

    return Response.json({
      id: instance.id,
      details: await instance.status(),
    });
  },
};
```

Pages Function (functions/request.ts):

```typescript
export const onRequest: PagesFunction<Env> = async (context) => {
  let payload = { hello: "world" };
  const instanceStatus = await fetch("https://YOUR_WORKER.workers.dev/", {
    method: "POST",
    body: JSON.stringify(payload),
  });

  return Response.json(instanceStatus);
};
```

---

## Durable AI Agents

**Docs:** https://developers.cloudflare.com/workflows/get-started/durable-agents/

Workflows can be combined with the Agents SDK to build durable AI agents. Key capabilities:

| Problem | Solution |
|---------|----------|
| Extended agent loops | Durable execution that survives interruptions |
| Unreliable API calls | Automated retry with independent checkpoints |
| Human approval waits | `waitForEvent()` pauses for extended periods |
| Job polling | `step.sleep()` avoids resource consumption |

Each `step.do()` creates a checkpoint enabling resumption after interruptions. The `AgentWorkflow` base class enables bidirectional communication with WebSocket clients and progress reporting.

Workflow definition with durable agent loop (src/workflow.ts):

```typescript
import { AgentWorkflow } from "agents/workflows";
import type { AgentWorkflowEvent, AgentWorkflowStep } from "agents/workflows";
import Anthropic from "@anthropic-ai/sdk";
import {
  tools,
  searchReposTool,
  getRepoTool,
  type SearchReposInput,
  type GetRepoInput,
} from "./tools";
import type { ResearchAgent } from "./agent";

type Params = { task: string };

export class ResearchWorkflow extends AgentWorkflow<ResearchAgent, Params> {
  async run(event: AgentWorkflowEvent<Params>, step: AgentWorkflowStep) {
    const client = new Anthropic({ apiKey: this.env.ANTHROPIC_API_KEY });

    const messages: Anthropic.MessageParam[] = [
      { role: "user", content: event.payload.task },
    ];

    const toolDefinitions = tools.map(({ run, ...rest }) => rest);

    // Durable agent loop - each turn is checkpointed
    for (let turn = 0; turn < 10; turn++) {
      // Report progress to Agent and connected clients
      await this.reportProgress({
        step: `llm-turn-${turn}`,
        status: "running",
        percent: turn / 10,
        message: `Processing turn ${turn + 1}...`,
      });

      const response = (await step.do(
        `llm-turn-${turn}`,
        { retries: { limit: 3, delay: "10 seconds", backoff: "exponential" } },
        async () => {
          const msg = await client.messages.create({
            model: "claude-sonnet-4-5-20250929",
            max_tokens: 4096,
            tools: toolDefinitions,
            messages,
          });
          // Serialize for Workflow state
          return JSON.parse(JSON.stringify(msg));
        },
      )) as Anthropic.Message;

      if (!response || !response.content) continue;

      messages.push({ role: "assistant", content: response.content });

      if (response.stop_reason === "end_turn") {
        const textBlock = response.content.find(
          (b): b is Anthropic.TextBlock => b.type === "text",
        );
        const result = {
          status: "complete",
          turns: turn + 1,
          result: textBlock?.text ?? null,
        };

        // Report completion (durable)
        await step.reportComplete(result);
        return result;
      }

      const toolResults: Anthropic.ToolResultBlockParam[] = [];

      for (const block of response.content) {
        if (block.type !== "tool_use") continue;

        // Broadcast tool execution to clients
        this.broadcastToClients({
          type: "tool_call",
          tool: block.name,
          turn,
        });

        const result = await step.do(
          `tool-${turn}-${block.id}`,
          { retries: { limit: 2, delay: "5 seconds" } },
          async () => {
            switch (block.name) {
              case "search_repos":
                return searchReposTool.run(block.input as SearchReposInput);
              case "get_repo":
                return getRepoTool.run(block.input as GetRepoInput);
              default:
                return `Unknown tool: ${block.name}`;
            }
          },
        );

        toolResults.push({
          type: "tool_result",
          tool_use_id: block.id,
          content: result,
        });
      }

      messages.push({ role: "user", content: toolResults });
    }

    return { status: "max_turns_reached", turns: 10 };
  }
}
```

Agent implementation with workflow lifecycle hooks (src/agent.ts):

```typescript
import { Agent } from "agents";

type State = {
  currentWorkflow?: string;
  status?: string;
};

export class ResearchAgent extends Agent<Env, State> {
  initialState: State = {};

  // Start a research task - called via HTTP or WebSocket
  async startResearch(task: string) {
    const instanceId = await this.runWorkflow("RESEARCH_WORKFLOW", { task });
    this.setState({
      ...this.state,
      currentWorkflow: instanceId,
      status: "running",
    });
    return { instanceId };
  }

  // Get status of a workflow
  async getResearchStatus(instanceId: string) {
    return this.getWorkflow(instanceId);
  }

  // Called when workflow reports progress
  async onWorkflowProgress(
    workflowName: string,
    instanceId: string,
    progress: unknown,
  ) {
    // Broadcast to all connected WebSocket clients
    this.broadcast(JSON.stringify({ type: "progress", instanceId, progress }));
  }

  // Called when workflow completes
  async onWorkflowComplete(
    workflowName: string,
    instanceId: string,
    result?: unknown,
  ) {
    this.setState({ ...this.state, status: "complete" });
    this.broadcast(JSON.stringify({ type: "complete", instanceId, result }));
  }

  // Called when workflow errors
  async onWorkflowError(
    workflowName: string,
    instanceId: string,
    error: string,
  ) {
    this.setState({ ...this.state, status: "error" });
    this.broadcast(JSON.stringify({ type: "error", instanceId, error }));
  }
}
```

API route handler (src/index.ts):

```typescript
import { getAgentByName, routeAgentRequest } from "agents";

export { ResearchAgent } from "./agent";
export { ResearchWorkflow } from "./workflow";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // Route WebSocket connections to /agents/research-agent/{name}
    const agentResponse = await routeAgentRequest(request, env);
    if (agentResponse) return agentResponse;

    // HTTP API for starting research tasks
    if (request.method === "POST" && url.pathname === "/research") {
      const { task, agentId } = await request.json<{
        task: string;
        agentId?: string;
      }>();

      // Get agent instance by name (creates if doesn't exist)
      const agent = await getAgentByName(
        env.ResearchAgent,
        agentId ?? "default",
      );

      // Start the research workflow via RPC
      const result = await agent.startResearch(task);
      return Response.json(result);
    }

    // Check workflow status
    if (url.pathname === "/status") {
      const instanceId = url.searchParams.get("instanceId");
      const agentId = url.searchParams.get("agentId") ?? "default";

      if (!instanceId) {
        return Response.json({ error: "instanceId required" }, { status: 400 });
      }

      const agent = await getAgentByName(env.ResearchAgent, agentId);
      const status = await agent.getResearchStatus(instanceId);

      return Response.json(status);
    }

    return new Response("POST /research with { task } to start", {
      status: 400,
    });
  },
} satisfies ExportedHandler<Env>;
```

---

## Glossary

**Docs:** https://developers.cloudflare.com/workflows/reference/glossary/

| Term | Definition |
|------|------------|
| **Durable Execution** | Programming model enabling reliable execution with automatic state persistence, retry capability, and resistance to failures |
| **Event** | Trigger for a Workflow instance, may include optional parameters |
| **Instance** | A specific running, paused, or errored occurrence of a Workflow. One Workflow can generate unlimited instances |
| **Step** | Self-contained, individually retryable component. Can emit optional state for persistence. Max 1,024 per Workflow |
| **Workflow** | Named Workflow definition associated with a single Workers script |
