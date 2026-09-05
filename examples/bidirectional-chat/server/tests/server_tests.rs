//! Chat configuration, persistence, and production application startup tests.

use anyhow::Result;
use bidirectional_chat_server::config::{
    AdminConfig, AdminUser, AuthConfig, ChatConfig, Config, LoggingConfig, RateLimitConfig,
    RoomConfig, ServerConfig,
};
use bidirectional_chat_server::{ApplicationDependencies, build_application};
use config::{Config as FileConfig, File};
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAlgorithm, SessionConfig};
use std::sync::Arc;
use tempfile::TempDir;

/// Test server instance
struct TestServer {
    server: axum_test::TestServer,
}

impl TestServer {
    async fn start(config: Config) -> Result<Self> {
        let application = build_application(
            &config,
            ApplicationDependencies {
                identity_provider: Arc::new(LocalUserProvider::new()),
                seed_development_users: false,
            },
        )
        .await?;
        let app = application.router;

        Ok(Self {
            server: axum_test::TestServer::builder()
                .mock_transport()
                .build(app)?,
        })
    }

    async fn shutdown(self) {}
}

// Helper function to create test configuration
async fn create_test_config() -> Result<(Config, TempDir)> {
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

    Ok((config, temp_dir))
}

#[tokio::test]
async fn test_config_defaults() {
    let config = Config::default();
    assert_eq!(config.server.port, 3000);
    assert_eq!(config.auth.jwt_ttl_seconds, 86400);
    assert_eq!(config.chat.max_message_length, 1000);
}

#[tokio::test]
async fn test_config_validation() {
    let mut config = Config::default();

    // Test invalid port
    config.server.port = 0;
    assert!(config.validate().is_err());

    // Test invalid JWT TTL
    config.server.port = 3000;
    config.auth.jwt_ttl_seconds = 0;
    assert!(config.validate().is_err());

    // Test invalid message length
    config.auth.jwt_ttl_seconds = 3600;
    config.chat.max_message_length = 0;
    assert!(config.validate().is_err());
}

#[test]
fn config_example_loads_with_session_compatible_secret() -> Result<()> {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
    let config: Config = FileConfig::builder()
        .add_source(File::from(config_path))
        .build()?
        .try_deserialize()?;

    config.validate()?;

    let session_config = SessionConfig {
        jwt_secret: config.auth.jwt_secret,
        jwt_ttl: chrono::Duration::seconds(config.auth.jwt_ttl_seconds),
        enforce_active_sessions: true,
        algorithm: JwtAlgorithm::HS256,
        iss: Some("bidirectional-chat".to_string()),
        aud: Some("bidirectional-chat".to_string()),
        require_iss_aud: true,
        max_sessions_per_user: ras_identity_session::DEFAULT_MAX_SESSIONS_PER_USER,
    };

    session_config.validate()?;

    Ok(())
}

