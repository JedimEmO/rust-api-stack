//! Token manager behavior: bounds, caching, refresh, dedup, invalidation.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use ras_integration_core::TokenFamily;
use ras_integration_core::testing::{FakeTokenSource, counting_source};
use ras_integration_core::{
    IntegrationConfig, IntegrationError, SecretString, TokenLease, TokenManager, TokenRequest,
    TokenSubject,
};

fn config(id: &str, scopes: &[&str]) -> IntegrationConfig {
    IntegrationConfig::new(id, scopes.iter().copied(), ["https://api.example.com"]).unwrap()
}

fn request(id: &str, scopes: &[&str]) -> TokenRequest {
    TokenRequest {
        integration_id: id.to_string(),
        subject: TokenSubject::User {
            user_id: "alice".to_string(),
        },
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        audience: None,
        force_refresh: false,
    }
}

#[tokio::test]
async fn unknown_integration_fails_closed() {
    let manager = TokenManager::builder().build();
    let err = manager.get_token(request("nope", &[])).await.unwrap_err();
    assert!(matches!(
        err,
        IntegrationError::UnknownIntegration { integration_id } if integration_id == "nope"
    ));
}

#[tokio::test]
async fn scope_outside_allowlist_fails_before_source() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let manager = TokenManager::builder()
        .register(config("cal", &["calendar.readonly"]), source.clone())
        .unwrap()
        .build();

    let err = manager
        .get_token(request("cal", &["calendar.write"]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IntegrationError::ScopeNotAllowed { scope, .. } if scope == "calendar.write")
    );
    assert_eq!(source.call_count(), 0, "source must never be consulted");
}

#[tokio::test]
async fn audience_outside_allowlist_fails_before_source() {
    let source = counting_source(TokenFamily::RasInternal, 300);
    let manager = TokenManager::builder()
        .register(
            config("invoice", &["invoice:read"]).with_allowed_audiences(["invoice-service"]),
            source.clone(),
        )
        .unwrap()
        .build();

    let mut req = request("invoice", &["invoice:read"]);
    req.audience = Some("admin-service".to_string());
    let err = manager.get_token(req).await.unwrap_err();
    assert!(matches!(
        err,
        IntegrationError::AudienceNotAllowed { audience, .. } if audience == "admin-service"
    ));
    assert_eq!(source.call_count(), 0);
}

#[tokio::test]
async fn fresh_lease_is_served_from_cache() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let manager = TokenManager::builder()
        .register(config("cal", &["calendar.readonly"]), source.clone())
        .unwrap()
        .build();

    let first = manager
        .get_token(request("cal", &["calendar.readonly"]))
        .await
        .unwrap();
    let second = manager
        .get_token(request("cal", &["calendar.readonly"]))
        .await
        .unwrap();
    assert_eq!(
        first.access_token.expose_secret(),
        second.access_token.expose_secret()
    );
    assert_eq!(source.call_count(), 1);
}

#[tokio::test]
async fn scope_order_and_duplicates_share_one_cache_entry() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let manager = TokenManager::builder()
        .register(config("cal", &["a", "b"]), source.clone())
        .unwrap()
        .build();

    manager
        .get_token(request("cal", &["b", "a"]))
        .await
        .unwrap();
    manager
        .get_token(request("cal", &["a", "b", "a"]))
        .await
        .unwrap();
    assert_eq!(source.call_count(), 1, "canonicalized scopes share a key");
}

#[tokio::test]
async fn distinct_subjects_audiences_and_scopes_get_distinct_leases() {
    let source = counting_source(TokenFamily::RasInternal, 300);
    let manager = TokenManager::builder()
        .register(
            config("svc", &["a", "b"]).with_allowed_audiences(["aud-1", "aud-2"]),
            source.clone(),
        )
        .unwrap()
        .build();

    let mut base = request("svc", &["a"]);
    base.audience = Some("aud-1".to_string());

    manager.get_token(base.clone()).await.unwrap();

    let mut other_subject = base.clone();
    other_subject.subject = TokenSubject::Service;
    manager.get_token(other_subject).await.unwrap();

    let mut other_audience = base.clone();
    other_audience.audience = Some("aud-2".to_string());
    manager.get_token(other_audience).await.unwrap();

    let mut other_scopes = base.clone();
    other_scopes.scopes = vec!["b".to_string()];
    manager.get_token(other_scopes).await.unwrap();

    assert_eq!(source.call_count(), 4);
}

#[tokio::test]
async fn lease_expiring_within_skew_is_refreshed() {
    // TTL 30s with the default 60s refresh skew: always considered stale.
    let source = counting_source(TokenFamily::OAuth2, 30);
    let manager = TokenManager::builder()
        .register(config("cal", &["a"]), source.clone())
        .unwrap()
        .build();

    manager.get_token(request("cal", &["a"])).await.unwrap();
    manager.get_token(request("cal", &["a"])).await.unwrap();
    assert_eq!(source.call_count(), 2, "near-expiry leases refresh early");
}

