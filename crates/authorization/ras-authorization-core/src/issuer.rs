//! The internal service token issuer.
//!
//! Issuance pipeline, every step fail-closed:
//!
//! 1. Verify the caller's service identity through the pluggable verifier.
//! 2. The service must be registered and enabled.
//! 3. The target audience must belong to a registered service.
//! 4. Requested permissions must all be granted to the service principal
//!    *for that audience* (audience-scoped grants).
//! 5. If a service-graph policy is loaded, the caller→audience edge must be
//!    declared and the requested permissions within the edge's ceiling.
//! 6. Mint a short-lived single-audience `ras_internal_access` JWT stamped
//!    with the current `authz_version`.
//!
//! Every issuance, denial, and identity failure emits an audit event.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use ras_authorization_token::{JwkSet, KeyRing, RasClaims, SigningKey};
use tokio::sync::RwLock;

use crate::audit::{AuditEvent, AuditEventKind, AuditSink};
use crate::error::AuthzError;
use crate::model::{Principal, ServiceGraphPolicy};
use crate::store::AuthorizationStore;
use crate::verifier::{ServiceIdentityProof, ServiceIdentityVerifier};

/// A request for an internal service-to-service token.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InternalTokenRequest {
    /// The caller's identity proof.
    pub proof: ServiceIdentityProof,
    /// Target service audience.
    pub audience: String,
    /// Requested permissions (must be granted for that audience).
    pub permissions: Vec<String>,
}

/// A successfully issued token.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IssuedToken {
    /// The signed JWT. Bearer credential — handle accordingly.
    pub token: String,
    pub expires_at: DateTime<Utc>,
    /// The claims that were signed (for logging-safe introspection; contains
    /// no secret material).
    #[serde(skip)]
    pub claims: RasClaims,
}

/// Builder for [`TokenIssuer`].
pub struct TokenIssuerBuilder {
    issuer: String,
    keys: KeyRing,
    store: Arc<dyn AuthorizationStore>,
    verifier: Arc<dyn ServiceIdentityVerifier>,
    audit: Arc<dyn AuditSink>,
    token_ttl: Duration,
}

impl TokenIssuerBuilder {
    /// Override the internal token TTL (default 5 minutes). Internal tokens
    /// are validated offline, so the TTL bounds revocation latency.
    pub fn token_ttl(mut self, ttl: Duration) -> Self {
        self.token_ttl = ttl;
        self
    }

    /// Attach an audit sink (default: drop events).
    pub fn audit(mut self, audit: Arc<dyn AuditSink>) -> Self {
        self.audit = audit;
        self
    }

    pub fn build(self) -> TokenIssuer {
        TokenIssuer {
            issuer: self.issuer,
            keys: RwLock::new(self.keys),
            store: self.store,
            verifier: self.verifier,
            audit: self.audit,
            policy: RwLock::new(None),
            token_ttl: self.token_ttl,
        }
    }
}

/// The RAS authority's internal token issuer.
pub struct TokenIssuer {
    issuer: String,
    keys: RwLock<KeyRing>,
    store: Arc<dyn AuthorizationStore>,
    verifier: Arc<dyn ServiceIdentityVerifier>,
    audit: Arc<dyn AuditSink>,
    policy: RwLock<Option<ServiceGraphPolicy>>,
    token_ttl: Duration,
}

impl TokenIssuer {
    /// Start building an issuer. `issuer` is the `iss` claim value (the
    /// authority's URL or stable identifier).
    pub fn builder(
        issuer: impl Into<String>,
        active_key: SigningKey,
        store: Arc<dyn AuthorizationStore>,
        verifier: Arc<dyn ServiceIdentityVerifier>,
    ) -> TokenIssuerBuilder {
        TokenIssuerBuilder {
            issuer: issuer.into(),
            keys: KeyRing::new(active_key),
            store,
            verifier,
            audit: Arc::new(crate::audit::NoopAuditSink),
            token_ttl: Duration::minutes(5),
        }
    }

    /// The `iss` value this issuer signs with.
    pub fn issuer_id(&self) -> &str {
        &self.issuer
    }

    /// Load (or replace) the service-graph policy. With a policy loaded,
    /// service-principal issuance outside the declared edges fails closed.
    pub async fn load_policy(&self, policy: ServiceGraphPolicy) {
        self.audit
            .record(
                AuditEvent::new(
                    AuditEventKind::PolicyLoaded,
                    format!(
                        "loaded service graph policy {} ({} edges)",
                        policy.policy_id,
                        policy.edges.len()
                    ),
                )
                .with_target(policy.topology_name.clone()),
            )
            .await;
        *self.policy.write().await = Some(policy);
    }

