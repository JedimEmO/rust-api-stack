//! Message handlers for WebSocket communication

use crate::{
    ConnectionContext, ServerError, ServerResult,
    connection::{ChannelMessageSender, OutboundMessage},
};
use axum::extract::ws::WebSocket;
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionManager};
use ras_jsonrpc_types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, error_codes};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

mod config;
mod contract;
mod io;
pub use crate::subscriptions::{SubscriptionAccounting, SubscriptionLimits};
pub use config::{
    AuthRevalidation, DEFAULT_AUTH_REVALIDATION_INTERVAL, KeepaliveConfig, PermissionChangePolicy,
};
pub use contract::MessageHandler;
pub(crate) use io::AxumWebSocketIo;
pub use io::{WebSocketIo, WebSocketIoMessage};

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
    async fn revalidate_credentials(&self) -> ServerResult<bool> {
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
    ///
    /// Every handler call and socket write runs under supervision: a forced
    /// disconnect, a due credential re-validation and (for writes) the idle
    /// timeout still take effect while it waits, so a stalled peer or a slow
    /// handler cannot suspend them.
    pub async fn run_with_io<S: WebSocketIo + ?Sized>(
        mut self,
        socket: &mut S,
    ) -> ServerResult<()> {
        info!(
            "Starting WebSocket handler for connection: {}",
            self.context.id
        );

        let mut life = Lifecycle::new(self.auth_revalidation.as_ref(), &self.keepalive);
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

        // Whether the socket can still take a write. False once a write was
        // abandoned part-way or failed.
        let mut socket_ok = true;

        let stop = 'run: {
            match self
                .supervise(
                    &mut life,
                    false,
                    self.handler.on_connect(self.context.clone()),
                )
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => error!("Error in on_connect handler: {}", e),
                Err(stop) => break 'run stop,
            }

            let established = text_message(&BidirectionalMessage::ConnectionEstablished {
                connection_id: self.context.id,
            })?;
            if let Err(stop) = self.write(&mut life, socket, established).await {
                socket_ok = false;
                break 'run stop;
            }

            loop {
                let message_rx = &mut self.message_rx;
                let event = tokio::select! {
                    // Lifecycle first, so an overdue re-validation or a forced
                    // disconnect runs before the next message is processed.
                    biased;
                    interrupt = life.next_interrupt(&self.context.sender, true) => {
                        Event::Interrupt(interrupt)
                    }
                    event = async {
                        tokio::select! {
                            _ = async { ping_timer.as_mut().expect("guarded by is_some").tick().await },
                                if ping_timer.is_some() => Event::Ping,
                            message = socket.recv() => Event::Inbound(message),
                            message = message_rx.recv() => Event::Outbound(message),
                        }
                    } => event,
                };

                let outgoing = match event {
                    Event::Interrupt(Interrupt::Closed) => break Stop::Disconnected,
                    Event::Interrupt(Interrupt::Idle) => break Stop::Idle,
                    Event::Interrupt(Interrupt::Revalidate) => {
                        match self.revalidate_credentials().await {
                            Ok(true) => continue,
                            Ok(false) => break Stop::Revoked,
                            Err(e) => break Stop::Failed(e),
                        }
                    }
                    Event::Ping => WebSocketIoMessage::Ping(Vec::new()),
                    Event::Inbound(None) => {
                        debug!("WebSocket connection closed by client");
                        break Stop::Ended;
                    }
                    Event::Inbound(Some(Err(e))) => break Stop::Failed(e),
                    Event::Inbound(Some(Ok(message))) => {
                        life.touch();
                        let reply = self
                            .supervise(&mut life, false, self.handle_websocket_message(message))
                            .await;
                        // The idle clock measures silence while listening;
                        // the handler's run time is not the peer's silence.
                        life.touch();
                        match reply {
                            Ok(Ok(Some(reply))) => reply,
                            Ok(Ok(None)) => continue,
                            Ok(Err(e)) => break Stop::Failed(e),
                            Err(stop) => break stop,
                        }
                    }
                    Event::Outbound(None) => {
                        debug!("Message channel closed");
                        break Stop::Ended;
                    }
                    Event::Outbound(Some(OutboundMessage { message, topic })) => {
                        if !self.passes_egress_gate(topic.as_deref()).await {
                            continue;
                        }
                        text_message(&message)?
                    }
                };

                if let Err(stop) = self.write(&mut life, socket, outgoing).await {
                    socket_ok = false;
                    break stop;
                }
            }
        };

        let close_reason = match &stop {
            Stop::Ended => None,
            Stop::Disconnected => Some("disconnected by server"),
            Stop::Revoked => Some("credentials no longer valid"),
            Stop::Idle => Some("idle timeout"),
            Stop::Failed(e) => {
                error!(
                    "Closing connection {} after an error: {}",
                    self.context.id, e
                );
                None
            }
        };
        match &stop {
            Stop::Disconnected => info!("Connection {} disconnected by server", self.context.id),
            Stop::Revoked => warn!(
                "Closing connection {}: credentials no longer valid",
                self.context.id
            ),
            Stop::Idle => warn!(
                "Closing connection {}: idle for {:?}",
                self.context.id, self.keepalive.idle_timeout
            ),
            Stop::Ended | Stop::Failed(_) => {}
        }

        if socket_ok {
            let drain = matches!(stop, Stop::Disconnected);
            let _ = tokio::time::timeout(CLOSE_GRACE, async {
                // A forced disconnect still delivers what was queued before
                // it, such as a notice telling the client why.
                if drain {
                    self.drain_outbound(socket).await;
                }
                if let Some(reason) = close_reason {
                    let _ = socket
                        .send(WebSocketIoMessage::Close(Some(reason.to_string())))
                        .await;
                }
            })
            .await;
        }

        // Refuse sends from anything still holding this connection's sender.
        self.context.sender.close();

        // Return this connection's subscription slots to the service pool
        self.context.release_all_subscriptions().await;

        if let Err(e) = self.handler.on_disconnect(self.context.clone(), None).await {
            error!("Error in on_disconnect handler: {}", e);
        }

        // A connection closed for a security reason keeps no identity, so a
        // clone of its context held elsewhere authorizes nothing.
        if matches!(stop, Stop::Disconnected | Stop::Revoked) {
            self.context.clear_user().await;
        }

        if socket_ok {
            let closed_msg = text_message(&BidirectionalMessage::ConnectionClosed {
                connection_id: self.context.id,
                reason: None,
            })?;
            let _ = tokio::time::timeout(CLOSE_GRACE, socket.send(closed_msg)).await;
        }

        info!(
            "WebSocket handler finished for connection: {}",
            self.context.id
        );
        Ok(())
    }

    /// Run `work` while the connection's lifecycle keeps going: a due
    /// re-validation runs (pausing `work`), and a forced disconnect, failed
    /// re-validation or, with `watch_idle`, the idle timeout abandons `work`.
    async fn supervise<T>(
        &self,
        life: &mut Lifecycle,
        watch_idle: bool,
        work: impl Future<Output = T>,
    ) -> Result<T, Stop> {
        tokio::pin!(work);
        loop {
            let interrupt = tokio::select! {
                // Work first: a handler that disconnects its own connection
                // and then returns still gets its response out.
                biased;
                output = &mut work => return Ok(output),
                interrupt = life.next_interrupt(&self.context.sender, watch_idle) => interrupt,
            };
            match interrupt {
                Interrupt::Closed => return Err(Stop::Disconnected),
                Interrupt::Idle => return Err(Stop::Idle),
                Interrupt::Revalidate => match self.revalidate_credentials().await {
                    Ok(true) => {}
                    Ok(false) => return Err(Stop::Revoked),
                    Err(e) => return Err(Stop::Failed(e)),
                },
            }
        }
    }

    /// Write one message under supervision. A write that makes no progress
    /// for the idle timeout is abandoned.
    async fn write<S: WebSocketIo + ?Sized>(
        &self,
        life: &mut Lifecycle,
        socket: &mut S,
        message: WebSocketIoMessage,
    ) -> Result<(), Stop> {
        match self.supervise(life, true, socket.send(message)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(Stop::Failed(e)),
            Err(stop) => Err(stop),
        }
    }

    /// Egress gate: a message routed on a topic while the subscription was
    /// still in the manager index is dropped here if the connection no longer
    /// holds it, closing the window between re-authorization and index
    /// removal.
    async fn passes_egress_gate(&self, topic: Option<&str>) -> bool {
        let Some(topic) = topic else {
            return true;
        };
        if self.context.is_subscribed_to(topic).await {
            return true;
        }
        debug!(
            "Dropping message on '{}' for connection {}: not subscribed",
            topic, self.context.id
        );
        false
    }

    /// Send what is already queued for the socket. The sender is closed, so
    /// nothing new arrives and the loop ends.
    async fn drain_outbound<S: WebSocketIo + ?Sized>(&mut self, socket: &mut S) {
        while let Ok(OutboundMessage { message, topic }) = self.message_rx.try_recv() {
            if !self.passes_egress_gate(topic.as_deref()).await {
                continue;
            }
            let Ok(message) = text_message(&message) else {
                continue;
            };
            if socket.send(message).await.is_err() {
                break;
            }
        }
    }

    /// Process one inbound frame and return the reply to write, if any.
    async fn handle_websocket_message(
        &self,
        msg: WebSocketIoMessage,
    ) -> ServerResult<Option<WebSocketIoMessage>> {
        match msg {
            WebSocketIoMessage::Text(text) => {
                if text.len() > self.max_message_size {
                    warn!("Received oversized text message: {} bytes", text.len());
                    return Err(ServerError::InvalidRequest(
                        "Message exceeds maximum size".to_string(),
                    ));
                }
                debug!("Received text message ({} bytes)", text.len());
                self.handle_text_message(text).await
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
                    Ok(text) => self.handle_text_message(text).await,
                    Err(_) => {
                        warn!("Received non-UTF-8 binary message, ignoring");
                        Ok(None)
                    }
                }
            }
            WebSocketIoMessage::Ping(data) => {
                debug!("Received ping");
                self.handler.on_ping(self.context.clone()).await?;
                Ok(Some(WebSocketIoMessage::Pong(data)))
            }
            WebSocketIoMessage::Pong(_) => {
                debug!("Received pong");
                self.handler.on_pong(self.context.clone()).await?;
                Ok(None)
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
    async fn handle_text_message(&self, text: String) -> ServerResult<Option<WebSocketIoMessage>> {
        if let Ok(msg) = serde_json::from_str::<BidirectionalMessage>(&text) {
            return self.handle_bidirectional_message(msg).await;
        }

        if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&text) {
            return self.handle_jsonrpc_request(request).await;
        }

        // Neither shape parsed. Per JSON-RPC 2.0, answer with a Parse Error
        // (-32700, id null) and keep the connection open; only transport
        // failures terminate the handler loop.
        warn!(
            "Could not parse message as JSON-RPC or bidirectional message on connection {}",
            self.context.id
        );
        let response = JsonRpcResponse::error(JsonRpcError::parse_error(), None);
        text_message(&BidirectionalMessage::Response(response)).map(Some)
    }

    /// Handle bidirectional messages
    async fn handle_bidirectional_message(
        &self,
        msg: BidirectionalMessage,
    ) -> ServerResult<Option<WebSocketIoMessage>> {
        match msg {
            BidirectionalMessage::Request(request) => self.handle_jsonrpc_request(request).await,
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
                    return text_message(&BidirectionalMessage::Response(response)).map(Some);
                }
                self.handler
                    .handle_subscribe(topics, self.context.clone())
                    .await?;
                Ok(None)
            }
            BidirectionalMessage::Unsubscribe { topics } => {
                if let Err(reason) = self.check_subscription_limits(&topics) {
                    warn!(
                        "Rejected unsubscribe on connection {}: {}",
                        self.context.id, reason
                    );
                    return Ok(None);
                }
                self.handler
                    .handle_unsubscribe(topics, self.context.clone())
                    .await?;
                Ok(None)
            }
            BidirectionalMessage::Ping => {
                self.handler.on_ping(self.context.clone()).await?;
                Ok(None)
            }
            BidirectionalMessage::Pong => {
                self.handler.on_pong(self.context.clone()).await?;
                Ok(None)
            }
            // Other message types are typically server-to-client
            _ => {
                warn!("Received unexpected bidirectional message type from client");
                Ok(None)
            }
        }
    }

    /// Handle JSON-RPC requests
    async fn handle_jsonrpc_request(
        &self,
        request: JsonRpcRequest,
    ) -> ServerResult<Option<WebSocketIoMessage>> {
        debug!("Handling JSON-RPC request: {}", request.method);
        let request_id = request.id.clone();

        // A notification (no `id`) gets no response, whatever the handler
        // returns; a failure is only logged.
        if request.is_notification() {
            let method = request.method.clone();
            match self
                .handler
                .handle_request(request, self.context.clone())
                .await
            {
                Ok(_) => {}
                Err(e) => error!("Error handling notification {}: {}", method, e),
            }
            return Ok(None);
        }

        let response = match self
            .handler
            .handle_request(request, self.context.clone())
            .await
        {
            Ok(Some(response)) => response,
            // No response needed (notification)
            Ok(None) => return Ok(None),
            Err(e) => {
                error!("Error handling request: {}", e);
                JsonRpcResponse::error(jsonrpc_error_from_server_error(&e), request_id)
            }
        };
        text_message(&BidirectionalMessage::Response(response)).map(Some)
    }
}

