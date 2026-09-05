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
use subtle::ConstantTimeEq;
use tracing::{debug, error, info, warn};
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
        let response = self
            .client
            .post(token_endpoint)
            .form(params)
            .send()
            .await
            .map_err(log_upstream_error)?;

        if !response.status().is_success() {
            // Never log or propagate the raw provider response body — it can
            // contain tokens or other sensitive material. Status only.
            let status = response.status();
            error!("Token exchange failed with status {}", status);
            return Err(OAuth2Error::TokenExchangeFailed(format!(
                "token endpoint returned status {status}"
            )));
        }

        let token_response: TokenResponse = response.json().await.map_err(|e| {
            // reqwest decode errors embed the request URL; keep that in the log only (I6).
            warn!(error = %e, "token endpoint returned an undecodable response");
            OAuth2Error::InvalidTokenResponse("undecodable token response".to_string())
        })?;

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
            .await
            .map_err(log_upstream_error)?;

        if !response.status().is_success() {
            // Status only; the raw body may echo the bearer token.
            let status = response.status();
            error!("User info request failed with status {}", status);
            return Err(OAuth2Error::UserInfoFailed(format!(
                "userinfo endpoint returned status {status}"
            )));
        }

        let user_info: UserInfoResponse = response.json().await.map_err(|e| {
            warn!(error = %e, "userinfo endpoint returned an undecodable response");
            OAuth2Error::InvalidUserInfoResponse("undecodable userinfo response".to_string())
        })?;

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

/// Reserved OAuth/OIDC query parameters that the library sets itself. Neither
/// `provider_config.auth_params` nor caller-supplied `additional_params` may
/// override them — many providers honour the last occurrence of a duplicated
/// query parameter, so an injected second `redirect_uri` / `state` / PKCE value
/// would be an authorization-code-theft or CSRF vector.
const RESERVED_AUTH_PARAMS: &[&str] = &[
    "response_type",
    "client_id",
    "client_secret",
    "redirect_uri",
    "state",
    "nonce",
    "scope",
    "code_challenge",
    "code_challenge_method",
    "grant_type",
    "code",
    "code_verifier",
    // OIDC request objects (Core §6): parameters inside a `request` /
    // `request_uri` JWT take precedence over the query parameters we set, so
    // permitting them would re-establish the exact override primitive this
    // denylist removes (e.g. silently dropping PKCE or overriding state/nonce).
    "request",
    "request_uri",
    // Response delivery / audience controls a caller must not influence.
    "response_mode",
    "resource",
    "audience",
    "id_token_hint",
];

/// Reject any key that collides (case-insensitively) with a reserved parameter.
fn reject_reserved_params<'a, I>(keys: I, source: &str) -> OAuth2Result<()>
where
    I: IntoIterator<Item = &'a String>,
{
    for key in keys {
        if RESERVED_AUTH_PARAMS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(key))
        {
            return Err(OAuth2Error::InvalidAuthorizationParam(format!(
                "{source} may not set the reserved parameter `{key}`"
            )));
        }
    }
    Ok(())
}

/// OAuth2 client for handling authorization flows
#[derive(Clone)]
pub struct OAuth2Client {
    http_transport: Arc<dyn OAuth2HttpTransport>,
    state_store: Arc<dyn OAuth2StateStore>,
    state_ttl_seconds: u64,
}

impl OAuth2Client {
    /// Create a client with bounded HTTP timeouts.
    ///
    /// # Panics
    /// Panics if the HTTP client cannot be built. Use [`Self::try_new`] to
    /// handle construction errors.
    pub fn new(
        state_store: Arc<dyn OAuth2StateStore>,
        state_ttl_seconds: u64,
        http_timeout_seconds: u64,
    ) -> Self {
        Self::try_new(state_store, state_ttl_seconds, http_timeout_seconds)
            .expect("failed to build OAuth2 HTTP client")
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
        self.generate_authorization_url_bound(provider_config, additional_params, None)
            .await
    }

