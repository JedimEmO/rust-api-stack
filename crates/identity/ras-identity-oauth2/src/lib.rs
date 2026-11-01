//! An OAuth2 sign-in provider for Rust API Stack services.
//!
//! [`OAuth2Provider`] runs the authorization-code flow with PKCE and implements
//! [`IdentityProvider`] under the ID `"oauth2"`, so `ras-identity-session` can turn a
//! completed flow into a JWT. Start a flow with [`OAuth2Provider::start_flow`], which returns
//! the authorization URL and a login-CSRF binding to store in a cookie; complete it by
//! passing an [`OAuth2AuthPayload::Callback`] to the session service.
//!
//! The callback checks the single-use `state`, the binding, and, when the provider returns
//! an `id_token`, its issuer, audience, expiry, nonce and subject. The `id_token` signature
//! is not verified. Set [`OAuth2ProviderConfig::issuer`] for any OpenID Connect provider;
//! an `id_token` is rejected without it.
//!
//! [`InMemoryStateStore`] keeps pending flows in one process; implement
//! [`OAuth2StateStore`] to share them between instances.
//!
//! The crate has no Cargo features. See the
//! [README](https://github.com/JedimEmO/rust-api-stack/blob/master/crates/identity/ras-identity-oauth2/README.md)
//! for configuration, the full list of checks and provider compatibility.

mod client;
mod config;
mod error;
mod provider;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub use client::{OAuth2Client, PkceChallenge};
pub use config::{OAuth2Config, OAuth2ProviderConfig, UserInfoMapping};
pub use error::{OAuth2Error, OAuth2Result};
pub use provider::{OAuth2AuthPayload, OAuth2Provider, OAuth2Response};
pub use state::{InMemoryStateStore, OAuth2State, OAuth2StateStore};
pub use types::{
    AuthorizationRedirect, AuthorizationRequest, AuthorizationResponse, ProviderMetadata,
    TokenResponse, UserInfoResponse,
};

// Re-export common types for convenience
pub use ras_identity_core::{IdentityProvider, VerifiedIdentity};
