Verify the migrated @cloudflare/actors sockets example works at {{URL}}.

Clone the original source from https://github.com/cloudflare/actors/tree/6bbf82b239016ecb205d3b40ff1aa9b8c88b2fa7/examples/sockets and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the page loads with a message input and send button
2. Type a message and click send, confirm a response is received
3. Confirm the WebSocket connection is established (check for connection indicators or response messages)
4. Test the disconnect button if present, confirm the connection closes
5. Reconnect and confirm messaging works again

## Code review

Read through the migrated source and compare against the original. Check that:

- WebSocket connection handling (`onWebSocketConnect`, `onWebSocketMessage`, `onWebSocketDisconnect`) has working equivalents
- Broadcasting with sender exclusion (`this.sockets.message('...', '*', [ws])`) is implemented
- The HTML client correctly connects via WebSocket to the actor
- No original functionality was silently dropped

## Pass criteria

- Page loads without errors
- Can send a message and receive a response via WebSocket
- WebSocket connection and disconnection work correctly
- Broadcasting logic is implemented
- No original features are missing from the migrated code
