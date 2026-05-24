//! HTTP credential transport helpers for bearer and cookie-based sessions.

use cookie::{
    Cookie, SameSite,
    time::{Duration, OffsetDateTime},
};
use http::header::{AUTHORIZATION, COOKIE, HeaderName, SET_COOKIE};
use http::{HeaderMap, HeaderValue};
use thiserror::Error;

const DEFAULT_COOKIE_NAME: &str = "__Host-ras-session";
const DEFAULT_CSRF_COOKIE_NAME: &str = "__Host-ras-csrf";
const DEFAULT_CSRF_HEADER: &str = "x-ras-csrf";

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

/// Errors that can occur while extracting or validating HTTP auth credentials.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthTransportError {
    /// No configured credential transport found a token.
    #[error("missing authentication credentials")]
    MissingCredentials,

    /// The `Authorization` header was present but was not a valid bearer token.
    #[error("invalid authorization header")]
    InvalidAuthorizationHeader,

    /// Cookie-authenticated request failed CSRF validation.
    #[error("CSRF validation failed")]
    CsrfValidationFailed,

    /// Cookie configuration is internally inconsistent.
    #[error("invalid cookie configuration: {0}")]
    InvalidCookieConfig(String),

    /// The request contained ambiguous or invalid cookie credentials.
    #[error("invalid cookie header: {0}")]
    InvalidCookieHeader(String),

    /// CSRF configuration is internally inconsistent.
    #[error("invalid CSRF configuration: {0}")]
    InvalidCsrfConfig(String),

    /// Auth transport configuration is internally inconsistent.
    #[error("invalid auth transport configuration: {0}")]
    InvalidAuthTransportConfig(String),

    /// Generated cookie header could not be represented as an HTTP header.
    #[error("invalid set-cookie header: {0}")]
    InvalidSetCookieHeader(String),
}

/// SameSite setting for generated session cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieSameSite {
    /// Send cookies for same-site requests and top-level cross-site navigations.
    Lax,
    /// Send cookies only for same-site requests.
    Strict,
    /// Send cookies cross-site. Requires `Secure`.
    None,
}

impl CookieSameSite {
    fn as_cookie_same_site(self) -> SameSite {
        match self {
            Self::Lax => SameSite::Lax,
            Self::Strict => SameSite::Strict,
            Self::None => SameSite::None,
        }
    }
}

/// Configuration for accepting and emitting a session cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCookieConfig {
    /// Cookie name. Defaults to a host-only secure-cookie prefix.
    pub name: String,
    /// Cookie path. Defaults to `/`.
    pub path: String,
    /// Optional cookie domain. Must remain `None` for `__Host-` cookies.
    pub domain: Option<String>,
    /// Whether to emit `Secure`.
    pub secure: bool,
    /// Whether to emit `HttpOnly`.
    pub http_only: bool,
    /// SameSite policy.
    pub same_site: CookieSameSite,
    /// Optional `Max-Age` in seconds for the set-cookie helper.
    pub max_age_seconds: Option<i64>,
}

impl Default for AuthCookieConfig {
    fn default() -> Self {
        Self {
            name: DEFAULT_COOKIE_NAME.to_string(),
            path: "/".to_string(),
            domain: None,
            secure: true,
            http_only: true,
            same_site: CookieSameSite::Lax,
            max_age_seconds: None,
        }
    }
}

impl AuthCookieConfig {
    /// Create a secure cookie configuration with a custom name.
    ///
    /// Prefer [`Self::default`] or [`Self::host_prefixed`] for production browser sessions.
    /// Plain shared-domain names are easier to confuse with cookies set by subdomains.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Create a secure `__Host-` prefixed cookie configuration with a custom suffix.
    pub fn host_prefixed(name: impl Into<String>) -> Self {
        let name = name.into();
        let suffix = name.strip_prefix("__Host-").unwrap_or(&name);
        Self {
            name: format!("__Host-{suffix}"),
            ..Self::default()
        }
    }

