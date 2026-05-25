//! Chat server auth and lifecycle integration tests
//!
//! These tests cover:
//! - In-memory server startup and health checks
//! - Login and registration flows
//! - Permission-bearing session creation
//! - Concurrent login handling

use anyhow::Result;
use axum::{Router, http::StatusCode, routing::get};
use bidirectional_chat_api::*;
use bidirectional_chat_server::config::{
    AdminConfig, AdminUser, AuthConfig, ChatConfig, Config, LoggingConfig, RateLimitConfig,
    RoomConfig, ServerConfig,
};
use chrono::Utc;
use ras_auth_core::AuthenticatedUser;
use ras_identity_core::{UserPermissions, VerifiedIdentity};
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAlgorithm, JwtAuthProvider, SessionConfig, SessionService};
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager, WebSocketServiceBuilder,
    service::{BuiltWebSocketService, websocket_handler},
};
use ras_jsonrpc_bidirectional_types::{ConnectionId, ConnectionManager};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use tempfile::TempDir;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

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
                jwt_secret: "test-secret-key-that-is-long-enough".to_string(),
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

        // Add admin users
        for admin_user in &config.admin.users {
            let _ = identity_provider
                .add_user(
                    admin_user.username.clone(),
                    admin_user.password.clone(),
                    admin_user.email.clone(),
                    admin_user.display_name.clone(),
                )
                .await;
        }

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

        // Create session service
        let session_config = SessionConfig {
            jwt_secret: config.auth.jwt_secret.clone(),
            jwt_ttl: chrono::Duration::seconds(config.auth.jwt_ttl_seconds),
            refresh_enabled: config.auth.refresh_enabled,
            enforce_active_sessions: true,
            algorithm: JwtAlgorithm::HS256,
        };

        let session_service = Arc::new(
            SessionService::new(session_config)
                .unwrap()
                .with_permissions(Arc::new(TestChatPermissions::new(
                    config.admin.users.clone(),
                ))),
        );

        session_service
            .register_provider(Box::new((*identity_provider).clone()))
            .await;

        // Create JWT auth provider
        let auth_provider = JwtAuthProvider::new(session_service.clone());

        // Create connection manager
        let connection_manager = Arc::new(DefaultConnectionManager::new());

        // Create chat server
        let chat_server = Arc::new(ChatServer::new(config.chat.clone()).await?);

        // Create handler
        let handler = Arc::new(ChatServiceHandler::new(
            chat_server.clone(),
            connection_manager.clone(),
        ));

        // Build WebSocket service
        let ws_service = WebSocketServiceBuilder::builder()
            .handler(handler)
            .auth_provider(Arc::new(auth_provider.clone()))
            .require_auth(true)
            .build()
            .build_with_manager(connection_manager);

        // Create routers
        let auth_router = Router::new()
            .route("/auth/login", axum::routing::post(login_handler))
            .route("/auth/register", axum::routing::post(register_handler))
            .with_state((
                Arc::clone(&session_service),
                Arc::clone(&identity_provider),
                chat_server,
            ));

        type ChatServiceType = BuiltWebSocketService<
            ChatServiceHandler<ChatServer, DefaultConnectionManager>,
            JwtAuthProvider,
            DefaultConnectionManager,
        >;
        let ws_router = Router::new()
            .route("/ws", get(websocket_handler::<ChatServiceType>))
            .with_state(ws_service);

        let health_router = Router::new().route("/health", get(|| async { "OK" }));

        // Combine all routers
        let app = Router::new()
            .merge(auth_router)
            .merge(ws_router)
            .merge(health_router)
            .layer(CorsLayer::permissive());

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

// Permission provider for tests
#[derive(Clone)]
struct TestChatPermissions {
    admin_users: Vec<AdminUser>,
}

impl TestChatPermissions {
    fn new(admin_users: Vec<AdminUser>) -> Self {
        Self { admin_users }
    }
}

