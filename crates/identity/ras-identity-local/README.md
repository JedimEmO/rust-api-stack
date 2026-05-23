# ras-identity-local

Local username/password identity provider for the Rust Agent Stack.

## Overview

This crate provides a secure local authentication implementation using:
- Argon2 password hashing (industry standard)
- Timing-attack mitigation for missing users
- Username-enumeration resistant error responses
- Thread-safe concurrent request handling

## Features

- **Password Storage**: Uses per-user salted Argon2id hashes
- **Duplicate Protection**: Rejects duplicate usernames instead of overwriting users
- **Attack Protection**: Missing users are checked against a fixed Argon2 sentinel hash
- **Concurrency Limiting**: A semaphore bounds simultaneous authentication attempts
- **Thread-Safe**: Safe for use in async multi-threaded environments

## Usage

### Basic Setup

```rust
use ras_identity_core::IdentityProvider;
use ras_identity_local::LocalUserProvider;

let provider = LocalUserProvider::new();
provider
    .add_user(
        "alice".to_string(),
        "secure_password".to_string(),
        Some("alice@example.com".to_string()),
        Some("Alice".to_string()),
    )
    .await?;

// Verify identity
let auth_payload = serde_json::json!({
    "username": "alice",
    "password": "secure_password"
});

let identity = provider.verify(auth_payload).await?;
assert_eq!(identity.subject, "alice");
```

### Integration with Session Service

```rust
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAlgorithm, SessionConfig, SessionService};
use chrono::Duration;
use std::sync::Arc;

// Set up identity provider
let provider = LocalUserProvider::new();
provider
    .add_user(
        "alice".to_string(),
        "secure_password".to_string(),
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

// Run the authentication flow
let jwt = session_service
    .begin_session(
        "local",
        serde_json::json!({
            "username": "alice",
            "password": "secure_password"
        }),
    )
    .await?;
```

## Security Features

### Attack Protection

1. **Timing Attack Mitigation**
   - Verifies non-existent users against a fixed Argon2 sentinel hash
   - Reduces user-existence timing differences during password verification

2. **Username Enumeration Mitigation**
   - Always returns `InvalidCredentials` for any failure
   - No distinction between "user not found" and "wrong password"

3. **Concurrency Limiting**
   - Semaphore limits to 5 concurrent authentication attempts
   - Bounds authentication work; deploy an external rate limiter for per-user or per-IP policies

4. **Input Validation**
   - Handles empty credentials gracefully
   - Rejects malformed authentication payloads
   - Handles very long credentials without leaking user-existence details
   - Safe handling of special characters
   - Rejects duplicate usernames during user creation

### Password Requirements

The implementation uses Argon2 default settings:
- Memory cost: 19 MiB
- Time cost: 2 iterations
- Parallelism: 1 thread
- Output length: 32 bytes

## Testing

The crate includes focused security tests for:
- Username enumeration attempts
- Concurrent request handling
- Input validation edge cases
- Optional timing attack and brute-force simulations behind the `timing-tests` feature

Run tests with:
```bash
cargo test -p ras-identity-local --locked
```

Run the timing-sensitive statistical check explicitly when the host is quiet
enough for stable measurements:
```bash
cargo test -p ras-identity-local --locked --features timing-tests -- --ignored
```

## Checks

```bash
cargo test -p ras-identity-local --locked
cargo clippy -p ras-identity-local --all-targets --all-features --locked -- -D warnings
```

## Best Practices

1. **Never log passwords** or password hashes
2. **Use strong passwords** - consider implementing password policies
3. **Monitor failed attempts** - implement account lockout in production
4. **Rotate secrets** - change JWT secrets periodically
5. **Use HTTPS** - always use TLS in production environments
