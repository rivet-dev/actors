Verify the migrated PartyKit socket.io chat example works at {{URL}}.

Clone the original source from https://github.com/partykit/partykit/tree/5527a744d25ff051a204806b85af504cb0fe2f7b/examples/socket.io-chat and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the chat UI loads with a login/username prompt
2. Enter a username and join the chat
3. Type a message and send it, confirm it appears in the chat
4. Confirm typing indicator functionality (if the UI shows "user is typing..." when typing in the input)
5. Open a second tab, join with a different username, and confirm messages from one tab appear in the other
6. Confirm user join/leave notifications appear in the chat
7. Check that the user count or participant list updates

## Code review

Read through the migrated source and compare against the original. Check that:

- The `party.io` socket.io adapter pattern is migrated to native RivetKit WebSocket handling (socket.io events become WebSocket messages)
- Socket.IO events (new message, add user, typing, stop typing, disconnect) are all preserved as WebSocket message types
- `socket.broadcast.emit` (broadcast with sender exclusion) is implemented
- Cross-party HTTP communication (`lobby.parties.main.get(partyName).fetch("/user-count")`) is migrated to actor-to-actor calls if present
- User connection tracking (join/leave notifications, user count) is preserved
- The chat UI with login, messages, and typing indicators is preserved
- No original functionality was silently dropped

## Pass criteria

- Chat UI loads without errors
- Can enter a username and join
- Sending a message shows it in the chat
- Messages broadcast to other connected clients
- Typing indicators work
- User join/leave notifications appear
- No original features are missing from the migrated code
