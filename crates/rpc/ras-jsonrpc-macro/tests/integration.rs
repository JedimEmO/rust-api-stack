use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use serde::{Deserialize, Serialize};

// Test types for requests and responses
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
    role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct User {
    id: String,
    name: String,
    role: String,
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

mod basic_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    // Generate the service using our macro
    jsonrpc_service!({
        service_name: MyService,
        methods: [
            /// Sign in with user credentials.
            UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
            WITH_PERMISSIONS(["admin"]) create_user(CreateUserRequest) -> User,
            WITH_PERMISSIONS([]) get_profile(()) -> User,
        ]
    });

    pub struct MyServiceImpl;

    impl MyServiceTrait for MyServiceImpl {
        async fn sign_in(
            &self,
            _request: SignInRequest,
        ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(SignInResponse {
                jwt: "test-jwt".to_string(),
                user_id: "123".to_string(),
            })
        }

        async fn create_user(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            request: CreateUserRequest,
        ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
            Ok(User {
                id: "new-id".to_string(),
                name: request.name,
                role: request.role,
            })
        }

        async fn get_profile(
            &self,
            user: &ras_jsonrpc_core::AuthenticatedUser,
            _request: (),
        ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
            Ok(User {
                id: user.user_id.clone(),
                name: "Test User".to_string(),
                role: "user".to_string(),
            })
        }
    }
}

#[tokio::test]
async fn test_macro_generates_code() {
    use basic_service::*;

    // Create a service builder
    let builder = MyServiceBuilder::new(MyServiceImpl)
        .base_url("/api/v1")
        .auth_provider(TestAuthProvider);

    // Build the router (this ensures all generated code compiles)
    let _router = builder.build().expect("Failed to build router");

    println!("Macro generated code successfully!");
}

// Generate a service with OpenRPC enabled
mod openrpc_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: OpenRpcService,
        openrpc: true,
        methods: [
            /// Sign in with user credentials.
            UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
            /// Create a user account.
            ///
            /// Requires administrator permissions and returns the created user.
            WITH_PERMISSIONS(["admin"]) create_user(CreateUserRequest) -> User,
            WITH_PERMISSIONS([]) sign_out(()) -> (),
        ]
    });

    pub struct OpenRpcServiceImpl;

    impl OpenRpcServiceTrait for OpenRpcServiceImpl {
        async fn sign_in(
            &self,
            _request: SignInRequest,
        ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(SignInResponse {
                jwt: "test-jwt".to_string(),
                user_id: "123".to_string(),
            })
        }

        async fn create_user(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            request: CreateUserRequest,
        ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
            Ok(User {
                id: "new-id".to_string(),
                name: request.name,
                role: request.role,
            })
        }

        async fn sign_out(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            _request: (),
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }
}

// Generate a service with custom OpenRPC output path
mod custom_path_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: CustomPathService,
        openrpc: { output: "custom/path/service.json" },
        methods: [
            UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
            WITH_PERMISSIONS(["admin"]) delete_everything(()) -> (),
        ]
    });

    pub struct CustomPathServiceImpl;

    impl CustomPathServiceTrait for CustomPathServiceImpl {
        async fn sign_in(
            &self,
            _request: SignInRequest,
        ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(SignInResponse {
                jwt: "test-jwt".to_string(),
                user_id: "123".to_string(),
            })
        }

        async fn delete_everything(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            _request: (),
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }
}

#[tokio::test]
async fn test_openrpc_generation() {
    use openrpc_service::*;

    // Create a service builder with OpenRPC enabled
    let builder = OpenRpcServiceBuilder::new(OpenRpcServiceImpl)
        .base_url("/api/v1")
        .auth_provider(TestAuthProvider);

    // Build the router
    let _router = builder.build().expect("Failed to build router");

    // Generate and write OpenRPC document
    let openrpc_doc = generate_openrpcservice_openrpc();
    assert_eq!(openrpc_doc["openrpc"], "1.3.2");
    assert_eq!(openrpc_doc["info"]["title"], "OpenRpcService JSON-RPC API");

    // Check that methods are present
    let methods = openrpc_doc["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 3);

    // Check sign_in method (unauthorized)
    let sign_in_method = methods.iter().find(|m| m["name"] == "sign_in").unwrap();
    assert!(sign_in_method.get("x-authentication").is_none());
    assert_eq!(sign_in_method["summary"], "Sign in with user credentials.");
    assert_eq!(
        sign_in_method["description"],
        "Sign in with user credentials."
    );

    // Check create_user method (requires admin permission)
    let create_user_method = methods.iter().find(|m| m["name"] == "create_user").unwrap();
    assert_eq!(
        create_user_method["x-authentication"]["required"].as_bool(),
        Some(true)
    );
    assert_eq!(create_user_method["x-permissions"][0], "admin");
    assert_eq!(create_user_method["summary"], "Create a user account.");
    assert_eq!(
        create_user_method["description"],
        "Create a user account.\n\nRequires administrator permissions and returns the created user."
    );

    // Check undocumented fallback summary remains generated
    let sign_out_method = methods.iter().find(|m| m["name"] == "sign_out").unwrap();
    assert_eq!(sign_out_method["summary"], "Calls the sign_out method");
    assert!(sign_out_method.get("description").is_none());

    // Test writing to file
    assert!(generate_openrpcservice_openrpc_to_file().is_ok());

    println!("OpenRPC generation test passed!");
}

#[tokio::test]
async fn test_custom_openrpc_path() {
    use custom_path_service::*;

    // Create a service builder
    let builder = CustomPathServiceBuilder::new(CustomPathServiceImpl)
        .base_url("/api/v2")
        .auth_provider(TestAuthProvider);

    // Build the router
    let _router = builder.build().expect("Failed to build router");

    // Generate OpenRPC document
    let openrpc_doc = generate_custompathservice_openrpc();
    assert_eq!(openrpc_doc["openrpc"], "1.3.2");

    println!("Custom OpenRPC path test passed!");
}
