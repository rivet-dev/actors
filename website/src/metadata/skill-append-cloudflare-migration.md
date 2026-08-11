## Cloudflare Migration: Key Architectural Differences

Cloudflare Workers have implicit access to Durable Objects, D1, Queues, and other bindings via the `env` parameter. In RivetKit, your server is a normal Node.js/Hono application that talks to actors via an explicit client.

### Server-Side Actor Access

Use `createClient` from `rivetkit/client` to call actors from your server routes. This replaces the Cloudflare `env.MY_DO.get(id)` / `env.MY_DO.getByName(name)` pattern.

```ts
import { Hono } from "hono";
import { createClient } from "rivetkit/client";
import type { registry } from "./actors";

const app = new Hono();

// Create a client pointing at your local RivetKit server
const client = createClient<typeof registry>("http://localhost:6420");

app.get("/hello/:name", async (c) => {
  // Get or create an actor by key (replaces env.MY_DO.idFromName / getByName)
  const handle = await client.myActor.getOrCreate([c.req.param("name")]);
  const result = await handle.sayHello();
  return c.json(result);
});
```

### Server Entrypoint Pattern

Your `server.ts` must both mount the registry handler (so actors can run) and bind to a port. This replaces the Cloudflare Worker `export default { fetch }` pattern.

```ts
import { Hono } from "hono";
import { registry } from "./actors";

const app = new Hono();

// Mount RivetKit handler (serves actor RPCs, WebSocket upgrades, metadata)
app.route("/", registry.handler());

// Add your own routes alongside the registry
app.get("/", (c) => c.text("OK"));

export default app;
```

The dev server is started by the project tooling (e.g., `vite` with `vite-plugin-srvx`, or `@hono/node-server`). You do not need to call `serve()` manually when using vite-plugin-srvx.

### Project Setup

Follow the project setup instructions in `BASE_SKILL.md` (the RivetKit skill). The migrated project should be a standard RivetKit project with:
- `package.json` with `rivetkit` and framework dependencies
- `src/actors.ts` defining actors and the registry via `setup()`
- `src/server.ts` as the Hono entrypoint mounting `registry.handler()`
- `vite.config.ts` if using vite-plugin-srvx (required for vite-based projects)
