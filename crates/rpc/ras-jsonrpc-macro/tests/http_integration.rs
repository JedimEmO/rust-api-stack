use rand::Rng;
use ras_jsonrpc_core::{AuthCookieConfig, AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

// Test data structures for various scenarios
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SignInRequest {
    email: String,
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SignInResponse {
    jwt: String,
    user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateUserRequest {
    name: String,
    email: String,
    permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct User {
    id: Option<i32>,
    name: String,
    email: String,
    permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComplexRequest {
    data: Vec<NestedData>,
    metadata: Option<MetadataInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NestedData {
    id: i32,
    value: String,
    active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MetadataInfo {
    version: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProcessingResult {
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

// Cookie auth installs double-submit CSRF protection by default.
fn create_cookie_test_server() -> axum_test::TestServer {
    let builder = TestServiceBuilder::new(TestServiceImpl)
        .base_url("/rpc")
        .auth_provider(TestAuthProvider::new())
        .auth_cookie(AuthCookieConfig::default());

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

#[path = "http_integration/auth.rs"]
mod auth;
#[path = "http_integration/client.rs"]
mod client;
#[path = "http_integration/parameters.rs"]
mod parameters;
#[path = "http_integration/specs.rs"]
mod specs;
