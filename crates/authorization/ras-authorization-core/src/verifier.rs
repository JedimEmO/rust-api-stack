//! Pluggable service identity verification.
//!
//! A service proves its identity to the RAS authority through a
//! [`ServiceIdentityVerifier`]. The proof payload is verifier-specific JSON,
//! so production adapters (Kubernetes service-account JWTs, SPIFFE/SPIRE,
//! mTLS, cloud workload identity) can plug in without changing the issuer.
//!
//! [`StaticSecretVerifier`] is the development/simple-deployment verifier:
//! a static per-service secret. It is deliberately the *only* verifier
//! shipped here; production deployments should prefer workload identity.

use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::error::AuthzError;

/// A service's identity proof: which service it claims to be plus
/// verifier-specific evidence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServiceIdentityProof {
    pub service_id: String,
    /// Verifier-specific payload. For [`StaticSecretVerifier`]:
    /// `{"client_secret": "..."}`. Treated as sensitive: never logged.
    pub proof: serde_json::Value,
}

/// A successfully verified service identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedServiceIdentity {
    pub service_id: String,
}

/// Verifies service identity proofs.
///
/// Implementations must fail closed and must not leak why verification
/// failed (wrong service vs wrong credential) beyond
/// [`AuthzError::IdentityVerificationFailed`].
#[async_trait]
pub trait ServiceIdentityVerifier: Send + Sync {
    async fn verify(
        &self,
        proof: &ServiceIdentityProof,
    ) -> Result<VerifiedServiceIdentity, AuthzError>;
}

/// Development/simple-mode verifier: per-service static client secrets.
///
/// **Not for production.** Static secrets are long-lived bearer credentials
/// with no rotation, binding, or replay story; use a workload-identity
/// verifier in real deployments. Secrets must be at least 32 bytes and are
/// compared in constant time.
#[derive(Default)]
pub struct StaticSecretVerifier {
    secrets: RwLock<HashMap<String, Vec<u8>>>,
}

impl StaticSecretVerifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a service's static secret.
    pub async fn register(
        &self,
        service_id: impl Into<String>,
        secret: impl Into<Vec<u8>>,
    ) -> Result<(), AuthzError> {
        let secret = secret.into();
        if secret.len() < 32 {
            return Err(AuthzError::InvalidConfig(
                "static service secrets must be at least 32 bytes".to_string(),
            ));
        }
        self.secrets.write().await.insert(service_id.into(), secret);
        Ok(())
    }
}

/// Constant-time byte comparison: timing reveals only the lengths.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[async_trait]
impl ServiceIdentityVerifier for StaticSecretVerifier {
    async fn verify(
        &self,
        proof: &ServiceIdentityProof,
    ) -> Result<VerifiedServiceIdentity, AuthzError> {
        let failed = || AuthzError::IdentityVerificationFailed {
            service_id: proof.service_id.clone(),
        };

        let presented = proof
            .proof
            .get("client_secret")
            .and_then(|value| value.as_str())
            .ok_or_else(failed)?;

        let secrets = self.secrets.read().await;
        let expected = secrets.get(&proof.service_id).ok_or_else(failed)?;
        if constant_time_eq(presented.as_bytes(), expected) {
            Ok(VerifiedServiceIdentity {
                service_id: proof.service_id.clone(),
            })
        } else {
            Err(failed())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "a-static-service-secret-of-32-bytes!";

    fn proof(service_id: &str, secret: &str) -> ServiceIdentityProof {
        ServiceIdentityProof {
            service_id: service_id.to_string(),
            proof: serde_json::json!({ "client_secret": secret }),
        }
    }

    #[tokio::test]
    async fn correct_secret_verifies() {
        let verifier = StaticSecretVerifier::new();
        verifier
            .register("billing", SECRET.as_bytes())
            .await
            .unwrap();
        let identity = verifier.verify(&proof("billing", SECRET)).await.unwrap();
        assert_eq!(identity.service_id, "billing");
    }

    #[tokio::test]
    async fn wrong_secret_unknown_service_and_malformed_proof_fail_identically() {
        let verifier = StaticSecretVerifier::new();
        verifier
            .register("billing", SECRET.as_bytes())
            .await
            .unwrap();

        for bad in [
            proof("billing", "wrong-secret-that-is-also-32-bytes!"),
            proof("unknown-service", SECRET),
            ServiceIdentityProof {
                service_id: "billing".to_string(),
                proof: serde_json::json!({}),
            },
        ] {
            let err = verifier.verify(&bad).await.unwrap_err();
            assert!(matches!(err, AuthzError::IdentityVerificationFailed { .. }));
        }
    }

    #[tokio::test]
    async fn short_secrets_are_rejected_at_registration() {
        let verifier = StaticSecretVerifier::new();
        let err = verifier
            .register("billing", b"short".to_vec())
            .await
            .unwrap_err();
        assert!(matches!(err, AuthzError::InvalidConfig(_)));
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
