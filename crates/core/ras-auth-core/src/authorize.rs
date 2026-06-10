//! Shared request-authorization pipeline for generated services.
//!
//! Every service macro (REST, file, JSON-RPC, bidirectional WebSocket) used
//! to inline its own copy of the credential → CSRF → authenticate →
//! permission-group sequence. These helpers are the single implementation;
//! generated code maps the returned [`AuthorizeError`] to its own protocol's
//! response shape.

use crate::{
    AuthError, AuthProvider, AuthTransportConfig, AuthenticatedUser, extract_auth_credential,
    validate_csrf_for_credential,
};
use http::HeaderMap;

/// Why [`authorize_request`] rejected a request.
#[derive(Debug)]
pub enum AuthorizeError {
    /// No usable credential was found in the request
    MissingCredential,
    /// Double-submit CSRF validation failed for a cookie credential
    CsrfValidationFailed,
    /// The credential did not authenticate
    AuthenticationFailed(AuthError),
    /// The service was built without an auth provider
    NoAuthProvider,
    /// Authenticated, but no required permission group was satisfied
    InsufficientPermissions(AuthError),
}

/// OR-of-AND permission check shared by all generated services.
///
/// `groups` is a disjunction of conjunctions: access is granted when the user
/// holds every permission of at least one group (verified through the
/// provider's `check_permissions`, which custom providers may override). A
/// group list with no non-empty groups — `WITH_PERMISSIONS([])` or any empty
/// inner group — grants access to any authenticated user.
pub fn check_permission_groups<P>(
    provider: &P,
    user: &AuthenticatedUser,
    groups: &[Vec<String>],
) -> Result<(), AuthError>
where
    P: AuthProvider + ?Sized,
{
    if !groups.iter().any(|group| !group.is_empty()) {
        return Ok(());
    }

    for group in groups {
        if group.is_empty() || provider.check_permissions(user, group).is_ok() {
            return Ok(());
        }
    }

    Err(AuthError::InsufficientPermissions {
        required: groups
            .iter()
            .find(|group| !group.is_empty())
            .cloned()
            .unwrap_or_default(),
        has: user.permissions.iter().cloned().collect(),
    })
}

/// Set-membership variant of [`check_permission_groups`] for contexts without
/// an auth provider (e.g. the bidirectional WebSocket handler, which
/// authorizes against the cached connection user).
pub fn user_satisfies_permission_groups(user: &AuthenticatedUser, groups: &[Vec<String>]) -> bool {
    if !groups.iter().any(|group| !group.is_empty()) {
        return true;
    }

    groups
        .iter()
        .any(|group| !group.is_empty() && group.iter().all(|perm| user.permissions.contains(perm)))
        || groups.iter().any(|group| group.is_empty())
}

/// The credential → CSRF → authenticate → permission pipeline shared by the
/// generated REST and file-service servers.
///
/// `method` is the HTTP method, used to scope CSRF validation to unsafe
/// requests. Errors are ordered so no work happens for unauthenticated
/// callers: the request body has not been touched when this returns `Err`.
pub async fn authorize_request<P>(
    method: &str,
    headers: &HeaderMap,
    auth_transport: &AuthTransportConfig,
    auth_provider: Option<&P>,
    required_permission_groups: &[Vec<String>],
) -> Result<AuthenticatedUser, AuthorizeError>
where
    P: AuthProvider + ?Sized,
{
    let credential = extract_auth_credential(headers, auth_transport)
        .map_err(|_| AuthorizeError::MissingCredential)?;

    validate_csrf_for_credential(method, headers, &credential, auth_transport)
        .map_err(|_| AuthorizeError::CsrfValidationFailed)?;

    let provider = auth_provider.ok_or(AuthorizeError::NoAuthProvider)?;

    let user = provider
        .authenticate(credential.token().to_string())
        .await
        .map_err(AuthorizeError::AuthenticationFailed)?;

    check_permission_groups(provider, &user, required_permission_groups)
        .map_err(AuthorizeError::InsufficientPermissions)?;

    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthFuture;
    use std::collections::HashSet;

    struct StaticProvider;

    impl AuthProvider for StaticProvider {
        fn authenticate(&self, token: String) -> AuthFuture<'_> {
            Box::pin(async move {
                if token == "good" {
                    Ok(user(&["read", "write"]))
                } else {
                    Err(AuthError::InvalidToken)
                }
            })
        }
    }

    fn user(perms: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "u".into(),
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            metadata: None,
        }
    }

    fn groups(groups: &[&[&str]]) -> Vec<Vec<String>> {
        groups
            .iter()
            .map(|g| g.iter().map(|p| p.to_string()).collect())
            .collect()
    }

    #[test]
    fn empty_group_list_is_authenticated_only() {
        assert!(check_permission_groups(&StaticProvider, &user(&[]), &[]).is_ok());
        assert!(user_satisfies_permission_groups(&user(&[]), &[]));
    }

    #[test]
    fn empty_inner_group_grants_any_authenticated_user() {
        let g = groups(&[&["admin"], &[]]);
        assert!(check_permission_groups(&StaticProvider, &user(&[]), &g).is_ok());
        assert!(user_satisfies_permission_groups(&user(&[]), &g));
    }

    #[test]
    fn and_within_group_or_between_groups() {
        let g = groups(&[&["read", "write"], &["admin"]]);

        // Satisfies the first group (all permissions present).
        assert!(check_permission_groups(&StaticProvider, &user(&["read", "write"]), &g).is_ok());
        assert!(user_satisfies_permission_groups(
            &user(&["read", "write"]),
            &g
        ));

        // Satisfies the second group.
        assert!(check_permission_groups(&StaticProvider, &user(&["admin"]), &g).is_ok());
        assert!(user_satisfies_permission_groups(&user(&["admin"]), &g));

        // Partial match on the first group, none on the second: denied.
        let denied = check_permission_groups(&StaticProvider, &user(&["read"]), &g).unwrap_err();
        assert!(matches!(
            denied,
            AuthError::InsufficientPermissions { required, .. } if required == vec!["read", "write"]
        ));
        assert!(!user_satisfies_permission_groups(&user(&["read"]), &g));
    }

    #[tokio::test]
    async fn authorize_request_full_pipeline() {
        let transport = AuthTransportConfig::default();
        let mut headers = HeaderMap::new();

        // No credential
        let err = authorize_request(
            "POST",
            &headers,
            &transport,
            Some(&StaticProvider),
            &groups(&[&["read"]]),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AuthorizeError::MissingCredential));

        headers.insert("authorization", "Bearer bad".parse().unwrap());
        let err = authorize_request(
            "POST",
            &headers,
            &transport,
            Some(&StaticProvider),
            &groups(&[&["read"]]),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AuthorizeError::AuthenticationFailed(_)));

        headers.insert("authorization", "Bearer good".parse().unwrap());

        // Missing provider
        let err = authorize_request(
            "POST",
            &headers,
            &transport,
            None::<&StaticProvider>,
            &groups(&[&["read"]]),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AuthorizeError::NoAuthProvider));

        // Insufficient permissions
        let err = authorize_request(
            "POST",
            &headers,
            &transport,
            Some(&StaticProvider),
            &groups(&[&["admin"]]),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AuthorizeError::InsufficientPermissions(_)));

        // Success
        let user = authorize_request(
            "POST",
            &headers,
            &transport,
            Some(&StaticProvider),
            &groups(&[&["read", "write"]]),
        )
        .await
        .unwrap();
        assert_eq!(user.user_id, "u");
    }
}
