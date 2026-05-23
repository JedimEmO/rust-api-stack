# ras-jsonrpc-core

Core authentication and authorization traits for JSON-RPC services.

## Overview

This crate provides the foundational authentication, authorization, and version migration traits used by the `ras-jsonrpc-macro` procedural macro to generate type-safe JSON-RPC services with axum integration. It defines the `AuthProvider` trait that enables flexible authentication mechanisms while maintaining a consistent interface.

## Features

- **Async authentication**: Full async/await support for authentication operations
- **Permission-based authorization**: Fine-grained permission checking
- **Flexible auth providers**: Support for JWT, API keys, or custom authentication
- **Typed error handling**: Explicit error variants for common authentication scenarios
- **Permission helpers**: Default `check_permissions` logic on `AuthProvider`
- **Version migration**: Re-exports `VersionMigration` for opt-in API compatibility paths
- **Integration ready**: Re-exports JSON-RPC types for convenience

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
ras-jsonrpc-core = "0.1.2"
```

### Implementing an Auth Provider

```rust
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::{HashMap, HashSet};

struct DemoAuthProvider {
    users_by_token: HashMap<String, AuthenticatedUser>,
}

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            self.users_by_token
                .get(&token)
                .cloned()
                .ok_or(AuthError::InvalidToken)
        })
    }
}

let mut users_by_token = HashMap::new();
users_by_token.insert(
    "admin-token".to_string(),
    AuthenticatedUser {
        user_id: "admin-user".to_string(),
        permissions: HashSet::from(["admin".to_string(), "user".to_string()]),
        metadata: None,
    },
);

let auth_provider = DemoAuthProvider { users_by_token };
```

### Using with Permissions

```rust
use ras_jsonrpc_core::{AuthProvider, AuthResult};

async fn example_usage(auth_provider: &impl AuthProvider) -> AuthResult<()> {
    let user = auth_provider.authenticate("admin-token".to_string()).await?;

    auth_provider.check_permissions(
        &user,
        &["admin".to_string()]
    )?;

    Ok(())
}
```

## Authentication Flow

### 1. Token Validation
```rust
// Extract token from request headers
let token = extract_bearer_token(&headers)?;

// Validate token
let user = auth_provider.authenticate(token).await?;
```

### 2. Permission Checking
```rust
// Check if user has required permissions
auth_provider.check_permissions(
    &user,
    &["admin".to_string(), "write".to_string()]
)?;
```

### 3. Authenticate Then Authorize
```rust
let user = auth_provider.authenticate(token).await?;
auth_provider.check_permissions(&user, &["admin".to_string()])?;
```

## Error Types

The crate provides typed authentication and authorization errors:

```rust
use ras_jsonrpc_core::AuthError;

match auth_result {
    Err(AuthError::InvalidToken) => {
        // Token is malformed or invalid
    }
    Err(AuthError::TokenExpired) => {
        // Token has expired
    }
    Err(AuthError::InsufficientPermissions { required, has }) => {
        // User lacks required permissions
        eprintln!("Need {:?}, but user has {:?}", required, has);
    }
    Err(AuthError::AuthenticationRequired) => {
        // No token provided but authentication required
    }
    Err(AuthError::Internal(msg)) => {
        // Internal authentication error
        eprintln!("Auth error: {}", msg);
    }
    Ok(user) => {
        // Authentication successful
        println!("Authenticated user: {}", user.user_id);
    }
}
```

## Types

### AuthenticatedUser

Represents a successfully authenticated user:

```rust
pub struct AuthenticatedUser {
    /// Unique identifier for the user
    pub user_id: String,
    
    /// Set of permissions granted to this user
    pub permissions: HashSet<String>,
    
    /// Optional additional metadata about the user
    pub metadata: Option<serde_json::Value>,
}
```

### Type Aliases

- `AuthResult<T>` - Result type for authentication operations
- `AuthFuture<'a, T>` - Boxed future for async authentication

## Integration with ras-jsonrpc-macro

This crate is designed to work with the `ras-jsonrpc-macro` procedural macro:

```rust
use ras_jsonrpc_macro::jsonrpc_service;

jsonrpc_service!({
    service_name: MyService,
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS(["admin"]) delete_user(UserId) -> (),
    ]
});

struct MyServiceImpl;

impl MyServiceTrait for MyServiceImpl {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Validate credentials and issue a token.
    }

    async fn get_profile(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        // Load the authenticated user's profile.
    }

    async fn delete_user(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        request: UserId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Only users with the admin permission can reach here.
    }
}

// Use with the generated builder. The JSON-RPC route defaults to `/rpc`.
let service = MyServiceBuilder::new(MyServiceImpl)
    .base_url("/api/rpc")
    .auth_provider(JwtAuthProvider::new("secret"))
    .build()?;
```

### Version Migrations

The macro uses `VersionMigration<From, To>` for opt-in legacy compatibility. A legacy JSON-RPC method can migrate its request into the canonical request type, call the canonical service method, then migrate the canonical response back to the legacy response type.

```rust
use ras_jsonrpc_core::VersionMigration;

struct RenameCompat;

impl VersionMigration<RenameUserV1, RenameUserV2> for RenameCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserV1) -> Result<RenameUserV2, Self::Error> {
        Ok(RenameUserV2 {
            display_name: value.name,
            notify: false,
        })
    }
}

impl VersionMigration<RenameUserResponseV2, RenameUserResponseV1> for RenameCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserResponseV2) -> Result<RenameUserResponseV1, Self::Error> {
        Ok(RenameUserResponseV1 {
            name: value.display_name,
        })
    }
}
```

## Example Auth Providers

### Bearer Token Authentication
```rust
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::{HashMap, HashSet};

struct StaticBearerAuthProvider {
    users_by_token: HashMap<String, AuthenticatedUser>,
}

impl AuthProvider for StaticBearerAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            self.users_by_token
                .get(&token)
                .cloned()
                .ok_or(AuthError::InvalidToken)
        })
    }
}

let mut users_by_token = HashMap::new();
users_by_token.insert(
    "admin-token".to_string(),
    AuthenticatedUser {
        user_id: "admin-user".to_string(),
        permissions: HashSet::from(["admin".to_string(), "user".to_string()]),
        metadata: None,
    },
);

let auth_provider = StaticBearerAuthProvider { users_by_token };
```

### API Key Authentication
```rust
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::{HashMap, HashSet};

struct ApiKeyAuthProvider {
    keys: HashMap<String, HashSet<String>>,
}

impl AuthProvider for ApiKeyAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let permissions = self
                .keys
                .get(&token)
                .cloned()
                .ok_or(AuthError::InvalidToken)?;

            Ok(AuthenticatedUser {
                user_id: format!("api-key:{}", token),
                permissions,
                metadata: None,
            })
        })
    }
}
```

### Composite Authentication
Compose providers by implementing `AuthProvider` on a type that holds several
providers and tries each one in order, returning the first successful
`AuthenticatedUser`. Keep the error generic, such as `AuthError::InvalidToken`,
so clients cannot distinguish which auth method failed.

See the [`examples/`](../../../examples/) directory for runnable service examples.

## Re-exports

For convenience, this crate re-exports all types from `ras-jsonrpc-types`:

```rust
use ras_jsonrpc_core::{JsonRpcRequest, JsonRpcResponse, JsonRpcError};
```

## Checks

```bash
cargo test -p ras-jsonrpc-core --locked
cargo clippy -p ras-jsonrpc-core --all-targets --all-features --locked -- -D warnings
```

## License

This project is licensed under either MIT or Apache-2.0.
