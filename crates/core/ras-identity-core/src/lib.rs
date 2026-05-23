//! Core identity provider traits and types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Unsupported authentication method")]
    UnsupportedMethod,

    #[error("Invalid authentication payload")]
    InvalidPayload,

    #[error("Session error: {0}")]
    SessionError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

pub type IdentityResult<T> = Result<T, IdentityError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedIdentity {
    pub provider_id: String,
    pub subject: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn provider_id(&self) -> &str;

    async fn verify(&self, auth_payload: serde_json::Value) -> IdentityResult<VerifiedIdentity>;
}

#[async_trait]
pub trait UserPermissions: Send + Sync {
    async fn get_permissions(&self, identity: &VerifiedIdentity) -> IdentityResult<Vec<String>>;
}

/// A default implementation that returns no permissions
pub struct NoopPermissions;

#[async_trait]
impl UserPermissions for NoopPermissions {
    async fn get_permissions(&self, _identity: &VerifiedIdentity) -> IdentityResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// A static permissions provider that returns the same permissions for all users
pub struct StaticPermissions {
    permissions: Vec<String>,
}

impl StaticPermissions {
    pub fn new(permissions: Vec<String>) -> Self {
        Self { permissions }
    }
}

#[async_trait]
impl UserPermissions for StaticPermissions {
    async fn get_permissions(&self, _identity: &VerifiedIdentity) -> IdentityResult<Vec<String>> {
        Ok(self.permissions.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vi() -> VerifiedIdentity {
        VerifiedIdentity {
            provider_id: "test".into(),
            subject: "alice".into(),
            email: Some("a@b.com".into()),
            display_name: Some("Alice".into()),
            metadata: None,
        }
    }

    struct SubjectIdentityProvider;

    #[async_trait]
    impl IdentityProvider for SubjectIdentityProvider {
        fn provider_id(&self) -> &str {
            "subject"
        }

        async fn verify(
            &self,
            auth_payload: serde_json::Value,
        ) -> IdentityResult<VerifiedIdentity> {
            let subject = auth_payload
                .get("subject")
                .and_then(|value| value.as_str())
                .ok_or(IdentityError::InvalidPayload)?;

            Ok(VerifiedIdentity {
                provider_id: self.provider_id().to_string(),
                subject: subject.to_string(),
                email: auth_payload
                    .get("email")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                display_name: None,
                metadata: Some(json!({ "source": "test" })),
            })
        }
    }

    #[test]
    fn identity_error_display_per_variant() {
        assert_eq!(
            IdentityError::InvalidCredentials.to_string(),
            "Invalid credentials"
        );
        assert_eq!(
            IdentityError::ProviderNotFound("foo".into()).to_string(),
            "Provider not found: foo"
        );
        assert_eq!(
            IdentityError::ProviderError("bad".into()).to_string(),
            "Provider error: bad"
        );
        assert_eq!(
            IdentityError::UnsupportedMethod.to_string(),
            "Unsupported authentication method"
        );
        assert_eq!(
            IdentityError::InvalidPayload.to_string(),
            "Invalid authentication payload"
        );
        assert_eq!(
            IdentityError::SessionError("expired".into()).to_string(),
            "Session error: expired"
        );

        let parse_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let wrapped: IdentityError = parse_err.into();
        assert!(wrapped.to_string().starts_with("Serialization error:"));
    }

    #[tokio::test]
    async fn noop_permissions_returns_empty() {
        let p = NoopPermissions;
        let perms = p.get_permissions(&vi()).await.unwrap();
        assert!(perms.is_empty());
    }

    #[tokio::test]
    async fn static_permissions_returns_provided_list() {
        let p = StaticPermissions::new(vec!["a".into(), "b".into()]);
        let perms = p.get_permissions(&vi()).await.unwrap();
        assert_eq!(perms, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn static_permissions_returns_a_fresh_vec_each_time() {
        let p = StaticPermissions::new(vec!["read".into(), "write".into()]);
        let mut first = p.get_permissions(&vi()).await.unwrap();
        first.push("mutated".to_string());

        let second = p.get_permissions(&vi()).await.unwrap();

        assert_eq!(second, vec!["read".to_string(), "write".to_string()]);
    }

    #[test]
    fn verified_identity_serde_round_trips() {
        let v = vi();
        let json = serde_json::to_string(&v).unwrap();
        let parsed: VerifiedIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.subject, "alice");
        assert_eq!(parsed.provider_id, "test");
    }

    #[test]
    fn verified_identity_serializes_optional_profile_and_metadata() {
        let identity = VerifiedIdentity {
            provider_id: "oauth2".to_string(),
            subject: "user-123".to_string(),
            email: Some("user@example.test".to_string()),
            display_name: Some("Example User".to_string()),
            metadata: Some(json!({
                "tenant": "demo",
                "groups": ["engineering", "admin"]
            })),
        };

        assert_eq!(
            serde_json::to_value(identity).unwrap(),
            json!({
                "provider_id": "oauth2",
                "subject": "user-123",
                "email": "user@example.test",
                "display_name": "Example User",
                "metadata": {
                    "tenant": "demo",
                    "groups": ["engineering", "admin"]
                }
            })
        );
    }

    #[tokio::test]
    async fn identity_provider_trait_verifies_valid_payload_and_rejects_invalid_payload() {
        let provider = SubjectIdentityProvider;

        let identity = provider
            .verify(json!({
                "subject": "alice",
                "email": "alice@example.test"
            }))
            .await
            .expect("valid payload verifies");

        assert_eq!(identity.provider_id, "subject");
        assert_eq!(identity.subject, "alice");
        assert_eq!(identity.email, Some("alice@example.test".to_string()));
        assert_eq!(identity.metadata, Some(json!({ "source": "test" })));

        let error = provider
            .verify(json!({ "email": "alice@example.test" }))
            .await
            .expect_err("missing subject should fail");

        assert!(matches!(error, IdentityError::InvalidPayload));
    }
}
