//! Message sender contracts for bidirectional JSON-RPC.
use crate::{BidirectionalMessage, ConnectionId, Result};
use async_trait::async_trait;
mod noop;
pub use noop::NoOpMessageSender;
#[cfg(not(target_arch = "wasm32"))]
mod websocket;
#[cfg(not(target_arch = "wasm32"))]
pub use websocket::WebSocketMessageSender;

/// Trait for sending messages over WebSocket connections
#[async_trait]
pub trait MessageSender: Send + Sync {
    /// Send a message to a WebSocket connection
    async fn send_message(&self, message: BidirectionalMessage) -> Result<()>;

    /// Close the connection
    async fn close(&self) -> Result<()>;

    /// Check if the connection is still open
    async fn is_connected(&self) -> bool;

    /// Get the connection ID
    fn connection_id(&self) -> ConnectionId;
}

/// Extension trait for message senders with convenience methods
#[async_trait]
pub trait MessageSenderExt: MessageSender {
    /// Send a JSON-RPC request
    async fn send_request(&self, request: ras_jsonrpc_types::JsonRpcRequest) -> Result<()> {
        self.send_message(BidirectionalMessage::Request(request))
            .await
    }

    /// Send a JSON-RPC response
    async fn send_response(&self, response: ras_jsonrpc_types::JsonRpcResponse) -> Result<()> {
        self.send_message(BidirectionalMessage::Response(response))
            .await
    }

    /// Send a server notification
    async fn send_notification(&self, method: &str, params: serde_json::Value) -> Result<()> {
        let notification = crate::ServerNotification {
            method: method.to_string(),
            params,
            metadata: None,
        };
        self.send_message(BidirectionalMessage::ServerNotification(notification))
            .await
    }

    /// Send a ping message
    async fn send_ping(&self) -> Result<()> {
        self.send_message(BidirectionalMessage::Ping).await
    }

    /// Send a pong message
    async fn send_pong(&self) -> Result<()> {
        self.send_message(BidirectionalMessage::Pong).await
    }

    /// Send a subscription confirmation
    async fn send_subscription_update(&self, topics: Vec<String>, subscribed: bool) -> Result<()> {
        let message = if subscribed {
            BidirectionalMessage::Subscribe { topics }
        } else {
            BidirectionalMessage::Unsubscribe { topics }
        };
        self.send_message(message).await
    }
}

// Blanket implementation for all MessageSender types
impl<T: MessageSender> MessageSenderExt for T {}

#[cfg(test)]
mod tests;
