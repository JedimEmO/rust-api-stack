//! WebSocket service implementation with builder pattern

use crate::{
    ConnectionContext, DefaultConnectionManager, MessageHandler, MessageRouter, ServerError,
    ServerResult, WebSocketHandler, WebSocketUpgrade,
    connection::ChannelMessageSender,
    handler::{
        AuthRevalidation, AxumWebSocketIo, DEFAULT_AUTH_REVALIDATION_INTERVAL, KeepaliveConfig,
        PermissionChangePolicy, SubscriptionAccounting, SubscriptionLimits, WebSocketIo,
        WebSocketIoMessage,
    },
};
use axum::{
    extract::{State, ws::WebSocketUpgrade as AxumWebSocketUpgrade},
    http::HeaderMap,
    response::Response,
};
use bon::Builder;
use ras_auth_core::AuthProvider;
use ras_jsonrpc_bidirectional_types::{ConnectionId, ConnectionInfo, ConnectionManager};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

const DEFAULT_MESSAGE_CHANNEL_CAPACITY: usize = 1024;
const DEFAULT_MAX_MESSAGE_SIZE: usize = 1024 * 1024;
/// Default cap on simultaneous connections. Bounded by default so a single
/// deployment cannot be driven to memory exhaustion by connection count;
/// pass `max_connections(None)` explicitly to lift it.
pub const DEFAULT_MAX_CONNECTIONS: usize = 10_000;

/// Trait for services that handle WebSocket JSON-RPC communication
#[allow(async_fn_in_trait)]
pub trait WebSocketService: Clone + Send + Sync + 'static {
    /// The message handler type
    type Handler: MessageHandler;
    /// The auth provider type
    type AuthProvider: AuthProvider;
    /// The connection manager type
    type ConnectionManager: ConnectionManager;

    /// Get the message handler
    fn handler(&self) -> Arc<Self::Handler>;

    /// Get the auth provider
    fn auth_provider(&self) -> Arc<Self::AuthProvider>;

    /// Get the connection manager
    fn connection_manager(&self) -> Arc<Self::ConnectionManager>;

    /// Check if authentication is required
    fn require_auth(&self) -> bool;

    /// Maximum queued outbound messages per connection.
    fn message_channel_capacity(&self) -> usize {
        DEFAULT_MESSAGE_CHANNEL_CAPACITY
    }

    /// Maximum accepted inbound WebSocket message size in bytes.
    fn max_message_size(&self) -> usize {
        DEFAULT_MAX_MESSAGE_SIZE
    }

    /// How often to re-run authentication for a live connection.
    ///
    /// Bounds the lifetime of revoked/expired credentials on a long-lived
    /// WebSocket to at most one interval.
    fn auth_revalidation_interval(&self) -> Duration {
        DEFAULT_AUTH_REVALIDATION_INTERVAL
    }

    /// Configured cap on simultaneous connections, for reporting. Admission
    /// itself is decided by [`connection_permits`](Self::connection_permits).
    fn max_connections(&self) -> Option<usize> {
        self.connection_permits().map(|_| DEFAULT_MAX_CONNECTIONS)
    }

    /// Semaphore whose permits are the connection slots. A permit is taken
    /// before the upgrade and held for the life of the connection, so the cap
    /// cannot be exceeded by concurrent upgrades. Return `None` only to run
    /// unbounded; there is no advisory fallback.
    fn connection_permits(&self) -> Option<Arc<tokio::sync::Semaphore>>;

    /// Limits on client-initiated subscriptions.
    fn subscription_limits(&self) -> SubscriptionLimits {
        SubscriptionLimits::default()
    }

    /// Service-wide subscription counter shared by every connection, so the
    /// global cap holds whatever manager or handler is in use.
    fn subscription_accounting(&self) -> Arc<SubscriptionAccounting>;

    /// Server-side ping interval and idle timeout.
    fn keepalive(&self) -> KeepaliveConfig {
        KeepaliveConfig::default()
    }

    /// What to do when a connection's permissions change on re-validation.
    fn on_permission_change(&self) -> PermissionChangePolicy {
        PermissionChangePolicy::default()
    }

    /// Handle WebSocket upgrade
    async fn handle_upgrade(
        &self,
        upgrade: AxumWebSocketUpgrade,
        headers: HeaderMap,
    ) -> Result<Response, (axum::http::StatusCode, String)> {
        // Refuse before upgrading when the connection cap is reached. The
        // permit is held by the connection future, so admission is atomic.
        let permit = match admit_connection(self).await {
            Ok(permit) => permit,
            Err(()) => {
                return Err((
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "Connection limit reached".to_string(),
                ));
            }
        };

        // Enforce the message limit at the transport so an oversized frame is
        // rejected before it is buffered, not after (W2).
        let max_message_size = self.max_message_size();
        let upgrade = upgrade
            .max_message_size(max_message_size)
            .max_frame_size(max_message_size)
            // Select the real subprotocol so browsers that also offered
            // `token.<jwt>` accept the upgrade without the token being echoed.
            .protocols([crate::upgrade::WS_SUBPROTOCOL]);
        let ws_upgrade = WebSocketUpgrade::new(upgrade, headers);
        // Captured pre-upgrade so the connection can periodically re-validate it.
        let auth_token = ws_upgrade.extract_auth_token();
        let service = self.clone();

        ws_upgrade
            .on_upgrade_with_auth(
                &*self.auth_provider(),
                self.require_auth(),
                move |socket, user| {
                    Box::pin(async move {
                        let _permit = permit;
                        let mut socket = AxumWebSocketIo::new(socket);
                        if let Err(e) = run_connection_with_io(
                            service,
                            &mut socket,
                            user,
                            auth_token,
                            Admission::Granted,
                        )
                        .await
                        {
                            error!("WebSocket connection error: {}", e);
                        }
                    })
                },
            )
            .await
    }

    /// Handle an individual WebSocket connection
    fn handle_connection(
        &self,
        socket: axum::extract::ws::WebSocket,
        user: Option<ras_auth_core::AuthenticatedUser>,
        auth_token: Option<String>,
    ) -> impl std::future::Future<Output = ServerResult<()>> + Send {
        let service = self.clone();
        async move {
            let mut socket = AxumWebSocketIo::new(socket);
            run_connection_with_io(service, &mut socket, user, auth_token, Admission::Pending).await
        }
    }

    /// Handle an individual WebSocket connection over an injected socket implementation.
    ///
    /// This runs the same service lifecycle as the Axum upgrade path while letting tests and
    /// alternate transports exercise the connection without binding a real socket.
    fn handle_connection_with_io<'a, S>(
        &'a self,
        socket: &'a mut S,
        user: Option<ras_auth_core::AuthenticatedUser>,
    ) -> impl std::future::Future<Output = ServerResult<()>> + Send + 'a
    where
        S: WebSocketIo + ?Sized + 'a,
    {
        let service = self.clone();
        async move { run_connection_with_io(service, socket, user, None, Admission::Pending).await }
    }
}

