//! OAuth2 token source and consent flow tests against an in-process fake
//! provider transport.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Duration;
use ras_integration_core::{
    GrantStore, InMemoryGrantStore, IntegrationError, SecretString, TokenRequest, TokenSource,
    TokenSubject, UserGrant,
};
use ras_integration_oauth2::{ConsentFlow, OAuth2ProviderConfig, OAuth2TokenSource};
use ras_transport_core::http::{HeaderMap, StatusCode};
use ras_transport_core::{
    HttpTransport, RequestBody, TransportError, TransportRequest, TransportResponse,
    byte_stream_from,
};
use tokio::sync::Mutex;

const TOKEN_ENDPOINT: &str = "https://provider.test/oauth/token";

/// Fake provider: scripted responses, records decoded form bodies.
#[derive(Default)]
struct FakeProvider {
    responses: Mutex<VecDeque<(StatusCode, serde_json::Value)>>,
    requests: Mutex<Vec<HashMap<String, String>>>,
}

impl FakeProvider {
    async fn push_response(&self, status: StatusCode, body: serde_json::Value) {
        self.responses.lock().await.push_back((status, body));
    }

    async fn requests(&self) -> Vec<HashMap<String, String>> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl HttpTransport for FakeProvider {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let params = match &request.body {
            RequestBody::Bytes(bytes) => {
                serde_urlencoded::from_bytes::<Vec<(String, String)>>(bytes)
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            }
            _ => HashMap::new(),
        };
        self.requests.lock().await.push(params);

        let (status, body) = self
            .responses
            .lock()
            .await
            .pop_front()
            .unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({})));
        Ok(TransportResponse::new(
            status,
            HeaderMap::new(),
            byte_stream_from(futures::stream::iter(vec![Ok(Bytes::from(
                serde_json::to_vec(&body).unwrap(),
            ))])),
        ))
    }
}

fn provider_config() -> OAuth2ProviderConfig {
    OAuth2ProviderConfig::new(TOKEN_ENDPOINT, "ras-client")
        .unwrap()
        .with_authorization_endpoint("https://provider.test/oauth/authorize")
        .unwrap()
        .with_client_secret("ras-client-secret")
}

async fn seeded_grants() -> Arc<InMemoryGrantStore> {
    let grants = Arc::new(InMemoryGrantStore::new());
    grants
        .put_user_grant(UserGrant {
            integration_id: "google-calendar".to_string(),
            user_id: "alice".to_string(),
            refresh_token: SecretString::new("stored-refresh-token"),
            scopes: vec![
                "calendar.readonly".to_string(),
                "calendar.write".to_string(),
            ],
        })
        .await
        .unwrap();
    grants
}

fn user_request(scopes: &[&str]) -> TokenRequest {
    TokenRequest {
        integration_id: "google-calendar".to_string(),
        subject: TokenSubject::User {
            user_id: "alice".to_string(),
        },
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        audience: None,
        force_refresh: false,
    }
}

fn source_with(provider: Arc<FakeProvider>, grants: Arc<InMemoryGrantStore>) -> OAuth2TokenSource {
    OAuth2TokenSource::new(provider_config(), provider, grants).unwrap()
}

#[tokio::test]
async fn refresh_flow_sends_expected_params_and_returns_lease() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(
            StatusCode::OK,
            serde_json::json!({
                "access_token": "fresh-access-token",
                "token_type": "Bearer",
                "expires_in": 3600,
                "scope": "calendar.readonly"
            }),
        )
        .await;
    let source = source_with(provider.clone(), seeded_grants().await);

    let lease = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap();
    assert_eq!(lease.access_token.expose_secret(), "fresh-access-token");
    assert!(lease.expires_at.is_some());
    assert_eq!(lease.scopes, vec!["calendar.readonly"]);

    let requests = provider.requests().await;
    assert_eq!(requests.len(), 1);
    let params = &requests[0];
    assert_eq!(params["grant_type"], "refresh_token");
    assert_eq!(params["refresh_token"], "stored-refresh-token");
    assert_eq!(params["client_id"], "ras-client");
    assert_eq!(params["client_secret"], "ras-client-secret");
    assert_eq!(params["scope"], "calendar.readonly");
}

#[tokio::test]
async fn missing_grant_is_consent_required_without_provider_call() {
    let provider = Arc::new(FakeProvider::default());
    let source = source_with(provider.clone(), Arc::new(InMemoryGrantStore::new()));

    let err = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        IntegrationError::ConsentRequired { user_id, .. } if user_id == "alice"
    ));
    assert!(provider.requests().await.is_empty());
}

#[tokio::test]
async fn scopes_beyond_grant_are_consent_required_with_missing_scopes() {
    let provider = Arc::new(FakeProvider::default());
    let source = source_with(provider.clone(), seeded_grants().await);

    let err = source
        .issue_token(&user_request(&["calendar.readonly", "drive.readonly"]))
        .await
        .unwrap_err();
    let IntegrationError::ConsentRequired { missing_scopes, .. } = err else {
        panic!("expected ConsentRequired, got {err:?}");
    };
    assert_eq!(missing_scopes, vec!["drive.readonly"]);
    assert!(provider.requests().await.is_empty());
}

