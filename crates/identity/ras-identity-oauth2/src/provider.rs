//! OAuth2 identity provider implementation.

use crate::client::OAuth2Client;
use crate::config::{OAuth2Config, OAuth2ProviderConfig};
use crate::error::{OAuth2Error, OAuth2Result};
use crate::state::OAuth2StateStore;
use crate::types::AuthorizationResponse;
use async_trait::async_trait;
use ras_identity_core::{IdentityError, IdentityProvider, IdentityResult, VerifiedIdentity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// OAuth2 authentication payload for the verify method
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OAuth2AuthPayload {
    /// Start the OAuth2 flow - returns authorization URL
    StartFlow {
        provider_id: String,
        additional_params: Option<HashMap<String, String>>,
    },
    /// Complete the OAuth2 flow with callback data
    Callback {
        provider_id: String,
        /// Absent when the provider redirected back with `error` instead (I5).
        #[serde(default)]
        code: Option<String>,
        state: String,
        error: Option<String>,
        error_description: Option<String>,
        /// Session-binding value captured when the flow was started (e.g.
        /// from a cookie); required when the flow was started with one.
        #[serde(default)]
        binding: Option<String>,
    },
}

/// Response from the OAuth2 provider
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OAuth2Response {
    /// Authorization URL to redirect the user to.
    ///
    /// `binding` is the login-CSRF binding for this flow: the integrator must
    /// store it (e.g. in a cookie) and echo it back on the callback payload.
    /// `start_flow` always populates it; it is `None` only for the explicit
    /// unbound escape hatch (`start_flow_bound(.., None)`).
    AuthorizationUrl {
        url: String,
        state: String,
        binding: Option<String>,
    },
    /// Error response
    Error { message: String },
}

/// OAuth2 provider that implements IdentityProvider
#[derive(Clone)]
pub struct OAuth2Provider {
    client: OAuth2Client,
    provider_configs: HashMap<String, OAuth2ProviderConfig>,
}

impl OAuth2Provider {
    /// Build a provider from `config`.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be built or a provider configuration
    /// fails [`OAuth2ProviderConfig::validate`] (e.g. a non-`https://`
    /// endpoint without `allow_insecure_endpoints`). Use [`Self::try_new`] to
    /// get an error instead.
    pub fn new(config: OAuth2Config, state_store: Arc<dyn OAuth2StateStore>) -> Self {
        Self::try_new(config, state_store).expect("invalid OAuth2 configuration")
    }

    pub fn try_new(
        config: OAuth2Config,
        state_store: Arc<dyn OAuth2StateStore>,
    ) -> OAuth2Result<Self> {
        for provider_config in config.providers.values() {
            provider_config.validate()?;
        }
        let provider_configs = config.providers.clone();
        let client = OAuth2Client::try_new(
            state_store,
            config.state_ttl_seconds,
            config.http_timeout_seconds,
        )?;

        Ok(Self {
            client,
            provider_configs,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_client(
        provider_configs: HashMap<String, OAuth2ProviderConfig>,
        client: OAuth2Client,
    ) -> Self {
        Self {
            client,
            provider_configs,
        }
    }

    /// Add a provider configuration. Fails if the configuration does not pass
    /// [`OAuth2ProviderConfig::validate`] (I9).
    pub fn add_provider(&mut self, provider_config: OAuth2ProviderConfig) -> OAuth2Result<()> {
        provider_config.validate()?;
        self.provider_configs
            .insert(provider_config.provider_id.clone(), provider_config);
        Ok(())
    }

    /// Get a provider configuration
    fn get_provider_config(&self, provider_id: &str) -> OAuth2Result<&OAuth2ProviderConfig> {
        self.provider_configs.get(provider_id).ok_or_else(|| {
            OAuth2Error::ConfigError(format!("Provider '{}' not configured", provider_id))
        })
    }

    /// Start an OAuth2 authorization flow.
    ///
    /// Returns the authorization URL to redirect the user to, the `state`
    /// parameter, and a login-CSRF `binding` that this method generates for
    /// you. The integrator must store the binding (e.g. in a cookie) and echo
    /// it on the callback payload; a callback without it is rejected. This is
    /// the supported way to initiate a flow; `verify()` only completes one.
    ///
    /// Use [`Self::start_flow_bound`] only if you want to supply your own
    /// binding value or explicitly opt out of binding.
    pub async fn start_flow(
        &self,
        provider_id: &str,
        additional_params: Option<HashMap<String, String>>,
    ) -> OAuth2Result<OAuth2Response> {
        // Always bind by default: an unbound flow lets an attacker start a flow
        // and trick a victim into completing it, joining the attacker's app
        // session to the victim's IdP identity.
        let binding = uuid::Uuid::new_v4().to_string();
        self.start_flow_bound(provider_id, additional_params, Some(binding))
            .await
    }

    /// Start a flow with an explicit (or absent) session binding.
    ///
    /// `binding` should be an unguessable value the integrator can recover on
    /// callback (e.g. a random cookie value); the callback payload must then
    /// carry the identical value or it is rejected, preventing login CSRF.
    /// Passing `None` opts out of binding — only do this for non-browser flows
    /// where login CSRF does not apply. Prefer [`Self::start_flow`], which
    /// generates a binding for you.
    pub async fn start_flow_bound(
        &self,
        provider_id: &str,
        additional_params: Option<HashMap<String, String>>,
        binding: Option<String>,
    ) -> OAuth2Result<OAuth2Response> {
        let provider_config = self.get_provider_config(provider_id)?;
        let params = additional_params.unwrap_or_default();

        let (auth_url, state) = self
            .client
            .generate_authorization_url_bound(provider_config, params, binding.clone())
            .await?;

        info!("Started OAuth2 flow for provider: {}", provider_id);

        // Echo the binding back so the integrator can set the matching cookie.
        Ok(OAuth2Response::AuthorizationUrl {
            url: auth_url,
            state,
            binding,
        })
    }

    /// Handle the callback request
    async fn handle_callback(
        &self,
        provider_id: &str,
        code: Option<String>,
        state: String,
        error: Option<String>,
        error_description: Option<String>,
        binding: Option<String>,
    ) -> OAuth2Result<VerifiedIdentity> {
        let provider_config = self.get_provider_config(provider_id)?;

        let callback_response = AuthorizationResponse {
            code,
            state,
            error,
            error_description,
            binding,
        };

        // Exchange code for tokens
        let token_response = self
            .client
            .handle_callback(provider_config, callback_response)
            .await?;

        // Get user info
        let user_info = self
            .client
            .get_user_info(provider_config, &token_response.access_token)
            .await?;

        // Bind the userinfo response to the id_token: identity is derived from
        // userinfo, so a wrong/confused userinfo endpoint must not be able to
        // change the account when an id_token established the subject.
        // Fail closed if the id_token carries no `sub` (validate_id_token_claims
        // already requires it, but this must never silently pass). When a custom
        // `subject_field` is configured the resolved identity subject is a
        // userinfo claim trusted transitively via this `sub` binding.
        if let Some(id_token) = &token_response.id_token {
            let id_sub = crate::client::id_token_subject(id_token)?.ok_or_else(|| {
                OAuth2Error::InvalidIdToken(
                    "id_token is missing the required `sub` claim".to_string(),
                )
            })?;
            if id_sub != user_info.sub {
                return Err(OAuth2Error::InvalidIdToken(
                    "userinfo subject does not match id_token subject".to_string(),
                ));
            }
        }

        // Map user info to VerifiedIdentity
        let verified_identity =
            self.map_user_info_to_identity(provider_id, user_info, provider_config)?;

        info!(
            "Successfully verified identity for provider: {}",
            provider_id
        );

        Ok(verified_identity)
    }

    /// Map OAuth2 user info to VerifiedIdentity
    fn map_user_info_to_identity(
        &self,
        provider_id: &str,
        user_info: crate::types::UserInfoResponse,
        provider_config: &OAuth2ProviderConfig,
    ) -> OAuth2Result<VerifiedIdentity> {
        // Use custom mapping if provided
        let (subject, email, name, picture) =
            if let Some(mapping) = &provider_config.user_info_mapping {
                let subject = mapping
                    .subject_field
                    .as_ref()
                    .and_then(|field| user_info.additional_claims.get(field))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or(user_info.sub);

                let email = mapping
                    .email_field
                    .as_ref()
                    .and_then(|field| user_info.additional_claims.get(field))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or(user_info.email);

                let name = mapping
                    .name_field
                    .as_ref()
                    .and_then(|field| user_info.additional_claims.get(field))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or(user_info.name);

                let picture = mapping
                    .picture_field
                    .as_ref()
                    .and_then(|field| user_info.additional_claims.get(field))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or(user_info.picture);

                (subject, email, name, picture)
            } else {
                (
                    user_info.sub,
                    user_info.email,
                    user_info.name,
                    user_info.picture,
                )
            };

        // Build metadata
        let mut metadata = serde_json::Map::new();
        if let Some(pic) = picture {
            metadata.insert("picture".to_string(), serde_json::Value::String(pic));
        }
        if let Some(verified) = user_info.email_verified {
            metadata.insert(
                "email_verified".to_string(),
                serde_json::Value::Bool(verified),
            );
        }

        // Only allow-listed additional claims reach metadata (and thus the
        // session JWT); everything else the IdP returns is dropped (I8).
        let mut additional_claims = user_info.additional_claims;
        for claim in &provider_config.metadata_claims {
            if let Some(value) = additional_claims.remove(claim) {
                metadata.insert(claim.clone(), value);
            }
        }

        Ok(VerifiedIdentity {
            provider_id: format!("oauth2:{}", provider_id),
            subject,
            email,
            display_name: name,
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(metadata))
            },
        })
    }
}

#[async_trait]
impl IdentityProvider for OAuth2Provider {
    fn provider_id(&self) -> &str {
        "oauth2"
    }

    async fn verify(&self, auth_payload: serde_json::Value) -> IdentityResult<VerifiedIdentity> {
        // Parse the payload
        let payload: OAuth2AuthPayload =
            serde_json::from_value(auth_payload).map_err(|_| IdentityError::InvalidPayload)?;

        match payload {
            OAuth2AuthPayload::StartFlow { .. } => {
                // Flow initiation is not identity verification and has no
                // identity to return. Call `OAuth2Provider::start_flow`
                // directly to obtain the authorization URL.
                Err(IdentityError::UnsupportedMethod)
            }
            OAuth2AuthPayload::Callback {
                provider_id,
                code,
                state,
                error,
                error_description,
                binding,
            } => {
                // For callback, we complete the flow and return the verified identity
                self.handle_callback(&provider_id, code, state, error, error_description, binding)
                    .await
                    .map_err(|e| IdentityError::ProviderError(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserInfoMapping;
    use crate::state::InMemoryStateStore;

    fn google_config() -> OAuth2ProviderConfig {
        OAuth2ProviderConfig {
            provider_id: "google".to_string(),
            client_id: "test_client_id".to_string(),
            client_secret: "test_secret".to_string(),
            authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
            userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v1/userinfo".to_string()),
            issuer: None,
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
            metadata_claims: Vec::new(),
            allow_insecure_endpoints: false,
        }
    }

    fn create_test_provider() -> OAuth2Provider {
        let mut config = OAuth2Config::default();
        let google_config = google_config();
        config.providers.insert("google".to_string(), google_config);

        let state_store = Arc::new(InMemoryStateStore::new());
        OAuth2Provider::new(config, state_store)
    }

    #[tokio::test]
    async fn test_start_flow() {
        let provider = create_test_provider();

        let result = provider.start_flow("google", None).await.unwrap();
        match result {
            OAuth2Response::AuthorizationUrl {
                url,
                state,
                binding,
            } => {
                assert!(url.contains("https://accounts.google.com/o/oauth2/v2/auth"));
                assert!(url.contains("response_type=code"));
                assert!(url.contains("client_id=test_client_id"));
                assert!(!state.is_empty());
                // Default start_flow always binds.
                assert!(binding.is_some_and(|b| !b.is_empty()));
            }
            _ => panic!("Expected AuthorizationUrl response"),
        }

        // Flow initiation returns a URL, so it cannot satisfy identity verification.
        let payload = serde_json::json!({
            "type": "StartFlow",
            "provider_id": "google",
            "additional_params": null
        });
        assert!(matches!(
            provider.verify(payload).await,
            Err(IdentityError::UnsupportedMethod)
        ));
    }

    #[tokio::test]
    async fn verify_rejects_invalid_payload() {
        let provider = create_test_provider();

        let result = provider
            .verify(serde_json::json!({
                "type": "StartFlow",
                "additional_params": null
            }))
            .await;

        assert!(matches!(result, Err(IdentityError::InvalidPayload)));
    }

    #[tokio::test]
    async fn verify_reports_unknown_provider() {
        let provider = create_test_provider();

        let result = provider.start_flow("missing", None).await;

        let Err(error) = result else {
            panic!("expected error for missing provider");
        };
        assert!(
            error
                .to_string()
                .contains("Provider 'missing' not configured")
        );
    }

    #[tokio::test]
    async fn add_provider_makes_start_flow_available() {
        let state_store = Arc::new(InMemoryStateStore::new());
        let mut provider = OAuth2Provider::new(OAuth2Config::default(), state_store);
        provider.add_provider(google_config()).unwrap();

        let mut params = HashMap::new();
        params.insert("prompt".to_string(), "consent".to_string());
        let response = provider
            .start_flow("google", Some(params))
            .await
            .expect("start_flow succeeds");

        let OAuth2Response::AuthorizationUrl {
            url,
            state,
            binding,
        } = response
        else {
            panic!("expected authorization URL response");
        };
        assert!(url.contains("prompt=consent"));
        assert!(!state.is_empty());
        assert!(binding.is_some());
    }

    fn fake_id_token(payload: serde_json::Value) -> String {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("{header}.{payload}.signature")
    }

    struct FixedTransport {
        id_token: Option<String>,
        userinfo_sub: String,
    }

    #[async_trait]
    impl crate::client::OAuth2HttpTransport for FixedTransport {
        async fn exchange_code(
            &self,
            _token_endpoint: &str,
            _params: &HashMap<String, String>,
        ) -> OAuth2Result<crate::types::TokenResponse> {
            Ok(crate::types::TokenResponse {
                access_token: "access-token".to_string(),
                token_type: "Bearer".to_string(),
                expires_in: Some(3600),
                refresh_token: None,
                scope: None,
                id_token: self.id_token.clone(),
            })
        }

        async fn get_user_info(
            &self,
            _userinfo_endpoint: &str,
            _access_token: &str,
        ) -> OAuth2Result<crate::types::UserInfoResponse> {
            Ok(crate::types::UserInfoResponse {
                sub: self.userinfo_sub.clone(),
                email: None,
                email_verified: None,
                name: None,
                given_name: None,
                family_name: None,
                picture: None,
                locale: None,
                additional_claims: HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn callback_rejects_userinfo_subject_mismatch_with_id_token() {
        use crate::state::{OAuth2State, OAuth2StateStore};

        let mut config = google_config();
        config.issuer = Some("https://issuer.test".to_string());
        let exp = chrono::Utc::now().timestamp() + 600;
        // Build an id_token that passes iss/aud/exp/nonce so validation reaches
        // the subject cross-check. It carries sub = "id-token-subject".
        let id_token = fake_id_token(serde_json::json!({
            "iss": "https://issuer.test",
            "aud": config.client_id,
            "sub": "id-token-subject",
            "exp": exp,
            "nonce": "nonce-abc",
        }));

        // userinfo returns a DIFFERENT subject than the id_token (confused deputy).
        let transport = Arc::new(FixedTransport {
            id_token: Some(id_token),
            userinfo_sub: "userinfo-subject".to_string(),
        });
        let state_store = Arc::new(InMemoryStateStore::new());
        let client =
            crate::client::OAuth2Client::with_http_transport(state_store.clone(), 600, transport);
        let mut providers = HashMap::new();
        providers.insert("google".to_string(), config.clone());
        let provider = OAuth2Provider::with_client(providers, client);

        // Store a flow state with a known nonce + binding so the id_token matches.
        let state = OAuth2State::new("google".to_string(), config.redirect_uri.clone(), None, 600)
            .with_nonce("nonce-abc".to_string())
            .with_binding(Some("binding-xyz".to_string()));
        let state_param = state.state.clone();
        state_store.store(state).await.unwrap();

        let result = provider
            .handle_callback(
                "google",
                Some("code".to_string()),
                state_param,
                None,
                None,
                Some("binding-xyz".to_string()),
            )
            .await;

        assert!(
            matches!(result, Err(OAuth2Error::InvalidIdToken(_))),
            "subject mismatch must be rejected, got {result:?}"
        );
    }

    #[test]
    fn test_user_info_mapping() {
        let provider = create_test_provider();
        let provider_config = provider.get_provider_config("google").unwrap();

        let user_info = crate::types::UserInfoResponse {
            sub: "123456".to_string(),
            email: Some("user@example.com".to_string()),
            email_verified: Some(true),
            name: Some("Test User".to_string()),
            given_name: Some("Test".to_string()),
            family_name: Some("User".to_string()),
            picture: Some("https://example.com/picture.jpg".to_string()),
            locale: Some("en".to_string()),
            additional_claims: HashMap::new(),
        };

        let identity = provider
            .map_user_info_to_identity("google", user_info, provider_config)
            .unwrap();

        assert_eq!(identity.provider_id, "oauth2:google");
        assert_eq!(identity.subject, "123456");
        assert_eq!(identity.email, Some("user@example.com".to_string()));
        assert_eq!(identity.display_name, Some("Test User".to_string()));

        let metadata = identity.metadata.unwrap();
        assert_eq!(metadata["picture"], "https://example.com/picture.jpg");
        assert_eq!(metadata["email_verified"].as_bool(), Some(true));
    }

    #[test]
    fn custom_user_info_mapping_prefers_additional_claims_and_preserves_metadata() {
        let provider = create_test_provider();
        let mut provider_config = google_config();
        provider_config.metadata_claims = vec!["tenant".to_string()];
        provider_config.user_info_mapping = Some(UserInfoMapping {
            subject_field: Some("external_id".to_string()),
            email_field: Some("mail".to_string()),
            name_field: Some("display".to_string()),
            picture_field: Some("avatar".to_string()),
        });

        let mut additional_claims = HashMap::new();
        additional_claims.insert(
            "external_id".to_string(),
            serde_json::Value::String("mapped-subject".to_string()),
        );
        additional_claims.insert(
            "mail".to_string(),
            serde_json::Value::String("mapped@example.com".to_string()),
        );
        additional_claims.insert(
            "display".to_string(),
            serde_json::Value::String("Mapped User".to_string()),
        );
        additional_claims.insert(
            "avatar".to_string(),
            serde_json::Value::String("https://example.com/avatar.png".to_string()),
        );
        additional_claims.insert(
            "tenant".to_string(),
            serde_json::Value::String("engineering".to_string()),
        );

        let identity = provider
            .map_user_info_to_identity(
                "google",
                crate::types::UserInfoResponse {
                    sub: "fallback-subject".to_string(),
                    email: Some("fallback@example.com".to_string()),
                    email_verified: Some(false),
                    name: Some("Fallback User".to_string()),
                    given_name: None,
                    family_name: None,
                    picture: None,
                    locale: None,
                    additional_claims,
                },
                &provider_config,
            )
            .unwrap();

        assert_eq!(identity.subject, "mapped-subject");
        assert_eq!(identity.email.as_deref(), Some("mapped@example.com"));
        assert_eq!(identity.display_name.as_deref(), Some("Mapped User"));

        let metadata = identity.metadata.unwrap();
        assert_eq!(metadata["picture"], "https://example.com/avatar.png");
        assert_eq!(metadata["email_verified"].as_bool(), Some(false));
        assert_eq!(metadata["tenant"], "engineering");
        // Claims consumed by the mapping are not re-exported unless allow-listed.
        assert!(metadata.get("external_id").is_none());
        assert!(metadata.get("mail").is_none());
    }

    #[test]
    fn i8_only_allow_listed_additional_claims_reach_metadata() {
        let provider = create_test_provider();
        let mut provider_config = google_config();
        provider_config.metadata_claims = vec!["hd".to_string(), "absent".to_string()];

        let mut additional_claims = HashMap::new();
        additional_claims.insert("hd".to_string(), serde_json::json!("example.com"));
        additional_claims.insert("groups".to_string(), serde_json::json!(["admins"]));
        additional_claims.insert("is_admin".to_string(), serde_json::json!(true));

        let user_info = crate::types::UserInfoResponse {
            sub: "subject".to_string(),
            email: None,
            email_verified: None,
            name: None,
            given_name: None,
            family_name: None,
            picture: None,
            locale: None,
            additional_claims,
        };

        let identity = provider
            .map_user_info_to_identity("google", user_info.clone(), &provider_config)
            .unwrap();
        let metadata = identity.metadata.unwrap();
        assert_eq!(metadata["hd"], "example.com");
        assert!(metadata.get("groups").is_none());
        assert!(metadata.get("is_admin").is_none());
        assert!(metadata.get("absent").is_none());

        // Default (empty allow-list): nothing extra is propagated.
        let identity = provider
            .map_user_info_to_identity("google", user_info, &google_config())
            .unwrap();
        assert!(identity.metadata.is_none());
    }

    #[test]
    fn i9_provider_construction_rejects_insecure_endpoints() {
        let mut insecure = google_config();
        insecure.token_endpoint = "http://oauth.test/token".to_string();

        let config = OAuth2Config::new().add_provider(insecure.clone());
        let state_store: Arc<dyn OAuth2StateStore> = Arc::new(InMemoryStateStore::new());
        assert!(matches!(
            OAuth2Provider::try_new(config, state_store.clone()),
            Err(OAuth2Error::ConfigError(_))
        ));

        let mut provider = OAuth2Provider::new(OAuth2Config::default(), state_store.clone());
        assert!(matches!(
            provider.add_provider(insecure.clone()),
            Err(OAuth2Error::ConfigError(_))
        ));
        assert!(provider.get_provider_config("google").is_err());

        insecure.allow_insecure_endpoints = true;
        provider.add_provider(insecure.clone()).unwrap();
        assert!(provider.get_provider_config("google").is_ok());
        assert!(
            OAuth2Provider::try_new(OAuth2Config::new().add_provider(insecure), state_store)
                .is_ok()
        );
    }

    #[test]
    #[should_panic(expected = "invalid OAuth2 configuration")]
    fn i9_provider_new_panics_on_insecure_endpoints() {
        let mut insecure = google_config();
        insecure.authorization_endpoint = "http://oauth.test/authorize".to_string();
        let state_store = Arc::new(InMemoryStateStore::new());
        let _ = OAuth2Provider::new(OAuth2Config::new().add_provider(insecure), state_store);
    }

    #[test]
    fn user_info_mapping_omits_empty_metadata() {
        let provider = create_test_provider();
        let provider_config = provider.get_provider_config("google").unwrap();

        let identity = provider
            .map_user_info_to_identity(
                "google",
                crate::types::UserInfoResponse {
                    sub: "subject-only".to_string(),
                    email: None,
                    email_verified: None,
                    name: None,
                    given_name: None,
                    family_name: None,
                    picture: None,
                    locale: None,
                    additional_claims: HashMap::new(),
                },
                provider_config,
            )
            .unwrap();

        assert_eq!(identity.subject, "subject-only");
        assert!(identity.email.is_none());
        assert!(identity.display_name.is_none());
        assert!(identity.metadata.is_none());
    }
}
