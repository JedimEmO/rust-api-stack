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
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

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
/// (default 24h). Set `iss`/`aud` in production so tokens minted for one service
/// are not accepted by another sharing the same secret.
#[derive(Clone)]
pub struct SessionConfig {
    pub jwt_secret: String,
    pub jwt_ttl: Duration,
    pub enforce_active_sessions: bool,
    pub algorithm: JwtAlgorithm,
    /// Expected token issuer. When `Some`, it is encoded into new tokens and
    /// verified on `verify_session`; a mismatch is rejected.
    pub iss: Option<String>,
    /// Expected token audience. When `Some`, it is encoded into new tokens and
    /// verified on `verify_session`; a token for a different `aud` is rejected.
    /// This is the cross-service confused-deputy guard (M3).
    pub aud: Option<String>,
}

/// Redacting `Debug` so `jwt_secret` never lands in logs (L1).
impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConfig")
            .field("jwt_secret", &"[REDACTED]")
            .field("jwt_ttl", &self.jwt_ttl)
            .field("enforce_active_sessions", &self.enforce_active_sessions)
            .field("algorithm", &self.algorithm)
            .field("iss", &self.iss)
            .field("aud", &self.aud)
            .finish()
    }
}

impl SessionConfig {
    pub fn new(jwt_secret: impl Into<String>) -> Result<Self, SessionError> {
        let config = Self {
            jwt_secret: jwt_secret.into(),
            jwt_ttl: Duration::hours(24),
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
            iss: None,
            aud: None,
        };
        config.validate()?;
        Ok(config)
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

        Ok(())
    }
}

fn validate_jwt_secret(secret: &str) -> Result<(), SessionError> {
    let trimmed = secret.trim();
    let insecure_placeholders = [
        "change-me-in-production",
        "change-me",
        "secret",
        "test-secret",
        "test-secret-key",
    ];

    if trimmed.len() < 32 {
        return Err(SessionError::InvalidConfig(
            "jwt_secret must be at least 32 bytes".to_string(),
        ));
    }

    if insecure_placeholders
        .iter()
        .any(|placeholder| trimmed.eq_ignore_ascii_case(placeholder))
    {
        return Err(SessionError::InvalidConfig(
            "jwt_secret must not use a placeholder value".to_string(),
        ));
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
    active_sessions: Arc<RwLock<HashMap<String, JwtClaims>>>,
    permissions_provider: Option<Arc<dyn UserPermissions>>,
}
impl SessionService {
    pub fn new(config: SessionConfig) -> Result<Self, SessionError> {
        config.validate()?;
        Ok(Self {
            config,
            providers: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            permissions_provider: None,
        })
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
        if self.config.enforce_active_sessions {
            self.cleanup_expired_sessions().await;
        }

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
            sessions.insert(jti.clone(), claims.clone());
        }

        let token = encode_jwt(&claims, &self.config.jwt_secret, self.config.algorithm)?;

        Ok(token)
    }

    pub async fn verify_session(&self, token: &str) -> Result<JwtClaims, SessionError> {
        if self.config.enforce_active_sessions {
            self.cleanup_expired_sessions().await;
        }

        let claims =
            decode_jwt::<JwtClaims>(token, &self.config.jwt_secret, self.config.algorithm)?;

        if claims.exp <= Utc::now().timestamp() {
            return Err(SessionError::TokenExpired);
        }

        // Cross-service confused-deputy guard: reject tokens minted for a
        // different issuer/audience when this service configures them (M3).
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
    /// Expired sessions are otherwise only pruned opportunistically when
    /// begin_session/verify_session run, so a traffic lull leaves them in
    /// memory indefinitely. The task holds only a weak reference and stops
    /// when the service is dropped (or when the returned handle is aborted).
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

    const TEST_SECRET: &str = "test-secret-that-is-long-enough-for-hs256";

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
        let config = SessionConfig::new(TEST_SECRET).unwrap();
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
        let config = SessionConfig::new(TEST_SECRET).unwrap();
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
        let result = SessionConfig::new("change-me-in-production");
        assert!(matches!(result, Err(SessionError::InvalidConfig(_))));
    }

    #[test]
    fn debug_redacts_jwt_secret() {
        let config = SessionConfig::new(TEST_SECRET).unwrap();
        let debug = format!("{config:?}");
        assert!(!debug.contains(TEST_SECRET));
        assert!(debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn token_for_one_audience_is_rejected_by_another_service() {
        // Two services share a secret but configure different audiences (M3).
        let service_a = SessionService::new(
            SessionConfig::new(TEST_SECRET).unwrap().with_audience("svc-a"),
        )
        .unwrap();
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
        let service_b = SessionService::new(
            SessionConfig::new(TEST_SECRET).unwrap().with_audience("svc-b"),
        )
        .unwrap();
        assert!(matches!(
            service_b.verify_session(&token).await,
            Err(SessionError::InvalidSession)
        ));

        // The issuing service (correct audience) still accepts it.
        assert!(service_a.verify_session(&token).await.is_ok());
    }

    #[tokio::test]
    async fn permissions_are_frozen_into_the_token_snapshot() {
        // Names the documented freeze behavior (M3): the permission set is
        // copied into the JWT at begin_session and returned verbatim on verify;
        // it is not reloaded. If a reload is ever added, this test must change.
        let permissions_provider =
            Arc::new(StaticPermissions::new(vec!["read".to_string()]));
        let service = SessionService::new(SessionConfig::new(TEST_SECRET).unwrap())
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
        let config = SessionConfig::new(TEST_SECRET).unwrap();
        let service = SessionService::new(config).unwrap();

        {
            let mut sessions = service.active_sessions.write().await;
            sessions.insert(
                "expired".to_string(),
                JwtClaims {
                    sub: "user".to_string(),
                    exp: Utc::now().timestamp() - 1,
                    iat: Utc::now().timestamp() - 10,
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
        let config = SessionConfig::new(TEST_SECRET).unwrap();
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
        let mut config = SessionConfig::new(TEST_SECRET).unwrap();
        config.jwt_ttl = Duration::zero();

        let error = config.validate().expect_err("zero ttl should fail");

        assert!(
            matches!(error, SessionError::InvalidConfig(message) if message == "jwt_ttl must be positive")
        );
    }

    #[tokio::test]
    async fn begin_session_reports_unknown_identity_provider() {
        let config = SessionConfig::new(TEST_SECRET).unwrap();
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
        let mut config = SessionConfig::new(TEST_SECRET).unwrap();
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
        let config = SessionConfig::new(TEST_SECRET).unwrap();
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
        let config = SessionConfig::new(TEST_SECRET).unwrap();
        let service = std::sync::Arc::new(SessionService::new(config).unwrap());

        // Plant an already-expired session directly in the store.
        let now = chrono::Utc::now().timestamp();
        service.active_sessions.write().await.insert(
            "expired-jti".to_string(),
            JwtClaims {
                sub: "alice".to_string(),
                exp: now - 10,
                iat: now - 20,
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
}
