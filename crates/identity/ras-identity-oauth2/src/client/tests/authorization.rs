use super::*;

#[tokio::test]
async fn test_authorization_url_generation() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(state_store, 600, 30);

    let provider_config = provider_config();

    let (auth_url, state) = client
        .generate_authorization_url(&provider_config, HashMap::new())
        .await
        .unwrap();

    // Verify URL structure
    let url = Url::parse(&auth_url).unwrap();
    assert_eq!(url.host_str(), Some("example.com"));
    assert_eq!(url.path(), "/auth");

    // Verify query parameters
    let params: HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(params.get("response_type"), Some(&"code".into()));
    assert_eq!(params.get("client_id"), Some(&"test_client_id".into()));
    assert_eq!(
        params.get("redirect_uri"),
        Some(&"http://localhost:3000/callback".into())
    );
    assert_eq!(params.get("state"), Some(&state.into()));
    assert_eq!(params.get("scope"), Some(&"openid email".into()));
    assert!(params.contains_key("code_challenge"));
    assert_eq!(params.get("code_challenge_method"), Some(&"S256".into()));
}

#[tokio::test]
async fn authorization_url_merges_provider_and_request_params_without_pkce() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(state_store.clone(), 600, 30);
    let mut provider_config = provider_config();
    provider_config.use_pkce = false;
    provider_config
        .auth_params
        .insert("prompt".to_string(), "consent".to_string());

    let mut additional_params = HashMap::new();
    additional_params.insert("login_hint".to_string(), "user@example.com".to_string());

    let (auth_url, state_param) = client
        .generate_authorization_url(&provider_config, additional_params)
        .await
        .unwrap();

    let url = Url::parse(&auth_url).unwrap();
    let params: HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(params.get("prompt"), Some(&"consent".into()));
    assert_eq!(params.get("login_hint"), Some(&"user@example.com".into()));
    assert!(!params.contains_key("code_challenge"));
    assert!(!params.contains_key("code_challenge_method"));

    let stored_state = state_store.retrieve(&state_param).await.unwrap();
    assert_eq!(stored_state.provider_id, "test_provider");
    assert!(stored_state.code_verifier.is_none());
}

#[tokio::test]
async fn authorization_url_includes_nonce_and_stores_it() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(state_store.clone(), 600, 30);

    let (auth_url, state) = client
        .generate_authorization_url(&provider_config(), HashMap::new())
        .await
        .unwrap();

    let url = Url::parse(&auth_url).unwrap();
    let params: HashMap<_, _> = url.query_pairs().collect();
    let url_nonce = params.get("nonce").expect("nonce in URL").to_string();
    assert!(!url_nonce.is_empty());

    let stored = state_store.retrieve(&state).await.unwrap();
    assert_eq!(stored.nonce.as_deref(), Some(url_nonce.as_str()));
}

#[tokio::test]
async fn reserved_params_cannot_override_security_parameters() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(state_store, 600, 30);
    let config = provider_config();

    for reserved in [
        "redirect_uri",
        "response_type",
        "state",
        "client_id",
        "code_challenge",
        "nonce",
    ] {
        let mut params = HashMap::new();
        params.insert(reserved.to_string(), "attacker".to_string());
        let result = client.generate_authorization_url(&config, params).await;
        assert!(
            matches!(result, Err(OAuth2Error::InvalidAuthorizationParam(_))),
            "expected `{reserved}` to be rejected, got {result:?}"
        );
    }

    // Case-insensitive match is enforced too.
    let mut params = HashMap::new();
    params.insert(
        "Redirect_URI".to_string(),
        "https://evil.test/cb".to_string(),
    );
    assert!(matches!(
        client.generate_authorization_url(&config, params).await,
        Err(OAuth2Error::InvalidAuthorizationParam(_))
    ));

    // A safe extra parameter is still accepted and appears exactly once.
    let mut params = HashMap::new();
    params.insert("login_hint".to_string(), "user@example.com".to_string());
    let (url, _) = client
        .generate_authorization_url(&config, params)
        .await
        .unwrap();
    let parsed = Url::parse(&url).unwrap();
    assert_eq!(
        parsed
            .query_pairs()
            .filter(|(k, _)| k == "redirect_uri")
            .count(),
        1
    );
    assert!(url.contains("login_hint=user%40example.com"));
}

#[tokio::test]
async fn reserved_params_in_provider_auth_params_are_rejected() {
    let state_store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(state_store, 600, 30);
    let mut config = provider_config();
    config.auth_params.insert(
        "redirect_uri".to_string(),
        "https://evil.test/cb".to_string(),
    );

    assert!(matches!(
        client
            .generate_authorization_url(&config, HashMap::new())
            .await,
        Err(OAuth2Error::InvalidAuthorizationParam(_))
    ));
}
