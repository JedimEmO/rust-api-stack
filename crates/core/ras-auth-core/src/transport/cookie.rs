use super::AuthTransportError;
use ::cookie::{
    Cookie, SameSite,
    time::{Duration, OffsetDateTime},
};
use http::{
    HeaderMap, HeaderValue,
    header::{COOKIE, HeaderName, SET_COOKIE},
};
const DEFAULT_COOKIE_NAME: &str = "__Host-ras-session";

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

/// Header name used by cookie helper return values.
pub fn set_cookie_header_name() -> HeaderName {
    SET_COOKIE
}

pub(super) fn extract_cookie(
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
