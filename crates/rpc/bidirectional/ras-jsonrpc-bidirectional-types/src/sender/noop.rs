use super::MessageSender;
use crate::{BidirectionalMessage, ConnectionId, Result};
use async_trait::async_trait;

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
