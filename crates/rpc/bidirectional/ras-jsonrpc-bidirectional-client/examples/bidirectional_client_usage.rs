//! Basic usage example for the bidirectional JSON-RPC client
//!
//! This example demonstrates how to create a client, connect to a server,
//! make JSON-RPC calls, and handle notifications.

use ras_jsonrpc_bidirectional_client::{ClientBuilder, ConnectionEvent};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for better debugging
    tracing_subscriber::fmt::init();

    println!("Creating bidirectional JSON-RPC client...");

    let url = std::env::var("BIDIRECTIONAL_CLIENT_URL")
        .unwrap_or_else(|_| "ws://localhost:8080/ws".to_string());
    let token =
        std::env::var("BIDIRECTIONAL_CLIENT_TOKEN").unwrap_or_else(|_| "demo-token".to_string());

    // Create a client with configuration
    let client = ClientBuilder::new(url.clone())
        .with_jwt_token(token)
        .with_jwt_in_header(true) // Send JWT in Authorization header
        .with_header("User-Agent", "RasClient/1.0")
        .with_request_timeout(Duration::from_secs(30))
        .with_connection_timeout(Duration::from_secs(10))
        .with_heartbeat_interval(Some(Duration::from_secs(30)))
        .with_auto_connect(false) // Connect manually for this example
        .build()
        .await?;

    println!("Client created successfully!");

    // Register connection event handlers
    client.on_connection_event(
        "main",
        Arc::new(|event| match event {
            ConnectionEvent::Connected { connection_id } => {
                println!("Connected to server with ID: {}", connection_id);
            }
            ConnectionEvent::Disconnected { reason } => {
                println!("Disconnected from server. Reason: {:?}", reason);
            }
            _ => {}
        }),
    );

    // Register notification handlers
    client.on_notification(
        "user_message",
        Arc::new(|method, params| {
            println!("Received notification '{}': {:?}", method, params);
        }),
    );

    client.on_notification(
        "system_alert",
        Arc::new(|method, params| {
            println!("System alert '{}': {:?}", method, params);
        }),
    );

    // Connect to the server
    println!("Connecting to WebSocket server...");
    match client.connect().await {
        Ok(()) => {
            println!("Connected successfully!");
        }
        Err(e) => {
            println!("Failed to connect: {}", e);
            println!("Make sure a WebSocket server is running on {}", url);
            println!("   You can use the bidirectional server example or any compatible server.");
            return Ok(());
        }
    }

    // Subscribe to some topics
    println!("Subscribing to topics...");
    if let Err(e) = client
        .subscribe(
            "chat_room_general",
            Arc::new(|method, params| {
                println!("Chat message: {} - {:?}", method, params);
            }),
        )
        .await
    {
        println!("Failed to subscribe to chat_room_general: {}", e);
    }

    if let Err(e) = client
        .subscribe(
            "user_updates",
            Arc::new(|method, params| {
                println!("User update: {} - {:?}", method, params);
            }),
        )
        .await
    {
        println!("Failed to subscribe to user_updates: {}", e);
    }

    // Make some JSON-RPC calls
    println!("Making JSON-RPC calls...");

    // Call 1: Get server info
    match client.call("get_server_info", None).await {
        Ok(response) => {
            println!("Server info response: {:?}", response);
        }
        Err(e) => {
            println!("Failed to get server info: {}", e);
        }
    }

    // Call 2: Get user profile
    match client
        .call(
            "get_user_profile",
            Some(json!({
                "user_id": 123,
                "include_preferences": true
            })),
        )
        .await
    {
        Ok(response) => {
            println!("User profile response: {:?}", response);
        }
        Err(e) => {
            println!("Failed to get user profile: {}", e);
        }
    }

    // Call 3: Update user status (with error handling)
    match client
        .call(
            "update_user_status",
            Some(json!({
                "status": "online",
                "message": "Working on Rust projects"
            })),
        )
        .await
    {
        Ok(response) => {
            println!("Status update response: {:?}", response);
        }
        Err(e) => {
            println!("Failed to update status: {}", e);
        }
    }

    // Send some notifications (fire-and-forget)
    println!("Sending notifications...");

    if let Err(e) = client
        .notify(
            "user_activity",
            Some(json!({
                "action": "example_run",
                "timestamp": chrono::Utc::now().to_rfc3339()
            })),
        )
        .await
    {
        println!("Failed to send user_activity notification: {}", e);
    }

    if let Err(e) = client
        .notify("heartbeat", Some(json!({"client": "ras-client"})))
        .await
    {
        println!("Failed to send heartbeat notification: {}", e);
    }

    // Wait a bit to receive any server notifications
    println!("Waiting for server notifications (5 seconds)...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Display client statistics
    println!("\nClient Statistics:");
    println!("  Connection state: {:?}", client.state().await);
    println!("  Connection ID: {:?}", client.connection_id().await);
    println!("  Pending requests: {}", client.pending_requests_count());
    println!(
        "  Active subscriptions: {:?}",
        client.active_subscriptions()
    );

    // Cleanup expired requests (if any)
    client.cleanup_expired_requests().await;

    // Unsubscribe from one topic
    println!("Unsubscribing from chat_room_general...");
    if let Err(e) = client.unsubscribe("chat_room_general").await {
        println!("Failed to unsubscribe: {}", e);
    }

    // Wait a bit more
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Disconnect gracefully
    println!("Disconnecting from server...");
    if let Err(e) = client.disconnect().await {
        println!("Error during disconnect: {}", e);
    } else {
        println!("Disconnected successfully!");
    }

    println!("\nExample completed.");
    println!("To see this example working with a real server:");
    println!(
        "   1. Run a compatible bidirectional JSON-RPC server on {}",
        url
    );
    println!(
        "   2. Set BIDIRECTIONAL_CLIENT_URL and BIDIRECTIONAL_CLIENT_TOKEN if your server uses different values"
    );
    println!(
        "   3. Run this example again with cargo run -p ras-jsonrpc-bidirectional-client --example bidirectional_client_usage --locked"
    );

    Ok(())
}
