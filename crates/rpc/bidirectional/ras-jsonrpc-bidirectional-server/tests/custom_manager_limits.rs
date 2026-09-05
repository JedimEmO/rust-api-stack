//! Subscription limits must hold for the extensibility API, not just the
//! bundled manager: a permissive custom `ConnectionManager` supplied through
//! `build_with_manager`, combined with a greedy custom `MessageHandler` that
//! writes straight into the connection context, still cannot exceed the
//! service's configured caps.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ras_auth_core::{AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_bidirectional_server::handler::{WebSocketIo, WebSocketIoMessage};
use ras_jsonrpc_bidirectional_server::{
    ConnectionContext, MessageHandler, ServerResult, SubscriptionLimits, WebSocketService,
    WebSocketServiceBuilder,
};
use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, ConnectionId, ConnectionInfo, ConnectionManager, Result,
};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use std::collections::VecDeque;

/// Accepts every subscription, tracks nothing about limits, reports no count.
#[derive(Default)]
struct PermissiveManager {
    conns: Mutex<HashMap<ConnectionId, ConnectionInfo>>,
    subs: Mutex<HashMap<ConnectionId, Vec<String>>>,
    /// Every add_subscription ever accepted, never pruned.
    log: Mutex<Vec<(ConnectionId, String)>>,
}

#[async_trait]
impl ConnectionManager for PermissiveManager {
    async fn add_connection(&self, info: ConnectionInfo) -> Result<()> {
        self.conns.lock().unwrap().insert(info.id, info);
        Ok(())
    }
    async fn add_connection_with_sender(
        &self,
        info: ConnectionInfo,
        _sender: Box<dyn std::any::Any + Send + Sync>,
    ) -> Result<()> {
        self.add_connection(info).await
    }
    async fn remove_connection(&self, id: ConnectionId) -> Result<()> {
        self.conns.lock().unwrap().remove(&id);
        self.subs.lock().unwrap().remove(&id);
        Ok(())
    }
    async fn get_connection(&self, id: ConnectionId) -> Result<Option<ConnectionInfo>> {
        Ok(self.conns.lock().unwrap().get(&id).cloned())
    }
    async fn get_all_connections(&self) -> Result<Vec<ConnectionInfo>> {
        Ok(self.conns.lock().unwrap().values().cloned().collect())
    }
    async fn get_subscribed_connections(&self, _topic: &str) -> Result<Vec<ConnectionInfo>> {
        Ok(Vec::new())
    }
    async fn set_connection_user(&self, _id: ConnectionId, _user: AuthenticatedUser) -> Result<()> {
        Ok(())
    }
    async fn clear_connection_user(&self, _id: ConnectionId) -> Result<()> {
        Ok(())
    }
    async fn add_subscription(&self, id: ConnectionId, topic: String) -> Result<()> {
        self.log.lock().unwrap().push((id, topic.clone()));
        self.subs.lock().unwrap().entry(id).or_default().push(topic);
        Ok(())
    }
    async fn remove_subscription(&self, id: ConnectionId, topic: &str) -> Result<()> {
        if let Some(list) = self.subs.lock().unwrap().get_mut(&id) {
            list.retain(|t| t != topic);
        }
        Ok(())
    }
    async fn get_subscriptions(&self, id: ConnectionId) -> Result<Vec<String>> {
        Ok(self
            .subs
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }
    async fn send_to_connection(&self, _id: ConnectionId, _m: BidirectionalMessage) -> Result<()> {
        Ok(())
    }
    async fn broadcast_to_topic(&self, _t: &str, _m: BidirectionalMessage) -> Result<usize> {
        Ok(0)
    }
    async fn broadcast_to_authenticated(&self, _m: BidirectionalMessage) -> Result<usize> {
        Ok(0)
    }
    async fn broadcast_to_permission(&self, _p: &str, _m: BidirectionalMessage) -> Result<usize> {
        Ok(0)
    }
    async fn register_pending_request(
        &self,
        _id: ConnectionId,
        _rid: serde_json::Value,
        _tx: tokio::sync::oneshot::Sender<JsonRpcResponse>,
    ) -> Result<()> {
        Ok(())
    }
    async fn remove_pending_request(
        &self,
        _id: ConnectionId,
        _rid: &serde_json::Value,
    ) -> Result<Option<tokio::sync::oneshot::Sender<JsonRpcResponse>>> {
        Ok(None)
    }
    async fn handle_pending_response(
        &self,
        _id: ConnectionId,
        _r: JsonRpcResponse,
    ) -> Result<bool> {
        Ok(false)
    }
}

/// Ignores the request and subscribes the context to `count` topics directly.
struct GreedyHandler {
    count: usize,
    prefix: &'static str,
}

#[async_trait]
impl MessageHandler for GreedyHandler {
    async fn handle_request(
        &self,
        _request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }
    async fn handle_subscribe(
        &self,
        _topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        for i in 0..self.count {
            let _ = context.subscribe(format!("{}:{i}", self.prefix)).await;
        }
        Ok(())
    }
}

struct NoAuth;
impl AuthProvider for NoAuth {
    fn authenticate(&self, _t: String) -> AuthFuture<'_> {
        Box::pin(async { Err(ras_auth_core::AuthError::InvalidToken) })
    }
}

