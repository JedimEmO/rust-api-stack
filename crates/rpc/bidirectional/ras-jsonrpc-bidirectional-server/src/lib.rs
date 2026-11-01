//! The Axum WebSocket server for bidirectional JSON-RPC: clients call methods on the server, and
//! the server sends notifications and calls back over the same socket.
//!
//! Most services use this crate through the code that `ras-jsonrpc-bidirectional-macro`
//! generates, which implements [`MessageHandler`] and builds the service. Used by hand, the main
//! items are:
//!
//! - [`MessageRouter`], a [`MessageHandler`] that maps method names to closures;
//! - [`WebSocketServiceBuilder`], which turns a handler and an [`AuthProvider`] into a service,
//!   with limits on message size, connections and subscriptions, keepalive, and periodic
//!   re-validation of the connection's token;
//! - [`service::websocket_handler`], the Axum handler that serves a [`WebSocketService`];
//! - [`DefaultConnectionManager`], which tracks connections and topic subscriptions and sends or
//!   broadcasts to them;
//! - [`ServerError`], whose [`ServerError::Rpc`] variant carries an error the client may see.
//!
//! The connection is authenticated during the upgrade, from an `Authorization: Bearer` header, a
//! `token.<jwt>` WebSocket subprotocol for browsers, or an `X-Auth-Token` header. A JSON-RPC
//! request without an `id` is a notification and never gets a response.
//!
//! The crate has no Cargo features. See the [README] and the [macro guide].
//!
//! [README]: https://github.com/JedimEmO/rust-api-stack/blob/master/crates/rpc/bidirectional/ras-jsonrpc-bidirectional-server/README.md
//! [macro guide]: https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/macros/bidirectional-jsonrpc-service.md

pub mod connection;
pub mod error;
pub mod handler;
pub mod manager;
pub mod router;
pub mod service;
mod subscriptions;
pub mod upgrade;

pub use connection::{
    ChannelMessageSender, ConnectionContext, OutboundMessage, SubscriptionPolicy,
};
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
