//! Token validation.
//!
//! [`TokenValidator`] performs the full RAS validation pipeline: header
//! parsing, algorithm allowlisting, key resolution by `kid`, key/header
//! algorithm cross-check, signature verification, and claim checks (token
//! type, issuer, expiry/not-before with clock skew, audience policy, and
//! per-type structural invariants).

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};

use crate::claims::{RasClaims, TokenType};
use crate::error::TokenError;
use crate::keyring::{DecodedHeader, KeyRing};
use crate::keys::{JwkSet, SigningAlgorithm, VerifyingKey};

/// Resolves a verification key for a token's `kid`.
pub trait KeyResolver: Send + Sync {
    fn resolve_key(&self, kid: &str) -> Option<VerifyingKey>;
}

impl KeyResolver for JwkSet {
    fn resolve_key(&self, kid: &str) -> Option<VerifyingKey> {
        self.find(kid)
            .and_then(|jwk| VerifyingKey::from_jwk(jwk).ok())
    }
}

impl KeyResolver for KeyRing {
    fn resolve_key(&self, kid: &str) -> Option<VerifyingKey> {
        self.resolve(kid)
    }
}

impl KeyResolver for VerifyingKey {
    fn resolve_key(&self, kid: &str) -> Option<VerifyingKey> {
        (self.kid() == kid).then(|| self.clone())
    }
}

impl<R: KeyResolver + ?Sized> KeyResolver for std::sync::Arc<R> {
    fn resolve_key(&self, kid: &str) -> Option<VerifyingKey> {
        (**self).resolve_key(kid)
    }
}

/// How the validator treats the `aud` claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudiencePolicy {
    /// `aud` must equal this audience exactly. Use for internal service
    /// tokens and gateway-derived tokens.
    Exact(String),
    /// `aud` must be absent. Use for multi-audience web sessions, whose
    /// permissions live in `audience_permissions`.
    Absent,
}

/// Validation policy. Construct with [`ValidationOptions::new`]; the
/// algorithm allowlist defaults to asymmetric algorithms only, so HS256
/// (shared-secret embedded mode) requires an explicit opt-in via
/// [`ValidationOptions::allow_algorithm`].
#[derive(Debug, Clone)]
pub struct ValidationOptions {
    pub expected_issuer: String,
    pub audience: AudiencePolicy,
    pub expected_token_types: Vec<TokenType>,
    pub allowed_algorithms: Vec<SigningAlgorithm>,
    pub clock_skew: Duration,
}

impl ValidationOptions {
    pub fn new(
        expected_issuer: impl Into<String>,
        audience: AudiencePolicy,
        expected_token_types: impl Into<Vec<TokenType>>,
    ) -> Self {
        Self {
            expected_issuer: expected_issuer.into(),
            audience,
            expected_token_types: expected_token_types.into(),
            allowed_algorithms: vec![SigningAlgorithm::ES256, SigningAlgorithm::EdDSA],
            clock_skew: Duration::seconds(30),
        }
    }

    /// Add an algorithm to the allowlist (e.g. HS256 for embedded mode).
    pub fn allow_algorithm(mut self, algorithm: SigningAlgorithm) -> Self {
        if !self.allowed_algorithms.contains(&algorithm) {
            self.allowed_algorithms.push(algorithm);
        }
        self
    }

    /// Override the clock-skew tolerance (default 30 seconds).
    pub fn with_clock_skew(mut self, skew: Duration) -> Self {
        self.clock_skew = skew;
        self
    }
}

/// Validates RAS tokens against a key resolver and a fixed policy.
pub struct TokenValidator<R> {
    resolver: R,
    options: ValidationOptions,
}

impl<R: KeyResolver> TokenValidator<R> {
    pub fn new(resolver: R, options: ValidationOptions) -> Self {
        Self { resolver, options }
    }

    pub fn options(&self) -> &ValidationOptions {
        &self.options
    }

    /// Validate `token` against the current wall clock.
    pub fn validate(&self, token: &str) -> Result<RasClaims, TokenError> {
        self.validate_at(token, Utc::now())
    }

