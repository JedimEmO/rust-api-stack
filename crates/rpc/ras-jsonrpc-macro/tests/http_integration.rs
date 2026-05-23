use rand::Rng;
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

// Test data structures for various scenarios
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct SignInRequest {
    email: String,
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct SignInResponse {
    jwt: String,
    user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct CreateUserRequest {
    name: String,
    email: String,
    permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct User {
    id: Option<i32>,
    name: String,
    email: String,
    permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct ComplexRequest {
    data: Vec<NestedData>,
    metadata: Option<MetadataInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct NestedData {
    id: i32,
    value: String,
    active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct MetadataInfo {
    version: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct ProcessingResult {
    processed_count: usize,
    errors: Vec<String>,
    success: bool,
}

// Simple test auth provider
struct TestAuthProvider {
    valid_tokens: HashSet<String>,
}

impl TestAuthProvider {
    fn new() -> Self {
        let mut valid_tokens = HashSet::new();
        valid_tokens.insert("valid-admin-token".to_string());
        valid_tokens.insert("valid-user-token".to_string());
        valid_tokens.insert("valid-empty-perms-token".to_string());

        Self { valid_tokens }
    }
}

impl AuthProvider for TestAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if !self.valid_tokens.contains(&token) {
                return Err(AuthError::InvalidToken);
            }

            let (user_id, permissions) = match token.as_str() {
                "valid-admin-token" => {
                    ("admin-user", vec!["admin".to_string(), "user".to_string()])
                }
                "valid-user-token" => ("regular-user", vec!["user".to_string()]),
                "valid-empty-perms-token" => ("guest-user", vec![]),
                _ => return Err(AuthError::InvalidToken),
            };

            Ok(AuthenticatedUser {
                user_id: user_id.to_string(),
                permissions: permissions.into_iter().collect(),
                metadata: None,
            })
        })
    }
}

// Generate a broad test service
jsonrpc_service!({
    service_name: TestService,
    openrpc: true,
    methods: [
        // No auth required
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        UNAUTHORIZED get_public_info(()) -> String,
        UNAUTHORIZED echo_complex(ComplexRequest) -> ComplexRequest,

        // Any valid token required (empty permissions list)
        WITH_PERMISSIONS([]) sign_out(()) -> (),
        WITH_PERMISSIONS([]) get_user_info(()) -> User,
        WITH_PERMISSIONS([]) process_data(Vec<String>) -> ProcessingResult,

        // Specific permissions required
        WITH_PERMISSIONS(["admin"]) delete_everything(()) -> (),
        WITH_PERMISSIONS(["admin"]) create_user(CreateUserRequest) -> User,
        WITH_PERMISSIONS(["admin", "moderator"]) moderate_content(String) -> bool,

        // User permission required
        WITH_PERMISSIONS(["user"]) update_profile(User) -> User,
        WITH_PERMISSIONS(["user"]) get_user_data(i32) -> Option<User>,
    ]
});

struct TestServiceImpl;

impl TestServiceTrait for TestServiceImpl {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        if request.email == "admin@test.com" && request.password == "admin123" {
            Ok(SignInResponse {
                jwt: "valid-admin-token".to_string(),
                user_id: "admin-user".to_string(),
            })
        } else if request.email == "user@test.com" && request.password == "user123" {
            Ok(SignInResponse {
                jwt: "valid-user-token".to_string(),
                user_id: "regular-user".to_string(),
            })
        } else {
            Err("Invalid credentials".into())
        }
    }

    async fn get_public_info(
        &self,
        _request: (),
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("This is public information".to_string())
    }

    async fn echo_complex(
        &self,
        request: ComplexRequest,
    ) -> Result<ComplexRequest, Box<dyn std::error::Error + Send + Sync>> {
        Ok(request)
    }

    async fn sign_out(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn get_user_info(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        Ok(User {
            id: Some(123),
            name: format!("User {}", user.user_id),
            email: format!("{}@test.com", user.user_id),
            permissions: user.permissions.iter().cloned().collect(),
        })
    }

    async fn process_data(
        &self,
        _user: &AuthenticatedUser,
        data: Vec<String>,
    ) -> Result<ProcessingResult, Box<dyn std::error::Error + Send + Sync>> {
        Ok(ProcessingResult {
            processed_count: data.len(),
            errors: vec![],
            success: true,
        })
    }

    async fn delete_everything(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn create_user(
        &self,
        _user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        Ok(User {
            id: Some(rand::thread_rng().gen_range(1000..9999)),
            name: request.name,
            email: request.email,
            permissions: request.permissions,
        })
    }

    async fn moderate_content(
        &self,
        _user: &AuthenticatedUser,
        content: String,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(!content.contains("spam"))
    }

    async fn update_profile(
        &self,
        _user: &AuthenticatedUser,
        mut user: User,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        user.id = Some(456);
        Ok(user)
    }

    async fn get_user_data(
        &self,
        _user: &AuthenticatedUser,
        user_id: i32,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>> {
        if user_id == 123 {
            Ok(Some(User {
                id: Some(user_id),
                name: "Found User".to_string(),
                email: "found@test.com".to_string(),
                permissions: vec!["user".to_string()],
            }))
        } else {
            Ok(None)
        }
    }
}

fn create_test_server() -> axum_test::TestServer {
    let builder = TestServiceBuilder::new(TestServiceImpl)
        .base_url("/rpc")
        .auth_provider(TestAuthProvider::new());

    let app = builder.build().expect("Failed to build app");
    axum_test::TestServer::builder()
        .mock_transport()
        .build(app)
        .unwrap()
}

async fn make_jsonrpc_request(
    server: &axum_test::TestServer,
    method: &str,
    params: Value,
    token: Option<&str>,
) -> Value {
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let mut request = server.post("/rpc").json(&request_body);

    if let Some(token) = token {
        request = request.authorization_bearer(token);
    }

    request.await.json()
}

#[tokio::test]
async fn test_unauthorized_methods() {
    let server = create_test_server();

    // Test sign_in with valid credentials
    let response = make_jsonrpc_request(
        &server,
        "sign_in",
        json!({
            "email": "admin@test.com",
            "password": "admin123"
        }),
        None,
    )
    .await;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response.get("error").is_none());

    let result = &response["result"];
    assert_eq!(result["jwt"], "valid-admin-token");
    assert_eq!(result["user_id"], "admin-user");

    // Test sign_in with invalid credentials
    let response = make_jsonrpc_request(
        &server,
        "sign_in",
        json!({
            "email": "wrong@test.com",
            "password": "wrong"
        }),
        None,
    )
    .await;

    assert!(response.get("error").is_some());

    // Test get_public_info
    let response = make_jsonrpc_request(&server, "get_public_info", json!(()), None).await;

    assert_eq!(response["result"], "This is public information");

    // Test echo_complex
    let complex_data = json!({
        "data": [
            {"id": 1, "value": "test", "active": true},
            {"id": 2, "value": "test2", "active": false}
        ],
        "metadata": {
            "version": "1.0",
            "tags": ["test", "demo"]
        }
    });

    let response = make_jsonrpc_request(&server, "echo_complex", complex_data.clone(), None).await;

    assert_eq!(response["result"], complex_data);
}

#[tokio::test]
async fn test_authentication_required_methods() {
    let server = create_test_server();

    // Test without token - should fail
    let response = make_jsonrpc_request(&server, "sign_out", json!(()), None).await;

    assert!(response.get("error").is_some());
    let error = &response["error"];
    assert_eq!(error["code"], -32001); // Custom auth error code

    // Test with valid token - should succeed
    let response =
        make_jsonrpc_request(&server, "sign_out", json!(()), Some("valid-admin-token")).await;

    assert!(response.get("error").is_none());
    assert_eq!(response["result"], json!(()));

    // Test get_user_info with valid token
    let response = make_jsonrpc_request(
        &server,
        "get_user_info",
        json!(()),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "User regular-user");
    assert_eq!(result["email"], "regular-user@test.com");

    // Test process_data
    let response = make_jsonrpc_request(
        &server,
        "process_data",
        json!(["item1", "item2", "item3"]),
        Some("valid-empty-perms-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["processed_count"], 3);
    assert_eq!(result["success"].as_bool(), Some(true));
}

#[tokio::test]
async fn test_admin_permission_methods() {
    let server = create_test_server();

    // Test with user token (insufficient permissions) - should fail
    let response = make_jsonrpc_request(
        &server,
        "delete_everything",
        json!(()),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_some());
    let error = &response["error"];
    assert_eq!(error["code"], -32002); // Insufficient permissions error

    // Test with admin token - should succeed
    let response = make_jsonrpc_request(
        &server,
        "delete_everything",
        json!(()),
        Some("valid-admin-token"),
    )
    .await;

    assert!(response.get("error").is_none());

    // Test create_user with admin token
    let response = make_jsonrpc_request(
        &server,
        "create_user",
        json!({
            "name": "New User",
            "email": "new@test.com",
            "permissions": ["user"]
        }),
        Some("valid-admin-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "New User");
    assert_eq!(result["email"], "new@test.com");
    assert!(result["id"].as_i64().unwrap() >= 1000);
}

#[tokio::test]
async fn test_user_permission_methods() {
    let server = create_test_server();

    // Test with empty permissions token - should fail
    let response = make_jsonrpc_request(
        &server,
        "update_profile",
        json!({
            "name": "Updated User",
            "email": "updated@test.com",
            "permissions": []
        }),
        Some("valid-empty-perms-token"),
    )
    .await;

    assert!(response.get("error").is_some());

    // Test with user token - should succeed
    let response = make_jsonrpc_request(
        &server,
        "update_profile",
        json!({
            "name": "Updated User",
            "email": "updated@test.com",
            "permissions": []
        }),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "Updated User");
    assert_eq!(result["id"], 456);

    // Test get_user_data with existing user
    let response = make_jsonrpc_request(
        &server,
        "get_user_data",
        json!(123),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "Found User");

    // Test get_user_data with non-existing user
    let response = make_jsonrpc_request(
        &server,
        "get_user_data",
        json!(999),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    assert_eq!(response["result"], json!(null));
}

#[tokio::test]
async fn test_invalid_requests() {
    let server = create_test_server();

    // Test method not found
    let response = make_jsonrpc_request(&server, "non_existent_method", json!(()), None).await;

    assert!(response.get("error").is_some());
    let error = &response["error"];
    assert_eq!(error["code"], -32601); // Method not found

    // Test invalid JSON-RPC format (missing jsonrpc field)
    let invalid_request = json!({
        "method": "sign_in",
        "params": {},
        "id": 1
    });

    let json_response: Value = server.post("/rpc").json(&invalid_request).await.json();
    assert!(json_response.get("error").is_some());

    // Test invalid parameters for a method
    let response = make_jsonrpc_request(&server, "sign_in", json!("invalid_params"), None).await;

    assert!(response.get("error").is_some());
}

#[tokio::test]
async fn test_concurrent_requests() {
    let server = std::sync::Arc::new(create_test_server());

    // Test multiple concurrent requests
    let mut handles = vec![];

    for _ in 0..10 {
        let server = std::sync::Arc::clone(&server);
        let handle = tokio::spawn(async move {
            make_jsonrpc_request(&server, "get_public_info", json!(()), None).await
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let results = futures::future::join_all(handles).await;

    // All requests should succeed
    for result in results {
        let response = result.unwrap();
        assert_eq!(response["result"], "This is public information");
    }
}

#[tokio::test]
async fn test_openrpc_generation() {
    // Test that OpenRPC document is generated correctly
    let openrpc_doc = generate_testservice_openrpc();

    assert_eq!(openrpc_doc["openrpc"], "1.3.2");
    assert_eq!(openrpc_doc["info"]["title"], "TestService JSON-RPC API");

    let methods = openrpc_doc["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 11); // We have 11 methods defined

    // Check that unauthorized methods don't have authentication metadata
    let sign_in_method = methods.iter().find(|m| m["name"] == "sign_in").unwrap();
    assert!(sign_in_method.get("x-authentication").is_none());

    // Check that admin methods have correct permissions
    let delete_method = methods
        .iter()
        .find(|m| m["name"] == "delete_everything")
        .unwrap();
    assert_eq!(
        delete_method["x-authentication"]["required"].as_bool(),
        Some(true)
    );
    assert_eq!(delete_method["x-permissions"][0], "admin");

    // Check that methods with multiple permissions are correct
    let moderate_method = methods
        .iter()
        .find(|m| m["name"] == "moderate_content")
        .unwrap();
    let permissions = moderate_method["x-permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), 2);
    assert!(permissions.contains(&json!("admin")));
    assert!(permissions.contains(&json!("moderator")));
}

#[cfg(feature = "client")]
#[test]
fn test_client_generation() {
    // Test that client generation compiles and produces valid API
    let client_result = TestServiceClientBuilder::new()
        .server_url("http://example.invalid/rpc")
        .with_timeout(std::time::Duration::from_millis(1000))
        .build();

    assert!(client_result.is_ok());

    let mut client = client_result.unwrap();
    client.set_bearer_token(Some("test-token"));
    assert_eq!(client.bearer_token(), Some("test-token"));
}
