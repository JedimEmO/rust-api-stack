//! Message handlers for WebSocket communication

use crate::{ConnectionContext, ServerError, ServerResult, connection::OutboundMessage};
use async_trait::async_trait;
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::stream::StreamExt;
use ras_auth_core::AuthProvider;
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionManager};
use ras_jsonrpc_types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, error_codes};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Trait for handling JSON-RPC requests within a WebSocket context
#[async_trait]
pub trait MessageHandler: Send + Sync + 'static {
    /// Handle an incoming JSON-RPC request
    ///
    /// # Arguments
    /// * `request` - The JSON-RPC request to handle
    /// * `context` - The connection context containing auth info and metadata
    ///
    /// # Returns
    /// * `Ok(Some(response))` - Response to send back to client
    /// * `Ok(None)` - No response needed (for notifications)
    /// * `Err(error)` - Error occurred during handling
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>>;

    /// Decide whether this connection may subscribe to `topic`.
    ///
    /// Default-deny: services that broadcast over topics must override this
    /// (or `handle_subscribe`) to allow the topics a connection is entitled
    /// to. Errors propagate to the handler loop and close the connection.
    async fn authorize_subscribe(
        &self,
        _topic: &str,
        _context: &Arc<ConnectionContext>,
    ) -> ServerResult<bool> {
        Ok(false)
    }

    /// Handle subscription requests
    async fn handle_subscribe(
        &self,
        topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        // Default implementation subscribes the connection to each topic the
        // service authorizes via `authorize_subscribe`; denied topics are
        // skipped without closing the connection.
        for topic in topics {
            if self.authorize_subscribe(&topic, &context).await? {
                if let Err(e) = context.subscribe(topic.clone()).await {
                    warn!(
                        "Refused subscription to topic '{}' for connection {}: {}",
                        topic, context.id, e
                    );
                }
            } else {
                warn!(
                    "Denied subscription to topic '{}' for connection {}",
                    topic, context.id
                );
            }
        }
        Ok(())
    }

    /// Handle unsubscription requests
    async fn handle_unsubscribe(
        &self,
        topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        for topic in topics {
            context.unsubscribe(&topic).await;
        }
        Ok(())
    }

    /// Handle connection established event
    async fn on_connect(&self, context: Arc<ConnectionContext>) -> ServerResult<()> {
        info!("Connection established: {}", context.id);
        Ok(())
    }

    /// Handle connection closed event
    async fn on_disconnect(
        &self,
        context: Arc<ConnectionContext>,
        reason: Option<String>,
    ) -> ServerResult<()> {
        info!("Connection closed: {} (reason: {:?})", context.id, reason);
        Ok(())
    }

    /// Handle ping message
    async fn on_ping(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        debug!("Received ping");
        Ok(())
    }

    /// Handle pong message
    async fn on_pong(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        debug!("Received pong");
        Ok(())
    }
}

/// WebSocket message shape used by the server handler loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketIoMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<String>),
}

impl From<Message> for WebSocketIoMessage {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => Self::Text(text.to_string()),
            Message::Binary(data) => Self::Binary(data.to_vec()),
            Message::Ping(data) => Self::Ping(data.to_vec()),
            Message::Pong(data) => Self::Pong(data.to_vec()),
            Message::Close(frame) => Self::Close(frame.map(|frame| frame.reason.to_string())),
        }
    }
}

/// Minimal socket interface used by the message loop.
#[async_trait]
pub trait WebSocketIo: Send {
    async fn send(&mut self, message: WebSocketIoMessage) -> ServerResult<()>;
    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>>;
}

pub(crate) struct AxumWebSocketIo {
    socket: WebSocket,
}

impl AxumWebSocketIo {
    pub(crate) fn new(socket: WebSocket) -> Self {
        Self { socket }
    }
}

#[async_trait]
impl WebSocketIo for AxumWebSocketIo {
    async fn send(&mut self, message: WebSocketIoMessage) -> ServerResult<()> {
        let message = match message {
            WebSocketIoMessage::Text(text) => Message::Text(text.into()),
            WebSocketIoMessage::Binary(data) => Message::Binary(data.into()),
            WebSocketIoMessage::Ping(data) => Message::Ping(data.into()),
            WebSocketIoMessage::Pong(data) => Message::Pong(data.into()),
            WebSocketIoMessage::Close(reason) => Message::Close(reason.map(|reason| CloseFrame {
                code: axum::extract::ws::close_code::NORMAL,
                reason: reason.into(),
            })),
        };

        self.socket
            .send(message)
            .await
            .map_err(|e| ServerError::WebSocketError(e.to_string()))
    }

    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
        self.socket.next().await.map(|message| {
            message
                .map(WebSocketIoMessage::from)
                .map_err(|e| ServerError::WebSocketError(e.to_string()))
        })
    }
}

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
/// [`MessageHandler::authorize_subscribe`] against the refreshed user and
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

