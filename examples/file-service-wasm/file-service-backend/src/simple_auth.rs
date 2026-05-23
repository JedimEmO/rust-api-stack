use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::HashSet;

/// Simple mock auth provider that accepts "validtoken" as a bearer token.
#[derive(Clone)]
pub struct SimpleAuthProvider;

impl AuthProvider for SimpleAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token == "validtoken" {
                let mut permissions = HashSet::new();
                permissions.insert("user".to_string());

                Ok(AuthenticatedUser {
                    user_id: "testuser".to_string(),
                    permissions,
                    metadata: None,
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validtoken_authenticates_as_user() {
        let provider = SimpleAuthProvider;

        let user = provider
            .authenticate("validtoken".to_string())
            .await
            .expect("valid bearer token");

        assert_eq!(user.user_id, "testuser");
        assert!(user.permissions.contains("user"));
        assert!(!user.permissions.contains("admin"));
    }

    #[tokio::test]
    async fn unknown_token_is_rejected() {
        let provider = SimpleAuthProvider;

        let error = provider
            .authenticate("wrong-token".to_string())
            .await
            .expect_err("unknown token should be rejected");

        assert!(matches!(error, AuthError::InvalidToken));
    }
}
