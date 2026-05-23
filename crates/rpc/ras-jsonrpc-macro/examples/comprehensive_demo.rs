//! Feature tour example for the jsonrpc_service macro
//!
//! This example demonstrates:
//! - Service without OpenRPC
//! - Service with OpenRPC enabled (default path)
//! - Service with OpenRPC enabled (custom path)
//! - Various authentication requirements
//! - Multiple permission combinations

use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// Common types used across all services
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserRequest {
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdminAction {
    pub action: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    pub success: bool,
    pub message: String,
}

// Service without OpenRPC generation
mod basic_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: BasicService,
        methods: [
            UNAUTHORIZED health_check(()) -> StatusResponse,
            WITH_PERMISSIONS(["user.read"]) get_user(UserRequest) -> UserResponse,
        ]
    });
}

// Service with OpenRPC enabled using default path (target/openrpc/{lowercase-service-name}.json)
mod api_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: ApiService,
        openrpc: true,
        methods: [
            UNAUTHORIZED register(UserRequest) -> UserResponse,
            WITH_PERMISSIONS([]) authenticated_ping(()) -> StatusResponse,
            WITH_PERMISSIONS(["user.read"]) get_profile(()) -> UserResponse,
            WITH_PERMISSIONS(["user.write"]) update_profile(UserRequest) -> UserResponse,
            WITH_PERMISSIONS(["admin.read", "admin.write"]) admin_action(AdminAction) -> StatusResponse,
        ]
    });
}

// Service with OpenRPC enabled using custom output path
mod documented_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: DocumentedService,
        openrpc: { output: "docs/api/service.openrpc.json" },
        methods: [
            UNAUTHORIZED status(()) -> StatusResponse,
            WITH_PERMISSIONS(["service.use"]) process_request(UserRequest) -> UserResponse,
        ]
    });
}

// Simple auth provider for demo
struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            match token.as_str() {
                "admin-token" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("admin.read".to_string());
                    permissions.insert("admin.write".to_string());
                    Ok(AuthenticatedUser {
                        user_id: "admin-user".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                "user-token" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("user.read".to_string());
                    permissions.insert("user.write".to_string());
                    Ok(AuthenticatedUser {
                        user_id: "regular-user".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                "service-token" => {
                    let mut permissions = HashSet::new();
                    permissions.insert("service.use".to_string());
                    Ok(AuthenticatedUser {
                        user_id: "service-user".to_string(),
                        permissions,
                        metadata: None,
                    })
                }
                _ => Err(AuthError::InvalidToken),
            }
        })
    }
}

struct BasicServiceImpl;

impl basic_service::BasicServiceTrait for BasicServiceImpl {
    async fn health_check(
        &self,
        _request: (),
    ) -> Result<StatusResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(StatusResponse {
            success: true,
            message: "Service is healthy".to_string(),
        })
    }

    async fn get_user(
        &self,
        _user: &AuthenticatedUser,
        request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserResponse {
            id: "user-123".to_string(),
            username: request.username,
        })
    }
}

struct ApiServiceImpl;

impl api_service::ApiServiceTrait for ApiServiceImpl {
    async fn register(
        &self,
        request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserResponse {
            id: "new-user-456".to_string(),
            username: request.username,
        })
    }

    async fn authenticated_ping(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> Result<StatusResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(StatusResponse {
            success: true,
            message: "Pong!".to_string(),
        })
    }

    async fn get_profile(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserResponse {
            id: user.user_id.clone(),
            username: "profile_user".to_string(),
        })
    }

    async fn update_profile(
        &self,
        _user: &AuthenticatedUser,
        request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserResponse {
            id: "updated-user".to_string(),
            username: request.username,
        })
    }

    async fn admin_action(
        &self,
        _user: &AuthenticatedUser,
        action: AdminAction,
    ) -> Result<StatusResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(StatusResponse {
            success: true,
            message: format!(
                "Admin action {} on {} executed",
                action.action, action.target
            ),
        })
    }
}

struct DocumentedServiceImpl;

impl documented_service::DocumentedServiceTrait for DocumentedServiceImpl {
    async fn status(
        &self,
        _request: (),
    ) -> Result<StatusResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(StatusResponse {
            success: true,
            message: "Service is operational".to_string(),
        })
    }

    async fn process_request(
        &self,
        _user: &AuthenticatedUser,
        request: UserRequest,
    ) -> Result<UserResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserResponse {
            id: "processed-789".to_string(),
            username: request.username,
        })
    }
}

fn main() {
    println!("=== JSON-RPC Service Feature Tour ===\n");

    // Test basic service (no OpenRPC)
    println!("1. Basic Service (no OpenRPC):");
    let basic_builder = basic_service::BasicServiceBuilder::new(BasicServiceImpl)
        .base_url("/basic")
        .auth_provider(DemoAuthProvider);
    let _basic_router = basic_builder.build().expect("Failed to build BasicService");
    println!("   OK BasicService compiled successfully");
    println!("   OK No OpenRPC functions generated\n");

    // Test API service with default OpenRPC
    println!("2. API Service (OpenRPC enabled, default path):");
    let api_builder = api_service::ApiServiceBuilder::new(ApiServiceImpl)
        .base_url("/api/v1")
        .auth_provider(DemoAuthProvider);
    let _api_router = api_builder.build().expect("Failed to build ApiService");

    // Generate OpenRPC document
    let openrpc_doc = api_service::generate_apiservice_openrpc();
    println!("   OK ApiService compiled successfully");
    println!("   OK OpenRPC document generated:");
    println!("     - OpenRPC version: {}", openrpc_doc["openrpc"]);
    println!("     - API title: {}", openrpc_doc["info"]["title"]);
    println!(
        "     - Methods count: {}",
        openrpc_doc["methods"].as_array().unwrap().len()
    );

    // Write to default path
    match api_service::generate_apiservice_openrpc_to_file() {
        Ok(()) => println!("   OK Written to: target/openrpc/apiservice.json"),
        Err(e) => println!("   ERROR writing file: {}", e),
    }
    println!();

    // Test documented service with custom OpenRPC path
    println!("3. Documented Service (OpenRPC enabled, custom path):");
    let doc_builder = documented_service::DocumentedServiceBuilder::new(DocumentedServiceImpl)
        .base_url("/docs/api")
        .auth_provider(DemoAuthProvider);
    let _doc_router = doc_builder
        .build()
        .expect("Failed to build DocumentedService");

    // Generate OpenRPC document
    let doc_openrpc = documented_service::generate_documentedservice_openrpc();
    println!("   OK DocumentedService compiled successfully");
    println!("   OK OpenRPC document generated with custom path");
    println!(
        "     - Methods count: {}",
        doc_openrpc["methods"].as_array().unwrap().len()
    );

    // Write to custom path
    match documented_service::generate_documentedservice_openrpc_to_file() {
        Ok(()) => println!("   OK Written to: docs/api/service.openrpc.json"),
        Err(e) => println!("   ERROR writing file: {}", e),
    }
    println!();

    // Show method info
    println!("4. Method Summary:");
    println!("   BasicService:");
    println!("     - health_check: UNAUTHORIZED");
    println!("     - get_user: requires [user.read]");
    println!();
    println!("   ApiService:");
    println!("     - register: UNAUTHORIZED");
    println!("     - authenticated_ping: requires authentication (no specific permissions)");
    println!("     - get_profile: requires [user.read]");
    println!("     - update_profile: requires [user.write]");
    println!("     - admin_action: requires [admin.read, admin.write]");
    println!();
    println!("   DocumentedService:");
    println!("     - status: UNAUTHORIZED");
    println!("     - process_request: requires [service.use]");
}
