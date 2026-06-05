use axum::Router;
use axum::http::StatusCode;
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tower::ServiceExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRequest {
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResponse {
    result: String,
}

jsonrpc_service!({
    service_name: TestService,
    methods: [
        UNAUTHORIZED public_method(TestRequest) -> TestResponse,
        WITH_PERMISSIONS(["user"]) user_method(TestRequest) -> TestResponse,
        WITH_PERMISSIONS(["admin"]) admin_method(TestRequest) -> TestResponse,
    ]
});

struct TestServiceImpl;

impl TestServiceTrait for TestServiceImpl {
    async fn public_method(
        &self,
        request: TestRequest,
    ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TestResponse {
            result: format!("Public: {}", request.value),
        })
    }

    async fn user_method(
        &self,
        _user: &AuthenticatedUser,
        request: TestRequest,
    ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TestResponse {
            result: format!("User: {}", request.value),
        })
    }

    async fn admin_method(
        &self,
        _user: &AuthenticatedUser,
        request: TestRequest,
    ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TestResponse {
            result: format!("Admin: {}", request.value),
        })
    }
}

// Mock auth provider
struct MockAuthProvider;

impl AuthProvider for MockAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            match token.as_str() {
                "user-token" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("user".to_string());
                    Ok(AuthenticatedUser {
                        user_id: "user1".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                "admin-token" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("admin".to_string());
                    Ok(AuthenticatedUser {
                        user_id: "admin1".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                "expired-token" => Err(AuthError::TokenExpired),
                _ => Err(AuthError::InvalidToken),
            }
        })
    }
}

async fn make_jsonrpc_request(
    app: Router,
    method: &str,
    params: serde_json::Value,
    auth_header: Option<&str>,
) -> axum::response::Response {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri("/rpc")
        .header("Content-Type", "application/json");

    if let Some(auth) = auth_header {
        request = request.header("Authorization", auth);
    }

    let request = request
        .body(axum::body::Body::from(request_body.to_string()))
        .unwrap();

    app.oneshot(request).await.unwrap()
}

fn test_app() -> Router {
    TestServiceBuilder::new(TestServiceImpl)
        .base_url("/rpc")
        .auth_provider(MockAuthProvider)
        .build()
        .expect("Failed to build router")
}

#[tokio::test]
async fn test_authentication_required_returns_401() {
    let app = test_app();

    // Test: No auth header for protected method should return 401
    let response = make_jsonrpc_request(
        app.clone(),
        "user_method",
        serde_json::json!({"value": "test"}),
        None,
    )
    .await;

    // Should return 401 for authentication required
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Check the JSON-RPC error code
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], -32001); // AUTHENTICATION_REQUIRED
}

#[tokio::test]
async fn test_insufficient_permissions_returns_403() {
    let app = test_app();

    // Test: User token trying to access admin method should return 403
    let response = make_jsonrpc_request(
        app.clone(),
        "admin_method",
        serde_json::json!({"value": "test"}),
        Some("Bearer user-token"),
    )
    .await;

    // Should return 403 for insufficient permissions
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Check the JSON-RPC error code
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], -32002); // INSUFFICIENT_PERMISSIONS
}

#[tokio::test]
async fn test_invalid_token_returns_401() {
    let app = test_app();

    // Test: Invalid token should return 401
    let response = make_jsonrpc_request(
        app.clone(),
        "user_method",
        serde_json::json!({"value": "test"}),
        Some("Bearer invalid-token"),
    )
    .await;

    // Should return 401 for invalid token
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Check the JSON-RPC error code
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], -32001); // AUTHENTICATION_REQUIRED (due to failed authentication)
}

#[tokio::test]
async fn test_successful_auth_returns_200() {
    let app = test_app();

    // Test: Valid user token accessing user method should return 200
    let response = make_jsonrpc_request(
        app.clone(),
        "user_method",
        serde_json::json!({"value": "test"}),
        Some("Bearer user-token"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);

    // Check successful response
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["result"].is_object());
    assert_eq!(json["result"]["result"], "User: test");
}

#[tokio::test]
async fn test_token_expired_returns_401() {
    let app = test_app();

    // Test: Expired token should return 401
    let response = make_jsonrpc_request(
        app.clone(),
        "user_method",
        serde_json::json!({"value": "test"}),
        Some("Bearer expired-token"),
    )
    .await;

    // Should return 401 for expired token
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Check the JSON-RPC error code
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], -32003); // TOKEN_EXPIRED
}
