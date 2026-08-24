# Hello World - Supabase Functions

A minimal Rivet Actor counter running on Supabase Edge Functions with the WebAssembly runtime.

## Getting Started

```sh
git clone https://github.com/rivet-dev/rivet.git
cd rivet/examples/hello-world-supabase-functions
npm install
npx supabase start
npm run dev
```

`supabase functions serve` requires the local Supabase stack, so `npx supabase start` must run first. `rivet dev` then runs a local Rivet engine and spawns `supabase functions serve` for you.

Call the actor from another terminal:

```sh
npm run client
```

## Prerequisites

- [Supabase CLI](https://supabase.com/docs/guides/cli)
- Docker, for Supabase's local Edge Runtime

## Implementation

The function calls `serve` from `@rivetkit/supabase`, which loads the WebAssembly runtime and serves the Rivet handler. `RIVET_ENDPOINT` is the only required variable, and `rivet dev` passes it to the function automatically.

The edge runtime runs in a container, so it reaches the engine on the host at `http://host.docker.internal:6420` rather than loopback. `supabase functions serve` supplies the required `host-gateway` mapping on Linux, and Docker Desktop provides the alias on macOS and Windows, so this works across platforms. Set `RIVET_ENDPOINT` yourself only to override that.

[`supabase/functions/rivet/deno.json`](https://github.com/rivet-dev/rivet/tree/main/examples/hello-world-supabase-functions/supabase/functions/rivet/deno.json) maps `rivetkit` onto the pre-bundled `@rivetkit/supabase` so Deno can resolve the import and the deploy stays small.

See [`supabase/functions/rivet/index.ts`](https://github.com/rivet-dev/rivet/tree/main/examples/hello-world-supabase-functions/supabase/functions/rivet/index.ts).

## Resources

Read more about [actions](/docs/actors/actions) and [state](/docs/actors/state), or follow the [Supabase Functions Quickstart](/docs/actors/quickstart/supabase) and [deploy guide](/docs/deploy/supabase).

## License

MIT
