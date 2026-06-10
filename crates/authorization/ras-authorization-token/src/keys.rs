//! Signing/verification key material and JWKS types.
//!
//! Supports three algorithms:
//!
//! - `ES256` (ECDSA P-256) — recommended default; widest interop with
//!   existing infrastructure (Envoy, Istio, API gateways).
//! - `EdDSA` (Ed25519) — compact modern alternative.
//! - `HS256` (HMAC) — shared-secret mode for embedded single-process
//!   deployments only. HMAC keys never appear in JWKS.

use std::fmt;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use p256::pkcs8::{DecodePrivateKey as _, EncodePrivateKey as _, LineEnding};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::error::TokenError;

/// Supported JWS signing algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SigningAlgorithm {
    /// HMAC-SHA256 shared secret. Embedded/dev mode only.
    #[serde(rename = "HS256")]
    HS256,
    /// ECDSA over P-256 with SHA-256.
    #[serde(rename = "ES256")]
    ES256,
    /// Ed25519.
    #[serde(rename = "EdDSA")]
    EdDSA,
}

impl SigningAlgorithm {
    /// The JOSE `alg` header value.
    pub fn name(&self) -> &'static str {
        match self {
            Self::HS256 => "HS256",
            Self::ES256 => "ES256",
            Self::EdDSA => "EdDSA",
        }
    }

    /// Parse a JOSE `alg` header value. Unknown algorithms (including
    /// `none`) return `None` and must be rejected by callers.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "HS256" => Some(Self::HS256),
            "ES256" => Some(Self::ES256),
            "EdDSA" => Some(Self::EdDSA),
            _ => None,
        }
    }
}

impl fmt::Display for SigningAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Byte secret that redacts itself in `Debug` and never derives serde
/// serialization, so HMAC secrets cannot leak through logs or accidental
/// serialization.
#[derive(Clone)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Access the raw secret. Deliberately verbose so call sites are easy
    /// to audit.
    pub fn expose_secret(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretBytes(<redacted>)")
    }
}

enum SigningKeyMaterial {
    Hmac(SecretBytes),
    Es256(Box<p256::ecdsa::SigningKey>),
    Ed25519(Box<ed25519_dalek::SigningKey>),
}

/// A private signing key with its key id.
///
/// `Debug` prints only the kid and algorithm; private material is never
/// formatted.
pub struct SigningKey {
    kid: String,
    material: SigningKeyMaterial,
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey")
            .field("kid", &self.kid)
            .field("algorithm", &self.algorithm())
            .finish_non_exhaustive()
    }
}

impl SigningKey {
    /// Generate a fresh ES256 (P-256) key.
    pub fn generate_es256(kid: impl Into<String>) -> Self {
        Self {
            kid: kid.into(),
            material: SigningKeyMaterial::Es256(Box::new(p256::ecdsa::SigningKey::random(
                &mut rand_core::OsRng,
            ))),
        }
    }

    /// Generate a fresh Ed25519 key.
    pub fn generate_ed25519(kid: impl Into<String>) -> Self {
        Self {
            kid: kid.into(),
            material: SigningKeyMaterial::Ed25519(Box::new(ed25519_dalek::SigningKey::generate(
                &mut rand_core::OsRng,
            ))),
        }
    }

    /// Build an HS256 key from a shared secret (embedded/dev mode only).
    /// The secret must be at least 32 bytes.
    pub fn from_hmac_secret(
        kid: impl Into<String>,
        secret: impl Into<Vec<u8>>,
    ) -> Result<Self, TokenError> {
        let secret = secret.into();
        if secret.len() < 32 {
            return Err(TokenError::InvalidKey(
                "HMAC secret must be at least 32 bytes".to_string(),
            ));
        }
        Ok(Self {
            kid: kid.into(),
            material: SigningKeyMaterial::Hmac(SecretBytes::new(secret)),
        })
    }