/// Whether the caller already holds a connection slot.
enum Admission {
    /// `handle_upgrade` admitted the connection and holds its permit.
    Granted,
    /// Admission still has to happen (transports that bypass `handle_upgrade`).
    Pending,
}

/// Take a connection slot. `Ok(Some(permit))` when a cap is configured,
/// `Ok(None)` when the service runs unbounded, `Err(())` when the cap is
/// reached.
async fn admit_connection<Svc: WebSocketService>(
    service: &Svc,
) -> std::result::Result<Option<tokio::sync::OwnedSemaphorePermit>, ()> {
    match service.connection_permits() {
        Some(permits) => permits.try_acquire_owned().map(Some).map_err(|_| ()),
        None => Ok(None),
    }
}

async fn run_connection_with_io<Svc, S>(
    service: Svc,
    socket: &mut S,
    user: Option<ras_auth_core::AuthenticatedUser>,
    auth_token: Option<String>,
    admission: Admission,
) -> ServerResult<()>
where
    Svc: WebSocketService,
    S: WebSocketIo + ?Sized,
{
    let connection_id = ConnectionId::new();
    info!("New WebSocket connection: {}", connection_id);

    // Enforce the connection cap for transports that bypass handle_upgrade.
    let _permit = match admission {
        Admission::Granted => None,
        Admission::Pending => match admit_connection(&service).await {
            Ok(permit) => permit,
            Err(()) => {
                let _ = socket
                    .send(WebSocketIoMessage::Close(Some(
                        "connection limit reached".to_string(),
                    )))
                    .await;
                return Err(ServerError::Internal(
                    "connection limit reached".to_string(),
                ));
            }
        },
    };

    let channel_capacity = service.message_channel_capacity().max(1);
    let (message_tx, message_rx) = mpsc::channel(channel_capacity);
    let sender = ChannelMessageSender::new(connection_id, message_tx);

    let mut info = ConnectionInfo::new(connection_id);
    if let Some(user) = user.clone() {
        info.set_user(user);
    }

    let manager: Arc<dyn ConnectionManager> = service.connection_manager();
    let context = Arc::new(
        ConnectionContext::new(connection_id, sender.clone()).with_subscription_policy(
            crate::connection::SubscriptionPolicy {
                limits: service.subscription_limits(),
                accounting: service.subscription_accounting(),
                manager: Some(manager.clone()),
            },
        ),
    );
    if let Some(user) = user {
        context.set_user(user).await;
    }

    service
        .connection_manager()
        .add_connection_with_sender(info, Box::new(sender.clone()))
        .await
        .map_err(ServerError::ConnectionError)?;

    let mut handler = WebSocketHandler::new(
        service.handler(),
        context.clone(),
        message_rx,
        service.max_message_size(),
    )
    .with_connection_manager(manager)
    .with_keepalive(service.keepalive());

    // Authenticated connections re-validate their token periodically so
    // revocation/expiry takes effect without waiting for a disconnect.
    if let Some(token) = auth_token {
        handler = handler.with_auth_revalidation(AuthRevalidation {
            auth_provider: service.auth_provider(),
            token,
            interval: service.auth_revalidation_interval(),
            on_permission_change: service.on_permission_change(),
        });
    }

    let result = handler.run_with_io(socket).await;

    if let Err(e) = service
        .connection_manager()
        .remove_connection(connection_id)
        .await
    {
        error!("Failed to remove connection {}: {}", connection_id, e);
    }

    result
}

