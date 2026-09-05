//! Connection context and management

pub use crate::subscriptions::SubscriptionPolicy;
use ras_auth_core::AuthenticatedUser;
use ras_jsonrpc_bidirectional_types::{
    BidirectionalError, BidirectionalMessage, ConnectionId, ConnectionInfo,
};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::warn;

/// A message queued for a connection's socket.
///
/// `topic` is set when the message was routed through a topic broadcast. The
/// handler loop re-checks the subscription right before writing to the socket
/// and drops the message if it was revoked in the meantime, so a broadcast
/// that snapshotted the topic index moments before a permission change can
/// never reach a connection that no longer holds the subscription.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub message: BidirectionalMessage,
    pub topic: Option<String>,
}

impl From<BidirectionalMessage> for OutboundMessage {
    fn from(message: BidirectionalMessage) -> Self {
        Self {
            message,
            topic: None,
        }
    }
}

/// A simple channel-based message sender
#[derive(Debug, Clone)]
pub struct ChannelMessageSender {
    connection_id: ConnectionId,
    sender: mpsc::Sender<OutboundMessage>,
}

impl ChannelMessageSender {
    /// Create a new channel message sender
    pub fn new(connection_id: ConnectionId, sender: mpsc::Sender<OutboundMessage>) -> Self {
        Self {
            connection_id,
            sender,
        }
    }

    /// Send a message through the channel
    pub async fn send(&self, message: BidirectionalMessage) -> Result<(), String> {
        self.sender
            .send(message.into())
            .await
            .map_err(|e| e.to_string())
    }

