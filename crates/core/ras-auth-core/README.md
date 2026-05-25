# ras-auth-core

Core authentication and authorization traits for Rust Agent Stack services.

## Overview

This crate defines the shared authentication contract used by the REST,
JSON-RPC, bidirectional JSON-RPC, and identity crates:

- `AuthProvider` - Main trait for authentication providers
- `AuthenticatedUser` - Represents an authenticated user with permissions
- `AuthError` - Common error types for authentication failures
- `AuthFuture` - Boxed future type returned by authentication providers
- `AuthTransportConfig` - HTTP credential transport configuration for bearer and cookie auth
- `AuthCookieConfig` - Secure session cookie settings and `Set-Cookie` helpers
- `CsrfConfig` - CSRF guard for cookie-authenticated unsafe requests

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

### HTTP Credential Transport

`AuthProvider` still validates a token string. HTTP services can choose how the
token reaches the server:

```rust
use ras_auth_core::{AuthCookieConfig, AuthTransportConfig, CsrfConfig};

let transport = AuthTransportConfig::default()
    .with_cookie(AuthCookieConfig::default())
    .with_csrf(CsrfConfig::default());
```

Bearer tokens remain enabled by default. If both `Authorization: Bearer ...` and
the configured cookie are present, bearer wins. If the bearer header is present
but malformed, the request fails instead of falling back to the cookie.

Cookie helpers emit secure defaults:

```rust
use ras_auth_core::AuthCookieConfig;

let cookie = AuthCookieConfig::default();
let set_cookie = cookie.session_cookie_header_value("jwt-token")?;
let clear_cookie = cookie.clear_cookie_header_value()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The default cookie is `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, and uses a
`__Host-` name. Use `insecure_for_local_development()` only for local HTTP.

`CsrfConfig::default()` uses a double-submit token: issue a CSRF cookie with
`csrf_cookie_header_value(...)`, then have browser clients echo the same token in
the `x-ras-csrf` header on cookie-authenticated `POST`, `PUT`, `PATCH`, and
`DELETE` requests. Use `header_presence_only(...)` only behind restrictive
credentialed CORS where a presence-only custom header is an intentional tradeoff.

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
