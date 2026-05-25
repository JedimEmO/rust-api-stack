//! OAuth2 client implementation with PKCE support.

use crate::config::OAuth2ProviderConfig;
use crate::error::{OAuth2Error, OAuth2Result};
use crate::state::{OAuth2State, OAuth2StateStore};
use crate::types::{AuthorizationResponse, TokenResponse, UserInfoResponse};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, thread_rng};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use url::Url;

#[async_trait::async_trait]
pub(crate) trait OAuth2HttpTransport: Send + Sync {
    async fn exchange_code(
        &self,
        token_endpoint: &str,
        params: &HashMap<String, String>,
    ) -> OAuth2Result<TokenResponse>;

    async fn get_user_info(
        &self,
        userinfo_endpoint: &str,
        access_token: &str,
    ) -> OAuth2Result<UserInfoResponse>;
}

#[derive(Clone)]
struct ReqwestOAuth2HttpTransport {
    client: Client,
}

#[async_trait::async_trait]
impl OAuth2HttpTransport for ReqwestOAuth2HttpTransport {
    async fn exchange_code(
        &self,
        token_endpoint: &str,
        params: &HashMap<String, String>,
    ) -> OAuth2Result<TokenResponse> {
        let response = self.client.post(token_endpoint).form(params).send().await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Token exchange failed: {}", error_text);
            return Err(OAuth2Error::TokenExchangeFailed(error_text));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| OAuth2Error::InvalidTokenResponse(e.to_string()))?;

        info!("Successfully exchanged code for tokens");
        Ok(token_response)
    }

    async fn get_user_info(
        &self,
        userinfo_endpoint: &str,
        access_token: &str,
    ) -> OAuth2Result<UserInfoResponse> {
        let response = self
            .client
            .get(userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("User info request failed: {}", error_text);
            return Err(OAuth2Error::UserInfoFailed(error_text));
        }

        let user_info: UserInfoResponse = response
            .json()
            .await
            .map_err(|e| OAuth2Error::InvalidUserInfoResponse(e.to_string()))?;

        debug!(
            "Successfully retrieved user info for subject: {}",
            user_info.sub
        );
        Ok(user_info)
    }
}

/// PKCE code challenge and verifier
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
}

impl Default for PkceChallenge {
    fn default() -> Self {
        Self::new()
    }
}

impl PkceChallenge {
    /// Generate a new PKCE challenge
    pub fn new() -> Self {
        let code_verifier = Self::generate_code_verifier();
        let code_challenge = Self::generate_code_challenge(&code_verifier);

        Self {
            code_verifier,
            code_challenge,
            code_challenge_method: "S256".to_string(),
        }
    }

