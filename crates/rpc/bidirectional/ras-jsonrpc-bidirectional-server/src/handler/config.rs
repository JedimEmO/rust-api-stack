//! Revalidation and keepalive policy for a connection.

use ras_auth_core::AuthProvider;
use std::sync::Arc;
use std::time::Duration;

/// Default interval between credential re-validations on long-lived connections.
pub const DEFAULT_AUTH_REVALIDATION_INTERVAL: Duration = Duration::from_secs(30);

/// Periodic credential re-validation for a long-lived connection.
///
/// The token is captured before the WebSocket upgrade and re-run through the
/// auth provider on every `interval` tick. Failure closes the connection;
/// success refreshes the cached user (so permission changes propagate). This
/// bounds the lifetime of revoked/expired credentials on an open socket to at
/// most one interval.
pub struct AuthRevalidation {
    /// Provider used to re-run authentication
    pub auth_provider: Arc<dyn AuthProvider>,
    /// Token captured at upgrade time
    pub token: String,
    /// How often to re-validate
    pub interval: Duration,
    /// What to do when re-validation succeeds but the permission set changed
    pub on_permission_change: PermissionChangePolicy,
}

/// Policy applied when a live connection's permissions change on
/// re-validation (W1).
///
/// In both modes every held subscription is re-run through
/// [`MessageHandler::authorize_subscribe`](super::MessageHandler::authorize_subscribe) against the refreshed user and
/// topics that are no longer authorized are dropped, so a downgraded
/// connection stops receiving topic broadcasts within one interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionChangePolicy {
    /// Keep the socket open and silently drop subscriptions that are no
    /// longer authorized.
    #[default]
    DropSubscriptions,
    /// Close the socket so the client must reconnect and re-authenticate.
    Close,
}

/// Server-side keepalive for a connection (W4).
///
/// The server sends a WebSocket ping every `ping_interval`; browsers and
/// tungstenite answer pings automatically, and any inbound frame (including
/// the pong) resets the idle clock. A connection that stays silent for
/// `idle_timeout` is closed, so half-open sockets are reclaimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepaliveConfig {
    /// Interval between server-initiated pings (`None` disables pings)
    pub ping_interval: Option<Duration>,
    /// Close the socket after this long without any inbound frame
    /// (`None` disables the idle timeout)
    pub idle_timeout: Option<Duration>,
}

impl Default for KeepaliveConfig {
    fn default() -> Self {
        Self {
            ping_interval: Some(Duration::from_secs(30)),
            idle_timeout: Some(Duration::from_secs(90)),
        }
    }
}