#[tokio::test]
async fn invalid_grant_response_maps_to_consent_required() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "invalid_grant", "error_description": "revoked"}),
        )
        .await;
    let source = source_with(provider, seeded_grants().await);

    let err = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::ConsentRequired { .. }));
}

#[tokio::test]
async fn invalid_scope_response_maps_to_denied() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "invalid_scope"}),
        )
        .await;
    let source = source_with(provider, seeded_grants().await);

    let err = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IntegrationError::Denied { reason, .. } if reason.contains("invalid_scope"))
    );
}

#[tokio::test]
async fn provider_5xx_and_malformed_responses_are_provider_errors() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({}))
        .await;
    let source = source_with(provider.clone(), seeded_grants().await);
    let err = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Provider { .. }));

    provider
        .push_response(StatusCode::OK, serde_json::json!({"unexpected": true}))
        .await;
    let err = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Provider { .. }));
}

#[tokio::test]
async fn rotated_refresh_token_is_persisted_before_lease_returns() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(
            StatusCode::OK,
            serde_json::json!({
                "access_token": "fresh-access-token",
                "expires_in": 3600,
                "refresh_token": "rotated-refresh-token"
            }),
        )
        .await;
    let grants = seeded_grants().await;
    let source = source_with(provider, grants.clone());

    source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap();

    let stored = grants
        .get_user_grant("google-calendar", "alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.refresh_token.expose_secret(),
        "rotated-refresh-token"
    );
    // Consented scopes are preserved through rotation.
    assert_eq!(stored.scopes.len(), 2);
}

/// Grant store whose writes fail — proves rotation persistence failures
/// surface instead of being swallowed.
struct ReadOnlyGrantStore(Arc<InMemoryGrantStore>);

#[async_trait]
impl GrantStore for ReadOnlyGrantStore {
    async fn get_user_grant(
        &self,
        integration_id: &str,
        user_id: &str,
    ) -> Result<Option<UserGrant>, IntegrationError> {
        self.0.get_user_grant(integration_id, user_id).await
    }

    async fn put_user_grant(&self, _grant: UserGrant) -> Result<(), IntegrationError> {
        Err(IntegrationError::GrantStore(
            "simulated write failure".to_string(),
        ))
    }

    async fn remove_user_grant(
        &self,
        integration_id: &str,
        user_id: &str,
    ) -> Result<bool, IntegrationError> {
        self.0.remove_user_grant(integration_id, user_id).await
    }
}

#[tokio::test]
async fn failed_rotation_persistence_surfaces_as_error() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(
            StatusCode::OK,
            serde_json::json!({
                "access_token": "fresh-access-token",
                "refresh_token": "rotated-refresh-token"
            }),
        )
        .await;
    let grants: Arc<dyn GrantStore> = Arc::new(ReadOnlyGrantStore(seeded_grants().await));
    let source = OAuth2TokenSource::new(provider_config(), provider, grants).unwrap();

    let err = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::GrantStore(_)));
}

#[tokio::test]
async fn client_credentials_flow_sends_secret_and_audience() {
    let provider = Arc::new(FakeProvider::default());
    provider
        .push_response(
            StatusCode::OK,
            serde_json::json!({"access_token": "svc-token", "expires_in": 600}),
        )
        .await;
    let source = source_with(provider.clone(), Arc::new(InMemoryGrantStore::new()));

    let lease = source
        .issue_token(&TokenRequest {
            integration_id: "partner-api".to_string(),
            subject: TokenSubject::Service,
            scopes: vec!["partner:read".to_string()],
            audience: Some("https://partner.example.com/api".to_string()),
            force_refresh: false,
        })
        .await
        .unwrap();
    assert_eq!(lease.access_token.expose_secret(), "svc-token");

    let requests = provider.requests().await;
    let params = &requests[0];
    assert_eq!(params["grant_type"], "client_credentials");
    assert_eq!(params["client_secret"], "ras-client-secret");
    assert_eq!(params["audience"], "https://partner.example.com/api");
    assert_eq!(params["scope"], "partner:read");
}

#[tokio::test]
async fn client_credentials_without_secret_fails_closed() {
    let provider = Arc::new(FakeProvider::default());
    let config = OAuth2ProviderConfig::new(TOKEN_ENDPOINT, "ras-client").unwrap();
    let source = OAuth2TokenSource::new(
        config,
        provider.clone(),
        Arc::new(InMemoryGrantStore::new()),
    )
    .unwrap();

    let err = source
        .issue_token(&TokenRequest {
            integration_id: "partner-api".to_string(),
            subject: TokenSubject::Service,
            scopes: vec![],
            audience: None,
            force_refresh: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::InvalidConfig(_)));
    assert!(provider.requests().await.is_empty());
}