    /// Import an asymmetric private key from PKCS#8 PEM.
    pub fn from_pkcs8_pem(
        kid: impl Into<String>,
        algorithm: SigningAlgorithm,
        pem: &str,
    ) -> Result<Self, TokenError> {
        let material = match algorithm {
            SigningAlgorithm::ES256 => SigningKeyMaterial::Es256(Box::new(
                p256::ecdsa::SigningKey::from_pkcs8_pem(pem)
                    .map_err(|err| TokenError::InvalidKey(format!("invalid ES256 PEM: {err}")))?,
            )),
            SigningAlgorithm::EdDSA => SigningKeyMaterial::Ed25519(Box::new(
                ed25519_dalek::SigningKey::from_pkcs8_pem(pem)
                    .map_err(|err| TokenError::InvalidKey(format!("invalid Ed25519 PEM: {err}")))?,
            )),
            SigningAlgorithm::HS256 => {
                return Err(TokenError::InvalidKey(
                    "HMAC keys are raw secrets, not PKCS#8 documents".to_string(),
                ));
            }
        };
        Ok(Self {
            kid: kid.into(),
            material,
        })
    }

    /// Export the private key as PKCS#8 PEM for persistence.
    ///
    /// The returned string is sensitive material; treat it like any other
    /// secret. HMAC keys cannot be exported this way.
    pub fn to_pkcs8_pem(&self) -> Result<String, TokenError> {
        match &self.material {
            SigningKeyMaterial::Es256(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map(|pem| pem.to_string())
                .map_err(|err| TokenError::InvalidKey(format!("ES256 PEM export failed: {err}"))),
            SigningKeyMaterial::Ed25519(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map(|pem| pem.to_string())
                .map_err(|err| TokenError::InvalidKey(format!("Ed25519 PEM export failed: {err}"))),
            SigningKeyMaterial::Hmac(_) => Err(TokenError::InvalidKey(
                "HMAC keys cannot be exported as PKCS#8".to_string(),
            )),
        }
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn algorithm(&self) -> SigningAlgorithm {
        match &self.material {
            SigningKeyMaterial::Hmac(_) => SigningAlgorithm::HS256,
            SigningKeyMaterial::Es256(_) => SigningAlgorithm::ES256,
            SigningKeyMaterial::Ed25519(_) => SigningAlgorithm::EdDSA,
        }
    }

    /// The verification counterpart of this key. For HMAC this carries the
    /// shared secret (and therefore must stay inside the trust boundary).
    pub fn verifying_key(&self) -> VerifyingKey {
        let material = match &self.material {
            SigningKeyMaterial::Hmac(secret) => VerifyingKeyMaterial::Hmac(secret.clone()),
            SigningKeyMaterial::Es256(key) => {
                VerifyingKeyMaterial::Es256(p256::ecdsa::VerifyingKey::from(key.as_ref()))
            }
            SigningKeyMaterial::Ed25519(key) => VerifyingKeyMaterial::Ed25519(key.verifying_key()),
        };
        VerifyingKey {
            kid: self.kid.clone(),
            material,
        }
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Result<Vec<u8>, TokenError> {
        match &self.material {
            SigningKeyMaterial::Hmac(secret) => {
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.expose_secret())
                    .map_err(|err| TokenError::InvalidKey(format!("invalid HMAC secret: {err}")))?;
                mac.update(message);
                Ok(mac.finalize().into_bytes().to_vec())
            }
            SigningKeyMaterial::Es256(key) => {
                use p256::ecdsa::signature::Signer;
                let signature: p256::ecdsa::Signature = key.sign(message);
                Ok(signature.to_bytes().to_vec())
            }
            SigningKeyMaterial::Ed25519(key) => {
                use ed25519_dalek::Signer;
                Ok(key.sign(message).to_bytes().to_vec())
            }
        }
    }
}

#[derive(Clone)]
enum VerifyingKeyMaterial {
    Hmac(SecretBytes),
    Es256(p256::ecdsa::VerifyingKey),
    Ed25519(ed25519_dalek::VerifyingKey),
}

/// A verification key with its key id.
#[derive(Clone)]
pub struct VerifyingKey {
    kid: String,
    material: VerifyingKeyMaterial,
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifyingKey")
            .field("kid", &self.kid)
            .field("algorithm", &self.algorithm())
            .finish_non_exhaustive()
    }
}

impl VerifyingKey {
    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn algorithm(&self) -> SigningAlgorithm {
        match &self.material {
            VerifyingKeyMaterial::Hmac(_) => SigningAlgorithm::HS256,
            VerifyingKeyMaterial::Es256(_) => SigningAlgorithm::ES256,
            VerifyingKeyMaterial::Ed25519(_) => SigningAlgorithm::EdDSA,
        }
    }

    /// Verify `signature` over `message`. All failures collapse to
    /// [`TokenError::InvalidSignature`]; callers learn nothing about why.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), TokenError> {
        match &self.material {
            VerifyingKeyMaterial::Hmac(secret) => {
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.expose_secret())
                    .map_err(|_| TokenError::InvalidSignature)?;
                mac.update(message);
                mac.verify_slice(signature)
                    .map_err(|_| TokenError::InvalidSignature)
            }
            VerifyingKeyMaterial::Es256(key) => {
                use p256::ecdsa::signature::Verifier;
                let signature = p256::ecdsa::Signature::from_slice(signature)
                    .map_err(|_| TokenError::InvalidSignature)?;
                key.verify(message, &signature)
                    .map_err(|_| TokenError::InvalidSignature)
            }
            VerifyingKeyMaterial::Ed25519(key) => {
                use ed25519_dalek::Verifier;
                let bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| TokenError::InvalidSignature)?;
                let signature = ed25519_dalek::Signature::from_bytes(&bytes);
                key.verify(message, &signature)
                    .map_err(|_| TokenError::InvalidSignature)
            }
        }
    }

    /// JWK representation. `None` for HMAC keys: shared secrets must never
    /// be published through a JWKS document.
    pub fn to_jwk(&self) -> Option<Jwk> {
        match &self.material {
            VerifyingKeyMaterial::Hmac(_) => None,
            VerifyingKeyMaterial::Es256(key) => {
                let point = key.to_encoded_point(false);
                Some(Jwk {
                    kty: "EC".to_string(),
                    crv: Some("P-256".to_string()),
                    x: point.x().map(|x| URL_SAFE_NO_PAD.encode(x)),
                    y: point.y().map(|y| URL_SAFE_NO_PAD.encode(y)),
                    kid: self.kid.clone(),
                    alg: Some(SigningAlgorithm::ES256.name().to_string()),
                    key_use: Some("sig".to_string()),
                })
            }
            VerifyingKeyMaterial::Ed25519(key) => Some(Jwk {
                kty: "OKP".to_string(),
                crv: Some("Ed25519".to_string()),
                x: Some(URL_SAFE_NO_PAD.encode(key.to_bytes())),
                y: None,
                kid: self.kid.clone(),
                alg: Some(SigningAlgorithm::EdDSA.name().to_string()),
                key_use: Some("sig".to_string()),
            }),
        }
    }

    /// Build a verification key from a JWK entry.
    pub fn from_jwk(jwk: &Jwk) -> Result<Self, TokenError> {
        let decode = |field: &Option<String>, name: &str| -> Result<Vec<u8>, TokenError> {
            let value = field
                .as_deref()
                .ok_or_else(|| TokenError::InvalidKey(format!("JWK missing {name}")))?;
            URL_SAFE_NO_PAD.decode(value).map_err(|err| {
                TokenError::InvalidKey(format!("JWK {name} is not base64url: {err}"))
            })
        };

        let material = match (jwk.kty.as_str(), jwk.crv.as_deref()) {
            ("EC", Some("P-256")) => {
                let x = decode(&jwk.x, "x")?;
                let y = decode(&jwk.y, "y")?;
                if x.len() != 32 || y.len() != 32 {
                    return Err(TokenError::InvalidKey(
                        "P-256 coordinates must be 32 bytes".to_string(),
                    ));
                }
                let mut sec1 = Vec::with_capacity(65);
                sec1.push(0x04);
                sec1.extend_from_slice(&x);
                sec1.extend_from_slice(&y);
                VerifyingKeyMaterial::Es256(
                    p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1).map_err(|err| {
                        TokenError::InvalidKey(format!("invalid P-256 point: {err}"))
                    })?,
                )
            }
            ("OKP", Some("Ed25519")) => {
                let x = decode(&jwk.x, "x")?;
                let bytes: [u8; 32] = x.as_slice().try_into().map_err(|_| {
                    TokenError::InvalidKey("Ed25519 public key must be 32 bytes".to_string())
                })?;
                VerifyingKeyMaterial::Ed25519(
                    ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|err| {
                        TokenError::InvalidKey(format!("invalid Ed25519 key: {err}"))
                    })?,
                )
            }
            (kty, crv) => {
                return Err(TokenError::InvalidKey(format!(
                    "unsupported JWK key type {kty:?} with curve {crv:?}"
                )));
            }
        };

        Ok(Self {
            kid: jwk.kid.clone(),
            material,
        })
    }
}

