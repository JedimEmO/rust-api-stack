//! Gateway errors and their HTTP mappings.

use thiserror::Error;

/// Errors raised while routing, validating, deriving, or proxying.
///
/// Mapped to coarse HTTP statuses by the proxy layer; token values never
/// appear in any variant.
#[derive(Debug, Error)]
pub enum GatewayError {
    /// Configuration/profile validation failure (startup time).
    #[error("invalid gateway configuration: {0}")]
    InvalidConfig(String),

    /// No route matches the request path. Fails closed as 404.
    #[error("no route matches the request path")]
    RouteNotFound,

    /// The request carried no session credential.
    #[error("missing web session")]
    MissingSession,

    /// The web session failed validation.
    #[error("invalid web session")]
    InvalidSession(#[source] ras_authorization_token::TokenError),

    /// The validated session holds no permissions for the route's audience
    /// (and the route is not declared authenticated-only).
    #[error("session has no permissions for audience {audience:?}")]
    NoPermissionsForAudience { audience: String },

    /// Derived-token signing failed.
    #[error("token derivation failed")]
    Derivation(#[source] ras_authorization_token::TokenError),

    /// The upstream call failed.
    #[error("upstream error: {0}")]
    Upstream(#[from] ras_transport_core::TransportError),

    /// WebSocket/connection-upgrade proxying is not supported in v1; such
    /// requests fail closed.
    #[error("connection upgrades are not supported by the gateway")]
    UpgradeNotSupported,
}
