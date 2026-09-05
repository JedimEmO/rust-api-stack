//! Auth and lifecycle tests through the production chat application.
//!
//! These tests cover:
//! - In-memory fixture startup and health checks
//! - Login and registration flows
//! - Permission-bearing session creation
//! - Concurrent login handling

use anyhow::Result;
use axum::http::StatusCode;
use bidirectional_chat_server::config::{
    AdminConfig, AdminUser, AuthConfig, ChatConfig, Config, LoggingConfig, RateLimitConfig,
    RoomConfig, ServerConfig,
};
use bidirectional_chat_server::{ApplicationDependencies, build_application};
use ras_identity_local::LocalUserProvider;
use ras_identity_session::SessionService;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

/// Test server with auth and WebSocket routers wired through in-memory transport.
struct TestChatServer {
    server: Arc<axum_test::TestServer>,
    session_service: Arc<SessionService>,
    _temp_dir: TempDir,
}

impl TestChatServer {
    /// Start a new test chat server
    async fn start() -> Result<Self> {
        let temp_dir = TempDir::new()?;
        let data_dir = temp_dir.path().join("chat_data");

        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".parse().unwrap(),
                port: 3001,
                cors: Default::default(),
            },
            auth: AuthConfig {
                jwt_secret: "af64d581a2e84c58924af06fe77e57bf06352f27be50c16e".to_string(),
                jwt_ttl_seconds: 3600,
                refresh_enabled: true,
                jwt_algorithm: "HS256".to_string(),
            },
            chat: ChatConfig {
                data_dir,
                max_message_length: 1000,
                max_room_name_length: 50,
                max_users_per_room: 10,
                default_rooms: vec![RoomConfig {
                    id: "general".to_string(),
                    name: "General".to_string(),
                    description: Some("General chat room".to_string()),
                }],
                persist_messages: true,
                persist_rooms: true,
                persist_profiles: true,
            },
            admin: AdminConfig {
                users: vec![AdminUser {
                    username: "admin".to_string(),
                    password: "admin123456".to_string(),
                    email: Some("admin@test.com".to_string()),
                    display_name: Some("Test Admin".to_string()),
                    permissions: vec![
                        "admin".to_string(),
                        "moderator".to_string(),
                        "user".to_string(),
                    ],
                }],
                auto_create: true,
            },
            rate_limit: RateLimitConfig {
                enabled: false,
                ..Default::default()
            },
            logging: LoggingConfig::default(),
        };

        // Set up server components
        let identity_provider = Arc::new(LocalUserProvider::new());

        // Add test users
        let test_users = vec![
            ("alice", "alice123", Some("alice@test.com"), Some("Alice")),
            ("bob", "bob123", Some("bob@test.com"), Some("Bob")),
            (
                "charlie",
                "charlie123",
                Some("charlie@test.com"),
                Some("Charlie"),
            ),
        ];

        for (username, password, email, display_name) in &test_users {
            let _ = identity_provider
                .add_user(
                    username.to_string(),
                    password.to_string(),
                    email.map(|s| s.to_string()),
                    display_name.map(|s| s.to_string()),
                )
                .await;
        }

        let application = build_application(
            &config,
            ApplicationDependencies {
                identity_provider,
                seed_development_users: false,
            },
        )
        .await?;
        let session_service = application.session_service;
        let app = application.router;

        Ok(Self {
            server: Arc::new(
                axum_test::TestServer::builder()
                    .mock_transport()
                    .build(app)?,
            ),
            session_service,
            _temp_dir: temp_dir,
        })
    }

    async fn shutdown(self) {}

    /// Helper to login and get a token
    async fn login(&self, username: &str, password: &str) -> Result<String> {
        let response = self
            .server
            .post("/auth/login")
            .json(&json!({
                "username": username,
                "password": password,
            }))
            .await;

        if response.status_code() != StatusCode::OK {
            anyhow::bail!("Login failed with status: {}", response.status_code());
        }

        let body: serde_json::Value = response.json();
        Ok(body["token"].as_str().unwrap().to_string())
    }
}

#[tokio::test]
async fn test_server_lifecycle() -> Result<()> {
    let server = TestChatServer::start().await?;

    // Check health endpoint
    let response = server.server.get("/health").await;
    response.assert_status_ok();

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_user_authentication() -> Result<()> {
    let server = TestChatServer::start().await?;

    // Test login with valid credentials
    let token = server.login("alice", "alice123").await?;
    assert!(!token.is_empty());

    // Test login with invalid credentials
    let result = server.login("alice", "wrongpass").await;
    assert!(result.is_err());

    // Test login with non-existent user
    let result = server.login("nonexistent", "anypass").await;
    assert!(result.is_err());

    // Test malformed login payloads
    let missing_password = server
        .server
        .post("/auth/login")
        .json(&json!({ "username": "alice" }))
        .await;
    missing_password.assert_status(StatusCode::BAD_REQUEST);

    let missing_username = server
        .server
        .post("/auth/login")
        .json(&json!({ "password": "alice123" }))
        .await;
    missing_username.assert_status(StatusCode::BAD_REQUEST);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_user_registration() -> Result<()> {
    let server = TestChatServer::start().await?;

    // Register a new user
    let response = server
        .server
        .post("/auth/register")
        .json(&json!({
            "username": "newuser",
            "password": "newpass123",
            "email": "new@test.com",
            "display_name": "New User"
        }))
        .await;

    response.assert_status(StatusCode::CREATED);

    // The new user is added to the same identity provider that backs login.
    let token = server.login("newuser", "newpass123").await?;
    assert!(!token.is_empty());

    // Duplicate registration is rejected instead of overwriting credentials.
    let response = server
        .server
        .post("/auth/register")
        .json(&json!({
            "username": "newuser",
            "password": "newpass123"
        }))
        .await;

    response.assert_status(StatusCode::CONFLICT);

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_admin_permissions() -> Result<()> {
    let server = TestChatServer::start().await?;

    // Login as admin
    let admin_token = server.login("admin", "admin123456").await?;
    assert!(!admin_token.is_empty());
    let admin_claims = server.session_service.verify_session(&admin_token).await?;
    assert!(admin_claims.permissions.contains("admin"));
    assert!(admin_claims.permissions.contains("moderator"));

    // Login as regular user
    let user_token = server.login("alice", "alice123").await?;
    assert!(!user_token.is_empty());
    let user_claims = server.session_service.verify_session(&user_token).await?;
    assert!(user_claims.permissions.contains("user"));
    assert!(!user_claims.permissions.contains("admin"));

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_multiple_concurrent_users() -> Result<()> {
    let server = TestChatServer::start().await?;

    // Login multiple users concurrently
    let handles: Vec<_> = vec!["alice", "bob", "charlie"]
        .into_iter()
        .map(|username| {
            let server = Arc::clone(&server.server);
            tokio::spawn(async move {
                let response = server
                    .post("/auth/login")
                    .json(&json!({
                        "username": username,
                        "password": format!("{}123", username),
                    }))
                    .await;

                response.assert_status_ok();
                let body: serde_json::Value = response.json();
                assert!(body["token"].is_string());
            })
        })
        .collect();

    // Wait for all logins to complete
    for handle in handles {
        handle.await?;
    }

    server.shutdown().await;
    Ok(())
}
