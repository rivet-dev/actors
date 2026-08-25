# Workflow SDK (Beta)

A minimal Workflow SDK app backed by Rivet Actors.

This integration is in beta.

## Prerequisites

- Node.js 22 or newer
- pnpm

## Getting Started

```sh
cp .env.example .env
pnpm build
pnpm dev
```

Start an order with `curl -X POST http://localhost:3000/orders/42`. The first
World operation lazily starts Rivet in the same process and waits until it is
ready; there is no separate Rivet server command.

Run `pnpm test` for the Workflow SDK's official `@workflow/vitest` integration test.
The World package itself runs the `@workflow/world-testing` conformance suite
against an owned Rivet Engine.

## Features

- Workflow World backed by Rivet Actors
- Durable steps and waits
- The Workflow SDK's supported Vitest harness

## Implementation

- [Workflow](https://github.com/rivet-dev/rivet/blob/main/examples/workflow-sdk/workflows/order.ts)
- [Integration test](https://github.com/rivet-dev/rivet/blob/main/examples/workflow-sdk/workflows/order.integration.test.ts)
- [HTTP server](https://github.com/rivet-dev/rivet/blob/main/examples/workflow-sdk/src/server.ts)

## Resources

- [Rivet Workflow SDK guide](https://rivet.dev/actors/integrations/workflow-sdk)
- [Workflow SDK](https://workflow-sdk.dev)

## License

MIT
