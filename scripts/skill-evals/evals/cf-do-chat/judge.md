Verify the migrated Cloudflare Workers chat demo works at {{URL}}.

Clone the original source from https://github.com/cloudflare/workers-chat-demo/tree/dd32ce87617a9df6c614004d2fc2fb0628698121 and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the page loads with a room selection or chat UI
2. Enter a username and join a chat room
3. Send a message and confirm it appears in the chat
4. Open a second tab to the same room, send a message from each tab, and confirm both tabs receive messages via WebSocket broadcast
5. Reload the page and confirm chat history is preserved (messages persisted to storage)
6. Send several messages rapidly and confirm rate limiting behavior works (the original uses a separate RateLimiter DO)

## Code review

Read through the migrated source and compare against the original. Check that:

- WebSocket connection handling (connect, message, close) is fully implemented
- WebSocket hibernation behavior is preserved (connections survive actor sleep)
- Chat history is persisted to storage (KV or SQLite)
- Broadcast to all connected clients works with sender identification
- Rate limiting logic exists (either as a separate actor or inline)
- Session/attachment metadata is maintained across WebSocket reconnections
- No original functionality was silently dropped

## Pass criteria

- Page loads without errors
- Can join a room and set a username
- Sending a message shows it in the chat
- Messages broadcast to other connected clients in real-time
- Chat history persists across page reload
- Rate limiting is implemented
- No original features are missing from the migrated code
