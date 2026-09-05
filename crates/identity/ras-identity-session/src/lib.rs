//! Session management with JWT token generation and validation.
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

/// Clock-skew leeway applied to the `iat` and `nbf` claims (S4): a token whose
/// `iat`/`nbf` lies further than this in the future is rejected.
pub const CLOCK_SKEW_LEEWAY_SECS: i64 = 60;

/// Default cap on concurrently tracked sessions per user (S5).
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
