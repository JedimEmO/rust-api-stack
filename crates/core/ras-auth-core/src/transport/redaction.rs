use super::AuthTransportConfig;
use super::csrf::DEFAULT_CSRF_HEADER;
use http::{
    HeaderMap, HeaderValue,
    header::{AUTHORIZATION, COOKIE, HeaderName, SET_COOKIE},
};

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
