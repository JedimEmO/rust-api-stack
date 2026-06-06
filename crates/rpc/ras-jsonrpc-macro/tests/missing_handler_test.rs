use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use serde::{Deserialize, Serialize};

// Test types
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TestRequest {
    data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TestResponse {
    result: String,
}

// Simple auth provider for testing
struct TestAuthProvider;

impl AuthProvider for TestAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token == "valid-token" {
                Ok(AuthenticatedUser {
                    user_id: "test-user".to_string(),
                    permissions: ["admin".to_string()].into_iter().collect(),
                    metadata: None,
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        })
    }
}

// Generate a test service
mod test_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: TestService,
        methods: [
            UNAUTHORIZED method_one(TestRequest) -> TestResponse,
            WITH_PERMISSIONS(["admin"]) method_two(TestRequest) -> TestResponse,
            WITH_PERMISSIONS([]) method_three(TestRequest) -> TestResponse,
        ]
    });

    pub struct TestServiceImpl;

    impl TestServiceTrait for TestServiceImpl {
        async fn method_one(
            &self,
            _request: TestRequest,
        ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(TestResponse {
                result: "one".to_string(),
            })
        }

        async fn method_two(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            _request: TestRequest,
        ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(TestResponse {
                result: "two".to_string(),
            })
        }

        async fn method_three(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            _request: TestRequest,
        ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(TestResponse {
                result: "three".to_string(),
            })
        }
    }
}

#[test]
fn test_trait_based_builder_builds_when_trait_is_complete() {
    use test_service::*;

    let builder = TestServiceBuilder::new(TestServiceImpl)
        .base_url("/api")
        .auth_provider(TestAuthProvider);
    let result = builder.build();
    assert!(result.is_ok());
}