    /// Validate `token` as of `now`. Exposed for deterministic tests.
    pub fn validate_at(&self, token: &str, now: DateTime<Utc>) -> Result<RasClaims, TokenError> {
        let mut segments = token.split('.');
        let header_segment = segments
            .next()
            .ok_or_else(|| TokenError::Malformed("missing header segment".to_string()))?;
        let payload_segment = segments
            .next()
            .ok_or_else(|| TokenError::Malformed("missing payload segment".to_string()))?;
        let signature_segment = segments
            .next()
            .ok_or_else(|| TokenError::Malformed("missing signature segment".to_string()))?;
        if segments.next().is_some() {
            return Err(TokenError::Malformed("too many segments".to_string()));
        }

        let header_bytes = URL_SAFE_NO_PAD
            .decode(header_segment)
            .map_err(|err| TokenError::Malformed(format!("header is not base64url: {err}")))?;
        let header: DecodedHeader = serde_json::from_slice(&header_bytes)
            .map_err(|err| TokenError::Malformed(format!("header is not valid JSON: {err}")))?;

        // Unknown algorithms (including "none") fail here, before any
        // signature or key handling.
        let algorithm = SigningAlgorithm::from_name(&header.alg)
            .filter(|alg| self.options.allowed_algorithms.contains(alg))
            .ok_or_else(|| TokenError::DisallowedAlgorithm(header.alg.clone()))?;

        let kid = header.kid.ok_or(TokenError::MissingKeyId)?;
        let key = self
            .resolver
            .resolve_key(&kid)
            .ok_or(TokenError::UnknownKeyId { kid })?;

        // The resolved key must itself use the declared algorithm; this is
        // the guard against key-type confusion (e.g. an HS256 token naming
        // an ES256 key's kid).
        if key.algorithm() != algorithm {
            return Err(TokenError::AlgorithmKeyMismatch {
                key: key.algorithm().name().to_string(),
                header: algorithm.name().to_string(),
            });
        }

        let signature = URL_SAFE_NO_PAD
            .decode(signature_segment)
            .map_err(|err| TokenError::Malformed(format!("signature is not base64url: {err}")))?;
        let signing_input = format!("{header_segment}.{payload_segment}");
        key.verify(signing_input.as_bytes(), &signature)?;

        let payload = URL_SAFE_NO_PAD
            .decode(payload_segment)
            .map_err(|err| TokenError::Malformed(format!("payload is not base64url: {err}")))?;
        let claims: RasClaims = serde_json::from_slice(&payload)
            .map_err(|err| TokenError::Malformed(format!("claims are not valid: {err}")))?;

        if !self
            .options
            .expected_token_types
            .contains(&claims.token_type)
        {
            return Err(TokenError::TokenTypeMismatch {
                expected: self.options.expected_token_types.clone(),
                actual: claims.token_type,
            });
        }

        if claims.iss != self.options.expected_issuer {
            return Err(TokenError::IssuerMismatch {
                expected: self.options.expected_issuer.clone(),
                actual: claims.iss,
            });
        }

        let now_ts = now.timestamp();
        let skew = self.options.clock_skew.num_seconds();
        if now_ts >= claims.exp + skew {
            return Err(TokenError::Expired);
        }
        if let Some(nbf) = claims.nbf
            && now_ts + skew < nbf
        {
            return Err(TokenError::NotYetValid);
        }

        match &self.options.audience {
            AudiencePolicy::Exact(expected) => {
                if claims.aud.as_deref() != Some(expected.as_str()) {
                    return Err(TokenError::AudienceMismatch {
                        expected: Some(expected.clone()),
                        actual: claims.aud,
                    });
                }
            }
            AudiencePolicy::Absent => {
                if claims.aud.is_some() {
                    return Err(TokenError::AudienceMismatch {
                        expected: None,
                        actual: claims.aud,
                    });
                }
            }
        }

        claims.validate_shape().map_err(TokenError::InvalidClaims)?;

        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::claims::PrincipalKind;
    use crate::keyring::{KeyRing, sign_claims};
    use crate::keys::SigningKey;

    const ISSUER: &str = "https://auth.internal";

    fn internal_claims() -> RasClaims {
        RasClaims::internal_service(
            ISSUER,
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec!["invoice:read".to_string()],
            Duration::minutes(5),
        )
    }

    fn internal_options() -> ValidationOptions {
        ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::InternalService],
        )
    }

    fn validator_for(key: &SigningKey, options: ValidationOptions) -> TokenValidator<VerifyingKey> {
        TokenValidator::new(key.verifying_key(), options)
    }

    #[test]
    fn round_trip_es256_eddsa_and_opted_in_hs256() {
        for key in [
            SigningKey::generate_es256("k"),
            SigningKey::generate_ed25519("k"),
            SigningKey::from_hmac_secret("k", vec![3u8; 32]).unwrap(),
        ] {
            let token = sign_claims(&key, &internal_claims()).unwrap();
            let options = internal_options().allow_algorithm(SigningAlgorithm::HS256);
            let claims = validator_for(&key, options).validate(&token).unwrap();
            assert_eq!(claims.sub, "billing-service");
            assert_eq!(claims.permissions, vec!["invoice:read"]);
        }
    }

    #[test]
    fn hs256_is_rejected_without_explicit_opt_in() {
        let key = SigningKey::from_hmac_secret("k", vec![3u8; 32]).unwrap();
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let err = validator_for(&key, internal_options())
            .validate(&token)
            .unwrap_err();
        assert!(matches!(err, TokenError::DisallowedAlgorithm(alg) if alg == "HS256"));
    }

    #[test]
    fn alg_none_is_rejected() {
        let key = SigningKey::generate_es256("k");
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let payload = token.split('.').nth(1).unwrap();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","kid":"k"}"#);
        let forged = format!("{header}.{payload}.");
        let err = validator_for(&key, internal_options())
            .validate(&forged)
            .unwrap_err();
        assert!(matches!(err, TokenError::DisallowedAlgorithm(alg) if alg == "none"));
    }

    #[test]
    fn key_type_confusion_is_rejected() {
        // Sign with HMAC but claim the kid of an ES256 key, with HS256
        // allowed: resolved key algorithm must still match the header.
        let es_key = SigningKey::generate_es256("shared-kid");
        let hmac_key = SigningKey::from_hmac_secret("shared-kid", vec![3u8; 32]).unwrap();
        let token = sign_claims(&hmac_key, &internal_claims()).unwrap();
        let options = internal_options().allow_algorithm(SigningAlgorithm::HS256);
        let err = validator_for(&es_key, options)
            .validate(&token)
            .unwrap_err();
        assert!(matches!(err, TokenError::AlgorithmKeyMismatch { .. }));
    }

    #[test]
    fn missing_and_unknown_kid_are_rejected() {
        let key = SigningKey::generate_es256("known");
        let claims = internal_claims();

        let token = sign_claims(&key, &claims).unwrap();
        let other = SigningKey::generate_es256("other");
        let err = validator_for(&other, internal_options())
            .validate(&token)
            .unwrap_err();
        assert!(matches!(err, TokenError::UnknownKeyId { kid } if kid == "known"));

        // Header without kid.
        let payload = token.split('.').nth(1).unwrap().to_string();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256"}"#);
        let forged = format!("{header}.{payload}.AA");
        let err = validator_for(&key, internal_options())
            .validate(&forged)
            .unwrap_err();
        assert!(matches!(err, TokenError::MissingKeyId));
    }

    #[test]
    fn tampered_payload_fails_signature_check() {
        let key = SigningKey::generate_es256("k");
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let mut parts: Vec<&str> = token.split('.').collect();

        let mut tampered = internal_claims();
        tampered.permissions = vec!["invoice:admin".to_string()];
        let forged_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&tampered).unwrap());
        parts[1] = &forged_payload;
        let forged = parts.join(".");

        let err = validator_for(&key, internal_options())
            .validate(&forged)
            .unwrap_err();
        assert!(matches!(err, TokenError::InvalidSignature));
    }

    #[test]
    fn expired_token_is_rejected_with_skew_tolerance() {
        let key = SigningKey::generate_es256("k");
        let claims = internal_claims();
        let token = sign_claims(&key, &claims).unwrap();
        let validator = validator_for(&key, internal_options());

        let exp = DateTime::from_timestamp(claims.exp, 0).unwrap();
        // 10 seconds past expiry but within the 30s skew: still accepted.
        assert!(
            validator
                .validate_at(&token, exp + Duration::seconds(10))
                .is_ok()
        );
        // Past expiry plus skew: rejected.
        let err = validator
            .validate_at(&token, exp + Duration::seconds(31))
            .unwrap_err();
        assert!(matches!(err, TokenError::Expired));
    }

    #[test]
    fn not_yet_valid_token_is_rejected() {
        let key = SigningKey::generate_es256("k");
        let mut claims = internal_claims();
        claims.nbf = Some(claims.iat + 300);
        let token = sign_claims(&key, &claims).unwrap();
        let validator = validator_for(&key, internal_options());

        let now = DateTime::from_timestamp(claims.iat, 0).unwrap();
        let err = validator.validate_at(&token, now).unwrap_err();
        assert!(matches!(err, TokenError::NotYetValid));
        // Within skew of nbf: accepted.
        assert!(
            validator
                .validate_at(&token, now + Duration::seconds(271))
                .is_ok()
        );
    }

    #[test]
    fn wrong_issuer_is_rejected() {
        let key = SigningKey::generate_es256("k");
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let options = ValidationOptions::new(
            "https://other-authority",
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::InternalService],
        );
        let err = validator_for(&key, options).validate(&token).unwrap_err();
        assert!(matches!(err, TokenError::IssuerMismatch { .. }));
    }

    #[test]
    fn wrong_audience_is_rejected() {
        let key = SigningKey::generate_es256("k");
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let options = ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("billing-service".to_string()),
            vec![TokenType::InternalService],
        );
        let err = validator_for(&key, options).validate(&token).unwrap_err();
        assert!(matches!(err, TokenError::AudienceMismatch { .. }));
    }

    #[test]
    fn wrong_token_type_is_rejected() {
        let key = SigningKey::generate_es256("k");
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let options = ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::GatewayAccess],
        );
        let err = validator_for(&key, options).validate(&token).unwrap_err();
        assert!(matches!(
            err,
            TokenError::TokenTypeMismatch {
                actual: TokenType::InternalService,
                ..
            }
        ));
    }

    #[test]
    fn web_session_requires_absent_audience_policy() {
        let key = SigningKey::generate_es256("k");
        let claims = RasClaims::web_session(
            ISSUER,
            "alice",
            BTreeMap::from([(
                "invoice-service".to_string(),
                vec!["invoice:read".to_string()],
            )]),
            Duration::minutes(30),
        );
        let token = sign_claims(&key, &claims).unwrap();

        let options =
            ValidationOptions::new(ISSUER, AudiencePolicy::Absent, vec![TokenType::WebSession]);
        let validated = validator_for(&key, options).validate(&token).unwrap();
        assert_eq!(
            validated.permissions_for_audience("invoice-service"),
            Some(&["invoice:read".to_string()][..])
        );

        // The same web session must not pass an Exact-audience validator.
        let options = ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::WebSession],
        );
        let err = validator_for(&key, options).validate(&token).unwrap_err();
        assert!(matches!(err, TokenError::AudienceMismatch { .. }));
    }

    #[test]
    fn validation_works_through_serialized_jwks() {
        let key = SigningKey::generate_es256("k1");
        let mut ring = KeyRing::new(key);
        let token_old = ring.sign(&internal_claims()).unwrap();
        ring.rotate(SigningKey::generate_ed25519("k2"));
        let token_new = ring.sign(&internal_claims()).unwrap();

        // Serialize the JWKS as a downstream service would fetch it.
        let json = serde_json::to_string(&ring.jwks()).unwrap();
        let jwks: JwkSet = serde_json::from_str(&json).unwrap();
        let validator = TokenValidator::new(jwks, internal_options());

        assert!(validator.validate(&token_old).is_ok());
        assert!(validator.validate(&token_new).is_ok());

        // Emergency removal: rebuild JWKS without k1 and the old token dies.
        ring.remove_retired("k1");
        let validator = TokenValidator::new(ring.jwks(), internal_options());
        assert!(matches!(
            validator.validate(&token_old).unwrap_err(),
            TokenError::UnknownKeyId { .. }
        ));
        assert!(validator.validate(&token_new).is_ok());
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let key = SigningKey::generate_es256("k");
        let validator = validator_for(&key, internal_options());
        for token in ["", "a.b", "a.b.c.d", "!!!.###.$$$"] {
            assert!(matches!(
                validator.validate(token).unwrap_err(),
                TokenError::Malformed(_)
            ));
        }
    }
}
