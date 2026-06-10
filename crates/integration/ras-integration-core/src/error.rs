//! Error types for the outbound token framework.

use thiserror::Error;

/// Errors from token acquisition, configuration validation, and authorized
/// outbound requests.
///
/// Variants never carry token or secret values.
#[derive(Debug, Error)]
pub enum IntegrationError {
    /// No integration is registered under this id.
    #[error("unknown integration {integration_id:?}")]
    UnknownIntegration { integration_id: String },

    /// No stored grant exists (or the grant was revoked) for this user and
    /// integration; the application must run a consent flow to obtain one.
    #[error("consent required for user {user_id:?} on integration {integration_id:?}")]
    ConsentRequired {
        integration_id: String,
        user_id: String,
        /// Scopes that were requested but are not covered by any stored
        /// grant. Empty when no grant exists at all.
        missing_scopes: Vec<String>,
    },

    /// A requested scope is outside the integration's configured
    /// `allowed_scopes`. Fails closed before any token source is consulted.
    #[error("scope {scope:?} is not allowed for integration {integration_id:?}")]
    ScopeNotAllowed {
        integration_id: String,
        scope: String,
    },

    /// A requested audience is outside the integration's configured
    /// `allowed_audiences`.
    #[error("audience {audience:?} is not allowed for integration {integration_id:?}")]
    AudienceNotAllowed {
        integration_id: String,
        audience: String,
    },

    /// The outbound URL is not covered by the integration's allowed hosts.
    /// Managed bearer tokens are never attached to unvalidated hosts.
    #[error("url {url:?} is not an allowed outbound host for integration {integration_id:?}")]
    HostNotAllowed { integration_id: String, url: String },

    /// The token source rejected the request for authorization reasons
    /// (e.g. the RAS authority refused issuance).
    #[error("token request denied for integration {integration_id:?}: {reason}")]
    Denied {
        integration_id: String,
        reason: String,
    },

    /// The upstream provider failed (network error, 5xx, malformed
    /// response).
    #[error("provider error for integration {integration_id:?}: {reason}")]
    Provider {
        integration_id: String,
        reason: String,
    },

    /// The grant store failed. Surfaced loudly because losing a rotated
    /// refresh token is unrecoverable.
    #[error("grant store error: {0}")]
    GrantStore(String),

    /// Invalid integration configuration.
    #[error("invalid integration configuration: {0}")]
    InvalidConfig(String),

    /// Transport-level failure while executing an authorized request.
    #[error("transport error: {0}")]
    Transport(#[from] ras_transport_core::TransportError),
}
