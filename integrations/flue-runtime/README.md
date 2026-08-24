# @rivet-dev/flue

A [Flue](https://github.com/rivet-dev/flue) target that compiles agents and
workflows into a [Rivet](https://rivet.dev) Actor application. It provides
durable sessions, submission recovery, inter-actor `dispatch()`, and Flue
storage backed by each actor's native SQLite database (`c.db`).

```sh
npm add @rivet-dev/flue
```

```ts
// flue.config.ts
import { defineConfig } from "@flue/cli/config";
import { rivet } from "@rivet-dev/flue";

export default defineConfig({
	target: rivet({ actors: "./actors.ts" }),
});
```

Export a RivetKit registry from `actors.ts`. `flue build` then emits a server
entrypoint that maps every agent and workflow to a durable Rivet Actor.

Full documentation, setup, and configuration:
**https://rivet.dev/docs/integrations/flue**
