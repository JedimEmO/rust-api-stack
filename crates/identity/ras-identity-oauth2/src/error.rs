//! OAuth2 error types.

use thiserror::Error;

pub type OAuth2Result<T> = Result<T, OAuth2Error>;

#[derive(Debug, Error)]
pub enum OAuth2Error {
    /// An outbound request to the provider failed. The underlying
    /// `reqwest::Error` (which embeds the request URL) is available via
    /// `source()` and is logged at `warn` where it occurs; the `Display`
    /// text is fixed so it can safely reach client-facing error strings (I6).
    #[error("upstream request failed")]
    HttpError(#[from] reqwest::Error),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Invalid state parameter")]
    InvalidState,

    #[error("Reserved OAuth parameter cannot be overridden: {0}")]
    InvalidAuthorizationParam(String),

    #[error("State not found or expired")]
    StateNotFound,

    #[error("Missing authorization code")]
    MissingAuthorizationCode,

    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    #[error("Invalid id_token: {0}")]
    InvalidIdToken(String),

    /// No longer returned by `InMemoryStateStore`, which evicts the oldest
    /// pending flow at capacity instead of refusing (I3). Kept for custom
    /// `OAuth2StateStore` implementations that still want to refuse.
    #[deprecated(
        since = "0.3.0",
        note = "InMemoryStateStore evicts the oldest pending flow at capacity instead of returning this"
    )]
    #[error("Too many pending OAuth2 flows")]
    TooManyPendingFlows,

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

    /// The provider redirected back with an `error` parameter (e.g. the user
    /// denied consent). Only the standardized error code is carried;
    /// `error_description` is logged server-side and never echoed (I5).
    #[error("Provider denied the authorization request: {error}")]
    ProviderDenied { error: String },

    /// The callback carried neither an authorization `code` nor an `error`.
    #[error("Invalid OAuth2 callback: missing both code and error")]
    InvalidCallback,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[tokio::test]
    async fn i6_http_error_display_is_fixed_and_hides_url() {
        // An unsupported scheme fails inside reqwest before any network I/O,
        // and the resulting error embeds the request URL in its Display.
        let reqwest_error = reqwest::Client::new()
            .get("ftp://secret-host.internal/token?client_secret=abc")
            .send()
            .await
            .unwrap_err();
        assert!(reqwest_error.to_string().contains("secret-host.internal"));

        let error: OAuth2Error = reqwest_error.into();
        assert_eq!(error.to_string(), "upstream request failed");
        assert!(!error.to_string().contains("secret-host"));
        // The detail is still reachable for server-side diagnostics.
        assert!(error.source().is_some());
    }

    #[test]
    fn i5_provider_denied_display_carries_only_the_error_code() {
        let error = OAuth2Error::ProviderDenied {
            error: "access_denied".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Provider denied the authorization request: access_denied"
        );
    }
}
