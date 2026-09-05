use ras_auth_core::AuthenticatedUser;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt, sync::Arc};
use uuid::Uuid;

/// Unique identifier for a WebSocket connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Create a new random connection ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a connection ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a connected client
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Unique connection identifier
    pub id: ConnectionId,
    /// Authenticated user information (if authenticated)
    pub user: Option<Arc<AuthenticatedUser>>,
    /// Topics this connection is subscribed to
    pub subscriptions: HashSet<String>,
    /// Connection metadata (e.g., user agent, IP address)
    pub metadata: serde_json::Value,
    /// When the connection was established
    pub connected_at: chrono::DateTime<chrono::Utc>,
}

impl ConnectionInfo {
    /// Create a new connection info
    pub fn new(id: ConnectionId) -> Self {
        Self {
            id,
            user: None,
            subscriptions: HashSet::new(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            connected_at: chrono::Utc::now(),
        }
    }

    /// Check if the connection is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.user.is_some()
    }

    /// Check if the connection has a specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.user
            .as_ref()
            .map(|u| u.permissions.contains(permission))
            .unwrap_or(false)
    }

    /// Check if the connection is subscribed to a topic
    pub fn is_subscribed_to(&self, topic: &str) -> bool {
        self.subscriptions.contains(topic)
    }

    /// Add a subscription
    pub fn subscribe(&mut self, topic: String) {
        self.subscriptions.insert(topic);
    }

    /// Remove a subscription
    pub fn unsubscribe(&mut self, topic: &str) -> bool {
        self.subscriptions.remove(topic)
    }

    /// Set authenticated user
    pub fn set_user(&mut self, user: AuthenticatedUser) {
        self.user = Some(Arc::new(user));
    }

    /// Clear authenticated user
    pub fn clear_user(&mut self) {
        self.user = None;
    }
}
