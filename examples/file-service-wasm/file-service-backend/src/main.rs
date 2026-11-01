use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod file_service;
mod simple_auth;
mod storage;

use file_service::FileServiceImpl;
use simple_auth::SimpleAuthProvider;
use storage::FileStorage;

#[tokio::main]
async fn main() -> Result<()> {
    // Load `.env` first, so a `RUST_LOG` set there reaches the tracing filter.
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "file_service_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Create storage directory
    let storage_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./uploads".to_string());
    tokio::fs::create_dir_all(&storage_path).await?;
    info!("Using storage path: {}", storage_path);

    // Initialize components
    let storage = Arc::new(FileStorage::new(storage_path));
    let auth_provider = SimpleAuthProvider;

    // Create service implementation
    let service = FileServiceImpl::new(storage.clone());

    // Build the router using the macro-generated builder
    let app = file_service_api::DocumentServiceBuilder::new(service)
        .auth_provider(auth_provider)
        .build()
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("File service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
