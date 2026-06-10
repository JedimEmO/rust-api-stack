//! Outbound token framework for RAS services (issue #12).
//!
//! RAS services frequently call other systems: third-party APIs through
//! OAuth2 grants, and other internal RAS services through RAS-issued tokens.
//! This crate provides the reusable, fail-closed core:
//!
//! - [`TokenSource`] — pluggable token acquisition (OAuth2 in
//!   `ras-integration-oauth2`, the RAS internal issuer in
//!   `ras-integration-ras`, [`StaticTokenSource`] for legacy/API-key cases,
//!   [`testing::FakeTokenSource`] for tests).
//! - [`TokenManager`] — bounds-checked, cached, deduplicated acquisition.
//!   Cache keys include token family, integration, subject (with principal
//!   mode), audience, canonical scopes, and config version, so token
//!   families and principals can never collide.
//! - [`IntegrationConfig`] — declared allowed scopes, audiences, and
//!   outbound base URLs per integration; anything outside fails closed.
//! - [`AuthorizedHttpClient`] — the capability-scoped client handlers should
//!   receive. Bound to one integration/subject/scope set; validates the
//!   outbound host *before* minting a token; never auto-replays requests
//!   after auth failures.
//! - [`GrantStore`]/[`UserGrant`] — refresh-token grant persistence with an
//!   in-memory implementation for tests and dev.
//! - [`SecretString`] — redacted secret wrapper used for all token and grant
//!   material; no serde, no `Debug` leakage.
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use ras_integration_core::{
//!     IntegrationConfig, StaticTokenSource, TokenManager, TokenRequest, TokenSubject,
//! };
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let manager = Arc::new(
//!     TokenManager::builder()
//!         .register(
//!             IntegrationConfig::new(
//!                 "metrics-push",
//!                 ["metrics:write"],
//!                 ["https://metrics.internal"],
//!             )
//!             .unwrap(),
//!             Arc::new(StaticTokenSource::new("static-api-key")),
//!         )
//!         .unwrap()
//!         .build(),
//! );
//!
//! let lease = manager
//!     .get_token(TokenRequest {
//!         integration_id: "metrics-push".to_string(),
//!         subject: TokenSubject::Service,
//!         scopes: vec!["metrics:write".to_string()],
//!         audience: None,
//!         force_refresh: false,
//!     })
//!     .await
//!     .unwrap();
//! assert_eq!(lease.access_token.expose_secret(), "static-api-key");
//! # });
//! ```

mod client;
mod config;
mod error;
mod grants;
mod manager;
mod secret;
pub mod testing;
mod types;

pub use client::AuthorizedHttpClient;
pub use config::IntegrationConfig;
pub use error::IntegrationError;
pub use grants::{GrantStore, InMemoryGrantStore, UserGrant};
pub use manager::{TokenManager, TokenManagerBuilder};
pub use secret::SecretString;
pub use types::{
    StaticTokenSource, TokenFamily, TokenLease, TokenRequest, TokenSource, TokenSubject,
};
