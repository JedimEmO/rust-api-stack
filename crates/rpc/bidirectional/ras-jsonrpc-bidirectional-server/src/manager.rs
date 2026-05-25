//! Default connection manager implementation using DashMap

use crate::connection::ChannelMessageSender;
use async_trait::async_trait;
use dashmap::DashMap;
use ras_auth_core::AuthenticatedUser;
use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, ConnectionId, ConnectionInfo, ConnectionManager, Result,
};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

/// Thread-safe connection manager using DashMap for concurrent access
#[derive(Debug, Default)]
pub struct DefaultConnectionManager {
    /// Active connections indexed by ConnectionId
    connections: DashMap<ConnectionId, (ConnectionInfo, ChannelMessageSender)>,

    /// Topic subscriptions - maps topic to set of connection IDs
    subscriptions: DashMap<String, Vec<ConnectionId>>,

    /// Pending requests for server-to-client RPC calls
    /// Maps connection_id -> request_id -> response_sender
    pending_requests: DashMap<
        ConnectionId,
        HashMap<serde_json::Value, oneshot::Sender<ras_jsonrpc_types::JsonRpcResponse>>,
    >,
}

impl DefaultConnectionManager {
    /// Create a new connection manager
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
            subscriptions: DashMap::new(),
            pending_requests: DashMap::new(),
        }
    }

    /// Get the number of active connections
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Get all connection IDs
    pub fn get_connection_ids(&self) -> Vec<ConnectionId> {
        self.connections.iter().map(|entry| *entry.key()).collect()
    }

    /// Get connections subscribed to a topic
    pub fn get_topic_connections(&self, topic: &str) -> Vec<ConnectionId> {
        self.subscriptions
            .get(topic)
            .map(|entry| entry.value().clone())
            .unwrap_or_default()
    }

    /// Get all active topics
    pub fn get_active_topics(&self) -> Vec<String> {
        self.subscriptions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Add a connection with its message sender for external management
    pub async fn add_connection_with_sender_direct(
        &self,
        info: ConnectionInfo,
        sender: ChannelMessageSender,
    ) -> Result<()> {
        self.connections.insert(info.id, (info.clone(), sender));
        info!("Added connection: {}", info.id);
        Ok(())
    }

    /// Get the message sender for a connection
    pub fn get_sender(&self, id: ConnectionId) -> Option<ChannelMessageSender> {
        self.connections.get(&id).map(|entry| entry.1.clone())
    }
}

#[async_trait]
impl ConnectionManager for DefaultConnectionManager {
    async fn add_connection(&self, info: ConnectionInfo) -> Result<()> {
        // Connections without an erased sender receive a closed internal channel.
        // Runtime transports should call add_connection_with_sender.
        let (tx, _rx) = mpsc::channel(1);
        let sender = ChannelMessageSender::new(info.id, tx);
        self.connections.insert(info.id, (info.clone(), sender));
        info!("Added connection: {}", info.id);
        Ok(())
    }

    async fn add_connection_with_sender(
        &self,
        info: ConnectionInfo,
        sender: Box<dyn std::any::Any + Send + Sync>,
    ) -> Result<()> {
        // Try to downcast to ChannelMessageSender
        if let Ok(channel_sender) = sender.downcast::<ChannelMessageSender>() {
            self.connections
                .insert(info.id, (info.clone(), *channel_sender));
            info!("Added connection with sender: {}", info.id);
            Ok(())
        } else {
            // Store the connection even when the erased sender has an unexpected type.
            self.add_connection(info).await
        }
    }

    async fn remove_connection(&self, id: ConnectionId) -> Result<()> {
        if let Some((_, (info, _))) = self.connections.remove(&id) {
            // Remove from all topic subscriptions
            for topic in info.subscriptions.iter() {
                if let Some(mut entry) = self.subscriptions.get_mut(topic) {
                    entry.retain(|&connection_id| connection_id != id);
                    if entry.is_empty() {
                        drop(entry);
                        self.subscriptions.remove(topic);
                    }
                }
            }

            // Clean up pending requests for this connection
            self.pending_requests.remove(&id);

            info!("Removed connection: {}", id);
        } else {
            warn!("Attempted to remove non-existent connection: {}", id);
        }

        Ok(())
    }

    async fn get_connection(&self, id: ConnectionId) -> Result<Option<ConnectionInfo>> {
        Ok(self.connections.get(&id).map(|entry| entry.0.clone()))
    }

    async fn get_all_connections(&self) -> Result<Vec<ConnectionInfo>> {
        Ok(self
            .connections
            .iter()
            .map(|entry| entry.value().0.clone())
            .collect())
    }

    async fn get_subscribed_connections(&self, topic: &str) -> Result<Vec<ConnectionInfo>> {
        let connection_ids = self.get_topic_connections(topic);
        let mut connections = Vec::new();

        for id in connection_ids {
            if let Some(entry) = self.connections.get(&id) {
                connections.push(entry.0.clone());
            }
        }

        Ok(connections)
    }

    async fn set_connection_user(&self, id: ConnectionId, user: AuthenticatedUser) -> Result<()> {
        if let Some(mut entry) = self.connections.get_mut(&id) {
            entry.0.set_user(user);
            debug!("Set user for connection: {}", id);
        } else {
            warn!("Attempted to set user for non-existent connection: {}", id);
        }
        Ok(())
    }

    async fn clear_connection_user(&self, id: ConnectionId) -> Result<()> {
        if let Some(mut entry) = self.connections.get_mut(&id) {
            entry.0.clear_user();
            debug!("Cleared user for connection: {}", id);
        } else {
            warn!(
                "Attempted to clear user for non-existent connection: {}",
                id
            );
        }
        Ok(())
    }

