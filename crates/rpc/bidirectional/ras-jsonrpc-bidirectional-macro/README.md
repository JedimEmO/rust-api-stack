# ras-jsonrpc-bidirectional-macro

Procedural macro for generating type-safe bidirectional JSON-RPC services over WebSockets.

See the canonical mdBook
[`jsonrpc_bidirectional_service!` guide](../../../../documentation/src/macros/bidirectional-jsonrpc-service.md)
for the rationale, auth model, usage flow, and runnable examples.

This crate provides the `jsonrpc_bidirectional_service!` macro that generates both server and client code for bidirectional JSON-RPC communication, including authentication support and type-safe message enums.

## Features

- **Server Code Generation**: Generates service traits and handlers for client-to-server JSON-RPC methods
- **Client Code Generation**: Generates type-safe client structs with method calls and notification handlers
- **Authentication Integration**: Supports JWT-based authentication with permission-based access control
- **Type Safety**: Generated Rust request and response paths are checked at compile time
- **WebSocket Integration**: Works with the bidirectional runtime crates

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ras-auth-core = "0.1.0"
ras-jsonrpc-types = "0.1.1"
ras-jsonrpc-bidirectional-types = "0.1.0"
ras-jsonrpc-bidirectional-macro = { version = "0.1.0", default-features = false }
ras-jsonrpc-bidirectional-server = { version = "0.1.0", optional = true }
ras-jsonrpc-bidirectional-client = { version = "0.1.0", optional = true }

[features]
default = []
server = [
    "dep:ras-jsonrpc-bidirectional-server",
]
client = [
    "dep:ras-jsonrpc-bidirectional-client",
]
```

The generated code checks the API crate's `server` and `client` features.
Downstream server and client crates select behavior by enabling those features
on the shared API crate dependency.

If you define `server_to_client_calls`, also add `tokio = { version = "1.0", features = ["sync", "time"], optional = true }` and `uuid = { version = "1", features = ["v4"], optional = true }`, then include `dep:tokio` and `dep:uuid` in the `server` feature. The generated server-side client handle uses them for pending response channels, timeouts, and request IDs.

### Basic Example

```rust
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRequest {
    pub user_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub message: String,
    pub timestamp: u64,
}

// Generate bidirectional service
jsonrpc_bidirectional_service!({
    service_name: UserService,
    client_to_server: [
        UNAUTHORIZED get_user(UserRequest) -> UserResponse,
        WITH_PERMISSIONS(["admin"]) delete_user(UserRequest) -> bool,
        WITH_PERMISSIONS(["write"] | ["admin"]) update_user(UserRequest) -> UserResponse,
    ],
    server_to_client: [
        status_notification(StatusUpdate),
        user_updated(UserResponse),
    ],
    server_to_client_calls: [
    ]
});
```

This generates:

#### Server Side (with `#[cfg(feature = "server")]`)

```rust
// Service trait to implement
#[async_trait::async_trait]
pub trait UserServiceService: Send + Sync {
    async fn get_user(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>>;

    async fn delete_user(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: UserRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    async fn update_user(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>>;
    
    // Notification methods
    async fn notify_status_notification(&self, connection_id: ConnectionId, params: StatusUpdate) -> ras_jsonrpc_bidirectional_types::Result<()>;
    async fn notify_user_updated(&self, connection_id: ConnectionId, params: UserResponse) -> ras_jsonrpc_bidirectional_types::Result<()>;
}

// The server feature also emits `UserServiceHandler` and
// `UserServiceBuilder::new(service, auth_provider)` for Axum wiring.
```

#### Client Side (with `#[cfg(feature = "client")]`)

```rust
impl UserServiceClient {
    // Method calls
    pub async fn get_user(&self, request: UserRequest) -> ClientResult<UserResponse>;
    pub async fn delete_user(&self, request: UserRequest) -> ClientResult<bool>;
    pub async fn update_user(&self, request: UserRequest) -> ClientResult<UserResponse>;
    
    // Notification handlers
    pub fn on_status_notification<F>(&mut self, handler: F)
    where F: Fn(StatusUpdate) + Send + Sync + 'static;
    
    pub fn on_user_updated<F>(&mut self, handler: F) 
    where F: Fn(UserResponse) + Send + Sync + 'static;
    
    // Connection management
    pub async fn connect(&self) -> ClientResult<()>;
    pub async fn disconnect(&self) -> ClientResult<()>;
    pub async fn is_connected(&self) -> bool;
}

// The client feature also emits `UserServiceClientBuilder` for connection
// configuration and typed client construction.
```

### Server Implementation Example

