//! PKCE consent-flow helpers.
//!
//! Applications own their HTTP routes; this module owns the security
//! invariants of the consent round trip:
//!
//! - `state` is opaque, random, single-use, and expiring.
//! - Each pending consent is bound to the initiating RAS user, integration,
//!   provider authorization endpoint, redirect URI, requested scopes, and
//!   PKCE verifier. The callback must present the same state *and* user.
//! - The PKCE verifier never leaves the process until the code exchange.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use ras_integration_core::{GrantStore, IntegrationError, SecretString, TokenLease, UserGrant};
use ras_transport_core::http::Method;
use ras_transport_core::{HttpTransport, TransportRequest};
use sha2::{Digest, Sha256};
use url::Url;

use crate::config::OAuth2ProviderConfig;

/// A prepared authorization request: send the user to `url`, keep `state`
/// in the redirect round trip.
#[derive(Debug, Clone)]
pub struct AuthorizationRedirect {
    pub url: String,
    pub state: String,
}

struct PendingConsent {
    integration_id: String,
    user_id: String,
    redirect_uri: String,
    scopes: Vec<String>,
    code_verifier: SecretString,
    expires_at: DateTime<Utc>,
}

/// Tracks pending consent flows and validates callbacks.
pub struct ConsentFlow {
    pending: Mutex<HashMap<String, PendingConsent>>,
    ttl: Duration,
}

impl ConsentFlow {
    /// `ttl` bounds how long a consent round trip may take (10 minutes is a
    /// reasonable default).
    pub fn new(ttl: Duration) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Begin a consent flow for `user_id` on `integration_id`. Returns the
    /// provider authorization URL (with PKCE S256 challenge) and the opaque
    /// state value.
    pub fn begin(
        &self,
        config: &OAuth2ProviderConfig,
        integration_id: impl Into<String>,
        user_id: impl Into<String>,
        redirect_uri: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<AuthorizationRedirect, IntegrationError> {
        let integration_id = integration_id.into();
        let endpoint = config.authorization_endpoint.as_ref().ok_or_else(|| {
            IntegrationError::InvalidConfig(format!(
                "integration {integration_id:?}: provider has no authorization endpoint configured"
            ))
        })?;

        let user_id = user_id.into();
        let redirect_uri = redirect_uri.into();
        let scopes: Vec<String> = scopes.into_iter().map(Into::into).collect();

        let state = random_urlsafe(32);
        let code_verifier = random_urlsafe(48);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));

        let mut url = Url::parse(endpoint).map_err(|err| {
            IntegrationError::InvalidConfig(format!("invalid authorization endpoint: {err}"))
        })?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", &scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");

        let mut pending = self.pending.lock().expect("consent flow lock poisoned");
        let now = Utc::now();
        pending.retain(|_, consent| consent.expires_at > now);
        pending.insert(
            state.clone(),
            PendingConsent {
                integration_id,
                user_id,
                redirect_uri,
                scopes,
                code_verifier: SecretString::new(code_verifier),
                expires_at: now + self.ttl,
            },
        );

        Ok(AuthorizationRedirect {
            url: url.to_string(),
            state,
        })
    }

    /// Validate a provider callback. Consumes the state (single use) and
    /// checks expiry plus binding to the initiating user and integration.
    /// On success returns the [`ValidatedConsent`] needed for the code
    /// exchange.
    pub fn validate_callback(
        &self,
        state: &str,
        expected_integration_id: &str,
        expected_user_id: &str,
    ) -> Result<ValidatedConsent, IntegrationError> {
        let denied = |reason: &str| IntegrationError::Denied {
            integration_id: expected_integration_id.to_string(),
            reason: reason.to_string(),
        };

        let consent = {
            let mut pending = self.pending.lock().expect("consent flow lock poisoned");
            pending.remove(state)
        }
        .ok_or_else(|| denied("unknown or already-used consent state"))?;

        if consent.expires_at <= Utc::now() {
            return Err(denied("consent state expired"));
        }
        if consent.integration_id != expected_integration_id {
            return Err(denied("consent state belongs to a different integration"));
        }
        if consent.user_id != expected_user_id {
            return Err(denied("consent state belongs to a different user"));
        }

        Ok(ValidatedConsent {
            integration_id: consent.integration_id,
            user_id: consent.user_id,
            redirect_uri: consent.redirect_uri,
            scopes: consent.scopes,
            code_verifier: consent.code_verifier,
        })
    }
}

