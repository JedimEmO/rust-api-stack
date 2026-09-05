use super::cookie::extract_cookie;
use super::{AuthTransportConfig, AuthTransportError};
use http::{HeaderMap, header::AUTHORIZATION};

/// Source from which an authentication token was extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthTokenSource {
    /// `Authorization: Bearer ...`
    Bearer,
    /// Configured HTTP cookie.
    Cookie,
}

/// Authentication token extracted from an HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCredential {
    token: String,
    source: AuthTokenSource,
}

impl AuthCredential {
    /// Create a credential for tests or custom extractors.
    pub fn new(token: impl Into<String>, source: AuthTokenSource) -> Self {
        Self {
            token: token.into(),
            source,
        }
    }

    /// The token value to pass to `AuthProvider::authenticate`.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The transport that supplied the token.
    pub fn source(&self) -> AuthTokenSource {
        self.source
    }
}

/// Extract an auth credential from configured HTTP transports.
pub fn extract_auth_credential(
    headers: &HeaderMap,
    config: &AuthTransportConfig,
) -> Result<AuthCredential, AuthTransportError> {
    config.validate()?;

    if config.bearer
        && let Some(header) = headers.get(AUTHORIZATION)
    {
        let header = header
            .to_str()
            .map_err(|_| AuthTransportError::InvalidAuthorizationHeader)?;
        let (scheme, token) = header
            .split_once(' ')
            .ok_or(AuthTransportError::InvalidAuthorizationHeader)?;
        if !scheme.eq_ignore_ascii_case("Bearer") || token.trim().is_empty() {
            return Err(AuthTransportError::InvalidAuthorizationHeader);
        }
        let token = token.trim();

        return Ok(AuthCredential::new(token, AuthTokenSource::Bearer));
    }

    if let Some(cookie_config) = &config.cookie
        && let Some(token) = extract_cookie(headers, &cookie_config.name)?
    {
        return Ok(AuthCredential::new(token, AuthTokenSource::Cookie));
    }

    Err(AuthTransportError::MissingCredentials)
}
