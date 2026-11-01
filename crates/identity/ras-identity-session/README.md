# ras-identity-session

JWT sessions for Rust Agent Stack services.
`SessionService` turns a login verified by an `IdentityProvider` into a signed JWT,
and `JwtAuthProvider` lets the generated services accept that JWT as their `AuthProvider`.

Tokens are signed with HMAC (`HS256`, `HS384` or `HS512`) and a shared secret.
The service does not issue refresh tokens, and does not read tokens from other issuers
such as an OIDC provider's RS256 tokens.

## Installation

```toml
[dependencies]
ras-identity-session = "0.4"
ras-identity-local = "0.3" # or another IdentityProvider
```

## Usage

```rust
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAuthProvider, SessionConfig, SessionService};
use std::sync::Arc;

let provider = LocalUserProvider::new();
provider
    .add_user("alice".to_string(), "correct-horse-battery-staple".to_string(), None, None)
    .await?;

// A random secret of at least 32 bytes, for instance from `openssl rand -hex 32`.
let session_config = SessionConfig::new(
    std::env::var("JWT_SECRET")?,
    "my-service", // iss
    "my-service", // aud
)?;
let session_service = Arc::new(SessionService::new(session_config)?);
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

// Pass this to a generated service builder's `auth_provider(...)`.
let auth_provider = JwtAuthProvider::new(session_service.clone());
```

`begin_session` takes the provider ID and that provider's login payload.
To put permissions into the token, attach a `UserPermissions` source
with `SessionService::with_permissions` before wrapping the service in an `Arc`.
Without one, tokens carry no permissions.

## Configuration

`SessionConfig::new(secret, issuer, audience)` returns a config with these defaults:

| Field | Default | Meaning |
|---|---|---|
| `jwt_ttl` | 24 hours | Token lifetime. Must be positive. |
| `algorithm` | `JwtAlgorithm::HS256` | HMAC algorithm. Tokens with any other `alg` header are rejected. |
| `enforce_active_sessions` | `true` | Keep a registry of live sessions, so they can be revoked. |
| `max_sessions_per_user` | 32 | Sessions per user in the registry; the oldest is evicted at the cap. |
| `iss`, `aud` | the arguments | Written into each token and required to match on verify. |

Change a field with struct update syntax, for example
`SessionConfig { jwt_ttl: chrono::Duration::hours(1), ..config }`.
`SessionService::new` validates the config again, so an invalid edit fails there.

The secret is used as raw bytes; a hex string is not decoded.
Validation requires at least 32 bytes after trimming whitespace,
at least 10 distinct byte values, and no run of 8 or more identical bytes.
It also rejects secrets that contain placeholder words such as `secret`, `password`, `example` or `change-me`.
`SessionConfig`'s `Debug` output redacts the secret.

`iss` and `aud` are required by default,
so a token minted by another service that shares the secret is rejected here.
`SessionConfig::new_unscoped(secret)` and `allow_unscoped_tokens()` opt out;
use them only for a single service whose secret is never shared.

## Verification

`verify_session` checks, in order:

1. The token has three segments, its `alg` matches the configured algorithm,
   and its HMAC signature is valid. The signature is compared in constant time.
2. `exp` is in the future. There is no leeway on expiry.
3. `iat`, and `nbf` when present, are no more than 60 seconds in the future.
4. `iss` and `aud` match the configured values, when configured.
5. With `enforce_active_sessions`, the token's `jti` is still in the session registry.

`JwtAuthProvider` maps an expired token to `AuthError::TokenExpired`
and every other failure to `AuthError::InvalidToken`.
It builds the `AuthenticatedUser` from the `sub`, `permissions` and `metadata` claims.

## Sessions and revocation

With `enforce_active_sessions` on, `begin_session` records each token's `jti`,
and `end_session(jti)` removes it, so the token stops working at once:

```rust
let claims = session_service.verify_session(&token).await?;
session_service.end_session(&claims.jti).await;
```

There is no call that ends all of one user's sessions.

The registry lives in process memory.
A restart ends every session,
and in a deployment with several instances, each instance accepts only the tokens it issued.
Turn `enforce_active_sessions` off to accept any valid, unexpired token on any instance;
`end_session` then has no effect, and a token stays valid until `exp`.

Expired sessions are removed from the registry lazily,
at most once a minute, when `begin_session` or `verify_session` runs.
Call `start_cleanup_task(interval)` on the `Arc<SessionService>` to sweep them during quiet periods too.

## Permissions are fixed at login

The permissions from the `UserPermissions` source are copied into the token when the session begins.
They are not reloaded on verify.
A permission change therefore reaches a user at their next login,
or sooner if you end the session and they sign in again.
Keep `jwt_ttl` short when permissions change often and revocation is off.

## JWT claims

- `sub`: the identity's subject.
- `exp` and `iat`: expiry and issue time, in Unix seconds. `nbf` is read when present but never written.
- `iss` and `aud`: the configured issuer and audience, when set. `aud` is a single string.
- `jti`: a random UUID that identifies the session.
- `provider_id`: the identity provider that verified the login.
- `email`, `display_name`, `permissions` and `metadata`, from the identity and the permission source.

The claims are signed, not encrypted.
Anyone holding the token can read them, so keep secrets out of identity metadata.

## Checks

```bash
cargo test -p ras-identity-session --locked
cargo clippy -p ras-identity-session --all-targets --all-features --locked -- -D warnings
```
