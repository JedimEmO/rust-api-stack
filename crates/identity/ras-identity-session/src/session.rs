use crate::jwt::{decode_jwt, encode_jwt};
use crate::{CLOCK_SKEW_LEEWAY_SECS, JwtClaims, SessionConfig, SessionError};
use chrono::Utc;
use ras_identity_core::{IdentityError, IdentityProvider, UserPermissions};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Minimum spacing between lazy expired-session sweeps triggered from
/// `begin_session`/`verify_session` (S1).
const LAZY_CLEANUP_INTERVAL_SECS: u64 = 60;

pub struct SessionService {
    config: SessionConfig,
    providers: Arc<RwLock<HashMap<String, Box<dyn IdentityProvider>>>>,
    /// Keyed by `jti`.
    active_sessions: Arc<RwLock<HashMap<String, JwtClaims>>>,
    permissions_provider: Option<Arc<dyn UserPermissions>>,
    /// Reference point for `next_lazy_cleanup`.
    created_at: Instant,
    /// Seconds since `created_at` at which the next lazy sweep is due (S1).
    next_lazy_cleanup: AtomicU64,
}
impl SessionService {
    pub fn new(config: SessionConfig) -> Result<Self, SessionError> {
        config.validate()?;
        Ok(Self {
            config,
            providers: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            permissions_provider: None,
            created_at: Instant::now(),
            next_lazy_cleanup: AtomicU64::new(0),
        })
    }

    /// Lazy fallback sweep (S1): prunes expired sessions at most once per
    /// [`LAZY_CLEANUP_INTERVAL_SECS`], so deployments that never start
    /// [`start_cleanup_task`](Self::start_cleanup_task) still get bounded
    /// growth without taking the write lock on every request.
    async fn maybe_lazy_cleanup(&self) {
        if !self.config.enforce_active_sessions {
            return;
        }
        let now = self.created_at.elapsed().as_secs();
        let next = self.next_lazy_cleanup.load(Ordering::Relaxed);
        // The first call sweeps (next == 0), then at most once per interval.
        // compare_exchange ensures only one of several concurrent callers
        // performs the sweep; the losers see the bumped deadline and skip.
        if now >= next
            && self
                .next_lazy_cleanup
                .compare_exchange(
                    next,
                    now + LAZY_CLEANUP_INTERVAL_SECS,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
        {
            self.cleanup_expired_sessions().await;
        }
    }

    pub fn with_permissions(mut self, provider: Arc<dyn UserPermissions>) -> Self {
        self.permissions_provider = Some(provider);
        self
    }

    pub fn set_permissions_provider(&mut self, provider: Arc<dyn UserPermissions>) {
        self.permissions_provider = Some(provider);
    }

    pub async fn register_provider(&self, provider: Box<dyn IdentityProvider>) {
        let mut providers = self.providers.write().await;
        providers.insert(provider.provider_id().to_string(), provider);
    }

    pub async fn begin_session(
        &self,
        provider_id: &str,
        auth_payload: serde_json::Value,
    ) -> Result<String, SessionError> {
        self.maybe_lazy_cleanup().await;

        let providers = self.providers.read().await;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| IdentityError::ProviderNotFound(provider_id.to_string()))?;

        let identity = provider.verify(auth_payload).await?;

        let now = Utc::now();
        let exp = now + self.config.jwt_ttl;
        let jti = Uuid::new_v4().to_string();

        let permissions = if let Some(ref perm_provider) = self.permissions_provider {
            perm_provider.get_permissions(&identity).await?
        } else {
            Vec::new()
        };

        let claims = JwtClaims {
            sub: identity.subject.clone(),
            exp: exp.timestamp(),
            iat: now.timestamp(),
            nbf: None,
            jti: jti.clone(),
            provider_id: identity.provider_id.clone(),
            email: identity.email.clone(),
            display_name: identity.display_name.clone(),
            permissions: permissions.into_iter().collect(),
            metadata: identity.metadata,
            iss: self.config.iss.clone(),
            aud: self.config.aud.clone(),
        };

        if self.config.enforce_active_sessions {
            let mut sessions = self.active_sessions.write().await;
            // Per-user cap (S5): evict the oldest sessions (by iat) so this
            // user never holds more than `max_sessions_per_user` entries.
            let max = self.config.max_sessions_per_user;
            let mut owned: Vec<(i64, String)> = sessions
                .iter()
                .filter(|(_, c)| c.sub == claims.sub)
                .map(|(jti, c)| (c.iat, jti.clone()))
                .collect();
            if owned.len() >= max {
                owned.sort();
                let surplus = owned.len() + 1 - max;
                for (_, old_jti) in owned.into_iter().take(surplus) {
                    sessions.remove(&old_jti);
                }
            }
            sessions.insert(jti.clone(), claims.clone());
        }

        let token = encode_jwt(&claims, &self.config.jwt_secret, self.config.algorithm)?;

        Ok(token)
    }

