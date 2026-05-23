use async_trait::async_trait;
use ras_identity_core::{IdentityError, UserPermissions, VerifiedIdentity};
use tracing::info;

/// Custom permissions provider for Google OAuth2 users
///
/// This provider demonstrates how to assign permissions based on user attributes
/// from the OAuth2 identity. In a real application, you would typically:
/// - Query a database to get user roles and permissions
/// - Check with an external authorization service
/// - Apply business logic based on user attributes
pub struct GoogleOAuth2Permissions {
    // In a real implementation, this might contain:
    // - Database connection pool
    // - External service clients
    // - Configuration for role mappings
}

impl GoogleOAuth2Permissions {
    pub fn new() -> Self {
        Self {}
    }

    /// Determine permissions based on user's email domain and other attributes
    fn calculate_permissions(&self, identity: &VerifiedIdentity) -> Vec<String> {
        let mut permissions = Vec::new();

        // Base permissions for all authenticated users
        permissions.push("user:read".to_string());
        permissions.push("profile:read".to_string());

        // Check email domain for additional permissions
        if let Some(email) = &identity.email {
            if email.ends_with("@example.com") {
                // Users from example.com get admin permissions
                permissions.push("admin:read".to_string());
                permissions.push("admin:write".to_string());
                permissions.push("user:write".to_string());
                permissions.push("system:manage".to_string());
                info!("Granted admin permissions to user with email: {}", email);
            } else if email.ends_with("@trusted-domain.com") {
                // Users from trusted-domain.com get elevated permissions
                permissions.push("user:write".to_string());
                permissions.push("content:create".to_string());
                permissions.push("content:edit".to_string());
                info!("Granted elevated permissions to user with email: {}", email);
            }
        }

        // Check metadata for additional context
        if let Some(metadata) = &identity.metadata {
            // If user has verified email, grant additional permissions
            if let Some(email_verified) = metadata.get("email_verified")
                && email_verified.as_bool().unwrap_or(false)
            {
                permissions.push("email:verified".to_string());
            }

            // Example: Grant permissions based on other OAuth2 claims
            if let Some(locale) = metadata.get("locale")
                && let Some(locale_str) = locale.as_str()
                && locale_str.starts_with("en")
            {
                permissions.push("content:english".to_string());
            }
        }

        // Check subject for special users (in a real app, this might be a database lookup)
        match identity.subject.as_str() {
            // Special system administrator
            "104872499792737890123" => {
                permissions.push("system:admin".to_string());
                permissions.push("debug:access".to_string());
                info!("Granted system admin permissions to special user");
            }
            // Beta tester
            subject if subject.starts_with("beta_") => {
                permissions.push("beta:access".to_string());
                permissions.push("feature:preview".to_string());
            }
            _ => {}
        }

        // Remove duplicates
        permissions.sort();
        permissions.dedup();

        permissions
    }
}

#[async_trait]
impl UserPermissions for GoogleOAuth2Permissions {
    async fn get_permissions(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<Vec<String>, IdentityError> {
        info!(
            "Calculating permissions for user: {} (provider: {})",
            identity.subject, identity.provider_id
        );

        let permissions = self.calculate_permissions(identity);

        info!(
            "Assigned {} permissions to user {}: {:?}",
            permissions.len(),
            identity.subject,
            permissions
        );

        Ok(permissions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_identity(
        subject: &str,
        email: Option<&str>,
        email_verified: Option<bool>,
    ) -> VerifiedIdentity {
        let mut metadata = serde_json::Map::new();
        if let Some(verified) = email_verified {
            metadata.insert(
                "email_verified".to_string(),
                serde_json::Value::Bool(verified),
            );
        }

        VerifiedIdentity {
            provider_id: "oauth2:google".to_string(),
            subject: subject.to_string(),
            email: email.map(|e| e.to_string()),
            display_name: Some("Test User".to_string()),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(metadata))
            },
        }
    }

    #[tokio::test]
    async fn test_basic_user_permissions() {
        let provider = GoogleOAuth2Permissions::new();
        let identity = create_test_identity("12345", Some("user@gmail.com"), Some(true));

        let permissions = provider.get_permissions(&identity).await.unwrap();

        assert!(permissions.contains(&"user:read".to_string()));
        assert!(permissions.contains(&"profile:read".to_string()));
        assert!(permissions.contains(&"email:verified".to_string()));
        assert!(!permissions.contains(&"admin:read".to_string()));
    }

    #[tokio::test]
    async fn test_admin_user_permissions() {
        let provider = GoogleOAuth2Permissions::new();
        let identity = create_test_identity("67890", Some("admin@example.com"), Some(true));

        let permissions = provider.get_permissions(&identity).await.unwrap();

        assert!(permissions.contains(&"user:read".to_string()));
        assert!(permissions.contains(&"admin:read".to_string()));
        assert!(permissions.contains(&"admin:write".to_string()));
        assert!(permissions.contains(&"system:manage".to_string()));
    }

    #[tokio::test]
    async fn test_trusted_domain_permissions() {
        let provider = GoogleOAuth2Permissions::new();
        let identity =
            create_test_identity("11111", Some("developer@trusted-domain.com"), Some(true));

        let permissions = provider.get_permissions(&identity).await.unwrap();

        assert!(permissions.contains(&"user:write".to_string()));
        assert!(permissions.contains(&"content:create".to_string()));
        assert!(permissions.contains(&"content:edit".to_string()));
        assert!(!permissions.contains(&"admin:read".to_string()));
    }

    #[tokio::test]
    async fn test_special_system_admin() {
        let provider = GoogleOAuth2Permissions::new();
        let identity = create_test_identity(
            "104872499792737890123",
            Some("sysadmin@example.com"),
            Some(true),
        );

        let permissions = provider.get_permissions(&identity).await.unwrap();

        assert!(permissions.contains(&"system:admin".to_string()));
        assert!(permissions.contains(&"debug:access".to_string()));
        assert!(permissions.contains(&"admin:read".to_string()));
    }

    #[tokio::test]
    async fn test_beta_user_permissions() {
        let provider = GoogleOAuth2Permissions::new();
        let identity = create_test_identity("beta_12345", Some("beta@test.com"), Some(true));

        let permissions = provider.get_permissions(&identity).await.unwrap();

        assert!(permissions.contains(&"beta:access".to_string()));
        assert!(permissions.contains(&"feature:preview".to_string()));
    }

    #[tokio::test]
    async fn test_unverified_email() {
        let provider = GoogleOAuth2Permissions::new();
        let identity = create_test_identity("98765", Some("user@gmail.com"), Some(false));

        let permissions = provider.get_permissions(&identity).await.unwrap();

        assert!(permissions.contains(&"user:read".to_string()));
        assert!(!permissions.contains(&"email:verified".to_string()));
    }
}
