use super::*;
use crate::connection::ChannelMessageSender;
use ras_jsonrpc_bidirectional_types::ConnectionId;
use std::collections::VecDeque;
use std::sync::Mutex;

/// A minimal MessageHandler that only implements the required method —
/// every other method falls through to the default impl, which is what
/// these tests are verifying.
struct PassThrough;

#[async_trait]
impl MessageHandler for PassThrough {
    async fn handle_request(
        &self,
        _request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }
}

struct RespondingHandler;

#[async_trait]
impl MessageHandler for RespondingHandler {
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(Some(JsonRpcResponse::success(
            serde_json::json!({
                "method": request.method,
                "params": request.params,
            }),
            request.id,
        )))
    }
}

struct RecoveringHandler;

#[async_trait]
impl MessageHandler for RecoveringHandler {
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        if request.method == "fail" {
            return Err(ServerError::InvalidRequest("bad request".into()));
        }

        Ok(Some(JsonRpcResponse::success(
            serde_json::json!({
                "method": request.method,
            }),
            request.id,
        )))
    }
}

struct RecordingLifecycle {
    disconnect_reasons: Mutex<Vec<Option<String>>>,
}

impl RecordingLifecycle {
    fn new() -> Self {
        Self {
            disconnect_reasons: Mutex::new(Vec::new()),
        }
    }

    fn disconnect_reasons(&self) -> Vec<Option<String>> {
        self.disconnect_reasons
            .lock()
            .expect("disconnect reasons lock")
            .clone()
    }
}

#[async_trait]
impl MessageHandler for RecordingLifecycle {
    async fn handle_request(
        &self,
        _request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }

    async fn on_disconnect(
        &self,
        _context: Arc<ConnectionContext>,
        reason: Option<String>,
    ) -> ServerResult<()> {
        self.disconnect_reasons
            .lock()
            .expect("disconnect reasons lock")
            .push(reason);
        Ok(())
    }
}

struct InMemorySocket {
    incoming: VecDeque<WebSocketIoMessage>,
    outgoing: Vec<WebSocketIoMessage>,
    close_when_empty: bool,
}

impl InMemorySocket {
    fn closing(incoming: impl IntoIterator<Item = WebSocketIoMessage>) -> Self {
        Self {
            incoming: incoming.into_iter().collect(),
            outgoing: Vec::new(),
            close_when_empty: true,
        }
    }

    fn pending() -> Self {
        Self {
            incoming: VecDeque::new(),
            outgoing: Vec::new(),
            close_when_empty: false,
        }
    }
}

#[async_trait]
impl WebSocketIo for InMemorySocket {
    async fn send(&mut self, message: WebSocketIoMessage) -> ServerResult<()> {
        self.outgoing.push(message);
        Ok(())
    }

    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
        if let Some(message) = self.incoming.pop_front() {
            return Some(Ok(message));
        }

        if self.close_when_empty {
            None
        } else {
            std::future::pending::<Option<ServerResult<WebSocketIoMessage>>>().await
        }
    }
}

fn ctx() -> Arc<ConnectionContext> {
    let id = ConnectionId::new();
    let (tx, _rx) = mpsc::channel(4);
    let sender = ChannelMessageSender::new(id, tx);
    Arc::new(ConnectionContext::new(id, sender))
}

fn ctx_with(policy: crate::connection::SubscriptionPolicy) -> Arc<ConnectionContext> {
    let id = ConnectionId::new();
    let (tx, _rx) = mpsc::channel(4);
    let sender = ChannelMessageSender::new(id, tx);
    Arc::new(ConnectionContext::new(id, sender).with_subscription_policy(policy))
}

fn limits_policy(limits: SubscriptionLimits) -> crate::connection::SubscriptionPolicy {
    crate::connection::SubscriptionPolicy {
        limits,
        ..Default::default()
    }
}

struct AllowListHandler;

#[async_trait]
impl MessageHandler for AllowListHandler {
    async fn handle_request(
        &self,
        _request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }

    async fn authorize_subscribe(
        &self,
        topic: &str,
        _context: &Arc<ConnectionContext>,
    ) -> ServerResult<bool> {
        Ok(topic == "room:allowed")
    }
}

fn auth_user(id: &str) -> ras_auth_core::AuthenticatedUser {
    ras_auth_core::AuthenticatedUser {
        user_id: id.to_string(),
        permissions: std::collections::HashSet::new(),
        metadata: None,
    }
}