#[tokio::test]
async fn service_account_subjects_are_rejected() {
    let provider = Arc::new(FakeProvider::default());
    let source = source_with(provider, Arc::new(InMemoryGrantStore::new()));
    let err = source
        .issue_token(&TokenRequest {
            integration_id: "partner-api".to_string(),
            subject: TokenSubject::ServiceAccount {
                service_account_id: "bot-1".to_string(),
            },
            scopes: vec![],
            audience: None,
            force_refresh: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Denied { .. }));
}

// --- Consent flow ---

#[test]
fn begin_builds_pkce_authorization_url() {
    let flow = ConsentFlow::new(Duration::minutes(10));
    let redirect = flow
        .begin(
            &provider_config(),
            "google-calendar",
            "alice",
            "https://app.example.com/oauth/callback",
            ["calendar.readonly"],
        )
        .unwrap();

    let url = url::Url::parse(&redirect.url).unwrap();
    let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["client_id"], "ras-client");
    assert_eq!(
        params["redirect_uri"],
        "https://app.example.com/oauth/callback"
    );
    assert_eq!(params["scope"], "calendar.readonly");
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["state"], redirect.state);
    assert!(!params["code_challenge"].is_empty());
}

#[test]
fn callback_state_is_single_use_and_bound_to_user_and_integration() {
    let flow = ConsentFlow::new(Duration::minutes(10));
    let config = provider_config();
    let redirect = flow
        .begin(&config, "google-calendar", "alice", "https://cb", ["s"])
        .unwrap();

    // Wrong integration / wrong user fail and consume nothing... but state
    // is single-use, so test the failures on fresh states.
    let wrong_user = flow
        .begin(&config, "google-calendar", "alice", "https://cb", ["s"])
        .unwrap();
    assert!(
        flow.validate_callback(&wrong_user.state, "google-calendar", "mallory")
            .is_err()
    );

    let wrong_integration = flow
        .begin(&config, "google-calendar", "alice", "https://cb", ["s"])
        .unwrap();
    assert!(
        flow.validate_callback(&wrong_integration.state, "github", "alice")
            .is_err()
    );

    // Unknown state fails.
    assert!(
        flow.validate_callback("forged-state", "google-calendar", "alice")
            .is_err()
    );

    // Correct binding succeeds exactly once.
    assert!(
        flow.validate_callback(&redirect.state, "google-calendar", "alice")
            .is_ok()
    );
    assert!(
        flow.validate_callback(&redirect.state, "google-calendar", "alice")
            .is_err(),
        "state must be single-use"
    );
}

#[test]
fn expired_state_is_rejected() {
    let flow = ConsentFlow::new(Duration::seconds(-1));
    let redirect = flow
        .begin(
            &provider_config(),
            "google-calendar",
            "alice",
            "https://cb",
            ["s"],
        )
        .unwrap();
    let err = flow
        .validate_callback(&redirect.state, "google-calendar", "alice")
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Denied { .. }));
}

#[tokio::test]
async fn full_consent_exchange_stores_grant_and_enables_refresh_flow() {
    let provider = Arc::new(FakeProvider::default());
    let transport: Arc<dyn HttpTransport> = provider.clone();
    let grants: Arc<dyn GrantStore> = Arc::new(InMemoryGrantStore::new());
    let config = provider_config();
    let flow = ConsentFlow::new(Duration::minutes(10));

    let redirect = flow
        .begin(
            &config,
            "google-calendar",
            "alice",
            "https://app.example.com/cb",
            ["calendar.readonly"],
        )
        .unwrap();
    let consent = flow
        .validate_callback(&redirect.state, "google-calendar", "alice")
        .unwrap();

    provider
        .push_response(
            StatusCode::OK,
            serde_json::json!({
                "access_token": "first-access-token",
                "expires_in": 3600,
                "refresh_token": "first-refresh-token"
            }),
        )
        .await;
    let lease = consent
        .exchange_code(&config, &transport, &grants, "auth-code-123")
        .await
        .unwrap();
    assert_eq!(lease.access_token.expose_secret(), "first-access-token");

    // The exchange sent the code, verifier, and bound redirect URI.
    let requests = provider.requests().await;
    let params = &requests[0];
    assert_eq!(params["grant_type"], "authorization_code");
    assert_eq!(params["code"], "auth-code-123");
    assert_eq!(params["redirect_uri"], "https://app.example.com/cb");
    assert!(!params["code_verifier"].is_empty());

    // The stored grant now powers the regular refresh flow.
    provider
        .push_response(
            StatusCode::OK,
            serde_json::json!({"access_token": "second-access-token", "expires_in": 3600}),
        )
        .await;
    let source = OAuth2TokenSource::new(config, transport, grants).unwrap();
    let lease = source
        .issue_token(&user_request(&["calendar.readonly"]))
        .await
        .unwrap();
    assert_eq!(lease.access_token.expose_secret(), "second-access-token");

    let requests = provider.requests().await;
    assert_eq!(requests[1]["refresh_token"], "first-refresh-token");
}