    async fn add_subscription(&self, id: ConnectionId, topic: String) -> Result<()> {
        // Update topic subscriptions
        self.subscriptions
            .entry(topic.clone())
            .or_default()
            .push(id);

        // Update connection subscriptions
        if let Some(mut entry) = self.connections.get_mut(&id) {
            entry.0.subscribe(topic.clone());
        }

        debug!("Connection {} subscribed to topic {}", id, topic);
        Ok(())
    }

    async fn remove_subscription(&self, id: ConnectionId, topic: &str) -> Result<()> {
        // Update topic subscriptions
        if let Some(mut entry) = self.subscriptions.get_mut(topic) {
            entry.retain(|&connection_id| connection_id != id);
            if entry.is_empty() {
                drop(entry);
                self.subscriptions.remove(topic);
            }
        }

        // Update connection subscriptions
        if let Some(mut entry) = self.connections.get_mut(&id) {
            entry.0.unsubscribe(topic);
        }

        debug!("Connection {} unsubscribed from topic {}", id, topic);
        Ok(())
    }

    async fn get_subscriptions(&self, id: ConnectionId) -> Result<Vec<String>> {
        if let Some(entry) = self.connections.get(&id) {
            Ok(entry.0.subscriptions.iter().cloned().collect())
        } else {
            Ok(Vec::new())
        }
    }

    async fn send_to_connection(
        &self,
        id: ConnectionId,
        message: BidirectionalMessage,
    ) -> Result<()> {
        if let Some(entry) = self.connections.get(&id) {
            entry
                .1
                .send(message)
                .await
                .map_err(ras_jsonrpc_bidirectional_types::BidirectionalError::SendError)?;
        } else {
            warn!("Attempted to send to non-existent connection: {}", id);
        }
        Ok(())
    }

    async fn broadcast_to_topic(
        &self,
        topic: &str,
        message: BidirectionalMessage,
    ) -> Result<usize> {
        let topic_connections = self.get_topic_connections(topic);

        if topic_connections.is_empty() {
            debug!("No connections subscribed to topic: {}", topic);
            return Ok(0);
        }

        let mut failed_connections = Vec::new();
        let mut sent_count = 0;

        for connection_id in &topic_connections {
            if let Some(entry) = self.connections.get(connection_id) {
                if let Err(e) = entry.1.send(message.clone()).await {
                    warn!("Failed to broadcast to connection {}: {}", connection_id, e);
                    failed_connections.push(*connection_id);
                } else {
                    sent_count += 1;
                }
            } else {
                failed_connections.push(*connection_id);
            }
        }

        // Clean up failed connections from topic subscriptions
        if !failed_connections.is_empty() {
            for connection_id in failed_connections {
                let _ = self.remove_subscription(connection_id, topic).await;
            }
        }

        debug!(
            "Broadcasted to {} connections on topic: {}",
            sent_count, topic
        );
        Ok(sent_count)
    }

    async fn broadcast_to_authenticated(&self, message: BidirectionalMessage) -> Result<usize> {
        let mut sent_count = 0;

        for entry in self.connections.iter() {
            let (info, sender) = entry.value();
            if info.is_authenticated() {
                if let Err(e) = sender.send(message.clone()).await {
                    warn!(
                        "Failed to broadcast to authenticated connection {}: {}",
                        info.id, e
                    );
                } else {
                    sent_count += 1;
                }
            }
        }

        debug!("Broadcasted to {} authenticated connections", sent_count);
        Ok(sent_count)
    }

    async fn broadcast_to_permission(
        &self,
        permission: &str,
        message: BidirectionalMessage,
    ) -> Result<usize> {
        let mut sent_count = 0;

        for entry in self.connections.iter() {
            let (info, sender) = entry.value();
            if info.has_permission(permission) {
                if let Err(e) = sender.send(message.clone()).await {
                    warn!(
                        "Failed to broadcast to connection {} with permission {}: {}",
                        info.id, permission, e
                    );
                } else {
                    sent_count += 1;
                }
            }
        }

        debug!(
            "Broadcasted to {} connections with permission: {}",
            sent_count, permission
        );
        Ok(sent_count)
    }

    async fn register_pending_request(
        &self,
        connection_id: ConnectionId,
        request_id: serde_json::Value,
        response_sender: oneshot::Sender<ras_jsonrpc_types::JsonRpcResponse>,
    ) -> Result<()> {
        self.pending_requests
            .entry(connection_id)
            .or_default()
            .insert(request_id, response_sender);

        debug!(
            "Registered pending request for connection: {}",
            connection_id
        );
        Ok(())
    }

    async fn remove_pending_request(
        &self,
        connection_id: ConnectionId,
        request_id: &serde_json::Value,
    ) -> Result<Option<oneshot::Sender<ras_jsonrpc_types::JsonRpcResponse>>> {
        if let Some(mut entry) = self.pending_requests.get_mut(&connection_id) {
            let sender = entry.remove(request_id);
            if entry.is_empty() {
                drop(entry);
                self.pending_requests.remove(&connection_id);
            }
            debug!("Removed pending request for connection: {}", connection_id);
            Ok(sender)
        } else {
            Ok(None)
        }
    }

    async fn handle_pending_response(
        &self,
        connection_id: ConnectionId,
        response: ras_jsonrpc_types::JsonRpcResponse,
    ) -> Result<bool> {
        if let Some(request_id) = &response.id
            && let Some(sender) = self
                .remove_pending_request(connection_id, request_id)
                .await?
        {
            if sender.send(response).is_err() {
                warn!("Failed to send response to pending request - receiver dropped");
            }
            debug!("Handled pending response for connection: {}", connection_id);
            return Ok(true);
        }
        Ok(false)
    }
}