    /// The public JWKS for downstream validation.
    pub async fn jwks(&self) -> JwkSet {
        self.keys.read().await.jwks()
    }

    /// Rotate the signing key. Outstanding tokens stay valid until expiry.
    pub async fn rotate_key(&self, new_active: SigningKey) {
        let kid = new_active.kid().to_string();
        self.keys.write().await.rotate(new_active);
        self.audit
            .record(
                AuditEvent::new(AuditEventKind::SigningKeyRotated, "signing key rotated")
                    .with_target(kid),
            )
            .await;
    }

    /// Emergency-remove a retired verification key, immediately invalidating
    /// tokens signed with it.
    pub async fn remove_retired_key(&self, kid: &str) -> bool {
        let removed = self.keys.write().await.remove_retired(kid);
        if removed {
            self.audit
                .record(
                    AuditEvent::new(
                        AuditEventKind::SigningKeyRemoved,
                        "retired signing key removed",
                    )
                    .with_target(kid.to_string()),
                )
                .await;
        }
        removed
    }

    /// Issue an internal service token. See the module docs for the
    /// fail-closed pipeline.
    pub async fn issue_internal_token(
        &self,
        request: InternalTokenRequest,
    ) -> Result<IssuedToken, AuthzError> {
        match self.try_issue(&request).await {
            Ok(issued) => {
                self.audit
                    .record(
                        AuditEvent::new(
                            AuditEventKind::TokenIssued,
                            format!(
                                "issued internal token for audience {:?} with permissions {:?}",
                                request.audience, request.permissions
                            ),
                        )
                        .with_actor(request.proof.service_id.clone())
                        .with_target(request.audience.clone()),
                    )
                    .await;
                Ok(issued)
            }
            Err(err) => {
                let kind = match &err {
                    AuthzError::IdentityVerificationFailed { .. } => {
                        AuditEventKind::IdentityVerificationFailed
                    }
                    _ => AuditEventKind::TokenIssuanceDenied,
                };
                self.audit
                    .record(
                        AuditEvent::new(kind, err.to_string())
                            .with_actor(request.proof.service_id.clone())
                            .with_target(request.audience.clone()),
                    )
                    .await;
                Err(err)
            }
        }
    }

    async fn try_issue(&self, request: &InternalTokenRequest) -> Result<IssuedToken, AuthzError> {
        // 1. Identity.
        let identity = self.verifier.verify(&request.proof).await?;

        // 2. Registration.
        let service = self
            .store
            .get_service(&identity.service_id)
            .await?
            .ok_or_else(|| AuthzError::UnknownService {
                service_id: identity.service_id.clone(),
            })?;
        if !service.enabled {
            return Err(AuthzError::ServiceDisabled {
                service_id: service.service_id,
            });
        }

        // 3. Target audience must exist.
        if !self.store.audience_exists(&request.audience).await? {
            return Err(AuthzError::UnknownAudience {
                audience: request.audience.clone(),
            });
        }

        // 4. Audience-scoped grants.
        let principal = Principal::Service {
            service_id: service.service_id.clone(),
        };
        let resolved = self.store.resolve_permissions(&principal).await?;
        let granted = resolved.get(&request.audience);
        let missing: Vec<String> = request
            .permissions
            .iter()
            .filter(|permission| !granted.is_some_and(|set| set.contains(*permission)))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(AuthzError::PermissionsNotGranted {
                audience: request.audience.clone(),
                missing,
            });
        }

        // 5. Topology policy, when loaded.
        if let Some(policy) = self.policy.read().await.as_ref() {
            let edge = policy
                .edge(&service.service_id, &request.audience)
                .ok_or_else(|| AuthzError::EdgeNotAllowed {
                    caller: service.service_id.clone(),
                    audience: request.audience.clone(),
                })?;
            let outside: Vec<String> = request
                .permissions
                .iter()
                .filter(|permission| !edge.permissions.contains(*permission))
                .cloned()
                .collect();
            if !outside.is_empty() {
                return Err(AuthzError::PermissionsNotGranted {
                    audience: request.audience.clone(),
                    missing: outside,
                });
            }
        }

        // 6. Mint.
        let authz_version = self.store.authz_version().await?;
        let claims = RasClaims::internal_service(
            self.issuer.clone(),
            service.service_id,
            principal.principal_kind(),
            request.audience.clone(),
            request.permissions.clone(),
            self.token_ttl,
        )
        .with_authz_version(authz_version);

        let token = self.keys.read().await.sign(&claims)?;
        let expires_at = claims
            .expires_at()
            .ok_or_else(|| AuthzError::InvalidConfig("token expiry out of range".to_string()))?;

        Ok(IssuedToken {
            token,
            expires_at,
            claims,
        })
    }
}
