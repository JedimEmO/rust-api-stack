//! Error types for token signing and validation.

use thiserror::Error;

use crate::claims::TokenType;

/// Errors produced while signing, encoding, decoding, or validating RAS tokens.
#[derive(Debug, Error)]
pub enum TokenError {
    /// Serialization of the header or claims failed.
    #[error("failed to encode token: {0}")]
    Encoding(String),

    /// The token is structurally invalid (wrong segment count, bad base64, bad JSON).
    #[error("malformed token: {0}")]
    Malformed(String),

    /// The signature did not verify against the resolved key.
    #[error("token signature is invalid")]
    InvalidSignature,

    /// The token header declares an algorithm outside the validator's allowlist.
    #[error("token algorithm {0:?} is not allowed")]
    DisallowedAlgorithm(String),

    /// The token header carries no `kid`.
    #[error("token key id is missing")]
    MissingKeyId,

    /// No verification key is known for the token's `kid`.
    #[error("no verification key found for key id {kid:?}")]
    UnknownKeyId { kid: String },

    /// The resolved key's algorithm does not match the header algorithm.
    /// Guards against key-type confusion attacks.
    #[error("key/header algorithm mismatch: key uses {key}, header declares {header}")]
    AlgorithmKeyMismatch { key: String, header: String },

    /// The token's `exp` is in the past (beyond clock skew).
    #[error("token has expired")]
    Expired,

    /// The token's `nbf` is in the future (beyond clock skew).
    #[error("token is not yet valid")]
    NotYetValid,

    /// The token's `iss` does not match the expected issuer.
    #[error("token issuer mismatch: expected {expected:?}, got {actual:?}")]
    IssuerMismatch { expected: String, actual: String },

    /// The token's `aud` does not satisfy the validator's audience policy.
    #[error("token audience mismatch: expected {expected:?}, got {actual:?}")]
    AudienceMismatch {
        expected: Option<String>,
        actual: Option<String>,
    },

    /// The token's `typ` is not one of the expected token types.
    #[error("token type mismatch: expected one of {expected:?}, got {actual:?}")]
    TokenTypeMismatch {
        expected: Vec<TokenType>,
        actual: TokenType,
    },

    /// The claims violate the structural invariants of their token type.
    #[error("invalid token claims: {0}")]
    InvalidClaims(String),

    /// Key material could not be constructed, imported, or exported.
    #[error("invalid key material: {0}")]
    InvalidKey(String),
}
