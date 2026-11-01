# ras-identity-core

The traits that sit between a login method and a session in the Rust API Stack.
An `IdentityProvider` checks a login attempt and says who the user is;
a `UserPermissions` implementation says what that user may do.

`ras-identity-local` (username and password) and `ras-identity-oauth2` implement `IdentityProvider`,
and `ras-identity-session` consumes both traits to issue JWTs.
Depend on this crate directly when you write your own provider or permission source.

## Installation

```toml
[dependencies]
ras-identity-core = "0.1"
async-trait = "0.1"
serde_json = "1"
```

## Traits

```rust
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn provider_id(&self) -> &str;

    async fn verify(&self, auth_payload: serde_json::Value) -> IdentityResult<VerifiedIdentity>;
}

#[async_trait]
pub trait UserPermissions: Send + Sync {
    async fn get_permissions(&self, identity: &VerifiedIdentity) -> IdentityResult<Vec<String>>;
}
```

`provider_id` is the name a session service uses to route a login to this provider.
`verify` receives the provider-specific login payload as JSON,
and returns a `VerifiedIdentity` or an `IdentityError`.

## Verified identity

`VerifiedIdentity` is the provider-neutral result of a successful login:

```rust
pub struct VerifiedIdentity {
    pub provider_id: String,
    pub subject: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}
```

`subject` is the stable user identifier within that provider.
`ras-identity-session` copies it into the JWT `sub` claim.

## Built-in permission providers

- `NoopPermissions` returns no permissions.
- `StaticPermissions` returns the same permissions for every identity.

```rust
use ras_identity_core::StaticPermissions;

let permissions = StaticPermissions::new(vec!["read".to_string(), "write".to_string()]);
```

## Writing a provider

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

This provider trusts whatever subject the payload names, so it only shows the shape.
A real provider checks a credential before it returns an identity,
and returns `IdentityError::InvalidCredentials` for every failed check,
so a caller cannot tell an unknown user from a wrong password.

## Checks

```bash
cargo test -p ras-identity-core --locked
cargo clippy -p ras-identity-core --all-targets --all-features --locked -- -D warnings
```
