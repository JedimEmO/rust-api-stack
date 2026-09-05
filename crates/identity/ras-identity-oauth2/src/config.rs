//! OAuth2 configuration types.

use crate::error::{OAuth2Error, OAuth2Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// OAuth2 provider configuration
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuth2ProviderConfig {
    pub provider_id: String,
    pub client_id: String,
    /// Never serialized (I1b): configs are frequently dumped for diagnostics.
    #[serde(skip_serializing)]
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
    /// Allow-list of additional userinfo claims copied into the verified
    /// identity's metadata (and therefore into the session JWT). Empty by
    /// default: only `picture` and `email_verified` are ever propagated (I8).
    #[serde(default)]
    pub metadata_claims: Vec<String>,
    /// Permit non-`https://` authorization/token/userinfo endpoints. Only for
    /// local development against a mock IdP; tokens and client secrets travel
    /// over these URLs (I9).
    #[serde(default)]
    pub allow_insecure_endpoints: bool,
}

impl OAuth2ProviderConfig {
    /// Validate the configuration before it is used to talk to a provider.
    ///
    /// Rejects endpoint URLs that are not `https://` unless
    /// `allow_insecure_endpoints` is set (I9).
    pub fn validate(&self) -> OAuth2Result<()> {
        if self.allow_insecure_endpoints {
            return Ok(());
        }
        let endpoints = [
            ("authorization_endpoint", Some(&self.authorization_endpoint)),
            ("token_endpoint", Some(&self.token_endpoint)),
            ("userinfo_endpoint", self.userinfo_endpoint.as_ref()),
        ];
        for (name, url) in endpoints {
            if let Some(url) = url
                && !is_https(url)
            {
                return Err(OAuth2Error::ConfigError(format!(
                    "provider '{}': {name} must use https:// (set allow_insecure_endpoints for local development)",
                    self.provider_id
                )));
            }
        }
        Ok(())
    }
}

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
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
            .field("metadata_claims", &self.metadata_claims)
            .field("allow_insecure_endpoints", &self.allow_insecure_endpoints)
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
            metadata_claims: Vec::new(),
            allow_insecure_endpoints: false,
        }
    }

    #[test]
    fn i9_validate_rejects_non_https_endpoints_unless_allowed() {
        let mut p = provider();
        assert!(p.validate().is_ok());

        p.authorization_endpoint = "http://x/auth".into();
        assert!(
            matches!(p.validate(), Err(OAuth2Error::ConfigError(msg)) if msg.contains("authorization_endpoint"))
        );
        p.allow_insecure_endpoints = true;
        assert!(p.validate().is_ok());

        let mut p = provider();
        p.token_endpoint = "HTTP://x/token".into();
        assert!(
            matches!(p.validate(), Err(OAuth2Error::ConfigError(msg)) if msg.contains("token_endpoint"))
        );

        let mut p = provider();
        p.userinfo_endpoint = Some("ftp://x/info".into());
        assert!(
            matches!(p.validate(), Err(OAuth2Error::ConfigError(msg)) if msg.contains("userinfo_endpoint"))
        );

        // No userinfo endpoint is fine; case-insensitive scheme is fine.
        let mut p = provider();
        p.userinfo_endpoint = None;
        p.token_endpoint = "HTTPS://x/token".into();
        assert!(p.validate().is_ok());
    }

    #[test]
    fn i9_and_i8_new_fields_default_when_absent_from_json() {
        let json = serde_json::json!({
            "provider_id": "google",
            "client_id": "cid",
            "client_secret": "secret",
            "authorization_endpoint": "https://x/auth",
            "token_endpoint": "https://x/token",
            "userinfo_endpoint": null,
            "redirect_uri": "https://app/cb",
            "scopes": [],
            "auth_params": {},
            "use_pkce": true,
            "user_info_mapping": null
        });
        let parsed: OAuth2ProviderConfig = serde_json::from_value(json).unwrap();
        assert!(parsed.metadata_claims.is_empty());
        assert!(!parsed.allow_insecure_endpoints);
        assert_eq!(parsed.client_secret, "secret");
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
    fn i1b_provider_config_serialize_omits_client_secret() {
        let mut p = provider();
        p.client_secret = "super-secret-value".into();
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("client_secret").is_none());
        assert!(!json.to_string().contains("super-secret-value"));
        assert_eq!(json["provider_id"], "google");
        assert_eq!(json["client_id"], "cid");

        // The secret-less document is not accepted back: the secret is required on deserialize.
        assert!(serde_json::from_value::<OAuth2ProviderConfig>(json).is_err());
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
