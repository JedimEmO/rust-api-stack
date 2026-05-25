//! Connection manager trait for bidirectional JSON-RPC

use crate::{BidirectionalMessage, ConnectionId, ConnectionInfo, Result};
use async_trait::async_trait;
use ras_auth_core::AuthenticatedUser;

/// Trait for managing WebSocket connections
#[async_trait]
pub trait ConnectionManager: Send + Sync {
    /// Add a new connection
    async fn add_connection(&self, info: ConnectionInfo) -> Result<()>;

    /// Add a new connection with an existing message sender channel
    /// Default implementation falls back to add_connection
    async fn add_connection_with_sender(
        &self,
        info: ConnectionInfo,
        _sender: Box<dyn std::any::Any + Send + Sync>,
    ) -> Result<()> {
        // Default implementation ignores the sender and falls back to add_connection
        self.add_connection(info).await
    }

    /// Remove a connection
    async fn remove_connection(&self, id: ConnectionId) -> Result<()>;

    /// Get connection information
    async fn get_connection(&self, id: ConnectionId) -> Result<Option<ConnectionInfo>>;

    /// Get all active connections
    async fn get_all_connections(&self) -> Result<Vec<ConnectionInfo>>;

    /// Get connections subscribed to a topic
    async fn get_subscribed_connections(&self, topic: &str) -> Result<Vec<ConnectionInfo>>;

    /// Update connection authentication
    async fn set_connection_user(&self, id: ConnectionId, user: AuthenticatedUser) -> Result<()>;

    /// Clear connection authentication
    async fn clear_connection_user(&self, id: ConnectionId) -> Result<()>;

    /// Add subscription to a connection
    async fn add_subscription(&self, id: ConnectionId, topic: String) -> Result<()>;

    /// Remove subscription from a connection
    async fn remove_subscription(&self, id: ConnectionId, topic: &str) -> Result<()>;

    /// Get all subscriptions for a connection
    async fn get_subscriptions(&self, id: ConnectionId) -> Result<Vec<String>>;

    /// Send a message to a specific connection
    async fn send_to_connection(
        &self,
        id: ConnectionId,
        message: BidirectionalMessage,
    ) -> Result<()>;

    /// Broadcast a message to all connections subscribed to a topic
    async fn broadcast_to_topic(&self, topic: &str, message: BidirectionalMessage)
    -> Result<usize>;

    /// Broadcast a message to all authenticated connections
    async fn broadcast_to_authenticated(&self, message: BidirectionalMessage) -> Result<usize>;

    /// Broadcast a message to all connections with a specific permission
    async fn broadcast_to_permission(
        &self,
        permission: &str,
        message: BidirectionalMessage,
    ) -> Result<usize>;

    /// Check if a connection exists
    async fn connection_exists(&self, id: ConnectionId) -> Result<bool> {
        Ok(self.get_connection(id).await?.is_some())
    }

    /// Get the number of active connections
    async fn connection_count(&self) -> Result<usize> {
        Ok(self.get_all_connections().await?.len())
    }

    /// Get the number of authenticated connections
    async fn authenticated_connection_count(&self) -> Result<usize> {
        let connections = self.get_all_connections().await?;
        Ok(connections.iter().filter(|c| c.is_authenticated()).count())
    }

    /// Clean up stale connections (optional implementation)
    async fn cleanup_stale_connections(&self) -> Result<usize> {
        Ok(0) // Default: no cleanup
    }

    /// Register a pending request for server-to-client RPC calls
    async fn register_pending_request(
        &self,
        connection_id: ConnectionId,
        request_id: serde_json::Value,
        response_sender: tokio::sync::oneshot::Sender<ras_jsonrpc_types::JsonRpcResponse>,
    ) -> Result<()>;

    /// Remove a pending request (used for cleanup)
    async fn remove_pending_request(
        &self,
        connection_id: ConnectionId,
        request_id: &serde_json::Value,
    ) -> Result<Option<tokio::sync::oneshot::Sender<ras_jsonrpc_types::JsonRpcResponse>>>;

    /// Handle an incoming response for a pending request
    async fn handle_pending_response(
        &self,
        connection_id: ConnectionId,
        response: ras_jsonrpc_types::JsonRpcResponse,
    ) -> Result<bool>;
}

/// Extension trait for connection managers with convenience methods
#[async_trait]
pub trait ConnectionManagerExt: ConnectionManager {
    /// Send a JSON-RPC notification to a connection
    async fn notify_connection(
        &self,
        id: ConnectionId,
        method: &str,
        params: serde_json::Value,
    ) -> Result<()> {
        let message = BidirectionalMessage::ServerNotification(crate::ServerNotification {
            method: method.to_string(),
            params,
            metadata: None,
        });
        self.send_to_connection(id, message).await
    }

