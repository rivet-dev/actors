Verify the migrated PartyServer durable chat template works at {{URL}}.

Clone the original source from https://github.com/cloudflare/templates/tree/30d1642da7e2b42913dc63a4a5ffca9bb01b9679/durable-chat-template and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the React chat UI loads
2. Confirm a username is assigned (the original generates random names)
3. Type a message and send it, confirm it appears in the chat list
4. Open a second tab to the same room URL, send a message from each tab, and confirm both receive messages via WebSocket broadcast
5. Reload the page and confirm message history is preserved (the original persists to SQLite)
6. Navigate to a different room URL (e.g., append a different path/query) and confirm it creates a separate room

## Code review

Read through the migrated source and compare against the original. Check that:

- The `Server` class from `partyserver` is migrated to a RivetKit actor
- WebSocket connection handling with hibernation is preserved
- SQLite storage for message persistence (`ctx.storage.sql.exec()`) is migrated to actor SQLite
- `this.broadcast()` to all connections is implemented
- The React frontend with `usePartySocket` hook is migrated to use `@rivetkit/react` or equivalent
- URL-based room routing (different rooms via URL) is implemented via actor keys
- The shared types between client and server are preserved
- No original functionality was silently dropped

## Pass criteria

- React chat UI loads without errors
- Can send and see messages
- Messages broadcast to other connected clients
- Message history persists across page reload (SQLite-backed)
- Different room URLs create separate rooms
- No original features are missing from the migrated code
