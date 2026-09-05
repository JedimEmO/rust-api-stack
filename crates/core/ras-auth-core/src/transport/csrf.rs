use super::cookie::extract_cookie;
use super::{
    AuthCookieConfig, AuthCredential, AuthTokenSource, AuthTransportConfig, AuthTransportError,
};
use http::{HeaderMap, HeaderValue, header::HeaderName};
use subtle::ConstantTimeEq;

const DEFAULT_CSRF_COOKIE_NAME: &str = "__Host-ras-csrf";
pub(super) const DEFAULT_CSRF_HEADER: &str = "x-ras-csrf";

/// Header names that provide no CSRF protection because a browser either sends
/// them automatically cross-origin (CORS-safelisted request headers) or
/// populates them itself (forbidden headers a page cannot control). A CSRF
/// header must be a custom header, since only a custom header forces a CORS
/// preflight that a cross-site attacker cannot satisfy.
const CSRF_UNSAFE_HEADER_NAMES: &[&str] = &[
    // CORS-safelisted request headers — sent cross-origin without a preflight.
    "accept",
    "accept-language",
    "content-language",
    "content-type",
    // Browser-controlled / forbidden headers — auto-sent, not page-settable.
    "cookie",
    "origin",
    "referer",
    "host",
    "user-agent",
    "content-length",
    "connection",
    "accept-encoding",
    "date",
];

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

    /// Require the custom header to carry a single, static, process-wide value.
    ///
    /// **Dangerous.** A static value is not bound to a session: any attacker
    /// who learns it once (from a leaked bundle, a shared client, or a single
    /// captured request) can forge unsafe cookie-authenticated requests for
    /// every user until the value is rotated. This disables the double-submit
    /// cookie check. Prefer [`Self::default`] for browser sessions.
    pub fn dangerous_static_value(mut self, expected_value: impl Into<String>) -> Self {
        self.expected_value = Some(expected_value.into());
        self.cookie_name = None;
        self
    }

    /// Deprecated alias for [`Self::dangerous_static_value`].
    #[deprecated(
        since = "0.3.0",
        note = "renamed to `dangerous_static_value`; a static CSRF value is not \
                bound to a session and is a weak CSRF defense"
    )]
    pub fn with_expected_value(self, expected_value: impl Into<String>) -> Self {
        self.dangerous_static_value(expected_value)
    }

    /// Require the custom header to match this CSRF cookie.
    pub fn with_cookie_name(mut self, cookie_name: impl Into<String>) -> Self {
        self.cookie_name = Some(cookie_name.into());
        self.expected_value = None;
        self
    }

    /// Require only a non-empty custom header.
    ///
    /// **Dangerous.** This mode relies entirely on the browser refusing to send
    /// a custom header cross-origin without a successful CORS preflight. It is
    /// only sound behind a restrictive credentialed CORS policy and is not a
    /// complete CSRF defense by itself. Prefer [`Self::default`] for browser
    /// sessions.
    pub fn dangerous_header_presence_only(header_name: HeaderName) -> Self {
        Self {
            header_name,
            expected_value: None,
            cookie_name: None,
        }
    }

    /// Deprecated alias for [`Self::dangerous_header_presence_only`].
    #[deprecated(
        since = "0.3.0",
        note = "renamed to `dangerous_header_presence_only`; presence-only CSRF \
                depends on restrictive CORS and is a weak CSRF defense"
    )]
    pub fn header_presence_only(header_name: HeaderName) -> Self {
        Self::dangerous_header_presence_only(header_name)
    }

    /// Whether this configuration uses one of the weak, opt-in modes
    /// ([`Self::dangerous_static_value`] or
    /// [`Self::dangerous_header_presence_only`]) rather than the default
    /// double-submit cookie check.
    ///
    /// Returns the mode name for logging, or `None` for the double-submit mode.
    pub fn dangerous_mode(&self) -> Option<&'static str> {
        match (&self.expected_value, &self.cookie_name) {
            (Some(_), _) => Some("static_value"),
            (None, None) => Some("header_presence_only"),
            (None, Some(_)) => None,
        }
    }

    /// Emit a `warn!` if this CSRF config is in a weak mode. Called from the
    /// [`AuthTransportConfig`] builders (once per construction) and, as a
    /// fallback for struct-literal construction, once per process from
    /// [`AuthTransportConfig::validate`].
    pub(super) fn warn_if_dangerous(&self) {
        if let Some(mode) = self.dangerous_mode() {
            tracing::warn!(
                csrf_mode = mode,
                csrf_header = %self.header_name,
                "cookie auth is configured with a weak CSRF mode \
                 (`CsrfConfig::dangerous_*`); this is not a complete CSRF defense. \
                 Prefer the default double-submit cookie mode for browser sessions"
            );
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
        // A CORS-safelisted or browser-controlled header name provides zero CSRF
        // protection (it is sent automatically cross-origin), so reject it —
        // otherwise `dangerous_header_presence_only(HeaderName::from_static("accept"))`
        // would produce a config that passes validation but never blocks a
        // forged request.
        let header = self.header_name.as_str();
        if CSRF_UNSAFE_HEADER_NAMES
            .iter()
            .any(|name| header.eq_ignore_ascii_case(name))
        {
            return Err(AuthTransportError::InvalidCsrfConfig(format!(
                "CSRF header `{header}` is CORS-safelisted or browser-controlled \
                 and provides no protection; use a custom header name (e.g. \
                 `x-csrf-token`)"
            )));
        }

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
            && !ct_eq_str(value, expected)
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

            if cookie_value.trim().is_empty() || !ct_eq_str(&cookie_value, value) {
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

/// Validate CSRF policy for a previously extracted credential.
pub fn validate_csrf_for_credential(
    method: &str,
    headers: &HeaderMap,
    credential: &AuthCredential,
    config: &AuthTransportConfig,
) -> Result<(), AuthTransportError> {
    config.validate()?;

    if credential.source() != AuthTokenSource::Cookie || !is_unsafe_method(method) {
        return Ok(());
    }

    match &config.csrf {
        Some(csrf) => csrf.validate_headers(headers),
        None => Ok(()),
    }
}

/// Constant-time string comparison for CSRF tokens.
///
/// Length is allowed to leak (subtle short-circuits on differing lengths), but
/// equal-length values are compared without an input-dependent early return.
fn ct_eq_str(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn is_unsafe_method(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    )
}
