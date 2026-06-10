//! Gateway configuration: route rules, the compiled route table, and
//! consumption of generated topology gateway profiles.

use std::collections::BTreeMap;

use chrono::Duration;
use serde::Deserialize;

use crate::error::GatewayError;

/// One route: a path prefix mapped to a backend audience and upstream.
#[derive(Debug, Clone)]
pub struct RouteRule {
    /// Path prefix. Matching is segment-aligned and longest-prefix-wins;
    /// `/api` matches `/api` and `/api/x` but never `/api-private`.
    pub prefix: String,
    /// The backend audience derived tokens are narrowed to.
    pub audience: String,
    /// Upstream base URL (deployment-specific binding).
    pub upstream: String,
    /// Allow requests whose session has *no* permissions for this audience.
    /// Defaults to false: missing target-audience permissions fail closed.
    pub authenticated_only: bool,
}

impl RouteRule {
    pub fn new(
        prefix: impl Into<String>,
        audience: impl Into<String>,
        upstream: impl Into<String>,
    ) -> Self {
        Self {
            prefix: prefix.into(),
            audience: audience.into(),
            upstream: upstream.into(),
            authenticated_only: false,
        }
    }

    /// Mark this route as authenticated-only (empty permission set allowed).
    pub fn authenticated_only(mut self) -> Self {
        self.authenticated_only = true;
        self
    }
}

/// Top-level gateway configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Expected issuer of inbound web sessions.
    pub session_issuer: String,
    /// Issuer stamped into derived backend tokens (the gateway acts as
    /// delegated RAS auth infrastructure under this name).
    pub derived_token_issuer: String,
    /// Route rules. Validated into a [`RouteTable`].
    pub routes: Vec<RouteRule>,
    /// Cookie consulted for the web session when no `Authorization` header
    /// is present.
    pub session_cookie: String,
    /// Lifetime of derived backend tokens (default 2 minutes; always also
    /// bounded by the session expiry).
    pub derived_token_ttl: Duration,
    /// Upper bound on derived-token cache reuse (default 60 seconds).
    pub cache_max_ttl: Duration,
}

impl GatewayConfig {
    pub fn new(
        session_issuer: impl Into<String>,
        derived_token_issuer: impl Into<String>,
        routes: Vec<RouteRule>,
    ) -> Self {
        Self {
            session_issuer: session_issuer.into(),
            derived_token_issuer: derived_token_issuer.into(),
            routes,
            session_cookie: "ras_session".to_string(),
            derived_token_ttl: Duration::minutes(2),
            cache_max_ttl: Duration::seconds(60),
        }
    }

    /// Build a config from a generated topology gateway profile (TOML) plus
    /// deployment-provided upstream bindings (audience → base URL).
    ///
    /// Startup-validates that every profile route resolves to an upstream;
    /// missing bindings fail closed with the full missing list.
    pub fn from_profile_toml(
        session_issuer: impl Into<String>,
        derived_token_issuer: impl Into<String>,
        profile_toml: &str,
        upstreams: &BTreeMap<String, String>,
    ) -> Result<Self, GatewayError> {
        let profile: GatewayProfile = toml::from_str(profile_toml).map_err(|err| {
            GatewayError::InvalidConfig(format!("invalid gateway profile: {err}"))
        })?;

        let missing: Vec<String> = profile
            .routes
            .values()
            .filter(|route| !upstreams.contains_key(&route.audience))
            .map(|route| route.audience.clone())
            .collect();
        if !missing.is_empty() {
            return Err(GatewayError::InvalidConfig(format!(
                "no upstream binding for audiences {missing:?}"
            )));
        }

        let routes = profile
            .routes
            .into_iter()
            .map(|(prefix, route)| RouteRule {
                upstream: upstreams[&route.audience].clone(),
                prefix,
                audience: route.audience,
                authenticated_only: route.authenticated_only,
            })
            .collect();
        Ok(Self::new(session_issuer, derived_token_issuer, routes))
    }
}

/// A generated gateway profile artifact (the topology crate emits this).
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayProfile {
    pub schema_version: u32,
    pub topology: String,
    pub profile: String,
    pub profile_id: String,
    /// Route prefix → audience mapping. Upstream bindings are deliberately
    /// absent: they are deployment-specific.
    pub routes: BTreeMap<String, ProfileRoute>,
}

/// One route entry in a generated profile.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileRoute {
    pub audience: String,
    #[serde(default)]
    pub authenticated_only: bool,
}

/// Compiled, validated route table with deterministic longest-prefix
/// matching.
#[derive(Debug)]
pub struct RouteTable {
    /// Sorted by prefix length descending, so the first match wins.
    routes: Vec<RouteRule>,
}

