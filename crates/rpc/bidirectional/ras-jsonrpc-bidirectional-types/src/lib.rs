//! Core types for bidirectional JSON-RPC communication over WebSockets
//!
//! This crate provides the fundamental types and traits needed for bidirectional
//! JSON-RPC communication, including connection management, message routing,
//! and subscription handling.

use ras_auth_core::AuthenticatedUser;
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

pub mod error;
pub mod manager;
pub mod sender;

pub use error::BidirectionalError;
pub use manager::ConnectionManager;
#[cfg(not(target_arch = "wasm32"))]
pub use sender::WebSocketMessageSender;
pub use sender::{MessageSender, MessageSenderExt, NoOpMessageSender};

/// Unique identifier for a WebSocket connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Create a new random connection ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a connection ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Messages that can be sent bidirectionally between client and server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BidirectionalMessage {
    /// JSON-RPC request from either client or server
    Request(JsonRpcRequest),
    /// JSON-RPC response from either client or server
    Response(JsonRpcResponse),
    /// Server-initiated notification
    ServerNotification(ServerNotification),
    /// Broadcast message from server to multiple clients
    Broadcast(BroadcastMessage),
    /// Subscription management
    Subscribe {
        topics: Vec<String>,
    },
    Unsubscribe {
        topics: Vec<String>,
    },
    /// Connection lifecycle
    ConnectionEstablished {
        connection_id: ConnectionId,
    },
    ConnectionClosed {
        connection_id: ConnectionId,
        reason: Option<String>,
    },
    /// Heartbeat/keepalive
    Ping,
    Pong,
}

/// Server-initiated messages (not including broadcasts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMessage {
    /// The connection to send to
    pub connection_id: ConnectionId,
    /// The message to send
    pub message: BidirectionalMessage,
}

/// Server-initiated notification to specific client(s)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerNotification {
    /// Notification method name
    pub method: String,
    /// Notification parameters
    pub params: serde_json::Value,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

/// Broadcast message from server to multiple clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastMessage {
    /// Topic/channel for the broadcast
    pub topic: String,
    /// Broadcast method name
    pub method: String,
    /// Broadcast parameters
    pub params: serde_json::Value,
    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

/// Information about a connected client
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Unique connection identifier
    pub id: ConnectionId,
    /// Authenticated user information (if authenticated)
    pub user: Option<Arc<AuthenticatedUser>>,
    /// Topics this connection is subscribed to
    pub subscriptions: HashSet<String>,
    /// Connection metadata (e.g., user agent, IP address)
    pub metadata: serde_json::Value,
    /// When the connection was established
    pub connected_at: chrono::DateTime<chrono::Utc>,
}

impl ConnectionInfo {
    /// Create a new connection info
    pub fn new(id: ConnectionId) -> Self {
        Self {
            id,
            user: None,
            subscriptions: HashSet::new(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            connected_at: chrono::Utc::now(),
        }
    }

    /// Check if the connection is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.user.is_some()
    }

    /// Check if the connection has a specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.user
            .as_ref()
            .map(|u| u.permissions.contains(permission))
            .unwrap_or(false)
    }

    /// Check if the connection is subscribed to a topic
    pub fn is_subscribed_to(&self, topic: &str) -> bool {
        self.subscriptions.contains(topic)
    }

    /// Add a subscription
    pub fn subscribe(&mut self, topic: String) {
        self.subscriptions.insert(topic);
    }

    /// Remove a subscription
    pub fn unsubscribe(&mut self, topic: &str) -> bool {
        self.subscriptions.remove(topic)
    }

    /// Set authenticated user
    pub fn set_user(&mut self, user: AuthenticatedUser) {
        self.user = Some(Arc::new(user));
    }

    /// Clear authenticated user
    pub fn clear_user(&mut self) {
        self.user = None;
    }
}

/// Result type for bidirectional operations
pub type Result<T> = std::result::Result<T, BidirectionalError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id() {
        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        assert_ne!(id1, id2);

        let uuid = Uuid::new_v4();
        let id3 = ConnectionId::from_uuid(uuid);
        assert_eq!(id3.as_uuid(), &uuid);
    }

    #[test]
    fn test_connection_info() {
        let mut info = ConnectionInfo::new(ConnectionId::new());
        assert!(!info.is_authenticated());
        assert!(!info.has_permission("admin"));

        // Test subscriptions
        info.subscribe("topic1".to_string());
        info.subscribe("topic2".to_string());
        assert!(info.is_subscribed_to("topic1"));
        assert!(info.is_subscribed_to("topic2"));
        assert!(!info.is_subscribed_to("topic3"));

        assert!(info.unsubscribe("topic1"));
        assert!(!info.is_subscribed_to("topic1"));
        assert!(!info.unsubscribe("topic1")); // Already unsubscribed
    }

    #[test]
    fn test_message_serialization() {
        let msg = BidirectionalMessage::Ping;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"ping\""));

        let notification = ServerNotification {
            method: "test.notify".to_string(),
            params: serde_json::json!({"data": "test"}),
            metadata: None,
        };
        let msg = BidirectionalMessage::ServerNotification(notification);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: BidirectionalMessage = serde_json::from_str(&json).unwrap();

        if let BidirectionalMessage::ServerNotification(notif) = deserialized {
            assert_eq!(notif.method, "test.notify");
        } else {
            panic!("Expected ServerNotification");
        }
    }
}