    /// Send a message that was routed on `topic`; the receiving handler drops
    /// it if the connection is no longer subscribed when it is dequeued.
    pub async fn send_on_topic(
        &self,
        topic: &str,
        message: BidirectionalMessage,
    ) -> Result<(), String> {
        self.sender
            .send(OutboundMessage {
                message,
                topic: Some(topic.to_string()),
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Get the connection ID
    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }
}

/// Context information for an active WebSocket connection.
///
/// Subscription state can only change through [`subscribe`](Self::subscribe)
/// and [`unsubscribe`](Self::unsubscribe), which enforce the service's
/// [`SubscriptionPolicy`] and mirror into the manager. There is no unchecked
/// path, so the caps hold no matter which callback a handler mutates from.
#[derive(Debug, Clone)]
pub struct ConnectionContext {
    /// Unique identifier for this connection
    pub id: ConnectionId,

    /// Connection information (user, subscriptions, metadata)
    info: Arc<RwLock<ConnectionInfo>>,

    /// Message sender for this connection
    pub sender: ChannelMessageSender,

    policy: SubscriptionPolicy,
}

impl ConnectionContext {
    /// Create a new connection context with a standalone default policy
    /// (default limits, private counter, no manager). Services attach the
    /// shared policy with [`with_subscription_policy`](Self::with_subscription_policy).
    pub fn new(id: ConnectionId, sender: ChannelMessageSender) -> Self {
        let info = ConnectionInfo::new(id);
        Self {
            id,
            info: Arc::new(RwLock::new(info)),
            sender,
            policy: SubscriptionPolicy::default(),
        }
    }

    /// Attach the service-wide subscription policy.
    pub fn with_subscription_policy(mut self, policy: SubscriptionPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The policy this context enforces.
    pub fn subscription_policy(&self) -> &SubscriptionPolicy {
        &self.policy
    }

    /// Check if the connection is authenticated
    pub async fn is_authenticated(&self) -> bool {
        self.info.read().await.is_authenticated()
    }

    /// Get the authenticated user (if any)
    pub async fn get_user(&self) -> Option<Arc<AuthenticatedUser>> {
        self.info.read().await.user.clone()
    }

    /// Set the authenticated user
    pub async fn set_user(&self, user: AuthenticatedUser) {
        self.info.write().await.set_user(user);
    }

    /// Clear the authenticated user
    pub async fn clear_user(&self) {
        self.info.write().await.clear_user();
    }

    /// Check if the connection has a specific permission
    pub async fn has_permission(&self, permission: &str) -> bool {
        self.info.read().await.has_permission(permission)
    }

    /// Check if the connection is subscribed to a topic
    pub async fn is_subscribed_to(&self, topic: &str) -> bool {
        self.info.read().await.is_subscribed_to(topic)
    }

    /// Subscribe this connection to `topic`, subject to the policy.
    ///
    /// Checks the topic length and the per-connection cap, reserves a slot
    /// against the global cap, and mirrors into the manager's topic index;
    /// any refusal leaves state, counter and index untouched. Idempotent for
    /// a topic already held.
    pub async fn subscribe(&self, topic: String) -> Result<(), BidirectionalError> {
        let limits = &self.policy.limits;
        if topic.is_empty() || topic.len() > limits.max_topic_length {
            return Err(BidirectionalError::InvalidTopic(
                "topic name length out of range".to_string(),
            ));
        }

        // The write guard serializes every mutation of this connection's
        // subscriptions, so the cap check and the insert are one unit.
        let mut info = self.info.write().await;
        if info.is_subscribed_to(&topic) {
            return Ok(());
        }
        if info.subscriptions.len() >= limits.max_topics_per_connection {
            return Err(BidirectionalError::SubscriptionLimitReached(
                "subscription limit for this connection reached".to_string(),
            ));
        }
        if !self
            .policy
            .accounting
            .reserve(limits.max_total_subscriptions)
        {
            return Err(BidirectionalError::SubscriptionLimitReached(
                "global subscription limit reached".to_string(),
            ));
        }
        if let Some(manager) = &self.policy.manager
            && let Err(e) = manager.add_subscription(self.id, topic.clone()).await
        {
            self.policy.accounting.release(1);
            return Err(e);
        }
        info.subscribe(topic);
        Ok(())
    }

    /// Remove a subscription. Returns whether it was held.
    ///
    /// The context is updated first, so the egress gate stops delivering on
    /// the topic before the manager index catches up.
    pub async fn unsubscribe(&self, topic: &str) -> bool {
        let removed = {
            let mut info = self.info.write().await;
            info.unsubscribe(topic)
        };
        if removed {
            self.policy.accounting.release(1);
            if let Some(manager) = &self.policy.manager
                && let Err(e) = manager.remove_subscription(self.id, topic).await
            {
                warn!(
                    "Manager failed to drop subscription '{}' for {}: {}",
                    topic, self.id, e
                );
            }
        }
        removed
    }

    /// Release every held subscription (connection teardown).
    pub(crate) async fn release_all_subscriptions(&self) {
        let topics = self.get_subscriptions().await;
        for topic in topics {
            self.unsubscribe(&topic).await;
        }
    }

    /// Get all subscriptions
    pub async fn get_subscriptions(&self) -> Vec<String> {
        self.info
            .read()
            .await
            .subscriptions
            .iter()
            .cloned()
            .collect()
    }

    /// Update connection metadata
    pub async fn set_metadata(&self, key: &str, value: serde_json::Value) {
        let mut info = self.info.write().await;
        if let serde_json::Value::Object(map) = &mut info.metadata {
            map.insert(key.to_string(), value);
        }
    }

    /// Get connection metadata
    pub async fn get_metadata(&self, key: &str) -> Option<serde_json::Value> {
        let info = self.info.read().await;
        if let serde_json::Value::Object(map) = &info.metadata {
            map.get(key).cloned()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ras_auth_core::AuthenticatedUser;
    use std::collections::HashSet;

    fn user(id: &str, perms: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: id.to_string(),
            permissions: perms.iter().map(|s| s.to_string()).collect::<HashSet<_>>(),
            metadata: None,
        }
    }

    fn ctx() -> ConnectionContext {
        let id = ConnectionId::new();
        let (tx, _rx) = mpsc::channel(8);
        let sender = ChannelMessageSender::new(id, tx);
        ConnectionContext::new(id, sender)
    }

    #[tokio::test]
    async fn channel_sender_send_propagates_and_id_round_trips() {
        let id = ConnectionId::new();
        let (tx, mut rx) = mpsc::channel(2);
        let sender = ChannelMessageSender::new(id, tx);
        assert_eq!(sender.connection_id(), id);

        sender.send(BidirectionalMessage::Ping).await.unwrap();
        let received = rx.recv().await.unwrap();
        assert!(matches!(received.message, BidirectionalMessage::Ping));
    }

    #[tokio::test]
    async fn channel_sender_returns_string_error_when_closed() {
        let id = ConnectionId::new();
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let sender = ChannelMessageSender::new(id, tx);
        let err = sender.send(BidirectionalMessage::Ping).await.unwrap_err();
        assert!(!err.is_empty());
    }

    #[tokio::test]
    async fn auth_state_round_trips() {
        let c = ctx();
        assert!(!c.is_authenticated().await);
        assert!(c.get_user().await.is_none());
        assert!(!c.has_permission("admin").await);

        c.set_user(user("alice", &["admin"])).await;
        assert!(c.is_authenticated().await);
        assert_eq!(c.get_user().await.unwrap().user_id, "alice");
        assert!(c.has_permission("admin").await);
        assert!(!c.has_permission("nope").await);

        c.clear_user().await;
        assert!(!c.is_authenticated().await);
    }

    #[tokio::test]
    async fn subscriptions_round_trip() {
        let c = ctx();
        assert!(c.get_subscriptions().await.is_empty());
        assert!(!c.is_subscribed_to("t1").await);

        c.subscribe("t1".into()).await.unwrap();
        c.subscribe("t2".into()).await.unwrap();
        assert!(c.is_subscribed_to("t1").await);
        assert_eq!(c.get_subscriptions().await.len(), 2);

        assert!(c.unsubscribe("t1").await);
        assert!(!c.is_subscribed_to("t1").await);
        // Idempotent: removing again returns false.
        assert!(!c.unsubscribe("t1").await);
    }

    #[tokio::test]
    async fn metadata_get_set() {
        let c = ctx();
        assert!(c.get_metadata("k").await.is_none());
        c.set_metadata("k", serde_json::json!("v")).await;
        assert_eq!(c.get_metadata("k").await.unwrap(), serde_json::json!("v"));
        assert!(c.get_metadata("missing").await.is_none());
    }
}
