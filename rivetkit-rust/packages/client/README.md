# RivetKit Rust Client

_The Rust client for RivetKit, the Stateful Serverless Framework_

Use this client to connect to RivetKit services from Rust applications.

## Resources

- [Quickstart](https://rivet.dev/docs)
- [Documentation](https://rivet.dev/docs/clients/rust)
- [Examples](https://github.com/rivet-dev/rivet/tree/main/examples)

## Getting Started

### Step 1: Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
rivetkit-client = "0.1.0"
```

### Step 2: Connect to Actor

```rust
use rivetkit_client::{Client, ClientConfig, EncodingKind, GetOrCreateOptions, TransportKind};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a client connected to your RivetKit endpoint
    let client = Client::new(
        ClientConfig::new("http://localhost:8080")
            .transport(TransportKind::Ws)
            .encoding(EncodingKind::Json),
    );

    // Connect to a chat room actor
    let chat_room = client.get_or_create(
        "chat-room",
        ["keys-here"].into(),
        GetOrCreateOptions::default()
    )?.connect();

    // Listen for new messages
    chat_room.on_event("newMessage", |args| {
        let username = args[0].as_str().unwrap();
        let message = args[1].as_str().unwrap();
        println!("Message from {}: {}", username, message);
    }).await;

    // Send message to room
    chat_room.action("sendMessage", vec![
        json!("william"),
        json!("All the world's a stage.")
    ]).await?;

    // When finished
    client.disconnect();

    Ok(())
}
```

### Supported Transport Methods

The Rust client supports WebSocket transport for actor connections:

- `TransportKind::Ws`: WebSockets

`TransportKind::Sse` is reserved but is not currently implemented. This limitation only applies to the Rust client's actor-connection transport. Actors can still return raw server-sent event streams from an `onRequest` handler.

### Supported Encodings

The Rust client supports multiple encoding formats:

- `EncodingKind::Json`: JSON encoding
- `EncodingKind::Cbor`: CBOR binary encoding

## Community & Support

- Join our [Discord](https://rivet.dev/discord)
- Follow us on [X](https://x.com/rivet_gg)
- Follow us on [Bluesky](https://bsky.app/profile/rivet.gg)
- File bug reports in [GitHub Issues](https://github.com/rivet-dev/rivet/issues)
- Post questions & ideas in [GitHub Discussions](https://github.com/rivet-dev/rivet/discussions)

## License

Apache 2.0
