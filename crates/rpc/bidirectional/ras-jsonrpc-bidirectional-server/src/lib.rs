//! Server-side WebSocket handling for bidirectional JSON-RPC communication
//!
//! This crate provides the server infrastructure for handling WebSocket connections
//! with JSON-RPC message routing, authentication, and connection management.

pub mod connection;
pub mod error;
pub mod handler;
pub mod manager;
pub mod router;
pub mod service;
pub mod upgrade;

pub use connection::{ChannelMessageSender, ConnectionContext, OutboundMessage};
pub use error::{ServerError, ServerResult};
pub use handler::{
    AuthRevalidation, KeepaliveConfig, MessageHandler, PermissionChangePolicy,
    SubscriptionAccounting, SubscriptionLimits, WebSocketHandler,
};
pub use manager::DefaultConnectionManager;
pub use router::MessageRouter;
pub use service::{WebSocketService, WebSocketServiceBuilder};
pub use upgrade::{WS_SUBPROTOCOL, WS_TOKEN_SUBPROTOCOL_PREFIX, WebSocketUpgrade};

// Re-export types from bidirectional-types for convenience
pub use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, BroadcastMessage, ConnectionId, ConnectionInfo, MessageSender,
    ServerMessage, ServerNotification,
};

// Re-export auth types for convenience
pub use ras_auth_core::{AuthError, AuthProvider, AuthenticatedUser};

// Re-export JSON-RPC types
pub use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
