//! Example demonstrating the trait-based JSON-RPC service setup.
//! All methods must be implemented by the generated trait.

use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use serde::{Deserialize, Serialize};

// Example types
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct CalculateRequest {
    a: i32,
    b: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct CalculateResponse {
    result: i32,
}

// Mock auth provider
struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token == "demo-token" {
                Ok(AuthenticatedUser {
                    user_id: "demo-user".to_string(),
                    permissions: ["user".to_string()].into_iter().collect(),
                    metadata: None,
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        })
    }
}

// Generate a calculator service
mod calculator_service {
    use super::*;
    use ras_jsonrpc_macro::jsonrpc_service;

    jsonrpc_service!({
        service_name: CalculatorService,
        methods: [
            UNAUTHORIZED add(CalculateRequest) -> CalculateResponse,
            UNAUTHORIZED subtract(CalculateRequest) -> CalculateResponse,
            WITH_PERMISSIONS(["user"]) multiply(CalculateRequest) -> CalculateResponse,
            WITH_PERMISSIONS(["user"]) divide(CalculateRequest) -> CalculateResponse,
        ]
    });

    pub struct CalculatorServiceImpl;

    impl CalculatorServiceTrait for CalculatorServiceImpl {
        async fn add(
            &self,
            req: CalculateRequest,
        ) -> Result<CalculateResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(CalculateResponse {
                result: req.a + req.b,
            })
        }

        async fn subtract(
            &self,
            req: CalculateRequest,
        ) -> Result<CalculateResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(CalculateResponse {
                result: req.a - req.b,
            })
        }

        async fn multiply(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            req: CalculateRequest,
        ) -> Result<CalculateResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(CalculateResponse {
                result: req.a * req.b,
            })
        }

        async fn divide(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            req: CalculateRequest,
        ) -> Result<CalculateResponse, Box<dyn std::error::Error + Send + Sync>> {
            if req.b == 0 {
                Err("Division by zero".into())
            } else {
                Ok(CalculateResponse {
                    result: req.a / req.b,
                })
            }
        }
    }
}

fn main() {
    use calculator_service::*;

    println!("=== JSON-RPC Service Trait Demo ===\n");

    println!("Building service from a trait implementation that covers every method...");

    let complete_builder = CalculatorServiceBuilder::new(CalculatorServiceImpl)
        .base_url("/api/calc")
        .auth_provider(DemoAuthProvider);

    // This should succeed
    let _router = complete_builder.build().expect("Failed to build service");
    println!("Build succeeded. All handlers are configured.");

    println!("\nSummary:");
    println!("- The JSON-RPC service builder accepts a generated trait implementation");
    println!("- Missing methods are now compile-time trait implementation errors");
    println!("- The builder still configures route path, auth, and observability hooks");
}
