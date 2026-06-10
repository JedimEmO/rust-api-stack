//! The shared RAS claims model.
//!
//! Every token issued inside a RAS deployment — browser web sessions, internal
//! service-to-service access tokens, and gateway-derived backend tokens — uses
//! the same [`RasClaims`] structure, distinguished by [`TokenType`]. Keeping a
//! single claims shape is what lets the auth gateway narrow a web session into
//! a backend token without inventing a second convention.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The token family, carried in the `typ` claim.
///
/// Validators must always pin the expected token type: a web session must
/// never be accepted where an internal service token is required, and vice
/// versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenType {
    /// Browser-facing web session. Multi-audience: permissions are grouped
    /// per target audience in `audience_permissions`, and `aud` is absent.
    #[serde(rename = "ras_web_session")]
    WebSession,
    /// Internal service-to-service access token issued by the RAS authority.
    /// Single-audience: `aud` names the target service.
    #[serde(rename = "ras_internal_access")]
    InternalService,
    /// Backend token derived by the auth gateway from a validated web
    /// session. Single-audience, containing only that audience's permissions.
    #[serde(rename = "ras_gateway_access")]
    GatewayAccess,
}

/// The kind of principal a token's `sub` identifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// A human user authenticated through an identity provider.
    User,
    /// A registered internal service acting as itself.
    Service,
    /// A non-human service account with explicit grants.
    ServiceAccount,
    /// A registered application principal.
    Application,
}

