use super::*;

#[tokio::test]
async fn test_client_builder() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .with_jwt_token("test_token".to_string())
        .with_request_timeout(Duration::from_secs(60))
        .build()
        .await
        .expect("Failed to build client");

    assert_eq!(client.config().url, "ws://localhost:8080");
    assert_eq!(client.config().request_timeout, Duration::from_secs(60));
    assert!(matches!(client.config().auth, AuthConfig::JwtHeader { .. }));
}

#[tokio::test]
async fn builder_jwt_in_query_params_and_full_setters() {
    // Builder options must survive construction without a connection.
    let custom = ReconnectConfig::default();
    let client = ClientBuilder::new("ws://localhost:8080")
        .with_jwt_token("tok".into())
        .with_header("X-Custom", "v")
        .with_request_timeout(Duration::from_secs(11))
        .with_reconnect_config(custom)
        .with_heartbeat_interval(None)
        .with_connection_timeout(Duration::from_secs(7))
        .with_auto_connect(false)
        .build()
        .await
        .expect("build");

    assert!(matches!(client.config().auth, AuthConfig::JwtHeader { .. }));
    assert_eq!(client.config().request_timeout, Duration::from_secs(11));
    assert_eq!(client.config().connection_timeout, Duration::from_secs(7));
    assert!(client.config().heartbeat_interval.is_none());
    assert_eq!(
        client.config().custom_headers.get("X-Custom"),
        Some(&"v".to_string())
    );
    assert!(client.active_subscriptions().is_empty());
    assert_eq!(client.pending_requests_count(), 0);
}

#[tokio::test]
async fn builder_without_token_yields_no_auth() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("build");
    assert!(matches!(client.config().auth, AuthConfig::None));
}
