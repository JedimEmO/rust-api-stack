use super::MessageSender;
use crate::{BidirectionalError, BidirectionalMessage, ConnectionId, Result};
use async_trait::async_trait;
use futures::sink::SinkExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMessage;

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
