# ras-identity-core

Core identity traits and types for Rust Agent Stack.

## Overview

This crate defines the small interfaces shared by local authentication, OAuth2 authentication, and JWT session management:

- `IdentityProvider` verifies an authentication payload and returns a `VerifiedIdentity`.
- `UserPermissions` maps a verified identity to permission strings.
- `VerifiedIdentity` is the provider-neutral identity record passed into session creation.

## Traits

```rust
use async_trait::async_trait;
use ras_identity_core::{IdentityProvider, IdentityResult, VerifiedIdentity};

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn provider_id(&self) -> &str;

    async fn verify(&self, auth_payload: serde_json::Value) -> IdentityResult<VerifiedIdentity>;
}
```

```rust
use async_trait::async_trait;
use ras_identity_core::{IdentityResult, UserPermissions, VerifiedIdentity};

#[async_trait]
pub trait UserPermissions: Send + Sync {
    async fn get_permissions(&self, identity: &VerifiedIdentity) -> IdentityResult<Vec<String>>;
}
```

## Verified Identity

```rust
pub struct VerifiedIdentity {
    pub provider_id: String,
    pub subject: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
```

## Built-In Permission Providers

- `NoopPermissions` returns no permissions.
- `StaticPermissions` returns the same permissions for every identity.

```rust
use ras_identity_core::StaticPermissions;

let permissions = StaticPermissions::new(vec!["read".to_string(), "write".to_string()]);
```

## Provider Example

```rust
use async_trait::async_trait;
use ras_identity_core::{IdentityError, IdentityProvider, IdentityResult, VerifiedIdentity};

struct MyIdentityProvider;

#[async_trait]
impl IdentityProvider for MyIdentityProvider {
    fn provider_id(&self) -> &str {
        "my-provider"
    }

    async fn verify(&self, auth_payload: serde_json::Value) -> IdentityResult<VerifiedIdentity> {
        let subject = auth_payload
            .get("subject")
            .and_then(|value| value.as_str())
            .ok_or(IdentityError::InvalidPayload)?;

        Ok(VerifiedIdentity {
            provider_id: self.provider_id().to_string(),
            subject: subject.to_string(),
            email: None,
            display_name: None,
            metadata: None,
        })
    }
}
```

## Checks

```bash
cargo test -p ras-identity-core --locked
cargo clippy -p ras-identity-core --all-targets --all-features --locked -- -D warnings
```