/// Builder for creating WebSocket services
#[derive(Builder)]
pub struct WebSocketServiceBuilder<H, A, M = DefaultConnectionManager> {
    /// Message handler
    handler: Arc<H>,
    /// Auth provider
    auth_provider: Arc<A>,
    /// Connection manager
    connection_manager: Option<Arc<M>>,
    /// Whether authentication is required (secure default: required)
    #[builder(default = true)]
    require_auth: bool,
    /// Maximum queued outbound messages per connection
    #[builder(default = DEFAULT_MESSAGE_CHANNEL_CAPACITY)]
    message_channel_capacity: usize,
    /// Maximum accepted inbound WebSocket message size in bytes
    #[builder(default = DEFAULT_MAX_MESSAGE_SIZE)]
    max_message_size: usize,
    /// Interval between credential re-validations for live connections
    #[builder(default = DEFAULT_AUTH_REVALIDATION_INTERVAL)]
    auth_revalidation_interval: Duration,
    /// Maximum simultaneous connections (`None` = unbounded)
    #[builder(required, default = Some(DEFAULT_MAX_CONNECTIONS))]
    max_connections: Option<usize>,
    /// Limits on client-initiated subscriptions
    #[builder(default)]
    subscription_limits: SubscriptionLimits,
    /// Server-side ping interval and idle timeout
    #[builder(default)]
    keepalive: KeepaliveConfig,
    /// Policy when permissions change on re-validation
    #[builder(default)]
    on_permission_change: PermissionChangePolicy,
}

impl<H, A> WebSocketServiceBuilder<H, A, DefaultConnectionManager>
where
    H: MessageHandler,
    A: AuthProvider,
{
    /// Build the WebSocket service with default connection manager
    pub fn build(self) -> BuiltWebSocketService<H, A, DefaultConnectionManager> {
        BuiltWebSocketService {
            handler: self.handler,
            auth_provider: self.auth_provider,
            connection_manager: self.connection_manager.unwrap_or_else(|| {
                Arc::new(DefaultConnectionManager::with_subscription_limits(
                    self.subscription_limits,
                ))
            }),
            require_auth: self.require_auth,
            message_channel_capacity: self.message_channel_capacity,
            max_message_size: self.max_message_size,
            auth_revalidation_interval: self.auth_revalidation_interval,
            max_connections: self.max_connections,
            connection_permits: self
                .max_connections
                .map(|limit| Arc::new(tokio::sync::Semaphore::new(limit))),
            subscription_limits: self.subscription_limits,
            subscription_accounting: Arc::new(SubscriptionAccounting::default()),
            keepalive: self.keepalive,
            on_permission_change: self.on_permission_change,
        }
    }
}

