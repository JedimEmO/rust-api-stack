# ras-jsonrpc-bidirectional-types

Core types for bidirectional JSON-RPC communication over WebSockets.

## Overview

This crate provides the fundamental types and traits needed for bidirectional JSON-RPC communication, including:

- **Connection Management**: `ConnectionId`, `ConnectionInfo`, and `ConnectionManager` trait
- **Message Types**: `BidirectionalMessage` enum for all message types that can flow between client and server
- **Message Sending**: `MessageSender` trait and `WebSocketMessageSender` implementation
- **Authentication Integration**: Built-in support for authenticated connections using `ras-auth-core`
- **Subscription Management**: Topic-based subscription system for broadcast messages

## Key Types

### `ConnectionId`
A unique identifier for each WebSocket connection, backed by a UUID.

### `BidirectionalMessage`
An enum representing all possible message types:
- `Request`: JSON-RPC request (from client or server)
- `Response`: JSON-RPC response (from client or server)
- `ServerNotification`: Server-initiated notification to specific clients
- `Broadcast`: Server broadcast to all subscribed clients
- `Subscribe`/`Unsubscribe`: Subscription management
- `ConnectionEstablished`/`ConnectionClosed`: Connection lifecycle
- `Ping`/`Pong`: Heartbeat messages

### `ConnectionInfo`
Stores information about each connection:
- Connection ID
- Authenticated user (optional)
- Topic subscriptions
- Metadata
- Connection timestamp

### Traits

#### `ConnectionManager`
Manages WebSocket connections, including:
- Adding/removing connections
- Authentication management
- Subscription handling
- Message routing and broadcasting

#### `MessageSender`
Sends messages over a WebSocket connection with convenience methods for:
- JSON-RPC requests/responses
- Server notifications
- Ping/pong
- Subscription updates

## Usage Example

```rust
use ras_jsonrpc_bidirectional_types::{
    ConnectionId, ConnectionInfo, MessageSender, MessageSenderExt, NoOpMessageSender,
};
use serde_json::json;

// Create a connection
let conn_id = ConnectionId::new();
let mut info = ConnectionInfo::new(conn_id);

// Subscribe to topics
info.subscribe("updates".to_string());
info.subscribe("notifications".to_string());

// Send messages through any MessageSender implementation. NoOpMessageSender is
// useful for tests or dry-run flows; production code usually uses a WebSocket sender.
let sender = NoOpMessageSender::with_connection_id(conn_id);
assert_eq!(sender.connection_id(), conn_id);
sender.send_ping().await?;
sender.send_notification("user.updated", json!({"id": 123})).await?;
```

## Features

- Full bidirectional JSON-RPC support
- Type-safe message handling
- Built-in authentication and permission checking
- Topic-based publish/subscribe pattern
- WebSocket integration with tokio-tungstenite
- Extensible traits for custom implementations

## Checks

```bash
cargo test -p ras-jsonrpc-bidirectional-types --locked
cargo clippy -p ras-jsonrpc-bidirectional-types --all-targets --all-features --locked -- -D warnings
```
