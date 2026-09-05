use super::*;

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
