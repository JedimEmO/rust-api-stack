//! OAuth2 error types.

use thiserror::Error;

pub type OAuth2Result<T> = Result<T, OAuth2Error>;

#[derive(Debug, Error)]
pub enum OAuth2Error {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Invalid state parameter")]
    InvalidState,

    #[error("State not found or expired")]
    StateNotFound,

    #[error("Missing authorization code")]
    MissingAuthorizationCode,

    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    #[error("Invalid id_token: {0}")]
    InvalidIdToken(String),

    #[error("User info request failed: {0}")]
    UserInfoFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("URL parsing error: {0}")]
    UrlError(#[from] url::ParseError),

    #[error("Identity error: {0}")]
    IdentityError(#[from] ras_identity_core::IdentityError),

    #[error("PKCE verification failed")]
    PkceVerificationFailed,

    #[error("Invalid token response: {0}")]
    InvalidTokenResponse(String),

    #[error("Invalid user info response: {0}")]
    InvalidUserInfoResponse(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Callback error: {0}")]
    CallbackError(String),
}
