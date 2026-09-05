//! Session management with JWT token generation and validation.

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_identity_core::{IdentityError, IdentityProvider, UserPermissions};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Sha384, Sha512};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Clock-skew leeway applied to the `iat` and `nbf` claims (S4): a token whose
/// `iat`/`nbf` lies further than this in the future is rejected.
pub const CLOCK_SKEW_LEEWAY_SECS: i64 = 60;

/// Minimum spacing between lazy expired-session sweeps triggered from
/// `begin_session`/`verify_session` (S1).
const LAZY_CLEANUP_INTERVAL_SECS: u64 = 60;

/// Default cap on concurrently tracked sessions per user (S5).
pub const DEFAULT_MAX_SESSIONS_PER_USER: usize = 32;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("JWT error: {0}")]
    JwtError(String),

    #[error("JWT token expired")]
    TokenExpired,

    #[error("Identity error: {0}")]
    IdentityError(#[from] IdentityError),

    #[error("Session not found")]
    SessionNotFound,

    #[error("Invalid session")]
    InvalidSession,

    #[error("Invalid session configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    /// Not-before. Optional; when present the token is rejected until then
    /// (with [`CLOCK_SKEW_LEEWAY_SECS`] leeway).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    pub jti: String,
    pub provider_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub permissions: HashSet<String>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JwtAlgorithm {
    #[serde(rename = "HS256")]
    HS256,
    #[serde(rename = "HS384")]
    HS384,
    #[serde(rename = "HS512")]
    HS512,
}

impl JwtAlgorithm {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "HS256" => Some(Self::HS256),
            "HS384" => Some(Self::HS384),
            "HS512" => Some(Self::HS512),
            _ => None,
        }
    }
}

/// Session/JWT configuration.
///
/// Permission semantics: the permissions granted at [`SessionService::begin_session`]
/// are frozen into the JWT and are **not** reloaded on verify. With the default
/// `enforce_active_sessions: true`, revoking a session (`end_session`) takes
/// effect immediately per-`jti`; otherwise grants are fixed for `jwt_ttl`
/// (default 24h).
///
/// `iss` and `aud` are **required by default** (S2): [`SessionConfig::new`] and
/// [`SessionService::new`] fail unless both are set (see
/// [`SessionConfig::with_issuer`] / [`SessionConfig::with_audience`]) or the
/// deployment explicitly opts out with [`SessionConfig::allow_unscoped_tokens`].
/// This keeps tokens minted for one service from being accepted by another that
/// shares the same secret.
///
/// When `enforce_active_sessions` is on, expired entries are only swept lazily
/// (at most once per minute, from `begin_session`/`verify_session`), so start
/// [`SessionService::start_cleanup_task`] to keep the store bounded during
/// traffic lulls.
#[derive(Clone)]
pub struct SessionConfig {
    pub jwt_secret: String,
    pub jwt_ttl: Duration,
    pub enforce_active_sessions: bool,
    pub algorithm: JwtAlgorithm,
    /// Expected token issuer. Encoded into new tokens and verified on
    /// `verify_session`; a mismatch is rejected. Required unless
    /// `require_iss_aud` is false.
    pub iss: Option<String>,
    /// Expected token audience. Encoded into new tokens and verified on
    /// `verify_session`; a token for a different `aud` is rejected. This is the
    /// cross-service confused-deputy guard. Required unless
    /// `require_iss_aud` is false.
    pub aud: Option<String>,
    /// When true (default), validation fails if `iss` or `aud` is `None`.
    /// Set to false via [`SessionConfig::allow_unscoped_tokens`] only for
    /// single-service deployments that never share a secret (S2).
    pub require_iss_aud: bool,
    /// Maximum concurrently tracked sessions per `sub` when
    /// `enforce_active_sessions` is on. Once reached, the oldest session (by
    /// `iat`) is evicted when a new one begins (S5). Default 32.
    pub max_sessions_per_user: usize,
}

/// Redacting `Debug` so `jwt_secret` never lands in logs.
impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConfig")
            .field("jwt_secret", &"[REDACTED]")
            .field("jwt_ttl", &self.jwt_ttl)
            .field("enforce_active_sessions", &self.enforce_active_sessions)
            .field("algorithm", &self.algorithm)
            .field("iss", &self.iss)
            .field("aud", &self.aud)
            .field("require_iss_aud", &self.require_iss_aud)
            .field("max_sessions_per_user", &self.max_sessions_per_user)
            .finish()
    }
}

