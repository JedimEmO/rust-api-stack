use super::*;

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
fn with_cookie_installs_default_csrf_and_validates() {
    let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());

    assert!(config.csrf.is_some());
    assert!(config.validate().is_ok());
}

#[test]
fn cookie_without_csrf_fails_validate() {
    let config = AuthTransportConfig {
        bearer: true,
        cookie: Some(AuthCookieConfig::default()),
        csrf: None,
    };

    let error = config.validate().unwrap_err();

    assert!(matches!(
        error,
        AuthTransportError::InvalidAuthTransportConfig(_)
    ));
}

#[test]
fn with_cookie_default_still_requires_csrf_header_on_unsafe_cookie_request() {
    let config = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());
    let cookie = AuthCredential::new("cookie-token", AuthTokenSource::Cookie);

    // No CSRF header present -> unsafe cookie request is rejected.
    assert_eq!(
        validate_csrf_for_credential("POST", &HeaderMap::new(), &cookie, &config).unwrap_err(),
        AuthTransportError::CsrfValidationFailed
    );

    // Bearer credentials stay exempt even on unsafe methods.
    let bearer = AuthCredential::new("bearer-token", AuthTokenSource::Bearer);
    assert!(validate_csrf_for_credential("POST", &HeaderMap::new(), &bearer, &config).is_ok());

    // GET cookie requests stay exempt.
    assert!(validate_csrf_for_credential("GET", &HeaderMap::new(), &cookie, &config).is_ok());

    // Valid double-submit header + cookie passes.
    let headers = headers(&[
        (DEFAULT_CSRF_HEADER, "csrf-token"),
        ("cookie", "__Host-ras-csrf=csrf-token"),
    ]);
    assert!(validate_csrf_for_credential("POST", &headers, &cookie, &config).is_ok());
}

#[test]
fn a1_dangerous_modes_are_reported_and_deprecated_aliases_still_work() {
    let presence =
        CsrfConfig::dangerous_header_presence_only(HeaderName::from_static("x-csrf-token"));
    assert_eq!(presence.dangerous_mode(), Some("header_presence_only"));

    let static_value = CsrfConfig::default().dangerous_static_value("shared-secret");
    assert_eq!(static_value.dangerous_mode(), Some("static_value"));

    assert_eq!(CsrfConfig::default().dangerous_mode(), None);
    assert_eq!(
        CsrfConfig::default()
            .with_cookie_name("__Host-other")
            .dangerous_mode(),
        None
    );

    // The deprecated names remain as thin wrappers for one release.
    #[allow(deprecated)]
    let legacy_presence = CsrfConfig::header_presence_only(HeaderName::from_static("x-csrf-token"));
    assert_eq!(legacy_presence, presence);
    #[allow(deprecated)]
    let legacy_static = CsrfConfig::default().with_expected_value("shared-secret");
    assert_eq!(legacy_static, static_value);
}

#[test]
fn a1_cookie_auth_with_weak_csrf_mode_warns_at_construction() {
    let warnings = capture_warnings(|| {
        let _ = AuthTransportConfig::default()
            .with_cookie(AuthCookieConfig::default())
            .with_csrf(CsrfConfig::dangerous_header_presence_only(
                HeaderName::from_static("x-csrf-token"),
            ));
    });
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("csrf_mode=\"header_presence_only\""));
    assert!(warnings[0].contains("weak CSRF mode"));

    // Ordering does not matter: csrf first, then cookie.
    let warnings = capture_warnings(|| {
        let _ = AuthTransportConfig::default()
            .with_csrf(CsrfConfig::default().dangerous_static_value("shared-secret"))
            .with_cookie(AuthCookieConfig::default());
    });
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("csrf_mode=\"static_value\""));

    // Weak CSRF without cookie auth is irrelevant (bearer-only) — no warning.
    let warnings = capture_warnings(|| {
        let _ = AuthTransportConfig::default().with_csrf(
            CsrfConfig::dangerous_header_presence_only(HeaderName::from_static("x-csrf-token")),
        );
    });
    assert!(warnings.is_empty(), "{warnings:?}");

    // The default double-submit mode never warns.
    let warnings = capture_warnings(|| {
        let _ = AuthTransportConfig::default().with_cookie(AuthCookieConfig::default());
    });
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a1_struct_literal_weak_csrf_warns_from_validate_once() {
    // Struct-literal construction bypasses the builders, so `validate`
    // warns as a fallback — but only once per distinct weak config per
    // process, since it runs on every request. A header name unique to
    // this test keeps it independent of test ordering.
    let config = AuthTransportConfig {
        bearer: true,
        cookie: Some(AuthCookieConfig::default()),
        csrf: Some(CsrfConfig::dangerous_header_presence_only(
            HeaderName::from_static("x-a1-struct-literal-csrf"),
        )),
    };
    let warnings = capture_warnings(|| {
        config.validate().unwrap();
        config.validate().unwrap();
        config.validate().unwrap();
    });
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("csrf_mode=\"header_presence_only\""));
}
