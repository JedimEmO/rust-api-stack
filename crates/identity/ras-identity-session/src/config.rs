use crate::{DEFAULT_MAX_SESSIONS_PER_USER, JwtAlgorithm, SessionError};
use chrono::Duration;
use std::collections::HashSet;

/// Session/JWT configuration.
///
/// Permission semantics: the permissions granted at [`crate::SessionService::begin_session`]
/// are frozen into the JWT and are **not** reloaded on verify. With the default
/// `enforce_active_sessions: true`, revoking a session (`end_session`) takes
/// effect immediately per-`jti`; otherwise grants are fixed for `jwt_ttl`
/// (default 24h).
///
/// `iss` and `aud` are **required by default** (S2): [`SessionConfig::new`] and
/// [`crate::SessionService::new`] fail unless both are set (see
/// [`SessionConfig::with_issuer`] / [`SessionConfig::with_audience`]) or the
/// deployment explicitly opts out with [`SessionConfig::allow_unscoped_tokens`].
/// This keeps tokens minted for one service from being accepted by another that
/// shares the same secret.
///
/// When `enforce_active_sessions` is on, expired entries are only swept lazily
/// (at most once per minute, from `begin_session`/`verify_session`), so start
/// [`crate::SessionService::start_cleanup_task`] to keep the store bounded during
/// traffic lulls.
#[derive(Clone)]
pub struct SessionConfig {
    pub jwt_secret: String,
    pub jwt_ttl: Duration,
    pub enforce_active_sessions: bool,
    pub algorithm: JwtAlgorithm,
    /// Expected token issuer. Encoded into new tokens and verified on
    /// `verify_session`; a mismatch is rejected. Required unless
    /// `require_iss_aud` is false.
    pub iss: Option<String>,
    /// Expected token audience. Encoded into new tokens and verified on
    /// `verify_session`; a token for a different `aud` is rejected. This is the
    /// cross-service confused-deputy guard. Required unless
    /// `require_iss_aud` is false.
    pub aud: Option<String>,
    /// When true (default), validation fails if `iss` or `aud` is `None`.
    /// Set to false via [`SessionConfig::allow_unscoped_tokens`] only for
    /// single-service deployments that never share a secret (S2).
    pub require_iss_aud: bool,
    /// Maximum concurrently tracked sessions per `sub` when
    /// `enforce_active_sessions` is on. Once reached, the oldest session (by
    /// `iat`) is evicted when a new one begins (S5). Default 32.
    pub max_sessions_per_user: usize,
}

/// Redacting `Debug` so `jwt_secret` never lands in logs.
impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConfig")
            .field("jwt_secret", &"[REDACTED]")
            .field("jwt_ttl", &self.jwt_ttl)
            .field("enforce_active_sessions", &self.enforce_active_sessions)
            .field("algorithm", &self.algorithm)
            .field("iss", &self.iss)
            .field("aud", &self.aud)
            .field("require_iss_aud", &self.require_iss_aud)
            .field("max_sessions_per_user", &self.max_sessions_per_user)
            .finish()
    }
}

impl SessionConfig {
    /// Build a config with an issuer and audience. Both are required by
    /// default; use [`SessionConfig::allow_unscoped_tokens`] on the result to
    /// opt out for single-service deployments.
    pub fn new(
        jwt_secret: impl Into<String>,
        issuer: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self, SessionError> {
        let config = Self {
            jwt_secret: jwt_secret.into(),
            jwt_ttl: Duration::hours(24),
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
            iss: Some(issuer.into()),
            aud: Some(audience.into()),
            require_iss_aud: true,
            max_sessions_per_user: DEFAULT_MAX_SESSIONS_PER_USER,
        };
        config.validate()?;
        Ok(config)
    }

    /// Build a config **without** an issuer/audience. Only valid for
    /// single-service deployments that never share `jwt_secret`; any token
    /// signed with the secret is accepted regardless of which service minted
    /// it. Equivalent to `new(..)` followed by [`allow_unscoped_tokens`].
    ///
    /// [`allow_unscoped_tokens`]: SessionConfig::allow_unscoped_tokens
    pub fn new_unscoped(jwt_secret: impl Into<String>) -> Result<Self, SessionError> {
        let config = Self {
            jwt_secret: jwt_secret.into(),
            jwt_ttl: Duration::hours(24),
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
            iss: None,
            aud: None,
            require_iss_aud: false,
            max_sessions_per_user: DEFAULT_MAX_SESSIONS_PER_USER,
        };
        config.validate()?;
        Ok(config)
    }

    /// Explicit opt-out from the `iss`/`aud` requirement (S2). Only for
    /// single-service deployments where the secret is never shared.
    pub fn allow_unscoped_tokens(mut self) -> Self {
        self.require_iss_aud = false;
        self
    }

    /// Cap concurrently tracked sessions per user (S5). Must be at least 1.
    pub fn with_max_sessions_per_user(mut self, max: usize) -> Self {
        self.max_sessions_per_user = max;
        self
    }

    /// Set the expected issuer (`iss`). Production services should set this.
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.iss = Some(issuer.into());
        self
    }

