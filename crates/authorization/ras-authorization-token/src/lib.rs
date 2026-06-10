//! Shared RAS token primitives.
//!
//! This crate defines the single token model used across a RAS deployment:
//!
//! - **Claims** ([`RasClaims`]) shared by all token families
//!   ([`TokenType::WebSession`], [`TokenType::InternalService`],
//!   [`TokenType::GatewayAccess`]), with per-family structural invariants.
//! - **Keys** ([`SigningKey`], [`VerifyingKey`]) supporting ES256
//!   (recommended), EdDSA, and HS256 (embedded/dev shared-secret mode), plus
//!   JWKS serialization ([`Jwk`], [`JwkSet`]) for asymmetric keys.
//! - **Signing and rotation** ([`KeyRing`]): an active signing key with
//!   retired verification keys, so rotation does not invalidate outstanding
//!   tokens, and emergency removal does.
//! - **Validation** ([`TokenValidator`]): algorithm allowlist (asymmetric by
//!   default), `kid` resolution through a [`KeyResolver`], key/header
//!   algorithm cross-checks, signature verification, issuer/audience/expiry/
//!   not-before/token-type checks with configurable clock skew.
//!
//! Higher layers build on this: the authorization control plane issues
//! [`TokenType::InternalService`] tokens, web sessions carry
//! audience-grouped permissions, and the auth gateway narrows sessions into
//! [`TokenType::GatewayAccess`] tokens — all with the same claims shape and
//! validation pipeline.
//!
//! # Example
//!
//! ```
//! use chrono::Duration;
//! use ras_authorization_token::{
//!     AudiencePolicy, KeyRing, PrincipalKind, RasClaims, SigningKey, TokenType,
//!     TokenValidator, ValidationOptions,
//! };
//!
//! // Authority side: sign an internal service token.
//! let ring = KeyRing::new(SigningKey::generate_es256("2026-06-key-1"));
//! let claims = RasClaims::internal_service(
//!     "https://auth.internal",
//!     "billing-service",
//!     PrincipalKind::Service,
//!     "invoice-service",
//!     vec!["invoice:write".to_string()],
//!     Duration::minutes(5),
//! );
//! let token = ring.sign(&claims).unwrap();
//!
//! // Downstream side: validate via the published JWKS.
//! let validator = TokenValidator::new(
//!     ring.jwks(),
//!     ValidationOptions::new(
//!         "https://auth.internal",
//!         AudiencePolicy::Exact("invoice-service".to_string()),
//!         vec![TokenType::InternalService],
//!     ),
//! );
//! let validated = validator.validate(&token).unwrap();
//! assert_eq!(validated.sub, "billing-service");
//! ```

mod claims;
mod error;
mod keyring;
mod keys;
mod validate;

pub use claims::{PrincipalKind, RasClaims, TokenType};
pub use error::TokenError;
pub use keyring::{KeyRing, sign_claims};
pub use keys::{Jwk, JwkSet, SecretBytes, SigningAlgorithm, SigningKey, VerifyingKey};
pub use validate::{AudiencePolicy, KeyResolver, TokenValidator, ValidationOptions};
