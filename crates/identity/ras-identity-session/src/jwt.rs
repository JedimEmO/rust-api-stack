use crate::SessionError;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Sha256, Sha384, Sha512};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JwtAlgorithm {
    #[serde(rename = "HS256")]
    HS256,
    #[serde(rename = "HS384")]
    HS384,
    #[serde(rename = "HS512")]
    HS512,
}

impl JwtAlgorithm {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "HS256" => Some(Self::HS256),
            "HS384" => Some(Self::HS384),
            "HS512" => Some(Self::HS512),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct JwtHeader {
    typ: &'static str,
    alg: JwtAlgorithm,
}

#[derive(Deserialize)]
struct DecodedJwtHeader {
    alg: JwtAlgorithm,
}

fn jwt_error(message: impl Into<String>) -> SessionError {
    SessionError::JwtError(message.into())
}

pub(super) fn encode_jwt<T: Serialize>(
    claims: &T,
    secret: &str,
    algorithm: JwtAlgorithm,
) -> Result<String, SessionError> {
    let header = JwtHeader {
        typ: "JWT",
        alg: algorithm,
    };
    let header = serde_json::to_vec(&header)
        .map_err(|err| jwt_error(format!("failed to encode JWT header: {err}")))?;
    let claims = serde_json::to_vec(claims)
        .map_err(|err| jwt_error(format!("failed to encode JWT claims: {err}")))?;

    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(claims)
    );
    let signature = sign_jwt(&signing_input, secret.as_bytes(), algorithm)?;

    Ok(format!(
        "{}.{}",
        signing_input,
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

pub(super) fn decode_jwt<T: DeserializeOwned>(
    token: &str,
    secret: &str,
    expected_algorithm: JwtAlgorithm,
) -> Result<T, SessionError> {
    let mut parts = token.split('.');
    let encoded_header = parts
        .next()
        .ok_or_else(|| jwt_error("missing JWT header"))?;
    let encoded_claims = parts
        .next()
        .ok_or_else(|| jwt_error("missing JWT claims"))?;
    let encoded_signature = parts
        .next()
        .ok_or_else(|| jwt_error("missing JWT signature"))?;

    if parts.next().is_some() {
        return Err(jwt_error("JWT has too many segments"));
    }

    let header = URL_SAFE_NO_PAD
        .decode(encoded_header)
        .map_err(|err| jwt_error(format!("invalid JWT header encoding: {err}")))?;
    let header: DecodedJwtHeader = serde_json::from_slice(&header)
        .map_err(|err| jwt_error(format!("invalid JWT header: {err}")))?;

    if header.alg != expected_algorithm {
        return Err(jwt_error("unexpected JWT algorithm"));
    }

    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|err| jwt_error(format!("invalid JWT signature encoding: {err}")))?;
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    verify_jwt_signature(
        &signing_input,
        secret.as_bytes(),
        expected_algorithm,
        &signature,
    )?;

    let claims = URL_SAFE_NO_PAD
        .decode(encoded_claims)
        .map_err(|err| jwt_error(format!("invalid JWT claims encoding: {err}")))?;
    serde_json::from_slice(&claims).map_err(|err| jwt_error(format!("invalid JWT claims: {err}")))
}

fn sign_jwt(
    signing_input: &str,
    secret: &[u8],
    algorithm: JwtAlgorithm,
) -> Result<Vec<u8>, SessionError> {
    match algorithm {
        JwtAlgorithm::HS256 => {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            Ok(mac.finalize().into_bytes().to_vec())
        }
        JwtAlgorithm::HS384 => {
            let mut mac = Hmac::<Sha384>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            Ok(mac.finalize().into_bytes().to_vec())
        }
        JwtAlgorithm::HS512 => {
            let mut mac = Hmac::<Sha512>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            Ok(mac.finalize().into_bytes().to_vec())
        }
    }
}

fn verify_jwt_signature(
    signing_input: &str,
    secret: &[u8],
    algorithm: JwtAlgorithm,
    signature: &[u8],
) -> Result<(), SessionError> {
    match algorithm {
        JwtAlgorithm::HS256 => {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            mac.verify_slice(signature)
                .map_err(|_| jwt_error("invalid JWT signature"))
        }
        JwtAlgorithm::HS384 => {
            let mut mac = Hmac::<Sha384>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            mac.verify_slice(signature)
                .map_err(|_| jwt_error("invalid JWT signature"))
        }
        JwtAlgorithm::HS512 => {
            let mut mac = Hmac::<Sha512>::new_from_slice(secret)
                .map_err(|err| jwt_error(format!("invalid JWT secret: {err}")))?;
            mac.update(signing_input.as_bytes());
            mac.verify_slice(signature)
                .map_err(|_| jwt_error("invalid JWT signature"))
        }
    }
}
