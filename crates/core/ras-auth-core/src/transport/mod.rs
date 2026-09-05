//! HTTP credential transport helpers for bearer and cookie-based sessions.
mod cookie;
mod credential;
mod csrf;
mod redaction;

pub use cookie::{AuthCookieConfig, CookieSameSite, set_cookie_header_name};
pub use credential::{AuthCredential, AuthTokenSource, extract_auth_credential};
pub use csrf::{CsrfConfig, validate_csrf_for_credential};
pub use redaction::{redact_sensitive_headers, redact_sensitive_headers_for_auth_transport};
use thiserror::Error;

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
    ///
    /// Cookie credentials are vulnerable to CSRF on unsafe methods, so this also
    /// installs a default double-submit [`CsrfConfig`] when none is configured
    /// yet. Override it with [`Self::with_csrf`] if you need a different policy;
    /// there is intentionally no builder path to cookie auth without CSRF.
    pub fn with_cookie(mut self, cookie: AuthCookieConfig) -> Self {
        self.cookie = Some(cookie);
        if self.csrf.is_none() {
            self.csrf = Some(CsrfConfig::default());
        }
        self.warn_if_weak_csrf();
        self
    }

    /// Enable CSRF protection for cookie-authenticated unsafe requests.
    ///
    /// Passing a `CsrfConfig::dangerous_*` mode together with cookie auth logs
    /// a `warn!` at construction time.
    pub fn with_csrf(mut self, csrf: CsrfConfig) -> Self {
        self.csrf = Some(csrf);
        self.warn_if_weak_csrf();
        self
    }

    /// Log a warning when cookie auth is paired with a weak CSRF mode.
    fn warn_if_weak_csrf(&self) {
        if self.cookie.is_some()
            && let Some(csrf) = &self.csrf
        {
            csrf.warn_if_dangerous();
        }
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

        // Cookie credentials are automatically attached by the browser, so
        // cookie auth without a CSRF guard lets any cross-site request act as
        // the victim on unsafe methods. `with_cookie` installs a default CSRF
        // config; a struct literal that clears it must fail closed here.
        if self.cookie.is_some() && self.csrf.is_none() {
            return Err(AuthTransportError::InvalidAuthTransportConfig(
                "cookie auth requires a CSRF configuration; use with_cookie (which sets a \
                 default double-submit CsrfConfig) or with_csrf"
                    .to_string(),
            ));
        }

        if let Some(cookie) = &self.cookie {
            cookie.validate()?;
        }

        if let Some(csrf) = &self.csrf {
            csrf.validate()?;
        }

        // `validate` runs on every request, so the weak-mode warning is
        // rate-limited here to once per distinct weak config per process. The
        // builders (`with_cookie`, `with_csrf`) warn unconditionally at
        // construction time; this is the fallback for struct-literal configs.
        if self.cookie.is_some()
            && let Some(csrf) = &self.csrf
            && let Some(mode) = csrf.dangerous_mode()
        {
            static WEAK_CSRF_WARNED: std::sync::Mutex<Vec<(String, &'static str)>> =
                std::sync::Mutex::new(Vec::new());
            let key = (csrf.header_name.as_str().to_string(), mode);
            let mut warned = WEAK_CSRF_WARNED
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !warned.contains(&key) {
                warned.push(key);
                csrf.warn_if_dangerous();
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
