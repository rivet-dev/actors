Verify the migrated Cloudflare Agents GitHub webhook example works at {{URL}}.

Clone the original source from https://github.com/cloudflare/agents/tree/aba7432d5d395505df88e09b06e1cdd10f5bdad3/examples/github-webhook and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the React dashboard loads without errors
2. Confirm the UI shows a repo events list or dashboard (may be empty without webhook data)
3. Check for any console errors in the browser
4. If there's a way to view stats or event history, confirm those UI elements are present and functional
5. Reload the page and confirm the UI state is consistent

## Code review

Read through the migrated source and compare against the original. Check that:

- The Agent class with `@callable()` methods has working equivalents as actor actions
- SQLite storage via `this.sql` tagged templates is migrated to actor SQLite (`c.db`)
- State management (`setState` / `this.state`) is migrated to actor state
- HMAC-SHA256 webhook signature verification logic is preserved
- WebSocket streaming of real-time events to connected clients is implemented
- The React frontend with `useAgent` is migrated to use `@rivetkit/react` or equivalent
- `routeAgentRequest` / `getAgentByName` routing is replaced with actor client routing
- No original functionality was silently dropped

## Pass criteria

- React dashboard loads without errors
- Events list or dashboard UI is present
- No console errors
- SQLite-backed storage is implemented in the actor
- Webhook signature verification logic exists
- WebSocket streaming to clients is implemented
- No original features are missing from the migrated code
