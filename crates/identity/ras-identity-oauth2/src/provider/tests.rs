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
        OAuth2Provider::try_new(OAuth2Config::new().add_provider(insecure), state_store).is_ok()
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
