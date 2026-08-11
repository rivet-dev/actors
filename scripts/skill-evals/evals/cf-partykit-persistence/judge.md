Verify the migrated PartyKit persistence example works at {{URL}}.

Clone the original source from https://github.com/partykit/partykit/tree/5527a744d25ff051a204806b85af504cb0fe2f7b/examples/persistence and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the page loads without errors
2. Find the counter display and note the initial value
3. Click the increment button, confirm the count increases
4. Click the decrement button, confirm the count decreases
5. Reload the page and confirm the count persisted across reload
6. Open a second tab to the same URL, change the count in one tab, and confirm the other tab receives the update via WebSocket broadcast

## Code review

Read through the migrated source and compare against the original. Check that:

- `room.storage.get()` / `room.storage.put()` KV persistence is migrated to actor KV or state
- `room.broadcast()` to all connections is implemented
- `onConnect` handler is migrated to equivalent WebSocket connection handling
- The HTML client with increment/decrement buttons is preserved
- No original functionality was silently dropped

## Pass criteria

- Page loads without errors
- Increment works
- Decrement works
- Count persists after reload
- Changes broadcast to other connected clients
- No original features are missing from the migrated code
