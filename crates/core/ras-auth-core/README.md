# ras-auth-core

Core authentication and authorization traits for Rust Agent Stack services.

## Overview

This crate defines the shared authentication contract used by the REST,
JSON-RPC, bidirectional JSON-RPC, and identity crates:

- `AuthProvider` - Main trait for authentication providers
- `AuthenticatedUser` - Represents an authenticated user with permissions
- `AuthError` - Common error types for authentication failures
- `AuthFuture` - Boxed future type returned by authentication providers

## Key Types

### AuthProvider

The main trait that authentication providers must implement:

```rust
use ras_auth_core::{AuthFuture, AuthProvider};

pub trait AuthProvider: Send + Sync + 'static {
    fn authenticate(&self, token: String) -> AuthFuture<'_>;
}
```

The trait also provides a default `check_permissions` implementation that
requires every requested permission to be present in the authenticated user.

### AuthenticatedUser

Represents a successfully authenticated user:

```rust
use std::collections::HashSet;

pub struct AuthenticatedUser {
    pub user_id: String,
    pub permissions: HashSet<String>,
    pub metadata: Option<serde_json::Value>,
}
```

### AuthError

Common authentication error types:

```rust
pub enum AuthError {
    InvalidToken,
    TokenExpired,
    InsufficientPermissions {
        required: Vec<String>,
        has: Vec<String>,
    },
    AuthenticationRequired,
    Internal(String),
}
```

## Usage

This crate is typically used as a dependency by:
- Authentication provider implementations (JWT, OAuth, etc.)
- JSON-RPC and REST service macros
- Service implementations requiring authentication

## Example

```rust
use std::collections::HashSet;

use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};

struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token != "valid-token" {
                return Err(AuthError::InvalidToken);
            }

            Ok(AuthenticatedUser {
                user_id: "user-123".to_string(),
                permissions: HashSet::from([
                    "read".to_string(),
                    "write".to_string(),
                ]),
                metadata: None,
            })
        })
    }
}
```

## Integration

This crate is used by:
- `ras-jsonrpc-macro` - For JSON-RPC service authentication
- `ras-rest-macro` - For REST API authentication
- `ras-identity-session` - For JWT-based authentication
- `ras-jsonrpc-bidirectional-server` - For WebSocket authentication

## Checks

```bash
cargo test -p ras-auth-core --locked
cargo clippy -p ras-auth-core --all-targets --all-features --locked -- -D warnings
```
