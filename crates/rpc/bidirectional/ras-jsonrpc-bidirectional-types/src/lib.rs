//! Core types for bidirectional JSON-RPC communication over WebSockets
//!
//! This crate provides the fundamental types and traits needed for bidirectional
//! JSON-RPC communication, including connection management, message routing,
//! and subscription handling.

pub mod error;
pub mod manager;
pub mod sender;

pub use error::BidirectionalError;
pub use manager::ConnectionManager;
#[cfg(not(target_arch = "wasm32"))]
pub use sender::WebSocketMessageSender;
pub use sender::{MessageSender, MessageSenderExt, NoOpMessageSender};

mod connection;
mod wire;
pub use connection::{ConnectionId, ConnectionInfo};
pub use wire::{BidirectionalMessage, BroadcastMessage, ServerMessage, ServerNotification};

/// Result type for bidirectional operations
pub type Result<T> = std::result::Result<T, BidirectionalError>;

#[cfg(test)]
mod tests;
