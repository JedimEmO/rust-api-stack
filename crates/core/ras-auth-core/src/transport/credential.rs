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
///
/// The `Debug` output shows `[REDACTED]` in place of the token.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthCredential {
    token: String,
    source: AuthTokenSource,
}

impl std::fmt::Debug for AuthCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthCredential")
            .field("token", &"[REDACTED]")
            .field("source", &self.source)
            .finish()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_token() {
        let credential = AuthCredential::new("secret-bearer-token", AuthTokenSource::Bearer);
        let debug = format!("{credential:?}");
        assert!(!debug.contains("secret-bearer-token"), "{debug}");
        assert!(debug.contains("[REDACTED]"));
        assert!(debug.contains("Bearer"));
    }
}
