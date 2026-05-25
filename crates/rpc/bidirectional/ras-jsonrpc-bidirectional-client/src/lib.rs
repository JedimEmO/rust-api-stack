//! Cross-platform WebSocket client for bidirectional JSON-RPC communication
//!
//! This crate provides a unified client interface for bidirectional JSON-RPC communication
//! over WebSockets that works on both native and WASM targets. It supports:
//!
//! - JWT authentication via headers or connection params
//! - Sending JSON-RPC requests and receiving responses
//! - Receiving server notifications with registered handlers
//! - Connection lifecycle management (connect, disconnect, status)
//! - Subscription management
//! - Builder pattern for client configuration
//!
//! # Platform Support
//!
//! - **Native**: Uses `tokio-tungstenite` for WebSocket communication
//! - **WASM**: Uses `web-sys` WebSocket API for browser compatibility
//!
//! # Examples
//!
//! ```rust,no_run
//! use ras_jsonrpc_bidirectional_client::ClientBuilder;
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = ClientBuilder::new("ws://localhost:8080/ws")
//!         .with_jwt_token("demo-token".to_string())
//!         .build()
//!         .await?;
//!
//!     // Make a JSON-RPC call
//!     let response = client.call("get_user_info", Some(json!({"user_id": 123}))).await?;
//!     println!("Response: {:?}", response);
//!
//!     Ok(())
//! }
//! ```

use async_trait::async_trait;
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionId};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::sync::Arc;

pub mod client;
pub mod config;
pub mod error;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use client::{Client, ClientBuilder};
pub use config::{ClientConfig, ReconnectConfig};
pub use error::ClientError;

/// Type alias for notification handlers
pub type NotificationHandler = Arc<dyn Fn(&str, &Value) + Send + Sync>;

/// Type alias for RPC request handlers (server-to-client RPC calls)
pub type RpcRequestHandler = Arc<
    dyn Fn(
            JsonRpcRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = ras_jsonrpc_types::JsonRpcResponse> + Send>,
        > + Send
        + Sync,
>;

/// Type alias for connection event handlers
pub type ConnectionEventHandler = Arc<dyn Fn(ConnectionEvent) + Send + Sync>;

/// Connection lifecycle events
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// Emitted after the server sends a connection-established message.
    Connected { connection_id: ConnectionId },
    /// Emitted when the server closes the connection or `Client::disconnect` completes.
    Disconnected { reason: Option<String> },
    /// Reserved for caller-managed reconnect orchestration.
    ///
    /// The current client does not spawn a background reconnect loop.
    Reconnecting { attempt: u32 },
    /// Reserved for caller-managed reconnect orchestration.
    ///
    /// The current client does not spawn a background reconnect loop.
    ReconnectFailed { attempt: u32, error: String },
    /// Reserved for transports or wrappers that surface authentication failures as events.
    AuthenticationFailed { error: String },
}

/// Trait for WebSocket transport implementations
#[cfg(not(target_arch = "wasm32"))]
pub trait TransportThreadBounds: Send + Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> TransportThreadBounds for T {}

#[cfg(target_arch = "wasm32")]
pub trait TransportThreadBounds {}

#[cfg(target_arch = "wasm32")]
impl<T> TransportThreadBounds for T {}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait WebSocketTransport: TransportThreadBounds {
    /// Connect to the WebSocket server
    async fn connect(&mut self) -> error::ClientResult<()>;

    /// Disconnect from the WebSocket server
    async fn disconnect(&mut self) -> error::ClientResult<()>;

    /// Send a message to the server
    async fn send(&mut self, message: &BidirectionalMessage) -> error::ClientResult<()>;

    /// Receive the next message from the server
    async fn receive(&mut self) -> error::ClientResult<Option<BidirectionalMessage>>;

    /// Check if the connection is currently active
    fn is_connected(&self) -> bool;

    /// Get the connection URL
    fn url(&self) -> &str;
}

/// Pending request waiting for a response
#[derive(Debug)]
pub struct PendingRequest {
    pub id: Value,
    pub sender: tokio::sync::oneshot::Sender<JsonRpcResponse>,
    pub created_at: std::time::Instant,
}

/// Request timeout configuration
#[derive(Debug, Clone)]
pub struct RequestTimeout {
    pub duration: std::time::Duration,
}

impl Default for RequestTimeout {
    fn default() -> Self {
        Self {
            duration: std::time::Duration::from_secs(30),
        }
    }
}

/// Client state tracking
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Subscription tracking
#[derive(Clone)]
pub struct Subscription {
    pub topic: String,
    pub handler: NotificationHandler,
    pub created_at: std::time::Instant,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("topic", &self.topic)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_event_debug() {
        let event = ConnectionEvent::Connected {
            connection_id: ConnectionId::new(),
        };
        assert!(format!("{:?}", event).contains("Connected"));
    }

    #[test]
    fn test_client_state() {
        assert_eq!(ClientState::Disconnected, ClientState::Disconnected);
        assert_ne!(ClientState::Connected, ClientState::Disconnected);
    }

    #[test]
    fn test_request_timeout_default() {
        let timeout = RequestTimeout::default();
        assert_eq!(timeout.duration.as_secs(), 30);
    }
}
