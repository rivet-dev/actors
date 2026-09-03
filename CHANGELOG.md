# Changelog

## Unreleased

- The Rust SDK now supports actor action sets containing up to 128 action types, increased from 16. TypeScript actor definitions remain unrestricted.

- `rivetkit` no longer exposes `ctx.sql` on actor contexts. Migrate raw SQLite calls to `ctx.db` from `rivetkit/db`, and keep Drizzle setup on the `rivetkit/db/drizzle` subpath.

  Migration example:

  ```ts
  import { db } from "rivetkit/db";

  const myActor = actor({
    db: db(),
    actions: {
      listTodos: async (ctx) => {
        return await ctx.db.execute("SELECT * FROM todos ORDER BY created_at DESC");
      },
    },
  });
  ```

- `rivetkit` no longer exports the old concrete typed error classes from `rivetkit/actor/errors` such as `QueueFull`, `ActorNotFound`, and `ActionTimedOut`. The native runtime now standardizes on `RivetError` plus `group` and `code` so the same error shape survives HTTP, WebSocket, and bridge boundaries instead of depending on `instanceof` across runtimes.

  Migration example:

  ```ts
  try {
    await actor.someAction();
  } catch (e) {
    if (e instanceof QueueFull) {
      // old path
    }

    if (isRivetErrorCode(e, "queue", "full")) {
      // new path
    }
  }
  ```

  Common replacements:

  | Removed class                     | Use now                                           |
  | --------------------------------- | ------------------------------------------------- |
  | `QueueFull`                       | `isRivetErrorCode(e, "queue", "full")`            |
  | `QueueMessageTooLarge`            | `isRivetErrorCode(e, "queue", "message_too_large")` |
  | `QueueMessageInvalid`             | `isRivetErrorCode(e, "queue", "message_invalid")` |
  | `QueuePayloadInvalid`             | `isRivetErrorCode(e, "queue", "invalid_payload")` |
  | `QueueCompletionPayloadInvalid`   | `isRivetErrorCode(e, "queue", "invalid_completion_payload")` |
  | `QueueAlreadyCompleted`           | `isRivetErrorCode(e, "queue", "already_completed")` |
  | `ActionTimedOut`                  | `isRivetErrorCode(e, "action", "timed_out")`      |
  | `ActionNotFound`                  | `isRivetErrorCode(e, "action", "not_found")`      |
  | `ActorNotFound`                   | `isRivetErrorCode(e, "actor", "not_found")`       |
  | `ActorStopping`                   | `isRivetErrorCode(e, "actor", "stopping")`        |
  | `ActorAborted`                    | `isRivetErrorCode(e, "actor", "aborted")`         |
  | `IncomingMessageTooLong`          | `isRivetErrorCode(e, "message", "incoming_too_long")` |
  | `OutgoingMessageTooLong`          | `isRivetErrorCode(e, "message", "outgoing_too_long")` |
  | `InvalidEncoding`                 | `isRivetErrorCode(e, "encoding", "invalid")`      |
  | `InvalidRequest`                  | `isRivetErrorCode(e, "request", "invalid")`       |
  | `InvalidQueryJSON`                | `isRivetErrorCode(e, "request", "invalid_query_json")` |
  | `RequestHandlerNotDefined`        | `isRivetErrorCode(e, "handler", "request_not_defined")` |
  | `WebSocketHandlerNotDefined`      | `isRivetErrorCode(e, "handler", "websocket_not_defined")` |
  | `FeatureNotImplemented`           | `isRivetErrorCode(e, "feature", "not_implemented")` |
  | `Unsupported`                     | `isRivetErrorCode(e, "feature", "unsupported")`   |

  Keep catching `UserError` when you intentionally throw user-facing application errors yourself. The removal only affects the built-in typed subclasses that used to wrap framework/runtime failures.

- Restored `Registry.handler(request)` and `Registry.serve()` for the native serverless runner endpoint described in `.agent/specs/serverless-restoration.md`. The route surface is `/api/rivet`, `/api/rivet/health`, `/api/rivet/metadata`, and `/api/rivet/start`; user traffic still goes through the Rivet Engine gateway.
- `Registry.start()` now starts the native envoy path only. Built-in `staticDir` serving is not wired through the native engine subprocess yet and remains a follow-up.
- Restored the supported `rivetkit/test`, `rivetkit/inspector`, and `rivetkit/inspector/client` entrypoints. `rivetkit/test` now waits for the native envoy metadata endpoint instead of the removed TypeScript in-memory runtime.
- Restored the zero-runtime `*ContextOf` helper types on the root `rivetkit` export so patterns like `ActionContextOf<typeof myActor>` work again. `PATH_CONNECT`, `PATH_WEBSOCKET_PREFIX`, `KV_KEYS`, `ActorKv`, `ActorInstance`, `ActorRouter`, `createActorRouter`, and `routeWebSocket` stay removed.
- `rivetkit/driver-helpers` and `rivetkit/driver-helpers/websocket` stay removed. They only exposed internal runtime/router helpers; migrate to the public `rivetkit`, `rivetkit/client`, and engine-client APIs instead of importing package internals.
- `rivetkit/topologies/*` stays removed. The topology helpers are deleted on this branch; keep custom coordinate/partition logic in app code if you still need it.
- `rivetkit/dynamic` and `rivetkit/sandbox/*` stay permanently removed on this branch. There is no in-package replacement, so move those integrations out of `rivetkit` imports instead of waiting for a hidden subpath to come back.
