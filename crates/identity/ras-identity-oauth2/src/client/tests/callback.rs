use super::*;

#[tokio::test]
async fn handle_callback_rejects_state_for_wrong_provider_without_transport_call() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store, transport.clone());
    let provider_config = provider_config();

    let (_, state) = client
        .generate_authorization_url(&provider_config, HashMap::new())
        .await
        .unwrap();

    let mut wrong_provider = provider_config.clone();
    wrong_provider.provider_id = "other_provider".to_string();

    let error = client
        .handle_callback(
            &wrong_provider,
            AuthorizationResponse {
                code: Some("auth-code".to_string()),
                state,
                error: None,
                error_description: None,
                binding: None,
            },
        )
        .await
        .expect_err("provider mismatch should reject callback");

    assert!(matches!(error, OAuth2Error::InvalidState));
    assert!(transport.token_requests().is_empty());
}

#[tokio::test]
async fn i5_handle_callback_maps_provider_error_to_fixed_variant_without_description() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store, transport.clone());
    let provider_config = provider_config();

    let (_, state) = client
        .generate_authorization_url(&provider_config, HashMap::new())
        .await
        .unwrap();

    // A legitimate denial carries no code at all.
    let error = client
        .handle_callback(
            &provider_config,
            AuthorizationResponse {
                code: None,
                state,
                error: Some("access_denied".to_string()),
                error_description: Some("user denied consent <script>".to_string()),
                binding: None,
            },
        )
        .await
        .expect_err("provider callback error should be surfaced");

    match &error {
        OAuth2Error::ProviderDenied { error } => assert_eq!(error, "access_denied"),
        other => panic!("expected ProviderDenied, got {other:?}"),
    }
    // The free-text description never reaches the error string.
    assert!(!error.to_string().contains("user denied consent"));
    assert!(transport.token_requests().is_empty());
}

#[tokio::test]
async fn i5_handle_callback_without_code_or_error_is_invalid_callback() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store, transport.clone());
    let provider_config = provider_config();

    let (_, state) = client
        .generate_authorization_url(&provider_config, HashMap::new())
        .await
        .unwrap();

    let error = client
        .handle_callback(
            &provider_config,
            AuthorizationResponse {
                code: None,
                state,
                error: None,
                error_description: None,
                binding: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, OAuth2Error::InvalidCallback));
    assert!(transport.token_requests().is_empty());
}

#[test]
fn i5_authorization_response_deserializes_without_code() {
    let denied: AuthorizationResponse = serde_json::from_value(serde_json::json!({
        "state": "s",
        "error": "access_denied"
    }))
    .unwrap();
    assert!(denied.code.is_none());
    assert_eq!(denied.error.as_deref(), Some("access_denied"));
}

#[test]
fn i7_binding_matches_is_exact_and_rejects_missing() {
    assert!(binding_matches("cookie-123", Some("cookie-123")));
    assert!(!binding_matches("cookie-123", Some("cookie-124")));
    assert!(!binding_matches("cookie-123", Some("cookie-12")));
    assert!(!binding_matches("cookie-123", Some("cookie-1234")));
    assert!(!binding_matches("cookie-123", Some("")));
    assert!(!binding_matches("cookie-123", None));
}

#[tokio::test]
async fn handle_callback_omits_code_verifier_when_pkce_is_disabled() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store, transport.clone());
    let mut provider_config = provider_config();
    provider_config.use_pkce = false;

    let (_, state) = client
        .generate_authorization_url(&provider_config, HashMap::new())
        .await
        .unwrap();

    let token = client
        .handle_callback(
            &provider_config,
            AuthorizationResponse {
                code: Some("auth-code".to_string()),
                state,
                error: None,
                error_description: None,
                binding: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(token.access_token, "access-token");
    let requests = transport.token_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "https://example.com/token");
    assert_eq!(requests[0].1.get("code"), Some(&"auth-code".to_string()));
    assert_eq!(
        requests[0].1.get("grant_type"),
        Some(&"authorization_code".to_string())
    );
    assert!(!requests[0].1.contains_key("code_verifier"));
}

#[tokio::test]
async fn handle_callback_enforces_session_binding() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store.clone(), transport.clone());
    let config = provider_config();

    // A callback missing the binding is rejected before any token
    // exchange (and the one-use state is burned).
    let (_, state) = client
        .generate_authorization_url_bound(&config, HashMap::new(), Some("cookie-123".to_string()))
        .await
        .unwrap();
    let err = client
        .handle_callback(
            &config,
            AuthorizationResponse {
                code: Some("code".to_string()),
                state,
                error: None,
                error_description: None,
                binding: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, OAuth2Error::InvalidState));
    assert!(transport.token_requests().is_empty());

    // The matching binding completes the flow.
    let (_, state) = client
        .generate_authorization_url_bound(&config, HashMap::new(), Some("cookie-123".to_string()))
        .await
        .unwrap();
    client
        .handle_callback(
            &config,
            AuthorizationResponse {
                code: Some("code".to_string()),
                state,
                error: None,
                error_description: None,
                binding: Some("cookie-123".to_string()),
            },
        )
        .await
        .expect("bound callback succeeds");
    assert_eq!(transport.token_requests().len(), 1);
}

#[tokio::test]
async fn i7_handle_callback_rejects_wrong_binding_value() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let transport = Arc::new(RecordingTransport::new());
    let client = client_with_transport(state_store.clone(), transport.clone());
    let config = provider_config();

    let (_, state) = client
        .generate_authorization_url_bound(&config, HashMap::new(), Some("cookie-123".to_string()))
        .await
        .unwrap();
    let err = client
        .handle_callback(
            &config,
            AuthorizationResponse {
                code: Some("code".to_string()),
                state,
                error: None,
                error_description: None,
                binding: Some("cookie-124".to_string()),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, OAuth2Error::InvalidState));
    assert!(transport.token_requests().is_empty());

    // An unbound flow still ignores whatever the callback presents.
    let (_, state) = client
        .generate_authorization_url_bound(&config, HashMap::new(), None)
        .await
        .unwrap();
    client
        .handle_callback(
            &config,
            AuthorizationResponse {
                code: Some("code".to_string()),
                state,
                error: None,
                error_description: None,
                binding: Some("anything".to_string()),
            },
        )
        .await
        .expect("unbound flow ignores callback binding");
}
