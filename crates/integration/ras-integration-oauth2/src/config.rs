//! OAuth2 provider configuration.

use ras_integration_core::{IntegrationError, SecretString};
use url::Url;

/// Configuration for one external OAuth2/OIDC provider client.
///
/// `Debug` redacts the client secret.
#[derive(Clone)]
pub struct OAuth2ProviderConfig {
    /// The provider's token endpoint (must be https unless
    /// `danger_allow_insecure_http` is set, e.g. for in-process fakes).
    pub token_endpoint: String,
    /// The provider's authorization endpoint; required for consent flows.
    pub authorization_endpoint: Option<String>,
    /// OAuth client id registered with the provider.
    pub client_id: String,
    /// OAuth client secret; required for client-credentials, optional for
    /// public PKCE clients.
    pub client_secret: Option<SecretString>,
    /// Allow plain-http endpoints. For tests and in-process fakes only.
    pub danger_allow_insecure_http: bool,
}

impl std::fmt::Debug for OAuth2ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2ProviderConfig")
            .field("token_endpoint", &self.token_endpoint)
            .field("authorization_endpoint", &self.authorization_endpoint)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "danger_allow_insecure_http",
                &self.danger_allow_insecure_http,
            )
            .finish()
    }
}

impl OAuth2ProviderConfig {
    pub fn new(
        token_endpoint: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        let config = Self {
            token_endpoint: token_endpoint.into(),
            authorization_endpoint: None,
            client_id: client_id.into(),
            client_secret: None,
            danger_allow_insecure_http: false,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn with_authorization_endpoint(
        mut self,
        endpoint: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        self.authorization_endpoint = Some(endpoint.into());
        self.validate()?;
        Ok(self)
    }

    pub fn with_client_secret(mut self, secret: impl Into<SecretString>) -> Self {
        self.client_secret = Some(secret.into());
        self
    }

    /// Permit plain-http endpoints (tests/in-process fakes only).
    pub fn with_danger_allow_insecure_http(mut self) -> Self {
        self.danger_allow_insecure_http = true;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), IntegrationError> {
        for (name, endpoint) in [
            ("token_endpoint", Some(&self.token_endpoint)),
            (
                "authorization_endpoint",
                self.authorization_endpoint.as_ref(),
            ),
        ] {
            let Some(endpoint) = endpoint else { continue };
            let url = Url::parse(endpoint).map_err(|err| {
                IntegrationError::InvalidConfig(format!("invalid {name} {endpoint:?}: {err}"))
            })?;
            match url.scheme() {
                "https" => {}
                "http" if self.danger_allow_insecure_http => {}
                scheme => {
                    return Err(IntegrationError::InvalidConfig(format!(
                        "{name} {endpoint:?} uses scheme {scheme:?}; only https is allowed \
                         (or http with danger_allow_insecure_http)"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_endpoints_are_accepted() {
        let config = OAuth2ProviderConfig::new("https://provider.test/token", "client").unwrap();
        assert_eq!(config.client_id, "client");
    }

    #[test]
    fn http_requires_explicit_danger_flag() {
        assert!(OAuth2ProviderConfig::new("http://provider.test/token", "client").is_err());

        let config = OAuth2ProviderConfig {
            token_endpoint: "http://provider.test/token".to_string(),
            authorization_endpoint: None,
            client_id: "client".to_string(),
            client_secret: None,
            danger_allow_insecure_http: true,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn debug_redacts_client_secret() {
        let config = OAuth2ProviderConfig::new("https://provider.test/token", "client")
            .unwrap()
            .with_client_secret("super-secret");
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret"));
    }
}