    fn generate_code_verifier() -> String {
        let mut rng = thread_rng();
        let bytes: Vec<u8> = (0..64).map(|_| rng.r#gen::<u8>()).collect();
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn generate_code_challenge(verifier: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let result = hasher.finalize();
        URL_SAFE_NO_PAD.encode(result)
    }
}

/// OAuth2 client for handling authorization flows
#[derive(Clone)]
pub struct OAuth2Client {
    http_transport: Arc<dyn OAuth2HttpTransport>,
    state_store: Arc<dyn OAuth2StateStore>,
    state_ttl_seconds: u64,
}

impl OAuth2Client {
    pub fn new(
        state_store: Arc<dyn OAuth2StateStore>,
        state_ttl_seconds: u64,
        http_timeout_seconds: u64,
    ) -> Self {
        match Self::try_new(
            Arc::clone(&state_store),
            state_ttl_seconds,
            http_timeout_seconds,
        ) {
            Ok(client) => client,
            Err(error) => {
                error!(
                    "Failed to create configured OAuth2 HTTP client; using default client: {}",
                    error
                );
                Self {
                    http_transport: Arc::new(ReqwestOAuth2HttpTransport {
                        client: Client::new(),
                    }),
                    state_store,
                    state_ttl_seconds,
                }
            }
        }
    }

    pub fn try_new(
        state_store: Arc<dyn OAuth2StateStore>,
        state_ttl_seconds: u64,
        http_timeout_seconds: u64,
    ) -> OAuth2Result<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(http_timeout_seconds))
            .build()?;

        Ok(Self {
            http_transport: Arc::new(ReqwestOAuth2HttpTransport {
                client: http_client,
            }),
            state_store,
            state_ttl_seconds,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_http_transport(
        state_store: Arc<dyn OAuth2StateStore>,
        state_ttl_seconds: u64,
        http_transport: Arc<dyn OAuth2HttpTransport>,
    ) -> Self {
        Self {
            http_transport,
            state_store,
            state_ttl_seconds,
        }
    }

    #[cfg(test)]
    pub fn state_store(&self) -> &Arc<dyn OAuth2StateStore> {
        &self.state_store
    }

    /// Generate authorization URL for a provider
    pub async fn generate_authorization_url(
        &self,
        provider_config: &OAuth2ProviderConfig,
        additional_params: HashMap<String, String>,
    ) -> OAuth2Result<(String, String)> {
        let mut url = Url::parse(&provider_config.authorization_endpoint)?;

        // Generate PKCE if enabled
        let pkce = if provider_config.use_pkce {
            Some(PkceChallenge::new())
        } else {
            None
        };

        // Create and store state
        let state = OAuth2State::new(
            provider_config.provider_id.clone(),
            provider_config.redirect_uri.clone(),
            pkce.as_ref().map(|p| p.code_verifier.clone()),
            self.state_ttl_seconds,
        );

        let state_param = state.state.clone();
        self.state_store.store(state).await?;

        // Build query parameters
        let mut params = url.query_pairs_mut();
        params.append_pair("response_type", "code");
        params.append_pair("client_id", &provider_config.client_id);
        params.append_pair("redirect_uri", &provider_config.redirect_uri);
        params.append_pair("state", &state_param);

        // Add scopes
        if !provider_config.scopes.is_empty() {
            params.append_pair("scope", &provider_config.scopes.join(" "));
        }

        // Add PKCE parameters
        if let Some(pkce) = &pkce {
            params.append_pair("code_challenge", &pkce.code_challenge);
            params.append_pair("code_challenge_method", &pkce.code_challenge_method);
        }

        // Add provider-specific parameters
        for (key, value) in &provider_config.auth_params {
            params.append_pair(key, value);
        }

        // Add additional parameters from the request
        for (key, value) in &additional_params {
            params.append_pair(key, value);
        }

        drop(params);

        let auth_url = url.to_string();
        debug!(
            "Generated authorization URL for provider {}",
            provider_config.provider_id
        );

        Ok((auth_url, state_param))
    }

    /// Handle OAuth2 callback and exchange code for tokens
    pub async fn handle_callback(
        &self,
        provider_config: &OAuth2ProviderConfig,
        callback_response: AuthorizationResponse,
    ) -> OAuth2Result<TokenResponse> {
        // Verify state
        let state = self.state_store.retrieve(&callback_response.state).await?;

        if state.provider_id != provider_config.provider_id {
            return Err(OAuth2Error::InvalidState);
        }

        // Check for errors in callback
        if let Some(error) = &callback_response.error {
            let error_desc = callback_response
                .error_description
                .as_deref()
                .unwrap_or("No description");
            return Err(OAuth2Error::CallbackError(format!(
                "{}: {}",
                error, error_desc
            )));
        }

        // Exchange authorization code for tokens
        let token_response = self
            .exchange_code(
                provider_config,
                &callback_response.code,
                state.code_verifier.as_deref(),
            )
            .await?;

        Ok(token_response)
    }

    /// Exchange authorization code for tokens
    async fn exchange_code(
        &self,
        provider_config: &OAuth2ProviderConfig,
        code: &str,
        code_verifier: Option<&str>,
    ) -> OAuth2Result<TokenResponse> {
        let mut params = HashMap::new();
        params.insert("grant_type".to_string(), "authorization_code".to_string());
        params.insert("code".to_string(), code.to_string());
        params.insert("client_id".to_string(), provider_config.client_id.clone());
        params.insert(
            "client_secret".to_string(),
            provider_config.client_secret.clone(),
        );
        params.insert(
            "redirect_uri".to_string(),
            provider_config.redirect_uri.clone(),
        );

        // Add PKCE verifier if present
        if let Some(verifier) = code_verifier {
            params.insert("code_verifier".to_string(), verifier.to_string());
        }

        self.http_transport
            .exchange_code(&provider_config.token_endpoint, &params)
            .await
    }

    /// Get user info using access token
    pub async fn get_user_info(
        &self,
        provider_config: &OAuth2ProviderConfig,
        access_token: &str,
    ) -> OAuth2Result<UserInfoResponse> {
        let userinfo_endpoint = provider_config.userinfo_endpoint.as_ref().ok_or_else(|| {
            OAuth2Error::ConfigError("User info endpoint not configured".to_string())
        })?;

        self.http_transport
            .get_user_info(userinfo_endpoint, access_token)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{InMemoryStateStore, OAuth2StateStore};
    use std::sync::Mutex;

    struct RecordingTransport {
        token_requests: Mutex<Vec<(String, HashMap<String, String>)>>,
        userinfo_requests: Mutex<Vec<(String, String)>>,
    }

    impl RecordingTransport {
        fn new() -> Self {
            Self {
                token_requests: Mutex::new(Vec::new()),
                userinfo_requests: Mutex::new(Vec::new()),
            }
        }

        fn token_requests(&self) -> Vec<(String, HashMap<String, String>)> {
            self.token_requests
                .lock()
                .expect("token request lock")
                .clone()
        }

        fn userinfo_requests(&self) -> Vec<(String, String)> {
            self.userinfo_requests
                .lock()
                .expect("userinfo request lock")
                .clone()
        }
    }

    #[async_trait::async_trait]
    impl OAuth2HttpTransport for RecordingTransport {
        async fn exchange_code(
            &self,
            token_endpoint: &str,
            params: &HashMap<String, String>,
        ) -> OAuth2Result<TokenResponse> {
            self.token_requests
                .lock()
                .expect("token request lock")
                .push((token_endpoint.to_string(), params.clone()));
            Ok(TokenResponse {
                access_token: "access-token".to_string(),
                token_type: "Bearer".to_string(),
                expires_in: Some(3600),
                refresh_token: None,
                scope: None,
                id_token: None,
            })
        }

        async fn get_user_info(
            &self,
            userinfo_endpoint: &str,
            access_token: &str,
        ) -> OAuth2Result<UserInfoResponse> {
            self.userinfo_requests
                .lock()
                .expect("userinfo request lock")
                .push((userinfo_endpoint.to_string(), access_token.to_string()));
            Ok(UserInfoResponse {
                sub: "user-1".to_string(),
                email: Some("user@example.com".to_string()),
                email_verified: Some(true),
                name: Some("Test User".to_string()),
                given_name: None,
                family_name: None,
                picture: None,
                locale: None,
                additional_claims: HashMap::new(),
            })
        }
    }

    fn provider_config() -> OAuth2ProviderConfig {
        OAuth2ProviderConfig {
            provider_id: "test_provider".to_string(),
            client_id: "test_client_id".to_string(),
            client_secret: "test_secret".to_string(),
            authorization_endpoint: "https://example.com/auth".to_string(),
            token_endpoint: "https://example.com/token".to_string(),
            userinfo_endpoint: Some("https://example.com/userinfo".to_string()),
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec!["openid".to_string(), "email".to_string()],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
        }
    }

    fn client_with_transport(
        state_store: Arc<InMemoryStateStore>,
        transport: Arc<RecordingTransport>,
    ) -> OAuth2Client {
        OAuth2Client::with_http_transport(state_store, 600, transport)
    }

    #[test]
    fn test_pkce_generation() {
        let pkce1 = PkceChallenge::new();
        let pkce2 = PkceChallenge::new();

        // Verifiers should be different
        assert_ne!(pkce1.code_verifier, pkce2.code_verifier);

        // Challenges should be different
        assert_ne!(pkce1.code_challenge, pkce2.code_challenge);

        // Method should be S256
        assert_eq!(pkce1.code_challenge_method, "S256");

        // Verify the challenge is correctly generated
        let expected_challenge = PkceChallenge::generate_code_challenge(&pkce1.code_verifier);
        assert_eq!(pkce1.code_challenge, expected_challenge);
    }

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
                    code: "auth-code".to_string(),
                    state,
                    error: None,
                    error_description: None,
                },
            )
            .await
            .expect_err("provider mismatch should reject callback");

        assert!(matches!(error, OAuth2Error::InvalidState));
        assert!(transport.token_requests().is_empty());
    }

    #[tokio::test]
    async fn handle_callback_returns_provider_callback_error_without_transport_call() {
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
                    code: "ignored-code".to_string(),
                    state,
                    error: Some("access_denied".to_string()),
                    error_description: Some("user denied consent".to_string()),
                },
            )
            .await
            .expect_err("provider callback error should be surfaced");

        match error {
            OAuth2Error::CallbackError(message) => {
                assert_eq!(message, "access_denied: user denied consent");
            }
            other => panic!("expected callback error, got {other:?}"),
        }
        assert!(transport.token_requests().is_empty());
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
                    code: "auth-code".to_string(),
                    state,
                    error: None,
                    error_description: None,
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
}
