use crate::{OAuth2Error, OAuth2ProviderConfig, OAuth2Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

/// Claims checked on an id_token returned by the token endpoint.
#[derive(serde::Deserialize)]
struct IdTokenClaims {
    iss: Option<String>,
    sub: Option<String>,
    aud: Option<serde_json::Value>,
    /// Authorized party — required to equal `client_id` when `aud` has multiple
    /// entries (OIDC Core §3.1.3.7 / §2).
    azp: Option<String>,
    exp: Option<i64>,
    nonce: Option<String>,
}

/// Subject (`sub`) claim of an id_token, used to bind it to the userinfo
/// response so a confused-deputy userinfo cannot change the account.
pub(crate) fn id_token_subject(id_token: &str) -> OAuth2Result<Option<String>> {
    Ok(decode_id_token_claims(id_token)?.sub)
}

fn decode_id_token_claims(id_token: &str) -> OAuth2Result<IdTokenClaims> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| OAuth2Error::InvalidIdToken("malformed JWT".to_string()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| OAuth2Error::InvalidIdToken("invalid base64 payload".to_string()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| OAuth2Error::InvalidIdToken("invalid JSON payload".to_string()))
}

/// Validate the id_token issuer, audience, expiry, subject, and expected nonce.
/// Accepting an id_token requires a configured provider issuer.
///
/// The signature is not verified: the token was received directly from the
/// token endpoint over TLS, which OIDC Core §3.1.3.7 permits as a substitute
/// for signature validation in the authorization-code flow.
pub(crate) fn validate_id_token_claims(
    provider_config: &OAuth2ProviderConfig,
    id_token: &str,
    expected_nonce: Option<&str>,
) -> OAuth2Result<()> {
    let claims = decode_id_token_claims(id_token)?;

    // Issuer is fail-closed: an id_token whose issuer is unverified cannot be
    // trusted to identify the account, so accepting one without a configured
    // `issuer` is refused rather than silently skipped.
    let Some(expected_issuer) = &provider_config.issuer else {
        return Err(OAuth2Error::InvalidIdToken(
            "provider `issuer` must be configured to accept id_tokens".to_string(),
        ));
    };
    if claims.iss.as_deref() != Some(expected_issuer.as_str()) {
        return Err(OAuth2Error::InvalidIdToken(format!(
            "issuer mismatch: expected {expected_issuer}"
        )));
    }

    let client_id = provider_config.client_id.as_str();
    let audience_matches = match &claims.aud {
        Some(serde_json::Value::String(aud)) => aud == client_id,
        Some(serde_json::Value::Array(auds)) => {
            let contains = auds.iter().any(|aud| aud.as_str() == Some(client_id));
            if !contains {
                false
            } else if auds.len() > 1 {
                // Multiple audiences: `azp` must be present and equal client_id.
                claims.azp.as_deref() == Some(client_id)
            } else {
                true
            }
        }
        _ => false,
    };
    if !audience_matches {
        return Err(OAuth2Error::InvalidIdToken(
            "audience does not include this client (or azp mismatch for multi-audience token)"
                .to_string(),
        ));
    }

    match claims.exp {
        Some(exp) if exp > chrono::Utc::now().timestamp() => {}
        _ => {
            return Err(OAuth2Error::InvalidIdToken(
                "token expired or missing exp".to_string(),
            ));
        }
    }

    if let Some(expected) = expected_nonce
        && claims.nonce.as_deref() != Some(expected)
    {
        return Err(OAuth2Error::InvalidIdToken("nonce mismatch".to_string()));
    }

    // `sub` is REQUIRED by OIDC Core §2. Refuse an id_token without it so the
    // userinfo <-> id_token subject binding cannot silently no-op on a
    // token that carries no subject.
    match claims.sub.as_deref() {
        Some(sub) if !sub.trim().is_empty() => {}
        _ => {
            return Err(OAuth2Error::InvalidIdToken(
                "id_token is missing the required `sub` claim".to_string(),
            ));
        }
    }

    Ok(())
}
