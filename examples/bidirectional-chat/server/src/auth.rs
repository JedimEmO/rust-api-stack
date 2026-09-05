use crate::config;
use bidirectional_chat_api::auth::{
    HealthResponse, LoginRequest, LoginResponse, RegisterRequest, RegisterResponse,
};
use chrono::Utc;
use ras_identity_core::{UserPermissions, VerifiedIdentity};
use ras_identity_local::LocalUserProvider;
use ras_identity_session::SessionService;
use ras_rest_core::{RestError, RestResponse, RestResult};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub(crate) struct ChatPermissions {
    admin_users: Vec<config::AdminUser>,
}

// REST API handlers
#[derive(Clone)]
pub(crate) struct AuthHandlers {
    pub(crate) session_service: Arc<SessionService>,
    pub(crate) identity_provider: Arc<LocalUserProvider>,
}

impl ChatPermissions {
    pub(crate) fn new(admin_users: Vec<config::AdminUser>) -> Self {
        Self { admin_users }
    }
}

#[async_trait::async_trait]
impl UserPermissions for ChatPermissions {
    async fn get_permissions(
        &self,
        identity: &VerifiedIdentity,
    ) -> ras_identity_core::IdentityResult<Vec<String>> {
        // Check if user is in admin configuration
        for admin_user in &self.admin_users {
            if admin_user.username == identity.subject {
                return Ok(admin_user.permissions.clone());
            }
        }

        // Default permissions for regular users
        Ok(vec!["user".to_string()])
    }
}

impl AuthHandlers {
    async fn handle_login(&self, request: LoginRequest) -> RestResult<LoginResponse> {
        debug!("Processing login request");

        // Create auth payload
        let provider_id = request.provider.as_deref().unwrap_or("local");
        let auth_payload = json!({
            "username": request.username,
            "password": request.password,
            "provider": provider_id,
        });

        // Begin session
        let token = self
            .session_service
            .begin_session(provider_id, auth_payload)
            .await
            .map_err(|e| {
                warn!(provider = %provider_id, "Login failed: {}", e);
                RestError::unauthorized("Invalid credentials")
            })?;

        // Parse token to get user info (for response)
        let claims = self
            .session_service
            .verify_session(&token)
            .await
            .map_err(|e| {
                warn!("Token verification failed: {}", e);
                RestError::internal_server_error("Token verification failed")
            })?;

        info!(user_id = %claims.sub, "User logged in successfully");
        Ok(RestResponse::ok(LoginResponse {
            token,
            expires_at: claims.exp,
            user_id: claims.sub,
        }))
    }

    async fn handle_register(&self, request: RegisterRequest) -> RestResult<RegisterResponse> {
        debug!("Processing registration request");

        // Add user
        self.identity_provider
            .add_user(
                request.username.clone(),
                request.password,
                request.email.clone(),
                request.display_name.clone(),
            )
            .await
            .map_err(|e| {
                warn!(username = %request.username, "Registration failed: {}", e);
                RestError::conflict("Username already exists")
            })?;

        info!(username = %request.username, email = ?request.email, "User registered successfully");

        Ok(RestResponse::created(RegisterResponse {
            message: "User registered successfully".to_string(),
            username: request.username,
            display_name: request.display_name,
        }))
    }

    async fn handle_health(&self) -> RestResult<HealthResponse> {
        Ok(RestResponse::ok(HealthResponse {
            status: "OK".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }))
    }
}
pub(crate) struct AuthServiceImpl {
    pub(crate) handlers: AuthHandlers,
}

#[async_trait::async_trait]
impl bidirectional_chat_api::auth::ChatAuthServiceTrait for AuthServiceImpl {
    async fn post_auth_login(&self, request: LoginRequest) -> RestResult<LoginResponse> {
        self.handlers.handle_login(request).await
    }

    async fn post_auth_register(&self, request: RegisterRequest) -> RestResult<RegisterResponse> {
        self.handlers.handle_register(request).await
    }

    async fn get_health(&self) -> RestResult<HealthResponse> {
        self.handlers.handle_health().await
    }
}
