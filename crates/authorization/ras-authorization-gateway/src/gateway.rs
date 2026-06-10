//! The gateway core: session validation, audience narrowing, and the
//! derived-token cache.
//!
//! The gateway is trusted auth infrastructure with one narrow power: it may
//! *narrow* a valid web session into a short-lived single-audience backend
//! token. It never invents permissions, never widens audiences, and never
//! forwards the original session token.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use chrono::{DateTime, Duration, Utc};
use ras_authorization_token::{
    AudiencePolicy, JwkSet, KeyResolver, KeyRing, RasClaims, SigningKey, TokenType, TokenValidator,
    ValidationOptions,
};

use crate::config::{GatewayConfig, RouteRule, RouteTable};
use crate::error::GatewayError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DerivedCacheKey {
    session_jti: String,
    subject: String,
    audience: String,
    authz_version: Option<u64>,
}

#[derive(Clone)]
struct CachedDerived {
    token: String,
    reuse_until: DateTime<Utc>,
}

/// A derived single-audience backend token.
#[derive(Clone)]
pub struct DerivedToken {
    /// The signed `ras_gateway_access` JWT. Bearer credential.
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for DerivedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedToken")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Validates web sessions and derives audience-narrowed backend tokens.
pub struct AuthGateway {
    session_validator: TokenValidator<std::sync::Arc<dyn KeyResolver>>,
    keys: RwLock<KeyRing>,
    table: RouteTable,
    cache: Mutex<HashMap<DerivedCacheKey, CachedDerived>>,
    derived_token_issuer: String,
    derived_token_ttl: Duration,
    cache_max_ttl: Duration,
    session_cookie: String,
}

impl AuthGateway {
    /// Build a gateway.
    ///
    /// - `session_keys` resolves the RAS authority's web-session signing
    ///   keys (typically a fetched JWKS or shared `KeyRing`).
    /// - `derived_signing_key` signs gateway-derived tokens. It is gateway
    ///   auth infrastructure: publish [`AuthGateway::jwks`] for backends and
    ///   rotate via [`AuthGateway::rotate_key`].
    pub fn new(
        config: GatewayConfig,
        session_keys: std::sync::Arc<dyn KeyResolver>,
        derived_signing_key: SigningKey,
    ) -> Result<Self, GatewayError> {
        let table = RouteTable::new(config.routes)?;
        let session_validator = TokenValidator::new(
            session_keys,
            ValidationOptions::new(
                config.session_issuer,
                AudiencePolicy::Absent,
                vec![TokenType::WebSession],
            ),
        );
        Ok(Self {
            session_validator,
            keys: RwLock::new(KeyRing::new(derived_signing_key)),
            table,
            cache: Mutex::new(HashMap::new()),
            derived_token_issuer: config.derived_token_issuer,
            derived_token_ttl: config.derived_token_ttl,
            cache_max_ttl: config.cache_max_ttl,
            session_cookie: config.session_cookie,
        })
    }

    /// The cookie name consulted for web sessions.
    pub fn session_cookie(&self) -> &str {
        &self.session_cookie
    }

    /// The route table.
    pub fn routes(&self) -> &RouteTable {
        &self.table
    }

    /// JWKS for the gateway's derived-token signing keys. Backends validate
    /// `ras_gateway_access` tokens against this.
    pub fn jwks(&self) -> JwkSet {
        self.keys.read().expect("gateway key lock poisoned").jwks()
    }

    /// Rotate the derived-token signing key.
    pub fn rotate_key(&self, new_active: SigningKey) {
        self.keys
            .write()
            .expect("gateway key lock poisoned")
            .rotate(new_active);
    }

    /// Validate an inbound web session token.
    pub fn validate_session(&self, token: &str) -> Result<RasClaims, GatewayError> {
        self.session_validator
            .validate(token)
            .map_err(GatewayError::InvalidSession)
    }

    /// Derive (or reuse from cache) a single-audience backend token for a
    /// validated session and matched route.
    ///
    /// The derived token contains exactly the route audience's permissions
    /// from the session — never more. Sessions without permissions for the
    /// audience fail closed unless the route is declared authenticated-only.
    pub fn derive_for_route(
        &self,
        session: &RasClaims,
        route: &RouteRule,
    ) -> Result<DerivedToken, GatewayError> {
        let permissions = match session.permissions_for_audience(&route.audience) {
            Some(permissions) if !permissions.is_empty() => permissions.to_vec(),
            _ if route.authenticated_only => Vec::new(),
            _ => {
                return Err(GatewayError::NoPermissionsForAudience {
                    audience: route.audience.clone(),
                });
            }
        };

        let now = Utc::now();
        let session_expires_at = session.expires_at().ok_or_else(|| {
            GatewayError::InvalidSession(ras_authorization_token::TokenError::InvalidClaims(
                "session expiry out of range".to_string(),
            ))
        })?;

        let key = DerivedCacheKey {
            session_jti: session.jti.clone(),
            subject: session.sub.clone(),
            audience: route.audience.clone(),
            authz_version: session.authz_version,
        };

        {
            let cache = self.cache.lock().expect("gateway cache lock poisoned");
            if let Some(cached) = cache.get(&key)
                && now < cached.reuse_until
            {
                return Ok(DerivedToken {
                    token: cached.token.clone(),
                    expires_at: cached.reuse_until,
                });
            }
        }

        // Derived expiry: bounded by both the configured TTL and the
        // session's own expiry — a derived token never outlives its session.
        let expires_at = std::cmp::min(now + self.derived_token_ttl, session_expires_at);
        let mut claims = RasClaims::gateway_access(
            self.derived_token_issuer.clone(),
            session.sub.clone(),
            route.audience.clone(),
            permissions,
            session.authz_version,
            self.derived_token_ttl,
        );
        claims.exp = expires_at.timestamp();

        let token = self
            .keys
            .read()
            .expect("gateway key lock poisoned")
            .sign(&claims)
            .map_err(GatewayError::Derivation)?;

        let reuse_until = std::cmp::min(expires_at, now + self.cache_max_ttl);
        let mut cache = self.cache.lock().expect("gateway cache lock poisoned");
        cache.retain(|_, cached| cached.reuse_until > now);
        cache.insert(
            key,
            CachedDerived {
                token: token.clone(),
                reuse_until,
            },
        );

        Ok(DerivedToken { token, expires_at })
    }
}
