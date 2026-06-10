//! The token manager: bounds-checked, cached, deduplicated token acquisition.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use tokio::sync::Mutex;

use crate::config::IntegrationConfig;
use crate::error::IntegrationError;
use crate::types::{TokenFamily, TokenLease, TokenRequest, TokenSource, TokenSubject};

/// Cache key for a leased token.
///
/// Includes the token family, integration, subject (with principal mode),
/// audience, canonicalized scopes, and config version — so external OAuth
/// tokens, internal RAS tokens, different principals, and different
/// configurations can never collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    family: TokenFamily,
    integration_id: String,
    subject: String,
    audience: Option<String>,
    scopes: Vec<String>,
    config_version: u64,
}

struct RegisteredIntegration {
    config: IntegrationConfig,
    source: Arc<dyn TokenSource>,
}

/// Builder for [`TokenManager`].
#[derive(Default)]
pub struct TokenManagerBuilder {
    integrations: HashMap<String, RegisteredIntegration>,
    refresh_skew: Option<Duration>,
}

impl TokenManagerBuilder {
    /// Register an integration with its token source. Duplicate ids are
    /// rejected.
    pub fn register(
        mut self,
        config: IntegrationConfig,
        source: Arc<dyn TokenSource>,
    ) -> Result<Self, IntegrationError> {
        let id = config.integration_id.clone();
        if self.integrations.contains_key(&id) {
            return Err(IntegrationError::InvalidConfig(format!(
                "integration {id:?} registered twice"
            )));
        }
        self.integrations
            .insert(id, RegisteredIntegration { config, source });
        Ok(self)
    }

    /// How long before expiry a cached lease is considered stale and
    /// refreshed (default 60 seconds).
    pub fn refresh_skew(mut self, skew: Duration) -> Self {
        self.refresh_skew = Some(skew);
        self
    }

    pub fn build(self) -> TokenManager {
        TokenManager {
            integrations: self.integrations,
            cache: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            refresh_skew: self.refresh_skew.unwrap_or_else(|| Duration::seconds(60)),
        }
    }
}

/// Acquires, caches, and refreshes tokens through registered
/// [`TokenSource`]s, enforcing each integration's configured bounds.
///
/// Handler code should not use this directly; inject capability-scoped
/// [`crate::AuthorizedHttpClient`]s instead, so handlers cannot request
/// arbitrary integrations, scopes, audiences, or subjects.
pub struct TokenManager {
    integrations: HashMap<String, RegisteredIntegration>,
    cache: Mutex<HashMap<CacheKey, TokenLease>>,
    inflight: Mutex<HashMap<CacheKey, Arc<Mutex<()>>>>,
    refresh_skew: Duration,
}

impl TokenManager {
    pub fn builder() -> TokenManagerBuilder {
        TokenManagerBuilder::default()
    }

    /// The configuration for an integration, if registered.
    pub fn config(&self, integration_id: &str) -> Option<&IntegrationConfig> {
        self.integrations
            .get(integration_id)
            .map(|integration| &integration.config)
    }

    /// Check that `url` is an allowed outbound target for the integration.
    /// Managed bearer tokens must only be attached after this passes.
    pub fn validate_outbound_url(
        &self,
        integration_id: &str,
        url: &str,
    ) -> Result<(), IntegrationError> {
        let integration = self.integrations.get(integration_id).ok_or_else(|| {
            IntegrationError::UnknownIntegration {
                integration_id: integration_id.to_string(),
            }
        })?;
        if integration.config.allows_url(url) {
            Ok(())
        } else {
            Err(IntegrationError::HostNotAllowed {
                integration_id: integration_id.to_string(),
                url: url.to_string(),
            })
        }
    }

    /// Acquire a token for `request`, serving from cache when fresh.
    ///
    /// Scope and audience bounds are enforced before any source call.
    /// Concurrent requests for the same cache key are deduplicated: one
    /// caller refreshes, the rest wait and reuse the result.
    pub async fn get_token(&self, request: TokenRequest) -> Result<TokenLease, IntegrationError> {
        let integration = self
            .integrations
            .get(&request.integration_id)
            .ok_or_else(|| IntegrationError::UnknownIntegration {
                integration_id: request.integration_id.clone(),
            })?;

        for scope in &request.scopes {
            if !integration.config.allowed_scopes.contains(scope) {
                return Err(IntegrationError::ScopeNotAllowed {
                    integration_id: request.integration_id.clone(),
                    scope: scope.clone(),
                });
            }
        }
        if let Some(audience) = &request.audience
            && !integration.config.allowed_audiences.contains(audience)
        {
            return Err(IntegrationError::AudienceNotAllowed {
                integration_id: request.integration_id.clone(),
                audience: audience.clone(),
            });
        }

        let mut scopes = request.scopes.clone();
        scopes.sort();
        scopes.dedup();

        let key = CacheKey {
            family: integration.source.family(),
            integration_id: request.integration_id.clone(),
            subject: request.subject.cache_component(),
            audience: request.audience.clone(),
            scopes: scopes.clone(),
            config_version: integration.config.config_version,
        };

        if !request.force_refresh
            && let Some(lease) = self.cached_if_fresh(&key).await
        {
            return Ok(lease);
        }

        // Per-key refresh lock: dedups concurrent refreshes for the same key
        // without serializing unrelated keys.
        let lock = {
            let mut inflight = self.inflight.lock().await;
            inflight.entry(key.clone()).or_default().clone()
        };
        let _guard = lock.lock().await;

        // A concurrent caller may have refreshed while we waited.
        if !request.force_refresh
            && let Some(lease) = self.cached_if_fresh(&key).await
        {
            self.inflight.lock().await.remove(&key);
            return Ok(lease);
        }

        let normalized = TokenRequest {
            scopes,
            ..request.clone()
        };
        let result = integration.source.issue_token(&normalized).await;

        if let Ok(lease) = &result {
            let now = Utc::now();
            let mut cache = self.cache.lock().await;
            // Opportunistically drop dead leases so the cache stays bounded
            // by live keys.
            cache.retain(|_, cached| cached.expires_at.is_none_or(|expires_at| expires_at > now));
            cache.insert(key.clone(), lease.clone());
        }
        self.inflight.lock().await.remove(&key);
        result
    }

    /// Drop all cached leases for a subject on an integration. Call after
    /// revoking a grant so future requests re-consult the source.
    pub async fn invalidate(&self, integration_id: &str, subject: &TokenSubject) {
        let component = subject.cache_component();
        let mut cache = self.cache.lock().await;
        cache.retain(|key, _| !(key.integration_id == integration_id && key.subject == component));
    }

    async fn cached_if_fresh(&self, key: &CacheKey) -> Option<TokenLease> {
        let now = Utc::now();
        let mut cache = self.cache.lock().await;
        let fresh = match cache.get(key) {
            Some(lease) => lease
                .expires_at
                .is_none_or(|expires_at| expires_at > now + self.refresh_skew),
            None => return None,
        };
        if fresh {
            cache.get(key).cloned()
        } else {
            cache.remove(key);
            None
        }
    }
}
