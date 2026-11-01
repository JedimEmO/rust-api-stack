//! Guards that a user ID names its identity provider, so equal subjects at two providers never share an account or evict each other's sessions.

use async_trait::async_trait;
use ras_auth_core::AuthProvider;
use ras_identity_core::{IdentityProvider, IdentityResult, VerifiedIdentity};
use ras_identity_session::{JwtAuthProvider, SessionConfig, SessionError, SessionService};
use std::sync::Arc;

/// Registers under `registered_id` and reports identities for `identity_provider_id`.
struct FixedIdentity {
    registered_id: &'static str,
    identity_provider_id: &'static str,
}

impl FixedIdentity {
    fn new(id: &'static str) -> Self {
        Self::reporting(id, id)
    }

    fn reporting(registered_id: &'static str, identity_provider_id: &'static str) -> Self {
        Self {
            registered_id,
            identity_provider_id,
        }
    }
}

#[async_trait]
impl IdentityProvider for FixedIdentity {
    fn provider_id(&self) -> &str {
        self.registered_id
    }

    async fn verify(&self, _: serde_json::Value) -> IdentityResult<VerifiedIdentity> {
        // Two trusted providers can independently assign the same subject.
        Ok(VerifiedIdentity {
            provider_id: self.identity_provider_id.into(),
            subject: "12345".into(),
            email: None,
            display_name: None,
            metadata: None,
        })
    }
}

fn bare_service(max: usize) -> Arc<SessionService> {
    let config = SessionConfig::new(
        "f27929932dc7269b950dc1e5c064111f105c67036a7386ca",
        "review",
        "review",
    )
    .unwrap()
    .with_max_sessions_per_user(max);
    Arc::new(SessionService::new(config).unwrap())
}

async fn service(max: usize) -> Arc<SessionService> {
    let service = bare_service(max);
    service
        .register_provider(Box::new(FixedIdentity::new("idp-a")))
        .await;
    service
        .register_provider(Box::new(FixedIdentity::new("idp-b")))
        .await;
    service
}

#[tokio::test]
async fn different_providers_must_not_resolve_to_the_same_user_id() {
    let service = service(32).await;
    let a = service
        .begin_session("idp-a", serde_json::json!({}))
        .await
        .unwrap();
    let b = service
        .begin_session("idp-b", serde_json::json!({}))
        .await
        .unwrap();
    assert_ne!(
        service.verify_session(&a).await.unwrap().provider_id,
        service.verify_session(&b).await.unwrap().provider_id,
    );
    let auth = JwtAuthProvider::new(service);
    let user_a = auth.authenticate(a).await.unwrap();
    let user_b = auth.authenticate(b).await.unwrap();
    assert_eq!(user_a.user_id, "idp-a:12345");
    assert_eq!(user_b.user_id, "idp-b:12345");
}

#[tokio::test]
async fn logging_in_at_another_provider_must_not_evict_the_victim_session() {
    let service = service(1).await;
    let victim = service
        .begin_session("idp-a", serde_json::json!({}))
        .await
        .unwrap();
    service
        .begin_session("idp-b", serde_json::json!({}))
        .await
        .unwrap();
    assert!(service.verify_session(&victim).await.is_ok());
}

#[tokio::test]
async fn a_provider_may_issue_identities_in_its_own_sub_namespace() {
    let service = bare_service(32);
    service
        .register_provider(Box::new(FixedIdentity::reporting(
            "oauth2",
            "oauth2:google",
        )))
        .await;
    let token = service
        .begin_session("oauth2", serde_json::json!({}))
        .await
        .unwrap();
    let claims = service.verify_session(&token).await.unwrap();
    assert_eq!(claims.sub, "oauth2:google:12345");
    assert_eq!(claims.provider_id, "oauth2:google");
}

#[tokio::test]
async fn a_provider_must_not_issue_identities_for_another_provider() {
    let service = bare_service(32);
    for spoofed in ["idp-b", "idp-ab", "idp", ""] {
        service
            .register_provider(Box::new(FixedIdentity::reporting("idp-a", spoofed)))
            .await;
        let result = service.begin_session("idp-a", serde_json::json!({})).await;
        assert!(
            matches!(result, Err(SessionError::IdentityError(_))),
            "an identity for {spoofed:?} from provider idp-a must be rejected",
        );
    }
}

#[tokio::test]
#[should_panic(expected = "must not contain ':'")]
async fn provider_ids_with_a_colon_are_refused_at_registration() {
    bare_service(32)
        .register_provider(Box::new(FixedIdentity::new("oauth2:google")))
        .await;
}
