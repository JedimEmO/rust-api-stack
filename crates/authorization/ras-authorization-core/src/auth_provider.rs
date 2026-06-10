//! Downstream validation: accept RAS-issued tokens in existing RAS services.
//!
//! [`RasTokenAuthProvider`] implements `ras-auth-core`'s [`AuthProvider`],
//! so generated REST/JSON-RPC/file services accept internal service tokens
//! (and gateway-derived tokens) with their existing permission enforcement —
//! no macro changes required.

use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_authorization_token::{KeyResolver, RasClaims, TokenError, TokenValidator};

/// An [`AuthProvider`] validating RAS tokens (internal service tokens,
/// gateway-derived tokens) against a [`TokenValidator`].
///
/// The validator's options pin the expected issuer, audience, token types,
/// and algorithm allowlist; the resolver is typically the authority's JWKS.
/// Validated claims map to an [`AuthenticatedUser`] whose permissions are
/// the token's single-audience permission set, so generated services enforce
/// their `WITH_PERMISSIONS` requirements unchanged.
pub struct RasTokenAuthProvider<R> {
    validator: TokenValidator<R>,
}

impl<R: KeyResolver> RasTokenAuthProvider<R> {
    pub fn new(validator: TokenValidator<R>) -> Self {
        Self { validator }
    }

    fn to_user(claims: RasClaims) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: claims.sub.clone(),
            permissions: claims.permissions.iter().cloned().collect(),
            metadata: claims.metadata,
        }
    }
}

impl<R: KeyResolver + 'static> AuthProvider for RasTokenAuthProvider<R> {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let claims = self.validator.validate(&token).map_err(|err| match err {
                TokenError::Expired => AuthError::TokenExpired,
                _ => AuthError::InvalidToken,
            })?;
            Ok(Self::to_user(claims))
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use ras_authorization_token::{
        AudiencePolicy, KeyRing, PrincipalKind, SigningKey, TokenType, ValidationOptions,
    };

    use super::*;

    #[tokio::test]
    async fn internal_token_maps_to_authenticated_user_with_permissions() {
        let ring = KeyRing::new(SigningKey::generate_es256("k1"));
        let claims = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec!["invoice:read".to_string()],
            Duration::minutes(5),
        );
        let token = ring.sign(&claims).unwrap();

        let provider = RasTokenAuthProvider::new(TokenValidator::new(
            ring.jwks(),
            ValidationOptions::new(
                "https://auth.internal",
                AudiencePolicy::Exact("invoice-service".to_string()),
                vec![TokenType::InternalService],
            ),
        ));

        let user = provider.authenticate(token).await.unwrap();
        assert_eq!(user.user_id, "billing-service");
        assert!(user.permissions.contains("invoice:read"));
    }

    #[tokio::test]
    async fn wrong_audience_and_garbage_tokens_are_invalid() {
        let ring = KeyRing::new(SigningKey::generate_es256("k1"));
        let claims = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "other-service",
            vec![],
            Duration::minutes(5),
        );
        let token = ring.sign(&claims).unwrap();

        let provider = RasTokenAuthProvider::new(TokenValidator::new(
            ring.jwks(),
            ValidationOptions::new(
                "https://auth.internal",
                AudiencePolicy::Exact("invoice-service".to_string()),
                vec![TokenType::InternalService],
            ),
        ));

        assert!(matches!(
            provider.authenticate(token).await.unwrap_err(),
            AuthError::InvalidToken
        ));
        assert!(matches!(
            provider
                .authenticate("garbage".to_string())
                .await
                .unwrap_err(),
            AuthError::InvalidToken
        ));
    }

    #[tokio::test]
    async fn expired_token_maps_to_token_expired() {
        let ring = KeyRing::new(SigningKey::generate_es256("k1"));
        let mut claims = RasClaims::internal_service(
            "https://auth.internal",
            "billing-service",
            PrincipalKind::Service,
            "invoice-service",
            vec![],
            Duration::minutes(5),
        );
        claims.iat -= 7200;
        claims.exp = claims.iat + 60;
        let token = ring.sign(&claims).unwrap();

        let provider = RasTokenAuthProvider::new(TokenValidator::new(
            ring.jwks(),
            ValidationOptions::new(
                "https://auth.internal",
                AudiencePolicy::Exact("invoice-service".to_string()),
                vec![TokenType::InternalService],
            ),
        ));

        assert!(matches!(
            provider.authenticate(token).await.unwrap_err(),
            AuthError::TokenExpired
        ));
    }
}
