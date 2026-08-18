//! OAuth2 configuration types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// OAuth2 provider configuration
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuth2ProviderConfig {
    pub provider_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    /// Expected `iss` claim of id_tokens returned by this provider
    /// (e.g. `https://accounts.google.com`). When set, callbacks carrying
    /// an id_token with a different issuer are rejected.
    #[serde(default)]
    pub issuer: Option<String>,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    /// Additional parameters to include in authorization request
    pub auth_params: HashMap<String, String>,
    /// Whether to use PKCE (recommended for public clients)
    pub use_pkce: bool,
    /// Custom user info mapping
    pub user_info_mapping: Option<UserInfoMapping>,
}

/// Manual `Debug` that redacts `client_secret` so it never lands in logs (L1).
impl fmt::Debug for OAuth2ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2ProviderConfig")
            .field("provider_id", &self.provider_id)
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .field("authorization_endpoint", &self.authorization_endpoint)
            .field("token_endpoint", &self.token_endpoint)
            .field("userinfo_endpoint", &self.userinfo_endpoint)
            .field("issuer", &self.issuer)
            .field("redirect_uri", &self.redirect_uri)
            .field("scopes", &self.scopes)
            .field("auth_params", &self.auth_params)
            .field("use_pkce", &self.use_pkce)
            .field("user_info_mapping", &self.user_info_mapping)
            .finish()
    }
}

/// Mapping configuration for user info fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfoMapping {
    pub subject_field: Option<String>,
    pub email_field: Option<String>,
    pub name_field: Option<String>,
    pub picture_field: Option<String>,
}

impl Default for UserInfoMapping {
    fn default() -> Self {
        Self {
            subject_field: Some("sub".to_string()),
            email_field: Some("email".to_string()),
            name_field: Some("name".to_string()),
            picture_field: Some("picture".to_string()),
        }
    }
}

/// OAuth2 client configuration
#[derive(Debug, Clone)]
pub struct OAuth2Config {
    pub providers: HashMap<String, OAuth2ProviderConfig>,
    pub state_ttl_seconds: u64,
    pub http_timeout_seconds: u64,
}

impl Default for OAuth2Config {
    fn default() -> Self {
        Self {
            providers: HashMap::new(),
            state_ttl_seconds: 600, // 10 minutes
            http_timeout_seconds: 30,
        }
    }
}

impl OAuth2Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_provider(mut self, config: OAuth2ProviderConfig) -> Self {
        self.providers.insert(config.provider_id.clone(), config);
        self
    }

    pub fn with_state_ttl(mut self, seconds: u64) -> Self {
        self.state_ttl_seconds = seconds;
        self
    }

    pub fn with_http_timeout(mut self, seconds: u64) -> Self {
        self.http_timeout_seconds = seconds;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> OAuth2ProviderConfig {
        OAuth2ProviderConfig {
            provider_id: "google".into(),
            client_id: "cid".into(),
            client_secret: "secret".into(),
            authorization_endpoint: "https://x/auth".into(),
            token_endpoint: "https://x/token".into(),
            userinfo_endpoint: Some("https://x/info".into()),
            issuer: None,
            redirect_uri: "https://app/cb".into(),
            scopes: vec!["openid".into(), "email".into()],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
        }
    }

    #[test]
    fn user_info_mapping_default_uses_oidc_field_names() {
        let m = UserInfoMapping::default();
        assert_eq!(m.subject_field.as_deref(), Some("sub"));
        assert_eq!(m.email_field.as_deref(), Some("email"));
        assert_eq!(m.name_field.as_deref(), Some("name"));
        assert_eq!(m.picture_field.as_deref(), Some("picture"));
    }

    #[test]
    fn oauth2_config_builder_chains_settings() {
        let p = provider();
        let cfg = OAuth2Config::new()
            .add_provider(p.clone())
            .with_state_ttl(120)
            .with_http_timeout(7);
        assert_eq!(cfg.state_ttl_seconds, 120);
        assert_eq!(cfg.http_timeout_seconds, 7);
        assert!(cfg.providers.contains_key("google"));
    }

    #[test]
    fn provider_config_round_trips_through_serde() {
        let p = provider();
        let json = serde_json::to_string(&p).unwrap();
        let parsed: OAuth2ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider_id, p.provider_id);
        assert_eq!(parsed.client_id, p.client_id);
        assert_eq!(parsed.scopes, p.scopes);
        assert!(parsed.use_pkce);
    }

    #[test]
    fn user_info_mapping_serde() {
        let m = UserInfoMapping::default();
        let json = serde_json::to_string(&m).unwrap();
        let parsed: UserInfoMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.subject_field, m.subject_field);
    }

    #[test]
    fn debug_redacts_client_secret() {
        let mut p = provider();
        p.client_secret = "super-secret-value".into();
        let debug = format!("{p:?}");
        assert!(!debug.contains("super-secret-value"));
        assert!(debug.contains("[REDACTED]"));
        // Non-secret fields are still visible.
        assert!(debug.contains("google"));
    }

    #[test]
    fn defaults_are_sensible() {
        let cfg = OAuth2Config::default();
        assert!(cfg.providers.is_empty());
        assert_eq!(cfg.state_ttl_seconds, 600);
        assert_eq!(cfg.http_timeout_seconds, 30);
    }
}
