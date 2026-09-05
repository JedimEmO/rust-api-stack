//! OAuth2 client implementation with PKCE support.
mod authorization;
mod id_token;
mod pkce;
mod transport;

use crate::config::OAuth2ProviderConfig;
use crate::error::{OAuth2Error, OAuth2Result};
use crate::state::OAuth2StateStore;
use crate::types::{AuthorizationResponse, TokenResponse, UserInfoResponse};
pub(crate) use id_token::{id_token_subject, validate_id_token_claims};
pub use pkce::PkceChallenge;
use std::{collections::HashMap, sync::Arc};
use subtle::ConstantTimeEq;
use tracing::warn;
pub(crate) use transport::OAuth2HttpTransport;
use transport::ReqwestOAuth2HttpTransport;

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
        let http_transport = ReqwestOAuth2HttpTransport::new(http_timeout_seconds)?;

        Ok(Self {
            http_transport: Arc::new(http_transport),
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

#[cfg(test)]
mod tests;
