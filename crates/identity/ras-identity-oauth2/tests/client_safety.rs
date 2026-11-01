//! Guards the low-level `OAuth2Client` (https-only endpoints, no redirect following, browser-bound flows by default) and redacted `Debug` on secret-bearing types.

use axum::{
    Router,
    http::HeaderMap,
    routing::{get, post},
};
use ras_identity_oauth2::{
    AuthorizationResponse, InMemoryStateStore, OAuth2AuthPayload, OAuth2Client, OAuth2Error,
    OAuth2ProviderConfig, OAuth2Response, OAuth2State, OAuth2StateStore, TokenResponse,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

fn config(endpoint: String) -> OAuth2ProviderConfig {
    OAuth2ProviderConfig {
        provider_id: "review".into(),
        client_id: "review-client".into(),
        client_secret: "review-sensitive-secret".into(),
        authorization_endpoint: "https://idp.example/auth".into(),
        token_endpoint: endpoint,
        userinfo_endpoint: None,
        issuer: None,
        redirect_uri: "https://app.example/callback".into(),
        scopes: vec![],
        auth_params: HashMap::new(),
        use_pkce: true,
        user_info_mapping: None,
        metadata_claims: vec![],
        allow_insecure_endpoints: false,
    }
}

/// Serve `app` on a loopback port and return its base URL.
async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, task)
}

#[tokio::test]
async fn public_client_must_reject_insecure_provider_configuration() {
    let store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(store.clone(), 600, 5);
    let mut provider = config("http://idp.example/token".into());
    provider.authorization_endpoint = "http://idp.example/auth".into();
    provider.userinfo_endpoint = Some("http://idp.example/userinfo".into());
    assert!(provider.validate().is_err());

    assert!(matches!(
        client
            .generate_authorization_url(&provider, HashMap::new())
            .await,
        Err(OAuth2Error::ConfigError(_))
    ));

    let state = OAuth2State::new("review".into(), provider.redirect_uri.clone(), None, 600);
    let state_parameter = state.state.clone();
    store.store(state).await.unwrap();
    let callback = client
        .handle_callback(
            &provider,
            AuthorizationResponse {
                code: Some("review-code".into()),
                state: state_parameter,
                error: None,
                error_description: None,
                binding: None,
            },
        )
        .await;
    assert!(matches!(callback, Err(OAuth2Error::ConfigError(_))));

    assert!(matches!(
        client.get_user_info(&provider, "review-token").await,
        Err(OAuth2Error::ConfigError(_))
    ));
}

#[tokio::test]
async fn default_authorization_flow_must_be_bound_to_the_initiating_browser() {
    let store = Arc::new(InMemoryStateStore::new());
    let client = OAuth2Client::new(store.clone(), 600, 5);
    let provider = config("https://idp.example/token".into());

    let redirect = client
        .generate_authorization_url(&provider, HashMap::new())
        .await
        .unwrap();
    let binding = redirect
        .binding
        .expect("the default flow returns a binding");
    let stored = store.retrieve(&redirect.state).await.unwrap();
    assert_eq!(stored.binding.as_deref(), Some(binding.as_str()));

    // A callback from a browser that does not hold the binding is rejected
    // before any token request is made.
    for presented in [None, Some("another-browser".to_string())] {
        let redirect = client
            .generate_authorization_url(&provider, HashMap::new())
            .await
            .unwrap();
        let result = client
            .handle_callback(
                &provider,
                AuthorizationResponse {
                    code: Some("review-code".into()),
                    state: redirect.state,
                    error: None,
                    error_description: None,
                    binding: presented,
                },
            )
            .await;
        assert!(
            matches!(result, Err(OAuth2Error::InvalidState)),
            "{result:?}"
        );
    }
}

#[tokio::test]
async fn token_exchange_must_not_forward_credentials_to_a_redirect_target() {
    let captured = Arc::new(Mutex::new(String::new()));
    let capture = captured.clone();
    let (capture_base, capture_task) = serve(Router::new().route(
        "/capture",
        post(move |body: String| {
            let capture = capture.clone();
            async move {
                *capture.lock().unwrap() = body;
                axum::Json(
                    serde_json::json!({"access_token": "review-token", "token_type": "Bearer"}),
                )
            }
        }),
    ))
    .await;
    let capture_url = format!("{capture_base}/capture");
    let (token_base, token_task) = serve(Router::new().route(
        "/token",
        post(move || {
            let target = capture_url.clone();
            async move { axum::response::Redirect::temporary(&target) }
        }),
    ))
    .await;

    let mut provider = config(format!("{token_base}/token"));
    // Loopback HTTP isolates the redirect behaviour from the https-only check.
    provider.allow_insecure_endpoints = true;
    let store = Arc::new(InMemoryStateStore::new());
    let state = OAuth2State::new("review".into(), provider.redirect_uri.clone(), None, 600);
    let state_parameter = state.state.clone();
    store.store(state).await.unwrap();
    let client = OAuth2Client::new(store, 600, 5);
    let result = client
        .handle_callback(
            &provider,
            AuthorizationResponse {
                code: Some("review-code".into()),
                state: state_parameter,
                error: None,
                error_description: None,
                binding: None,
            },
        )
        .await;
    token_task.abort();
    capture_task.abort();
    assert!(
        captured.lock().unwrap().is_empty(),
        "the redirect target received the replayed token request body"
    );
    assert!(
        matches!(result, Err(OAuth2Error::TokenExchangeFailed(_))),
        "token endpoint redirects must fail closed, got {result:?}"
    );
}

