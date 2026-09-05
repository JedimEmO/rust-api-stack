use crate::config::validate_jwt_secret;
use crate::{DEFAULT_MAX_SESSIONS_PER_USER, JwtAlgorithm, JwtAuthProvider};
use chrono::Duration;
use ras_auth_core::AuthProvider;
use std::collections::HashSet;

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
    let future_iat = planted_claims("alice", "j1", now + CLOCK_SKEW_LEEWAY_SECS + 30, now + 3600);
    assert!(matches!(
        service
            .verify_session(&signed_token(&service, &future_iat))
            .await,
        Err(SessionError::InvalidSession)
    ));

    // iat within leeway -> accepted.
    let skewed_iat = planted_claims("alice", "j2", now + CLOCK_SKEW_LEEWAY_SECS - 5, now + 3600);
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