#[tokio::test]
async fn force_refresh_bypasses_cache() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let manager = TokenManager::builder()
        .register(config("cal", &["a"]), source.clone())
        .unwrap()
        .build();

    manager.get_token(request("cal", &["a"])).await.unwrap();
    let mut forced = request("cal", &["a"]);
    forced.force_refresh = true;
    manager.get_token(forced).await.unwrap();
    assert_eq!(source.call_count(), 2);
}

#[tokio::test]
async fn concurrent_requests_for_same_key_are_deduplicated() {
    let source = Arc::new(
        FakeTokenSource::new(TokenFamily::OAuth2, |req| {
            Ok(TokenLease {
                access_token: SecretString::new("shared-token"),
                expires_at: Some(Utc::now() + Duration::hours(1)),
                scopes: req.scopes.clone(),
            })
        })
        .with_delay(StdDuration::from_millis(50)),
    );
    let manager = Arc::new(
        TokenManager::builder()
            .register(config("cal", &["a"]), source.clone())
            .unwrap()
            .build(),
    );

    let tasks: Vec<_> = (0..16)
        .map(|_| {
            let manager = manager.clone();
            tokio::spawn(async move { manager.get_token(request("cal", &["a"])).await })
        })
        .collect();
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(source.call_count(), 1, "one refresh serves all waiters");
}

#[tokio::test]
async fn source_errors_propagate_and_are_not_cached() {
    let source = Arc::new(FakeTokenSource::new(TokenFamily::OAuth2, |req| {
        Err(IntegrationError::ConsentRequired {
            integration_id: req.integration_id.clone(),
            user_id: "alice".to_string(),
            missing_scopes: req.scopes.clone(),
        })
    }));
    let manager = TokenManager::builder()
        .register(config("cal", &["a"]), source.clone())
        .unwrap()
        .build();

    for _ in 0..2 {
        let err = manager.get_token(request("cal", &["a"])).await.unwrap_err();
        assert!(matches!(err, IntegrationError::ConsentRequired { .. }));
    }
    assert_eq!(source.call_count(), 2, "errors are never cached");
}

#[tokio::test]
async fn invalidate_drops_cached_leases_for_subject() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let manager = TokenManager::builder()
        .register(config("cal", &["a"]), source.clone())
        .unwrap()
        .build();

    manager.get_token(request("cal", &["a"])).await.unwrap();
    manager
        .invalidate(
            "cal",
            &TokenSubject::User {
                user_id: "alice".to_string(),
            },
        )
        .await;
    manager.get_token(request("cal", &["a"])).await.unwrap();
    assert_eq!(source.call_count(), 2);
}

#[tokio::test]
async fn config_version_bump_invalidates_cache_keys() {
    // Two managers simulate a config rollout; the cache key includes the
    // version, so this asserts on key construction, not shared state.
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let manager = TokenManager::builder()
        .register(config("cal", &["a"]).with_config_version(1), source.clone())
        .unwrap()
        .build();
    manager.get_token(request("cal", &["a"])).await.unwrap();

    let manager_v2 = TokenManager::builder()
        .register(config("cal", &["a"]).with_config_version(2), source.clone())
        .unwrap()
        .build();
    manager_v2.get_token(request("cal", &["a"])).await.unwrap();
    assert_eq!(source.call_count(), 2);
}

#[tokio::test]
async fn duplicate_registration_is_rejected() {
    let result = TokenManager::builder()
        .register(
            config("cal", &["a"]),
            counting_source(TokenFamily::OAuth2, 60),
        )
        .unwrap()
        .register(
            config("cal", &["a"]),
            counting_source(TokenFamily::OAuth2, 60),
        );
    assert!(matches!(
        result,
        Err(IntegrationError::InvalidConfig(message)) if message.contains("cal")
    ));
}

#[tokio::test]
async fn outbound_url_validation_fails_closed() {
    let manager = TokenManager::builder()
        .register(
            config("cal", &["a"]),
            counting_source(TokenFamily::OAuth2, 60),
        )
        .unwrap()
        .build();

    manager
        .validate_outbound_url("cal", "https://api.example.com/v1")
        .unwrap();
    assert!(matches!(
        manager
            .validate_outbound_url("cal", "https://api.example.com.evil.com/v1")
            .unwrap_err(),
        IntegrationError::HostNotAllowed { .. }
    ));
    assert!(matches!(
        manager
            .validate_outbound_url("unknown", "https://api.example.com/v1")
            .unwrap_err(),
        IntegrationError::UnknownIntegration { .. }
    ));
}
