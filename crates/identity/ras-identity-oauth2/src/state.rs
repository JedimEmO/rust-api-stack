//! OAuth2 state management for CSRF protection.

use crate::error::{OAuth2Error, OAuth2Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
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

/// Default cap on concurrently pending flows held in memory.
const DEFAULT_MAX_PENDING_STATES: usize = 10_000;

/// Minimum interval between opportunistic expired-state sweeps in `store`.
const PRUNE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// In-memory implementation of [`OAuth2StateStore`].
///
/// Holds at most `max_states` pending flows. Expired flows are swept
/// opportunistically (at most once every 10 seconds) so abandoned flows do not
/// accumulate without an external `cleanup_expired` schedule. When the store
/// is full, the flow closest to expiry is evicted to make room for the new one
/// rather than refusing it (I3), so a burst of flow starts degrades to "the
/// oldest pending login must be restarted" instead of "nobody can log in".
///
/// This makes an attacker's cost for evicting legitimate flows one request per
/// slot. Production deployments should rate-limit flow starts at the edge
/// (per client IP / session) and size `max_states` accordingly.
pub struct InMemoryStateStore {
    states: Arc<RwLock<Inner>>,
    max_states: usize,
}

struct Inner {
    states: HashMap<String, OAuth2State>,
    last_prune: Instant,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_PENDING_STATES)
    }

    /// Create a store holding at most `max_states` pending flows. See the
    /// type-level docs for pruning and eviction behavior.
    pub fn with_capacity(max_states: usize) -> Self {
        Self {
            states: Arc::new(RwLock::new(Inner {
                states: HashMap::new(),
                last_prune: Instant::now(),
            })),
            max_states,
        }
    }

    /// Number of pending flows currently held (expired ones included until swept).
    pub async fn len(&self) -> usize {
        self.states.read().await.states.len()
    }

    /// Whether no flows are pending.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

impl Inner {
    /// Sweep expired states, but at most once per [`PRUNE_INTERVAL`] unless forced.
    fn prune_expired(&mut self, force: bool) -> usize {
        let now_instant = Instant::now();
        if !force && now_instant.duration_since(self.last_prune) < PRUNE_INTERVAL {
            return 0;
        }
        self.last_prune = now_instant;
        let now = Utc::now();
        let before = self.states.len();
        self.states.retain(|_, stored| now <= stored.expires_at);
        before - self.states.len()
    }

    /// Remove the pending flow closest to expiry (ties broken by creation time).
    fn evict_oldest(&mut self) {
        let victim = self
            .states
            .iter()
            .min_by_key(|(_, s)| (s.expires_at, s.created_at))
            .map(|(k, _)| k.clone());
        if let Some(key) = victim {
            self.states.remove(&key);
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
        let mut inner = self.states.write().await;

        // Opportunistic, rate-limited pruning: abandoned flows must not
        // accumulate just because nobody schedules cleanup_expired, but a
        // full O(n) sweep on every flow start is not acceptable either (I3).
        inner.prune_expired(false);

        if inner.states.len() >= self.max_states && !inner.states.contains_key(&state.state) {
            // Full: force a sweep first, then evict the flow closest to expiry.
            inner.prune_expired(true);
            if inner.states.len() >= self.max_states {
                inner.evict_oldest();
            }
        }

        inner.states.insert(state.state.clone(), state);
        Ok(())
    }

    async fn retrieve(&self, state: &str) -> OAuth2Result<OAuth2State> {
        let mut inner = self.states.write().await;

        // Remove and return the state
        let oauth_state = inner
            .states
            .remove(state)
            .ok_or(OAuth2Error::StateNotFound)?;

        // Check if expired
        if oauth_state.is_expired() {
            return Err(OAuth2Error::StateNotFound);
        }

        Ok(oauth_state)
    }

    async fn cleanup_expired(&self) -> OAuth2Result<usize> {
        let mut inner = self.states.write().await;
        Ok(inner.prune_expired(true))
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

    fn state_with_ttl(ttl: u64) -> OAuth2State {
        OAuth2State::new(
            "google".to_string(),
            "http://localhost/cb".to_string(),
            None,
            ttl,
        )
    }

    #[tokio::test]
    async fn i3_store_evicts_oldest_instead_of_refusing_at_capacity() {
        let store = InMemoryStateStore::with_capacity(2);
        let oldest = state_with_ttl(100);
        let middle = state_with_ttl(200);
        let newest = state_with_ttl(300);
        store.store(oldest.clone()).await.unwrap();
        store.store(middle.clone()).await.unwrap();

        // Full: the third store succeeds and evicts the flow closest to expiry.
        store
            .store(newest.clone())
            .await
            .expect("store must not refuse new flows at capacity");
        assert_eq!(store.len().await, 2);
        assert!(matches!(
            store.retrieve(&oldest.state).await,
            Err(OAuth2Error::StateNotFound)
        ));
        assert!(store.retrieve(&middle.state).await.is_ok());
        assert!(store.retrieve(&newest.state).await.is_ok());
    }

    #[tokio::test]
    async fn i3_store_prefers_pruning_expired_over_evicting_live_flows() {
        let store = InMemoryStateStore::with_capacity(2);

        let live = state_with_ttl(300);
        let mut expired = state_with_ttl(300);
        expired.expires_at = Utc::now() - Duration::seconds(10);
        store.store(live.clone()).await.unwrap();
        store.store(expired).await.unwrap();

        // Full, but the periodic sweep has not fired yet: the forced sweep at
        // capacity removes the expired flow so the live one survives.
        store.store(state_with_ttl(300)).await.unwrap();
        assert_eq!(store.len().await, 2);
        assert!(store.retrieve(&live.state).await.is_ok());
    }

    #[tokio::test]
    async fn i3_store_does_not_sweep_expired_on_every_call() {
        let store = InMemoryStateStore::with_capacity(100);

        let mut expired = state_with_ttl(300);
        expired.expires_at = Utc::now() - Duration::seconds(10);
        store.store(expired).await.unwrap();

        // Well under capacity and within the prune interval: no sweep, so the
        // expired entry is still counted...
        store.store(state_with_ttl(300)).await.unwrap();
        assert_eq!(store.len().await, 2);

        // ...until the interval has elapsed.
        store.states.write().await.last_prune = Instant::now() - PRUNE_INTERVAL;
        store.store(state_with_ttl(300)).await.unwrap();
        assert_eq!(store.len().await, 2);
    }
}