impl SessionConfig {
    /// Build a config with an issuer and audience. Both are required by
    /// default; use [`SessionConfig::allow_unscoped_tokens`] on the result to
    /// opt out for single-service deployments.
    pub fn new(
        jwt_secret: impl Into<String>,
        issuer: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self, SessionError> {
        let config = Self {
            jwt_secret: jwt_secret.into(),
            jwt_ttl: Duration::hours(24),
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
            iss: Some(issuer.into()),
            aud: Some(audience.into()),
            require_iss_aud: true,
            max_sessions_per_user: DEFAULT_MAX_SESSIONS_PER_USER,
        };
        config.validate()?;
        Ok(config)
    }

    /// Build a config **without** an issuer/audience. Only valid for
    /// single-service deployments that never share `jwt_secret`; any token
    /// signed with the secret is accepted regardless of which service minted
    /// it. Equivalent to `new(..)` followed by [`allow_unscoped_tokens`].
    ///
    /// [`allow_unscoped_tokens`]: SessionConfig::allow_unscoped_tokens
    pub fn new_unscoped(jwt_secret: impl Into<String>) -> Result<Self, SessionError> {
        let config = Self {
            jwt_secret: jwt_secret.into(),
            jwt_ttl: Duration::hours(24),
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
            iss: None,
            aud: None,
            require_iss_aud: false,
            max_sessions_per_user: DEFAULT_MAX_SESSIONS_PER_USER,
        };
        config.validate()?;
        Ok(config)
    }

    /// Explicit opt-out from the `iss`/`aud` requirement (S2). Only for
    /// single-service deployments where the secret is never shared.
    pub fn allow_unscoped_tokens(mut self) -> Self {
        self.require_iss_aud = false;
        self
    }

    /// Cap concurrently tracked sessions per user (S5). Must be at least 1.
    pub fn with_max_sessions_per_user(mut self, max: usize) -> Self {
        self.max_sessions_per_user = max;
        self
    }

    /// Set the expected issuer (`iss`). Production services should set this.
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.iss = Some(issuer.into());
        self
    }

    /// Set the expected audience (`aud`). Production services should set this so
    /// a token minted for another service is rejected here.
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.aud = Some(audience.into());
        self
    }

    pub fn validate(&self) -> Result<(), SessionError> {
        validate_jwt_secret(&self.jwt_secret)?;

        if self.jwt_ttl <= Duration::zero() {
            return Err(SessionError::InvalidConfig(
                "jwt_ttl must be positive".to_string(),
            ));
        }

        if self.require_iss_aud && (self.iss.is_none() || self.aud.is_none()) {
            return Err(SessionError::InvalidConfig(
                "iss and aud must be set (use with_issuer/with_audience), or opt out \
                 explicitly with allow_unscoped_tokens() for single-service deployments"
                    .to_string(),
            ));
        }

        if self.max_sessions_per_user == 0 {
            return Err(SessionError::InvalidConfig(
                "max_sessions_per_user must be at least 1".to_string(),
            ));
        }

        Ok(())
    }
}

/// Minimum number of distinct byte values a secret must contain (S3).
///
/// Ten keeps `openssl rand -hex 24` (16 possible symbols) passing in
/// practice while still rejecting repeated-pattern strings.
const MIN_DISTINCT_SECRET_BYTES: usize = 10;
/// Longest permitted run of one repeated byte in a secret (S3).
const MAX_REPEATED_SECRET_BYTES: usize = 7;

/// Substrings (matched case-insensitively) that mark a secret as a
/// placeholder rather than random key material (S3).
const INSECURE_SECRET_SUBSTRINGS: &[&str] = &[
    "change-me",
    "changeme",
    "secret",
    "password",
    "example",
    "placeholder",
    "test-secret",
    "dev-secret",
    "insecure",
    "12345678",
    "abcdefgh",
    "your-secret",
];

