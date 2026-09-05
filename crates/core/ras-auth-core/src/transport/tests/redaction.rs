use super::*;

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
