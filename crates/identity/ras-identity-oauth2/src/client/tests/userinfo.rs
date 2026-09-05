use super::*;

#[tokio::test]
async fn get_user_info_returns_config_error_when_endpoint_is_missing() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store, transport.clone());
    let mut provider_config = provider_config();
    provider_config.userinfo_endpoint = None;

    let error = client
        .get_user_info(&provider_config, "access-token")
        .await
        .expect_err("missing userinfo endpoint should be a config error");

    match error {
        OAuth2Error::ConfigError(message) => {
            assert_eq!(message, "User info endpoint not configured");
        }
        other => panic!("expected config error, got {other:?}"),
    }
    assert!(transport.userinfo_requests().is_empty());
}

#[tokio::test]
async fn get_user_info_delegates_endpoint_and_access_token_to_transport() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store, transport.clone());
    let provider_config = provider_config();

    let user_info = client
        .get_user_info(&provider_config, "access-token")
        .await
        .unwrap();

    assert_eq!(user_info.sub, "user-1");
    assert_eq!(
        transport.userinfo_requests(),
        vec![(
            "https://example.com/userinfo".to_string(),
            "access-token".to_string()
        )]
    );
}
