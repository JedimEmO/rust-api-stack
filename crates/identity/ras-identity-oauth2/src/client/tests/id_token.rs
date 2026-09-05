use super::*;

#[test]
fn id_token_claim_validation_covers_iss_aud_exp_and_nonce() {
    let mut config = provider_config();
    config.issuer = Some("https://issuer.test".to_string());
    let exp = chrono::Utc::now().timestamp() + 600;

    let good = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "sub": "subject-1",
        "aud": "test_client_id",
        "exp": exp,
        "nonce": "nonce-1",
    }));
    assert!(validate_id_token_claims(&config, &good, Some("nonce-1")).is_ok());

    // An otherwise-valid id_token with no `sub` is rejected: the
    // userinfo binding must never run against an absent subject.
    let no_sub = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "aud": "test_client_id",
        "exp": exp,
        "nonce": "nonce-1",
    }));
    assert!(validate_id_token_claims(&config, &no_sub, Some("nonce-1")).is_err());

    // A single-element aud array containing this client is fine.
    let aud_single_array = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "sub": "subject-1",
        "aud": ["test_client_id"],
        "exp": exp,
    }));
    assert!(validate_id_token_claims(&config, &aud_single_array, None).is_ok());

    // Multi-audience token requires azp == client_id.
    let aud_array_with_azp = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "sub": "subject-1",
        "aud": ["other", "test_client_id"],
        "azp": "test_client_id",
        "exp": exp,
    }));
    assert!(validate_id_token_claims(&config, &aud_array_with_azp, None).is_ok());

    // Multi-audience token WITHOUT a matching azp is rejected.
    let aud_array_no_azp = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "aud": ["other", "test_client_id"],
        "exp": exp,
    }));
    assert!(validate_id_token_claims(&config, &aud_array_no_azp, None).is_err());

    let bad_iss = fake_id_token(serde_json::json!({
        "iss": "https://evil.test", "aud": "test_client_id", "exp": exp,
    }));
    assert!(matches!(
        validate_id_token_claims(&config, &bad_iss, None),
        Err(OAuth2Error::InvalidIdToken(_))
    ));

    let bad_aud = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test", "aud": "someone_else", "exp": exp,
    }));
    assert!(validate_id_token_claims(&config, &bad_aud, None).is_err());

    let expired = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "aud": "test_client_id",
        "exp": chrono::Utc::now().timestamp() - 10,
    }));
    assert!(validate_id_token_claims(&config, &expired, None).is_err());

    let wrong_nonce = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "aud": "test_client_id",
        "exp": exp,
        "nonce": "other",
    }));
    assert!(validate_id_token_claims(&config, &wrong_nonce, Some("nonce-1")).is_err());

    assert!(validate_id_token_claims(&config, "garbage", None).is_err());
}

#[test]
fn id_token_without_configured_issuer_is_rejected() {
    // issuer is None on provider_config() -> fail closed.
    let config = provider_config();
    assert!(config.issuer.is_none());
    let exp = chrono::Utc::now().timestamp() + 600;
    let token = fake_id_token(serde_json::json!({
        "iss": "https://issuer.test",
        "aud": "test_client_id",
        "exp": exp,
    }));
    assert!(matches!(
        validate_id_token_claims(&config, &token, None),
        Err(OAuth2Error::InvalidIdToken(_))
    ));
}

#[test]
fn id_token_subject_extracts_sub() {
    let token = fake_id_token(serde_json::json!({ "sub": "subject-123" }));
    assert_eq!(
        id_token_subject(&token).unwrap().as_deref(),
        Some("subject-123")
    );
}