    /// Set the expected audience (`aud`). Production services should set this so
    /// a token minted for another service is rejected here.
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.aud = Some(audience.into());
        self
    }

    pub fn validate(&self) -> Result<(), SessionError> {
        validate_jwt_secret(&self.jwt_secret)?;

        if self.jwt_ttl <= Duration::zero() {
            return Err(SessionError::InvalidConfig(
                "jwt_ttl must be positive".to_string(),
            ));
        }

        if self.require_iss_aud && (self.iss.is_none() || self.aud.is_none()) {
            return Err(SessionError::InvalidConfig(
                "iss and aud must be set (use with_issuer/with_audience), or opt out \
                 explicitly with allow_unscoped_tokens() for single-service deployments"
                    .to_string(),
            ));
        }

        if self.max_sessions_per_user == 0 {
            return Err(SessionError::InvalidConfig(
                "max_sessions_per_user must be at least 1".to_string(),
            ));
        }

        Ok(())
    }
}

/// Minimum number of distinct byte values a secret must contain (S3).
///
/// Ten keeps `openssl rand -hex 24` (16 possible symbols) passing in
/// practice while still rejecting repeated-pattern strings.
const MIN_DISTINCT_SECRET_BYTES: usize = 10;
/// Longest permitted run of one repeated byte in a secret (S3).
const MAX_REPEATED_SECRET_BYTES: usize = 7;

/// Substrings (matched case-insensitively) that mark a secret as a
/// placeholder rather than random key material (S3).
const INSECURE_SECRET_SUBSTRINGS: &[&str] = &[
    "change-me",
    "changeme",
    "secret",
    "password",
    "example",
    "placeholder",
    "test-secret",
    "dev-secret",
    "insecure",
    "12345678",
    "abcdefgh",
    "your-secret",
];

pub(super) fn validate_jwt_secret(secret: &str) -> Result<(), SessionError> {
    let trimmed = secret.trim();

    if trimmed.len() < 32 {
        return Err(SessionError::InvalidConfig(
            "jwt_secret must be at least 32 bytes".to_string(),
        ));
    }

    let lowered = trimmed.to_ascii_lowercase();
    if INSECURE_SECRET_SUBSTRINGS
        .iter()
        .any(|placeholder| lowered.contains(placeholder))
    {
        return Err(SessionError::InvalidConfig(
            "jwt_secret must not contain a placeholder value".to_string(),
        ));
    }

    let distinct: HashSet<u8> = trimmed.bytes().collect();
    if distinct.len() < MIN_DISTINCT_SECRET_BYTES {
        return Err(SessionError::InvalidConfig(format!(
            "jwt_secret must contain at least {MIN_DISTINCT_SECRET_BYTES} distinct byte values"
        )));
    }

    let mut run = 0usize;
    let mut prev = None;
    for byte in trimmed.bytes() {
        run = if prev == Some(byte) { run + 1 } else { 1 };
        if run > MAX_REPEATED_SECRET_BYTES {
            return Err(SessionError::InvalidConfig(format!(
                "jwt_secret must not repeat one byte more than {MAX_REPEATED_SECRET_BYTES} times in a row"
            )));
        }
        prev = Some(byte);
    }

    Ok(())
}