#[tokio::test]
async fn test_server_startup() -> Result<()> {
    let (config, _temp_dir) = create_test_config().await?;
    let server = TestServer::start(config).await?;

    // Test health endpoint
    let response = server.server.get("/health").await;

    response.assert_status_ok();
    assert_eq!(response.json::<serde_json::Value>()["status"], "OK");

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn test_persistence_initialization() -> Result<()> {
    use bidirectional_chat_server::persistence::PersistenceManager;

    let temp_dir = TempDir::new()?;
    let persistence = PersistenceManager::new(temp_dir.path());

    // Initialize persistence
    persistence.init().await?;

    // Verify directories were created
    assert!(temp_dir.path().exists());
    assert!(temp_dir.path().join("messages").exists());

    // Test save and load state
    let mut state = persistence.load_state().await?;
    assert_eq!(state.next_message_id, 1);

    state.next_message_id = 42;
    persistence.save_state(&state).await?;

    let loaded_state = persistence.load_state().await?;
    assert_eq!(loaded_state.next_message_id, 42);

    Ok(())
}

#[tokio::test]
async fn test_room_configuration() -> Result<()> {
    let (mut config, _temp_dir) = create_test_config().await?;

    // Add multiple default rooms
    config.chat.default_rooms = vec![
        RoomConfig {
            id: "general".to_string(),
            name: "General".to_string(),
            description: Some("General discussion".to_string()),
        },
        RoomConfig {
            id: "tech".to_string(),
            name: "Technology".to_string(),
            description: Some("Tech discussion".to_string()),
        },
    ];

    // Validation should pass
    assert!(config.validate().is_ok());

    // Test invalid room configuration
    config.chat.default_rooms.push(RoomConfig {
        id: "".to_string(), // Empty ID should fail
        name: "Invalid".to_string(),
        description: None,
    });

    assert!(config.validate().is_err());

    Ok(())
}

#[tokio::test]
async fn test_admin_configuration() -> Result<()> {
    let (mut config, _temp_dir) = create_test_config().await?;

    // Test valid admin configuration
    config.admin.users = vec![
        AdminUser {
            username: "admin1".to_string(),
            password: "adminpass123".to_string(),
            email: Some("admin1@test.com".to_string()),
            display_name: Some("Admin One".to_string()),
            permissions: vec!["admin".to_string()],
        },
        AdminUser {
            username: "moderator1".to_string(),
            password: "modpass123".to_string(),
            email: None,
            display_name: None,
            permissions: vec!["moderator".to_string(), "user".to_string()],
        },
    ];

    assert!(config.validate().is_ok());

    // Test invalid admin configuration (short password)
    config.admin.users.push(AdminUser {
        username: "badadmin".to_string(),
        password: "short".to_string(), // Too short
        email: None,
        display_name: None,
        permissions: vec!["admin".to_string()],
    });

    assert!(config.validate().is_err());

    Ok(())
}

#[tokio::test]
async fn test_rate_limit_configuration() -> Result<()> {
    let (mut config, _temp_dir) = create_test_config().await?;

    // Enable rate limiting with valid values
    config.rate_limit.enabled = true;
    config.rate_limit.messages_per_minute = 60;
    config.rate_limit.connections_per_ip = 5;
    config.rate_limit.login_attempts_per_hour = 10;

    assert!(config.validate().is_ok());

    // Test invalid rate limit configuration
    config.rate_limit.messages_per_minute = 0;
    assert!(config.validate().is_err());

    Ok(())
}

#[tokio::test]
async fn test_cors_configuration() -> Result<()> {
    let (mut config, _temp_dir) = create_test_config().await?;

    // Test allow any origin
    config.server.cors.allow_any_origin = true;
    assert!(config.validate().is_ok());

    // Test specific origins
    config.server.cors.allow_any_origin = false;
    config.server.cors.allowed_origins = vec![
        "http://localhost:3000".to_string(),
        "https://example.com".to_string(),
    ];
    assert!(config.validate().is_ok());

    // Test invalid CORS configuration (no origins when not allowing any)
    config.server.cors.allowed_origins.clear();
    assert!(config.validate().is_err());

    Ok(())
}

#[tokio::test]
async fn test_logging_configuration() -> Result<()> {
    let (mut config, _temp_dir) = create_test_config().await?;

    // Test valid log levels
    for level in ["trace", "debug", "info", "warn", "error"] {
        config.logging.level = level.to_string();
        assert!(config.validate().is_ok());
    }

    // Test invalid log level
    config.logging.level = "invalid".to_string();
    assert!(config.validate().is_err());

    // Test valid log formats
    for format in ["pretty", "json", "compact"] {
        config.logging.level = "info".to_string();
        config.logging.format = format.to_string();
        assert!(config.validate().is_ok());
    }

    // Test invalid log format
    config.logging.format = "invalid".to_string();
    assert!(config.validate().is_err());

    Ok(())
}

#[tokio::test]
async fn test_message_persistence() -> Result<()> {
    use bidirectional_chat_server::persistence::{PersistedMessage, PersistenceManager};
    use chrono::Utc;

    let temp_dir = TempDir::new()?;
    let persistence = PersistenceManager::new(temp_dir.path());
    persistence.init().await?;

    // Create and persist messages
    let room_id = "test-room";
    let messages = vec![
        PersistedMessage {
            id: 1,
            room_id: room_id.to_string(),
            username: "alice".to_string(),
            text: "Hello!".to_string(),
            timestamp: Utc::now(),
        },
        PersistedMessage {
            id: 2,
            room_id: room_id.to_string(),
            username: "bob".to_string(),
            text: "Hi there!".to_string(),
            timestamp: Utc::now(),
        },
    ];

    for msg in &messages {
        persistence.append_message(room_id, msg).await?;
    }

    // Load messages
    let loaded = persistence.load_room_messages(room_id, None).await?;
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].text, "Hello!");
    assert_eq!(loaded[1].text, "Hi there!");

    // Test limit
    let limited = persistence.load_room_messages(room_id, Some(1)).await?;
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].text, "Hi there!"); // Most recent

    // Test non-existent room
    let empty = persistence.load_room_messages("non-existent", None).await?;
    assert!(empty.is_empty());

    Ok(())
}

#[tokio::test]
async fn application_router_authenticates_websocket_and_persists_messages() -> Result<()> {
    use bidirectional_chat_api::{
        ChatServiceClientBuilder, JoinRoomRequest, ListRoomsRequest, SendMessageRequest,
    };
    use bidirectional_chat_server::persistence::PersistenceManager;
    use serde_json::json;
    use std::time::Duration;

    let (config, _temp_dir) = create_test_config().await?;
    let application = build_application(
        &config,
        ApplicationDependencies {
            identity_provider: Arc::new(LocalUserProvider::new()),
            seed_development_users: false,
        },
    )
    .await?;
    let manager = application.connection_manager;
    let server = axum_test::TestServer::builder()
        .http_transport()
        .build(application.router)?;
    server
        .post("/auth/register")
        .json(&json!({"username": "socket-user", "password": "socket-password"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    let login: serde_json::Value = server
        .post("/auth/login")
        .json(&json!({"username": "socket-user", "password": "socket-password"}))
        .await
        .json();
    let token = login["token"].as_str().unwrap().to_string();

    // Drive the router through the generated client, the same one the TUI uses.
    let http_url = server.server_address().expect("http transport address");
    let ws_url = format!(
        "ws://{}:{}/ws",
        http_url.host_str().unwrap(),
        http_url.port().unwrap()
    );
    let client = ChatServiceClientBuilder::new(ws_url)
        .with_jwt_token(token)
        .build()
        .await?;

    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await?;
        client
            .join_room(JoinRoomRequest {
                room_name: "general".to_string(),
            })
            .await?;
        client
            .send_message(SendMessageRequest {
                text: "persisted through application router".to_string(),
            })
            .await?;
        let rooms = client.list_rooms(ListRoomsRequest {}).await?;
        assert!(rooms.rooms.iter().any(|room| room.room_id == "general"));
        client.disconnect().await?;
        while manager.connection_count() != 0 {
            tokio::task::yield_now().await;
        }
        anyhow::Ok(())
    })
    .await??;

    let messages = PersistenceManager::new(&config.chat.data_dir)
        .load_room_messages("general", None)
        .await?;
    assert!(
        messages
            .iter()
            .any(|message| message.text == "persisted through application router")
    );
    Ok(())
}
