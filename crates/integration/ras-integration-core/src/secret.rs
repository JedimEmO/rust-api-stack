//! Redacted secret string wrapper.

use std::fmt;

/// A string secret (access token, refresh token, client secret) that redacts
/// itself in `Debug`/`Display` and deliberately implements neither
/// `Serialize` nor `Deserialize`, so grants and leases cannot leak secrets
/// through logs, error chains, or accidental serde serialization.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Access the raw secret. Deliberately verbose so call sites are easy to
    /// audit.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_redact() {
        let secret = SecretString::new("super-secret-token");
        assert_eq!(format!("{secret:?}"), "SecretString(<redacted>)");
        assert_eq!(format!("{secret}"), "<redacted>");
        assert_eq!(secret.expose_secret(), "super-secret-token");
    }
}