pub use crate::subscriptions::{SubscriptionAccounting, SubscriptionLimits};

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

/// WebSocket connection handler that manages the message flow
pub struct WebSocketHandler<H: MessageHandler> {
    /// The message handler for processing requests
    handler: Arc<H>,
    /// Connection context
    context: Arc<ConnectionContext>,
    /// Channel for receiving messages to send to client
    message_rx: mpsc::Receiver<OutboundMessage>,
    max_message_size: usize,
    /// Optional periodic credential re-validation
    auth_revalidation: Option<AuthRevalidation>,
    /// Connection manager kept in step with the cached user on re-validation
    /// (None when running without a manager, e.g. unit tests). Subscription
    /// mirroring goes through the context's `SubscriptionPolicy`.
    connection_manager: Option<Arc<dyn ConnectionManager>>,
    keepalive: KeepaliveConfig,
}

impl<H: MessageHandler> WebSocketHandler<H> {
    /// Create a new WebSocket handler
    pub fn new(
        handler: Arc<H>,
        context: Arc<ConnectionContext>,
        message_rx: mpsc::Receiver<OutboundMessage>,
        max_message_size: usize,
    ) -> Self {
        Self {
            handler,
            context,
            message_rx,
            max_message_size,
            auth_revalidation: None,
            connection_manager: None,
            keepalive: KeepaliveConfig::default(),
        }
    }

    /// Enable periodic credential re-validation for this connection.
    pub fn with_auth_revalidation(mut self, revalidation: AuthRevalidation) -> Self {
        self.auth_revalidation = Some(revalidation);
        self
    }

    /// Keep the manager's cached user in step on re-validation.
    pub fn with_connection_manager(mut self, manager: Arc<dyn ConnectionManager>) -> Self {
        self.connection_manager = Some(manager);
        self
    }

    /// Override the default keepalive settings.
    pub fn with_keepalive(mut self, keepalive: KeepaliveConfig) -> Self {
        self.keepalive = keepalive;
        self
    }

    /// Re-run authentication and re-authorize every held subscription.
    ///
    /// Returns `Ok(true)` to keep the connection, `Ok(false)` to close it.
    async fn revalidate_credentials(&mut self) -> ServerResult<bool> {
        let revalidation = self
            .auth_revalidation
            .as_ref()
            .expect("revalidation timer implies config");
        let user = match revalidation
            .auth_provider
            .authenticate(revalidation.token.clone())
            .await
        {
            Ok(user) => user,
            Err(e) => {
                warn!(
                    "Closing connection {}: credential re-validation failed: {}",
                    self.context.id, e
                );
                return Ok(false);
            }
        };

        let previous = self.context.get_user().await;
        let permissions_changed = previous
            .as_ref()
            .map(|prev| prev.permissions != user.permissions || prev.user_id != user.user_id)
            .unwrap_or(true);

        // Refresh cached identity/permissions in both stores
        self.context.set_user(user.clone()).await;
        if let Some(manager) = &self.connection_manager {
            let _ = manager.set_connection_user(self.context.id, user).await;
        }

        if permissions_changed && revalidation.on_permission_change == PermissionChangePolicy::Close
        {
            warn!(
                "Closing connection {}: permissions changed on re-validation",
                self.context.id
            );
            return Ok(false);
        }

        // Re-authorize held subscriptions against the refreshed user (W1)
        for topic in self.context.get_subscriptions().await {
            if !self
                .handler
                .authorize_subscribe(&topic, &self.context)
                .await?
            {
                warn!(
                    "Dropping subscription to '{}' on connection {}: no longer authorized",
                    topic, self.context.id
                );
                self.context.unsubscribe(&topic).await;
            }
        }
        Ok(true)
    }