fn validate_jwt_secret(secret: &str) -> Result<(), SessionError> {
    let trimmed = secret.trim();

    if trimmed.len() < 32 {
        return Err(SessionError::InvalidConfig(
            "jwt_secret must be at least 32 bytes".to_string(),
        ));
    }

    let lowered = trimmed.to_ascii_lowercase();
    if INSECURE_SECRET_SUBSTRINGS
        .iter()
        .any(|placeholder| lowered.contains(placeholder))
    {
        return Err(SessionError::InvalidConfig(
            "jwt_secret must not contain a placeholder value".to_string(),
        ));
    }

    let distinct: HashSet<u8> = trimmed.bytes().collect();
    if distinct.len() < MIN_DISTINCT_SECRET_BYTES {
        return Err(SessionError::InvalidConfig(format!(
            "jwt_secret must contain at least {MIN_DISTINCT_SECRET_BYTES} distinct byte values"
        )));
    }

    let mut run = 0usize;
    let mut prev = None;
    for byte in trimmed.bytes() {
        run = if prev == Some(byte) { run + 1 } else { 1 };
        if run > MAX_REPEATED_SECRET_BYTES {
            return Err(SessionError::InvalidConfig(format!(
                "jwt_secret must not repeat one byte more than {MAX_REPEATED_SECRET_BYTES} times in a row"
            )));
        }
        prev = Some(byte);
    }

    Ok(())
}

#[derive(Serialize)]
struct JwtHeader {
    typ: &'static str,
    alg: JwtAlgorithm,
}

#[derive(Deserialize)]
struct DecodedJwtHeader {
    alg: JwtAlgorithm,
}

fn jwt_error(message: impl Into<String>) -> SessionError {
    SessionError::JwtError(message.into())
}