/// Shared claims for all RAS token families.
///
/// Construct via [`RasClaims::web_session`], [`RasClaims::internal_service`],
/// or [`RasClaims::gateway_access`] so per-type invariants hold; signing and
/// validation both enforce [`RasClaims::validate_shape`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RasClaims {
    /// Issuer: the RAS authority (or gateway acting as delegated authority).
    pub iss: String,
    /// Subject: user id, service id, service-account id, or application id.
    pub sub: String,
    /// Token family. Serialized as the `typ` claim.
    #[serde(rename = "typ")]
    pub token_type: TokenType,
    /// What kind of principal `sub` identifies.
    pub principal_kind: PrincipalKind,
    /// Issued-at, seconds since epoch.
    pub iat: i64,
    /// Not-before, seconds since epoch. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    /// Expiry, seconds since epoch.
    pub exp: i64,
    /// Unique token id.
    pub jti: String,
    /// Target audience for single-audience tokens
    /// ([`TokenType::InternalService`], [`TokenType::GatewayAccess`]).
    /// Must be absent on web sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    /// Permissions for the single target audience. Empty for web sessions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
    /// Permissions grouped by audience, for web sessions only. A backend
    /// service must never be required to parse permissions for audiences
    /// other than its own; single-audience tokens therefore never carry this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_permissions: Option<BTreeMap<String, Vec<String>>>,
    /// Authorization snapshot version at issuance time. Lets caches and
    /// derived tokens detect stale authorization state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authz_version: Option<u64>,
    /// Free-form additional claims (display name, provider id, ...).
    /// Never authoritative for authorization decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl RasClaims {
    /// Build claims for a multi-audience browser web session.
    pub fn web_session(
        issuer: impl Into<String>,
        user_id: impl Into<String>,
        audience_permissions: BTreeMap<String, Vec<String>>,
        ttl: Duration,
    ) -> Self {
        let now = Utc::now();
        Self {
            iss: issuer.into(),
            sub: user_id.into(),
            token_type: TokenType::WebSession,
            principal_kind: PrincipalKind::User,
            iat: now.timestamp(),
            nbf: None,
            exp: (now + ttl).timestamp(),
            jti: Uuid::new_v4().to_string(),
            aud: None,
            permissions: Vec::new(),
            audience_permissions: Some(audience_permissions),
            authz_version: None,
            metadata: None,
        }
    }

    /// Build claims for an internal service-to-service access token.
    pub fn internal_service(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        principal_kind: PrincipalKind,
        audience: impl Into<String>,
        permissions: Vec<String>,
        ttl: Duration,
    ) -> Self {
        let now = Utc::now();
        Self {
            iss: issuer.into(),
            sub: subject.into(),
            token_type: TokenType::InternalService,
            principal_kind,
            iat: now.timestamp(),
            nbf: None,
            exp: (now + ttl).timestamp(),
            jti: Uuid::new_v4().to_string(),
            aud: Some(audience.into()),
            permissions,
            audience_permissions: None,
            authz_version: None,
            metadata: None,
        }
    }

    /// Build claims for a gateway-derived single-audience backend token.
    ///
    /// The gateway must only call this with permissions extracted from a
    /// validated web session for exactly `audience` — never invented or
    /// widened.
    pub fn gateway_access(
        issuer: impl Into<String>,
        user_id: impl Into<String>,
        audience: impl Into<String>,
        permissions: Vec<String>,
        authz_version: Option<u64>,
        ttl: Duration,
    ) -> Self {
        let now = Utc::now();
        Self {
            iss: issuer.into(),
            sub: user_id.into(),
            token_type: TokenType::GatewayAccess,
            principal_kind: PrincipalKind::User,
            iat: now.timestamp(),
            nbf: None,
            exp: (now + ttl).timestamp(),
            jti: Uuid::new_v4().to_string(),
            aud: Some(audience.into()),
            permissions,
            audience_permissions: None,
            authz_version,
            metadata: None,
        }
    }

    /// Set the authorization snapshot version.
    pub fn with_authz_version(mut self, version: u64) -> Self {
        self.authz_version = Some(version);
        self
    }

    /// Attach free-form metadata claims.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Expiry as a [`DateTime`].
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp(self.exp, 0)
    }

    /// Check the structural invariants for this token's type.
    ///
    /// Enforced on both the signing and validation paths, so a malformed or
    /// hostile token cannot smuggle multi-audience permissions into a
    /// single-audience context or vice versa.
    pub fn validate_shape(&self) -> Result<(), String> {
        if self.iss.is_empty() {
            return Err("iss must not be empty".to_string());
        }
        if self.sub.is_empty() {
            return Err("sub must not be empty".to_string());
        }
        if self.exp <= self.iat {
            return Err("exp must be after iat".to_string());
        }
        match self.token_type {
            TokenType::WebSession => {
                if self.aud.is_some() {
                    return Err("web session tokens must not carry aud".to_string());
                }
                if !self.permissions.is_empty() {
                    return Err(
                        "web session tokens carry permissions in audience_permissions only"
                            .to_string(),
                    );
                }
                if self.audience_permissions.is_none() {
                    return Err("web session tokens require audience_permissions".to_string());
                }
            }
            TokenType::InternalService | TokenType::GatewayAccess => {
                match self.aud.as_deref() {
                    None | Some("") => {
                        return Err("single-audience tokens require a non-empty aud".to_string());
                    }
                    Some(_) => {}
                }
                if self.audience_permissions.is_some() {
                    return Err(
                        "single-audience tokens must not carry audience_permissions".to_string()
                    );
                }
            }
        }
        Ok(())
    }

    /// Permissions this token grants for `audience`, or `None` if the token
    /// does not cover that audience at all.
    ///
    /// For web sessions this looks up the audience group; for single-audience
    /// tokens it returns the permission list only when `aud` matches exactly.
    pub fn permissions_for_audience(&self, audience: &str) -> Option<&[String]> {
        match self.token_type {
            TokenType::WebSession => self
                .audience_permissions
                .as_ref()
                .and_then(|map| map.get(audience))
                .map(Vec::as_slice),
            TokenType::InternalService | TokenType::GatewayAccess => {
                if self.aud.as_deref() == Some(audience) {
                    Some(&self.permissions)
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audience_map(audience: &str, permissions: &[&str]) -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([(
            audience.to_string(),
            permissions.iter().map(|p| p.to_string()).collect(),
        )])
    }

    #[test]
    fn web_session_shape_is_valid() {
        let claims = RasClaims::web_session(
            "https://auth.internal",
            "alice",
            audience_map("invoice-service", &["invoice:read"]),
            Duration::minutes(30),
        );
        assert!(claims.validate_shape().is_ok());
        assert_eq!(claims.token_type, TokenType::WebSession);
        assert_eq!(claims.principal_kind, PrincipalKind::User);
        assert!(claims.aud.is_none());
    }

    #[test]
    fn internal_service_shape_is_valid() {
        let claims = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec!["invoice:write".to_string()],
            Duration::minutes(5),
        );
        assert!(claims.validate_shape().is_ok());
        assert_eq!(claims.aud.as_deref(), Some("invoice-service"));
    }

    #[test]
    fn web_session_with_aud_is_rejected() {
        let mut claims = RasClaims::web_session(
            "https://auth.internal",
            "alice",
            BTreeMap::new(),
            Duration::minutes(30),
        );
        claims.aud = Some("invoice-service".to_string());
        assert!(claims.validate_shape().is_err());
    }

    #[test]
    fn web_session_with_flat_permissions_is_rejected() {
        let mut claims = RasClaims::web_session(
            "https://auth.internal",
            "alice",
            BTreeMap::new(),
            Duration::minutes(30),
        );
        claims.permissions = vec!["invoice:read".to_string()];
        assert!(claims.validate_shape().is_err());
    }

    #[test]
    fn single_audience_token_without_aud_is_rejected() {
        let mut claims = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec![],
            Duration::minutes(5),
        );
        claims.aud = None;
        assert!(claims.validate_shape().is_err());

        let mut empty_aud = RasClaims::gateway_access(
            "https://auth.internal",
            "alice",
            "invoice-service",
            vec![],
            None,
            Duration::minutes(5),
        );
        empty_aud.aud = Some(String::new());
        assert!(empty_aud.validate_shape().is_err());
    }

    #[test]
    fn single_audience_token_with_audience_permissions_is_rejected() {
        let mut claims = RasClaims::gateway_access(
            "https://auth.internal",
            "alice",
            "invoice-service",
            vec!["invoice:read".to_string()],
            Some(7),
            Duration::minutes(5),
        );
        claims.audience_permissions = Some(BTreeMap::new());
        assert!(claims.validate_shape().is_err());
    }

    #[test]
    fn expiry_must_follow_issuance() {
        let mut claims = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec![],
            Duration::minutes(5),
        );
        claims.exp = claims.iat;
        assert!(claims.validate_shape().is_err());
    }

    #[test]
    fn permissions_for_audience_resolves_per_token_type() {
        let session = RasClaims::web_session(
            "https://auth.internal",
            "alice",
            audience_map("invoice-service", &["invoice:read"]),
            Duration::minutes(30),
        );
        assert_eq!(
            session.permissions_for_audience("invoice-service"),
            Some(&["invoice:read".to_string()][..])
        );
        assert_eq!(session.permissions_for_audience("billing-service"), None);

        let internal = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec!["invoice:write".to_string()],
            Duration::minutes(5),
        );
        assert_eq!(
            internal.permissions_for_audience("invoice-service"),
            Some(&["invoice:write".to_string()][..])
        );
        assert_eq!(internal.permissions_for_audience("other-service"), None);
    }

    #[test]
    fn token_type_serializes_to_stable_names() {
        assert_eq!(
            serde_json::to_value(TokenType::WebSession).unwrap(),
            serde_json::json!("ras_web_session")
        );
        assert_eq!(
            serde_json::to_value(TokenType::InternalService).unwrap(),
            serde_json::json!("ras_internal_access")
        );
        assert_eq!(
            serde_json::to_value(TokenType::GatewayAccess).unwrap(),
            serde_json::json!("ras_gateway_access")
        );
    }

    #[test]
    fn claims_round_trip_through_json() {
        let claims = RasClaims::gateway_access(
            "https://auth.internal",
            "alice",
            "invoice-service",
            vec!["invoice:read".to_string(), "invoice:approve".to_string()],
            Some(42),
            Duration::minutes(2),
        );
        let json = serde_json::to_value(&claims).unwrap();
        assert_eq!(json["typ"], "ras_gateway_access");
        assert_eq!(json["authz_version"], 42);
        let round_trip: RasClaims = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip, claims);
    }
}
