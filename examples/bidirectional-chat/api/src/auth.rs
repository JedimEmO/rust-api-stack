//! REST API definitions for authentication endpoints

use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request payload for user login
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LoginRequest {
    /// Username for authentication
    pub username: String,
    /// Password for authentication
    pub password: String,
    /// Optional provider ID (defaults to "local")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// Request payload for user registration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RegisterRequest {
    /// Username for the new account
    pub username: String,
    /// Password for the new account
    pub password: String,
    /// Optional email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Optional display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Response payload for successful authentication
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LoginResponse {
    /// JWT token for authentication
    pub token: String,
    /// Token expiration timestamp (Unix timestamp)
    pub expires_at: i64,
    /// User ID
    pub user_id: String,
}

/// Response payload for successful registration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RegisterResponse {
    /// Success message
    pub message: String,
    /// Username of the created user
    pub username: String,
    /// Display name if provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Response payload for health check
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HealthResponse {
    /// Health status
    pub status: String,
    /// Server timestamp
    pub timestamp: String,
}

// Define the REST service
rest_service!({
    service_name: ChatAuthService,
    base_path: "/",
    openapi: true,
    serve_docs: false,
    endpoints: [
        // Authentication endpoints
        POST UNAUTHORIZED auth/login(LoginRequest) -> LoginResponse,
        POST UNAUTHORIZED auth/register(RegisterRequest) -> RegisterResponse,

        // Health check endpoint
        GET UNAUTHORIZED health() -> HealthResponse,
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn login_request_omits_default_provider_when_absent() {
        let request = LoginRequest {
            username: "alice".to_string(),
            password: "alice123".to_string(),
            provider: None,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "username": "alice",
                "password": "alice123"
            })
        );
    }

    #[test]
    fn register_response_omits_display_name_when_absent() {
        let response = RegisterResponse {
            message: "User registered successfully".to_string(),
            username: "alice".to_string(),
            display_name: None,
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "message": "User registered successfully",
                "username": "alice"
            })
        );
    }

    #[test]
    fn login_response_serializes_token_expiry_and_user_id() {
        let response = LoginResponse {
            token: "jwt-token".to_string(),
            expires_at: 1_779_552_000,
            user_id: "alice".to_string(),
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "token": "jwt-token",
                "expires_at": 1779552000,
                "user_id": "alice"
            })
        );
    }

    #[test]
    fn register_request_omits_absent_profile_fields() {
        let request = RegisterRequest {
            username: "alice".to_string(),
            password: "alice123".to_string(),
            email: None,
            display_name: None,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "username": "alice",
                "password": "alice123"
            })
        );
    }

    #[cfg(feature = "server")]
    fn operation<'a>(
        doc: &'a serde_json::Value,
        path: &str,
        method: &str,
    ) -> &'a serde_json::Value {
        &doc["paths"][path][method]
    }

    #[cfg(feature = "server")]
    #[test]
    fn generated_openapi_documents_public_auth_routes() {
        let doc = generate_chatauthservice_openapi();

        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "ChatAuthService REST API");

        let login = operation(&doc, "/auth/login", "post");
        assert!(login.is_object());
        assert!(login.get("security").is_none());
        assert_eq!(
            login["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/LoginRequest"
        );
        assert_eq!(
            login["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/LoginResponse"
        );

        let register = operation(&doc, "/auth/register", "post");
        assert!(register.is_object());
        assert!(register.get("security").is_none());
        assert_eq!(
            register["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/RegisterRequest"
        );

        let health = operation(&doc, "/health", "get");
        assert!(health.is_object());
        assert!(health.get("security").is_none());
    }
}