    /// Generate an authorization URL bound to the initiating browser session.
    ///
    /// `binding` should be an unguessable value the integrator can recover on
    /// callback (e.g. a random cookie value); the callback must then present
    /// the identical value, preventing login CSRF where an attacker tricks a
    /// victim into completing the attacker's flow.
    pub async fn generate_authorization_url_bound(
        &self,
        provider_config: &OAuth2ProviderConfig,
        additional_params: HashMap<String, String>,
        binding: Option<String>,
    ) -> OAuth2Result<(String, String)> {
        // Reject reserved-parameter overrides before doing any work.
        reject_reserved_params(provider_config.auth_params.keys(), "provider auth_params")?;
        reject_reserved_params(additional_params.keys(), "additional_params")?;

        let mut url = Url::parse(&provider_config.authorization_endpoint)?;

        // Generate PKCE if enabled
        let pkce = if provider_config.use_pkce {
            Some(PkceChallenge::new())
        } else {
            None
        };

        // OIDC nonce: echoed back inside the id_token and verified on
        // callback, binding the token to this authorization request.
        let nonce = uuid::Uuid::new_v4().to_string();

        // Create and store state
        let state = OAuth2State::new(
            provider_config.provider_id.clone(),
            provider_config.redirect_uri.clone(),
            pkce.as_ref().map(|p| p.code_verifier.clone()),
            self.state_ttl_seconds,
        )
        .with_nonce(nonce.clone())
        .with_binding(binding);

        let state_param = state.state.clone();
        self.state_store.store(state).await?;

        // Build query parameters
        let mut params = url.query_pairs_mut();
        params.append_pair("response_type", "code");
        params.append_pair("client_id", &provider_config.client_id);
        params.append_pair("redirect_uri", &provider_config.redirect_uri);
        params.append_pair("state", &state_param);
        params.append_pair("nonce", &nonce);

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

        // When the flow was bound to a browser session, the callback must
        // present the identical binding value (login-CSRF guard). Compared in
        // constant time so the binding cannot be recovered byte-by-byte (I7).
        if let Some(expected) = &state.binding
            && !binding_matches(expected, callback_response.binding.as_deref())
        {
            return Err(OAuth2Error::InvalidState);
        }

        // Check for errors in callback. Only the standardized error code is
        // surfaced; the free-text description stays in the server log (I5).
        if let Some(error) = &callback_response.error {
            warn!(
                provider = %provider_config.provider_id,
                error = %error,
                error_description = callback_response.error_description.as_deref().unwrap_or(""),
                "OAuth2 provider returned an error on callback"
            );
            return Err(OAuth2Error::ProviderDenied {
                error: error.clone(),
            });
        }

        let Some(code) = callback_response.code.as_deref() else {
            return Err(OAuth2Error::InvalidCallback);
        };

        // Exchange authorization code for tokens
        let token_response = self
            .exchange_code(provider_config, code, state.code_verifier.as_deref())
            .await?;

        // Validate id_token claims when the provider returned one. The token
        // arrived directly from the token endpoint over TLS, which OIDC Core
        // §3.1.3.7 permits in place of signature validation for the code
        // flow — but iss / aud / exp / nonce are still mandatory checks.
        if let Some(id_token) = &token_response.id_token {
            validate_id_token_claims(provider_config, id_token, state.nonce.as_deref())?;
        }

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

/// Claims checked on an id_token returned by the token endpoint.
#[derive(serde::Deserialize)]
struct IdTokenClaims {
    iss: Option<String>,
    sub: Option<String>,
    aud: Option<serde_json::Value>,
    /// Authorized party — required to equal `client_id` when `aud` has multiple
    /// entries (OIDC Core §3.1.3.7 / §2).
    azp: Option<String>,
    exp: Option<i64>,
    nonce: Option<String>,
}

/// Subject (`sub`) claim of an id_token, used to bind it to the userinfo
/// response so a confused-deputy userinfo cannot change the account.
pub(crate) fn id_token_subject(id_token: &str) -> OAuth2Result<Option<String>> {
    Ok(decode_id_token_claims(id_token)?.sub)
}

fn decode_id_token_claims(id_token: &str) -> OAuth2Result<IdTokenClaims> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| OAuth2Error::InvalidIdToken("malformed JWT".to_string()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| OAuth2Error::InvalidIdToken("invalid base64 payload".to_string()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| OAuth2Error::InvalidIdToken("invalid JSON payload".to_string()))
}

/// Validate the id_token issuer, audience, expiry, subject, and expected nonce.
/// Accepting an id_token requires a configured provider issuer.
///
/// The signature is not verified: the token was received directly from the
/// token endpoint over TLS, which OIDC Core §3.1.3.7 permits as a substitute
/// for signature validation in the authorization-code flow.
/// Log a transport-level failure at `warn` (the `reqwest::Error` carries the
/// request URL) and hand back the fixed-message error variant (I6).
fn log_upstream_error(error: reqwest::Error) -> OAuth2Error {
    warn!(error = %error, "upstream OAuth2 request failed");
    OAuth2Error::HttpError(error)
}

/// Constant-time comparison of the stored session binding against the value
/// presented on callback (I7). A missing callback value never matches.
fn binding_matches(expected: &str, presented: Option<&str>) -> bool {
    match presented {
        Some(presented) => {
            // `ct_eq` on slices short-circuits on length, but the length of the
            // binding is not secret (it is a UUID or caller-chosen value).
            expected.as_bytes().ct_eq(presented.as_bytes()).into()
        }
        None => false,
    }
}

pub(crate) fn validate_id_token_claims(
    provider_config: &OAuth2ProviderConfig,
    id_token: &str,
    expected_nonce: Option<&str>,
) -> OAuth2Result<()> {
    let claims = decode_id_token_claims(id_token)?;

    // Issuer is fail-closed: an id_token whose issuer is unverified cannot be
    // trusted to identify the account, so accepting one without a configured
    // `issuer` is refused rather than silently skipped.
    let Some(expected_issuer) = &provider_config.issuer else {
        return Err(OAuth2Error::InvalidIdToken(
            "provider `issuer` must be configured to accept id_tokens".to_string(),
        ));
    };
    if claims.iss.as_deref() != Some(expected_issuer.as_str()) {
        return Err(OAuth2Error::InvalidIdToken(format!(
            "issuer mismatch: expected {expected_issuer}"
        )));
    }

    let client_id = provider_config.client_id.as_str();
    let audience_matches = match &claims.aud {
        Some(serde_json::Value::String(aud)) => aud == client_id,
        Some(serde_json::Value::Array(auds)) => {
            let contains = auds.iter().any(|aud| aud.as_str() == Some(client_id));
            if !contains {
                false
            } else if auds.len() > 1 {
                // Multiple audiences: `azp` must be present and equal client_id.
                claims.azp.as_deref() == Some(client_id)
            } else {
                true
            }
        }
        _ => false,
    };
    if !audience_matches {
        return Err(OAuth2Error::InvalidIdToken(
            "audience does not include this client (or azp mismatch for multi-audience token)"
                .to_string(),
        ));
    }

    match claims.exp {
        Some(exp) if exp > chrono::Utc::now().timestamp() => {}
        _ => {
            return Err(OAuth2Error::InvalidIdToken(
                "token expired or missing exp".to_string(),
            ));
        }
    }

    if let Some(expected) = expected_nonce
        && claims.nonce.as_deref() != Some(expected)
    {
        return Err(OAuth2Error::InvalidIdToken("nonce mismatch".to_string()));
    }

    // `sub` is REQUIRED by OIDC Core §2. Refuse an id_token without it so the
    // userinfo <-> id_token subject binding cannot silently no-op on a
    // token that carries no subject.
    match claims.sub.as_deref() {
        Some(sub) if !sub.trim().is_empty() => {}
        _ => {
            return Err(OAuth2Error::InvalidIdToken(
                "id_token is missing the required `sub` claim".to_string(),
            ));
        }
    }

    Ok(())
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

    fn fake_id_token(payload: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("{header}.{payload}.signature")
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

    #[test]
    fn id_token_claim_validation_covers_iss_aud_exp_and_nonce() {
        let mut config = provider_config();
        config.issuer = Some("https://issuer.test".to_string());
        let exp = chrono::Utc::now().timestamp() + 600;

        let good = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "sub": "subject-1",
            "aud": "test_client_id",
            "exp": exp,
            "nonce": "nonce-1",
        }));
        assert!(validate_id_token_claims(&config, &good, Some("nonce-1")).is_ok());

        // An otherwise-valid id_token with no `sub` is rejected: the
        // userinfo binding must never run against an absent subject.
        let no_sub = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "aud": "test_client_id",
            "exp": exp,
            "nonce": "nonce-1",
        }));
        assert!(validate_id_token_claims(&config, &no_sub, Some("nonce-1")).is_err());

        // A single-element aud array containing this client is fine.
        let aud_single_array = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "sub": "subject-1",
            "aud": ["test_client_id"],
            "exp": exp,
        }));
        assert!(validate_id_token_claims(&config, &aud_single_array, None).is_ok());

        // Multi-audience token requires azp == client_id.
        let aud_array_with_azp = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "sub": "subject-1",
            "aud": ["other", "test_client_id"],
            "azp": "test_client_id",
            "exp": exp,
        }));
        assert!(validate_id_token_claims(&config, &aud_array_with_azp, None).is_ok());

        // Multi-audience token WITHOUT a matching azp is rejected.
        let aud_array_no_azp = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "aud": ["other", "test_client_id"],
            "exp": exp,
        }));
        assert!(validate_id_token_claims(&config, &aud_array_no_azp, None).is_err());

        let bad_iss = fake_id_token(serde_json::json!({
            "iss": "https://evil.test", "aud": "test_client_id", "exp": exp,
        }));
        assert!(matches!(
            validate_id_token_claims(&config, &bad_iss, None),
            Err(OAuth2Error::InvalidIdToken(_))
        ));

        let bad_aud = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test", "aud": "someone_else", "exp": exp,
        }));
        assert!(validate_id_token_claims(&config, &bad_aud, None).is_err());

        let expired = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "aud": "test_client_id",
            "exp": chrono::Utc::now().timestamp() - 10,
        }));
        assert!(validate_id_token_claims(&config, &expired, None).is_err());

        let wrong_nonce = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "aud": "test_client_id",
            "exp": exp,
            "nonce": "other",
        }));
        assert!(validate_id_token_claims(&config, &wrong_nonce, Some("nonce-1")).is_err());

        assert!(validate_id_token_claims(&config, "garbage", None).is_err());
    }

    #[test]
    fn id_token_without_configured_issuer_is_rejected() {
        // issuer is None on provider_config() -> fail closed.
        let config = provider_config();
        assert!(config.issuer.is_none());
        let exp = chrono::Utc::now().timestamp() + 600;
        let token = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "aud": "test_client_id",
            "exp": exp,
        }));
        assert!(matches!(
            validate_id_token_claims(&config, &token, None),
            Err(OAuth2Error::InvalidIdToken(_))
        ));
    }

    #[test]
    fn id_token_subject_extracts_sub() {
        let token = fake_id_token(serde_json::json!({ "sub": "subject-123" }));
        assert_eq!(
            id_token_subject(&token).unwrap().as_deref(),
            Some("subject-123")
        );
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

    #[tokio::test]
    async fn handle_callback_enforces_session_binding() {
        let state_store = Arc::new(InMemoryStateStore::new());
        let transport = Arc::new(RecordingTransport::new());
        let client = client_with_transport(state_store.clone(), transport.clone());
        let config = provider_config();

        // A callback missing the binding is rejected before any token
        // exchange (and the one-use state is burned).
        let (_, state) = client
            .generate_authorization_url_bound(
                &config,
                HashMap::new(),
                Some("cookie-123".to_string()),
            )
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
            .generate_authorization_url_bound(
                &config,
                HashMap::new(),
                Some("cookie-123".to_string()),
            )
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
            .generate_authorization_url_bound(
                &config,
                HashMap::new(),
                Some("cookie-123".to_string()),
            )
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
}
