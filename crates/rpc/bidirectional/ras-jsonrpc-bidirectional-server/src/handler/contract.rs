//! Service callbacks and subscription authorization contract.

use crate::{ConnectionContext, ServerResult};
use async_trait::async_trait;
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Trait for handling JSON-RPC requests within a WebSocket context
#[async_trait]
pub trait MessageHandler: Send + Sync + 'static {
    /// Handle an incoming JSON-RPC request
    ///
    /// # Arguments
    /// * `request` - The JSON-RPC request to handle
    /// * `context` - The connection context containing auth info and metadata
    ///
    /// # Returns
    /// * `Ok(Some(response))` - Response to send back to client
    /// * `Ok(None)` - No response needed (for notifications)
    /// * `Err(error)` - Error occurred during handling
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>>;

    /// Decide whether this connection may subscribe to `topic`.
    ///
    /// Default-deny: services that broadcast over topics must override this
    /// (or `handle_subscribe`) to allow the topics a connection is entitled
    /// to. Errors propagate to the handler loop and close the connection.
    async fn authorize_subscribe(
        &self,
        _topic: &str,
        _context: &Arc<ConnectionContext>,
    ) -> ServerResult<bool> {
        Ok(false)
    }

    /// Handle subscription requests
    async fn handle_subscribe(
        &self,
        topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        // Default implementation subscribes the connection to each topic the
        // service authorizes via `authorize_subscribe`; denied topics are
        // skipped without closing the connection.
        for topic in topics {
            if self.authorize_subscribe(&topic, &context).await? {
                if let Err(e) = context.subscribe(topic.clone()).await {
                    warn!(
                        "Refused subscription to topic '{}' for connection {}: {}",
                        topic, context.id, e
                    );
                }
            } else {
                warn!(
                    "Denied subscription to topic '{}' for connection {}",
                    topic, context.id
                );
            }
        }
        Ok(())
    }

    /// Handle unsubscription requests
    async fn handle_unsubscribe(
        &self,
        topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        for topic in topics {
            context.unsubscribe(&topic).await;
        }
        Ok(())
    }

    /// Handle connection established event
    async fn on_connect(&self, context: Arc<ConnectionContext>) -> ServerResult<()> {
        info!("Connection established: {}", context.id);
        Ok(())
    }

    /// Handle connection closed event
    async fn on_disconnect(
        &self,
        context: Arc<ConnectionContext>,
        reason: Option<String>,
    ) -> ServerResult<()> {
        info!("Connection closed: {} (reason: {:?})", context.id, reason);
        Ok(())
    }

    /// Handle ping message
    async fn on_ping(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        debug!("Received ping");
        Ok(())
    }

    /// Handle pong message
    async fn on_pong(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        debug!("Received pong");
        Ok(())
    }
}
