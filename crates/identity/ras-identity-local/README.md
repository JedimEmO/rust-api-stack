# ras-identity-local

A username and password `IdentityProvider` for Rust API Stack, with Argon2id password hashes.
Register it with `ras-identity-session` to turn a correct password into a JWT session.

Users live in an in-memory map.
They are added at runtime with `add_user` and disappear when the process exits;
the crate has no persistence and no way to load existing hashes.
That suits demos, tests and small services that seed their users at startup.
A service with a user database needs its own `IdentityProvider`,
which can follow the same verification approach.

## Installation

```toml
[dependencies]
ras-identity-local = "0.3"
ras-identity-core = "0.1" # for the IdentityProvider trait
```

## Usage

```rust
use ras_identity_core::IdentityProvider;
use ras_identity_local::LocalUserProvider;

let provider = LocalUserProvider::new();
provider
    .add_user(
        "alice".to_string(),
        "correct-horse-battery-staple".to_string(),
        Some("alice@example.com".to_string()),
        Some("Alice".to_string()),
    )
    .await?;

let identity = provider
    .verify(serde_json::json!({
        "username": "alice",
        "password": "correct-horse-battery-staple"
    }))
    .await?;
assert_eq!(identity.subject, "alice");
```

The provider ID is `"local"`, and the username becomes the identity's `subject`.
`add_user` fails with `LocalUserError::UserAlreadyExists` for a taken username,
rather than overwriting the user, and `remove_user` deletes one.

## With a session service

```rust
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{SessionConfig, SessionService};
use std::sync::Arc;

let provider = LocalUserProvider::new();
provider
    .add_user("alice".to_string(), "correct-horse-battery-staple".to_string(), None, None)
    .await?;

// A random secret of at least 32 bytes, for instance from `openssl rand -hex 32`.
let session_config = SessionConfig::new(std::env::var("JWT_SECRET")?, "my-service", "my-service")?;
let session_service = Arc::new(SessionService::new(session_config)?);
session_service.register_provider(Box::new(provider)).await;

let jwt = session_service
    .begin_session(
        "local",
        serde_json::json!({
            "username": "alice",
            "password": "correct-horse-battery-staple"
        }),
    )
    .await?;
```

## How passwords are checked

Each password is hashed with Argon2id and a random salt from the OS,
using the `argon2` crate's default parameters:
19 MiB of memory, 2 iterations, 1 lane, and a 32-byte output.
Hashing and verification run on Tokio's blocking pool, so they do not stall the async executor.

Every failed login returns the same `IdentityError::InvalidCredentials`,
whether the user is unknown or the password is wrong.
An unknown user is still checked against a fixed Argon2 hash with the same parameters,
so both cases cost about the same time.
This narrows the timing difference between them; it does not make the code constant-time.
A payload that is not `{"username", "password"}` returns `IdentityError::InvalidPayload`.

Passwords longer than `MAX_PASSWORD_BYTES` (1024 bytes) are refused before hashing.
`add_user` returns `LocalUserError::PasswordTooLong`,
and a login returns `InvalidCredentials` without running Argon2,
so a client cannot make the server hash megabytes of input.

At most five verifications run at once per provider; further logins wait for a slot.
This bounds the CPU and memory that Argon2 can use.
It is not a rate limit: there is no per-user or per-IP limit, and no account lockout.
Put those in front of the login endpoint.

The crate enforces no password policy beyond the length limit, and accepts an empty password.
Check password strength before you call `add_user`.

The password hash is never serialized: `LocalUser` skips `password_hash` when it serializes.
The `Debug` output of `LocalUser` and `LocalAuthPayload` shows `[REDACTED]` for the hash and the password.

## Cargo features

`timing-tests` enables a statistical timing test and a brute-force simulation.
It exists for this crate's own test suite.
Run the timing test on a quiet machine, because load on the host skews the measurements:

```bash
cargo test -p ras-identity-local --locked --features timing-tests -- --ignored
```

## Checks

```bash
cargo test -p ras-identity-local --locked
cargo clippy -p ras-identity-local --all-targets --all-features --locked -- -D warnings
```
