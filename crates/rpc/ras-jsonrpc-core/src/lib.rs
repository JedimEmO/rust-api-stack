//! Core authentication and authorization traits for JSON-RPC services.
//!
//! This crate provides the authentication and authorization traits used by the
//! `ras-jsonrpc-macro` procedural macro to generate type-safe JSON-RPC services
//! with axum integration.

// Re-export authentication types from ras-auth-core
pub use ras_auth_core::*;

// Re-export JSON-RPC types for convenience
pub use ras_jsonrpc_types::*;

// Re-export version migration traits for generated compatibility dispatch.
pub use ras_version_core::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    struct TestAuthProvider;

    impl AuthProvider for TestAuthProvider {
        fn authenticate(&self, _token: String) -> AuthFuture<'_> {
            Box::pin(async { Err(AuthError::InvalidToken) })
        }
    }

    #[derive(Debug, PartialEq)]
    struct RenameV1 {
        name: String,
    }

    #[derive(Debug, PartialEq)]
    struct RenameV2 {
        display_name: String,
        notify: bool,
    }

    #[derive(Debug, PartialEq)]
    struct RenameResponseV1 {
        name: String,
    }

    #[derive(Debug, PartialEq)]
    struct RenameResponseV2 {
        display_name: String,
        notified: bool,
    }

    struct RenameCompat;

    impl VersionMigration<RenameV1, RenameV2> for RenameCompat {
        type Error = std::convert::Infallible;

        fn migrate(value: RenameV1) -> Result<RenameV2, Self::Error> {
            Ok(RenameV2 {
                display_name: value.name,
                notify: false,
            })
        }
    }

    impl VersionMigration<RenameResponseV2, RenameResponseV1> for RenameCompat {
        type Error = std::convert::Infallible;

        fn migrate(value: RenameResponseV2) -> Result<RenameResponseV1, Self::Error> {
            Ok(RenameResponseV1 {
                name: value.display_name,
            })
        }
    }

    #[test]
    fn reexported_auth_types_support_permission_checks() {
        let provider = TestAuthProvider;
        let user = AuthenticatedUser {
            user_id: "user-1".to_string(),
            permissions: HashSet::from(["widgets:read".to_string()]),
            metadata: Some(json!({ "tenant": "demo" })),
        };

        let allowed = provider.check_permissions(&user, &["widgets:read".to_string()]);
        assert!(allowed.is_ok());

        let denied = provider
            .check_permissions(&user, &["widgets:write".to_string()])
            .expect_err("missing permission should fail");

        let AuthError::InsufficientPermissions { required, has } = denied else {
            panic!("expected insufficient permissions");
        };
        assert_eq!(required, vec!["widgets:write"]);
        assert_eq!(
            has.into_iter().collect::<HashSet<_>>(),
            HashSet::from(["widgets:read".to_string()])
        );
    }

    #[test]
    fn reexported_jsonrpc_types_build_canonical_error_response() {
        let request = JsonRpcRequest::new(
            "missing_method".to_string(),
            Some(json!({ "id": "widget-1" })),
            Some(json!(7)),
        );
        let error = JsonRpcError::method_not_found(&request.method);
        let response = JsonRpcResponse::error(error, request.id);

        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, Some(json!(7)));
        assert!(response.result.is_none());

        let error = response.error.expect("error response");
        assert_eq!(error.code, error_codes::METHOD_NOT_FOUND);
        assert_eq!(error.message, "Method not found: missing_method");
    }

    #[test]
    fn reexported_jsonrpc_error_encodes_permission_details() {
        let error = JsonRpcError::insufficient_permissions(
            vec!["widgets:write".to_string()],
            vec!["widgets:read".to_string()],
        );

        assert_eq!(error.code, error_codes::INSUFFICIENT_PERMISSIONS);
        assert_eq!(error.message, "Insufficient permissions");
        assert_eq!(
            error.data,
            Some(json!({
                "required": ["widgets:write"],
                "has": ["widgets:read"]
            }))
        );
    }

    #[test]
    fn reexported_jsonrpc_success_response_preserves_result_and_id() {
        let response = JsonRpcResponse::success(json!({ "ok": true }), Some(json!("req-1")));

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, Some(json!("req-1")));
        assert_eq!(response.result, Some(json!({ "ok": true })));
        assert!(response.error.is_none());
    }

    #[test]
    fn reexported_auth_error_serializes_structured_permission_details() {
        let error = AuthError::InsufficientPermissions {
            required: vec!["widgets:write".to_string()],
            has: vec!["widgets:read".to_string()],
        };

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "InsufficientPermissions": {
                    "required": ["widgets:write"],
                    "has": ["widgets:read"]
                }
            })
        );
    }

    #[test]
    fn reexported_version_migration_trait_can_be_implemented() {
        let canonical = RenameCompat::migrate(RenameV1 {
            name: "Updated widget".to_string(),
        })
        .expect("infallible migration");

        assert_eq!(
            canonical,
            RenameV2 {
                display_name: "Updated widget".to_string(),
                notify: false,
            }
        );
    }

    #[test]
    fn reexported_version_migration_trait_supports_response_downgrade() {
        let legacy = RenameCompat::migrate(RenameResponseV2 {
            display_name: "Updated widget".to_string(),
            notified: true,
        })
        .expect("infallible response migration");

        assert_eq!(
            legacy,
            RenameResponseV1 {
                name: "Updated widget".to_string()
            }
        );
    }
}