#[async_trait::async_trait]
impl UserPermissions for TestChatPermissions {
    async fn get_permissions(
        &self,
        identity: &VerifiedIdentity,
    ) -> ras_identity_core::IdentityResult<Vec<String>> {
        for admin_user in &self.admin_users {
            if admin_user.username == identity.subject {
                return Ok(admin_user.permissions.clone());
            }
        }
        Ok(vec!["user".to_string()])
    }
}

// Handler implementations
async fn login_handler(
    axum::extract::State((session_service, _identity_provider, _chat_server)): axum::extract::State<
        (Arc<SessionService>, Arc<LocalUserProvider>, Arc<ChatServer>),
    >,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    let provider_id = payload
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("local");

    let token = session_service
        .begin_session(provider_id, payload.clone())
        .await
        .map_err(|_| axum::http::StatusCode::UNAUTHORIZED)?;

    let claims = session_service
        .verify_session(&token)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(axum::Json(json!({
        "token": token,
        "expires_at": claims.exp,
        "user_id": claims.sub,
    })))
}

async fn register_handler(
    axum::extract::State((_session_service, identity_provider, _chat_server)): axum::extract::State<
        (Arc<SessionService>, Arc<LocalUserProvider>, Arc<ChatServer>),
    >,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    let username = payload
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    let password = payload
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    let email = payload
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let display_name = payload
        .get("display_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    identity_provider
        .add_user(
            username.to_string(),
            password.to_string(),
            email.clone(),
            display_name.clone(),
        )
        .await
        .map_err(|_| axum::http::StatusCode::CONFLICT)?;

    Ok(axum::Json(json!({
        "message": "User registered successfully",
        "username": username,
        "display_name": display_name,
    })))
}

// Import ChatServer from main.rs
use bidirectional_chat_server::persistence::{PersistedRoom, PersistenceManager};
use dashmap::DashMap;

#[derive(Debug, Clone)]
struct ChatRoom {
    id: String,
    name: String,
    users: HashSet<String>,
    created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct UserSession;

#[derive(Clone)]
struct ChatServer {
    rooms: Arc<DashMap<String, ChatRoom>>,
    user_sessions: Arc<DashMap<ConnectionId, UserSession>>,
    message_counter: Arc<RwLock<u64>>,
    persistence: Arc<PersistenceManager>,
}

impl ChatServer {
    async fn new(config: ChatConfig) -> Result<Self> {
        let persistence = Arc::new(PersistenceManager::new(&config.data_dir));
        persistence.init().await?;

        let mut state = persistence.load_state().await?;

        let server = Self {
            rooms: Arc::new(DashMap::new()),
            user_sessions: Arc::new(DashMap::new()),
            message_counter: Arc::new(RwLock::new(state.next_message_id)),
            persistence,
        };

        // Create default rooms
        if state.rooms.is_empty() {
            for room_config in &config.default_rooms {
                let room = ChatRoom {
                    id: room_config.id.clone(),
                    name: room_config.name.clone(),
                    users: HashSet::new(),
                    created_at: Utc::now(),
                };
                server.rooms.insert(room_config.id.clone(), room.clone());

                state.rooms.insert(
                    room_config.id.clone(),
                    PersistedRoom {
                        id: room.id,
                        name: room.name,
                        created_at: room.created_at,
                        users: room.users.clone(),
                    },
                );
            }

            if !state.rooms.is_empty() {
                server.persistence.save_state(&state).await?;
            }
        } else {
            for (id, persisted_room) in state.rooms {
                let room = ChatRoom {
                    id: persisted_room.id,
                    name: persisted_room.name,
                    users: HashSet::new(),
                    created_at: persisted_room.created_at,
                };
                server.rooms.insert(id, room);
            }
        }

        Ok(server)
    }

    async fn next_message_id(&self) -> u64 {
        let mut counter = self.message_counter.write().await;
        let id = *counter;
        *counter += 1;
        id
    }
}

// Minimal implementation of ChatServiceService for testing
#[async_trait::async_trait]
impl ChatServiceService for ChatServer {
    async fn send_message(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: SendMessageRequest,
    ) -> Result<SendMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Minimal implementation for testing
        let message_id = self.next_message_id().await;
        let timestamp = Utc::now().to_rfc3339();

        Ok(SendMessageResponse {
            message_id,
            timestamp,
        })
    }

    async fn join_room(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: JoinRoomRequest,
    ) -> Result<JoinRoomResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Minimal implementation for testing
        let room_id = request.room_name.clone();
        let user_count = 1;

        Ok(JoinRoomResponse {
            room_id,
            user_count,
            existing_users: vec![],
        })
    }

    async fn leave_room(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: LeaveRoomRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn list_rooms(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: ListRoomsRequest,
    ) -> Result<ListRoomsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let rooms: Vec<RoomInfo> = self
            .rooms
            .iter()
            .map(|entry| RoomInfo {
                room_id: entry.id.clone(),
                room_name: entry.name.clone(),
                user_count: entry.users.len() as u32,
            })
            .collect();

        Ok(ListRoomsResponse { rooms })
    }

    async fn kick_user(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: KickUserRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(true)
    }

    async fn broadcast_announcement(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: BroadcastAnnouncementRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn get_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        request: GetProfileRequest,
    ) -> Result<GetProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Return a default profile for testing
        let profile = UserProfile {
            username: request.username,
            display_name: None,
            avatar: CatAvatar {
                breed: CatBreed::Tabby,
                color: CatColor::Orange,
                expression: CatExpression::Happy,
            },
            created_at: Utc::now().to_rfc3339(),
            last_seen: Utc::now().to_rfc3339(),
        };

        Ok(GetProfileResponse { profile })
    }

    async fn update_profile(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        user: &AuthenticatedUser,
        request: UpdateProfileRequest,
    ) -> Result<UpdateProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Return updated profile for testing
        let profile = UserProfile {
            username: user.user_id.clone(),
            display_name: request.display_name,
            avatar: request.avatar.unwrap_or(CatAvatar {
                breed: CatBreed::Tabby,
                color: CatColor::Orange,
                expression: CatExpression::Happy,
            }),
            created_at: Utc::now().to_rfc3339(),
            last_seen: Utc::now().to_rfc3339(),
        };

        Ok(UpdateProfileResponse { profile })
    }

    // Notification methods (not used by server)
    async fn notify_message_received(
        &self,
        _connection_id: ConnectionId,
        _params: MessageReceivedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_joined(
        &self,
        _connection_id: ConnectionId,
        _params: UserJoinedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_left(
        &self,
        _connection_id: ConnectionId,
        _params: UserLeftNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_system_announcement(
        &self,
        _connection_id: ConnectionId,
        _params: SystemAnnouncementNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_kicked(
        &self,
        _connection_id: ConnectionId,
        _params: UserKickedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_room_created(
        &self,
        _connection_id: ConnectionId,
        _params: RoomCreatedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_room_deleted(
        &self,
        _connection_id: ConnectionId,
        _params: RoomDeletedNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn start_typing(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: StartTypingRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn stop_typing(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        _request: StopTypingRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn notify_user_started_typing(
        &self,
        _connection_id: ConnectionId,
        _params: UserStartedTypingNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    async fn notify_user_stopped_typing(
        &self,
        _connection_id: ConnectionId,
        _params: UserStoppedTypingNotification,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }

    // Lifecycle hooks
    async fn on_client_connected(
        &self,
        _client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn on_client_disconnected(
        &self,
        client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Remove user session
        self.user_sessions.remove(&client_id);
        Ok(())
    }

    async fn on_client_authenticated(
        &self,
        client_id: ConnectionId,
        _connection_manager: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create user session
        self.user_sessions.insert(client_id, UserSession);
        Ok(())
    }
}

// Tests

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
    missing_password.assert_status(StatusCode::UNAUTHORIZED);

    let missing_username = server
        .server
        .post("/auth/login")
        .json(&json!({ "password": "alice123" }))
        .await;
    missing_username.assert_status(StatusCode::UNAUTHORIZED);

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

    response.assert_status_ok();

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
