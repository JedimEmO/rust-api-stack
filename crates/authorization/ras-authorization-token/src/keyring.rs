//! Token encoding and signing-key rotation.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use crate::claims::RasClaims;
use crate::error::TokenError;
use crate::keys::{JwkSet, SigningKey, VerifyingKey};

#[derive(Serialize)]
struct JoseHeader<'a> {
    alg: &'a str,
    typ: &'static str,
    kid: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct DecodedHeader {
    pub alg: String,
    #[serde(default)]
    pub kid: Option<String>,
}

/// Sign `claims` with `key`, producing a compact JWS.
///
/// Claims shape is validated first, so a caller cannot sign a token that
/// violates its token-type invariants.
pub fn sign_claims(key: &SigningKey, claims: &RasClaims) -> Result<String, TokenError> {
    claims.validate_shape().map_err(TokenError::InvalidClaims)?;

    let header = JoseHeader {
        alg: key.algorithm().name(),
        typ: "JWT",
        kid: key.kid(),
    };
    let header = serde_json::to_vec(&header)
        .map_err(|err| TokenError::Encoding(format!("header serialization failed: {err}")))?;
    let payload = serde_json::to_vec(claims)
        .map_err(|err| TokenError::Encoding(format!("claims serialization failed: {err}")))?;

    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let signature = key.sign(signing_input.as_bytes())?;

    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

/// An active signing key plus retired verification keys.
///
/// Rotation keeps the old key's *verification* half so tokens signed before
/// the rotation stay valid until they expire; the private half is dropped.
/// [`KeyRing::remove_retired`] supports emergency revocation of a retired
/// key, immediately invalidating tokens signed with it.
#[derive(Debug)]
pub struct KeyRing {
    active: SigningKey,
    retired: Vec<VerifyingKey>,
}

impl KeyRing {
    pub fn new(active: SigningKey) -> Self {
        Self {
            active,
            retired: Vec::new(),
        }
    }

    pub fn active_kid(&self) -> &str {
        self.active.kid()
    }

    /// Sign claims with the active key.
    pub fn sign(&self, claims: &RasClaims) -> Result<String, TokenError> {
        sign_claims(&self.active, claims)
    }

    /// Install a new active key. The previous active key's verification half
    /// is retained so outstanding tokens remain valid.
    pub fn rotate(&mut self, new_active: SigningKey) {
        let old = std::mem::replace(&mut self.active, new_active);
        let old_verifier = old.verifying_key();
        self.retired.retain(|key| key.kid() != old_verifier.kid());
        self.retired.push(old_verifier);
    }

    /// Emergency-remove a retired verification key. Returns `true` if a key
    /// was removed. The active key cannot be removed; rotate first.
    pub fn remove_retired(&mut self, kid: &str) -> bool {
        let before = self.retired.len();
        self.retired.retain(|key| key.kid() != kid);
        self.retired.len() != before
    }

    /// All verification keys: active first, then retired.
    pub fn verifying_keys(&self) -> Vec<VerifyingKey> {
        let mut keys = vec![self.active.verifying_key()];
        keys.extend(self.retired.iter().cloned());
        keys
    }

    /// Resolve a verification key by kid.
    pub fn resolve(&self, kid: &str) -> Option<VerifyingKey> {
        if self.active.kid() == kid {
            return Some(self.active.verifying_key());
        }
        self.retired.iter().find(|key| key.kid() == kid).cloned()
    }

    /// The public JWKS document for this ring. HMAC keys are silently
    /// excluded — shared secrets are never published.
    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: self
                .verifying_keys()
                .iter()
                .filter_map(VerifyingKey::to_jwk)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::claims::{PrincipalKind, RasClaims};

    fn internal_claims() -> RasClaims {
        RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec!["invoice:read".to_string()],
            Duration::minutes(5),
        )
    }

    #[test]
    fn sign_rejects_invalid_claim_shape() {
        let key = SigningKey::generate_es256("k1");
        let mut claims = internal_claims();
        claims.aud = None;
        assert!(matches!(
            sign_claims(&key, &claims),
            Err(TokenError::InvalidClaims(_))
        ));
    }

    #[test]
    fn rotation_retains_old_verification_key() {
        let mut ring = KeyRing::new(SigningKey::generate_es256("k1"));
        assert_eq!(ring.active_kid(), "k1");

        ring.rotate(SigningKey::generate_es256("k2"));
        assert_eq!(ring.active_kid(), "k2");
        assert!(ring.resolve("k1").is_some());
        assert!(ring.resolve("k2").is_some());
        assert!(ring.resolve("k3").is_none());

        let jwks = ring.jwks();
        assert_eq!(jwks.keys.len(), 2);
        assert_eq!(jwks.keys[0].kid, "k2");
    }

    #[test]
    fn emergency_removal_drops_retired_key() {
        let mut ring = KeyRing::new(SigningKey::generate_es256("k1"));
        ring.rotate(SigningKey::generate_es256("k2"));

        assert!(ring.remove_retired("k1"));
        assert!(ring.resolve("k1").is_none());
        assert!(!ring.remove_retired("k1"));
        // Active key is not removable.
        assert!(!ring.remove_retired("k2"));
        assert!(ring.resolve("k2").is_some());
    }

    #[test]
    fn rotating_back_to_same_kid_does_not_duplicate_retired_entries() {
        let mut ring = KeyRing::new(SigningKey::generate_es256("k1"));
        ring.rotate(SigningKey::generate_es256("k2"));
        ring.rotate(SigningKey::generate_es256("k1"));
        ring.rotate(SigningKey::generate_es256("k2"));

        let kids: Vec<_> = ring
            .verifying_keys()
            .iter()
            .map(|key| key.kid().to_string())
            .collect();
        assert_eq!(kids.iter().filter(|kid| *kid == "k1").count(), 2 - 1);
        assert_eq!(kids.iter().filter(|kid| *kid == "k2").count(), 2);
    }

    #[test]
    fn jwks_excludes_hmac_keys() {
        let mut ring = KeyRing::new(SigningKey::from_hmac_secret("hs", vec![1u8; 32]).unwrap());
        assert!(ring.jwks().keys.is_empty());

        ring.rotate(SigningKey::generate_es256("es"));
        let jwks = ring.jwks();
        assert_eq!(jwks.keys.len(), 1);
        assert_eq!(jwks.keys[0].kid, "es");
    }

    #[test]
    fn header_carries_alg_kid_and_typ() {
        let key = SigningKey::generate_ed25519("ed-1");
        let token = sign_claims(&key, &internal_claims()).unwrap();
        let header_segment = token.split('.').next().unwrap();
        let header: serde_json::Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(header_segment)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(header["alg"], "EdDSA");
        assert_eq!(header["kid"], "ed-1");
        assert_eq!(header["typ"], "JWT");
    }
}