    /// Relax `Secure` for local HTTP development.
    ///
    /// Do not use this in production.
    pub fn insecure_for_local_development(mut self) -> Self {
        self.secure = false;
        if let Some(name) = self.name.strip_prefix("__Host-") {
            self.name = name.to_string();
        }
        self
    }

    /// Validate cookie prefix and browser-enforced security invariants.
    pub fn validate(&self) -> Result<(), AuthTransportError> {
        validate_cookie_name(&self.name)?;

        if self.path.trim().is_empty() {
            return Err(AuthTransportError::InvalidCookieConfig(
                "cookie path must not be empty".to_string(),
            ));
        }

        if !self.path.starts_with('/') {
            return Err(AuthTransportError::InvalidCookieConfig(
                "cookie path must start with '/'".to_string(),
            ));
        }

        if self.name.starts_with("__Secure-") && !self.secure {
            return Err(AuthTransportError::InvalidCookieConfig(
                "__Secure- cookies must be Secure".to_string(),
            ));
        }

        if self.name.starts_with("__Host-") {
            if !self.secure {
                return Err(AuthTransportError::InvalidCookieConfig(
                    "__Host- cookies must be Secure".to_string(),
                ));
            }
            if self.domain.is_some() {
                return Err(AuthTransportError::InvalidCookieConfig(
                    "__Host- cookies must not set Domain".to_string(),
                ));
            }
            if self.path != "/" {
                return Err(AuthTransportError::InvalidCookieConfig(
                    "__Host- cookies must use Path=/".to_string(),
                ));
            }
        }

        if self.same_site == CookieSameSite::None && !self.secure {
            return Err(AuthTransportError::InvalidCookieConfig(
                "SameSite=None cookies must be Secure".to_string(),
            ));
        }

        if let Some(domain) = &self.domain
            && domain.trim().is_empty()
        {
            return Err(AuthTransportError::InvalidCookieConfig(
                "cookie domain must not be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Build a `Set-Cookie` header value for a newly issued session token.
    pub fn session_cookie_header_value(
        &self,
        token: &str,
    ) -> Result<HeaderValue, AuthTransportError> {
        self.validate()?;

        let mut builder = Cookie::build((self.name.clone(), token.to_string()))
            .path(self.path.clone())
            .secure(self.secure)
            .http_only(self.http_only)
            .same_site(self.same_site.as_cookie_same_site());

        if let Some(domain) = &self.domain {
            builder = builder.domain(domain.clone());
        }

        if let Some(max_age) = self.max_age_seconds {
            builder = builder.max_age(Duration::seconds(max_age));
        }

        set_cookie_value(builder.build().to_string())
    }

    /// Build a `Set-Cookie` header value that clears this session cookie.
    pub fn clear_cookie_header_value(&self) -> Result<HeaderValue, AuthTransportError> {
        self.validate()?;

        let mut builder = Cookie::build((self.name.clone(), ""))
            .path(self.path.clone())
            .secure(self.secure)
            .http_only(self.http_only)
            .same_site(self.same_site.as_cookie_same_site())
            .max_age(Duration::seconds(0))
            .expires(OffsetDateTime::UNIX_EPOCH);

        if let Some(domain) = &self.domain {
            builder = builder.domain(domain.clone());
        }

        set_cookie_value(builder.build().to_string())
    }
}

fn validate_cookie_name(name: &str) -> Result<(), AuthTransportError> {
    if name.trim().is_empty() {
        return Err(AuthTransportError::InvalidCookieConfig(
            "cookie name must not be empty".to_string(),
        ));
    }

    if name.trim() != name {
        return Err(AuthTransportError::InvalidCookieConfig(
            "cookie name must not contain leading or trailing whitespace".to_string(),
        ));
    }

    for byte in name.bytes() {
        if byte <= 0x20
            || byte >= 0x7f
            || matches!(
                byte,
                b'(' | b')'
                    | b'<'
                    | b'>'
                    | b'@'
                    | b','
                    | b';'
                    | b':'
                    | b'\\'
                    | b'"'
                    | b'/'
                    | b'['
                    | b']'
                    | b'?'
                    | b'='
                    | b'{'
                    | b'}'
            )
        {
            return Err(AuthTransportError::InvalidCookieConfig(
                "cookie name must be a valid RFC6265 token".to_string(),
            ));
        }
    }

    Ok(())
}

fn set_cookie_value(value: String) -> Result<HeaderValue, AuthTransportError> {
    HeaderValue::from_str(&value)
        .map_err(|err| AuthTransportError::InvalidSetCookieHeader(err.to_string()))
}

/// CSRF guard configuration for cookie-authenticated unsafe requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfConfig {
    /// Header that must be present on unsafe cookie-authenticated requests.
    pub header_name: HeaderName,
    /// Optional exact value the header must carry. If set, this value is used
    /// instead of double-submit cookie validation.
    pub expected_value: Option<String>,
    /// Cookie whose value must match the CSRF header. Enabled by default.
    pub cookie_name: Option<String>,
}

impl Default for CsrfConfig {
    fn default() -> Self {
        Self {
            header_name: HeaderName::from_static(DEFAULT_CSRF_HEADER),
            expected_value: None,
            cookie_name: Some(DEFAULT_CSRF_COOKIE_NAME.to_string()),
        }
    }
}

impl CsrfConfig {
    /// Require a custom header and the default double-submit CSRF cookie.
    pub fn new(header_name: HeaderName) -> Self {
        Self {
            header_name,
            ..Self::default()
        }
    }

    /// Require the custom header to carry an exact value.
    ///
    /// This is intended for callers that validate a session-specific CSRF token
    /// outside of the default double-submit cookie flow.
    pub fn with_expected_value(mut self, expected_value: impl Into<String>) -> Self {
        self.expected_value = Some(expected_value.into());
        self.cookie_name = None;
        self
    }

    /// Require the custom header to match this CSRF cookie.
    pub fn with_cookie_name(mut self, cookie_name: impl Into<String>) -> Self {
        self.cookie_name = Some(cookie_name.into());
        self.expected_value = None;
        self
    }

    /// Require only a non-empty custom header.
    ///
    /// This mode depends on restrictive credentialed CORS and is not a complete
    /// CSRF defense by itself. Prefer [`Self::default`] for browser sessions.
    pub fn header_presence_only(header_name: HeaderName) -> Self {
        Self {
            header_name,
            expected_value: None,
            cookie_name: None,
        }
    }

    /// Build a `Set-Cookie` header value for the double-submit CSRF token.
    ///
    /// The CSRF cookie is intentionally not `HttpOnly` so browser clients can
    /// copy its value into the configured CSRF header.
    pub fn csrf_cookie_header_value(&self, token: &str) -> Result<HeaderValue, AuthTransportError> {
        self.csrf_cookie_config()?
            .session_cookie_header_value(token)
    }

    /// Build a `Set-Cookie` header value that clears the CSRF cookie.
    pub fn clear_csrf_cookie_header_value(&self) -> Result<HeaderValue, AuthTransportError> {
        self.csrf_cookie_config()?.clear_cookie_header_value()
    }

    /// Validate CSRF configuration.
    pub fn validate(&self) -> Result<(), AuthTransportError> {
        if let Some(expected) = &self.expected_value
            && expected.trim().is_empty()
        {
            return Err(AuthTransportError::InvalidCsrfConfig(
                "expected CSRF value must not be empty".to_string(),
            ));
        }

        if let Some(cookie_name) = &self.cookie_name {
            let cookie = AuthCookieConfig {
                name: cookie_name.clone(),
                http_only: false,
                ..AuthCookieConfig::default()
            };
            cookie.validate()?;
        }

        Ok(())
    }

    fn validate_headers(&self, headers: &HeaderMap) -> Result<(), AuthTransportError> {
        self.validate()?;

        let value = headers
            .get(&self.header_name)
            .ok_or(AuthTransportError::CsrfValidationFailed)?;
        let value = value
            .to_str()
            .map_err(|_| AuthTransportError::CsrfValidationFailed)?;

        if value.trim().is_empty() {
            return Err(AuthTransportError::CsrfValidationFailed);
        }

        if let Some(expected) = &self.expected_value
            && value != expected
        {
            return Err(AuthTransportError::CsrfValidationFailed);
        }

        if self.expected_value.is_some() {
            return Ok(());
        }

        if let Some(cookie_name) = &self.cookie_name {
            let Some(cookie_value) = extract_cookie(headers, cookie_name)? else {
                return Err(AuthTransportError::CsrfValidationFailed);
            };

            if cookie_value.trim().is_empty() || cookie_value != value {
                return Err(AuthTransportError::CsrfValidationFailed);
            }
        }

        Ok(())
    }

    fn csrf_cookie_config(&self) -> Result<AuthCookieConfig, AuthTransportError> {
        let cookie_name = self.cookie_name.as_ref().ok_or_else(|| {
            AuthTransportError::InvalidCsrfConfig(
                "CSRF cookie helper requires cookie validation mode".to_string(),
            )
        })?;

        let cookie = AuthCookieConfig {
            name: cookie_name.clone(),
            http_only: false,
            ..AuthCookieConfig::default()
        };
        cookie.validate()?;
        Ok(cookie)
    }
}

/// Configures which HTTP transports a generated service accepts for auth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthTransportConfig {
    /// Accept `Authorization: Bearer ...`.
    pub bearer: bool,
    /// Optional secure cookie credential transport.
    pub cookie: Option<AuthCookieConfig>,
    /// Optional CSRF guard for cookie-authenticated unsafe requests.
    pub csrf: Option<CsrfConfig>,
}

impl Default for AuthTransportConfig {
    fn default() -> Self {
        Self {
            bearer: true,
            cookie: None,
            csrf: None,
        }
    }
}

impl AuthTransportConfig {
    /// Enable cookie auth alongside the default bearer transport.
    pub fn with_cookie(mut self, cookie: AuthCookieConfig) -> Self {
        self.cookie = Some(cookie);
        self
    }

    /// Enable CSRF protection for cookie-authenticated unsafe requests.
    pub fn with_csrf(mut self, csrf: CsrfConfig) -> Self {
        self.csrf = Some(csrf);
        self
    }

    /// Disable bearer-token extraction.
    pub fn without_bearer(mut self) -> Self {
        self.bearer = false;
        self
    }

    /// Validate all configured auth transports.
    pub fn validate(&self) -> Result<(), AuthTransportError> {
        if !self.bearer && self.cookie.is_none() {
            return Err(AuthTransportError::InvalidAuthTransportConfig(
                "at least one auth transport must be enabled".to_string(),
            ));
        }

        if let Some(cookie) = &self.cookie {
            cookie.validate()?;
        }

        if let Some(csrf) = &self.csrf {
            csrf.validate()?;
        }

        Ok(())
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

/// Validate CSRF policy for a previously extracted credential.
pub fn validate_csrf_for_credential(
    method: &str,
    headers: &HeaderMap,
    credential: &AuthCredential,
    config: &AuthTransportConfig,
) -> Result<(), AuthTransportError> {
    config.validate()?;

    if credential.source != AuthTokenSource::Cookie || !is_unsafe_method(method) {
        return Ok(());
    }

    match &config.csrf {
        Some(csrf) => csrf.validate_headers(headers),
        None => Ok(()),
    }
}

/// Header name used by cookie helper return values.
pub fn set_cookie_header_name() -> HeaderName {
    SET_COOKIE
}

/// Clone headers with known credential-bearing values replaced by `[REDACTED]`.
pub fn redact_sensitive_headers(headers: &HeaderMap) -> HeaderMap {
    let mut redacted = headers.clone();

    redact_header(&mut redacted, AUTHORIZATION);
    redact_header(&mut redacted, COOKIE);
    redact_header(&mut redacted, SET_COOKIE);
    redact_header(
        &mut redacted,
        HeaderName::from_static("proxy-authorization"),
    );
    redact_header(&mut redacted, HeaderName::from_static("x-auth-token"));
    redact_header(&mut redacted, HeaderName::from_static("x-api-key"));
    redact_header(&mut redacted, HeaderName::from_static("x-csrf-token"));
    redact_header(&mut redacted, HeaderName::from_static("x-xsrf-token"));
    redact_header(&mut redacted, HeaderName::from_static(DEFAULT_CSRF_HEADER));
    redact_header(
        &mut redacted,
        HeaderName::from_static("sec-websocket-protocol"),
    );

    redacted
}

/// Clone headers with default sensitive values and configured auth transport
/// header secrets replaced by `[REDACTED]`.
pub fn redact_sensitive_headers_for_auth_transport(
    headers: &HeaderMap,
    config: &AuthTransportConfig,
) -> HeaderMap {
    let mut redacted = redact_sensitive_headers(headers);

    if let Some(csrf) = &config.csrf {
        redact_header(&mut redacted, csrf.header_name.clone());
    }

    redacted
}

fn redact_header(headers: &mut HeaderMap, name: HeaderName) {
    if headers.contains_key(&name) {
        headers.remove(&name);
        headers.insert(name, HeaderValue::from_static("[REDACTED]"));
    }
}

fn is_unsafe_method(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    )
}

fn extract_cookie(
    headers: &HeaderMap,
    cookie_name: &str,
) -> Result<Option<String>, AuthTransportError> {
    let mut found = None;

    for value in headers.get_all(COOKIE) {
        let Ok(raw) = value.to_str() else {
            continue;
        };

        for cookie in Cookie::split_parse(raw).filter_map(Result::ok) {
            if cookie.name() == cookie_name {
                if found.is_some() {
                    return Err(AuthTransportError::InvalidCookieHeader(format!(
                        "multiple {cookie_name} cookies were present"
                    )));
                }
                found = Some(cookie.value().to_string());
            }
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn extract_auth_credential_returns_bearer_token() {
        let headers = headers(&[("authorization", "Bearer abc123")]);

        let credential = extract_auth_credential(&headers, &AuthTransportConfig::default())
            .expect("bearer extracts");

        assert_eq!(credential.token(), "abc123");
        assert_eq!(credential.source(), AuthTokenSource::Bearer);
    }

    #[test]
    fn extract_auth_credential_accepts_case_insensitive_bearer_scheme() {
        let headers = headers(&[("authorization", "bearer abc123")]);

        let credential = extract_auth_credential(&headers, &AuthTransportConfig::default())
            .expect("bearer extracts");

        assert_eq!(credential.token(), "abc123");
        assert_eq!(credential.source(), AuthTokenSource::Bearer);
    }

    #[test]
    fn extract_auth_credential_returns_cookie_when_bearer_absent() {
        let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());
        let headers = headers(&[("cookie", "theme=dark; __Host-ras-session=cookie-token")]);

        let credential = extract_auth_credential(&headers, &config).expect("cookie extracts");

        assert_eq!(credential.token(), "cookie-token");
        assert_eq!(credential.source(), AuthTokenSource::Cookie);
    }

    #[test]
    fn extract_auth_credential_rejects_malformed_bearer_without_cookie_fallback() {
        let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());
        let headers = headers(&[
            ("authorization", "Basic abc123"),
            ("cookie", "__Host-ras-session=cookie-token"),
        ]);

        let error = extract_auth_credential(&headers, &config).unwrap_err();

        assert_eq!(error, AuthTransportError::InvalidAuthorizationHeader);
    }

    #[test]
    fn extract_auth_credential_prefers_bearer_when_both_are_present() {
        let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());
        let headers = headers(&[
            ("authorization", "Bearer bearer-token"),
            ("cookie", "__Host-ras-session=cookie-token"),
        ]);

        let credential = extract_auth_credential(&headers, &config).expect("credential extracts");

        assert_eq!(credential.token(), "bearer-token");
        assert_eq!(credential.source(), AuthTokenSource::Bearer);
    }

    #[test]
    fn extract_auth_credential_rejects_duplicate_session_cookies() {
        let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());
        let headers = headers(&[(
            "cookie",
            "__Host-ras-session=first; __Host-ras-session=second",
        )]);

        let error = extract_auth_credential(&headers, &config).unwrap_err();

        assert!(matches!(error, AuthTransportError::InvalidCookieHeader(_)));
    }

    #[test]
    fn auth_cookie_config_validates_host_prefix_constraints() {
        assert!(AuthCookieConfig::default().validate().is_ok());

        let error = AuthCookieConfig {
            secure: false,
            ..AuthCookieConfig::default()
        }
        .validate()
        .unwrap_err();
        assert!(matches!(error, AuthTransportError::InvalidCookieConfig(_)));

        let error = AuthCookieConfig {
            domain: Some("example.com".to_string()),
            ..AuthCookieConfig::default()
        }
        .validate()
        .unwrap_err();
        assert!(matches!(error, AuthTransportError::InvalidCookieConfig(_)));
    }

    #[test]
    fn auth_cookie_config_validates_secure_prefix_and_cookie_name() {
        let error = AuthCookieConfig {
            name: "__Secure-ras-session".to_string(),
            secure: false,
            ..AuthCookieConfig::default()
        }
        .validate()
        .unwrap_err();
        assert!(matches!(error, AuthTransportError::InvalidCookieConfig(_)));

        let error = AuthCookieConfig::new("bad;name").validate().unwrap_err();
        assert!(matches!(error, AuthTransportError::InvalidCookieConfig(_)));
    }

    #[test]
    fn auth_transport_config_validates_cookie_config_before_extraction() {
        let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig {
            secure: false,
            ..AuthCookieConfig::default()
        });

        let error = extract_auth_credential(&HeaderMap::new(), &config).unwrap_err();

        assert!(matches!(error, AuthTransportError::InvalidCookieConfig(_)));
    }