/// A single JSON Web Key (public verification keys only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    pub kid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    #[serde(rename = "use", default, skip_serializing_if = "Option::is_none")]
    pub key_use: Option<String>,
}

/// A JWKS document as served from an authority's JWKS endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

impl JwkSet {
    pub fn find(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|key| key.kid == kid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_key_material() {
        let secret = SecretBytes::new(vec![7u8; 32]);
        assert_eq!(format!("{secret:?}"), "SecretBytes(<redacted>)");

        let key = SigningKey::generate_es256("k1");
        let debug = format!("{key:?}");
        assert!(debug.contains("k1"));
        assert!(debug.contains("ES256"));
        assert!(!debug.contains("Es256("));

        let hmac = SigningKey::from_hmac_secret("k2", vec![7u8; 32]).unwrap();
        let debug = format!("{:?}", hmac.verifying_key());
        assert!(!debug.contains('7'));
    }

    #[test]
    fn hmac_secret_must_be_long_enough() {
        let err = SigningKey::from_hmac_secret("k1", b"short".to_vec()).unwrap_err();
        assert!(matches!(err, TokenError::InvalidKey(_)));
    }

    #[test]
    fn sign_verify_round_trip_per_algorithm() {
        let message = b"signing input";
        for key in [
            SigningKey::generate_es256("es"),
            SigningKey::generate_ed25519("ed"),
            SigningKey::from_hmac_secret("hs", vec![9u8; 32]).unwrap(),
        ] {
            let signature = key.sign(message).unwrap();
            let verifier = key.verifying_key();
            verifier.verify(message, &signature).unwrap();
            assert!(verifier.verify(b"other input", &signature).is_err());
            let mut tampered = signature.clone();
            tampered[0] ^= 0xff;
            assert!(verifier.verify(message, &tampered).is_err());
        }
    }

    #[test]
    fn jwk_round_trip_es256_and_ed25519() {
        for key in [
            SigningKey::generate_es256("es"),
            SigningKey::generate_ed25519("ed"),
        ] {
            let verifier = key.verifying_key();
            let jwk = verifier.to_jwk().expect("asymmetric keys have JWKs");
            assert_eq!(jwk.kid, key.kid());
            let restored = VerifyingKey::from_jwk(&jwk).unwrap();
            let signature = key.sign(b"msg").unwrap();
            restored.verify(b"msg", &signature).unwrap();
        }
    }

    #[test]
    fn hmac_keys_are_excluded_from_jwk() {
        let key = SigningKey::from_hmac_secret("hs", vec![9u8; 32]).unwrap();
        assert!(key.verifying_key().to_jwk().is_none());
    }

    #[test]
    fn jwk_with_unsupported_type_is_rejected() {
        let jwk = Jwk {
            kty: "RSA".to_string(),
            crv: None,
            x: None,
            y: None,
            kid: "k".to_string(),
            alg: None,
            key_use: None,
        };
        assert!(matches!(
            VerifyingKey::from_jwk(&jwk),
            Err(TokenError::InvalidKey(_))
        ));
    }

    #[test]
    fn pkcs8_export_import_round_trip() {
        for (key, alg) in [
            (SigningKey::generate_es256("es"), SigningAlgorithm::ES256),
            (SigningKey::generate_ed25519("ed"), SigningAlgorithm::EdDSA),
        ] {
            let pem = key.to_pkcs8_pem().unwrap();
            let restored = SigningKey::from_pkcs8_pem(key.kid(), alg, &pem).unwrap();
            let signature = restored.sign(b"msg").unwrap();
            key.verifying_key().verify(b"msg", &signature).unwrap();
        }
    }

    #[test]
    fn hmac_keys_cannot_use_pkcs8() {
        let key = SigningKey::from_hmac_secret("hs", vec![9u8; 32]).unwrap();
        assert!(key.to_pkcs8_pem().is_err());
        assert!(SigningKey::from_pkcs8_pem("hs", SigningAlgorithm::HS256, "ignored").is_err());
    }

    #[test]
    fn algorithm_names_round_trip_and_none_is_rejected() {
        for alg in [
            SigningAlgorithm::HS256,
            SigningAlgorithm::ES256,
            SigningAlgorithm::EdDSA,
        ] {
            assert_eq!(SigningAlgorithm::from_name(alg.name()), Some(alg));
        }
        assert_eq!(SigningAlgorithm::from_name("none"), None);
        assert_eq!(SigningAlgorithm::from_name("RS256"), None);
    }
}
