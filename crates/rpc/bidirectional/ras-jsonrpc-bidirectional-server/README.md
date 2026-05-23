# ras-jsonrpc-bidirectional-server

Server-side WebSocket handling for bidirectional JSON-RPC communication with Axum integration.

## Features

- **WebSocket Server**: Axum WebSocket runtime for bidirectional JSON-RPC services
- **Authentication**: JWT-based authentication during WebSocket handshake
- **Connection Management**: Thread-safe connection tracking with DashMap
- **Message Routing**: Dispatch JSON-RPC requests to appropriate handlers
- **Subscription Support**: Topic-based subscription and broadcasting
- **Builder Pattern**: Ergonomic service configuration
- **Connection Lifecycle**: Proper cleanup on disconnect
- **Permission-based Access**: Role-based access control for connections

## Core Components

### DefaultConnectionManager

Thread-safe connection manager using DashMap for concurrent access:

```rust
use ras_jsonrpc_bidirectional_server::DefaultConnectionManager;

let manager = DefaultConnectionManager::new();
// Manages connections, subscriptions, and message routing
```

### WebSocketService

Main service trait for handling WebSocket connections:

```rust
use ras_jsonrpc_bidirectional_server::{WebSocketService, WebSocketServiceBuilder};

let service = WebSocketServiceBuilder::builder()
    .handler(Arc::new(router))
    .auth_provider(Arc::new(auth_provider))
    .require_auth(true)
    .build();
```

### MessageRouter

Routes JSON-RPC requests to appropriate handlers:

```rust
use ras_jsonrpc_bidirectional_server::MessageRouter;

let mut router = MessageRouter::new();

// Register a handler that returns a value
router.register_value("echo", |req, _ctx| async move {
    Ok::<serde_json::Value, ServerError>(req.params.unwrap_or(json!(null)))
});

// Register a notification handler (no response)
router.register_notification("log", |req, _ctx| async move {
    println!("Log: {:?}", req.params);
    Ok(())
});
```

### WebSocketUpgrade

Handles WebSocket upgrade with authentication:

```rust
use ras_jsonrpc_bidirectional_server::WebSocketUpgrade;

let ws_upgrade = WebSocketUpgrade::new(upgrade, headers);

// Authenticate during handshake
let user = ws_upgrade.authenticate(&auth_provider).await?;

// Extract metadata
let metadata = ws_upgrade.create_metadata();
```

## Authentication Flow

1. **WebSocket Handshake**: Authentication occurs during the WebSocket upgrade
2. **Token Extraction**: Supports multiple authentication methods:
   - `Authorization: Bearer <token>` header
   - `sec-websocket-protocol: token.<jwt>` header  
   - `X-Auth-Token: <token>` header
3. **Connection Context**: Authenticated user info is stored per connection
4. **Permission Checks**: Handlers can check user permissions

## Usage Example

```rust
use axum::{routing::get, Router};
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_bidirectional_server::{
    create_router_service, websocket_handler, MessageRouter, ServerError
};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};

struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token != "demo-token" {
                return Err(AuthError::InvalidToken);
            }

            Ok(AuthenticatedUser {
                user_id: "demo-user".to_string(),
                permissions: HashSet::from(["user".to_string()]),
                metadata: None,
            })
        })
    }
}

#[tokio::main]
async fn main() {
    // Create a message router
    let mut router = MessageRouter::new();
    
    // Register handlers
    router.register_value("echo", |req, _ctx| async move {
        Ok::<serde_json::Value, ServerError>(req.params.unwrap_or(json!(null)))
    });
    
    // Create WebSocket service
    let ws_service = create_router_service(
        router,
        Arc::new(DemoAuthProvider),
        true // require authentication
    );
    
    // Create Axum router
    let app = Router::new()
        .route("/ws", get(websocket_handler::<_>))
        .with_state(ws_service);
    
    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## Connection Management

The server automatically manages:

- **Connection Tracking**: Each connection gets a unique ID
- **User Authentication**: Optional JWT-based authentication
- **Subscriptions**: Topic-based pub/sub messaging
- **Metadata**: Connection info (IP, user agent, etc.)
- **Cleanup**: Automatic cleanup on disconnect

## Message Types

Supports all bidirectional message types:

- **JSON-RPC Requests/Responses**: Standard JSON-RPC 2.0 protocol
- **Server Notifications**: Server-initiated messages to clients
- **Broadcasts**: Messages to all subscribers of a topic
- **Subscriptions**: Subscribe/unsubscribe to topics
- **Heartbeat**: Ping/pong for connection keepalive

## Broadcasting

Send messages to multiple connections:

```rust
// Broadcast to all authenticated users
manager.broadcast_to_authenticated(message).await?;

// Broadcast to users with specific permission
manager.broadcast_to_permission("admin", message).await?;

// Broadcast to topic subscribers
manager.broadcast_to_topic("updates", message).await?;
```

## Integration with Axum

The server is designed to integrate with Axum:

- **WebSocket Extractor**: Compatible with Axum's WebSocket support
- **State Management**: Uses Axum's state system
- **Error Handling**: Proper HTTP error responses for upgrade failures
- **Middleware Support**: Works with Axum middleware

## Thread Safety

All components are thread-safe:

- **DashMap**: Lock-free concurrent HashMap for connections
- **Arc**: Shared ownership for handlers and providers  
- **Send + Sync**: All public types implement Send + Sync
- **Async**: Fully async/await compatible

## Dependencies

- `axum`: Web framework with WebSocket support
- `tokio`: Async runtime
- `dashmap`: Lock-free concurrent HashMap
- `serde_json`: JSON serialization
- `tracing`: Logging and instrumentation
- `futures`: Stream utilities

## Testing

Run tests with:

```bash
cargo test -p ras-jsonrpc-bidirectional-server --locked
```

The crate includes tests for:

- Message routing
- Service configuration
- Header parsing
- Connection management
- In-memory WebSocket handler round trips without binding sockets

## Checks

```bash
cargo test -p ras-jsonrpc-bidirectional-server --locked
cargo clippy -p ras-jsonrpc-bidirectional-server --all-targets --all-features --locked -- -D warnings
```