```rust
use axum::{routing::get, Router};
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_bidirectional_server::websocket_handler;
use ras_jsonrpc_bidirectional_types::{ConnectionId, ConnectionManager};
use std::collections::HashSet;

struct MyUserService;

#[async_trait::async_trait]
impl UserServiceService for MyUserService {
    async fn get_user(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Implementation
        Ok(UserResponse {
            name: "John Doe".to_string(),
            email: "john@example.com".to_string(),
        })
    }
    
    async fn delete_user(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: UserRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Check user permissions are automatically validated by the generated code
        // Implementation
        Ok(true)
    }
    
    async fn update_user(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Implementation
        Ok(UserResponse {
            name: "Updated Name".to_string(),
            email: "updated@example.com".to_string(),
        })
    }
    
    async fn notify_status_notification(
        &self,
        _connection_id: ConnectionId,
        _params: StatusUpdate,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }
    
    async fn notify_user_updated(
        &self,
        _connection_id: ConnectionId,
        _params: UserResponse,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }
}

struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
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

// Create and run server
#[tokio::main]
async fn main() {
    let service = MyUserService;
    let auth_provider = MyAuthProvider;
    
    let websocket_service = UserServiceBuilder::new(service, auth_provider)
        .require_auth(false) // Set to true to require authentication for all methods
        .build();
    
    let app = Router::new()
        .route("/ws", get(websocket_handler::<_>))
        .with_state(websocket_service);
    
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

### Client Usage Example

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = UserServiceClientBuilder::new("ws://localhost:8080/ws")
        .with_jwt_token("demo-token".to_string())
        .build()
        .await?;
    
    // Register notification handlers
    client.on_status_notification(|status| {
        println!("Status update: {}", status.message);
    });
    
    client.on_user_updated(|user| {
        println!("User updated: {}", user.name);
    });
    
    // Connect to server
    client.connect().await?;
    
    // Make RPC calls
    let user = client.get_user(UserRequest { user_id: 123 }).await?;
    println!("User: {:?}", user);
    
    let deleted = client.delete_user(UserRequest { user_id: 123 }).await?;
    println!("Deleted: {}", deleted);
    
    Ok(())
}
```

## Macro Syntax

```rust
jsonrpc_bidirectional_service!({
    service_name: ServiceName,
    client_to_server: [
        UNAUTHORIZED method_name(RequestType) -> ResponseType,
        WITH_PERMISSIONS(["perm1", "perm2"]) method_name(RequestType) -> ResponseType,
        WITH_PERMISSIONS(["perm1"] | ["perm2"]) method_name(RequestType) -> ResponseType, // OR groups
    ],
    server_to_client: [
        notification_name(NotificationType),
        another_notification(AnotherType),
    ],
    server_to_client_calls: [
        server_call_name(RequestType) -> ResponseType,
    ]
});
```

### Authentication

- `UNAUTHORIZED`: No authentication required
- `WITH_PERMISSIONS(["perm1", "perm2"])`: User must have ALL listed permissions (AND logic)
- `WITH_PERMISSIONS(["perm1"] | ["perm2"])`: User must have permissions from ANY group (OR logic between groups, AND within groups)

### OpenRPC Generation

This bidirectional WebSocket macro does not currently generate OpenRPC documents. OpenRPC generation is available in `ras-jsonrpc-macro` for HTTP JSON-RPC services.

## Requirements

All request, response, and notification parameter types must implement:
- `serde::Serialize` + `serde::Deserialize`
- `Send` + `Sync` + `'static`

## Testing

Run tests with:

```bash
cargo test -p ras-jsonrpc-bidirectional-macro --locked
```

The end-to-end tests exercise generated service dispatch through the in-memory
WebSocket adapter. They do not bind sockets.

## Checks

```bash
cargo test -p ras-jsonrpc-bidirectional-macro --locked
cargo clippy -p ras-jsonrpc-bidirectional-macro --all-targets --all-features --locked -- -D warnings
```

## Generated Code Structure

The macro generates code conditionally compiled based on features:

- `#[cfg(feature = "server")]`: Server traits, handlers, and builders
- `#[cfg(feature = "client")]`: Client structs, builders, and message enums

This allows each API crate to expose only the generated surface its downstream
server or client crates need.

## Error Handling

Generated code provides typed error handling for:

- **Authentication errors**: Automatic JWT validation and permission checking
- **Serialization errors**: Type-safe JSON conversion with helpful error messages
- **Connection errors**: WebSocket connection management and recovery
- **Method errors**: User-defined error types from service implementations

## Integration with Runtime Crates

This macro works with the following runtime crates:

- `ras-jsonrpc-bidirectional-types`: Core types and traits
- `ras-jsonrpc-bidirectional-server`: Server-side WebSocket handling  
- `ras-jsonrpc-bidirectional-client`: Client-side WebSocket communication
- `ras-auth-core`: Authentication provider traits
- `ras-jsonrpc-types`: JSON-RPC 2.0 protocol types