impl<H, A, M> WebSocketServiceBuilder<H, A, M>
where
    H: MessageHandler,
    A: AuthProvider,
    M: ConnectionManager,
{
    /// Build the WebSocket service with custom connection manager
    pub fn build_with_manager(self, manager: Arc<M>) -> BuiltWebSocketService<H, A, M> {
        BuiltWebSocketService {
            handler: self.handler,
            auth_provider: self.auth_provider,
            connection_manager: manager,
            require_auth: self.require_auth,
            message_channel_capacity: self.message_channel_capacity,
            max_message_size: self.max_message_size,
            auth_revalidation_interval: self.auth_revalidation_interval,
            max_connections: self.max_connections,
            connection_permits: self
                .max_connections
                .map(|limit| Arc::new(tokio::sync::Semaphore::new(limit))),
            subscription_limits: self.subscription_limits,
            subscription_accounting: Arc::new(SubscriptionAccounting::default()),
            keepalive: self.keepalive,
            on_permission_change: self.on_permission_change,
        }
    }
}

/// Built WebSocket service
pub struct BuiltWebSocketService<H, A, M> {
    handler: Arc<H>,
    auth_provider: Arc<A>,
    connection_manager: Arc<M>,
    require_auth: bool,
    message_channel_capacity: usize,
    max_message_size: usize,
    auth_revalidation_interval: Duration,
    max_connections: Option<usize>,
    connection_permits: Option<Arc<tokio::sync::Semaphore>>,
    subscription_limits: SubscriptionLimits,
    subscription_accounting: Arc<SubscriptionAccounting>,
    keepalive: KeepaliveConfig,
    on_permission_change: PermissionChangePolicy,
}

impl<H, A, M> Clone for BuiltWebSocketService<H, A, M> {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            auth_provider: self.auth_provider.clone(),
            connection_manager: self.connection_manager.clone(),
            require_auth: self.require_auth,
            message_channel_capacity: self.message_channel_capacity,
            max_message_size: self.max_message_size,
            auth_revalidation_interval: self.auth_revalidation_interval,
            max_connections: self.max_connections,
            // Shared, not rebuilt: every clone must draw from the same pool.
            connection_permits: self.connection_permits.clone(),
            subscription_limits: self.subscription_limits,
            subscription_accounting: self.subscription_accounting.clone(),
            keepalive: self.keepalive,
            on_permission_change: self.on_permission_change,
        }
    }
}

impl<H, A, M> WebSocketService for BuiltWebSocketService<H, A, M>
where
    H: MessageHandler + 'static,
    A: AuthProvider + 'static,
    M: ConnectionManager + 'static,
{
    type Handler = H;
    type AuthProvider = A;
    type ConnectionManager = M;

    fn handler(&self) -> Arc<Self::Handler> {
        self.handler.clone()
    }

    fn auth_provider(&self) -> Arc<Self::AuthProvider> {
        self.auth_provider.clone()
    }

    fn connection_manager(&self) -> Arc<Self::ConnectionManager> {
        self.connection_manager.clone()
    }

    fn require_auth(&self) -> bool {
        self.require_auth
    }

    fn message_channel_capacity(&self) -> usize {
        self.message_channel_capacity
    }

    fn max_message_size(&self) -> usize {
        self.max_message_size
    }

    fn auth_revalidation_interval(&self) -> Duration {
        self.auth_revalidation_interval
    }

    fn max_connections(&self) -> Option<usize> {
        self.max_connections
    }

    fn connection_permits(&self) -> Option<Arc<tokio::sync::Semaphore>> {
        self.connection_permits.clone()
    }

    fn subscription_limits(&self) -> SubscriptionLimits {
        self.subscription_limits
    }

    fn subscription_accounting(&self) -> Arc<SubscriptionAccounting> {
        self.subscription_accounting.clone()
    }

    fn keepalive(&self) -> KeepaliveConfig {
        self.keepalive
    }

    fn on_permission_change(&self) -> PermissionChangePolicy {
        self.on_permission_change
    }
}

/// Convenience function to create a simple router-based service
pub fn create_router_service<A>(
    router: MessageRouter,
    auth_provider: Arc<A>,
    require_auth: bool,
) -> BuiltWebSocketService<MessageRouter, A, DefaultConnectionManager>
where
    A: AuthProvider,
{
    let builder = WebSocketServiceBuilder::builder()
        .handler(Arc::new(router))
        .auth_provider(auth_provider)
        .require_auth(require_auth)
        .build();
    builder.build()
}