/// A validated consent callback, ready for the authorization-code exchange.
pub struct ValidatedConsent {
    pub integration_id: String,
    pub user_id: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    code_verifier: SecretString,
}

impl std::fmt::Debug for ValidatedConsent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedConsent")
            .field("integration_id", &self.integration_id)
            .field("user_id", &self.user_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("scopes", &self.scopes)
            .field("code_verifier", &self.code_verifier)
            .finish()
    }
}

impl ValidatedConsent {
    /// Exchange the authorization `code` for tokens, store the refresh-token
    /// grant, and return the initial access-token lease.
    ///
    /// The PKCE verifier and the redirect URI bound at `begin` time are sent
    /// with the exchange; the grant is persisted with the originally
    /// requested scopes.
    pub async fn exchange_code(
        self,
        config: &OAuth2ProviderConfig,
        transport: &Arc<dyn HttpTransport>,
        grants: &Arc<dyn GrantStore>,
        code: &str,
    ) -> Result<TokenLease, IntegrationError> {
        let mut params = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("client_id", config.client_id.clone()),
            (
                "code_verifier",
                self.code_verifier.expose_secret().to_string(),
            ),
        ];
        if let Some(secret) = &config.client_secret {
            params.push(("client_secret", secret.expose_secret().to_string()));
        }
        let body =
            serde_urlencoded::to_string(&params).map_err(|err| IntegrationError::Provider {
                integration_id: self.integration_id.clone(),
                reason: format!("failed to encode code exchange: {err}"),
            })?;

        let request = TransportRequest::new(Method::POST, &config.token_endpoint)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(ras_transport_core::RequestBody::Bytes(body.into()));

        let response =
            transport
                .execute(request)
                .await
                .map_err(|err| IntegrationError::Provider {
                    integration_id: self.integration_id.clone(),
                    reason: format!("token endpoint unreachable: {err}"),
                })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|err| IntegrationError::Provider {
                integration_id: self.integration_id.clone(),
                reason: format!("failed to read exchange response: {err}"),
            })?;
        if !status.is_success() {
            return Err(IntegrationError::Denied {
                integration_id: self.integration_id.clone(),
                reason: format!("code exchange failed with status {status}"),
            });
        }

        #[derive(serde::Deserialize)]
        struct ExchangeResponse {
            access_token: String,
            #[serde(default)]
            expires_in: Option<i64>,
            #[serde(default)]
            refresh_token: Option<String>,
        }
        let exchange: ExchangeResponse =
            serde_json::from_slice(&bytes).map_err(|err| IntegrationError::Provider {
                integration_id: self.integration_id.clone(),
                reason: format!("malformed exchange response: {err}"),
            })?;

        let refresh_token = exchange
            .refresh_token
            .ok_or_else(|| IntegrationError::Provider {
                integration_id: self.integration_id.clone(),
                reason: "provider returned no refresh token; cannot store a grant".to_string(),
            })?;

        grants
            .put_user_grant(UserGrant {
                integration_id: self.integration_id.clone(),
                user_id: self.user_id.clone(),
                refresh_token: SecretString::new(refresh_token),
                scopes: self.scopes.clone(),
            })
            .await?;

        Ok(TokenLease {
            access_token: SecretString::new(exchange.access_token),
            expires_at: exchange
                .expires_in
                .map(|seconds| Utc::now() + Duration::seconds(seconds)),
            scopes: self.scopes,
        })
    }
}

fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}