    /// Fast-path validation of a subscribe/unsubscribe request so the client
    /// gets an error response. The authoritative checks live in
    /// `ConnectionContext::subscribe`.
    fn check_subscription_limits(&self, topics: &[String]) -> Result<(), &'static str> {
        let limits = &self.context.subscription_policy().limits;
        if topics.len() > limits.max_topics_per_message {
            return Err("too many topics in one message");
        }
        if topics
            .iter()
            .any(|topic| topic.len() > limits.max_topic_length || topic.is_empty())
        {
            return Err("topic name length out of range");
        }
        Ok(())
    }

    /// Run the WebSocket handler loop
    pub async fn run(self, socket: WebSocket) -> ServerResult<()> {
        let mut socket = AxumWebSocketIo::new(socket);
        self.run_with_io(&mut socket).await
    }

    /// Run the handler loop over an already-upgraded socket implementation.
    pub async fn run_with_io<S: WebSocketIo + ?Sized>(
        mut self,
        socket: &mut S,
    ) -> ServerResult<()> {
        info!(
            "Starting WebSocket handler for connection: {}",
            self.context.id
        );

        if let Err(e) = self.handler.on_connect(self.context.clone()).await {
            error!("Error in on_connect handler: {}", e);
        }

        let established_msg = BidirectionalMessage::ConnectionEstablished {
            connection_id: self.context.id,
        };
        if let Err(e) = socket
            .send(WebSocketIoMessage::Text(serde_json::to_string(
                &established_msg,
            )?))
            .await
        {
            error!("Failed to send connection established message: {}", e);
        }

        let mut revalidation_timer = self.auth_revalidation.as_ref().map(|revalidation| {
            // tokio panics on a zero period; a zero interval is a config
            // error, not a request to hammer the provider. Fall back to the
            // default rather than disabling re-validation.
            let interval = if revalidation.interval.is_zero() {
                warn!(
                    "auth re-validation interval is zero; using default {:?}",
                    DEFAULT_AUTH_REVALIDATION_INTERVAL
                );
                DEFAULT_AUTH_REVALIDATION_INTERVAL
            } else {
                revalidation.interval
            };
            let mut timer =
                tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            timer
        });

        let mut ping_timer = self
            .keepalive
            .ping_interval
            .filter(|interval| {
                if interval.is_zero() {
                    warn!("keepalive ping interval is zero; pings disabled");
                }
                !interval.is_zero()
            })
            .map(|interval| {
                let mut timer =
                    tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
                timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                timer
            });
        let idle_timeout = self.keepalive.idle_timeout.filter(|timeout| {
            if timeout.is_zero() {
                warn!("keepalive idle timeout is zero; idle timeout disabled");
            }
            !timeout.is_zero()
        });
        let idle_deadline = tokio::time::sleep(idle_timeout.unwrap_or(Duration::from_secs(0)));
        tokio::pin!(idle_deadline);

        loop {
            tokio::select! {
                // Re-validate credentials so revoked/expired tokens are
                // bounded to at most one interval on a long-lived connection
                _ = async { revalidation_timer.as_mut().expect("guarded by is_some").tick().await },
                    if revalidation_timer.is_some() =>
                {
                    match self.revalidate_credentials().await {
                        Ok(true) => {}
                        Ok(false) => {
                            let _ = socket
                                .send(WebSocketIoMessage::Close(Some(
                                    "credentials no longer valid".to_string(),
                                )))
                                .await;
                            break;
                        }
                        Err(e) => {
                            error!("Error re-authorizing subscriptions: {}", e);
                            break;
                        }
                    }
                }

                // Server-initiated keepalive ping
                _ = async { ping_timer.as_mut().expect("guarded by is_some").tick().await },
                    if ping_timer.is_some() =>
                {
                    if let Err(e) = socket.send(WebSocketIoMessage::Ping(Vec::new())).await {
                        error!("Error sending keepalive ping: {}", e);
                        break;
                    }
                }

                // Idle timeout: no inbound frame for the configured period
                _ = &mut idle_deadline, if idle_timeout.is_some() => {
                    warn!(
                        "Closing connection {}: idle for {:?}",
                        self.context.id,
                        idle_timeout.expect("guarded")
                    );
                    let _ = socket
                        .send(WebSocketIoMessage::Close(Some("idle timeout".to_string())))
                        .await;
                    break;
                }

                msg = socket.recv() => {
                    if let Some(timeout) = idle_timeout {
                        idle_deadline
                            .as_mut()
                            .reset(tokio::time::Instant::now() + timeout);
                    }
                    match msg {
                        Some(Ok(msg)) => {
                            if let Err(e) = self.handle_websocket_message(msg, socket).await {
                                error!("Error handling WebSocket message: {}", e);
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            debug!("WebSocket connection closed by client");
                            break;
                        }
                    }
                }

                msg = self.message_rx.recv() => {
                    match msg {
                        Some(OutboundMessage { message, topic }) => {
                            // Egress gate: a message routed on a topic while
                            // the subscription was still in the manager index
                            // is dropped here if the connection no longer
                            // holds it, closing the window between
                            // re-authorization and index removal.
                            if let Some(topic) = topic
                                && !self.context.is_subscribed_to(&topic).await
                            {
                                debug!(
                                    "Dropping message on '{}' for connection {}: not subscribed",
                                    topic, self.context.id
                                );
                                continue;
                            }
                            if let Err(e) = self.send_message(socket, message).await {
                                error!("Error sending message: {}", e);
                                break;
                            }
                        }
                        None => {
                            debug!("Message channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Return this connection's subscription slots to the service pool
        self.context.release_all_subscriptions().await;

        if let Err(e) = self.handler.on_disconnect(self.context.clone(), None).await {
            error!("Error in on_disconnect handler: {}", e);
        }

        let closed_msg = BidirectionalMessage::ConnectionClosed {
            connection_id: self.context.id,
            reason: None,
        };
        let _ = socket
            .send(WebSocketIoMessage::Text(serde_json::to_string(
                &closed_msg,
            )?))
            .await;

        info!(
            "WebSocket handler finished for connection: {}",
            self.context.id
        );
        Ok(())
    }

    /// Handle incoming WebSocket messages
    async fn handle_websocket_message<S: WebSocketIo + ?Sized>(
        &mut self,
        msg: WebSocketIoMessage,
        socket: &mut S,
    ) -> ServerResult<()> {
        match msg {
            WebSocketIoMessage::Text(text) => {
                if text.len() > self.max_message_size {
                    warn!("Received oversized text message: {} bytes", text.len());
                    return Err(ServerError::InvalidRequest(
                        "Message exceeds maximum size".to_string(),
                    ));
                }
                debug!("Received text message ({} bytes)", text.len());
                self.handle_text_message(text, socket).await
            }
            WebSocketIoMessage::Binary(data) => {
                if data.len() > self.max_message_size {
                    warn!("Received oversized binary message: {} bytes", data.len());
                    return Err(ServerError::InvalidRequest(
                        "Message exceeds maximum size".to_string(),
                    ));
                }
                debug!("Received binary message ({} bytes)", data.len());
                match String::from_utf8(data) {
                    Ok(text) => self.handle_text_message(text, socket).await,
                    Err(_) => {
                        warn!("Received non-UTF-8 binary message, ignoring");
                        Ok(())
                    }
                }
            }
            WebSocketIoMessage::Ping(data) => {
                debug!("Received ping");
                socket.send(WebSocketIoMessage::Pong(data)).await?;
                self.handler.on_ping(self.context.clone()).await
            }
            WebSocketIoMessage::Pong(_) => {
                debug!("Received pong");
                self.handler.on_pong(self.context.clone()).await
            }
            WebSocketIoMessage::Close(reason) => {
                debug!("Received close frame: {:?}", reason);
                self.handler
                    .on_disconnect(self.context.clone(), reason.clone())
                    .await?;
                Err(ServerError::WebSocketError("Connection closed".to_string()))
            }
        }
    }

    /// Handle text messages (JSON-RPC or bidirectional messages)
    async fn handle_text_message<S: WebSocketIo + ?Sized>(
        &mut self,
        text: String,
        socket: &mut S,
    ) -> ServerResult<()> {
        if let Ok(msg) = serde_json::from_str::<BidirectionalMessage>(&text) {
            return self.handle_bidirectional_message(msg, socket).await;
        }

        if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&text) {
            return self.handle_jsonrpc_request(request, socket).await;
        }

        // Neither shape parsed. Per JSON-RPC 2.0, answer with a Parse Error
        // (-32700, id null) and keep the connection open; only transport
        // failures terminate the handler loop.
        warn!(
            "Could not parse message as JSON-RPC or bidirectional message on connection {}",
            self.context.id
        );
        let response = JsonRpcResponse::error(JsonRpcError::parse_error(), None);
        self.send_message(socket, BidirectionalMessage::Response(response))
            .await
    }

    /// Handle bidirectional messages
    async fn handle_bidirectional_message<S: WebSocketIo + ?Sized>(
        &mut self,
        msg: BidirectionalMessage,
        _socket: &mut S,
    ) -> ServerResult<()> {
        match msg {
            BidirectionalMessage::Request(request) => {
                self.handle_jsonrpc_request(request, _socket).await
            }
            BidirectionalMessage::Subscribe { topics } => {
                let before = self.context.get_subscriptions().await;
                let new = topics.iter().filter(|t| !before.contains(t)).count();
                let policy = self.context.subscription_policy();
                let mut limit_error = self.check_subscription_limits(&topics).err().or_else(|| {
                    (before.len() + new > policy.limits.max_topics_per_connection)
                        .then_some("subscription limit for this connection reached")
                });
                if limit_error.is_none()
                    && policy.limits.max_total_subscriptions > 0
                    && policy.accounting.total() + new > policy.limits.max_total_subscriptions
                {
                    limit_error = Some("global subscription limit reached");
                }
                if let Some(reason) = limit_error {
                    warn!(
                        "Rejected subscribe on connection {}: {}",
                        self.context.id, reason
                    );
                    let response = JsonRpcResponse::error(
                        JsonRpcError::invalid_params(reason.to_string()),
                        None,
                    );
                    return self
                        .send_message(_socket, BidirectionalMessage::Response(response))
                        .await;
                }
                self.handler
                    .handle_subscribe(topics, self.context.clone())
                    .await
            }
            BidirectionalMessage::Unsubscribe { topics } => {
                if let Err(reason) = self.check_subscription_limits(&topics) {
                    warn!(
                        "Rejected unsubscribe on connection {}: {}",
                        self.context.id, reason
                    );
                    return Ok(());
                }
                self.handler
                    .handle_unsubscribe(topics, self.context.clone())
                    .await
            }
            BidirectionalMessage::Ping => self.handler.on_ping(self.context.clone()).await,
            BidirectionalMessage::Pong => self.handler.on_pong(self.context.clone()).await,
            // Other message types are typically server-to-client
            _ => {
                warn!("Received unexpected bidirectional message type from client");
                Ok(())
            }
        }
    }

    /// Handle JSON-RPC requests
    async fn handle_jsonrpc_request<S: WebSocketIo + ?Sized>(
        &mut self,
        request: JsonRpcRequest,
        socket: &mut S,
    ) -> ServerResult<()> {
        debug!("Handling JSON-RPC request: {}", request.method);
        let request_id = request.id.clone();

        match self
            .handler
            .handle_request(request, self.context.clone())
            .await
        {
            Ok(Some(response)) => {
                let response_msg = BidirectionalMessage::Response(response);
                self.send_message(socket, response_msg).await
            }
            Ok(None) => {
                // No response needed (notification)
                Ok(())
            }
            Err(e) => {
                error!("Error handling request: {}", e);
                let response =
                    JsonRpcResponse::error(jsonrpc_error_from_server_error(&e), request_id);
                self.send_message(socket, BidirectionalMessage::Response(response))
                    .await
            }
        }
    }

    /// Send a message to the WebSocket client
    async fn send_message<S: WebSocketIo + ?Sized>(
        &self,
        socket: &mut S,
        msg: BidirectionalMessage,
    ) -> ServerResult<()> {
        let json = serde_json::to_string(&msg)?;
        socket.send(WebSocketIoMessage::Text(json)).await
    }
}

fn jsonrpc_error_from_server_error(error: &ServerError) -> JsonRpcError {
    let code = match error {
        ServerError::AuthenticationFailed(_) => error_codes::AUTHENTICATION_REQUIRED,
        ServerError::PermissionDenied(_) => error_codes::INSUFFICIENT_PERMISSIONS,
        ServerError::InvalidRequest(_) => error_codes::INVALID_REQUEST,
        ServerError::HandlerNotFound(_) => error_codes::METHOD_NOT_FOUND,
        ServerError::SerializationError(_) => error_codes::INVALID_PARAMS,
        ServerError::UpgradeFailed(_)
        | ServerError::ConnectionNotFound(_)
        | ServerError::RoutingFailed(_)
        | ServerError::WebSocketError(_)
        | ServerError::ConnectionError(_)
        | ServerError::Internal(_) => error_codes::INTERNAL_ERROR,
    };

    // Send only a generic per-class message; the full error was already logged
    // server-side by the caller. Never interpolate handler/AuthError Display.
    JsonRpcError::new(code, error.client_message().to_string(), None)
}

#[cfg(test)]
mod tests;
