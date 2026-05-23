use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::HashSet;

/// Simple mock auth provider that accepts "validtoken" for user and "admintoken" for admin
#[derive(Clone)]
pub struct SimpleAuthProvider;

impl AuthProvider for SimpleAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            match token.as_str() {
                "validtoken" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("user".to_string());

                    Ok(AuthenticatedUser {
                        user_id: "testuser".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                "admintoken" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("admin".to_string());
                    permissions.insert("user".to_string());

                    Ok(AuthenticatedUser {
                        user_id: "admin".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                _ => Err(AuthError::InvalidToken),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validtoken_authenticates_with_user_permission() {
        let provider = SimpleAuthProvider;

        let user = provider
            .authenticate("validtoken".to_string())
            .await
            .expect("valid user token");

        assert_eq!(user.user_id, "testuser");
        assert!(user.permissions.contains("user"));
        assert!(!user.permissions.contains("admin"));
    }

    #[tokio::test]
    async fn admintoken_authenticates_with_admin_and_user_permissions() {
        let provider = SimpleAuthProvider;

        let user = provider
            .authenticate("admintoken".to_string())
            .await
            .expect("valid admin token");

        assert_eq!(user.user_id, "admin");
        assert!(user.permissions.contains("user"));
        assert!(user.permissions.contains("admin"));
    }

    #[tokio::test]
    async fn unknown_token_is_rejected() {
        let provider = SimpleAuthProvider;

        let error = provider
            .authenticate("unknown".to_string())
            .await
            .expect_err("unknown token should be rejected");

        assert!(matches!(error, AuthError::InvalidToken));
    }
}