/// Auth provider that replays a fixed sequence of results, then fails.
struct SequenceAuthProvider(
    Mutex<VecDeque<Result<ras_auth_core::AuthenticatedUser, ras_auth_core::AuthError>>>,
);

impl SequenceAuthProvider {
    fn new(
        results: impl IntoIterator<
            Item = Result<ras_auth_core::AuthenticatedUser, ras_auth_core::AuthError>,
        >,
    ) -> Self {
        Self(Mutex::new(results.into_iter().collect()))
    }
}

impl AuthProvider for SequenceAuthProvider {
    fn authenticate(&self, _token: String) -> ras_auth_core::AuthFuture<'_> {
        let result = self
            .0
            .lock()
            .expect("results lock")
            .pop_front()
            .unwrap_or(Err(ras_auth_core::AuthError::InvalidToken));
        Box::pin(async move { result })
    }
}

fn auth_user_with(id: &str, perms: &[&str]) -> ras_auth_core::AuthenticatedUser {
    let mut user = auth_user(id);
    user.permissions = perms.iter().map(|p| p.to_string()).collect();
    user
}

/// Authorizes any topic for connections holding `room:read`.
struct PermissionGated;

#[async_trait]
impl MessageHandler for PermissionGated {
    async fn handle_request(
        &self,
        _request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }

    async fn authorize_subscribe(
        &self,
        _topic: &str,
        context: &Arc<ConnectionContext>,
    ) -> ServerResult<bool> {
        Ok(context.has_permission("room:read").await)
    }
}

fn subscribe_msg(topics: Vec<String>) -> WebSocketIoMessage {
    WebSocketIoMessage::Text(
        serde_json::to_string(&BidirectionalMessage::Subscribe { topics }).unwrap(),
    )
}

async fn manager_with(context: &ConnectionContext) -> Arc<dyn ConnectionManager> {
    let manager: Arc<dyn ConnectionManager> = Arc::new(crate::DefaultConnectionManager::new());
    manager
        .add_connection(ras_jsonrpc_bidirectional_types::ConnectionInfo::new(
            context.id,
        ))
        .await
        .unwrap();
    manager
}

/// Like `PermissionGated`, but when it denies a topic during
/// re-authorization it first pushes a broadcast for that topic into the
/// connection's queue, simulating a `broadcast_to_topic` that snapshotted
/// the manager index in the window before the subscription was removed.
struct RacingAuthorizer;

#[async_trait]
impl MessageHandler for RacingAuthorizer {
    async fn handle_request(
        &self,
        _request: JsonRpcRequest,
        _context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }

    async fn authorize_subscribe(
        &self,
        topic: &str,
        context: &Arc<ConnectionContext>,
    ) -> ServerResult<bool> {
        if context.has_permission("room:read").await {
            return Ok(true);
        }
        let stale =
            BidirectionalMessage::Broadcast(ras_jsonrpc_bidirectional_types::BroadcastMessage {
                topic: topic.to_string(),
                method: "secret".into(),
                params: serde_json::json!({}),
                metadata: None,
            });
        context.sender.send_on_topic(topic, stale).await.unwrap();
        Ok(false)
    }
}

static GREEDY_ACCEPTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A custom handler that subscribes to far more topics than any limit
/// allows, from `on_connect` as well as `handle_subscribe`, straight on
/// the context.
struct GreedyHandler;

impl GreedyHandler {
    async fn grab(context: &ConnectionContext, prefix: &str) {
        for i in 0..10 {
            if context.subscribe(format!("{prefix}:{i}")).await.is_ok() {
                GREEDY_ACCEPTED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
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

    async fn on_connect(&self, context: Arc<ConnectionContext>) -> ServerResult<()> {
        Self::grab(&context, "connect").await;
        Ok(())
    }

    async fn handle_subscribe(
        &self,
        _topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        Self::grab(&context, "greedy").await;
        Ok(())
    }
}

fn bidirectional_outgoing(socket: &InMemorySocket) -> Vec<BidirectionalMessage> {
    socket
        .outgoing
        .iter()
        .filter_map(|message| match message {
            WebSocketIoMessage::Text(text) => serde_json::from_str(text).ok(),
            _ => None,
        })
        .collect()
}
mod keepalive;
mod lifecycle;
mod protocol;
mod revalidation;
mod subscriptions;
