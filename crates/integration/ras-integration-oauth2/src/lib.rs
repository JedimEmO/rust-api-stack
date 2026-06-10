//! OAuth2/OIDC token source for the RAS outbound token framework.
//!
//! Implements [`ras_integration_core::TokenSource`] for external OAuth2
//! providers:
//!
//! - **User subjects** use the refresh-token flow against grants stored in a
//!   [`ras_integration_core::GrantStore`], with scope subset-checks against
//!   the stored consent and transactional refresh-token rotation.
//! - **Service subjects** use client credentials, forwarding the requested
//!   audience for providers (e.g. Auth0) that support it.
//! - Missing or dead grants surface as
//!   [`ras_integration_core::IntegrationError::ConsentRequired`], never a
//!   silent fallback.
//!
//! [`ConsentFlow`] provides the consent-side helpers: PKCE S256
//! authorization URLs with opaque single-use expiring `state` bound to the
//! initiating user/integration/redirect/scopes, callback validation, and
//! the authorization-code exchange that persists the grant.
//!
//! All provider traffic goes through `ras-transport-core`'s
//! [`ras_transport_core::HttpTransport`], so tests and examples can run a
//! fake provider in-process.

mod config;
mod consent;
mod source;

pub use config::OAuth2ProviderConfig;
pub use consent::{AuthorizationRedirect, ConsentFlow, ValidatedConsent};
pub use source::OAuth2TokenSource;
