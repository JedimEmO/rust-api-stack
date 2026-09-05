use crate::{
    auth::{AuthHandlers, AuthServiceImpl, ChatPermissions},
    chat::ChatServer,
    config::Config,
};
use anyhow::Result;
use axum::{Router, routing::get};
use bidirectional_chat_api::auth::ChatAuthServiceBuilder;
use ras_identity_local::LocalUserProvider;
use ras_identity_session::{JwtAlgorithm, JwtAuthProvider, SessionConfig, SessionService};
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager, WebSocketServiceBuilder,
    service::{BuiltWebSocketService, websocket_handler},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info};

pub struct ApplicationDependencies {
    pub identity_provider: Arc<LocalUserProvider>,
    pub seed_development_users: bool,
}

pub struct ChatApplication {
    pub router: Router,
    pub session_service: Arc<SessionService>,
    pub connection_manager: Arc<DefaultConnectionManager>,
}

/// Assemble the REST and WebSocket application with explicit configuration and identity storage.
pub async fn build_application(
    config: &Config,
    dependencies: ApplicationDependencies,
) -> Result<ChatApplication> {
    // Create identity provider - use Arc to share between session service and registration
    info!("Setting up identity provider");
    let identity_provider = dependencies.identity_provider;

    // Add admin users from configuration
    if config.admin.auto_create {
        for admin_user in &config.admin.users {
            match identity_provider
                .add_user(
                    admin_user.username.clone(),
                    admin_user.password.clone(),
                    admin_user.email.clone(),
                    admin_user.display_name.clone(),
                )
                .await
            {
                Ok(_) => info!("Created admin user: {}", admin_user.username),
                Err(e) => {
                    // User might already exist, which is fine
                    debug!(
                        "Admin user {} might already exist: {}",
                        admin_user.username, e
                    );
                }
            }
        }
    }

    // Add some default test users if in development mode
    if dependencies.seed_development_users {
        let test_users = vec![
            (
                "alice",
                "alice123",
                Some("alice@example.com"),
                Some("Alice"),
            ),
            ("bob", "bob123", Some("bob@example.com"), Some("Bob")),
        ];

        for (username, password, email, display_name) in test_users {
            match identity_provider
                .add_user(
                    username.to_string(),
                    password.to_string(),
                    email.map(|s| s.to_string()),
                    display_name.map(|s| s.to_string()),
                )
                .await
            {
                Ok(_) => debug!("Created test user: {}", username),
                Err(e) => debug!("Test user {} might already exist: {}", username, e),
            }
        }
    }

    // Create session service from configuration
    let session_config = SessionConfig {
        jwt_secret: config.auth.jwt_secret.clone(),
        jwt_ttl: chrono::Duration::seconds(config.auth.jwt_ttl_seconds),
        enforce_active_sessions: true,
        algorithm: JwtAlgorithm::from_name(&config.auth.jwt_algorithm)
            .unwrap_or(JwtAlgorithm::HS256),
        iss: Some("bidirectional-chat".to_string()),
        aud: Some("bidirectional-chat".to_string()),
        require_iss_aud: true,
        max_sessions_per_user: ras_identity_session::DEFAULT_MAX_SESSIONS_PER_USER,
    };
    info!(
        "Creating session service with JWT TTL: {} seconds",
        config.auth.jwt_ttl_seconds
    );
    let session_service = Arc::new(
        SessionService::new(session_config)
            .map_err(anyhow::Error::from)?
            .with_permissions(Arc::new(ChatPermissions::new(config.admin.users.clone()))),
    );

    // Register the identity provider with the session service
    // We need to dereference the Arc and clone the inner provider since register_provider takes Box
    session_service
        .register_provider(Box::new((*identity_provider).clone()))
        .await;

    // Create JWT auth provider
    let auth_provider = Arc::new(JwtAuthProvider::new(session_service.clone()));

    // Create connection manager
    let connection_manager = Arc::new(DefaultConnectionManager::new());

    // Create chat server with configuration
    let chat_server = Arc::new(
        ChatServer::new_with_rate_limit(config.chat.clone(), config.rate_limit.clone())
            .await
            .map_err(|e| {
                error!("Failed to create chat server: {}", e);
                e
            })?,
    );

    // Create handler with the service and connection manager
    let handler = Arc::new(
        bidirectional_chat_api::ChatServiceHandler::new(
            chat_server.clone(),
            connection_manager.clone(),
        )
        .with_auth_provider(auth_provider.clone()),
    );

    // Build WebSocket service
    let ws_service = WebSocketServiceBuilder::builder()
        .handler(handler)
        .auth_provider(auth_provider.clone())
        .require_auth(true)
        .build()
        .build_with_manager(connection_manager.clone());

    // Create auth handlers with the shared identity provider
    let auth_handlers = AuthHandlers {
        session_service: session_service.clone(),
        identity_provider: identity_provider.clone(),
    };

    // Build REST service using the macro-generated builder
    // Create auth service implementation
    let auth_service_impl = AuthServiceImpl {
        handlers: auth_handlers.clone(),
    };

    let auth_router = ChatAuthServiceBuilder::new(auth_service_impl)
        .auth_provider(auth_provider.as_ref().clone())
        .build();

    // Create WebSocket endpoint
    type ChatServiceType = BuiltWebSocketService<
        bidirectional_chat_api::ChatServiceHandler<ChatServer, DefaultConnectionManager>,
        JwtAuthProvider,
        DefaultConnectionManager,
    >;
    let ws_router = Router::new()
        .route("/ws", get(websocket_handler::<ChatServiceType>))
        .with_state(ws_service);

    // Configure CORS based on configuration
    let cors_layer = if config.server.cors.allow_any_origin {
        CorsLayer::permissive()
    } else {
        let mut cors = CorsLayer::new();
        for origin in &config.server.cors.allowed_origins {
            cors = cors.allow_origin(origin.parse::<axum::http::HeaderValue>().unwrap());
        }
        cors
    };

    // Combine all routers
    let app = Router::new()
        .merge(auth_router)
        .merge(ws_router)
        .layer(cors_layer);

    Ok(ChatApplication {
        router: app,
        session_service,
        connection_manager,
    })
}
