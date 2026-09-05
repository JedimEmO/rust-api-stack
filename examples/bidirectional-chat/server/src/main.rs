//! Bidirectional chat server example.
use anyhow::Result;
use bidirectional_chat_server::{ApplicationDependencies, build_application, config::Config};
use ras_identity_local::LocalUserProvider;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables first (before config loading)
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("No .env file found or error loading: {}", e);
    }

    // Load configuration
    let config = Config::load().map_err(|e| {
        eprintln!("Failed to load configuration: {}", e);
        e
    })?;

    // Initialize tracing based on configuration
    use tracing_subscriber::{EnvFilter, fmt};

    let subscriber = fmt::Subscriber::builder()
        .with_env_filter(EnvFilter::new(config.log_filter()))
        .with_target(config.logging.target)
        .with_thread_ids(config.logging.thread_ids)
        .with_line_number(config.logging.line_numbers)
        .with_level(true)
        .with_ansi(true);

    // Apply format settings
    match config.logging.format.as_str() {
        "json" => {
            subscriber.with_ansi(false).init();
        }
        "compact" => {
            subscriber.compact().init();
        }
        _ => {
            // "pretty" or default
            subscriber.pretty().init();
        }
    }

    info!("Starting bidirectional chat server");
    info!("Configuration loaded from environment and config file");

    let application = build_application(
        &config,
        ApplicationDependencies {
            identity_provider: Arc::new(LocalUserProvider::new()),
            seed_development_users: cfg!(debug_assertions),
        },
    )
    .await?;
    let app = application.router;

    // Start server
    let addr = config.socket_addr();

    info!("Chat server listening on http://{}", addr);
    info!("WebSocket endpoint: ws://{}/ws", addr);
    info!("Health check endpoint: http://{}/health", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        error!("Failed to bind to address {}: {}", addr, e);
        e
    })?;

    info!("Server started successfully, ready to accept connections");

    axum::serve(listener, app).await.map_err(|e| {
        error!("Server error: {}", e);
        e
    })?;

    Ok(())
}
