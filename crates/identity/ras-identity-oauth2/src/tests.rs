//! Integration and security tests for OAuth2 implementation.

#[cfg(test)]
mod integration_tests {
    use crate::client::{OAuth2Client, OAuth2HttpTransport};
    use crate::error::{OAuth2Error, OAuth2Result};
    use crate::provider::OAuth2Response;
    use crate::{
        InMemoryStateStore, OAuth2Config, OAuth2Provider, OAuth2ProviderConfig, TokenResponse,
        UserInfoResponse,
    };
    use async_trait::async_trait;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::{Form, Json, Router};
    use axum_test::TestServer;
    use ras_identity_core::IdentityProvider;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct AxumOAuth2Transport {
        server: Arc<TestServer>,
    }

    #[async_trait]
    impl OAuth2HttpTransport for AxumOAuth2Transport {
        async fn exchange_code(
            &self,
            token_endpoint: &str,
            params: &HashMap<String, String>,
        ) -> OAuth2Result<TokenResponse> {
            let path = endpoint_path(token_endpoint)?;
            let response = self.server.post(&path).form(params).await;

            if !response.status_code().is_success() {
                return Err(OAuth2Error::TokenExchangeFailed(response.text()));
            }

            serde_json::from_slice(response.as_bytes())
                .map_err(|e| OAuth2Error::InvalidTokenResponse(e.to_string()))
        }

        async fn get_user_info(
            &self,
            userinfo_endpoint: &str,
            access_token: &str,
        ) -> OAuth2Result<UserInfoResponse> {
            let path = endpoint_path(userinfo_endpoint)?;
            let response = self
                .server
                .get(&path)
                .authorization_bearer(access_token)
                .await;

            if !response.status_code().is_success() {
                return Err(OAuth2Error::UserInfoFailed(response.text()));
            }

            serde_json::from_slice(response.as_bytes())
                .map_err(|e| OAuth2Error::InvalidUserInfoResponse(e.to_string()))
        }
    }

    fn endpoint_path(endpoint: &str) -> OAuth2Result<String> {
        let url = url::Url::parse(endpoint)?;
        let mut path = url.path().to_string();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        Ok(path)
    }

    fn setup_mock_oauth_server(router: Router) -> (Arc<TestServer>, OAuth2ProviderConfig) {
        let server = TestServer::builder()
            .mock_transport()
            .build(router)
            .expect("mock transport OAuth2 server should build");

        let provider_config = OAuth2ProviderConfig {
            provider_id: "mock_provider".to_string(),
            client_id: "mock_client_id".to_string(),
            client_secret: "mock_secret".to_string(),
            authorization_endpoint: "http://oauth.test/authorize".to_string(),
            token_endpoint: "http://oauth.test/token".to_string(),
            userinfo_endpoint: Some("http://oauth.test/userinfo".to_string()),
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec!["openid".to_string(), "email".to_string()],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
        };

        (Arc::new(server), provider_config)
    }

    fn client_with_server(
        state_store: Arc<InMemoryStateStore>,
        server: Arc<TestServer>,
    ) -> OAuth2Client {
        OAuth2Client::with_http_transport(
            state_store,
            600,
            Arc::new(AxumOAuth2Transport { server }),
        )
    }

    fn provider_with_server(
        provider_config: OAuth2ProviderConfig,
        state_store: Arc<InMemoryStateStore>,
        server: Arc<TestServer>,
    ) -> OAuth2Provider {
        let client = client_with_server(state_store, server);
        let mut provider_configs = HashMap::new();
        provider_configs.insert("mock_provider".to_string(), provider_config);
        OAuth2Provider::with_client(provider_configs, client)
    }

    fn success_oauth_router() -> Router {
        Router::new()
            .route("/token", post(token_success))
            .route("/userinfo", get(userinfo_success))
    }

