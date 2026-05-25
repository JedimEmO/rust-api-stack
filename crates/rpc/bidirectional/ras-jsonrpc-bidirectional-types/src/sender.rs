//! Message sender trait for bidirectional JSON-RPC

#[cfg(not(target_arch = "wasm32"))]
use crate::BidirectionalError;
use crate::{BidirectionalMessage, ConnectionId, Result};
use async_trait::async_trait;
#[cfg(not(target_arch = "wasm32"))]
use futures::sink::SinkExt;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::Message as WsMessage;

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

/// A message sender implementation using tokio-tungstenite
#[cfg(not(target_arch = "wasm32"))]
pub struct WebSocketMessageSender<S>
where
    S: SinkExt<WsMessage> + Send + Unpin,
{
    connection_id: ConnectionId,
    sink: Arc<Mutex<S>>,
    is_closed: Arc<Mutex<bool>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<S> WebSocketMessageSender<S>
where
    S: SinkExt<WsMessage> + Send + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    /// Create a new WebSocket message sender
    pub fn new(connection_id: ConnectionId, sink: S) -> Self {
        Self {
            connection_id,
            sink: Arc::new(Mutex::new(sink)),
            is_closed: Arc::new(Mutex::new(false)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<S> MessageSender for WebSocketMessageSender<S>
where
    S: SinkExt<WsMessage> + Send + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    async fn send_message(&self, message: BidirectionalMessage) -> Result<()> {
        if self.is_connected().await {
            let json = serde_json::to_string(&message)?;
            let ws_message = WsMessage::Text(json.into());

            let mut sink = self.sink.lock().await;
            sink.send(ws_message)
                .await
                .map_err(|e| BidirectionalError::SendError(e.to_string()))?;

            Ok(())
        } else {
            Err(BidirectionalError::ConnectionClosed)
        }
    }

    async fn close(&self) -> Result<()> {
        let mut is_closed = self.is_closed.lock().await;
        if !*is_closed {
            *is_closed = true;

            let mut sink = self.sink.lock().await;
            sink.send(WsMessage::Close(None))
                .await
                .map_err(|e| BidirectionalError::SendError(e.to_string()))?;
        }
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        !*self.is_closed.lock().await
    }

    fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }
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

/// A no-operation message sender that does nothing
pub struct NoOpMessageSender {
    connection_id: ConnectionId,
}

impl NoOpMessageSender {
    /// Create a new no-op message sender
    pub fn new() -> Self {
        Self {
            connection_id: ConnectionId::new(),
        }
    }

    /// Create a new no-op message sender with a specific connection ID
    pub fn with_connection_id(connection_id: ConnectionId) -> Self {
        Self { connection_id }
    }
}

impl Default for NoOpMessageSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessageSender for NoOpMessageSender {
    async fn send_message(&self, _message: BidirectionalMessage) -> Result<()> {
        // No-op senders acknowledge messages without producing side effects.
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        // Closing a no-op sender has no external state to update.
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        // Always report as connected for testing purposes
        true
    }

    fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_message_sender_ext() {
        // Create a mock sender
        struct MockSender {
            connection_id: ConnectionId,
            sent_messages: Arc<Mutex<Vec<BidirectionalMessage>>>,
        }

        #[async_trait]
        impl MessageSender for MockSender {
            async fn send_message(&self, message: BidirectionalMessage) -> Result<()> {
                self.sent_messages.lock().await.push(message);
                Ok(())
            }

            async fn close(&self) -> Result<()> {
                Ok(())
            }

            async fn is_connected(&self) -> bool {
                true
            }

            fn connection_id(&self) -> ConnectionId {
                self.connection_id
            }
        }

        let sender = MockSender {
            connection_id: ConnectionId::new(),
            sent_messages: Arc::new(Mutex::new(Vec::new())),
        };

        // Test convenience methods
        sender.send_ping().await.unwrap();
        sender.send_pong().await.unwrap();
        sender
            .send_notification("test.method", serde_json::json!({"key": "value"}))
            .await
            .unwrap();

        let messages = sender.sent_messages.lock().await;
        assert_eq!(messages.len(), 3);

        // Check message types
        assert!(matches!(messages[0], BidirectionalMessage::Ping));
        assert!(matches!(messages[1], BidirectionalMessage::Pong));
        assert!(matches!(
            &messages[2],
            BidirectionalMessage::ServerNotification(n) if n.method == "test.method"
        ));
    }

    #[tokio::test]
    async fn message_sender_ext_request_response_subscription() {
        struct Recorder {
            id: ConnectionId,
            sent: Arc<Mutex<Vec<BidirectionalMessage>>>,
        }
        #[async_trait]
        impl MessageSender for Recorder {
            async fn send_message(&self, message: BidirectionalMessage) -> Result<()> {
                self.sent.lock().await.push(message);
                Ok(())
            }
            async fn close(&self) -> Result<()> {
                Ok(())
            }
            async fn is_connected(&self) -> bool {
                true
            }
            fn connection_id(&self) -> ConnectionId {
                self.id
            }
        }
        let r = Recorder {
            id: ConnectionId::new(),
            sent: Arc::new(Mutex::new(Vec::new())),
        };

        r.send_request(ras_jsonrpc_types::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "m".into(),
            params: None,
            id: Some(serde_json::json!(1)),
        })
        .await
        .unwrap();
        r.send_response(ras_jsonrpc_types::JsonRpcResponse::success(
            serde_json::json!("ok"),
            Some(serde_json::json!(1)),
        ))
        .await
        .unwrap();
        r.send_subscription_update(vec!["t1".into()], true)
            .await
            .unwrap();
        r.send_subscription_update(vec!["t1".into()], false)
            .await
            .unwrap();

        let s = r.sent.lock().await;
        assert!(matches!(s[0], BidirectionalMessage::Request(_)));
        assert!(matches!(s[1], BidirectionalMessage::Response(_)));
        assert!(matches!(s[2], BidirectionalMessage::Subscribe { .. }));
        assert!(matches!(s[3], BidirectionalMessage::Unsubscribe { .. }));
    }

    #[tokio::test]
    async fn noop_message_sender_round_trip() {
        let id = ConnectionId::new();
        let sender = NoOpMessageSender::with_connection_id(id);
        assert_eq!(sender.connection_id(), id);
        assert!(sender.is_connected().await);
        sender
            .send_message(BidirectionalMessage::Ping)
            .await
            .unwrap();
        sender.close().await.unwrap();

        // Default constructor + Default impl.
        let s2 = NoOpMessageSender::new();
        let s3 = NoOpMessageSender::default();
        assert_ne!(s2.connection_id(), s3.connection_id());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn websocket_sender_drives_real_sink() {
        use futures::channel::mpsc;
        use futures::stream::StreamExt;

        // mpsc::channel's Sender impls Sink<T>, satisfying the SinkExt bound
        // on `WebSocketMessageSender::new`.
        let (tx, mut rx) = mpsc::channel::<WsMessage>(8);
        let id = ConnectionId::new();
        let sender = WebSocketMessageSender::new(id, tx);

        assert_eq!(sender.connection_id(), id);
        assert!(sender.is_connected().await);

        sender
            .send_message(BidirectionalMessage::Ping)
            .await
            .unwrap();
        // close once → emits a Close frame and flips is_closed.
        sender.close().await.unwrap();
        assert!(!sender.is_connected().await);
        // close again is idempotent (no panic, no extra send).
        sender.close().await.unwrap();

        // Sending after close yields ConnectionClosed.
        let err = sender
            .send_message(BidirectionalMessage::Pong)
            .await
            .unwrap_err();
        assert!(matches!(err, BidirectionalError::ConnectionClosed));

        // Drain what we actually pushed: a Text(Ping) and a Close.
        let mut received: Vec<WsMessage> = Vec::new();
        while let Some(m) = rx.next().await {
            received.push(m);
            if received.len() == 2 {
                break;
            }
        }
        assert_eq!(received.len(), 2);
        match &received[0] {
            WsMessage::Text(t) => assert!(t.contains("ping")),
            other => panic!("expected Text(ping), got {other:?}"),
        }
        assert!(matches!(received[1], WsMessage::Close(_)));
    }
}
