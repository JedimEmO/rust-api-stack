//! Grant storage: refresh tokens and other long-lived user grants.
//!
//! A refresh token is a stored *grant*. RAS never conjures one; the
//! application provides it through a consent flow, admin seeding, or
//! migration, and token sources use it to acquire access tokens. The
//! [`GrantStore`] is a security boundary: implementations hold long-lived
//! credentials and must be treated accordingly.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::error::IntegrationError;
use crate::secret::SecretString;

/// A stored user grant for an external integration.
///
/// `Debug` redacts the refresh token; the type deliberately does not
/// implement serde traits.
#[derive(Clone)]
pub struct UserGrant {
    pub integration_id: String,
    pub user_id: String,
    pub refresh_token: SecretString,
    /// The scopes the user consented to. Token requests are subset-checked
    /// against these; broader requests require a new consent flow.
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for UserGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserGrant")
            .field("integration_id", &self.integration_id)
            .field("user_id", &self.user_id)
            .field("refresh_token", &self.refresh_token)
            .field("scopes", &self.scopes)
            .finish()
    }
}

/// Persistence for user grants. Production deployments implement this over
/// their database/secret manager; [`InMemoryGrantStore`] serves tests, dev,
/// and examples.
#[async_trait]
pub trait GrantStore: Send + Sync {
    async fn get_user_grant(
        &self,
        integration_id: &str,
        user_id: &str,
    ) -> Result<Option<UserGrant>, IntegrationError>;

    /// Insert or replace a grant. Token sources call this to persist
    /// refresh-token rotation; failures must surface, because losing a
    /// rotated refresh token invalidates the stored grant silently.
    async fn put_user_grant(&self, grant: UserGrant) -> Result<(), IntegrationError>;

    /// Remove a grant (user disconnect / admin revocation). Returns whether
    /// a grant existed.
    async fn remove_user_grant(
        &self,
        integration_id: &str,
        user_id: &str,
    ) -> Result<bool, IntegrationError>;
}

/// In-memory grant store for tests, dev, and examples.
#[derive(Default)]
pub struct InMemoryGrantStore {
    grants: RwLock<HashMap<(String, String), UserGrant>>,
}

impl InMemoryGrantStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl GrantStore for InMemoryGrantStore {
    async fn get_user_grant(
        &self,
        integration_id: &str,
        user_id: &str,
    ) -> Result<Option<UserGrant>, IntegrationError> {
        let grants = self.grants.read().await;
        Ok(grants
            .get(&(integration_id.to_string(), user_id.to_string()))
            .cloned())
    }

    async fn put_user_grant(&self, grant: UserGrant) -> Result<(), IntegrationError> {
        let mut grants = self.grants.write().await;
        grants.insert((grant.integration_id.clone(), grant.user_id.clone()), grant);
        Ok(())
    }

    async fn remove_user_grant(
        &self,
        integration_id: &str,
        user_id: &str,
    ) -> Result<bool, IntegrationError> {
        let mut grants = self.grants.write().await;
        Ok(grants
            .remove(&(integration_id.to_string(), user_id.to_string()))
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(user: &str) -> UserGrant {
        UserGrant {
            integration_id: "google-calendar".to_string(),
            user_id: user.to_string(),
            refresh_token: SecretString::new("refresh-secret"),
            scopes: vec!["calendar.readonly".to_string()],
        }
    }

    #[tokio::test]
    async fn put_get_remove_round_trip() {
        let store = InMemoryGrantStore::new();
        assert!(
            store
                .get_user_grant("google-calendar", "alice")
                .await
                .unwrap()
                .is_none()
        );

        store.put_user_grant(grant("alice")).await.unwrap();
        let stored = store
            .get_user_grant("google-calendar", "alice")
            .await
            .unwrap()
            .expect("grant stored");
        assert_eq!(stored.scopes, vec!["calendar.readonly"]);

        assert!(
            store
                .remove_user_grant("google-calendar", "alice")
                .await
                .unwrap()
        );
        assert!(
            !store
                .remove_user_grant("google-calendar", "alice")
                .await
                .unwrap()
        );
    }

    #[test]
    fn grant_debug_redacts_refresh_token() {
        let debug = format!("{:?}", grant("alice"));
        assert!(!debug.contains("refresh-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
