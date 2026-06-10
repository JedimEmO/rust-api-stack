//! Optional RAS auth gateway for multi-service browser frontends (issue
//! #14).
//!
//! For one service, embedded auth (issue #13) needs no gateway. When a
//! browser frontend fans out to several backend services, forwarding the
//! full multi-audience web session everywhere leaks cross-service permission
//! data and forces every backend to understand audience maps. The gateway
//! fixes that:
//!
//! ```text
//! browser (ras_web_session cookie/bearer)
//!   -> gateway validates the session locally (JWKS, no authority call)
//!   -> longest-prefix route -> target audience
//!   -> session permissions narrowed to that audience only
//!   -> short-lived single-audience ras_gateway_access token minted/cached
//!   -> request proxied with only the derived bearer attached
//! backend validates the simple single-audience token via the gateway JWKS
//! ```
//!
//! Invariants enforced by construction and tests:
//!
//! - The original session token is never forwarded; inbound
//!   `Authorization`/`Cookie`/hop-by-hop headers are stripped.
//! - Derived tokens carry exactly one audience and only that audience's
//!   session permissions — never invented, never widened.
//! - Routes outside the table, sessions without the target audience's
//!   permissions (unless declared authenticated-only), and connection
//!   upgrades (WebSocket, v1) all fail closed.
//! - Derived tokens never outlive their session; the cache is a pure
//!   optimization keyed by session/subject/audience/authz-version.
//!
//! Deploy the gateway *behind* your existing ingress/load balancer — it is
//! an application-layer token exchanger, not a general-purpose ingress.
//! Route/audience profiles can be hand-written ([`RouteRule`]) or consumed
//! from generated topology artifacts
//! ([`GatewayConfig::from_profile_toml`]).
//!
//! Backends validate derived tokens with
//! [`backend_validation_options`] plus the gateway's JWKS — or directly via
//! `ras-authorization-core`'s `RasTokenAuthProvider` for generated RAS
//! services.

mod config;
mod error;
mod gateway;
mod proxy;

pub use config::{GatewayConfig, GatewayProfile, ProfileRoute, RouteRule, RouteTable};
pub use error::GatewayError;
pub use gateway::{AuthGateway, DerivedToken};
pub use proxy::gateway_router;

use ras_authorization_token::{AudiencePolicy, TokenType, ValidationOptions};

/// Validation options for a backend accepting gateway-derived tokens:
/// pinned issuer, exact audience, and the `ras_gateway_access` token type.
pub fn backend_validation_options(
    gateway_issuer: impl Into<String>,
    audience: impl Into<String>,
) -> ValidationOptions {
    ValidationOptions::new(
        gateway_issuer,
        AudiencePolicy::Exact(audience.into()),
        vec![TokenType::GatewayAccess],
    )
}
