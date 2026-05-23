# ras-jsonrpc-bidirectional-client

Cross-platform WebSocket client for bidirectional JSON-RPC communication that works on both native and WASM targets.

## Features

- **Cross-platform**: Works on both native (x86_64) and WASM targets
- **JWT Authentication**: Support for JWT tokens via headers or connection parameters
- **Bidirectional Communication**: Send JSON-RPC requests and receive responses, plus handle server notifications
- **Subscription Management**: Subscribe to topics and receive targeted notifications
- **Connection Lifecycle**: Explicit connect/disconnect, connection status, and lifecycle events
- **Builder Pattern**: Ergonomic client configuration
- **Type Safety**: Leverages the type system for safe JSON-RPC communication

## Platform Support

### Native (x86_64, ARM, etc.)
- Uses `tokio-tungstenite` for async WebSocket communication
- Full async/await support with Tokio runtime
- Supports the WebSocket features used by the RAS bidirectional client runtime

### WASM (Browser)
- Uses `web-sys` WebSocket API for browser compatibility
- Compatible with `wasm-bindgen` and web frameworks
- Handles browser-specific WebSocket limitations gracefully

## Quick Start

Add to your `Cargo.toml`:

For native clients:

```toml
[dependencies]
ras-jsonrpc-bidirectional-client = "0.1.0"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
tokio = { version = "1.0", features = ["full"] }
```

For browser WASM clients:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
ras-jsonrpc-bidirectional-client = {
    version = "0.1.0",
    default-features = false,
    features = ["wasm"],
}
```

### Basic Usage

```rust
use ras_jsonrpc_bidirectional_client::ClientBuilder;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and connect client
    let client = ClientBuilder::new("ws://localhost:8080/ws")
        .with_jwt_token("demo-token".to_string())
        .with_auto_connect(true)
        .build()
        .await?;

    // Make a JSON-RPC call
    let response = client.call("get_user_info", Some(json!({"user_id": 123}))).await?;
    println!("Response: {:?}", response);

    // Send a notification (fire-and-forget)
    client.notify("user_activity", Some(json!({"action": "page_view"}))).await?;

    Ok(())
}
```

### Handling Notifications

```rust
use std::sync::Arc;

// Register a notification handler
client.on_notification("user_message", Arc::new(|method, params| {
    println!("Received notification {}: {:?}", method, params);
}));

// Subscribe to a topic
client.subscribe("chat_room_123", Arc::new(|method, params| {
    println!("Chat message: {:?}", params);
})).await?;
```

### Connection Events

```rust
// Handle connection lifecycle events emitted by the client
client.on_connection_event("main", Arc::new(|event| {
    match event {
        ConnectionEvent::Connected { connection_id } => {
            println!("Connected with ID: {}", connection_id);
        }
        ConnectionEvent::Disconnected { reason } => {
            println!("Disconnected: {:?}", reason);
        }
        _ => {}
    }
}));
```

### Advanced Configuration

```rust
use ras_jsonrpc_bidirectional_client::ClientBuilder;
use std::time::Duration;

let client = ClientBuilder::new("wss://api.example.com/ws")
    .with_jwt_token("demo-token".to_string())
    .with_jwt_in_header(true)
    .with_header("User-Agent", "MyApp/1.0")
    .with_request_timeout(Duration::from_secs(30))
    .with_connection_timeout(Duration::from_secs(10))
    .with_heartbeat_interval(Some(Duration::from_secs(30)))
    .build()
    .await?;
```

## Authentication

The client supports multiple authentication methods:

### JWT in Authorization Header
```rust
let client = ClientBuilder::new("ws://localhost:8080/ws")
    .with_jwt_token("demo-token".to_string())
    .with_jwt_in_header(true)  // Default
    .build()
    .await?;
```

### JWT as Connection Parameter
```rust
let client = ClientBuilder::new("ws://localhost:8080/ws")
    .with_jwt_token("demo-token".to_string())
    .with_jwt_in_header(false)
    .build()
    .await?;
```

### Custom Headers
```rust
let client = ClientBuilder::new("ws://localhost:8080/ws")
    .with_header("X-API-Key", "demo-api-key")
    .with_header("X-Client-Version", "1.0.0")
    .build()
    .await?;
```

## Error Handling

The client exposes typed errors for connection, authentication, timeout, and protocol failures:

```rust
use ras_jsonrpc_bidirectional_client::ClientError;

match client.call("some_method", None).await {
    Ok(response) => {
        // Handle successful response
    }
    Err(ClientError::Timeout { timeout_seconds }) => {
        println!("Request timed out after {}s", timeout_seconds);
    }
    Err(ClientError::NotConnected) => {
        println!("Client is not connected");
        // Attempt to reconnect
        client.connect().await?;
    }
    Err(ClientError::Authentication(msg)) => {
        println!("Authentication failed: {}", msg);
    }
    Err(e) => {
        println!("Other error: {}", e);
    }
}
```

## Connection Management

### Manual Connection Control
```rust
// Connect manually
client.connect().await?;

// Check connection status
if client.is_connected().await {
    println!("Connected with ID: {:?}", client.connection_id().await);
}

// Disconnect
client.disconnect().await?;
```

### Reconnecting After Failure
The client does not spawn a background reconnect loop. If a request returns
`ClientError::NotConnected` or `client.state().await` is `ClientState::Failed`,
run your own retry loop and call `connect()` again. `ReconnectConfig` provides
backoff and maximum-attempt helpers for callers that want a shared retry policy:

- **Exponential backoff**: Delays increase exponentially between attempts
- **Jitter**: Random variation to prevent thundering herd
- **Maximum attempts**: Limit reconnection attempts

## WASM Considerations

When using in WASM environments:

1. **Feature flags**: Use the `wasm` feature for WASM targets
2. **Error handling**: JavaScript errors are wrapped in `ClientError::JavaScript`
3. **Console logging**: Use `console.log` for debugging in browsers
4. **Async runtime**: Use `wasm-bindgen-futures` for Promise integration

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
ras-jsonrpc-bidirectional-client = {
    version = "0.1.0",
    default-features = false,
    features = ["wasm"],
}
```

## Testing

The native implementation is covered by the regular Rust test suite. The WASM
feature can be checked with the `wasm32-unknown-unknown` target:

```bash
# Test native implementation
cargo test -p ras-jsonrpc-bidirectional-client --locked

# Check WASM feature build
cargo check -p ras-jsonrpc-bidirectional-client --locked \
  --target wasm32-unknown-unknown \
  --no-default-features \
  --features wasm

# Check native feature build explicitly
cargo check -p ras-jsonrpc-bidirectional-client --locked \
  --features native
```

## Checks

```bash
cargo test -p ras-jsonrpc-bidirectional-client --locked
cargo clippy -p ras-jsonrpc-bidirectional-client --all-targets --features native --locked -- -D warnings
cargo check -p ras-jsonrpc-bidirectional-client --locked \
  --target wasm32-unknown-unknown \
  --no-default-features \
  --features wasm
```

## Examples

See the `examples/` directory for usage examples:

- **Basic client**: Simple request/response
- **Subscription example**: Topic-based notifications
- **Authentication example**: JWT and custom auth
- **WASM example**: Browser-based client
- **Reconnection example**: Handling connection failures

## License

This project is licensed under either MIT or Apache-2.0. See
[LICENSE-MIT](../../../../LICENSE-MIT) and [LICENSE-APACHE](../../../../LICENSE-APACHE).
