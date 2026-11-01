use super::driver::IncomingMessageContext;
use super::*;
use crate::config::{AuthConfig, ReconnectConfig};
use std::sync::Mutex;
use std::time::Duration;

struct IncomingHarness {
    pending_requests: DashMap<Value, PendingRequest>,
    subscriptions: DashMap<String, Subscription>,
    notification_handlers: DashMap<String, NotificationHandler>,
    rpc_request_handlers: DashMap<String, RpcRequestHandler>,
    connection_event_handlers: DashMap<String, ConnectionEventHandler>,
    connection_id: RwLock<Option<ConnectionId>>,
    message_tx: RwLock<Option<mpsc::Sender<BidirectionalMessage>>>,
    connected_notify: tokio::sync::Notify,
    server_call_permits: Arc<tokio::sync::Semaphore>,
}

impl IncomingHarness {
    fn new() -> Self {
        Self::with_server_call_limit(16)
    }

    fn with_server_call_limit(limit: usize) -> Self {
        Self {
            pending_requests: DashMap::new(),
            subscriptions: DashMap::new(),
            notification_handlers: DashMap::new(),
            rpc_request_handlers: DashMap::new(),
            connection_event_handlers: DashMap::new(),
            connection_id: RwLock::new(None),
            message_tx: RwLock::new(None),
            connected_notify: tokio::sync::Notify::new(),
            server_call_permits: Arc::new(tokio::sync::Semaphore::new(limit)),
        }
    }

    fn context(&self) -> IncomingMessageContext<'_> {
        IncomingMessageContext {
            pending_requests: &self.pending_requests,
            subscriptions: &self.subscriptions,
            notification_handlers: &self.notification_handlers,
            rpc_request_handlers: &self.rpc_request_handlers,
            connection_event_handlers: &self.connection_event_handlers,
            connection_id: &self.connection_id,
            message_tx: &self.message_tx,
            connected_notify: &self.connected_notify,
            server_call_permits: &self.server_call_permits,
        }
    }
}

mod builder;
mod dispatch;
mod lifecycle;
mod requests;
mod subscriptions;
