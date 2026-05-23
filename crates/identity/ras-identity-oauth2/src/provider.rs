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
        code: String,
        state: String,
        error: Option<String>,
        error_description: Option<String>,
    },
}

/// Response from the OAuth2 provider
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OAuth2Response {
    /// Authorization URL to redirect the user to
    AuthorizationUrl { url: String, state: String },
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
    pub fn new(config: OAuth2Config, state_store: Arc<dyn OAuth2StateStore>) -> Self {
        let provider_configs = config.providers.clone();
        let client = OAuth2Client::new(
            state_store,
            config.state_ttl_seconds,
            config.http_timeout_seconds,
        );

        Self {
            client,
            provider_configs,
        }
    }

    pub fn try_new(
        config: OAuth2Config,
        state_store: Arc<dyn OAuth2StateStore>,
    ) -> OAuth2Result<Self> {
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

    /// Add a provider configuration
    pub fn add_provider(&mut self, provider_config: OAuth2ProviderConfig) {
        self.provider_configs
            .insert(provider_config.provider_id.clone(), provider_config);
    }

    /// Get a provider configuration
    fn get_provider_config(&self, provider_id: &str) -> OAuth2Result<&OAuth2ProviderConfig> {
        self.provider_configs.get(provider_id).ok_or_else(|| {
            OAuth2Error::ConfigError(format!("Provider '{}' not configured", provider_id))
        })
    }

    /// Handle the start flow request
    async fn handle_start_flow(
        &self,
        provider_id: &str,
        additional_params: Option<HashMap<String, String>>,
    ) -> OAuth2Result<OAuth2Response> {
        let provider_config = self.get_provider_config(provider_id)?;
        let params = additional_params.unwrap_or_default();

        let (auth_url, state) = self
            .client
            .generate_authorization_url(provider_config, params)
            .await?;

        info!("Started OAuth2 flow for provider: {}", provider_id);

        Ok(OAuth2Response::AuthorizationUrl {
            url: auth_url,
            state,
        })
    }

    /// Handle the callback request
    async fn handle_callback(
        &self,
        provider_id: &str,
        code: String,
        state: String,
        error: Option<String>,
        error_description: Option<String>,
    ) -> OAuth2Result<VerifiedIdentity> {
        let provider_config = self.get_provider_config(provider_id)?;

        let callback_response = AuthorizationResponse {
            code,
            state,
            error,
            error_description,
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

        // Add all additional claims to metadata
        for (key, value) in user_info.additional_claims {
            metadata.insert(key, value);
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
            OAuth2AuthPayload::StartFlow {
                provider_id,
                additional_params,
            } => {
                // For start flow, we return an error with the authorization URL
                let response = self
                    .handle_start_flow(&provider_id, additional_params)
                    .await
                    .map_err(|e| IdentityError::ProviderError(e.to_string()))?;

                // Return the response as a provider error (client should handle this specially)
                let response_json =
                    serde_json::to_string(&response).map_err(IdentityError::SerializationError)?;

                Err(IdentityError::ProviderError(response_json))
            }
            OAuth2AuthPayload::Callback {
                provider_id,
                code,
                state,
                error,
                error_description,
            } => {
                // For callback, we complete the flow and return the verified identity
                self.handle_callback(&provider_id, code, state, error, error_description)
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
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
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

        let payload = serde_json::json!({
            "type": "StartFlow",
            "provider_id": "google",
            "additional_params": null
        });

        let result = provider.verify(payload).await;

        // Start flow returns an error with the authorization URL
        assert!(result.is_err());

        if let Err(IdentityError::ProviderError(response_json)) = result {
            let response: OAuth2Response = serde_json::from_str(&response_json).unwrap();
            match response {
                OAuth2Response::AuthorizationUrl { url, state } => {
                    assert!(url.contains("https://accounts.google.com/o/oauth2/v2/auth"));
                    assert!(url.contains("response_type=code"));
                    assert!(url.contains("client_id=test_client_id"));
                    assert!(!state.is_empty());
                }
                _ => panic!("Expected AuthorizationUrl response"),
            }
        } else {
            panic!("Expected ProviderError");
        }
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

        let result = provider
            .verify(serde_json::json!({
                "type": "StartFlow",
                "provider_id": "missing"
            }))
            .await;

        let Err(IdentityError::ProviderError(message)) = result else {
            panic!("expected provider error for missing provider");
        };
        assert!(message.contains("Provider 'missing' not configured"));
    }

    #[tokio::test]
    async fn add_provider_makes_start_flow_available() {
        let state_store = Arc::new(InMemoryStateStore::new());
        let mut provider = OAuth2Provider::new(OAuth2Config::default(), state_store);
        provider.add_provider(google_config());

        let result = provider
            .verify(serde_json::json!({
                "type": "StartFlow",
                "provider_id": "google",
                "additional_params": {
                    "prompt": "consent"
                }
            }))
            .await;

        let Err(IdentityError::ProviderError(response_json)) = result else {
            panic!("expected authorization URL response encoded as provider error");
        };
        let response: OAuth2Response = serde_json::from_str(&response_json).unwrap();
        let OAuth2Response::AuthorizationUrl { url, state } = response else {
            panic!("expected authorization URL response");
        };
        assert!(url.contains("prompt=consent"));
        assert!(!state.is_empty());
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
