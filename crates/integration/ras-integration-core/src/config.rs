//! Per-integration configuration: scope/audience bounds and outbound host
//! allowlisting.

use std::collections::BTreeSet;

use url::Url;

use crate::error::IntegrationError;

/// Declarative bounds for one integration. Token requests and outbound URLs
/// outside these bounds fail closed before any token source is consulted.
#[derive(Debug, Clone)]
pub struct IntegrationConfig {
    /// Stable integration id (e.g. `"google-calendar"`, `"invoice-service"`).
    pub integration_id: String,
    /// Scopes/permissions that may be requested through this integration.
    pub allowed_scopes: BTreeSet<String>,
    /// Audiences that may be requested (internal service integrations).
    pub allowed_audiences: BTreeSet<String>,
    /// Base URLs managed bearer tokens may be attached to.
    allowed_base_urls: Vec<Url>,
    /// Bumped whenever the configuration changes; part of every cache key so
    /// stale leases die with the config that produced them.
    pub config_version: u64,
}

impl IntegrationConfig {
    /// Create a configuration. `allowed_base_urls` must be absolute http(s)
    /// URLs; everything else is rejected.
    pub fn new(
        integration_id: impl Into<String>,
        allowed_scopes: impl IntoIterator<Item = impl Into<String>>,
        allowed_base_urls: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, IntegrationError> {
        let integration_id = integration_id.into();
        let mut parsed = Vec::new();
        for base in allowed_base_urls {
            let base = base.as_ref();
            let url = Url::parse(base).map_err(|err| {
                IntegrationError::InvalidConfig(format!("invalid allowed base url {base:?}: {err}"))
            })?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(IntegrationError::InvalidConfig(format!(
                    "allowed base url {base:?} must be http or https"
                )));
            }
            if url.host_str().is_none() {
                return Err(IntegrationError::InvalidConfig(format!(
                    "allowed base url {base:?} must have a host"
                )));
            }
            parsed.push(url);
        }
        Ok(Self {
            integration_id,
            allowed_scopes: allowed_scopes.into_iter().map(Into::into).collect(),
            allowed_audiences: BTreeSet::new(),
            allowed_base_urls: parsed,
            config_version: 1,
        })
    }

    /// Add allowed audiences (for internal service integrations).
    pub fn with_allowed_audiences(
        mut self,
        audiences: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_audiences = audiences.into_iter().map(Into::into).collect();
        self
    }

    /// Set the config version (bump on every configuration change).
    pub fn with_config_version(mut self, version: u64) -> Self {
        self.config_version = version;
        self
    }

    /// Whether a managed bearer token may be attached to `url`.
    ///
    /// The URL must parse, and must match an allowed base URL on scheme,
    /// exact host, port, and path prefix (segment-aligned, so
    /// `/api` does not authorize `/api-private`). Host comparison is exact:
    /// `api.example.com.evil.com` never matches `api.example.com`.
    pub fn allows_url(&self, url: &str) -> bool {
        let Ok(candidate) = Url::parse(url) else {
            return false;
        };
        self.allowed_base_urls.iter().any(|base| {
            if candidate.scheme() != base.scheme()
                || candidate.host_str() != base.host_str()
                || candidate.port_or_known_default() != base.port_or_known_default()
            {
                return false;
            }
            let base_path = base.path().trim_end_matches('/');
            if base_path.is_empty() {
                return true;
            }
            let candidate_path = candidate.path();
            candidate_path == base_path
                || candidate_path
                    .strip_prefix(base_path)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(bases: &[&str]) -> IntegrationConfig {
        IntegrationConfig::new("test", ["scope:a"], bases.iter().copied()).unwrap()
    }

    #[test]
    fn rejects_non_http_and_relative_bases() {
        assert!(IntegrationConfig::new("t", ["s"], ["ftp://example.com"]).is_err());
        assert!(IntegrationConfig::new("t", ["s"], ["not a url"]).is_err());
        assert!(IntegrationConfig::new("t", ["s"], ["unix:///tmp/sock"]).is_err());
    }

    #[test]
    fn exact_host_matching_defeats_suffix_tricks() {
        let config = config(&["https://api.example.com"]);
        assert!(config.allows_url("https://api.example.com/v1/items"));
        assert!(!config.allows_url("https://api.example.com.evil.com/v1/items"));
        assert!(!config.allows_url("https://evil-api.example.com/v1/items"));
        assert!(!config.allows_url("https://evil.com/api.example.com"));
    }

    #[test]
    fn scheme_and_port_must_match() {
        let config = config(&["https://api.example.com"]);
        assert!(!config.allows_url("http://api.example.com/v1"));
        assert!(!config.allows_url("https://api.example.com:8443/v1"));
        // Default port is equivalent to explicit 443.
        assert!(config.allows_url("https://api.example.com:443/v1"));
    }

    #[test]
    fn path_prefix_is_segment_aligned() {
        let config = config(&["https://api.example.com/api"]);
        assert!(config.allows_url("https://api.example.com/api"));
        assert!(config.allows_url("https://api.example.com/api/items"));
        assert!(!config.allows_url("https://api.example.com/api-private/items"));
        assert!(!config.allows_url("https://api.example.com/other"));
    }

    #[test]
    fn malformed_candidate_urls_fail_closed() {
        let config = config(&["https://api.example.com"]);
        assert!(!config.allows_url("not a url"));
        assert!(!config.allows_url(""));
    }

    #[test]
    fn internal_http_bases_are_allowed_when_configured() {
        let config = config(&["http://invoice-service:3000"]);
        assert!(config.allows_url("http://invoice-service:3000/api/invoices"));
        assert!(!config.allows_url("http://invoice-service:3001/api/invoices"));
    }
}
