//! RAS-native authorization control plane (issue #13, embedded mode).
//!
//! External identity providers authenticate humans; RAS owns application
//! authorization. This crate provides the control plane for internal
//! services:
//!
//! - **Model** ([`Principal`], [`ServiceRegistration`],
//!   [`AudiencePermission`], [`RoleDefinition`]): grants are always scoped
//!   by *target audience* — "principal X may use permission P at audience A"
//!   — so identical permission strings on different services never satisfy
//!   each other.
//! - **Store** ([`AuthorizationStore`], [`InMemoryAuthorizationStore`]):
//!   service registry, roles, bindings, direct grants, and imported
//!   permission manifests. Grants of permissions unknown to an audience's
//!   manifests are rejected unless made through the explicit `grant_custom`
//!   path. Every mutation bumps `authz_version`.
//! - **Identity** ([`ServiceIdentityVerifier`]): pluggable proof
//!   verification. [`StaticSecretVerifier`] ships for dev/simple
//!   deployments; production should use workload identity (Kubernetes SA
//!   JWTs, SPIFFE, mTLS) through the same trait.
//! - **Issuer** ([`TokenIssuer`]): fail-closed internal token issuance —
//!   identity, registration, audience existence, audience-scoped grants,
//!   and (when loaded) topology [`ServiceGraphPolicy`] edges — minting
//!   short-lived single-audience JWTs via `ras-authorization-token`, with
//!   JWKS publication and key rotation.
//! - **Audit** ([`AuditSink`]): append-only events for registrations,
//!   grants, issuance outcomes, and key changes; never containing secrets
//!   or token values.
//! - **Embedded routes** ([`authority_router`]): `POST /auth/token` and
//!   `GET /auth/jwks.json` mounted into any axum app — the default
//!   single-process deployment preset.
//! - **Downstream validation** ([`RasTokenAuthProvider`]): an
//!   `ras-auth-core` [`ras_auth_core::AuthProvider`] so existing generated
//!   services accept RAS internal/gateway tokens unchanged.

mod audit;
mod auth_provider;
mod error;
mod issuer;
mod model;
mod router;
mod store;
mod verifier;

pub use audit::{AuditEvent, AuditEventKind, AuditSink, InMemoryAuditSink, NoopAuditSink};
pub use auth_provider::RasTokenAuthProvider;
pub use error::AuthzError;
pub use issuer::{InternalTokenRequest, IssuedToken, TokenIssuer, TokenIssuerBuilder};
pub use model::{
    AudiencePermission, Principal, ResolvedPermissions, RoleDefinition, ServiceEdge,
    ServiceGraphPolicy, ServiceRegistration,
};
pub use router::{TokenResponse, authority_router};
pub use store::{AuthorizationStore, InMemoryAuthorizationStore};
pub use verifier::{
    ServiceIdentityProof, ServiceIdentityVerifier, StaticSecretVerifier, VerifiedServiceIdentity,
};
