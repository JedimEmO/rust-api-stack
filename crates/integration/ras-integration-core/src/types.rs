//! Token request/lease types and the `TokenSource` abstraction.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::IntegrationError;
use crate::secret::SecretString;

/// The principal a token is requested for.
///
/// The variants correspond to the principal modes from the RAS authorization
/// model: service-as-service, user-delegated, and service-account calls.
/// Cache keys incorporate the full subject so the modes can never collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenSubject {
    /// The calling service's own identity (service-as-service).
    Service,
    /// On behalf of a RAS-authenticated user (user grants for external
    /// providers; user-delegated calls for internal services).
    User { user_id: String },
    /// A non-human service-account principal with explicit grants.
    ServiceAccount { service_account_id: String },
}

impl TokenSubject {
    /// Stable cache-key component, including the principal mode.
    pub(crate) fn cache_component(&self) -> String {
        match self {
            TokenSubject::Service => "service".to_string(),
            TokenSubject::User { user_id } => format!("user:{user_id}"),
            TokenSubject::ServiceAccount { service_account_id } => {
                format!("service_account:{service_account_id}")
            }
        }
    }
}

/// The family a token source produces. Cache keys include the family so
/// external OAuth tokens, internal RAS JWTs, and static/test tokens can
/// never collide or be attached through the wrong policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenFamily {
    /// External OAuth2/OIDC provider tokens.
    OAuth2,
    /// RAS-issued internal service tokens.
    RasInternal,
    /// Static/legacy tokens (API keys, fixed bearer tokens).
    Static,
    /// Custom adapter families.
    Custom(&'static str),
}

/// A request for an access token, normally built by [`crate::TokenManager`]
/// from a capability-scoped client rather than constructed in handler code.
#[derive(Debug, Clone)]
pub struct TokenRequest {
    /// Which configured integration the token is for.
    pub integration_id: String,
    /// The principal the token is requested for.
    pub subject: TokenSubject,
    /// Requested scopes (external providers) or permissions (internal
    /// services).
    pub scopes: Vec<String>,
    /// Target audience for audience-bound token sources (internal services).
    pub audience: Option<String>,
    /// Bypass the cache and force re-acquisition.
    pub force_refresh: bool,
}

/// A leased access token plus the metadata the manager needs for caching.
///
/// `Debug` shows everything except the token value.
#[derive(Clone)]
pub struct TokenLease {
    /// The bearer token value.
    pub access_token: SecretString,
    /// When the token expires; `None` means the source provided no expiry
    /// (e.g. static tokens) and the lease is cached until invalidated.
    pub expires_at: Option<DateTime<Utc>>,
    /// The scopes actually granted (may exceed the requested set).
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for TokenLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenLease")
            .field("access_token", &self.access_token)
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
}

/// A pluggable token acquisition strategy.
///
/// Implementations: OAuth2/OIDC providers (`ras-integration-oauth2`), the
/// RAS internal issuer (`ras-integration-ras`), static tokens
/// ([`crate::StaticTokenSource`]), and test fakes.
#[async_trait]
pub trait TokenSource: Send + Sync {
    /// The token family this source produces. Part of every cache key.
    fn family(&self) -> TokenFamily;

    /// Acquire a token for `request`.
    ///
    /// Sources must fail closed: no grant means
    /// [`IntegrationError::ConsentRequired`], denied authorization means
    /// [`IntegrationError::Denied`] — never a silent fallback.
    async fn issue_token(&self, request: &TokenRequest) -> Result<TokenLease, IntegrationError>;
}

/// A trivial source returning a fixed token (API keys, legacy systems,
/// tests). Family [`TokenFamily::Static`]; the lease never expires.
pub struct StaticTokenSource {
    token: SecretString,
}

impl StaticTokenSource {
    pub fn new(token: impl Into<SecretString>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

#[async_trait]
impl TokenSource for StaticTokenSource {
    fn family(&self) -> TokenFamily {
        TokenFamily::Static
    }

    async fn issue_token(&self, _request: &TokenRequest) -> Result<TokenLease, IntegrationError> {
        Ok(TokenLease {
            access_token: self.token.clone(),
            expires_at: None,
            scopes: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_cache_components_are_distinct_per_principal_mode() {
        let service = TokenSubject::Service.cache_component();
        let user = TokenSubject::User {
            user_id: "alice".to_string(),
        }
        .cache_component();
        let account = TokenSubject::ServiceAccount {
            service_account_id: "alice".to_string(),
        }
        .cache_component();
        assert_ne!(service, user);
        assert_ne!(user, account);
        assert_ne!(service, account);
    }

    #[test]
    fn lease_debug_redacts_token() {
        let lease = TokenLease {
            access_token: SecretString::new("token-value"),
            expires_at: None,
            scopes: vec!["a".to_string()],
        };
        let debug = format!("{lease:?}");
        assert!(!debug.contains("token-value"));
        assert!(debug.contains("<redacted>"));
    }
}