/// How long a closing connection keeps writing: messages queued before a
/// forced disconnect, then the close frames.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

fn text_message(message: &BidirectionalMessage) -> ServerResult<WebSocketIoMessage> {
    Ok(WebSocketIoMessage::Text(serde_json::to_string(message)?))
}

/// Why the handler loop stopped.
enum Stop {
    /// The peer closed the socket, or the outbound channel closed.
    Ended,
    /// The connection's sender was closed: a forced disconnect, or a peer too
    /// slow to keep its outbound queue from filling.
    Disconnected,
    /// Re-validation failed, or permissions changed under
    /// [`PermissionChangePolicy::Close`].
    Revoked,
    /// No inbound frame, or no progress on a write, for the idle timeout.
    Idle,
    /// A handler, transport or re-authorization error.
    Failed(ServerError),
}

enum Interrupt {
    Closed,
    Revalidate,
    Idle,
}

enum Event {
    Interrupt(Interrupt),
    Ping,
    Inbound(Option<ServerResult<WebSocketIoMessage>>),
    Outbound(Option<OutboundMessage>),
}

/// Timers that bound a connection's life: credential re-validation and the
/// idle timeout.
struct Lifecycle {
    revalidation: Option<tokio::time::Interval>,
    idle_timeout: Option<Duration>,
    idle_deadline: Pin<Box<tokio::time::Sleep>>,
}

