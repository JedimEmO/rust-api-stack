//! Capability-scoped HTTP clients.
//!
//! Handlers receive a preconfigured client for one integration, one subject,
//! and a fixed scope set — not the [`TokenManager`] itself — so handler code
//! cannot request arbitrary integrations, scopes, audiences, or subjects
//! (the confused-deputy guard from issue #12).

use std::sync::Arc;

use ras_transport_core::http::Method;
use ras_transport_core::{HttpTransport, TransportRequest, TransportResponse};
use serde::Serialize;

use crate::error::IntegrationError;
use crate::manager::TokenManager;
use crate::types::{TokenRequest, TokenSubject};

/// An HTTP client bound to one integration, subject, and scope set.
///
/// Every request is validated against the integration's outbound host
/// allowlist *before* a token is acquired or attached, and the bearer header
/// is set fail-closed. There is no automatic refresh-and-retry of requests:
/// replaying non-idempotent requests after a 401 is the caller's explicit
/// decision, never this client's.
#[derive(Clone)]
pub struct AuthorizedHttpClient {
    transport: Arc<dyn HttpTransport>,
    manager: Arc<TokenManager>,
    integration_id: String,
    subject: TokenSubject,
    scopes: Vec<String>,
    audience: Option<String>,
}

impl AuthorizedHttpClient {
    /// A client acting on behalf of a RAS-authenticated user.
    pub fn for_user(
        transport: Arc<dyn HttpTransport>,
        manager: Arc<TokenManager>,
        integration_id: impl Into<String>,
        user_id: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            transport,
            manager,
            integration_id: integration_id.into(),
            subject: TokenSubject::User {
                user_id: user_id.into(),
            },
            scopes: scopes.into_iter().map(Into::into).collect(),
            audience: None,
        }
    }

    /// A client acting as the calling service itself (service-as-service).
    pub fn for_service(
        transport: Arc<dyn HttpTransport>,
        manager: Arc<TokenManager>,
        integration_id: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            transport,
            manager,
            integration_id: integration_id.into(),
            subject: TokenSubject::Service,
            scopes: scopes.into_iter().map(Into::into).collect(),
            audience: None,
        }
    }

    /// A client acting as a service-account principal.
    pub fn for_service_account(
        transport: Arc<dyn HttpTransport>,
        manager: Arc<TokenManager>,
        integration_id: impl Into<String>,
        service_account_id: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            transport,
            manager,
            integration_id: integration_id.into(),
            subject: TokenSubject::ServiceAccount {
                service_account_id: service_account_id.into(),
            },
            scopes: scopes.into_iter().map(Into::into).collect(),
            audience: None,
        }
    }

    /// Pin the target audience (internal service integrations).
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// Execute `request` with a managed bearer token attached.
    ///
    /// Order matters: the URL is checked against the integration's host
    /// allowlist first, so no token is even minted for a disallowed target.
    pub async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, IntegrationError> {
        self.manager
            .validate_outbound_url(&self.integration_id, &request.url)?;

        let lease = self
            .manager
            .get_token(TokenRequest {
                integration_id: self.integration_id.clone(),
                subject: self.subject.clone(),
                scopes: self.scopes.clone(),
                audience: self.audience.clone(),
                force_refresh: false,
            })
            .await?;

        let request = request.bearer(lease.access_token.expose_secret())?;
        Ok(self.transport.execute(request).await?)
    }

    /// `GET` the URL with a managed bearer token.
    pub async fn get(&self, url: impl Into<String>) -> Result<TransportResponse, IntegrationError> {
        self.execute(TransportRequest::new(Method::GET, url)).await
    }

    /// `POST` a JSON body with a managed bearer token.
    pub async fn post_json<T: Serialize>(
        &self,
        url: impl Into<String>,
        body: &T,
    ) -> Result<TransportResponse, IntegrationError> {
        let request = TransportRequest::new(Method::POST, url).json(body)?;
        self.execute(request).await
    }
}
