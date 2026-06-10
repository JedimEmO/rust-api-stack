//! Capability-scoped client behavior: host validation before token minting,
//! fail-closed bearer attachment, no auto-replay.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use ras_integration_core::testing::counting_source;
use ras_integration_core::{
    AuthorizedHttpClient, IntegrationConfig, IntegrationError, TokenFamily, TokenManager,
};
use ras_transport_core::http::{HeaderMap, StatusCode};
use ras_transport_core::{
    HttpTransport, TransportError, TransportRequest, TransportResponse, byte_stream_from,
};
use tokio::sync::Mutex;

/// Records executed requests and answers 200 OK.
#[derive(Default)]
struct CapturingTransport {
    seen: Mutex<Vec<(String, Option<String>)>>,
}

impl CapturingTransport {
    async fn seen(&self) -> Vec<(String, Option<String>)> {
        self.seen.lock().await.clone()
    }
}

#[async_trait]
impl HttpTransport for CapturingTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let auth = request
            .headers
            .get(ras_transport_core::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());
        self.seen.lock().await.push((request.url.clone(), auth));
        Ok(TransportResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            byte_stream_from(futures::stream::iter(vec![Ok(Bytes::from_static(b"{}"))])),
        ))
    }
}

fn manager_with(source: Arc<dyn ras_integration_core::TokenSource>) -> Arc<TokenManager> {
    Arc::new(
        TokenManager::builder()
            .register(
                IntegrationConfig::new(
                    "google-calendar",
                    ["calendar.readonly"],
                    ["https://www.googleapis.com/calendar"],
                )
                .unwrap(),
                source,
            )
            .unwrap()
            .build(),
    )
}

#[tokio::test]
async fn bearer_token_is_attached_to_allowed_host() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let transport = Arc::new(CapturingTransport::default());
    let client = AuthorizedHttpClient::for_user(
        transport.clone(),
        manager_with(source),
        "google-calendar",
        "alice",
        ["calendar.readonly"],
    );

    let response = client
        .get("https://www.googleapis.com/calendar/v3/events")
        .await
        .unwrap();
    assert!(response.is_success());

    let seen = transport.seen().await;
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1.as_deref(), Some("Bearer token-0"));
}

#[tokio::test]
async fn disallowed_host_fails_before_any_token_is_minted() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let transport = Arc::new(CapturingTransport::default());
    let client = AuthorizedHttpClient::for_user(
        transport.clone(),
        manager_with(source.clone()),
        "google-calendar",
        "alice",
        ["calendar.readonly"],
    );

    let err = client
        .get("https://attacker.example.com/calendar/v3/events")
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::HostNotAllowed { .. }));
    assert_eq!(source.call_count(), 0, "no token minted for bad host");
    assert!(transport.seen().await.is_empty(), "nothing sent");
}

#[tokio::test]
async fn scopes_outside_client_capability_cannot_be_requested() {
    // The client is pinned to its scope set at construction; the manager
    // enforces config bounds. A client built with an unallowed scope fails
    // closed on every request.
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let client = AuthorizedHttpClient::for_user(
        Arc::new(CapturingTransport::default()),
        manager_with(source.clone()),
        "google-calendar",
        "alice",
        ["calendar.write"],
    );

    let err = client
        .get("https://www.googleapis.com/calendar/v3/events")
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::ScopeNotAllowed { .. }));
    assert_eq!(source.call_count(), 0);
}

#[tokio::test]
async fn repeated_calls_reuse_cached_lease() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let transport = Arc::new(CapturingTransport::default());
    let client = AuthorizedHttpClient::for_user(
        transport.clone(),
        manager_with(source.clone()),
        "google-calendar",
        "alice",
        ["calendar.readonly"],
    );

    for _ in 0..3 {
        client
            .get("https://www.googleapis.com/calendar/v3/events")
            .await
            .unwrap();
    }
    assert_eq!(source.call_count(), 1);
    let seen = transport.seen().await;
    assert_eq!(seen.len(), 3);
    assert!(
        seen.iter()
            .all(|(_, auth)| auth.as_deref() == Some("Bearer token-0"))
    );
}

#[tokio::test]
async fn post_json_sets_body_and_bearer() {
    let source = counting_source(TokenFamily::OAuth2, 3600);
    let transport = Arc::new(CapturingTransport::default());
    let client = AuthorizedHttpClient::for_user(
        transport.clone(),
        manager_with(source),
        "google-calendar",
        "alice",
        ["calendar.readonly"],
    );

    client
        .post_json(
            "https://www.googleapis.com/calendar/v3/events",
            &serde_json::json!({"summary": "standup"}),
        )
        .await
        .unwrap();
    let seen = transport.seen().await;
    assert_eq!(seen.len(), 1);
    assert!(seen[0].1.as_deref().unwrap().starts_with("Bearer "));
}