impl Lifecycle {
    fn new(revalidation: Option<&AuthRevalidation>, keepalive: &KeepaliveConfig) -> Self {
        let revalidation = revalidation.map(|revalidation| {
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
        let idle_timeout = keepalive.idle_timeout.filter(|timeout| {
            if timeout.is_zero() {
                warn!("keepalive idle timeout is zero; idle timeout disabled");
            }
            !timeout.is_zero()
        });
        let idle_deadline = Box::pin(tokio::time::sleep(
            idle_timeout.unwrap_or(Duration::from_secs(0)),
        ));
        Self {
            revalidation,
            idle_timeout,
            idle_deadline,
        }
    }

    /// Restart the idle clock.
    fn touch(&mut self) {
        if let Some(timeout) = self.idle_timeout {
            self.idle_deadline
                .as_mut()
                .reset(tokio::time::Instant::now() + timeout);
        }
    }

    /// Wait for the next lifecycle event. Cancel-safe: a tick consumed here is
    /// returned, never lost, because nothing else is awaited.
    async fn next_interrupt(
        &mut self,
        sender: &ChannelMessageSender,
        watch_idle: bool,
    ) -> Interrupt {
        let watch_idle = watch_idle && self.idle_timeout.is_some();
        let Self {
            revalidation,
            idle_deadline,
            ..
        } = self;
        tokio::select! {
            biased;
            _ = sender.closed() => Interrupt::Closed,
            _ = async { revalidation.as_mut().expect("guarded by is_some").tick().await },
                if revalidation.is_some() => Interrupt::Revalidate,
            _ = idle_deadline.as_mut(), if watch_idle => Interrupt::Idle,
        }
    }
}

fn jsonrpc_error_from_server_error(error: &ServerError) -> JsonRpcError {
    if let Some(public) = error.public_rpc_error() {
        return public;
    }
    let code = match error {
        ServerError::AuthenticationFailed(_) => error_codes::AUTHENTICATION_REQUIRED,
        ServerError::PermissionDenied(_) => error_codes::INSUFFICIENT_PERMISSIONS,
        ServerError::InvalidRequest(_) => error_codes::INVALID_REQUEST,
        ServerError::InvalidParams(_) => error_codes::INVALID_PARAMS,
        ServerError::HandlerNotFound(_) => error_codes::METHOD_NOT_FOUND,
        ServerError::SerializationError(_) => error_codes::INVALID_PARAMS,
        ServerError::UpgradeFailed(_)
        | ServerError::ConnectionNotFound(_)
        | ServerError::RoutingFailed(_)
        | ServerError::WebSocketError(_)
        | ServerError::ConnectionError(_)
        | ServerError::Internal(_)
        | ServerError::Rpc(_) => error_codes::INTERNAL_ERROR,
    };

    // Send only a generic per-class message; the full error was already logged
    // server-side by the caller. Never interpolate handler/AuthError Display.
    JsonRpcError::new(code, error.client_message().to_string(), None)
}

#[cfg(test)]
mod tests;
