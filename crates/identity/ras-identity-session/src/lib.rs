//! JWT sessions for Rust API Stack services.
//!
//! [`SessionService`] routes a login payload to a registered
//! `ras_identity_core::IdentityProvider`, collects permissions from an optional
//! `UserPermissions` source, and returns an HMAC-signed JWT. [`JwtAuthProvider`] implements
//! `ras_auth_core::AuthProvider` on top of [`SessionService::verify_session`], so the
//! generated services accept those tokens.
//!
//! The user ID in a session is `"<provider_id>:<subject>"`, such as `local:alice`, because a
//! subject is only unique within its provider. It is the JWT `sub` claim and
//! `AuthenticatedUser::user_id`; [`user_id`] builds it from a verified identity.
//!
//! [`SessionConfig::new`] takes the secret, issuer and audience, and rejects short or
//! placeholder secrets. By default sessions last 24 hours and are recorded in an in-memory
//! registry, so [`SessionService::end_session`] revokes a token at once; that registry is
//! per process, so a restart ends every session. Permissions are copied into the token at
//! login and are not reloaded on verify.
//!
//! The crate has no Cargo features. See the
//! [README](https://github.com/JedimEmO/rust-api-stack/blob/master/crates/identity/ras-identity-session/README.md)
//! for configuration, verification order and revocation.
mod auth;
mod claims;
mod config;
mod jwt;
mod session;

pub use auth::JwtAuthProvider;
pub use claims::JwtClaims;
pub use config::SessionConfig;
pub use jwt::JwtAlgorithm;
use ras_identity_core::{IdentityError, VerifiedIdentity};
pub use session::SessionService;
use thiserror::Error;

/// Clock-skew leeway applied to the `iat` and `nbf` claims: a token whose
/// `iat`/`nbf` lies further than this in the future is rejected.
pub const CLOCK_SKEW_LEEWAY_SECS: i64 = 60;

/// The user ID a session gives a verified identity: `"<provider_id>:<subject>"`.
///
/// Subjects are only unique within one identity provider, so the provider ID is
/// part of the user ID. Alice at the `local` provider is `local:alice`, and the
/// Google account `1234` behind `ras-identity-oauth2` is `oauth2:google:1234`.
/// [`SessionService`] writes this value to the JWT `sub` claim, and
/// [`JwtAuthProvider`] passes it on as `AuthenticatedUser::user_id`.
///
/// Registered provider IDs cannot contain `:`, so the text before the first `:`
/// always names the provider that verified the login, and two providers never
/// produce the same user ID.
pub fn user_id(identity: &VerifiedIdentity) -> String {
    format!("{}:{}", identity.provider_id, identity.subject)
}

/// Default cap on concurrently tracked sessions per user.
pub const DEFAULT_MAX_SESSIONS_PER_USER: usize = 32;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("JWT error: {0}")]
    JwtError(String),

    #[error("JWT token expired")]
    TokenExpired,

    #[error("Identity error: {0}")]
    IdentityError(#[from] IdentityError),

    #[error("Session not found")]
    SessionNotFound,

    #[error("Invalid session")]
    InvalidSession,

    #[error("Invalid session configuration: {0}")]
    InvalidConfig(String),
}