impl RouteTable {
    /// Validate and compile route rules:
    ///
    /// - prefixes must start with `/`; trailing slashes are normalized away
    ///   (except the root route `/`)
    /// - duplicate prefixes within one profile fail validation
    pub fn new(rules: Vec<RouteRule>) -> Result<Self, GatewayError> {
        if rules.is_empty() {
            return Err(GatewayError::InvalidConfig(
                "gateway requires at least one route".to_string(),
            ));
        }
        let mut routes = Vec::with_capacity(rules.len());
        for mut rule in rules {
            if !rule.prefix.starts_with('/') {
                return Err(GatewayError::InvalidConfig(format!(
                    "route prefix {:?} must start with '/'",
                    rule.prefix
                )));
            }
            if rule.prefix != "/" {
                rule.prefix = rule.prefix.trim_end_matches('/').to_string();
            }
            if rule.audience.is_empty() || rule.upstream.is_empty() {
                return Err(GatewayError::InvalidConfig(format!(
                    "route {:?} requires a non-empty audience and upstream",
                    rule.prefix
                )));
            }
            routes.push(rule);
        }
        for (index, rule) in routes.iter().enumerate() {
            if routes[..index]
                .iter()
                .any(|other| other.prefix == rule.prefix)
            {
                return Err(GatewayError::InvalidConfig(format!(
                    "conflicting routes for prefix {:?}",
                    rule.prefix
                )));
            }
        }
        routes.sort_by_key(|rule| std::cmp::Reverse(rule.prefix.len()));
        Ok(Self { routes })
    }

    /// Match a request path: longest prefix wins; matches are
    /// segment-aligned. Unmatched paths return `None` (fail closed).
    pub fn match_path(&self, path: &str) -> Option<&RouteRule> {
        self.routes.iter().find(|rule| {
            if rule.prefix == "/" {
                return true;
            }
            path == rule.prefix
                || path
                    .strip_prefix(rule.prefix.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(prefix: &str, audience: &str) -> RouteRule {
        RouteRule::new(prefix, audience, format!("http://{audience}:3000"))
    }

    #[test]
    fn longest_prefix_wins_and_matching_is_segment_aligned() {
        let table = RouteTable::new(vec![
            rule("/invoices", "invoice-service"),
            rule("/invoices/admin", "admin-service"),
            rule("/billing", "billing-service"),
        ])
        .unwrap();

        assert_eq!(
            table.match_path("/invoices/123").unwrap().audience,
            "invoice-service"
        );
        assert_eq!(
            table.match_path("/invoices/admin/users").unwrap().audience,
            "admin-service"
        );
        assert_eq!(
            table.match_path("/invoices").unwrap().audience,
            "invoice-service"
        );
        // Segment alignment: /invoices-extra does not match /invoices.
        assert!(table.match_path("/invoices-extra/1").is_none());
        // Unmatched paths fail closed.
        assert!(table.match_path("/unknown").is_none());
    }

    #[test]
    fn duplicate_prefixes_fail_validation() {
        let err = RouteTable::new(vec![
            rule("/invoices", "invoice-service"),
            rule("/invoices/", "billing-service"),
        ])
        .unwrap_err();
        assert!(matches!(err, GatewayError::InvalidConfig(_)));
    }

    #[test]
    fn invalid_prefixes_and_empty_tables_are_rejected() {
        assert!(matches!(
            RouteTable::new(vec![rule("invoices", "invoice-service")]).unwrap_err(),
            GatewayError::InvalidConfig(_)
        ));
        assert!(matches!(
            RouteTable::new(vec![]).unwrap_err(),
            GatewayError::InvalidConfig(_)
        ));
    }

    #[test]
    fn root_route_matches_everything() {
        let table = RouteTable::new(vec![rule("/", "app"), rule("/api", "api-service")]).unwrap();
        assert_eq!(table.match_path("/api/x").unwrap().audience, "api-service");
        assert_eq!(table.match_path("/anything").unwrap().audience, "app");
    }

    #[test]
    fn profile_loading_binds_upstreams_and_fails_closed_on_missing() {
        let profile = r#"
            schema_version = 1
            topology = "internal-tools"
            profile = "public_web"
            profile_id = "internal-tools/public_web@1"

            [routes."/invoices"]
            audience = "invoice-service"

            [routes."/billing"]
            audience = "billing-service"
            authenticated_only = true
        "#;

        let mut upstreams = BTreeMap::new();
        upstreams.insert(
            "invoice-service".to_string(),
            "http://invoice:3000".to_string(),
        );
        let err = GatewayConfig::from_profile_toml("iss", "gw", profile, &upstreams).unwrap_err();
        assert!(
            matches!(err, GatewayError::InvalidConfig(message) if message.contains("billing-service"))
        );

        upstreams.insert(
            "billing-service".to_string(),
            "http://billing:3000".to_string(),
        );
        let config = GatewayConfig::from_profile_toml("iss", "gw", profile, &upstreams).unwrap();
        assert_eq!(config.routes.len(), 2);
        let billing = config
            .routes
            .iter()
            .find(|route| route.audience == "billing-service")
            .unwrap();
        assert!(billing.authenticated_only);
        assert_eq!(billing.upstream, "http://billing:3000");
    }
}