    /// Broadcast a notification to a topic
    async fn notify_topic(
        &self,
        topic: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<usize> {
        let message = BidirectionalMessage::Broadcast(crate::BroadcastMessage {
            topic: topic.to_string(),
            method: method.to_string(),
            params,
            metadata: None,
        });
        self.broadcast_to_topic(topic, message).await
    }

    /// Send a ping to check if connection is alive
    async fn ping_connection(&self, id: ConnectionId) -> Result<()> {
        self.send_to_connection(id, BidirectionalMessage::Ping)
            .await
    }

    /// Get connections by user ID
    async fn get_user_connections(&self, user_id: &str) -> Result<Vec<ConnectionInfo>> {
        let all = self.get_all_connections().await?;
        Ok(all
            .into_iter()
            .filter(|c| {
                c.user
                    .as_ref()
                    .map(|u| u.user_id == user_id)
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Disconnect all connections for a user
    async fn disconnect_user(&self, user_id: &str) -> Result<usize> {
        let connections = self.get_user_connections(user_id).await?;
        let count = connections.len();

        for conn in connections {
            if let Err(e) = self.remove_connection(conn.id).await {
                tracing::error!("Failed to disconnect user connection {}: {}", conn.id, e);
            }
        }

        Ok(count)
    }
}

// Blanket implementation for all ConnectionManager types
impl<T: ConnectionManager> ConnectionManagerExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BidirectionalError, BroadcastMessage, ConnectionId, ConnectionInfo};
    use ras_auth_core::AuthenticatedUser;
    use ras_jsonrpc_types::JsonRpcResponse;
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    /// Minimal in-memory manager used to exercise the default `connection_*`
    /// methods and the `ConnectionManagerExt` helpers without depending on the
    /// server crate's full manager implementation. Uses sync `Mutex` for
    /// simplicity.
    #[derive(Default)]
    struct StubManager {
        conns: Mutex<HashMap<ConnectionId, ConnectionInfo>>,
        subs: Mutex<HashMap<String, HashSet<ConnectionId>>>,
        sent: Mutex<Vec<(ConnectionId, BidirectionalMessage)>>,
        broadcasts: Mutex<Vec<(String, BidirectionalMessage)>>,
    }

    #[async_trait]
    impl ConnectionManager for StubManager {
        async fn add_connection(&self, info: ConnectionInfo) -> Result<()> {
            self.conns.lock().unwrap().insert(info.id, info);
            Ok(())
        }
        async fn remove_connection(&self, id: ConnectionId) -> Result<()> {
            self.conns
                .lock()
                .unwrap()
                .remove(&id)
                .ok_or(BidirectionalError::ConnectionNotFound(id))?;
            Ok(())
        }
        async fn get_connection(&self, id: ConnectionId) -> Result<Option<ConnectionInfo>> {
            Ok(self.conns.lock().unwrap().get(&id).cloned())
        }
        async fn get_all_connections(&self) -> Result<Vec<ConnectionInfo>> {
            Ok(self.conns.lock().unwrap().values().cloned().collect())
        }
        async fn get_subscribed_connections(&self, topic: &str) -> Result<Vec<ConnectionInfo>> {
            let ids = self
                .subs
                .lock()
                .unwrap()
                .get(topic)
                .cloned()
                .unwrap_or_default();
            let conns = self.conns.lock().unwrap();
            Ok(ids.iter().filter_map(|id| conns.get(id).cloned()).collect())
        }
        async fn set_connection_user(
            &self,
            id: ConnectionId,
            user: AuthenticatedUser,
        ) -> Result<()> {
            if let Some(info) = self.conns.lock().unwrap().get_mut(&id) {
                info.set_user(user);
                Ok(())
            } else {
                Err(BidirectionalError::ConnectionNotFound(id))
            }
        }
        async fn clear_connection_user(&self, id: ConnectionId) -> Result<()> {
            if let Some(info) = self.conns.lock().unwrap().get_mut(&id) {
                info.clear_user();
                Ok(())
            } else {
                Err(BidirectionalError::ConnectionNotFound(id))
            }
        }
        async fn add_subscription(&self, id: ConnectionId, topic: String) -> Result<()> {
            self.subs
                .lock()
                .unwrap()
                .entry(topic.clone())
                .or_default()
                .insert(id);
            if let Some(info) = self.conns.lock().unwrap().get_mut(&id) {
                info.subscribe(topic);
            }
            Ok(())
        }
        async fn remove_subscription(&self, id: ConnectionId, topic: &str) -> Result<()> {
            if let Some(set) = self.subs.lock().unwrap().get_mut(topic) {
                set.remove(&id);
            }
            if let Some(info) = self.conns.lock().unwrap().get_mut(&id) {
                info.unsubscribe(topic);
            }
            Ok(())
        }
        async fn get_subscriptions(&self, id: ConnectionId) -> Result<Vec<String>> {
            Ok(self
                .conns
                .lock()
                .unwrap()
                .get(&id)
                .map(|c| c.subscriptions.iter().cloned().collect())
                .unwrap_or_default())
        }
        async fn send_to_connection(
            &self,
            id: ConnectionId,
            message: BidirectionalMessage,
        ) -> Result<()> {
            self.sent.lock().unwrap().push((id, message));
            Ok(())
        }
        async fn broadcast_to_topic(
            &self,
            topic: &str,
            message: BidirectionalMessage,
        ) -> Result<usize> {
            let n = self
                .subs
                .lock()
                .unwrap()
                .get(topic)
                .map(|s| s.len())
                .unwrap_or(0);
            self.broadcasts
                .lock()
                .unwrap()
                .push((topic.to_string(), message));
            Ok(n)
        }
        async fn broadcast_to_authenticated(
            &self,
            _message: BidirectionalMessage,
        ) -> Result<usize> {
            Ok(self.authenticated_connection_count().await?)
        }
        async fn broadcast_to_permission(
            &self,
            permission: &str,
            _message: BidirectionalMessage,
        ) -> Result<usize> {
            Ok(self
                .conns
                .lock()
                .unwrap()
                .values()
                .filter(|c| c.has_permission(permission))
                .count())
        }
        async fn register_pending_request(
            &self,
            _connection_id: ConnectionId,
            _request_id: serde_json::Value,
            _response_sender: oneshot::Sender<JsonRpcResponse>,
        ) -> Result<()> {
            Ok(())
        }
        async fn remove_pending_request(
            &self,
            _connection_id: ConnectionId,
            _request_id: &serde_json::Value,
        ) -> Result<Option<oneshot::Sender<JsonRpcResponse>>> {
            Ok(None)
        }
        async fn handle_pending_response(
            &self,
            _connection_id: ConnectionId,
            _response: JsonRpcResponse,
        ) -> Result<bool> {
            Ok(false)
        }
    }

    fn user(id: &str, perms: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: id.to_string(),
            permissions: perms.iter().map(|s| s.to_string()).collect(),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn default_methods_delegate_to_required_methods() {
        let mgr = StubManager::default();

        // Initially nothing is registered.
        assert_eq!(mgr.connection_count().await.unwrap(), 0);
        assert_eq!(mgr.authenticated_connection_count().await.unwrap(), 0);
        assert_eq!(mgr.cleanup_stale_connections().await.unwrap(), 0);

        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        mgr.add_connection(ConnectionInfo::new(id1)).await.unwrap();
        mgr.add_connection(ConnectionInfo::new(id2)).await.unwrap();

        assert!(mgr.connection_exists(id1).await.unwrap());
        assert!(mgr.connection_exists(id2).await.unwrap());
        assert_eq!(mgr.connection_count().await.unwrap(), 2);

        // Authenticate one connection.
        mgr.set_connection_user(id1, user("u1", &["read"]))
            .await
            .unwrap();
        assert_eq!(mgr.authenticated_connection_count().await.unwrap(), 1);

        // The default `add_connection_with_sender` must fall through to
        // `add_connection`.
        let id3 = ConnectionId::new();
        let unexpected_sender: Box<dyn std::any::Any + Send + Sync> = Box::new(()) as _;
        mgr.add_connection_with_sender(ConnectionInfo::new(id3), unexpected_sender)
            .await
            .unwrap();
        assert!(mgr.connection_exists(id3).await.unwrap());
    }

    #[tokio::test]
    async fn ext_helpers_route_to_correct_messages() {
        let mgr = StubManager::default();
        let id = ConnectionId::new();
        mgr.add_connection(ConnectionInfo::new(id)).await.unwrap();
        mgr.add_subscription(id, "room:1".into()).await.unwrap();

        // notify_connection wraps as ServerNotification.
        mgr.notify_connection(id, "evt", serde_json::json!({"k": 1}))
            .await
            .unwrap();
        {
            let sent = mgr.sent.lock().unwrap();
            assert_eq!(sent.len(), 1);
            match &sent[0].1 {
                BidirectionalMessage::ServerNotification(n) => assert_eq!(n.method, "evt"),
                other => panic!("unexpected: {other:?}"),
            }
        }

        // notify_topic broadcasts to the topic with one subscriber.
        let n = mgr
            .notify_topic("room:1", "msg", serde_json::json!("hi"))
            .await
            .unwrap();
        assert_eq!(n, 1);
        {
            let bs = mgr.broadcasts.lock().unwrap();
            assert!(matches!(
                &bs[0].1,
                BidirectionalMessage::Broadcast(BroadcastMessage { method, .. }) if method == "msg"
            ));
        }

        // ping_connection should produce a Ping payload.
        mgr.ping_connection(id).await.unwrap();
        let sent = mgr.sent.lock().unwrap();
        assert!(matches!(sent.last().unwrap().1, BidirectionalMessage::Ping));
    }

    #[tokio::test]
    async fn user_helpers_filter_and_disconnect() {
        let mgr = StubManager::default();
        let alice1 = ConnectionId::new();
        let alice2 = ConnectionId::new();
        let bob = ConnectionId::new();
        mgr.add_connection(ConnectionInfo::new(alice1))
            .await
            .unwrap();
        mgr.add_connection(ConnectionInfo::new(alice2))
            .await
            .unwrap();
        mgr.add_connection(ConnectionInfo::new(bob)).await.unwrap();
        mgr.set_connection_user(alice1, user("alice", &[]))
            .await
            .unwrap();
        mgr.set_connection_user(alice2, user("alice", &[]))
            .await
            .unwrap();
        mgr.set_connection_user(bob, user("bob", &[]))
            .await
            .unwrap();

        assert_eq!(mgr.get_user_connections("alice").await.unwrap().len(), 2);
        assert_eq!(mgr.get_user_connections("bob").await.unwrap().len(), 1);
        assert_eq!(mgr.get_user_connections("nobody").await.unwrap().len(), 0);

        let dropped = mgr.disconnect_user("alice").await.unwrap();
        assert_eq!(dropped, 2);
        assert!(!mgr.connection_exists(alice1).await.unwrap());
        assert!(!mgr.connection_exists(alice2).await.unwrap());
        // Bob unaffected.
        assert!(mgr.connection_exists(bob).await.unwrap());
    }

    #[tokio::test]
    async fn subscriptions_users_and_broadcast_filters_update_connection_state() {
        let mgr = StubManager::default();
        let id = ConnectionId::new();
        mgr.add_connection(ConnectionInfo::new(id)).await.unwrap();

        assert!(mgr.get_subscriptions(id).await.unwrap().is_empty());
        mgr.add_subscription(id, "room:1".to_string())
            .await
            .unwrap();
        mgr.add_subscription(id, "alerts".to_string())
            .await
            .unwrap();

        let mut subscriptions = mgr.get_subscriptions(id).await.unwrap();
        subscriptions.sort();
        assert_eq!(subscriptions, vec!["alerts", "room:1"]);
        assert_eq!(
            mgr.get_subscribed_connections("room:1")
                .await
                .unwrap()
                .len(),
            1
        );

        mgr.remove_subscription(id, "room:1").await.unwrap();
        let info = mgr.get_connection(id).await.unwrap().unwrap();
        assert!(!info.is_subscribed_to("room:1"));
        assert!(info.is_subscribed_to("alerts"));

        mgr.set_connection_user(id, user("alice", &["read", "write"]))
            .await
            .unwrap();
        assert_eq!(
            mgr.broadcast_to_authenticated(BidirectionalMessage::Ping)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            mgr.broadcast_to_permission("read", BidirectionalMessage::Ping)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            mgr.broadcast_to_permission("admin", BidirectionalMessage::Ping)
                .await
                .unwrap(),
            0
        );

        mgr.clear_connection_user(id).await.unwrap();
        assert_eq!(mgr.authenticated_connection_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn missing_connection_mutations_return_not_found() {
        let mgr = StubManager::default();
        let missing = ConnectionId::new();

        assert!(matches!(
            mgr.remove_connection(missing).await,
            Err(BidirectionalError::ConnectionNotFound(id)) if id == missing
        ));
        assert!(matches!(
            mgr.set_connection_user(missing, user("alice", &[])).await,
            Err(BidirectionalError::ConnectionNotFound(id)) if id == missing
        ));
        assert!(matches!(
            mgr.clear_connection_user(missing).await,
            Err(BidirectionalError::ConnectionNotFound(id)) if id == missing
        ));
        assert!(!mgr.connection_exists(missing).await.unwrap());
    }
}
