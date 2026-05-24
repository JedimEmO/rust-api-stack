# ras-jsonrpc-macro

Procedural macros for generating type-safe JSON-RPC services with authentication and axum integration.

## Overview

This crate provides the `jsonrpc_service!` procedural macro that generates type-safe JSON-RPC services with built-in authentication, authorization, and axum integration. It transforms a declarative service definition into a JSON-RPC router with compile-time checks for the generated service trait.

## Features

- **Declarative service definition**: Clean, readable syntax for defining JSON-RPC methods
- **Authentication integration**: Built-in support for `UNAUTHORIZED` and `WITH_PERMISSIONS` methods
- **Type safety**: Compile-time validation of request/response types
- **Axum integration**: Generates standard axum `Router` for easy composition
- **Trait-based service wiring**: Implement one generated trait and pass it to the service builder
- **Versioned methods**: Optional request/response migrations for legacy wire methods
- **Async support**: Full async/await support throughout
- **JSON-RPC 2.0 responses**: Generates standard success and error envelopes
- **OpenRPC document generation**: Automatic API documentation generation

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
ras-jsonrpc-macro = { version = "0.2.0", default-features = false }
ras-jsonrpc-core = { version = "0.1.2", optional = true }
ras-jsonrpc-types = "0.1.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.0.0-alpha.20"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
axum = { version = "0.8", optional = true }
tokio = { version = "1.0", features = ["full"], optional = true }
reqwest = { version = "0.12", features = ["json"], optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json"], optional = true }

[features]
default = ["server"]
server = [
    "ras-jsonrpc-macro/server",
    "dep:ras-jsonrpc-core",
    "dep:axum",
    "dep:tokio",
]
client = ["ras-jsonrpc-macro/client", "dep:reqwest"]
```

## Quick Start

### 1. Define Your Service

```rust
use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SignInRequest {
    email: String,
    password: String,
}

#[derive(Serialize, Deserialize)]
struct SignInResponse {
    jwt: String,
    user_id: String,
}

jsonrpc_service!({
    service_name: MyService,
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS(["admin"]) delete_user(UserId) -> (),
    ]
});
```

### 2. Implement an Auth Provider

```rust
use ras_jsonrpc_core::{AuthProvider, AuthenticatedUser, AuthFuture};
use std::collections::HashSet;

struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            // Validate the bearer token (simplified)
            if token.starts_with("valid_") {
                let mut permissions = HashSet::new();
                permissions.insert("user".to_string());
                
                if token.contains("admin") {
                    permissions.insert("admin".to_string());
                }
                
                Ok(AuthenticatedUser {
                    user_id: "user123".to_string(),
                    permissions,
                    metadata: None,
                })
            } else {
                Err(ras_jsonrpc_core::AuthError::InvalidToken)
            }
        })
    }
}
```

### 3. Build and Run Your Service

```rust
use axum::{Router, routing::get};

struct MyServiceImpl;

impl MyServiceTrait for MyServiceImpl {
    async fn sign_in(
        &self,
        _request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(SignInResponse {
            jwt: "valid_user_token".to_string(),
            user_id: "123".to_string(),
        })
    }