    pub async fn verify_session(&self, token: &str) -> Result<JwtClaims, SessionError> {
        self.maybe_lazy_cleanup().await;

        let claims =
            decode_jwt::<JwtClaims>(token, &self.config.jwt_secret, self.config.algorithm)?;

        let now = Utc::now().timestamp();
        if claims.exp <= now {
            return Err(SessionError::TokenExpired);
        }

        // Time-validity guards (S4): a token issued or valid only in the
        // future (beyond clock-skew leeway) is not accepted.
        if claims.iat > now + CLOCK_SKEW_LEEWAY_SECS {
            return Err(SessionError::InvalidSession);
        }
        if let Some(nbf) = claims.nbf
            && nbf > now + CLOCK_SKEW_LEEWAY_SECS
        {
            return Err(SessionError::InvalidSession);
        }

        // Cross-service confused-deputy guard: reject tokens minted for a
        // different issuer/audience when this service configures them.
        if let Some(expected_iss) = &self.config.iss
            && claims.iss.as_deref() != Some(expected_iss.as_str())
        {
            return Err(SessionError::InvalidSession);
        }
        if let Some(expected_aud) = &self.config.aud
            && claims.aud.as_deref() != Some(expected_aud.as_str())
        {
            return Err(SessionError::InvalidSession);
        }

        if self.config.enforce_active_sessions {
            let sessions = self.active_sessions.read().await;
            if !sessions.contains_key(&claims.jti) {
                return Err(SessionError::SessionNotFound);
            }
        }

        Ok(claims)
    }

    /// Number of sessions currently held in the in-memory store
    /// (only populated when `enforce_active_sessions` is on).
    pub async fn active_session_count(&self) -> usize {
        self.active_sessions.read().await.len()
    }

    /// Spawn a background task pruning expired sessions every `interval`.
    ///
    /// Start this whenever `enforce_active_sessions` is on. Without it,
    /// expired sessions are only pruned lazily (at most once a minute, and
    /// only when begin_session/verify_session run), so a traffic lull leaves
    /// them in memory until the next request. The task holds only a weak
    /// reference and stops when the service is dropped (or when the returned
    /// handle is aborted).
    pub fn start_cleanup_task(
        self: &std::sync::Arc<Self>,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        let service = std::sync::Arc::downgrade(self);
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                timer.tick().await;
                let Some(service) = service.upgrade() else {
                    break;
                };
                service.cleanup_expired_sessions().await;
            }
        })
    }

    pub async fn end_session(&self, jti: &str) -> Option<JwtClaims> {
        let mut sessions = self.active_sessions.write().await;
        sessions.remove(jti)
    }

    pub async fn cleanup_expired_sessions(&self) -> usize {
        let now = Utc::now().timestamp();
        let mut sessions = self.active_sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, claims| claims.exp > now);
        before - sessions.len()
    }
}

#[cfg(test)]
mod tests;