fn encode_jwt<T: Serialize>(
    claims: &T,
    secret: &str,
    algorithm: JwtAlgorithm,
) -> Result<String, SessionError> {
    let header = JwtHeader {
        typ: "JWT",
        alg: algorithm,
    };
    let header = serde_json::to_vec(&header)
        .map_err(|err| jwt_error(format!("failed to encode JWT header: {err}")))?;
    let claims = serde_json::to_vec(claims)
        .map_err(|err| jwt_error(format!("failed to encode JWT claims: {err}")))?;

    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(claims)
    );
    let signature = sign_jwt(&signing_input, secret.as_bytes(), algorithm)?;

    Ok(format!(
        "{}.{}",
        signing_input,
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn decode_jwt<T: DeserializeOwned>(
    token: &str,
    secret: &str,
    expected_algorithm: JwtAlgorithm,
) -> Result<T, SessionError> {
    let mut parts = token.split('.');
    let encoded_header = parts
        .next()
        .ok_or_else(|| jwt_error("missing JWT header"))?;
    let encoded_claims = parts
        .next()
        .ok_or_else(|| jwt_error("missing JWT claims"))?;
    let encoded_signature = parts
        .next()
        .ok_or_else(|| jwt_error("missing JWT signature"))?;

    if parts.next().is_some() {
        return Err(jwt_error("JWT has too many segments"));
    }

    let header = URL_SAFE_NO_PAD
        .decode(encoded_header)
        .map_err(|err| jwt_error(format!("invalid JWT header encoding: {err}")))?;
    let header: DecodedJwtHeader = serde_json::from_slice(&header)
        .map_err(|err| jwt_error(format!("invalid JWT header: {err}")))?;

    if header.alg != expected_algorithm {
        return Err(jwt_error("unexpected JWT algorithm"));
    }

    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|err| jwt_error(format!("invalid JWT signature encoding: {err}")))?;
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    verify_jwt_signature(
        &signing_input,
        secret.as_bytes(),
        expected_algorithm,
        &signature,
    )?;

    let claims = URL_SAFE_NO_PAD
        .decode(encoded_claims)
        .map_err(|err| jwt_error(format!("invalid JWT claims encoding: {err}")))?;
    serde_json::from_slice(&claims).map_err(|err| jwt_error(format!("invalid JWT claims: {err}")))
}

fn sign_jwt(
    signing_input: &str,
    secret: &[u8],
    algorithm: JwtAlgorithm,
) -> Result<Vec<u8>, SessionError> {
    match algorithm {
        JwtAlgorithm::HS256 => {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            Ok(mac.finalize().into_bytes().to_vec())
        }
        JwtAlgorithm::HS384 => {
            let mut mac = Hmac::<Sha384>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            Ok(mac.finalize().into_bytes().to_vec())
        }
        JwtAlgorithm::HS512 => {
            let mut mac = Hmac::<Sha512>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            Ok(mac.finalize().into_bytes().to_vec())
        }
    }
}

fn verify_jwt_signature(
    signing_input: &str,
    secret: &[u8],
    algorithm: JwtAlgorithm,
    signature: &[u8],
) -> Result<(), SessionError> {
    match algorithm {
        JwtAlgorithm::HS256 => {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            mac.verify_slice(signature)
                .map_err(|_| jwt_error("invalid JWT signature"))
        }
        JwtAlgorithm::HS384 => {
            let mut mac = Hmac::<Sha384>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            mac.verify_slice(signature)
                .map_err(|_| jwt_error("invalid JWT signature"))
        }
        JwtAlgorithm::HS512 => {
            let mut mac = Hmac::<Sha512>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            mac.verify_slice(signature)
                .map_err(|_| jwt_error("invalid JWT signature"))
        }
    }
}

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

#[derive(Clone)]
pub struct JwtAuthProvider {
    session_service: Arc<SessionService>,
}

impl JwtAuthProvider {
    pub fn new(session_service: Arc<SessionService>) -> Self {
        Self { session_service }
    }
}

#[async_trait]
impl AuthProvider for JwtAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let claims =
                self.session_service
                    .verify_session(&token)
                    .await
                    .map_err(|e| match e {
                        SessionError::TokenExpired => AuthError::TokenExpired,
                        _ => AuthError::InvalidToken,
                    })?;

            Ok(AuthenticatedUser {
                user_id: claims.sub,
                permissions: claims.permissions,
                metadata: claims.metadata,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ras_identity_core::StaticPermissions;
    use ras_identity_local::LocalUserProvider;

    const TEST_SECRET: &str = "f27929932dc7269b950dc1e5c064111f105c67036a7386ca";

    fn test_config() -> SessionConfig {
        SessionConfig::new(TEST_SECRET, "ras-test", "ras-test").unwrap()
    }

    async fn local_provider_with_user(username: &str, password: &str) -> LocalUserProvider {
        let provider = LocalUserProvider::new();
        provider
            .add_user(
                username.to_string(),
                password.to_string(),
                Some(format!("{username}@example.com")),
                Some(format!("{username} User")),
            )
            .await
            .unwrap();
        provider
    }

    #[tokio::test]
    async fn test_session_lifecycle() {
        let config = test_config();
        let session_service = SessionService::new(config).unwrap();

        let local_provider = LocalUserProvider::new();
        local_provider
            .add_user(
                "testuser".to_string(),
                "password123".to_string(),
                Some("test@example.com".to_string()),
                Some("Test User".to_string()),
            )
            .await
            .unwrap();

        session_service
            .register_provider(Box::new(local_provider))
            .await;

        let auth_payload = serde_json::json!({
            "username": "testuser",
            "password": "password123"
        });

        let token = session_service
            .begin_session("local", auth_payload)
            .await
            .unwrap();

        let claims = session_service.verify_session(&token).await.unwrap();
        assert_eq!(claims.sub, "testuser");
        assert_eq!(claims.provider_id, "local");
        assert!(claims.permissions.is_empty());

        session_service.end_session(&claims.jti).await;

        assert!(session_service.verify_session(&token).await.is_err());
    }

    #[tokio::test]
    async fn test_session_with_permissions() {
        let config = test_config();
        let permissions_provider = Arc::new(StaticPermissions::new(vec![
            "read".to_string(),
            "write".to_string(),
        ]));
        let session_service = SessionService::new(config)
            .unwrap()
            .with_permissions(permissions_provider);

        let local_provider = LocalUserProvider::new();
        local_provider
            .add_user(
                "admin".to_string(),
                "admin123".to_string(),
                Some("admin@example.com".to_string()),
                Some("Admin User".to_string()),
            )
            .await
            .unwrap();

        session_service
            .register_provider(Box::new(local_provider))
            .await;

        let auth_payload = serde_json::json!({
            "username": "admin",
            "password": "admin123"
        });

        let token = session_service
            .begin_session("local", auth_payload)
            .await
            .unwrap();

        let claims = session_service.verify_session(&token).await.unwrap();
        assert_eq!(claims.sub, "admin");
        assert_eq!(claims.permissions.len(), 2);
        assert!(claims.permissions.contains("read"));
        assert!(claims.permissions.contains("write"));
    }

    #[test]
    fn test_rejects_placeholder_secret() {
        let result = SessionConfig::new("change-me-in-production", "i", "a");
        assert!(matches!(result, Err(SessionError::InvalidConfig(_))));
    }

    #[test]
    fn debug_redacts_jwt_secret() {
        let config = test_config();
        let debug = format!("{config:?}");
        assert!(!debug.contains(TEST_SECRET));
        assert!(debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn token_for_one_audience_is_rejected_by_another_service() {
        // Two services share a secret but configure different audiences.
        let service_a = SessionService::new(test_config().with_audience("svc-a")).unwrap();
        let local = LocalUserProvider::new();
        local
            .add_user("u".to_string(), "password123".to_string(), None, None)
            .await
            .unwrap();
        service_a.register_provider(Box::new(local)).await;

        let token = service_a
            .begin_session(
                "local",
                serde_json::json!({"username": "u", "password": "password123"}),
            )
            .await
            .unwrap();

        // A service configured for a different audience rejects the token
        // (the aud check runs before the active-session check).
        let service_b = SessionService::new(test_config().with_audience("svc-b")).unwrap();
        assert!(matches!(
            service_b.verify_session(&token).await,
            Err(SessionError::InvalidSession)
        ));

        // The issuing service (correct audience) still accepts it.
        assert!(service_a.verify_session(&token).await.is_ok());
    }

    #[tokio::test]
    async fn permissions_are_frozen_into_the_token_snapshot() {
        // Verification returns the permissions captured at session creation.
        let permissions_provider = Arc::new(StaticPermissions::new(vec!["read".to_string()]));
        let service = SessionService::new(test_config())
            .unwrap()
            .with_permissions(permissions_provider);
        let local = LocalUserProvider::new();
        local
            .add_user("u".to_string(), "password123".to_string(), None, None)
            .await
            .unwrap();
        service.register_provider(Box::new(local)).await;

        let token = service
            .begin_session(
                "local",
                serde_json::json!({"username": "u", "password": "password123"}),
            )
            .await
            .unwrap();
        let claims = service.verify_session(&token).await.unwrap();
        assert_eq!(claims.permissions.len(), 1);
        assert!(claims.permissions.contains("read"));
    }

    #[tokio::test]
    async fn test_cleanup_expired_sessions() {
        let config = test_config();
        let service = SessionService::new(config).unwrap();

        {
            let mut sessions = service.active_sessions.write().await;
            sessions.insert(
                "expired".to_string(),
                JwtClaims {
                    sub: "user".to_string(),
                    exp: Utc::now().timestamp() - 1,
                    iat: Utc::now().timestamp() - 10,
                    nbf: None,
                    jti: "expired".to_string(),
                    provider_id: "local".to_string(),
                    email: None,
                    display_name: None,
                    permissions: HashSet::new(),
                    metadata: None,
                    iss: None,
                    aud: None,
                },
            );
        }

        assert_eq!(service.cleanup_expired_sessions().await, 1);
    }

    #[tokio::test]
    async fn test_malformed_exp_claim_is_rejected() {
        let config = test_config();
        let service = SessionService::new(config).unwrap();

        let token = encode_jwt(
            &serde_json::json!({
                "sub": "user",
                "exp": "not-a-number",
                "iat": Utc::now().timestamp(),
                "jti": "malformed",
                "provider_id": "local",
                "permissions": [],
            }),
            TEST_SECRET,
            JwtAlgorithm::HS256,
        )
        .unwrap();

        assert!(service.verify_session(&token).await.is_err());
    }

    #[test]
    fn session_config_rejects_non_positive_ttl() {
        let mut config = test_config();
        config.jwt_ttl = Duration::zero();

        let error = config.validate().expect_err("zero ttl should fail");

        assert!(
            matches!(error, SessionError::InvalidConfig(message) if message == "jwt_ttl must be positive")
        );
    }

    #[tokio::test]
    async fn begin_session_reports_unknown_identity_provider() {
        let config = test_config();
        let service = SessionService::new(config).unwrap();

        let error = service
            .begin_session("missing", serde_json::json!({}))
            .await
            .expect_err("unknown provider should fail");

        assert!(
            matches!(error, SessionError::IdentityError(IdentityError::ProviderNotFound(provider)) if provider == "missing")
        );
    }

    #[tokio::test]
    async fn verify_session_can_skip_active_session_store_when_configured() {
        let mut config = test_config();
        config.enforce_active_sessions = false;
        let service = SessionService::new(config).unwrap();
        service
            .register_provider(Box::new(
                local_provider_with_user("stateless", "password123").await,
            ))
            .await;

        let token = service
            .begin_session(
                "local",
                serde_json::json!({
                    "username": "stateless",
                    "password": "password123"
                }),
            )
            .await
            .unwrap();

        let claims = service.verify_session(&token).await.unwrap();
        assert_eq!(claims.sub, "stateless");
        assert!(
            service
                .active_sessions
                .read()
                .await
                .get(&claims.jti)
                .is_none()
        );
    }

    #[tokio::test]
    async fn jwt_auth_provider_maps_verified_claims_to_authenticated_user() {
        let config = test_config();
        let permissions = Arc::new(StaticPermissions::new(vec!["chat:read".to_string()]));
        let service = Arc::new(
            SessionService::new(config)
                .unwrap()
                .with_permissions(permissions),
        );
        service
            .register_provider(Box::new(
                local_provider_with_user("alice", "password123").await,
            ))
            .await;

        let token = service
            .begin_session(
                "local",
                serde_json::json!({
                    "username": "alice",
                    "password": "password123"
                }),
            )
            .await
            .unwrap();
        let auth_provider = JwtAuthProvider::new(service);

        let user = auth_provider.authenticate(token).await.unwrap();

        assert_eq!(user.user_id, "alice");
        assert!(user.permissions.contains("chat:read"));
        assert!(user.metadata.is_none());
    }

    #[tokio::test]
    async fn cleanup_task_prunes_expired_sessions_in_background() {
        let config = test_config();
        let service = std::sync::Arc::new(SessionService::new(config).unwrap());

        // Plant an already-expired session directly in the store.
        let now = chrono::Utc::now().timestamp();
        service.active_sessions.write().await.insert(
            "expired-jti".to_string(),
            JwtClaims {
                sub: "alice".to_string(),
                exp: now - 10,
                iat: now - 20,
                nbf: None,
                jti: "expired-jti".to_string(),
                provider_id: "local".to_string(),
                email: None,
                display_name: None,
                permissions: HashSet::new(),
                metadata: None,
                iss: None,
                aud: None,
            },
        );
        assert_eq!(service.active_session_count().await, 1);

        let handle = service.start_cleanup_task(std::time::Duration::from_millis(20));

        // The sweeper prunes the expired session without any begin/verify call.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while service.active_session_count().await != 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cleanup task prunes expired sessions");

        handle.abort();
    }

    // ---- S1 -------------------------------------------------------------

    fn planted_claims(sub: &str, jti: &str, iat: i64, exp: i64) -> JwtClaims {
        JwtClaims {
            sub: sub.to_string(),
            exp,
            iat,
            nbf: None,
            jti: jti.to_string(),
            provider_id: "local".to_string(),
            email: None,
            display_name: None,
            permissions: HashSet::new(),
            metadata: None,
            iss: Some("ras-test".to_string()),
            aud: Some("ras-test".to_string()),
        }
    }

    #[tokio::test]
    async fn s1_verify_session_does_not_take_write_lock() {
        let service = SessionService::new(test_config()).unwrap();
        service
            .register_provider(Box::new(
                local_provider_with_user("alice", "password123").await,
            ))
            .await;
        let token = service
            .begin_session(
                "local",
                serde_json::json!({"username": "alice", "password": "password123"}),
            )
            .await
            .unwrap();
        // Warm the lazy sweep so the next verify is definitely not "due".
        service.verify_session(&token).await.unwrap();

        // Hold a read guard on the store: a verify that tried to take the
        // write lock (the old inline cleanup) would deadlock here.
        let _read_guard = service.active_sessions.read().await;
        let verified = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            service.verify_session(&token),
        )
        .await
        .expect("verify_session must not block on the write lock");
        assert!(verified.is_ok());
    }

    #[tokio::test]
    async fn s1_lazy_cleanup_runs_at_most_once_per_interval() {
        let service = SessionService::new(test_config()).unwrap();
        service
            .register_provider(Box::new(
                local_provider_with_user("alice", "password123").await,
            ))
            .await;
        let token = service
            .begin_session(
                "local",
                serde_json::json!({"username": "alice", "password": "password123"}),
            )
            .await
            .unwrap();
        assert!(
            service.next_lazy_cleanup.load(Ordering::Relaxed) >= LAZY_CLEANUP_INTERVAL_SECS,
            "first call performs the lazy sweep and schedules the next one"
        );

        // Plant an expired entry after the sweep; a verify within the interval
        // must leave it alone (no sweep), proving the once-per-interval gate.
        let now = Utc::now().timestamp();
        service.active_sessions.write().await.insert(
            "expired".into(),
            planted_claims("bob", "expired", now - 20, now - 10),
        );
        service.verify_session(&token).await.unwrap();
        assert_eq!(service.active_session_count().await, 2);

        // Pretend the interval has elapsed: the next call sweeps.
        service.next_lazy_cleanup.store(0, Ordering::Relaxed);
        service.verify_session(&token).await.unwrap();
        assert_eq!(service.active_session_count().await, 1);
    }

    // ---- S2 -------------------------------------------------------------

    #[test]
    fn s2_iss_and_aud_are_required_by_default() {
        let mut config = test_config();
        config.aud = None;
        assert!(matches!(
            config.validate(),
            Err(SessionError::InvalidConfig(_))
        ));
        assert!(matches!(
            SessionService::new(config.clone()),
            Err(SessionError::InvalidConfig(_))
        ));

        let mut config = test_config();
        config.iss = None;
        assert!(matches!(
            config.validate(),
            Err(SessionError::InvalidConfig(_))
        ));

        // Struct-literal construction goes through the same check.
        let literal = SessionConfig {
            jwt_secret: TEST_SECRET.to_string(),
            jwt_ttl: Duration::hours(1),
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
            iss: None,
            aud: None,
            require_iss_aud: true,
            max_sessions_per_user: DEFAULT_MAX_SESSIONS_PER_USER,
        };
        assert!(matches!(
            SessionService::new(literal),
            Err(SessionError::InvalidConfig(_))
        ));
    }

    #[tokio::test]
    async fn s2_allow_unscoped_tokens_is_an_explicit_opt_out() {
        let config = SessionConfig::new_unscoped(TEST_SECRET).unwrap();
        assert!(!config.require_iss_aud);
        assert!(config.iss.is_none() && config.aud.is_none());

        let mut config = test_config();
        config.iss = None;
        config.aud = None;
        let config = config.allow_unscoped_tokens();
        let service = SessionService::new(config).unwrap();
        service
            .register_provider(Box::new(
                local_provider_with_user("alice", "password123").await,
            ))
            .await;
        let token = service
            .begin_session(
                "local",
                serde_json::json!({"username": "alice", "password": "password123"}),
            )
            .await
            .unwrap();
        let claims = service.verify_session(&token).await.unwrap();
        assert!(claims.iss.is_none() && claims.aud.is_none());
    }

    // ---- S3 -------------------------------------------------------------

    #[test]
    fn s3_secret_entropy_and_placeholder_checks() {
        // Placeholder substrings, case-insensitive, anywhere in the value.
        for bad in [
            "x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7g9h1j3-SECRET",
            "MyPassword-x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7",
            "x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7-Example-value",
            "your-secret-x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7g9h",
            "x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7g9-ChangeMe",
            "x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7g9h12345678",
            "x9k2m4p7q1w5e8r3t6y0u2i4o6p8a1s3d5f7g9h-insecure",
        ] {
            let err = validate_jwt_secret(bad).expect_err(bad);
            assert!(
                matches!(err, SessionError::InvalidConfig(ref m) if m.contains("placeholder")),
                "{bad}: {err}"
            );
        }

        // Fewer than MIN_DISTINCT_SECRET_BYTES distinct byte values.
        let low_entropy = "abababababababababababababababababababab";
        let err = validate_jwt_secret(low_entropy).unwrap_err();
        assert!(matches!(err, SessionError::InvalidConfig(ref m) if m.contains("distinct")));

        // A run of 8+ identical bytes, even with enough distinct bytes overall.
        let run = "9876543210klmnopqrstuvwxyzZZZZZZZZ";
        let err = validate_jwt_secret(run).unwrap_err();
        assert!(matches!(err, SessionError::InvalidConfig(ref m) if m.contains("in a row")));

        // A 7-byte run is still fine, and random-looking hex is accepted.
        validate_jwt_secret("9876543210klmnopqrstuvwxyzZZZZZZZ").unwrap();
        validate_jwt_secret(TEST_SECRET).unwrap();
        validate_jwt_secret("a1a61a06cb81a0908d140f62f740f3f1e1f3a5df67c5cba5").unwrap();
    }

    // ---- S4 -------------------------------------------------------------

    fn signed_token(service: &SessionService, claims: &JwtClaims) -> String {
        encode_jwt(claims, &service.config.jwt_secret, service.config.algorithm).unwrap()
    }

    #[tokio::test]
    async fn s4_rejects_iat_and_nbf_in_the_future() {
        let mut config = test_config();
        config.enforce_active_sessions = false;
        let service = SessionService::new(config).unwrap();
        let now = Utc::now().timestamp();

        // iat well in the future -> rejected.
        let future_iat =
            planted_claims("alice", "j1", now + CLOCK_SKEW_LEEWAY_SECS + 30, now + 3600);
        assert!(matches!(
            service
                .verify_session(&signed_token(&service, &future_iat))
                .await,
            Err(SessionError::InvalidSession)
        ));

        // iat within leeway -> accepted.
        let skewed_iat =
            planted_claims("alice", "j2", now + CLOCK_SKEW_LEEWAY_SECS - 5, now + 3600);
        service
            .verify_session(&signed_token(&service, &skewed_iat))
            .await
            .unwrap();

        // nbf well in the future -> rejected.
        let mut future_nbf = planted_claims("alice", "j3", now, now + 3600);
        future_nbf.nbf = Some(now + CLOCK_SKEW_LEEWAY_SECS + 30);
        assert!(matches!(
            service
                .verify_session(&signed_token(&service, &future_nbf))
                .await,
            Err(SessionError::InvalidSession)
        ));

        // nbf within leeway -> accepted; absent nbf (older tokens) -> accepted.
        let mut skewed_nbf = planted_claims("alice", "j4", now, now + 3600);
        skewed_nbf.nbf = Some(now + CLOCK_SKEW_LEEWAY_SECS - 5);
        service
            .verify_session(&signed_token(&service, &skewed_nbf))
            .await
            .unwrap();
        let token_without_nbf = encode_jwt(
            &serde_json::json!({
                "sub": "alice", "exp": now + 3600, "iat": now, "jti": "j5",
                "provider_id": "local", "permissions": [],
                "iss": "ras-test", "aud": "ras-test",
            }),
            TEST_SECRET,
            JwtAlgorithm::HS256,
        )
        .unwrap();
        let claims = service.verify_session(&token_without_nbf).await.unwrap();
        assert!(claims.nbf.is_none());
    }

    // ---- S5 -------------------------------------------------------------

    #[tokio::test]
    async fn s5_evicts_oldest_session_when_user_exceeds_cap() {
        let config = test_config().with_max_sessions_per_user(2);
        let service = SessionService::new(config).unwrap();
        service
            .register_provider(Box::new(
                local_provider_with_user("alice", "password123").await,
            ))
            .await;
        // Another user's sessions must be unaffected by alice's cap.
        let now = Utc::now().timestamp();
        service.active_sessions.write().await.insert(
            "bob-1".into(),
            planted_claims("bob", "bob-1", now - 100, now + 3600),
        );

        let login = || {
            service.begin_session(
                "local",
                serde_json::json!({"username": "alice", "password": "password123"}),
            )
        };
        let t1 = login().await.unwrap();
        let j1 = service.verify_session(&t1).await.unwrap().jti;
        // Make t1 unambiguously the oldest by iat regardless of clock granularity.
        service
            .active_sessions
            .write()
            .await
            .get_mut(&j1)
            .unwrap()
            .iat = now - 50;
        let t2 = login().await.unwrap();
        let t3 = login().await.unwrap();

        assert!(
            matches!(
                service.verify_session(&t1).await,
                Err(SessionError::SessionNotFound)
            ),
            "oldest session is evicted once the cap is reached"
        );
        assert!(service.verify_session(&t2).await.is_ok());
        assert!(service.verify_session(&t3).await.is_ok());
        assert_eq!(
            service.active_session_count().await,
            3,
            "2 for alice + 1 for bob"
        );
        assert!(service.active_sessions.read().await.contains_key("bob-1"));
    }

    #[test]
    fn s5_max_sessions_per_user_must_be_positive() {
        let config = test_config().with_max_sessions_per_user(0);
        assert!(matches!(
            config.validate(),
            Err(SessionError::InvalidConfig(_))
        ));
    }
}