    #[test]
    fn local_development_cookie_helper_removes_host_prefix() {
        let cookie = AuthCookieConfig::default().insecure_for_local_development();

        assert_eq!(cookie.name, "ras-session");
        assert!(!cookie.secure);
        assert!(cookie.validate().is_ok());
    }

    #[test]
    fn auth_cookie_config_builds_secure_set_cookie_header() {
        let value = AuthCookieConfig::default()
            .session_cookie_header_value("jwt-token")
            .expect("set-cookie header");
        let value = value.to_str().unwrap();

        assert!(value.starts_with("__Host-ras-session=jwt-token"));
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("SameSite=Lax"));
        assert!(value.contains("Secure"));
        assert!(value.contains("Path=/"));
    }

    #[test]
    fn auth_cookie_config_builds_clear_cookie_header() {
        let value = AuthCookieConfig::default()
            .clear_cookie_header_value()
            .expect("clear-cookie header");
        let value = value.to_str().unwrap();

        assert!(value.starts_with("__Host-ras-session="));
        assert!(value.contains("Max-Age=0"));
        assert!(value.contains("Expires="));
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("Path=/"));
    }

    #[test]
    fn csrf_validation_only_applies_to_cookie_auth_on_unsafe_methods() {
        let config = AuthTransportConfig::default()
            .with_cookie(AuthCookieConfig::default())
            .with_csrf(CsrfConfig::default());
        let bearer = AuthCredential::new("bearer-token", AuthTokenSource::Bearer);
        let cookie = AuthCredential::new("cookie-token", AuthTokenSource::Cookie);
        let headers_without_csrf = HeaderMap::new();
        let headers_with_csrf = headers(&[
            (DEFAULT_CSRF_HEADER, "csrf-token"),
            ("cookie", "__Host-ras-csrf=csrf-token"),
        ]);
        let headers_with_mismatched_csrf = headers(&[
            (DEFAULT_CSRF_HEADER, "csrf-token"),
            ("cookie", "__Host-ras-csrf=other-token"),
        ]);

        assert!(
            validate_csrf_for_credential("POST", &headers_without_csrf, &bearer, &config).is_ok()
        );
        assert!(
            validate_csrf_for_credential("GET", &headers_without_csrf, &cookie, &config).is_ok()
        );
        assert_eq!(
            validate_csrf_for_credential("POST", &headers_without_csrf, &cookie, &config)
                .unwrap_err(),
            AuthTransportError::CsrfValidationFailed
        );
        assert!(validate_csrf_for_credential("POST", &headers_with_csrf, &cookie, &config).is_ok());
        assert_eq!(
            validate_csrf_for_credential("POST", &headers_with_mismatched_csrf, &cookie, &config)
                .unwrap_err(),
            AuthTransportError::CsrfValidationFailed
        );
    }

    #[test]
    fn csrf_expected_value_mode_does_not_require_csrf_cookie() {
        let config = AuthTransportConfig::default()
            .with_cookie(AuthCookieConfig::default())
            .with_csrf(CsrfConfig::default().with_expected_value("csrf-token"));
        let cookie = AuthCredential::new("cookie-token", AuthTokenSource::Cookie);
        let headers = headers(&[(DEFAULT_CSRF_HEADER, "csrf-token")]);

        assert!(validate_csrf_for_credential("POST", &headers, &cookie, &config).is_ok());
    }

    #[test]
    fn csrf_config_builds_readable_double_submit_cookie() {
        let value = CsrfConfig::default()
            .csrf_cookie_header_value("csrf-token")
            .expect("set-cookie header");
        let value = value.to_str().unwrap();

        assert!(value.starts_with("__Host-ras-csrf=csrf-token"));
        assert!(!value.contains("HttpOnly"));
        assert!(value.contains("SameSite=Lax"));
        assert!(value.contains("Secure"));
        assert!(value.contains("Path=/"));
    }

    #[test]
    fn redact_sensitive_headers_removes_credential_values() {
        let headers = headers(&[
            ("authorization", "Bearer secret"),
            ("cookie", "__Host-ras-session=secret"),
            (DEFAULT_CSRF_HEADER, "csrf-secret"),
            ("user-agent", "test-agent"),
        ]);

        let redacted = redact_sensitive_headers(&headers);

        assert_eq!(
            redacted.get("authorization").unwrap(),
            HeaderValue::from_static("[REDACTED]")
        );
        assert_eq!(
            redacted.get("cookie").unwrap(),
            HeaderValue::from_static("[REDACTED]")
        );
        assert_eq!(
            redacted.get(DEFAULT_CSRF_HEADER).unwrap(),
            HeaderValue::from_static("[REDACTED]")
        );
        assert_eq!(redacted.get("user-agent").unwrap(), "test-agent");
    }

    #[test]
    fn redact_sensitive_headers_for_auth_transport_removes_custom_csrf_header() {
        let csrf_header = HeaderName::from_static("x-custom-csrf");
        let config = AuthTransportConfig::default().with_csrf(CsrfConfig::new(csrf_header.clone()));
        let headers = headers(&[("x-custom-csrf", "csrf-secret")]);

        let redacted = redact_sensitive_headers_for_auth_transport(&headers, &config);

        assert_eq!(
            redacted.get(csrf_header).unwrap(),
            HeaderValue::from_static("[REDACTED]")
        );
    }
}
