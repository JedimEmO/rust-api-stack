//! WebSocket service implementation with builder pattern

use crate::{
    ConnectionContext, DefaultConnectionManager, MessageHandler, MessageRouter, ServerError,
    ServerResult, WebSocketHandler, WebSocketUpgrade,
    connection::ChannelMessageSender,
    handler::{AxumWebSocketIo, WebSocketIo},
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
use tokio::sync::mpsc;
use tracing::{error, info};

const DEFAULT_MESSAGE_CHANNEL_CAPACITY: usize = 1024;
const DEFAULT_MAX_MESSAGE_SIZE: usize = 1024 * 1024;

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

    /// Handle WebSocket upgrade
    async fn handle_upgrade(
        &self,
        upgrade: AxumWebSocketUpgrade,
        headers: HeaderMap,
    ) -> Result<Response, (axum::http::StatusCode, String)> {
        let ws_upgrade = WebSocketUpgrade::new(upgrade, headers);
        let service = self.clone();

        ws_upgrade
            .on_upgrade_with_auth(
                &*self.auth_provider(),
                self.require_auth(),
                move |socket, user| {
                    Box::pin(async move {
                        if let Err(e) = service.handle_connection(socket, user).await {
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
    ) -> impl std::future::Future<Output = ServerResult<()>> + Send {
        let service = self.clone();
        async move {
            let mut socket = AxumWebSocketIo::new(socket);
            run_connection_with_io(service, &mut socket, user).await
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
        async move { run_connection_with_io(service, socket, user).await }
    }
}

async fn run_connection_with_io<Svc, S>(
    service: Svc,
    socket: &mut S,
    user: Option<ras_auth_core::AuthenticatedUser>,
) -> ServerResult<()>
where
    Svc: WebSocketService,
    S: WebSocketIo + ?Sized,
{
    let connection_id = ConnectionId::new();
    info!("New WebSocket connection: {}", connection_id);

    let channel_capacity = service.message_channel_capacity().max(1);
    let (message_tx, message_rx) = mpsc::channel(channel_capacity);
    let sender = ChannelMessageSender::new(connection_id, message_tx);

    let mut info = ConnectionInfo::new(connection_id);
    if let Some(user) = user.clone() {
        info.set_user(user);
    }

    let context = Arc::new(ConnectionContext::new(connection_id, sender.clone()));
    if let Some(user) = user {
        context.set_user(user).await;
    }

    service
        .connection_manager()
        .add_connection_with_sender(info, Box::new(sender.clone()))
        .await
        .map_err(ServerError::ConnectionError)?;

    let handler = WebSocketHandler::new(
        service.handler(),
        context.clone(),
        message_rx,
        service.max_message_size(),
    );

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
    /// Whether authentication is required
    #[builder(default = false)]
    require_auth: bool,
    /// Maximum queued outbound messages per connection
    #[builder(default = DEFAULT_MESSAGE_CHANNEL_CAPACITY)]
    message_channel_capacity: usize,
    /// Maximum accepted inbound WebSocket message size in bytes
    #[builder(default = DEFAULT_MAX_MESSAGE_SIZE)]
    max_message_size: usize,
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
            connection_manager: self
                .connection_manager
                .unwrap_or_else(|| Arc::new(DefaultConnectionManager::new())),
            require_auth: self.require_auth,
            message_channel_capacity: self.message_channel_capacity,
            max_message_size: self.max_message_size,
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
    }

    impl InMemorySocket {
        fn closing(incoming: impl IntoIterator<Item = WebSocketIoMessage>) -> Self {
            Self {
                incoming: incoming.into_iter().collect(),
                outgoing: Vec::new(),
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
            self.incoming.pop_front().map(Ok)
        }
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
