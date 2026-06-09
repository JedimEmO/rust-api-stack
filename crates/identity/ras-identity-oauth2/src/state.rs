//! OAuth2 state management for CSRF protection.

use crate::error::{OAuth2Error, OAuth2Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// OAuth2 state information stored during authorization flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2State {
    pub state: String,
    pub provider_id: String,
    pub redirect_uri: String,
    pub code_verifier: Option<String>,
    /// OIDC nonce sent in the authorization request; the id_token returned
    /// on callback must echo it.
    pub nonce: Option<String>,
    /// Optional caller-supplied value binding this flow to the browser
    /// session that started it (e.g. a random cookie value). When set, the
    /// callback must present the identical value, preventing login CSRF.
    pub binding: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

impl OAuth2State {
    pub fn new(
        provider_id: String,
        redirect_uri: String,
        code_verifier: Option<String>,
        ttl_seconds: u64,
    ) -> Self {
        let state = Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let expires_at = created_at + Duration::seconds(ttl_seconds as i64);

        Self {
            state,
            provider_id,
            redirect_uri,
            code_verifier,
            nonce: None,
            binding: None,
            created_at,
            expires_at,
            metadata: None,
        }
    }

    /// Attach an OIDC nonce to the flow.
    pub fn with_nonce(mut self, nonce: String) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Bind the flow to the initiating browser session (login-CSRF guard).
    pub fn with_binding(mut self, binding: Option<String>) -> Self {
        self.binding = binding;
        self
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

/// Trait for OAuth2 state storage
#[async_trait]
pub trait OAuth2StateStore: Send + Sync {
    /// Store a new state
    async fn store(&self, state: OAuth2State) -> OAuth2Result<()>;

    /// Retrieve and remove a state by its state parameter
    async fn retrieve(&self, state: &str) -> OAuth2Result<OAuth2State>;

    /// Clean up expired states
    async fn cleanup_expired(&self) -> OAuth2Result<usize>;
}

/// In-memory implementation of OAuth2StateStore
pub struct InMemoryStateStore {
    states: Arc<RwLock<HashMap<String, OAuth2State>>>,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OAuth2StateStore for InMemoryStateStore {
    async fn store(&self, state: OAuth2State) -> OAuth2Result<()> {
        let mut states = self.states.write().await;
        states.insert(state.state.clone(), state);
        Ok(())
    }

    async fn retrieve(&self, state: &str) -> OAuth2Result<OAuth2State> {
        let mut states = self.states.write().await;

        // Remove and return the state
        let oauth_state = states.remove(state).ok_or(OAuth2Error::StateNotFound)?;

        // Check if expired
        if oauth_state.is_expired() {
            return Err(OAuth2Error::StateNotFound);
        }

        Ok(oauth_state)
    }

    async fn cleanup_expired(&self) -> OAuth2Result<usize> {
        let mut states = self.states.write().await;
        let now = Utc::now();

        let expired_keys: Vec<String> = states
            .iter()
            .filter(|(_, state)| now > state.expires_at)
            .map(|(key, _)| key.clone())
            .collect();

        let count = expired_keys.len();

        for key in expired_keys {
            states.remove(&key);
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_state_store() {
        let store = InMemoryStateStore::new();

        // Create a state
        let state = OAuth2State::new(
            "google".to_string(),
            "http://localhost:3000/callback".to_string(),
            Some("verifier123".to_string()),
            300, // 5 minutes
        );

        let state_param = state.state.clone();

        // Store the state
        store.store(state.clone()).await.unwrap();

        // Retrieve the state
        let retrieved = store.retrieve(&state_param).await.unwrap();
        assert_eq!(retrieved.provider_id, "google");
        assert_eq!(retrieved.code_verifier, Some("verifier123".to_string()));

        // Try to retrieve again - should fail
        let result = store.retrieve(&state_param).await;
        assert!(matches!(result, Err(OAuth2Error::StateNotFound)));
    }

    #[tokio::test]
    async fn test_expired_state_cleanup() {
        let store = InMemoryStateStore::new();

        // Create an expired state
        let mut state = OAuth2State::new(
            "google".to_string(),
            "http://localhost:3000/callback".to_string(),
            None,
            300,
        );

        // Manually set to expired
        state.expires_at = Utc::now() - Duration::minutes(1);

        store.store(state.clone()).await.unwrap();

        // Cleanup expired states
        let cleaned = store.cleanup_expired().await.unwrap();
        assert_eq!(cleaned, 1);

        // Verify the state is gone
        let result = store.retrieve(&state.state).await;
        assert!(matches!(result, Err(OAuth2Error::StateNotFound)));
    }
}
