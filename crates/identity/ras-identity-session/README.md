# ras-identity-session

JWT session management and `AuthProvider` integration for Rust Agent Stack.

## Overview

This crate turns a verified identity from a registered `IdentityProvider` into a signed JWT. It can also keep an in-memory active-session registry so tokens can be revoked by JWT ID (`jti`) before they expire.

## Features

- JWT creation and validation with configurable TTL and signing algorithm
- Optional active-session enforcement for revocation
- Permission embedding through a `UserPermissions` provider
- `JwtAuthProvider` adapter for RAS JSON-RPC, REST, and WebSocket services

## Usage

```rust
use chrono::Duration;
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAlgorithm, JwtAuthProvider, SessionConfig, SessionService};
use std::sync::Arc;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let provider = LocalUserProvider::new();
provider
    .add_user(
        "alice".to_string(),
        "correct-horse-battery-staple".to_string(),
        Some("alice@example.com".to_string()),
        Some("Alice".to_string()),
    )
    .await?;

let session_service = Arc::new(SessionService::new(SessionConfig {
    jwt_secret: "use-at-least-32-bytes-of-random-secret".to_string(),
    jwt_ttl: Duration::hours(1),
    refresh_enabled: false,
    enforce_active_sessions: true,
    algorithm: JwtAlgorithm::HS256,
})?);

session_service.register_provider(Box::new(provider)).await;

let token = session_service
    .begin_session(
        "local",
        serde_json::json!({
            "username": "alice",
            "password": "correct-horse-battery-staple"
        }),
    )
    .await?;

let claims = session_service.verify_session(&token).await?;
assert_eq!(claims.sub, "alice");

let auth_provider = JwtAuthProvider::new(session_service.clone());
# let _ = auth_provider;
# Ok(())
# }
```

## Session Revocation

When `enforce_active_sessions` is `true`, `verify_session` checks that the token's `jti` is still present in the active-session registry.

```rust
let claims = session_service.verify_session(&token).await?;
session_service.end_session(&claims.jti).await;
```

## JWT Claims

Generated tokens include:

- `sub`: identity subject
- `exp` and `iat`: expiration and issue timestamps
- `jti`: session identifier used for revocation
- `provider_id`: identity provider that verified the user
- `email`, `display_name`, `permissions`, and provider metadata when available

## Security Notes

- Use a high-entropy `jwt_secret` of at least 32 bytes.
- Keep `enforce_active_sessions` enabled when immediate revocation matters.
- Refresh tokens are not issued by `SessionService`; implement refresh-token storage and rotation at the application layer if needed.
- Transmit JWTs only over HTTPS in production.

## Checks

```bash
cargo test -p ras-identity-session --locked
cargo clippy -p ras-identity-session --all-targets --all-features --locked -- -D warnings
```
