use super::*;

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
