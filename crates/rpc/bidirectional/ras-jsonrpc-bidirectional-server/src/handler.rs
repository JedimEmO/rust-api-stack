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
        // Default implementation unsubscribes the connection from each requested topic.
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
        // Default implementation records the ping at debug level.
        debug!("Received ping");
        Ok(())
    }

    /// Handle pong message
    async fn on_pong(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        // Default implementation records the pong at debug level.
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

/// Limits on client-initiated subscriptions (W3).
///
/// Enforced by the handler loop before [`MessageHandler::handle_subscribe`]
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

        // Notify handler of connection
        if let Err(e) = self.handler.on_connect(self.context.clone()).await {
            error!("Error in on_connect handler: {}", e);
        }

        // Send connection established message
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

        // Main message handling loop
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

                // Handle incoming WebSocket messages
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

                // Handle outgoing messages
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

        // Notify handler of disconnection
        if let Err(e) = self.handler.on_disconnect(self.context.clone(), None).await {
            error!("Error in on_disconnect handler: {}", e);
        }

        // Send connection closed message
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
                // Try to parse as UTF-8 text
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
        // Try to parse as BidirectionalMessage first
        if let Ok(msg) = serde_json::from_str::<BidirectionalMessage>(&text) {
            return self.handle_bidirectional_message(msg, socket).await;
        }

        // Try to parse as JSON-RPC request
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
                // Handle as JSON-RPC request
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
                // Send response back to client
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
    // server-side by the caller. Never interpolate handler/AuthError Display (H3).
    JsonRpcError::new(code, error.client_message().to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ChannelMessageSender;
    use ras_jsonrpc_bidirectional_types::ConnectionId;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[test]
    fn jsonrpc_error_from_server_error_sends_generic_message_not_handler_detail() {
        // Handler error carrying a secret -> client sees only a generic message,
        // stable code preserved, no data field (H3).
        let err = ServerError::Internal("database password is hunter2".into());
        let jsonrpc = jsonrpc_error_from_server_error(&err);
        assert_eq!(jsonrpc.code, error_codes::INTERNAL_ERROR);
        assert_eq!(jsonrpc.message, "Internal error");
        assert!(!jsonrpc.message.contains("hunter2"));
        assert!(jsonrpc.data.is_none());

        // AuthError detail must not reach the client either.
        let auth = ServerError::AuthenticationFailed(ras_auth_core::AuthError::Internal(
            "dsn=postgres://user:pw@host/db".into(),
        ));
        let jsonrpc = jsonrpc_error_from_server_error(&auth);
        assert_eq!(jsonrpc.code, error_codes::AUTHENTICATION_REQUIRED);
        assert_eq!(jsonrpc.message, "Authentication failed");
        assert!(!jsonrpc.message.contains("dsn"));

        // Stable codes for the invalid-request / method-not-found classes.
        assert_eq!(
            jsonrpc_error_from_server_error(&ServerError::InvalidRequest(
                "Invalid params: x".into()
            ))
            .code,
            error_codes::INVALID_REQUEST
        );
        assert_eq!(
            jsonrpc_error_from_server_error(&ServerError::HandlerNotFound("m".into())).code,
            error_codes::METHOD_NOT_FOUND
        );
    }

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

    #[tokio::test]
    async fn default_handle_subscribe_denies_all_topics() {
        let h = PassThrough;
        let c = ctx();
        h.handle_subscribe(vec!["a".into(), "b".into()], c.clone())
            .await
            .unwrap();
        assert!(!c.is_subscribed_to("a").await);
        assert!(!c.is_subscribed_to("b").await);
    }

    #[tokio::test]
    async fn default_authorize_subscribe_denies() {
        let h = PassThrough;
        let c = ctx();
        assert!(!h.authorize_subscribe("any-topic", &c).await.unwrap());
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

    #[tokio::test]
    async fn handle_subscribe_only_subscribes_authorized_topics() {
        let h = AllowListHandler;
        let c = ctx();
        h.handle_subscribe(vec!["room:allowed".into(), "room:denied".into()], c.clone())
            .await
            .unwrap();
        assert!(c.is_subscribed_to("room:allowed").await);
        assert!(!c.is_subscribed_to("room:denied").await);
    }

    #[tokio::test]
    async fn default_handle_unsubscribe_removes_from_context() {
        let h = PassThrough;
        let c = ctx();
        c.subscribe("a".into()).await.unwrap();
        c.subscribe("b".into()).await.unwrap();
        h.handle_unsubscribe(vec!["a".into()], c.clone())
            .await
            .unwrap();
        assert!(!c.is_subscribed_to("a").await);
        assert!(c.is_subscribed_to("b").await);
    }

    #[tokio::test]
    async fn default_lifecycle_methods_succeed() {
        let h = PassThrough;
        let c = ctx();
        h.on_connect(c.clone()).await.unwrap();
        h.on_ping(c.clone()).await.unwrap();
        h.on_pong(c.clone()).await.unwrap();
        h.on_disconnect(c.clone(), Some("bye".into()))
            .await
            .unwrap();
        // None reason path too.
        h.on_disconnect(c, None).await.unwrap();
    }

    #[tokio::test]
    async fn handler_loop_processes_jsonrpc_request_without_socket() {
        let request = JsonRpcRequest::new(
            "echo".into(),
            Some(serde_json::json!({"value": 42})),
            Some(serde_json::json!(7)),
        );
        let incoming = serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap();
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text(incoming)]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(RespondingHandler), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        let response = match &messages[1] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected response, got {other:?}"),
        };
        assert_eq!(response.id, Some(serde_json::json!(7)));
        assert_eq!(response.result.as_ref().unwrap()["method"], "echo");
        assert_eq!(response.result.as_ref().unwrap()["params"]["value"], 42);

        assert!(matches!(
            messages[2],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_sends_jsonrpc_error_and_continues_without_socket() {
        let fail = JsonRpcRequest::new(
            "fail".into(),
            Some(serde_json::json!({})),
            Some(serde_json::json!(1)),
        );
        let ok = JsonRpcRequest::new(
            "ok".into(),
            Some(serde_json::json!({})),
            Some(serde_json::json!(2)),
        );
        let mut socket = InMemorySocket::closing([
            WebSocketIoMessage::Text(
                serde_json::to_string(&BidirectionalMessage::Request(fail)).unwrap(),
            ),
            WebSocketIoMessage::Text(
                serde_json::to_string(&BidirectionalMessage::Request(ok)).unwrap(),
            ),
        ]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(RecoveringHandler), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        let error_response = match &messages[1] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected error response, got {other:?}"),
        };
        assert_eq!(error_response.id, Some(serde_json::json!(1)));
        let error = error_response.error.as_ref().expect("JSON-RPC error");
        assert_eq!(error.code, ras_jsonrpc_types::error_codes::INVALID_REQUEST);
        // Message is the generic per-class string; the handler's detail
        // ("bad request") stays server-side (H3).
        assert_eq!(error.message, "Invalid request");

        let success_response = match &messages[2] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected success response, got {other:?}"),
        };
        assert_eq!(success_response.id, Some(serde_json::json!(2)));
        assert_eq!(success_response.result.as_ref().unwrap()["method"], "ok");

        assert!(matches!(
            messages[3],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_processes_control_messages_without_socket() {
        let context = ctx();
        let subscribe = serde_json::to_string(&BidirectionalMessage::Subscribe {
            topics: vec!["room:1".into()],
        })
        .unwrap();
        let unsubscribe = serde_json::to_string(&BidirectionalMessage::Unsubscribe {
            topics: vec!["room:1".into()],
        })
        .unwrap();
        let mut socket = InMemorySocket::closing([
            WebSocketIoMessage::Text(subscribe),
            WebSocketIoMessage::Text(unsubscribe),
            WebSocketIoMessage::Ping(vec![1, 2, 3]),
        ]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), context.clone(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(!context.is_subscribed_to("room:1").await);
        assert!(
            socket
                .outgoing
                .contains(&WebSocketIoMessage::Pong(vec![1, 2, 3]))
        );
    }

    #[tokio::test]
    async fn handler_loop_sends_manager_messages_without_socket() {
        let notification = BidirectionalMessage::ServerNotification(
            ras_jsonrpc_bidirectional_types::ServerNotification {
                method: "server.note".into(),
                params: serde_json::json!({"ok": true}),
                metadata: None,
            },
        );
        let (tx, rx) = mpsc::channel(4);
        tx.send(OutboundMessage::from(notification)).await.unwrap();
        drop(tx);

        let mut socket = InMemorySocket::pending();
        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        match &messages[1] {
            BidirectionalMessage::ServerNotification(notification) => {
                assert_eq!(notification.method, "server.note");
                assert_eq!(notification.params["ok"], true);
            }
            other => panic!("expected server notification, got {other:?}"),
        }

        assert!(matches!(
            messages[2],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_answers_malformed_text_with_parse_error_and_continues() {
        let request = JsonRpcRequest::new(
            "echo".into(),
            Some(serde_json::json!({})),
            Some(serde_json::json!(9)),
        );
        let mut socket = InMemorySocket::closing([
            WebSocketIoMessage::Text("not json-rpc".to_string()),
            WebSocketIoMessage::Text(
                serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap(),
            ),
        ]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(RespondingHandler), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        // The garbage frame is answered with -32700 (id null)...
        let parse_error = match &messages[1] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected parse error response, got {other:?}"),
        };
        assert_eq!(parse_error.id, None);
        let error = parse_error.error.as_ref().expect("parse error");
        assert_eq!(error.code, ras_jsonrpc_types::error_codes::PARSE_ERROR);

        // ...and the connection keeps serving subsequent requests.
        let response = match &messages[2] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected response, got {other:?}"),
        };
        assert_eq!(response.id, Some(serde_json::json!(9)));

        assert!(matches!(
            messages[3],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_closes_oversized_text_without_response() {
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text("too large".into())]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 4)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));
        assert!(matches!(
            messages[1],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_ignores_non_utf8_binary_without_response() {
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Binary(vec![0xff, 0xfe])]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));
        assert!(matches!(
            messages[1],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_records_close_reason_without_socket() {
        let handler = Arc::new(RecordingLifecycle::new());
        let mut socket =
            InMemorySocket::closing([WebSocketIoMessage::Close(Some("client bye".to_string()))]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(handler.clone(), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(
            handler
                .disconnect_reasons()
                .contains(&Some("client bye".to_string()))
        );
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

    #[tokio::test(start_paused = true)]
    async fn revalidation_failure_closes_connection() {
        let context = ctx();
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::pending();

        WebSocketHandler::new(Arc::new(PassThrough), context, rx, 1024)
            .with_auth_revalidation(AuthRevalidation {
                auth_provider: Arc::new(SequenceAuthProvider::new([])),
                token: "revoked-token".into(),
                interval: Duration::from_secs(30),
                on_permission_change: PermissionChangePolicy::default(),
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(socket.outgoing.iter().any(|message| matches!(
            message,
            WebSocketIoMessage::Close(Some(reason)) if reason == "credentials no longer valid"
        )));
    }

    #[tokio::test(start_paused = true)]
    async fn revalidation_success_refreshes_cached_user() {
        let context = ctx();
        context.set_user(auth_user("stale")).await;
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::pending();

        WebSocketHandler::new(Arc::new(PassThrough), context.clone(), rx, 1024)
            .with_auth_revalidation(AuthRevalidation {
                auth_provider: Arc::new(SequenceAuthProvider::new([Ok(auth_user("fresh"))])),
                token: "valid-token".into(),
                interval: Duration::from_secs(30),
                on_permission_change: PermissionChangePolicy::default(),
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        // First tick refreshed the cached user; the second (sequence
        // exhausted) failed and closed the connection.
        assert_eq!(context.get_user().await.expect("user").user_id, "fresh");
        assert!(
            socket
                .outgoing
                .iter()
                .any(|message| matches!(message, WebSocketIoMessage::Close(_)))
        );
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

    #[tokio::test(start_paused = true)]
    async fn w1_revalidation_drops_subscriptions_no_longer_authorized() {
        let context = ctx();
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let manager = manager_with(&context).await;
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::pending();
        socket
            .incoming
            .push_back(subscribe_msg(vec!["room:1".into()]));

        // Tick 1 returns the same user with the permission revoked; tick 2 fails.
        let provider = SequenceAuthProvider::new([Ok(auth_user_with("u", &[]))]);
        WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
            .with_connection_manager(manager.clone())
            .with_auth_revalidation(AuthRevalidation {
                auth_provider: Arc::new(provider),
                token: "t".into(),
                interval: Duration::from_secs(30),
                on_permission_change: PermissionChangePolicy::DropSubscriptions,
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(!context.is_subscribed_to("room:1").await);
        assert!(
            manager
                .get_subscriptions(context.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            manager
                .get_subscribed_connections("room:1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn w1_subscribe_mirrors_into_manager_index() {
        let manager: Arc<dyn ConnectionManager> = Arc::new(crate::DefaultConnectionManager::new());
        let context = ctx_with(crate::connection::SubscriptionPolicy {
            manager: Some(manager.clone()),
            ..Default::default()
        });
        manager
            .add_connection(ras_jsonrpc_bidirectional_types::ConnectionInfo::new(
                context.id,
            ))
            .await
            .unwrap();

        context.subscribe("room:1".into()).await.unwrap();
        assert!(context.is_subscribed_to("room:1").await);
        assert_eq!(
            manager.get_subscriptions(context.id).await.unwrap(),
            vec!["room:1".to_string()]
        );

        assert!(context.unsubscribe("room:1").await);
        assert!(
            manager
                .get_subscriptions(context.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(context.subscription_policy().accounting.total(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn w1_close_policy_closes_socket_when_permissions_change() {
        let context = ctx();
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::pending();

        WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
            .with_auth_revalidation(AuthRevalidation {
                auth_provider: Arc::new(SequenceAuthProvider::new([Ok(auth_user_with("u", &[]))])),
                token: "t".into(),
                interval: Duration::from_secs(30),
                on_permission_change: PermissionChangePolicy::Close,
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        // Closed on the first tick (permission change), not the second (failure).
        assert_eq!(
            socket
                .outgoing
                .iter()
                .filter(|m| matches!(m, WebSocketIoMessage::Close(_)))
                .count(),
            1
        );
        assert!(context.get_user().await.unwrap().permissions.is_empty());
    }

    #[tokio::test]
    async fn w3_subscribe_over_per_message_limit_is_rejected() {
        let context = ctx_with(limits_policy(SubscriptionLimits {
            max_topics_per_message: 2,
            ..SubscriptionLimits::default()
        }));
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let (_tx, rx) = mpsc::channel(4);
        let topics: Vec<String> = (0..3).map(|i| format!("room:{i}")).collect();
        let mut socket = InMemorySocket::closing([subscribe_msg(topics)]);

        WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(context.get_subscriptions().await.is_empty());
        assert!(bidirectional_outgoing(&socket).iter().any(|m| matches!(
            m,
            BidirectionalMessage::Response(r) if r.error.is_some()
        )));
    }

    #[tokio::test]
    async fn w3_subscribe_over_per_connection_limit_is_rejected() {
        let context = ctx_with(limits_policy(SubscriptionLimits {
            max_topics_per_connection: 1,
            ..SubscriptionLimits::default()
        }));
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::closing([
            subscribe_msg(vec!["room:1".into()]),
            subscribe_msg(vec!["room:2".into()]),
        ]);

        WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        // First subscribe accepted silently; second answered with an error.
        let errors = bidirectional_outgoing(&socket)
            .iter()
            .filter(|m| matches!(m, BidirectionalMessage::Response(r) if r.error.is_some()))
            .count();
        assert_eq!(errors, 1);
        // Teardown released the held slot.
        assert_eq!(context.subscription_policy().accounting.total(), 0);
        assert!(context.get_subscriptions().await.is_empty());

        // Direct path: the context itself refuses the second topic.
        let direct = ctx_with(limits_policy(SubscriptionLimits {
            max_topics_per_connection: 1,
            ..SubscriptionLimits::default()
        }));
        direct.subscribe("room:1".into()).await.unwrap();
        assert!(matches!(
            direct.subscribe("room:2".into()).await,
            Err(ras_jsonrpc_bidirectional_types::BidirectionalError::SubscriptionLimitReached(_))
        ));
    }

    #[tokio::test]
    async fn w3_overlong_topic_is_rejected() {
        let context = ctx();
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::closing([subscribe_msg(vec!["x".repeat(300)])]);

        WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(context.get_subscriptions().await.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn w4_idle_connection_is_pinged_then_closed() {
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::pending();

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .with_keepalive(KeepaliveConfig {
                ping_interval: Some(Duration::from_secs(5)),
                idle_timeout: Some(Duration::from_secs(12)),
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let pings = socket
            .outgoing
            .iter()
            .filter(|m| matches!(m, WebSocketIoMessage::Ping(_)))
            .count();
        assert_eq!(pings, 2, "pings at 5s and 10s before the 12s idle close");
        assert!(socket.outgoing.iter().any(|m| matches!(
            m,
            WebSocketIoMessage::Close(Some(reason)) if reason == "idle timeout"
        )));
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
            let stale = BidirectionalMessage::Broadcast(
                ras_jsonrpc_bidirectional_types::BroadcastMessage {
                    topic: topic.to_string(),
                    method: "secret".into(),
                    params: serde_json::json!({}),
                    metadata: None,
                },
            );
            context.sender.send_on_topic(topic, stale).await.unwrap();
            Ok(false)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn w1_broadcast_queued_during_revocation_window_is_not_delivered() {
        let context = ctx();
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let manager = manager_with(&context).await;
        let (tx, rx) = mpsc::channel(4);
        // The handler's context must share this channel so the authorizer
        // can enqueue through `context.sender`.
        let context = Arc::new(ConnectionContext::new(
            context.id,
            ChannelMessageSender::new(context.id, tx),
        ));
        context.set_user(auth_user_with("u", &["room:read"])).await;
        let mut socket = InMemorySocket::pending();
        socket
            .incoming
            .push_back(subscribe_msg(vec!["room:1".into()]));

        WebSocketHandler::new(Arc::new(RacingAuthorizer), context.clone(), rx, 1024)
            .with_connection_manager(manager)
            .with_auth_revalidation(AuthRevalidation {
                auth_provider: Arc::new(SequenceAuthProvider::new([Ok(auth_user_with("u", &[]))])),
                token: "t".into(),
                interval: Duration::from_secs(30),
                on_permission_change: PermissionChangePolicy::DropSubscriptions,
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(!context.is_subscribed_to("room:1").await);
        let leaked = bidirectional_outgoing(&socket)
            .iter()
            .any(|m| matches!(m, BidirectionalMessage::Broadcast(b) if b.method == "secret"));
        assert!(
            !leaked,
            "broadcast queued during the revocation window must be dropped"
        );
    }

    #[tokio::test]
    async fn w1_egress_gate_only_filters_topic_routed_messages() {
        let context = ctx();
        let (tx, rx) = mpsc::channel(4);
        let ping = BidirectionalMessage::Ping;
        tx.send(OutboundMessage::from(ping.clone())).await.unwrap();
        tx.send(OutboundMessage {
            message: ping,
            topic: Some("room:never".into()),
        })
        .await
        .unwrap();
        drop(tx);

        let mut socket = InMemorySocket::pending();
        WebSocketHandler::new(Arc::new(PassThrough), context, rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let pings = bidirectional_outgoing(&socket)
            .iter()
            .filter(|m| matches!(m, BidirectionalMessage::Ping))
            .count();
        assert_eq!(
            pings, 1,
            "untagged delivered, topic-tagged unsubscribed dropped"
        );
    }

    #[tokio::test]
    async fn w3_global_subscription_cap_is_enforced_across_connections() {
        // Service-level accounting shared by both contexts. The first
        // connection stays open (pending socket) so its slots remain held
        // while the second connection tries to subscribe.
        let limits = SubscriptionLimits {
            max_total_subscriptions: 2,
            ..SubscriptionLimits::default()
        };
        let accounting = Arc::new(SubscriptionAccounting::default());
        let policy = crate::connection::SubscriptionPolicy {
            limits,
            accounting: accounting.clone(),
            manager: None,
        };

        let first = ctx_with(policy.clone());
        first.set_user(auth_user_with("u", &["room:read"])).await;
        let (_tx1, rx1) = mpsc::channel(4);
        let mut socket1 = InMemorySocket::pending();
        socket1
            .incoming
            .push_back(subscribe_msg(vec!["a".into(), "b".into()]));
        let first_run = {
            let first = first.clone();
            tokio::spawn(async move {
                WebSocketHandler::new(Arc::new(PermissionGated), first, rx1, 1024)
                    .with_keepalive(KeepaliveConfig {
                        ping_interval: None,
                        idle_timeout: None,
                    })
                    .run_with_io(&mut socket1)
                    .await
            })
        };
        for _ in 0..100 {
            if accounting.total() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(first.get_subscriptions().await.len(), 2);

        let second = ctx_with(policy);
        second.set_user(auth_user_with("u", &["room:read"])).await;
        let (_tx2, rx2) = mpsc::channel(4);
        let mut socket2 = InMemorySocket::closing([subscribe_msg(vec!["c".into()])]);
        WebSocketHandler::new(Arc::new(PermissionGated), second.clone(), rx2, 1024)
            .run_with_io(&mut socket2)
            .await
            .unwrap();

        assert!(second.get_subscriptions().await.is_empty());
        assert_eq!(accounting.total(), 2, "second connection reserved nothing");
        first_run.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn w4_zero_durations_do_not_panic() {
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::pending();

        // Zero ping and idle: both disabled; zero revalidation: default used.
        // The sequence provider fails on its first tick, which closes the loop.
        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .with_keepalive(KeepaliveConfig {
                ping_interval: Some(Duration::ZERO),
                idle_timeout: Some(Duration::ZERO),
            })
            .with_auth_revalidation(AuthRevalidation {
                auth_provider: Arc::new(SequenceAuthProvider::new([])),
                token: "t".into(),
                interval: Duration::ZERO,
                on_permission_change: PermissionChangePolicy::default(),
            })
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(
            !socket
                .outgoing
                .iter()
                .any(|m| matches!(m, WebSocketIoMessage::Ping(_)))
        );
        assert!(socket.outgoing.iter().any(|m| matches!(
            m,
            WebSocketIoMessage::Close(Some(reason)) if reason == "credentials no longer valid"
        )));
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

    #[tokio::test]
    async fn w3_custom_handler_cannot_exceed_limits_from_any_callback() {
        let limits = SubscriptionLimits {
            max_topics_per_connection: 3,
            ..SubscriptionLimits::default()
        };
        let manager: Arc<dyn ConnectionManager> = Arc::new(
            crate::DefaultConnectionManager::with_subscription_limits(limits),
        );
        let accounting = Arc::new(SubscriptionAccounting::default());
        let context = ctx_with(crate::connection::SubscriptionPolicy {
            limits,
            accounting: accounting.clone(),
            manager: Some(manager.clone()),
        });
        manager
            .add_connection(ras_jsonrpc_bidirectional_types::ConnectionInfo::new(
                context.id,
            ))
            .await
            .unwrap();
        let (_tx, rx) = mpsc::channel(4);
        let mut socket = InMemorySocket::closing([subscribe_msg(vec!["x".into()])]);

        WebSocketHandler::new(Arc::new(GreedyHandler), context.clone(), rx, 1024)
            .with_connection_manager(manager.clone())
            .run_with_io(&mut socket)
            .await
            .unwrap();

        // Greedy on_connect (10), handle_request-free, then greedy
        // handle_subscribe (10 more): the context admitted three in total,
        // the manager saw exactly those, and disconnect released exactly
        // those, so the counter is back to zero, not underflowed.
        assert_eq!(
            context.get_subscriptions().await.len(),
            0,
            "released on disconnect"
        );
        assert_eq!(
            manager.get_subscriptions(context.id).await.unwrap().len(),
            0
        );
        assert_eq!(accounting.total(), 0, "no underflow");
        assert_eq!(manager.total_subscription_count().await.unwrap(), 0);
        assert_eq!(
            GREEDY_ACCEPTED.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "exactly the cap accepted across on_connect and handle_subscribe"
        );
    }

    #[tokio::test]
    async fn handler_without_revalidation_does_not_authenticate() {
        // No auth provider involved at all: the loop must terminate on
        // socket close without ticking a revalidation timer.
        let mut socket = InMemorySocket::closing([]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert_eq!(messages.len(), 2);
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
}