#[tokio::test]
async fn userinfo_request_must_not_follow_redirects() {
    let reached = Arc::new(Mutex::new(false));
    let flag = reached.clone();
    let (capture_base, capture_task) = serve(Router::new().route(
        "/capture",
        get(move |_headers: HeaderMap| {
            let flag = flag.clone();
            async move {
                *flag.lock().unwrap() = true;
                axum::Json(serde_json::json!({"sub": "someone-else"}))
            }
        }),
    ))
    .await;
    let capture_url = format!("{capture_base}/capture");
    let (userinfo_base, userinfo_task) = serve(Router::new().route(
        "/userinfo",
        get(move || {
            let target = capture_url.clone();
            async move { axum::response::Redirect::temporary(&target) }
        }),
    ))
    .await;

    let mut provider = config("https://idp.example/token".into());
    provider.userinfo_endpoint = Some(format!("{userinfo_base}/userinfo"));
    provider.allow_insecure_endpoints = true;
    let client = OAuth2Client::new(Arc::new(InMemoryStateStore::new()), 600, 5);
    let result = client.get_user_info(&provider, "review-token").await;
    userinfo_task.abort();
    capture_task.abort();
    assert!(!*reached.lock().unwrap(), "the redirect was followed");
    assert!(
        matches!(result, Err(OAuth2Error::UserInfoFailed(_))),
        "userinfo redirects must fail closed, got {result:?}"
    );
}

#[test]
fn token_response_debug_redacts_tokens() {
    let response = TokenResponse {
        access_token: "secret-access".into(),
        token_type: "Bearer".into(),
        expires_in: Some(3600),
        refresh_token: Some("secret-refresh".into()),
        scope: Some("openid".into()),
        id_token: Some("secret-id-token".into()),
    };
    let debug = format!("{response:?}");
    for secret in ["secret-access", "secret-refresh", "secret-id-token"] {
        assert!(!debug.contains(secret), "{secret} leaked: {debug}");
    }
    assert!(debug.contains("Bearer") && debug.contains("3600"));
}

#[test]
fn authorization_response_debug_redacts_code_and_binding() {
    let response = AuthorizationResponse {
        code: Some("secret-code".into()),
        state: "visible-state".into(),
        error: None,
        error_description: None,
        binding: Some("secret-binding".into()),
    };
    let debug = format!("{response:?}");
    assert!(!debug.contains("secret-code"), "{debug}");
    assert!(!debug.contains("secret-binding"), "{debug}");
    assert!(debug.contains("visible-state"));
}

#[test]
fn oauth2_state_debug_redacts_code_verifier_and_binding() {
    let state = OAuth2State::new(
        "review".into(),
        "https://app.example/callback".into(),
        Some("secret-verifier".into()),
        600,
    )
    .with_binding(Some("secret-binding".into()));
    let debug = format!("{state:?}");
    assert!(!debug.contains("secret-verifier"), "{debug}");
    assert!(!debug.contains("secret-binding"), "{debug}");
    assert!(debug.contains(&state.state) && debug.contains("review"));
}

#[test]
fn oauth2_response_debug_redacts_binding() {
    let response = OAuth2Response::AuthorizationUrl {
        url: "https://idp.example/auth?state=visible-state".into(),
        state: "visible-state".into(),
        binding: Some("secret-binding".into()),
    };
    let debug = format!("{response:?}");
    assert!(!debug.contains("secret-binding"), "{debug}");
    assert!(debug.contains("visible-state"));
}

#[test]
fn oauth2_auth_payload_debug_redacts_code_and_binding() {
    let payload = OAuth2AuthPayload::Callback {
        provider_id: "review".into(),
        code: Some("secret-code".into()),
        state: "visible-state".into(),
        error: None,
        error_description: None,
        binding: Some("secret-binding".into()),
    };
    let debug = format!("{payload:?}");
    assert!(!debug.contains("secret-code"), "{debug}");
    assert!(!debug.contains("secret-binding"), "{debug}");
    assert!(debug.contains("visible-state"));
}
