use crate::ConnectionId;
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};

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
