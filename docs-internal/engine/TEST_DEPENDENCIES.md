# Test Dependencies

The `rivet-test-deps` package provides isolated test environments with configurable database and pub/sub backends.

## Configuration

Configure backends via environment variables:

- `RIVET_TEST_DATABASE`: Choose database backend
  - `foundationdb` - Runs FoundationDB in Docker
  - `postgres` - PostgreSQL in Docker
  - `filesystem` - RocksDB with temp directory (default)

- `RIVET_TEST_PUBSUB`: Choose pub/sub backend
  - `nats` - Runs NATS in Docker
  - `memory` - In-memory channels (default)

- `RUST_LOG`: Enable debug logs to see container lifecycle details
	- For example, `RUST_LOG=debug` will enable more verbose logs

## How It Works

1. **Port Selection**: Automatically picks unused ports for all services
2. **Container Management**: 
   - Starts Docker containers for services that require them
   - Each test gets unique container names using UUIDs
   - Containers are automatically cleaned up on drop
3. **Config Generation**: Creates a complete Rivet config with the selected backends
4. **Isolation**: Each test instance gets isolated resources (unique channels, temp dirs, containers)

## Usage

```rust
let deps = TestDeps::new().await?;
// Use deps.pools() and deps.config() for your tests
// Containers are cleaned up automatically when deps is dropped
```

For multi-datacenter tests:
```rust
let deps = TestDeps::new_multi(&[1, 2, 3]).await?;
```
