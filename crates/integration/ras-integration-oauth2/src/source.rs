//! The OAuth2 token source: refresh-token and client-credentials flows.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ras_integration_core::{
    GrantStore, IntegrationError, SecretString, TokenFamily, TokenLease, TokenRequest, TokenSource,
    TokenSubject, UserGrant,
};
use ras_transport_core::http::Method;
use ras_transport_core::{HttpTransport, TransportRequest};
use serde::Deserialize;

use crate::config::OAuth2ProviderConfig;

#[derive(Deserialize)]
struct TokenEndpointSuccess {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct TokenEndpointFailure {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// A [`TokenSource`] backed by an external OAuth2/OIDC provider.
///
/// - [`TokenSubject::User`]: refresh-token flow against a stored
///   [`UserGrant`]. Requested scopes are subset-checked against the grant;
///   broader requests return [`IntegrationError::ConsentRequired`]. Rotated
///   refresh tokens are persisted back to the [`GrantStore`] before the
///   lease is returned, and persistence failures surface as errors.
/// - [`TokenSubject::Service`]: client-credentials flow (requires a client
///   secret). The request audience is forwarded as the `audience` parameter
///   for providers that support it.
/// - [`TokenSubject::ServiceAccount`]: not supported by external providers
///   in v1; fails closed.
pub struct OAuth2TokenSource {
    config: OAuth2ProviderConfig,
    transport: Arc<dyn HttpTransport>,
    grants: Arc<dyn GrantStore>,
}

impl OAuth2TokenSource {
    pub fn new(
        config: OAuth2ProviderConfig,
        transport: Arc<dyn HttpTransport>,
        grants: Arc<dyn GrantStore>,
    ) -> Result<Self, IntegrationError> {
        config.validate()?;
        Ok(Self {
            config,
            transport,
            grants,
        })
    }

    async fn token_endpoint_request(
        &self,
        integration_id: &str,
        params: Vec<(&str, String)>,
        subject_user: Option<&str>,
        requested_scopes: &[String],
    ) -> Result<TokenEndpointSuccess, IntegrationError> {
        let body =
            serde_urlencoded::to_string(&params).map_err(|err| IntegrationError::Provider {
                integration_id: integration_id.to_string(),
                reason: format!("failed to encode token request: {err}"),
            })?;

        let request = TransportRequest::new(Method::POST, &self.config.token_endpoint)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(ras_transport_core::RequestBody::Bytes(body.into()));

        let response =
            self.transport
                .execute(request)
                .await
                .map_err(|err| IntegrationError::Provider {
                    integration_id: integration_id.to_string(),
                    reason: format!("token endpoint unreachable: {err}"),
                })?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|err| IntegrationError::Provider {
                integration_id: integration_id.to_string(),
                reason: format!("failed to read token response: {err}"),
            })?;

        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|err| IntegrationError::Provider {
                integration_id: integration_id.to_string(),
                reason: format!("malformed token response: {err}"),
            });
        }

        // 4xx: interpret the standard OAuth error body. Anything else (or an
        // unparseable body) is a provider failure.
        if status.is_client_error()
            && let Ok(failure) = serde_json::from_slice::<TokenEndpointFailure>(&bytes)
        {
            return Err(match (failure.error.as_str(), subject_user) {
                // The stored grant is dead (revoked/expired): the user must
                // re-consent.
                ("invalid_grant", Some(user_id)) => IntegrationError::ConsentRequired {
                    integration_id: integration_id.to_string(),
                    user_id: user_id.to_string(),
                    missing_scopes: requested_scopes.to_vec(),
                },
                _ => IntegrationError::Denied {
                    integration_id: integration_id.to_string(),
                    reason: match failure.error_description {
                        Some(description) => format!("{}: {description}", failure.error),
                        None => failure.error,
                    },
                },
            });
        }