/// Axum handler function for WebSocket upgrade
pub async fn websocket_handler<S>(
    ws: AxumWebSocketUpgrade,
    headers: HeaderMap,
    State(service): State<S>,
) -> Result<Response, (axum::http::StatusCode, String)>
where
    S: WebSocketService,
{
    service.handle_upgrade(ws, headers).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::{WebSocketIo, WebSocketIoMessage};
    use async_trait::async_trait;
    use ras_auth_core::{AuthError, AuthenticatedUser};
    use ras_jsonrpc_bidirectional_types::BidirectionalMessage;
    use serde_json::json;
    use std::collections::{HashSet, VecDeque};

    // Mock auth provider for testing
    #[derive(Clone)]
    struct MockAuthProvider;

    impl AuthProvider for MockAuthProvider {
        fn authenticate(&self, token: String) -> ras_auth_core::AuthFuture<'_> {
            Box::pin(async move {
                if token == "valid_token" {
                    Ok(AuthenticatedUser {
                        user_id: "test_user".to_string(),
                        permissions: HashSet::new(),
                        metadata: None,
                    })
                } else {
                    Err(AuthError::InvalidToken)
                }
            })
        }
    }

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "test_user".to_string(),
            permissions: HashSet::new(),
            metadata: None,
        }
    }

    struct InMemorySocket {
        incoming: VecDeque<WebSocketIoMessage>,
        outgoing: Vec<WebSocketIoMessage>,
        /// When set, `recv` waits on this instead of closing once drained.
        hold_open: Option<Arc<tokio::sync::Notify>>,
    }

    impl InMemorySocket {
        fn closing(incoming: impl IntoIterator<Item = WebSocketIoMessage>) -> Self {
            Self {
                incoming: incoming.into_iter().collect(),
                outgoing: Vec::new(),
                hold_open: None,
            }
        }

        fn held_open(release: Arc<tokio::sync::Notify>) -> Self {
            Self {
                incoming: VecDeque::new(),
                outgoing: Vec::new(),
                hold_open: Some(release),
            }
        }

        fn outgoing_messages(&self) -> impl Iterator<Item = BidirectionalMessage> + '_ {
            self.outgoing.iter().filter_map(|message| match message {
                WebSocketIoMessage::Text(text) => serde_json::from_str(text).ok(),
                _ => None,
            })
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
            if let Some(release) = &self.hold_open {
                release.notified().await;
            }
            None
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn connection_cap_is_atomic_under_concurrent_admission() {
        const CAP: usize = 3;
        const ATTEMPTS: usize = 40;
        let manager = Arc::new(DefaultConnectionManager::new());
        let service = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .max_connections(Some(CAP))
            .build()
            .build_with_manager(manager.clone());

        let release = Arc::new(tokio::sync::Notify::new());
        let start = Arc::new(tokio::sync::Barrier::new(ATTEMPTS));
        let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel();
        for _ in 0..ATTEMPTS {
            let service = service.clone();
            let release = release.clone();
            let start = start.clone();
            let done_tx = done_tx.clone();
            tokio::spawn(async move {
                let mut socket = InMemorySocket::held_open(release);
                start.wait().await;
                let result = service
                    .handle_connection_with_io(&mut socket, Some(test_user()))
                    .await;
                let _ = done_tx.send(result.is_ok());
            });
        }
        drop(done_tx);

        // All refusals return immediately; the admitted ones block until released.
        let mut refused = 0;
        while refused < ATTEMPTS - CAP {
            match tokio::time::timeout(Duration::from_secs(5), done_rx.recv()).await {
                Ok(Some(false)) => refused += 1,
                Ok(Some(true)) => panic!("an admitted connection finished before release"),
                _ => panic!("timed out waiting for refusals"),
            }
        }
        assert_eq!(manager.connection_count(), CAP, "never more than the cap");

        release.notify_waiters();
        let mut admitted = 0;
        while let Ok(Some(ok)) = tokio::time::timeout(Duration::from_secs(5), done_rx.recv()).await
        {
            assert!(ok);
            admitted += 1;
        }
        assert_eq!(admitted, CAP);
    }

    #[tokio::test]
    async fn connection_cap_is_bounded_by_default() {
        let service = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .build()
            .build();
        assert_eq!(service.max_connections(), Some(DEFAULT_MAX_CONNECTIONS));
        assert!(service.connection_permits().is_some());

        let unbounded = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .max_connections(None)
            .build()
            .build();
        assert_eq!(unbounded.max_connections(), None);
    }

    #[tokio::test]
    async fn test_service_builder() {
        let router = MessageRouter::new();
        let auth_provider = Arc::new(MockAuthProvider);

        let service = create_router_service(router, auth_provider, false);

        assert!(!service.require_auth());
        assert_eq!(service.connection_manager().connection_count(), 0);
    }

    #[tokio::test]
    async fn builder_requires_auth_by_default() {
        let builder = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .build();
        let service = builder.build();

        assert!(service.require_auth());
        assert_eq!(
            service.auth_revalidation_interval(),
            Duration::from_secs(30)
        );
    }

    #[tokio::test]
    async fn builder_revalidation_interval_is_configurable() {
        let builder = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .auth_revalidation_interval(Duration::from_secs(5))
            .build();
        let service = builder.build();

        assert_eq!(service.auth_revalidation_interval(), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn connection_cap_refuses_excess_connections() {
        let manager = Arc::new(DefaultConnectionManager::new());
        let builder = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .max_connections(Some(0))
            .build();
        let service = builder.build_with_manager(manager.clone());

        let mut socket = InMemorySocket::closing([]);
        let result = service
            .handle_connection_with_io(&mut socket, Some(test_user()))
            .await;

        assert!(result.is_err(), "connection over the cap must be refused");
        assert!(
            socket
                .outgoing
                .iter()
                .any(|message| matches!(message, WebSocketIoMessage::Close(_)))
        );
        assert_eq!(manager.connection_count(), 0);
    }

    #[tokio::test]
    async fn connection_cap_admits_connections_under_the_limit() {
        let manager = Arc::new(DefaultConnectionManager::new());
        let builder = WebSocketServiceBuilder::builder()
            .handler(Arc::new(MessageRouter::new()))
            .auth_provider(Arc::new(MockAuthProvider))
            .max_connections(Some(1))
            .build();
        let service = builder.build_with_manager(manager.clone());

        let mut socket = InMemorySocket::closing([]);
        service
            .handle_connection_with_io(&mut socket, Some(test_user()))
            .await
            .expect("connection under the cap is admitted");
    }

    #[tokio::test]
    async fn test_service_with_auth_required() {
        let router = MessageRouter::new();
        let auth_provider = Arc::new(MockAuthProvider);

        let builder = WebSocketServiceBuilder::builder()
            .handler(Arc::new(router))
            .auth_provider(auth_provider)
            .require_auth(true)
            .build();
        let service = builder.build();

        assert!(service.require_auth());
    }

    #[tokio::test]
    async fn handle_connection_with_io_round_trips_and_cleans_up_without_socket() {
        let mut router = MessageRouter::new();
        router.register_value("whoami", |_req, context| async move {
            let user = context.get_user().await.expect("authenticated user");
            Ok::<_, ServerError>(json!({ "user_id": user.user_id }))
        });

        let manager = Arc::new(DefaultConnectionManager::new());
        let builder = WebSocketServiceBuilder::builder()
            .handler(Arc::new(router))
            .auth_provider(Arc::new(MockAuthProvider))
            .message_channel_capacity(2)
            .max_message_size(16 * 1024)
            .build();
        let service = builder.build_with_manager(manager.clone());

        let request =
            ras_jsonrpc_types::JsonRpcRequest::new("whoami".to_string(), None, Some(json!(1)));
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text(
            serde_json::to_string(&request).unwrap(),
        )]);

        service
            .handle_connection_with_io(&mut socket, Some(test_user()))
            .await
            .unwrap();

        assert_eq!(manager.connection_count(), 0);

        let messages = socket.outgoing_messages().collect::<Vec<_>>();
        assert!(matches!(
            messages.first(),
            Some(BidirectionalMessage::ConnectionEstablished { .. })
        ));
        assert!(matches!(
            messages.last(),
            Some(BidirectionalMessage::ConnectionClosed { .. })
        ));

        let response = messages
            .iter()
            .find_map(|message| match message {
                BidirectionalMessage::Response(response) => Some(response),
                _ => None,
            })
            .expect("JSON-RPC response");
        assert_eq!(response.id, Some(json!(1)));
        assert_eq!(
            response.result.as_ref().expect("result"),
            &json!({ "user_id": "test_user" })
        );
    }
}