    async fn get_profile(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserProfile {
            name: format!("User {}", user.user_id),
            email: "user@example.com".to_string(),
        })
    }

    async fn delete_user(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        user_id: UserId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Admin {} deleting user {:?}", user.user_id, user_id);
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .nest("/api",
            MyServiceBuilder::new(MyServiceImpl)
                .base_url("/rpc")
                .auth_provider(MyAuthProvider)
                .build()
                .expect("service should build")
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server running on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
```

## Macro Syntax

### Service Definition

```rust
jsonrpc_service!({
    service_name: ServiceName,  // Name of the generated service
    openrpc: true,              // Optional: Enable OpenRPC generation
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS(["admin"]) delete_user(UserId) -> (),
    ]
});
```

### Method Definitions

#### Unauthorized Methods
```rust
UNAUTHORIZED method_name(RequestType) -> ResponseType,
```
- No authentication required
- Trait method signature: `fn method(&self, RequestType) -> impl Future<Output = Result<ResponseType, Error>> + Send`

#### Permission-Based Methods
```rust
WITH_PERMISSIONS(["perm1", "perm2"]) method_name(RequestType) -> ResponseType,
```
- Requires valid authentication
- Requires all listed permissions in the group
- Use `WITH_PERMISSIONS(["admin"] | ["moderator", "editor"])` for OR between permission groups
- Trait method signature: `fn method(&self, &AuthenticatedUser, RequestType) -> impl Future<Output = Result<ResponseType, Error>> + Send`

#### Empty Permissions (Any Valid Token)
```rust
WITH_PERMISSIONS([]) method_name(RequestType) -> ResponseType,
```
- Requires valid authentication
- No specific permissions required
- Trait method signature: `fn method(&self, &AuthenticatedUser, RequestType) -> impl Future<Output = Result<ResponseType, Error>> + Send`

## Generated Code

The macro generates:

### Service Builder
```rust
pub trait MyServiceTrait: Send + Sync + 'static {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>>;

    async fn get_profile(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>>;

    async fn delete_user(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        request: UserId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

impl<T: MyServiceTrait> MyServiceBuilder<T> {
    pub fn new(service: T) -> Self;
    pub fn base_url(self, base_url: impl Into<String>) -> Self;
    pub fn auth_provider<A: ras_jsonrpc_core::AuthProvider>(self, provider: A) -> Self;
    pub fn build(self) -> Result<axum::Router, String>;
}
```

### Request Handling
- Automatic JSON-RPC request/response parsing
- Authentication token extraction from `Authorization` header
- Permission validation
- Error handling with proper JSON-RPC error codes

## Versioned Methods

Versioning is opt-in. By default, the Rust method name is also the JSON-RPC wire method. Add a method block when you need a canonical wire name and one or more legacy compatibility methods.

```rust
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserV1 {
    name: String,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserV2 {
    display_name: String,
    notify: bool,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserResponseV1 {
    name: String,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserResponseV2 {
    display_name: String,
    notified: bool,
}

struct RenameUserCompat;

impl ras_jsonrpc_core::VersionMigration<RenameUserV1, RenameUserV2> for RenameUserCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserV1) -> Result<RenameUserV2, Self::Error> {
        Ok(RenameUserV2 {
            display_name: value.name,
            notify: false,
        })
    }
}

impl ras_jsonrpc_core::VersionMigration<RenameUserResponseV2, RenameUserResponseV1>
    for RenameUserCompat
{
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserResponseV2) -> Result<RenameUserResponseV1, Self::Error> {
        Ok(RenameUserResponseV1 {
            name: value.display_name,
        })
    }
}

jsonrpc_service!({
    service_name: UserService,
    openrpc: true,
    methods: [
        UNAUTHORIZED rename_user(RenameUserV2) -> RenameUserResponseV2 {
            version: v2,
            wire: "rename_user.v2",
            versions: [
                v1 {
                    wire: "rename_user.v1",
                    request: RenameUserV1,
                    response: RenameUserResponseV1,
                    migration: RenameUserCompat,
                },
            ],
        },
    ]
});
```

The generated server accepts both `rename_user.v2` and `rename_user.v1`. The generated Rust client exposes `rename_user(...)` for the canonical method and `rename_user_v1(...)` for the legacy method. Version labels can be identifiers such as `v1` or string labels such as `"1.0.0"`; string labels are sanitized for Rust method suffixes, for example `rename_user_v1_0_0(...)`.

## Authentication Flow

### 1. Token Extraction
The generated service automatically extracts Bearer tokens from the `Authorization` header:
```
Authorization: Bearer <token>
```

### 2. Method Routing
- `UNAUTHORIZED` methods bypass authentication
- `WITH_PERMISSIONS` methods require valid authentication and authorization

### 3. Error Responses
Authentication failures return proper JSON-RPC 2.0 error responses:

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "Authentication required"
  },
  "id": 1
}
```

## JSON-RPC Client Examples

### Sign In (Unauthorized)
```bash
curl -X POST http://localhost:3000/api/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "sign_in",
    "params": {
      "email": "user@example.com",
      "password": "secret"
    },
    "id": 1
  }'
```

### Get Profile (With Authentication)
```bash
curl -X POST http://localhost:3000/api/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer valid_user_token" \
  -d '{
    "jsonrpc": "2.0",
    "method": "get_profile",
    "params": {},
    "id": 2
  }'
```

### Delete User (Admin Permission Required)
```bash
curl -X POST http://localhost:3000/api/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer valid_admin_token" \
  -d '{
    "jsonrpc": "2.0",
    "method": "delete_user",
    "params": {"id": "user456"},
    "id": 3
  }'
```

## Error Handling

The macro generates typed error handling for:

- **Parse Errors**: Invalid JSON (-32700)
- **Invalid Request**: Malformed JSON-RPC (-32600)  
- **Method Not Found**: Unknown method (-32601)
- **Invalid Params**: Type mismatch (-32602)
- **Authentication Required**: Missing/invalid token (-32001)
- **Insufficient Permissions**: Missing permissions (-32002)
- **Internal Errors**: Handler errors (-32603)
- **Migration Errors**: Legacy request migration failures are invalid params (-32602); legacy response migration failures are internal errors (-32603)

## OpenRPC Document Generation

The macro can automatically generate OpenRPC specification documents for your JSON-RPC API. This provides machine-readable API documentation for clients, API explorers, and external tooling.

### Enabling OpenRPC

#### Default Output Path
```rust
jsonrpc_service!({
    service_name: MyService,
    openrpc: true,  // Generates to target/openrpc/myservice.json
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
    ]
});
```

#### Custom Output Path
```rust
jsonrpc_service!({
    service_name: MyService,
    openrpc: { output: "docs/api/myservice.json" },
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
    ]
});
```

### Generated Functions

When OpenRPC is enabled, the macro generates two additional functions:

```rust
// Generate OpenRPC document as a serde_json::Value
pub fn generate_myservice_openrpc() -> serde_json::Value

// Generate and write OpenRPC document to file
pub fn generate_myservice_openrpc_to_file() -> Result<(), std::io::Error>
```

### Requirements

All request and response types must implement the `schemars::JsonSchema` trait:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
struct MyRequest {
    /// Field documentation appears in the schema
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_field: Option<String>,
}
```

### OpenRPC Output

The generated OpenRPC document includes:

- **Service metadata**: Title, version, description
- **Method specifications**: Name, parameters, results
- **JSON Schemas**: Type definitions with descriptions
- **Authentication metadata**: `x-authentication` and `x-permissions` extensions for each method
- **Version metadata**: `x-ras-version`, `x-ras-canonical-version`, and `x-ras-canonical-method` extensions for versioned methods

### Example

```rust
use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
struct CreateUserRequest {
    /// User's email address
    email: String,
    /// User's display name
    name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct User {
    id: String,
    email: String,
    name: String,
}

jsonrpc_service!({
    service_name: UserService,
    openrpc: true,
    methods: [
        WITH_PERMISSIONS(["admin"]) create_user(CreateUserRequest) -> User,
    ]
});

// In your main function or build script:
fn main() {
    // Generate and save the OpenRPC document
    if let Err(e) = generate_userservice_openrpc_to_file() {
        eprintln!("Failed to generate OpenRPC: {}", e);
    }
}
```

This generates an OpenRPC document at `target/openrpc/userservice.json` with:
- Method documentation
- JSON schemas for all types
- Authentication requirements (`x-authentication: true`)
- Permission requirements (`x-permissions: ["admin"]`)

## Integration

This crate works with:

- [`ras-jsonrpc-core`](../ras-jsonrpc-core) - Authentication traits and types
- [`ras-jsonrpc-types`](../ras-jsonrpc-types) - JSON-RPC protocol types
- [`axum`](https://crates.io/crates/axum) - Web framework

## Examples

See the [`examples/`](../../../examples/) directory for usage examples:

- [`basic-jsonrpc-service`](../../../examples/basic-jsonrpc/service) - Runnable service with authentication
- [`usage.rs`](examples/usage.rs) - Standalone usage example
- [`openrpc_demo.rs`](examples/openrpc_demo.rs) - OpenRPC document generation example

## Checks

```bash
cargo test -p ras-jsonrpc-macro --locked
cargo clippy -p ras-jsonrpc-macro --all-targets --all-features --locked -- -D warnings
```

## License

This project is licensed under either MIT or Apache-2.0.
