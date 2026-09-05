use super::*;
use crate::state::{InMemoryStateStore, OAuth2StateStore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::sync::Mutex;
use url::Url;

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
        issuer: None,
        redirect_uri: "http://localhost:3000/callback".to_string(),
        scopes: vec!["openid".to_string(), "email".to_string()],
        auth_params: HashMap::new(),
        use_pkce: true,
        user_info_mapping: None,
        metadata_claims: Vec::new(),
        allow_insecure_endpoints: false,
    }
}

fn client_with_transport(
    state_store: Arc<InMemoryStateStore>,
    transport: Arc<RecordingTransport>,
) -> OAuth2Client {
    OAuth2Client::with_http_transport(state_store, 600, transport)
}

fn fake_id_token(payload: serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
    format!("{header}.{payload}.signature")
}

mod authorization;
mod callback;
mod id_token;
mod userinfo;
