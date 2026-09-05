//! Subscription limits, service-wide accounting, and policy configuration.

use ras_jsonrpc_bidirectional_types::ConnectionManager;
use std::sync::Arc;

/// Limits on client-initiated subscriptions (W3).
///
/// Enforced by the handler loop before [`MessageHandler::handle_subscribe`](crate::handler::MessageHandler::handle_subscribe)
/// runs, so services never see an over-limit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionLimits {
    /// Maximum topics in one `Subscribe`/`Unsubscribe` message
    pub max_topics_per_message: usize,
    /// Maximum concurrent subscriptions held by one connection
    pub max_topics_per_connection: usize,
    /// Maximum topic name length in bytes
    pub max_topic_length: usize,
    /// Maximum (connection, topic) pairs across the whole manager. `0`
    /// disables the cap. Enforced only when the manager reports its count
    /// (`ConnectionManager::total_subscription_count`); the default manager
    /// does.
    pub max_total_subscriptions: usize,
}

impl Default for SubscriptionLimits {
    fn default() -> Self {
        Self {
            max_topics_per_message: 64,
            max_topics_per_connection: 256,
            max_topic_length: 256,
            max_total_subscriptions: 100_000,
        }
    }
}

/// Service-wide count of held subscriptions, shared by every connection of a
/// service so the global cap is enforced by the server itself, independently
/// of which `ConnectionManager` or `MessageHandler` is plugged in.
#[derive(Debug, Default)]
pub struct SubscriptionAccounting {
    total: std::sync::atomic::AtomicUsize,
}

impl SubscriptionAccounting {
    /// Current number of (connection, topic) pairs held across the service.
    pub fn total(&self) -> usize {
        self.total.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Atomically reserve one slot; `false` when `max` (non-zero) is reached.
    pub(crate) fn reserve(&self, max: usize) -> bool {
        use std::sync::atomic::Ordering;
        let previous = self.total.fetch_add(1, Ordering::AcqRel);
        if max > 0 && previous >= max {
            self.total.fetch_sub(1, Ordering::AcqRel);
            return false;
        }
        true
    }

    pub(crate) fn release(&self, count: usize) {
        self.total
            .fetch_sub(count, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Everything a subscription mutation has to be checked against and mirrored
/// into. Owned by the service and shared by all of its connections.
#[derive(Clone, Default)]
pub struct SubscriptionPolicy {
    /// Caps applied to every subscribe
    pub limits: SubscriptionLimits,
    /// Service-wide counter behind the global cap
    pub accounting: Arc<SubscriptionAccounting>,
    /// Manager whose topic index mirrors accepted subscriptions
    pub manager: Option<Arc<dyn ConnectionManager>>,
}

impl std::fmt::Debug for SubscriptionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscriptionPolicy")
            .field("limits", &self.limits)
            .field("held", &self.accounting.total())
            .field("manager", &self.manager.is_some())
            .finish()
    }
}