struct Socket {
    incoming: VecDeque<WebSocketIoMessage>,
    outgoing: Vec<WebSocketIoMessage>,
}

#[async_trait]
impl WebSocketIo for Socket {
    async fn send(&mut self, m: WebSocketIoMessage) -> ServerResult<()> {
        self.outgoing.push(m);
        Ok(())
    }
    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
        self.incoming.pop_front().map(Ok)
    }
}

fn subscribe_frame() -> WebSocketIoMessage {
    WebSocketIoMessage::Text(
        serde_json::to_string(&BidirectionalMessage::Subscribe {
            topics: vec!["anything".into()],
        })
        .unwrap(),
    )
}

#[tokio::test]
async fn permissive_custom_manager_cannot_exceed_per_connection_cap() {
    let manager = Arc::new(PermissiveManager::default());
    let limits = SubscriptionLimits {
        max_topics_per_connection: 3,
        ..SubscriptionLimits::default()
    };
    let service = WebSocketServiceBuilder::builder()
        .handler(Arc::new(GreedyHandler {
            count: 10,
            prefix: "g",
        }))
        .auth_provider(Arc::new(NoAuth))
        .require_auth(false)
        .subscription_limits(limits)
        .build()
        .build_with_manager(manager.clone());

    let mut socket = Socket {
        incoming: VecDeque::from([subscribe_frame()]),
        outgoing: Vec::new(),
    };
    service
        .handle_connection_with_io(&mut socket, None)
        .await
        .unwrap();

    // The manager accepted everything it was given; the server only gave it
    // the cap's worth, and released them all on disconnect.
    assert_eq!(
        manager.log.lock().unwrap().len(),
        3,
        "manager saw more than the cap"
    );
    assert_eq!(service.subscription_accounting().total(), 0);
}

#[tokio::test]
async fn permissive_custom_manager_cannot_exceed_global_cap() {
    let manager = Arc::new(PermissiveManager::default());
    let limits = SubscriptionLimits {
        max_total_subscriptions: 5,
        max_topics_per_connection: 100,
        ..SubscriptionLimits::default()
    };
    let service = WebSocketServiceBuilder::builder()
        .handler(Arc::new(GreedyHandler {
            count: 4,
            prefix: "g",
        }))
        .auth_provider(Arc::new(NoAuth))
        .require_auth(false)
        .subscription_limits(limits)
        .build()
        .build_with_manager(manager.clone());

    // Keep the first connection open (its handler sees the frame, then the
    // socket parks on an empty queue) so its slots stay reserved while the
    // second connection subscribes.
    let (hold_tx, hold_rx) = tokio::sync::watch::channel(false);
    struct Parked {
        inner: Socket,
        hold: tokio::sync::watch::Receiver<bool>,
    }
    #[async_trait]
    impl WebSocketIo for Parked {
        async fn send(&mut self, m: WebSocketIoMessage) -> ServerResult<()> {
            self.inner.send(m).await
        }
        async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
            if let Some(m) = self.inner.incoming.pop_front() {
                return Some(Ok(m));
            }
            let _ = self.hold.wait_for(|released| *released).await;
            None
        }
    }
    let first = {
        let service = service.clone();
        let hold = hold_rx;
        tokio::spawn(async move {
            let mut socket = Parked {
                inner: Socket {
                    incoming: VecDeque::from([subscribe_frame()]),
                    outgoing: Vec::new(),
                },
                hold,
            };
            service.handle_connection_with_io(&mut socket, None).await
        })
    };
    // Wait until the first connection holds its 4 slots.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while service.subscription_accounting().total() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first connection should reserve its 4 slots");

    let mut socket = Socket {
        incoming: VecDeque::from([subscribe_frame()]),
        outgoing: Vec::new(),
    };
    service
        .handle_connection_with_io(&mut socket, None)
        .await
        .unwrap();

    // Second connection wanted 4 more; only 1 slot remained globally.
    assert_eq!(
        manager.log.lock().unwrap().len(),
        5,
        "4 from the first, 1 from the second"
    );
    assert_eq!(
        service.subscription_accounting().total(),
        4,
        "second connection released its 1"
    );

    hold_tx.send(true).unwrap();
    first.await.unwrap().unwrap();
    assert_eq!(service.subscription_accounting().total(), 0);
}
