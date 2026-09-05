use super::*;

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

    assert!(validate_csrf_for_credential("POST", &headers_without_csrf, &bearer, &config).is_ok());
    assert!(validate_csrf_for_credential("GET", &headers_without_csrf, &cookie, &config).is_ok());
    assert_eq!(
        validate_csrf_for_credential("POST", &headers_without_csrf, &cookie, &config).unwrap_err(),
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
        .with_csrf(CsrfConfig::default().dangerous_static_value("csrf-token"));
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
fn csrf_config_rejects_cors_safelisted_header_names() {
    // A safelisted / browser-controlled header name provides no CSRF
    // protection and must fail validation even though it is "present".
    for name in [
        "accept",
        "content-type",
        "Accept-Language",
        "cookie",
        "origin",
    ] {
        let csrf = CsrfConfig::dangerous_header_presence_only(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
        );
        let error = csrf.validate().expect_err(name);
        assert!(
            matches!(error, AuthTransportError::InvalidCsrfConfig(_)),
            "{name} should be rejected"
        );
    }

    // A genuinely custom header (forces a CORS preflight) is accepted.
    let ok = CsrfConfig::dangerous_header_presence_only(HeaderName::from_static("x-csrf-token"));
    assert!(ok.validate().is_ok());
}
