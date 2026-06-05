use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
#[cfg(all(feature = "server", feature = "client"))]
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Request to create a new user account
pub struct CreateUserRequest {
    /// The desired username for the new account
    username: String,
    /// Email address for the user
    email: String,
    /// Password for the account (will be hashed)
    password: String,
    /// Optional display name
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateUserResponse {
    user_id: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Request to get user details by ID
pub struct GetUserRequest {
    /// The unique user identifier
    user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetUserResponse {
    user_id: String,
    username: String,
    email: String,
    display_name: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Search for users by various criteria
pub struct SearchUsersRequest {
    /// Optional username pattern to search for
    #[serde(skip_serializing_if = "Option::is_none")]
    username_pattern: Option<String>,
    /// Optional email pattern to search for
    #[serde(skip_serializing_if = "Option::is_none")]
    email_pattern: Option<String>,
    /// Maximum number of results to return
    #[serde(default = "default_limit")]
    limit: u32,
    /// Offset for pagination
    #[serde(default)]
    offset: u32,
}

fn default_limit() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchUsersResponse {
    users: Vec<UserSummary>,
    total_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UserSummary {
    user_id: String,
    username: String,
    email: String,
}

// Generate the service with explorer enabled
jsonrpc_service!({
    service_name: UserManagementService,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED create_user(CreateUserRequest) -> CreateUserResponse,
        WITH_PERMISSIONS(["users:read"]) get_user(GetUserRequest) -> GetUserResponse,
        WITH_PERMISSIONS(["users:list", "admin"]) search_users(SearchUsersRequest) -> SearchUsersResponse,
    ]
});

// Mock auth provider for demo
#[derive(Clone)]
struct MockAuthProvider;

impl AuthProvider for MockAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token == "valid-token" {
                let mut permissions = HashSet::new();
                permissions.insert("users:read".to_string());
                permissions.insert("users:list".to_string());

                Ok(AuthenticatedUser {
                    user_id: "user123".to_string(),
                    permissions,
                    metadata: Some(serde_json::json!({
                        "username": "demo_user",
                        "email": "demo@example.com"
                    })),
                })
            } else if token == "admin-token" {
                let mut permissions = HashSet::new();
                permissions.insert("users:read".to_string());
                permissions.insert("users:list".to_string());
                permissions.insert("admin".to_string());

                Ok(AuthenticatedUser {
                    user_id: "admin".to_string(),
                    permissions,
                    metadata: Some(serde_json::json!({
                        "username": "admin",
                        "email": "admin@example.com"
                    })),
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        })
    }
}

// Service implementation
#[derive(Clone)]
struct UserManagementServiceImpl;

// Don't use async-trait, let the service define it directly
impl UserManagementServiceTrait for UserManagementServiceImpl {
    async fn create_user(
        &self,
        req: CreateUserRequest,
    ) -> Result<CreateUserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(CreateUserResponse {
            user_id: format!("user_{}", rand::random::<u32>()),
            message: format!("User '{}' created successfully", req.username),
        })
    }

    async fn get_user(
        &self,
        _user: &ras_auth_core::AuthenticatedUser,
        req: GetUserRequest,
    ) -> Result<GetUserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(GetUserResponse {
            user_id: req.user_id.clone(),
            username: "demo_user".to_string(),
            email: "demo@example.com".to_string(),
            display_name: Some("Demo User".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
    }

    async fn search_users(
        &self,
        _user: &ras_auth_core::AuthenticatedUser,
        req: SearchUsersRequest,
    ) -> Result<SearchUsersResponse, Box<dyn std::error::Error + Send + Sync>> {
        let users = vec![
            UserSummary {
                user_id: "user1".to_string(),
                username: "alice".to_string(),
                email: "alice@example.com".to_string(),
            },
            UserSummary {
                user_id: "user2".to_string(),
                username: "bob".to_string(),
                email: "bob@example.com".to_string(),
            },
        ];

        let filtered_users: Vec<UserSummary> = users
            .into_iter()
            .filter(|u| {
                if let Some(pattern) = &req.username_pattern
                    && !u.username.contains(pattern)
                {
                    return false;
                }
                if let Some(pattern) = &req.email_pattern
                    && !u.email.contains(pattern)
                {
                    return false;
                }
                true
            })
            .skip(req.offset as usize)
            .take(req.limit as usize)
            .collect();

        Ok(SearchUsersResponse {
            total_count: filtered_users.len() as u32,
            users: filtered_users,
        })
    }
}

#[cfg(all(feature = "server", feature = "client"))]
#[tokio::main]
async fn main() {
    // Generate OpenRPC document
    generate_usermanagementservice_openrpc_to_file().unwrap();

    // Create service
    let auth_provider = MockAuthProvider;

    // Build the service using the trait-backed builder
    let builder = UserManagementServiceBuilder::new(UserManagementServiceImpl)
        .base_url("/api")
        .auth_provider(auth_provider);

    // Create router with explorer
    let app = builder.build().expect("Failed to build app");

    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Server running at http://127.0.0.1:3000");
    println!("Explorer available at http://127.0.0.1:3000/explorer");
    println!("API endpoint at http://127.0.0.1:3000/api");
    println!("\nTry these tokens:");
    println!("  - 'valid-token': Regular user with users:read and users:list permissions");
    println!("  - 'admin-token': Admin user with all permissions");

    axum::serve(listener, app).await.unwrap();
}

#[cfg(not(all(feature = "server", feature = "client")))]
fn main() {
    println!("This example requires both 'server' and 'client' features to be enabled.");
    println!(
        "Run with: cargo run --locked --example explorer_params_demo --features server,client"
    );
}
