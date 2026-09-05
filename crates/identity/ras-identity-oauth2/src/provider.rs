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
mod tests;
