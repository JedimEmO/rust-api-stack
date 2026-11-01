//! JWT sessions for Rust Agent Stack services.
//!
//! [`SessionService`] routes a login payload to a registered
//! `ras_identity_core::IdentityProvider`, collects permissions from an optional
//! `UserPermissions` source, and returns an HMAC-signed JWT. [`JwtAuthProvider`] implements
//! `ras_auth_core::AuthProvider` on top of [`SessionService::verify_session`], so the
//! generated services accept those tokens.
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
use ras_identity_core::IdentityError;
pub use session::SessionService;
use thiserror::Error;

/// Clock-skew leeway applied to the `iat` and `nbf` claims: a token whose
/// `iat`/`nbf` lies further than this in the future is rejected.
pub const CLOCK_SKEW_LEEWAY_SECS: i64 = 60;

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