        Err(IntegrationError::Provider {
            integration_id: integration_id.to_string(),
            reason: format!("token endpoint returned status {status}"),
        })
    }

    fn lease_from(&self, success: TokenEndpointSuccess) -> TokenLease {
        TokenLease {
            access_token: SecretString::new(success.access_token),
            expires_at: success
                .expires_in
                .map(|seconds| Utc::now() + Duration::seconds(seconds)),
            scopes: success
                .scope
                .map(|scope| scope.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }

    async fn user_refresh_flow(
        &self,
        request: &TokenRequest,
        user_id: &str,
    ) -> Result<TokenLease, IntegrationError> {
        let grant = self
            .grants
            .get_user_grant(&request.integration_id, user_id)
            .await?
            .ok_or_else(|| IntegrationError::ConsentRequired {
                integration_id: request.integration_id.clone(),
                user_id: user_id.to_string(),
                missing_scopes: request.scopes.clone(),
            })?;

        let missing: Vec<String> = request
            .scopes
            .iter()
            .filter(|scope| !grant.scopes.contains(scope))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(IntegrationError::ConsentRequired {
                integration_id: request.integration_id.clone(),
                user_id: user_id.to_string(),
                missing_scopes: missing,
            });
        }

        let mut params = vec![
            ("grant_type", "refresh_token".to_string()),
            (
                "refresh_token",
                grant.refresh_token.expose_secret().to_string(),
            ),
            ("client_id", self.config.client_id.clone()),
        ];
        if let Some(secret) = &self.config.client_secret {
            params.push(("client_secret", secret.expose_secret().to_string()));
        }
        if !request.scopes.is_empty() {
            params.push(("scope", request.scopes.join(" ")));
        }

        let success = self
            .token_endpoint_request(
                &request.integration_id,
                params,
                Some(user_id),
                &request.scopes,
            )
            .await?;

        // Refresh-token rotation: persist the new grant before handing out
        // the lease. A failed save surfaces as an error — silently losing a
        // rotated refresh token would brick the stored grant.
        if let Some(rotated) = &success.refresh_token {
            self.grants
                .put_user_grant(UserGrant {
                    integration_id: grant.integration_id.clone(),
                    user_id: grant.user_id.clone(),
                    refresh_token: SecretString::new(rotated.clone()),
                    scopes: grant.scopes.clone(),
                })
                .await?;
        }

        Ok(self.lease_from(success))
    }

    async fn client_credentials_flow(
        &self,
        request: &TokenRequest,
    ) -> Result<TokenLease, IntegrationError> {
        let secret = self.config.client_secret.as_ref().ok_or_else(|| {
            IntegrationError::InvalidConfig(format!(
                "integration {:?}: client-credentials flow requires a client secret",
                request.integration_id
            ))
        })?;

        let mut params = vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", self.config.client_id.clone()),
            ("client_secret", secret.expose_secret().to_string()),
        ];
        if !request.scopes.is_empty() {
            params.push(("scope", request.scopes.join(" ")));
        }
        if let Some(audience) = &request.audience {
            params.push(("audience", audience.clone()));
        }

        let success = self
            .token_endpoint_request(&request.integration_id, params, None, &request.scopes)
            .await?;
        Ok(self.lease_from(success))
    }
}

#[async_trait]
impl TokenSource for OAuth2TokenSource {
    fn family(&self) -> TokenFamily {
        TokenFamily::OAuth2
    }

    async fn issue_token(&self, request: &TokenRequest) -> Result<TokenLease, IntegrationError> {
        match &request.subject {
            TokenSubject::User { user_id } => self.user_refresh_flow(request, user_id).await,
            TokenSubject::Service => self.client_credentials_flow(request).await,
            TokenSubject::ServiceAccount { .. } => Err(IntegrationError::Denied {
                integration_id: request.integration_id.clone(),
                reason: "external OAuth2 providers do not support RAS service-account \
                         principals; use a Service (client-credentials) or User subject"
                    .to_string(),
            }),
        }
    }
}