    async fn token_success(Form(form): Form<HashMap<String, String>>) -> Response {
        let required = [
            ("grant_type", "authorization_code"),
            ("code", "mock_auth_code"),
            ("client_id", "mock_client_id"),
            ("client_secret", "mock_secret"),
            ("redirect_uri", "http://localhost:3000/callback"),
        ];

        for (key, expected) in required {
            if form.get(key).map(String::as_str) != Some(expected) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_request",
                        "error_description": format!("missing or invalid {key}")
                    })),
                )
                    .into_response();
            }
        }

        if !form.contains_key("code_verifier") {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_request",
                    "error_description": "missing PKCE verifier"
                })),
            )
                .into_response();
        }

        Json(serde_json::json!({
            "access_token": "mock_access_token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "mock_refresh_token",
            "scope": "openid email"
        }))
        .into_response()
    }

    async fn userinfo_success(headers: HeaderMap) -> Response {
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());

        if auth != Some("Bearer mock_access_token") {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "invalid_token"
                })),
            )
                .into_response();
        }

        Json(serde_json::json!({
            "sub": "12345",
            "email": "test@example.com",
            "email_verified": true,
            "name": "Test User",
            "picture": "https://example.com/photo.jpg"
        }))
        .into_response()
    }

    fn token_error_router() -> Router {
        Router::new().route("/token", post(token_error))
    }

    async fn token_error() -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "The provided authorization code is invalid"
            })),
        )
            .into_response()
    }

    fn malformed_token_router() -> Router {
        Router::new().route("/token", post(malformed_token_response))
    }

    async fn malformed_token_response() -> Response {
        (StatusCode::OK, "not json").into_response()
    }

    #[tokio::test]
    async fn test_full_oauth2_flow() {
        let (server, provider_config) = setup_mock_oauth_server(success_oauth_router());
        let state_store = Arc::new(InMemoryStateStore::new());
        let provider = provider_with_server(provider_config, state_store, server);

        // Start OAuth2 flow via the typed API
        let start_result = provider.start_flow("mock_provider", None).await.unwrap();
        let auth_url = match start_result {
            OAuth2Response::AuthorizationUrl { url, state } => {
                assert!(url.contains("/authorize"));
                assert!(url.contains("response_type=code"));
                assert!(url.contains("code_challenge"));
                state
            }
            _ => panic!("Expected authorization URL"),
        };

        // StartFlow payloads are no longer routed through verify()
        let start_payload = serde_json::json!({
            "type": "StartFlow",
            "provider_id": "mock_provider"
        });
        assert!(matches!(
            provider.verify(start_payload).await,
            Err(ras_identity_core::IdentityError::UnsupportedMethod)
        ));

        // Simulate callback
        let callback_payload = serde_json::json!({
            "type": "Callback",
            "provider_id": "mock_provider",
            "code": "mock_auth_code",
            "state": auth_url
        });

        let callback_result = provider.verify(callback_payload).await;
        assert!(callback_result.is_ok());

        let identity = callback_result.unwrap();
        assert_eq!(identity.provider_id, "oauth2:mock_provider");
        assert_eq!(identity.subject, "12345");
        assert_eq!(identity.email, Some("test@example.com".to_string()));
        assert_eq!(identity.display_name, Some("Test User".to_string()));
    }

    #[tokio::test]
    async fn test_oauth2_error_handling() {
        let state_store = Arc::new(InMemoryStateStore::new());
        let config = OAuth2Config::default();
        let provider = OAuth2Provider::new(config, state_store);

        // Test invalid provider
        let result = provider.start_flow("nonexistent", None).await;
        assert!(result.is_err());

        // Test callback with invalid state
        let payload = serde_json::json!({
            "type": "Callback",
            "provider_id": "google",
            "code": "test_code",
            "state": "invalid_state"
        });

        let result = provider.verify(payload).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pkce_security() {
        use crate::client::PkceChallenge;
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        use sha2::{Digest, Sha256};

        let pkce = PkceChallenge::new();

        // Verify that code_challenge is SHA256(code_verifier)
        let mut hasher = Sha256::new();
        hasher.update(pkce.code_verifier.as_bytes());
        let expected_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        assert_eq!(pkce.code_challenge, expected_challenge);
        assert_eq!(pkce.code_challenge_method, "S256");

        // Verify code_verifier meets PKCE requirements (43-128 chars)
        assert!(pkce.code_verifier.len() >= 43);
        assert!(pkce.code_verifier.len() <= 128);
    }

    #[tokio::test]
    async fn test_state_parameter_security() {
        let state_store = Arc::new(InMemoryStateStore::new());
        let mut config = OAuth2Config::default();

        let provider_config = OAuth2ProviderConfig {
            provider_id: "test".to_string(),
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            authorization_endpoint: "https://example.com/auth".to_string(),
            token_endpoint: "https://example.com/token".to_string(),
            userinfo_endpoint: None,
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec![],
            auth_params: HashMap::new(),
            use_pkce: false,
            user_info_mapping: None,
        };

        config.providers.insert("test".to_string(), provider_config);
        let provider = OAuth2Provider::new(config, state_store.clone());

        // Generate two authorization URLs
        let state1 = extract_state(provider.start_flow("test", None).await);
        let state2 = extract_state(provider.start_flow("test", None).await);

        // States should be unique
        assert_ne!(state1, state2);

        // States should be cryptographically random (UUIDs)
        assert_eq!(state1.len(), 36); // UUID v4 format
        assert_eq!(state2.len(), 36);
    }

    fn extract_state(result: crate::OAuth2Result<OAuth2Response>) -> String {
        match result.expect("start_flow succeeds") {
            OAuth2Response::AuthorizationUrl { state, .. } => state,
            _ => panic!("Expected authorization URL"),
        }
    }

    #[tokio::test]
    async fn test_concurrent_state_handling() {
        use tokio::task;

        let state_store = Arc::new(InMemoryStateStore::new());
        let client = crate::OAuth2Client::new(state_store.clone(), 600, 30);

        let provider_config = OAuth2ProviderConfig {
            provider_id: "test".to_string(),
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            authorization_endpoint: "https://example.com/auth".to_string(),
            token_endpoint: "https://example.com/token".to_string(),
            userinfo_endpoint: None,
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec![],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
        };

        // Spawn multiple concurrent authorization requests
        let mut handles = vec![];
        for _ in 0..10 {
            let client = client.clone();
            let config = provider_config.clone();
            let handle = task::spawn(async move {
                client
                    .generate_authorization_url(&config, HashMap::new())
                    .await
            });
            handles.push(handle);
        }

        // Collect all states
        let mut states = vec![];
        for handle in handles {
            let (_, state) = handle.await.unwrap().unwrap();
            states.push(state);
        }

        // All states should be unique
        let unique_states: std::collections::HashSet<_> = states.iter().collect();
        assert_eq!(unique_states.len(), states.len());
    }

    #[tokio::test]
    async fn test_token_exchange_error_cases() {
        let (server, provider_config) = setup_mock_oauth_server(token_error_router());

        let state_store = Arc::new(InMemoryStateStore::new());
        let client = client_with_server(state_store, server);

        // Store a valid state first
        let state = crate::state::OAuth2State::new(
            "mock_provider".to_string(),
            provider_config.redirect_uri.clone(),
            Some("test_verifier".to_string()),
            600,
        );
        client.state_store().store(state.clone()).await.unwrap();

        let callback = crate::types::AuthorizationResponse {
            code: "invalid_code".to_string(),
            state: state.state,
            error: None,
            error_description: None,
        };

        let result = client.handle_callback(&provider_config, callback).await;
        assert!(matches!(
            result,
            Err(OAuth2Error::TokenExchangeFailed(message))
                if message.contains("invalid_grant")
        ));

        // Test 2: Malformed token response
        let (server, provider_config) = setup_mock_oauth_server(malformed_token_router());
        let state_store = Arc::new(InMemoryStateStore::new());
        let client = client_with_server(state_store, server);

        let state2 = crate::state::OAuth2State::new(
            "mock_provider".to_string(),
            provider_config.redirect_uri.clone(),
            None,
            600,
        );
        client.state_store().store(state2.clone()).await.unwrap();

        let callback2 = crate::types::AuthorizationResponse {
            code: "test_code".to_string(),
            state: state2.state,
            error: None,
            error_description: None,
        };

        let result = client.handle_callback(&provider_config, callback2).await;
        assert!(matches!(result, Err(OAuth2Error::InvalidTokenResponse(_))));
    }
}
