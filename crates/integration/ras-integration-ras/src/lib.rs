//! RAS-internal token source for the outbound token framework.
//!
//! [`RasInternalTokenSource`] implements
//! [`ras_integration_core::TokenSource`] by requesting internal service
//! tokens from the RAS authorization control plane
//! (`ras-authorization-core`). It holds **no signing keys and never mints
//! locally**: every lease is the result of a successful authorization and
//! issuance decision by the authority.
//!
//! Two authority transports:
//!
//! - [`EmbeddedAuthority`] calls a [`TokenIssuer`] in-process (the embedded
//!   deployment preset).
//! - [`HttpAuthority`] posts to a central authority's `POST /auth/token`
//!   route through `ras-transport-core`.
//!
//! v1 implements service-as-service issuance: the source authenticates with
//! its own service identity proof and only accepts
//! [`TokenSubject::Service`] requests. User-delegated and service-account
//! requests fail closed (the request/cache model already distinguishes
//! them, so adding delegation later is additive).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ras_authorization_core::{AuthzError, InternalTokenRequest, ServiceIdentityProof, TokenIssuer};
use ras_integration_core::{
    IntegrationError, SecretString, TokenFamily, TokenLease, TokenRequest, TokenSource,
    TokenSubject,
};
use ras_transport_core::http::Method;
use ras_transport_core::{HttpTransport, TransportRequest};

/// The authority-side result of an issuance request.
#[derive(Debug, serde::Deserialize)]
pub struct AuthorityTokenResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// How the token source reaches the RAS authority.
#[async_trait]
pub trait AuthorityClient: Send + Sync {
    async fn issue(
        &self,
        integration_id: &str,
        request: InternalTokenRequest,
    ) -> Result<AuthorityTokenResponse, IntegrationError>;
}

/// In-process authority access for the embedded deployment preset.
pub struct EmbeddedAuthority {
    issuer: Arc<TokenIssuer>,
}

impl EmbeddedAuthority {
    pub fn new(issuer: Arc<TokenIssuer>) -> Self {
        Self { issuer }
    }
}

#[async_trait]
impl AuthorityClient for EmbeddedAuthority {
    async fn issue(
        &self,
        integration_id: &str,
        request: InternalTokenRequest,
    ) -> Result<AuthorityTokenResponse, IntegrationError> {
        let issued = self
            .issuer
            .issue_internal_token(request)
            .await
            .map_err(|err| map_authz_error(integration_id, err))?;
        Ok(AuthorityTokenResponse {
            token: issued.token,
            expires_at: issued.expires_at,
        })
    }
}

fn map_authz_error(integration_id: &str, err: AuthzError) -> IntegrationError {
    match err {
        AuthzError::Token(_) | AuthzError::Store(_) => IntegrationError::Provider {
            integration_id: integration_id.to_string(),
            reason: err.to_string(),
        },
        denied => IntegrationError::Denied {
            integration_id: integration_id.to_string(),
            reason: denied.to_string(),
        },
    }
}

/// HTTP authority access for the central-authority deployment preset.
/// Posts [`InternalTokenRequest`] JSON to the authority's token route.
pub struct HttpAuthority {
    transport: Arc<dyn HttpTransport>,
    /// Full URL of the authority's token endpoint
    /// (e.g. `https://auth.internal/auth/token`).
    token_url: String,
}

impl HttpAuthority {
    pub fn new(transport: Arc<dyn HttpTransport>, token_url: impl Into<String>) -> Self {
        Self {
            transport,
            token_url: token_url.into(),
        }
    }
}

#[async_trait]
impl AuthorityClient for HttpAuthority {
    async fn issue(
        &self,
        integration_id: &str,
        request: InternalTokenRequest,
    ) -> Result<AuthorityTokenResponse, IntegrationError> {
        let provider_error = |reason: String| IntegrationError::Provider {
            integration_id: integration_id.to_string(),
            reason,
        };

        let http_request = TransportRequest::new(Method::POST, &self.token_url)
            .json(&request)
            .map_err(|err| provider_error(format!("failed to encode token request: {err}")))?;
        let response = self
            .transport
            .execute(http_request)
            .await
            .map_err(|err| provider_error(format!("authority unreachable: {err}")))?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|err| provider_error(format!("failed to read authority response: {err}")))?;

        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|err| provider_error(format!("malformed authority response: {err}")));
        }

        if status.is_client_error() {
            #[derive(serde::Deserialize)]
            struct AuthorityError {
                error: String,
            }
            let reason = serde_json::from_slice::<AuthorityError>(&bytes)
                .map(|body| body.error)
                .unwrap_or_else(|_| format!("authority returned status {status}"));
            return Err(IntegrationError::Denied {
                integration_id: integration_id.to_string(),
                reason,
            });
        }

        Err(provider_error(format!(
            "authority returned status {status}"
        )))
    }
}

/// [`TokenSource`] producing RAS-issued internal service tokens
/// (family [`TokenFamily::RasInternal`]).
pub struct RasInternalTokenSource {
    authority: Arc<dyn AuthorityClient>,
    /// This service's identity proof, presented on every issuance request.
    proof: ServiceIdentityProof,
}

impl RasInternalTokenSource {
    pub fn new(authority: Arc<dyn AuthorityClient>, proof: ServiceIdentityProof) -> Self {
        Self { authority, proof }
    }
}

#[async_trait]
impl TokenSource for RasInternalTokenSource {
    fn family(&self) -> TokenFamily {
        TokenFamily::RasInternal
    }

    async fn issue_token(&self, request: &TokenRequest) -> Result<TokenLease, IntegrationError> {
        // v1: service-as-service only. Other principal modes fail closed
        // here, before any authority call.
        match &request.subject {
            TokenSubject::Service => {}
            TokenSubject::User { .. } | TokenSubject::ServiceAccount { .. } => {
                return Err(IntegrationError::Denied {
                    integration_id: request.integration_id.clone(),
                    reason: "RasInternalTokenSource v1 issues service-as-service tokens only"
                        .to_string(),
                });
            }
        }

        let audience = request.audience.clone().ok_or_else(|| {
            IntegrationError::InvalidConfig(format!(
                "integration {:?}: internal token requests require a target audience",
                request.integration_id
            ))
        })?;

        let response = self
            .authority
            .issue(
                &request.integration_id,
                InternalTokenRequest {
                    proof: self.proof.clone(),
                    audience,
                    permissions: request.scopes.clone(),
                },
            )
            .await?;

        Ok(TokenLease {
            access_token: SecretString::new(response.token),
            expires_at: Some(response.expires_at),
            scopes: request.scopes.clone(),
        })
    }
}
