//! Main client implementation for bidirectional JSON-RPC communication

use crate::{
    ClientState, ConnectionEvent, ConnectionEventHandler, NotificationHandler, PendingRequest,
    RpcRequestHandler, Subscription, WebSocketTransport,
    config::{AuthConfig, ClientConfig, ReconnectConfig},
    error::{ClientError, ClientResult},
};
use dashmap::DashMap;
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionId};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{debug, error, info, warn};

#[cfg(not(target_arch = "wasm32"))]
use crate::native::NativeWebSocketTransport;

#[cfg(target_arch = "wasm32")]
use crate::wasm::WasmWebSocketTransport;

/// Bidirectional JSON-RPC WebSocket client
pub struct Client {
    config: ClientConfig,
    transport: Arc<RwLock<Box<dyn WebSocketTransport>>>,
    state: Arc<RwLock<ClientState>>,
    connection_id: Arc<RwLock<Option<ConnectionId>>>,
    pending_requests: Arc<DashMap<Value, PendingRequest>>,
    subscriptions: Arc<DashMap<String, Subscription>>,
    notification_handlers: Arc<DashMap<String, NotificationHandler>>,
    rpc_request_handlers: Arc<DashMap<String, RpcRequestHandler>>,
    connection_event_handlers: Arc<DashMap<String, ConnectionEventHandler>>,
    request_id_counter: Arc<AtomicU64>,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    message_tx: Arc<RwLock<Option<mpsc::Sender<BidirectionalMessage>>>>,
}

struct IncomingMessageContext<'a> {
    pending_requests: &'a DashMap<Value, PendingRequest>,
    subscriptions: &'a DashMap<String, Subscription>,
    notification_handlers: &'a DashMap<String, NotificationHandler>,
    rpc_request_handlers: &'a DashMap<String, RpcRequestHandler>,
    connection_event_handlers: &'a DashMap<String, ConnectionEventHandler>,
    connection_id: &'a RwLock<Option<ConnectionId>>,
    message_tx: &'a RwLock<Option<mpsc::Sender<BidirectionalMessage>>>,
}

impl Client {
    /// Create a new client with the given configuration
    pub async fn new(config: ClientConfig) -> ClientResult<Self> {
        config.validate().map_err(ClientError::configuration)?;

        #[cfg(not(target_arch = "wasm32"))]
        let transport: Box<dyn WebSocketTransport> =
            Box::new(NativeWebSocketTransport::new(config.clone()));

        #[cfg(target_arch = "wasm32")]
        let transport: Box<dyn WebSocketTransport> =
            Box::new(WasmWebSocketTransport::new(config.clone()));

        Ok(Self {
            config,
            transport: Arc::new(RwLock::new(transport)),
            state: Arc::new(RwLock::new(ClientState::Disconnected)),
            connection_id: Arc::new(RwLock::new(None)),
            pending_requests: Arc::new(DashMap::new()),
            subscriptions: Arc::new(DashMap::new()),
            notification_handlers: Arc::new(DashMap::new()),
            rpc_request_handlers: Arc::new(DashMap::new()),
            connection_event_handlers: Arc::new(DashMap::new()),
            request_id_counter: Arc::new(AtomicU64::new(1)),
            shutdown_tx: Arc::new(RwLock::new(None)),
            message_tx: Arc::new(RwLock::new(None)),
        })
    }

    /// Connect to the WebSocket server
    pub async fn connect(&self) -> ClientResult<()> {
        let mut state = self.state.write().await;
        if *state != ClientState::Disconnected {
            return Err(ClientError::AlreadyConnected);
        }
        *state = ClientState::Connecting;
        drop(state);

        // Connect transport
        let mut transport = self.transport.write().await;
        transport
            .connect()
            .await
            .map_err(|e| ClientError::connection(format!("Failed to connect: {}", e)))?;
        drop(transport);

        // Set up message handling
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (message_tx, message_rx) = mpsc::channel(self.config.message_buffer_size);

        *self.shutdown_tx.write().await = Some(shutdown_tx);
        *self.message_tx.write().await = Some(message_tx);

        // Start message handling task
        self.start_message_handler(message_rx, shutdown_rx).await?;

        // Start heartbeat if configured
        if let Some(interval) = self.config.heartbeat_interval {
            self.start_heartbeat(interval).await;
        }

        *self.state.write().await = ClientState::Connected;
        info!("Client connected to {}", self.config.url);

        loop {
            if self.connection_id.read().await.is_some() {
                break;
            }
        }

        Ok(())
    }

    /// Disconnect from the WebSocket server
    pub async fn disconnect(&self) -> ClientResult<()> {
        let mut state = self.state.write().await;
        if *state == ClientState::Disconnected {
            return Ok(());
        }
        *state = ClientState::Disconnected;
        drop(state);

        // Send shutdown signal
        if let Some(shutdown_tx) = self.shutdown_tx.write().await.take() {
            let _ = shutdown_tx.send(());
        }

        // Disconnect transport
        let mut transport = self.transport.write().await;
        transport
            .disconnect()
            .await
            .map_err(|e| ClientError::connection(format!("Failed to disconnect: {}", e)))?;

        // Clear connection state
        *self.connection_id.write().await = None;
        *self.message_tx.write().await = None;

        // Fail all pending requests
        let pending_ids: Vec<Value> = self
            .pending_requests
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        for id in pending_ids {
            if let Some((_, pending)) = self.pending_requests.remove(&id) {
                let _ = pending.sender.send(JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::internal_error(
                        "Client disconnected".to_string(),
                    ),
                    Some(pending.id),
                ));
            }
        }
        self.pending_requests.clear();

        self.emit_connection_event(ConnectionEvent::Disconnected { reason: None })
            .await;
        info!("Client disconnected");

        Ok(())
    }

    /// Make a JSON-RPC call and wait for the response
    pub async fn call(&self, method: &str, params: Option<Value>) -> ClientResult<JsonRpcResponse> {
        let state = self.state.read().await;
        if *state != ClientState::Connected {
            return Err(ClientError::NotConnected);
        }
        drop(state);

        let request_id = Value::Number(serde_json::Number::from(
            self.request_id_counter.fetch_add(1, Ordering::SeqCst),
        ));

        let request = JsonRpcRequest::new(method.to_string(), params, Some(request_id.clone()));

        let (response_tx, response_rx) = oneshot::channel();
        let pending = PendingRequest {
            id: request_id.clone(),
            sender: response_tx,
            created_at: Instant::now(),
        };

        // Check if we're over the pending request limit
        if self.pending_requests.len() >= self.config.max_pending_requests {
            return Err(ClientError::internal("Too many pending requests"));
        }

        self.pending_requests.insert(request_id, pending);

        // Send the request
        let message = BidirectionalMessage::Request(request);
        self.send_message(message).await?;

        // Wait for response with timeout
        let response = tokio::time::timeout(self.config.request_timeout, response_rx)
            .await
            .map_err(|_| ClientError::timeout(self.config.request_timeout.as_secs()))?
            .map_err(|_| ClientError::internal("Response channel closed"))?;

        Ok(response)
    }

    /// Send a notification (fire-and-forget)
    pub async fn notify(&self, method: &str, params: Option<Value>) -> ClientResult<()> {
        let state = self.state.read().await;
        if *state != ClientState::Connected {
            return Err(ClientError::NotConnected);
        }
        drop(state);

        let request = JsonRpcRequest::new(method.to_string(), params, None);
        let message = BidirectionalMessage::Request(request);
        self.send_message(message).await
    }

    /// Subscribe to a topic for receiving notifications
    pub async fn subscribe(&self, topic: &str, handler: NotificationHandler) -> ClientResult<()> {
        let state = self.state.read().await;
        if *state != ClientState::Connected {
            return Err(ClientError::NotConnected);
        }
        drop(state);

        let subscription = Subscription {
            topic: topic.to_string(),
            handler: handler.clone(),
            created_at: Instant::now(),
        };

        self.subscriptions.insert(topic.to_string(), subscription);

        // Send subscription message
        let message = BidirectionalMessage::Subscribe {
            topics: vec![topic.to_string()],
        };
        self.send_message(message).await?;

        debug!("Subscribed to topic: {}", topic);
        Ok(())
    }

    /// Unsubscribe from a topic
    pub async fn unsubscribe(&self, topic: &str) -> ClientResult<()> {
        let state = self.state.read().await;
        if *state != ClientState::Connected {
            return Err(ClientError::NotConnected);
        }
        drop(state);

        self.subscriptions.remove(topic);

        // Send unsubscription message
        let message = BidirectionalMessage::Unsubscribe {
            topics: vec![topic.to_string()],
        };
        self.send_message(message).await?;

        debug!("Unsubscribed from topic: {}", topic);
        Ok(())
    }

    /// Register a handler for specific notification methods
    pub fn on_notification(&self, method: &str, handler: NotificationHandler) {
        self.notification_handlers
            .insert(method.to_string(), handler);
        debug!("Registered notification handler for method: {}", method);
    }

    /// Register a handler for connection events
    pub fn on_connection_event(&self, name: &str, handler: ConnectionEventHandler) {
        self.connection_event_handlers
            .insert(name.to_string(), handler);
        debug!("Registered connection event handler: {}", name);
    }

    /// Register a handler for RPC requests from the server
    pub fn on_rpc_request(&self, method: &str, handler: RpcRequestHandler) {
        self.rpc_request_handlers
            .insert(method.to_string(), handler);
        debug!("Registered RPC request handler for method: {}", method);
    }

    /// Get the current connection state
    pub async fn state(&self) -> ClientState {
        *self.state.read().await
    }

    /// Get the current connection ID (if connected)
    pub async fn connection_id(&self) -> Option<ConnectionId> {
        *self.connection_id.read().await
    }

    /// Check if the client is currently connected
    pub async fn is_connected(&self) -> bool {
        *self.state.read().await == ClientState::Connected
    }

    /// Get client configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Get the number of pending requests
    pub fn pending_requests_count(&self) -> usize {
        self.pending_requests.len()
    }

    /// Get the list of active subscriptions
    pub fn active_subscriptions(&self) -> Vec<String> {
        self.subscriptions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    // Internal helper methods

    async fn send_message(&self, message: BidirectionalMessage) -> ClientResult<()> {
        if let Some(tx) = self.message_tx.read().await.as_ref() {
            tx.send(message)
                .await
                .map_err(|_| ClientError::send_failed("Message channel closed"))?;
        } else {
            return Err(ClientError::NotConnected);
        }
        Ok(())
    }

    async fn start_message_handler(
        &self,
        mut message_rx: mpsc::Receiver<BidirectionalMessage>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> ClientResult<()> {
        let transport = Arc::clone(&self.transport);
        let pending_requests = Arc::clone(&self.pending_requests);
        let subscriptions = Arc::clone(&self.subscriptions);
        let notification_handlers = Arc::clone(&self.notification_handlers);
        let rpc_request_handlers = Arc::clone(&self.rpc_request_handlers);
        let connection_event_handlers = Arc::clone(&self.connection_event_handlers);
        let connection_id = Arc::clone(&self.connection_id);
        let state = Arc::clone(&self.state);
        let message_tx_clone = Arc::clone(&self.message_tx);

        spawn_background(async move {
            let mut receive_interval = tokio::time::interval(Duration::from_millis(10));

            loop {
                tokio::select! {
                    // Handle shutdown signal
                    _ = &mut shutdown_rx => {
                        debug!("Message handler received shutdown signal");
                        break;
                    }

                    // Handle outgoing messages
                    message = message_rx.recv() => {
                        if let Some(message) = message {
                            let mut transport = transport.write().await;
                            if let Err(e) = transport.send(&message).await {
                                error!("Failed to send message: {}", e);
                            }
                        } else {
                            debug!("Message channel closed");
                            break;
                        }
                    }

                    // Handle incoming messages
                    _ = receive_interval.tick() => {
                        let transport_clone = Arc::clone(&transport);
                        let mut transport = transport_clone.write().await;
                        match transport.receive().await {
                            Ok(Some(message)) => {
                                let context = IncomingMessageContext {
                                    pending_requests: &pending_requests,
                                    subscriptions: &subscriptions,
                                    notification_handlers: &notification_handlers,
                                    rpc_request_handlers: &rpc_request_handlers,
                                    connection_event_handlers: &connection_event_handlers,
                                    connection_id: &connection_id,
                                    message_tx: &message_tx_clone,
                                };
                                Self::handle_incoming_message(
                                    message,
                                    context,
                                ).await;
                            }
                            Ok(None) => {
                                // No message available, continue
                            }
                            Err(e) => {
                                error!("Failed to receive message: {}", e);
                                *state.write().await = ClientState::Failed;
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_incoming_message(
        message: BidirectionalMessage,
        context: IncomingMessageContext<'_>,
    ) {
        match message {
            BidirectionalMessage::Response(response) => {
                if let Some(id) = &response.id {
                    if let Some((_, pending)) = context.pending_requests.remove(id) {
                        let _ = pending.sender.send(response);
                    } else {
                        warn!("Received response for unknown request ID: {:?}", id);
                    }
                }
            }
            BidirectionalMessage::ServerNotification(notification) => {
                // Handle notification with registered handlers
                if let Some(handler) = context.notification_handlers.get(&notification.method) {
                    handler(&notification.method, &notification.params);
                }
            }
            BidirectionalMessage::Broadcast(broadcast) => {
                // Handle broadcast to subscribed topics
                if let Some(subscription) = context.subscriptions.get(&broadcast.topic) {
                    (subscription.value().handler)(&broadcast.method, &broadcast.params);
                }
            }
            BidirectionalMessage::ConnectionEstablished {
                connection_id: conn_id,
            } => {
                *context.connection_id.write().await = Some(conn_id);
                Self::emit_connection_event_static(
                    ConnectionEvent::Connected {
                        connection_id: conn_id,
                    },
                    context.connection_event_handlers,
                )
                .await;
            }
            BidirectionalMessage::ConnectionClosed { reason, .. } => {
                *context.connection_id.write().await = None;
                Self::emit_connection_event_static(
                    ConnectionEvent::Disconnected { reason },
                    context.connection_event_handlers,
                )
                .await;
            }
            BidirectionalMessage::Request(request) => {
                // Handle incoming RPC request from server
                if let Some(_id) = &request.id {
                    if let Some(handler) = context.rpc_request_handlers.get(&request.method) {
                        debug!("Handling RPC request: {}", request.method);
                        let response = handler(request).await;

                        // Send response back to server
                        let response_message = BidirectionalMessage::Response(response);
                        let tx = context.message_tx.read().await.clone();
                        if let Some(tx) = tx
                            && let Err(e) = tx.send(response_message).await
                        {
                            error!("Failed to send RPC response: {}", e);
                        }
                    } else {
                        warn!("No handler registered for RPC method: {}", request.method);
                        // Send method not found error
                        let error_response = JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::new(
                                -32601,
                                "Method not found".to_string(),
                                None,
                            ),
                            request.id.clone(),
                        );
                        let response_message = BidirectionalMessage::Response(error_response);
                        let tx = context.message_tx.read().await.clone();
                        if let Some(tx) = tx
                            && let Err(e) = tx.send(response_message).await
                        {
                            error!("Failed to send error response: {}", e);
                        }
                    }
                } else {
                    debug!(
                        "Received RPC request without ID (notification): {}",
                        request.method
                    );
                }
            }
            BidirectionalMessage::Pong => {
                debug!("Received pong");
            }
            _ => {
                debug!("Received unhandled message: {:?}", message);
            }
        }
    }

    async fn emit_connection_event(&self, event: ConnectionEvent) {
        Self::emit_connection_event_static(event, &self.connection_event_handlers).await;
    }

    async fn emit_connection_event_static(
        event: ConnectionEvent,
        handlers: &DashMap<String, ConnectionEventHandler>,
    ) {
        for handler in handlers.iter() {
            handler.value()(event.clone());
        }
    }

    async fn start_heartbeat(&self, interval: Duration) {
        let message_tx = Arc::clone(&self.message_tx);
        let state = Arc::clone(&self.state);

        spawn_background(async move {
            let mut heartbeat_interval = tokio::time::interval(interval);
            heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                heartbeat_interval.tick().await;

                let current_state = *state.read().await;
                if current_state != ClientState::Connected {
                    break;
                }

                let tx_guard = message_tx.read().await;
                if let Some(tx) = tx_guard.as_ref() {
                    if tx.send(BidirectionalMessage::Ping).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
    }

    /// Clean up expired pending requests
    pub async fn cleanup_expired_requests(&self) {
        let timeout = self.config.request_timeout;
        let now = Instant::now();

        let expired_ids: Vec<Value> = self
            .pending_requests
            .iter()
            .filter_map(|entry| {
                if now.duration_since(entry.created_at) > timeout {
                    Some(entry.id.clone())
                } else {
                    None
                }
            })
            .collect();

        for id in expired_ids {
            if let Some((_, pending)) = self.pending_requests.remove(&id) {
                let _ = pending.sender.send(JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::internal_error("Request timeout".to_string()),
                    Some(pending.id),
                ));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future);
}

#[cfg(target_arch = "wasm32")]
fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}

/// Builder for creating a client with configuration
pub struct ClientBuilder {
    /// WebSocket URL to connect to
    url: String,

    /// JWT token for authentication
    jwt_token: Option<String>,

    /// Whether to send JWT token in header (true) or as parameter (false)
    jwt_in_header: bool,

    /// Custom headers
    custom_headers: HashMap<String, String>,

    /// Request timeout
    request_timeout: Duration,

    /// Reconnection configuration
    reconnect_config: Option<ReconnectConfig>,

    /// Heartbeat interval
    heartbeat_interval: Option<Duration>,

    /// Connection timeout
    connection_timeout: Duration,

    /// Auto-connect after building
    auto_connect: bool,
}

impl ClientBuilder {
    /// Create a new client builder with the given URL
    pub fn new<S: Into<String>>(url: S) -> Self {
        Self {
            url: url.into(),
            jwt_token: None,
            jwt_in_header: true,
            custom_headers: HashMap::new(),
            request_timeout: Duration::from_secs(30),
            reconnect_config: None,
            heartbeat_interval: Some(Duration::from_secs(30)),
            connection_timeout: Duration::from_secs(10),
            auto_connect: false,
        }
    }

    /// Set JWT token for authentication
    pub fn with_jwt_token(mut self, token: String) -> Self {
        self.jwt_token = Some(token);
        self
    }

    /// Set whether to send JWT token in header or as parameter
    pub fn with_jwt_in_header(mut self, in_header: bool) -> Self {
        self.jwt_in_header = in_header;
        self
    }

    /// Add a custom header
    pub fn with_header<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.custom_headers.insert(key.into(), value.into());
        self
    }

    /// Set request timeout
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set reconnection configuration
    pub fn with_reconnect_config(mut self, config: ReconnectConfig) -> Self {
        self.reconnect_config = Some(config);
        self
    }

    /// Set heartbeat interval
    pub fn with_heartbeat_interval(mut self, interval: Option<Duration>) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    /// Set connection timeout
    pub fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    /// Enable auto-connect after building
    pub fn with_auto_connect(mut self, auto_connect: bool) -> Self {
        self.auto_connect = auto_connect;
        self
    }

    /// Build the client
    pub async fn build(self) -> ClientResult<Client> {
        let auth = match self.jwt_token {
            Some(token) => {
                if self.jwt_in_header {
                    AuthConfig::JwtHeader { token }
                } else {
                    AuthConfig::JwtParams { token }
                }
            }
            None => AuthConfig::None,
        };

        let config = ClientConfig {
            url: self.url,
            auth,
            reconnect: self.reconnect_config.unwrap_or_default(),
            request_timeout: self.request_timeout,
            heartbeat_interval: self.heartbeat_interval,
            max_pending_requests: 1000,
            custom_headers: self.custom_headers,
            connection_timeout: self.connection_timeout,
            message_buffer_size: 1024,
            auto_subscribe_events: true,
        };

        let client = Client::new(config).await?;

        if self.auto_connect {
            client.connect().await?;
        }

        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct IncomingHarness {
        pending_requests: DashMap<Value, PendingRequest>,
        subscriptions: DashMap<String, Subscription>,
        notification_handlers: DashMap<String, NotificationHandler>,
        rpc_request_handlers: DashMap<String, RpcRequestHandler>,
        connection_event_handlers: DashMap<String, ConnectionEventHandler>,
        connection_id: RwLock<Option<ConnectionId>>,
        message_tx: RwLock<Option<mpsc::Sender<BidirectionalMessage>>>,
    }

    impl IncomingHarness {
        fn new() -> Self {
            Self {
                pending_requests: DashMap::new(),
                subscriptions: DashMap::new(),
                notification_handlers: DashMap::new(),
                rpc_request_handlers: DashMap::new(),
                connection_event_handlers: DashMap::new(),
                connection_id: RwLock::new(None),
                message_tx: RwLock::new(None),
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
            }
        }
    }

    #[tokio::test]
    async fn test_client_builder() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .with_jwt_token("test_token".to_string())
            .with_request_timeout(Duration::from_secs(60))
            .build()
            .await
            .expect("Failed to build client");

        assert_eq!(client.config().url, "ws://localhost:8080");
        assert_eq!(client.config().request_timeout, Duration::from_secs(60));
        assert!(matches!(client.config().auth, AuthConfig::JwtHeader { .. }));
    }

    #[tokio::test]
    async fn test_client_state() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("Failed to build client");

        assert_eq!(client.state().await, ClientState::Disconnected);
        assert!(!client.is_connected().await);
        assert!(client.connection_id().await.is_none());
    }

    #[tokio::test]
    async fn builder_jwt_in_query_params_and_full_setters() {
        // Exercise every with_* setter so each path is colored. We don't
        // auto-connect (no server), but the resulting config must reflect
        // each option.
        let custom = ReconnectConfig::default();
        let client = ClientBuilder::new("ws://localhost:8080")
            .with_jwt_token("tok".into())
            .with_jwt_in_header(false)
            .with_header("X-Custom", "v")
            .with_request_timeout(Duration::from_secs(11))
            .with_reconnect_config(custom)
            .with_heartbeat_interval(None)
            .with_connection_timeout(Duration::from_secs(7))
            .with_auto_connect(false)
            .build()
            .await
            .expect("build");

        assert!(matches!(client.config().auth, AuthConfig::JwtParams { .. }));
        assert_eq!(client.config().request_timeout, Duration::from_secs(11));
        assert_eq!(client.config().connection_timeout, Duration::from_secs(7));
        assert!(client.config().heartbeat_interval.is_none());
        assert_eq!(
            client.config().custom_headers.get("X-Custom"),
            Some(&"v".to_string())
        );
        assert!(client.active_subscriptions().is_empty());
        assert_eq!(client.pending_requests_count(), 0);
    }

    #[tokio::test]
    async fn builder_without_token_yields_no_auth() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("build");
        assert!(matches!(client.config().auth, AuthConfig::None));
    }

    #[tokio::test]
    async fn call_notify_subscribe_unsubscribe_require_connected_state() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("build");

        // call → NotConnected
        let err = client.call("m", None).await.unwrap_err();
        assert!(matches!(err, ClientError::NotConnected));

        // notify → NotConnected
        let err = client.notify("m", None).await.unwrap_err();
        assert!(matches!(err, ClientError::NotConnected));

        // subscribe → NotConnected
        let handler: NotificationHandler = std::sync::Arc::new(|_method: &str, _params: &Value| {});
        let err = client.subscribe("t", handler.clone()).await.unwrap_err();
        assert!(matches!(err, ClientError::NotConnected));

        // unsubscribe → NotConnected
        let err = client.unsubscribe("t").await.unwrap_err();
        assert!(matches!(err, ClientError::NotConnected));
    }

    #[tokio::test]
    async fn handler_registration_does_not_require_connected_state() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("build");

        let n: NotificationHandler = std::sync::Arc::new(|_, _| {});
        let e: ConnectionEventHandler = std::sync::Arc::new(|_event| {});
        client.on_notification("evt", n);
        client.on_connection_event("named", e);

        // cleanup_expired_requests is callable even with nothing pending.
        client.cleanup_expired_requests().await;

        // Disconnect-when-already-disconnected is a no-op success.
        client.disconnect().await.expect("disconnect ok");
    }

    #[tokio::test]
    async fn notify_subscribe_and_unsubscribe_send_expected_messages_when_connected() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("build");
        *client.state.write().await = ClientState::Connected;

        let (tx, mut rx) = mpsc::channel(4);
        *client.message_tx.write().await = Some(tx);

        client
            .notify("client.ready", Some(serde_json::json!({"ready": true})))
            .await
            .expect("notify");
        match rx.recv().await.expect("notify message") {
            BidirectionalMessage::Request(request) => {
                assert_eq!(request.method, "client.ready");
                assert_eq!(request.params, Some(serde_json::json!({"ready": true})));
                assert!(request.id.is_none());
            }
            other => panic!("unexpected notify message: {other:?}"),
        }

        let handler: NotificationHandler = std::sync::Arc::new(|_method, _params| {});
        client
            .subscribe("room:1", handler)
            .await
            .expect("subscribe");
        match rx.recv().await.expect("subscribe message") {
            BidirectionalMessage::Subscribe { topics } => {
                assert_eq!(topics, vec!["room:1".to_string()]);
            }
            other => panic!("unexpected subscribe message: {other:?}"),
        }
        assert_eq!(client.active_subscriptions(), vec!["room:1".to_string()]);

        client.unsubscribe("room:1").await.expect("unsubscribe");
        match rx.recv().await.expect("unsubscribe message") {
            BidirectionalMessage::Unsubscribe { topics } => {
                assert_eq!(topics, vec!["room:1".to_string()]);
            }
            other => panic!("unexpected unsubscribe message: {other:?}"),
        }
        assert!(client.active_subscriptions().is_empty());
    }

    #[tokio::test]
    async fn call_sends_request_and_completes_when_pending_response_arrives() {
        let client = std::sync::Arc::new(
            ClientBuilder::new("ws://localhost:8080")
                .build()
                .await
                .expect("build"),
        );
        *client.state.write().await = ClientState::Connected;

        let (tx, mut rx) = mpsc::channel(4);
        *client.message_tx.write().await = Some(tx);

        let call_task = {
            let client = std::sync::Arc::clone(&client);
            tokio::spawn(async move {
                client
                    .call("svc.echo", Some(serde_json::json!({"input": 1})))
                    .await
            })
        };

        let request_id = match rx.recv().await.expect("outgoing request") {
            BidirectionalMessage::Request(request) => {
                assert_eq!(request.method, "svc.echo");
                assert_eq!(request.params, Some(serde_json::json!({"input": 1})));
                request.id.expect("request id")
            }
            other => panic!("unexpected outgoing request: {other:?}"),
        };

        let (_, pending) = client
            .pending_requests
            .remove(&request_id)
            .expect("pending request registered");
        pending
            .sender
            .send(JsonRpcResponse::success(
                serde_json::json!({"output": 1}),
                Some(request_id),
            ))
            .expect("deliver response");

        let response = call_task.await.expect("join").expect("call response");
        assert_eq!(response.result, Some(serde_json::json!({"output": 1})));
        assert!(client.pending_requests.is_empty());
    }

    #[tokio::test]
    async fn call_returns_internal_error_when_pending_request_limit_is_reached() {
        let mut config = ClientConfig::new("ws://localhost:8080");
        config.max_pending_requests = 1;
        let client = Client::new(config).await.expect("client");
        *client.state.write().await = ClientState::Connected;

        let (message_tx, mut message_rx) = mpsc::channel(1);
        *client.message_tx.write().await = Some(message_tx);
        let (pending_tx, _pending_rx) = oneshot::channel();
        client.pending_requests.insert(
            serde_json::json!("existing"),
            PendingRequest {
                id: serde_json::json!("existing"),
                sender: pending_tx,
                created_at: Instant::now(),
            },
        );

        let err = client.call("svc.echo", None).await.unwrap_err();
        assert!(
            matches!(err, ClientError::Internal(message) if message == "Too many pending requests")
        );
        assert!(message_rx.try_recv().is_err());
        assert_eq!(client.pending_requests.len(), 1);
    }

    #[tokio::test]
    async fn cleanup_expired_requests_removes_expired_waiters_and_keeps_fresh_ones() {
        let mut config = ClientConfig::new("ws://localhost:8080");
        config.request_timeout = Duration::from_secs(1);
        let client = Client::new(config).await.expect("client");

        let (expired_tx, expired_rx) = oneshot::channel();
        client.pending_requests.insert(
            serde_json::json!("expired"),
            PendingRequest {
                id: serde_json::json!("expired"),
                sender: expired_tx,
                created_at: Instant::now() - Duration::from_secs(5),
            },
        );

        let (fresh_tx, _fresh_rx) = oneshot::channel();
        client.pending_requests.insert(
            serde_json::json!("fresh"),
            PendingRequest {
                id: serde_json::json!("fresh"),
                sender: fresh_tx,
                created_at: Instant::now(),
            },
        );

        client.cleanup_expired_requests().await;

        let timeout_response = expired_rx.await.expect("expired waiter notified");
        assert_eq!(timeout_response.id, Some(serde_json::json!("expired")));
        assert_eq!(
            timeout_response.error.expect("timeout error").code,
            ras_jsonrpc_types::error_codes::INTERNAL_ERROR
        );
        assert!(
            !client
                .pending_requests
                .contains_key(&serde_json::json!("expired"))
        );
        assert!(
            client
                .pending_requests
                .contains_key(&serde_json::json!("fresh"))
        );
    }

    #[tokio::test]
    async fn disconnect_clears_pending_requests_connection_state_and_emits_event() {
        let client = ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("build");
        *client.state.write().await = ClientState::Connected;
        *client.connection_id.write().await = Some(ConnectionId::new());

        let (message_tx, _message_rx) = mpsc::channel(1);
        *client.message_tx.write().await = Some(message_tx);
        let (pending_tx, pending_rx) = oneshot::channel();
        client.pending_requests.insert(
            serde_json::json!("in-flight"),
            PendingRequest {
                id: serde_json::json!("in-flight"),
                sender: pending_tx,
                created_at: Instant::now(),
            },
        );

        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let event_calls = std::sync::Arc::clone(&events);
        client.on_connection_event(
            "recorder",
            std::sync::Arc::new(move |event| {
                event_calls.lock().unwrap().push(event);
            }),
        );

        client.disconnect().await.expect("disconnect");

        assert_eq!(client.state().await, ClientState::Disconnected);
        assert!(client.connection_id().await.is_none());
        assert!(client.message_tx.read().await.is_none());
        assert!(client.pending_requests.is_empty());

        let failed_response = pending_rx.await.expect("pending waiter notified");
        assert_eq!(failed_response.id, Some(serde_json::json!("in-flight")));
        assert_eq!(
            failed_response.error.expect("disconnect error").code,
            ras_jsonrpc_types::error_codes::INTERNAL_ERROR
        );
        assert!(matches!(
            events.lock().unwrap().last().cloned().unwrap(),
            ConnectionEvent::Disconnected { reason: None }
        ));
    }

    #[tokio::test]
    async fn incoming_response_delivers_to_matching_pending_request() {
        let harness = IncomingHarness::new();
        let request_id = serde_json::json!(42);
        let (tx, rx) = oneshot::channel();
        harness.pending_requests.insert(
            request_id.clone(),
            PendingRequest {
                id: request_id.clone(),
                sender: tx,
                created_at: Instant::now(),
            },
        );

        Client::handle_incoming_message(
            BidirectionalMessage::Response(JsonRpcResponse::success(
                serde_json::json!({"ok": true}),
                Some(request_id),
            )),
            harness.context(),
        )
        .await;

        assert!(harness.pending_requests.is_empty());
        let response = rx.await.expect("pending response delivered");
        assert_eq!(response.result, Some(serde_json::json!({"ok": true})));

        Client::handle_incoming_message(
            BidirectionalMessage::Response(JsonRpcResponse::success(
                serde_json::json!("ignored"),
                Some(serde_json::json!("unknown")),
            )),
            harness.context(),
        )
        .await;
        Client::handle_incoming_message(
            BidirectionalMessage::Response(JsonRpcResponse::success(
                serde_json::json!("notification-like"),
                None,
            )),
            harness.context(),
        )
        .await;
    }

    #[tokio::test]
    async fn incoming_notifications_and_broadcasts_route_to_registered_handlers() {
        let harness = IncomingHarness::new();
        let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
        let broadcasts = std::sync::Arc::new(Mutex::new(Vec::new()));

        let notification_calls = std::sync::Arc::clone(&notifications);
        harness.notification_handlers.insert(
            "server.event".to_string(),
            std::sync::Arc::new(move |method, params| {
                notification_calls
                    .lock()
                    .unwrap()
                    .push((method.to_string(), params.clone()));
            }),
        );

        let broadcast_calls = std::sync::Arc::clone(&broadcasts);
        harness.subscriptions.insert(
            "room:1".to_string(),
            Subscription {
                topic: "room:1".to_string(),
                handler: std::sync::Arc::new(move |method, params| {
                    broadcast_calls
                        .lock()
                        .unwrap()
                        .push((method.to_string(), params.clone()));
                }),
                created_at: Instant::now(),
            },
        );

        Client::handle_incoming_message(
            BidirectionalMessage::ServerNotification(
                ras_jsonrpc_bidirectional_types::ServerNotification {
                    method: "server.event".to_string(),
                    params: serde_json::json!({"n": 1}),
                    metadata: None,
                },
            ),
            harness.context(),
        )
        .await;
        Client::handle_incoming_message(
            BidirectionalMessage::Broadcast(ras_jsonrpc_bidirectional_types::BroadcastMessage {
                topic: "room:1".to_string(),
                method: "chat.message".to_string(),
                params: serde_json::json!({"body": "hi"}),
                metadata: None,
            }),
            harness.context(),
        )
        .await;
        Client::handle_incoming_message(
            BidirectionalMessage::Broadcast(ras_jsonrpc_bidirectional_types::BroadcastMessage {
                topic: "room:2".to_string(),
                method: "chat.message".to_string(),
                params: serde_json::json!({"body": "ignored"}),
                metadata: None,
            }),
            harness.context(),
        )
        .await;

        assert_eq!(
            *notifications.lock().unwrap(),
            vec![("server.event".to_string(), serde_json::json!({"n": 1}))]
        );
        assert_eq!(
            *broadcasts.lock().unwrap(),
            vec![(
                "chat.message".to_string(),
                serde_json::json!({"body": "hi"})
            )]
        );
    }

    #[tokio::test]
    async fn incoming_connection_lifecycle_updates_id_and_emits_events() {
        let harness = IncomingHarness::new();
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let event_calls = std::sync::Arc::clone(&events);
        harness.connection_event_handlers.insert(
            "recorder".to_string(),
            std::sync::Arc::new(move |event| {
                event_calls.lock().unwrap().push(event);
            }),
        );

        let id = ConnectionId::new();
        Client::handle_incoming_message(
            BidirectionalMessage::ConnectionEstablished { connection_id: id },
            harness.context(),
        )
        .await;

        assert_eq!(*harness.connection_id.read().await, Some(id));
        let first_event = events.lock().unwrap().first().cloned().unwrap();
        assert!(matches!(
            first_event,
            ConnectionEvent::Connected { connection_id } if connection_id == id
        ));

        Client::handle_incoming_message(
            BidirectionalMessage::ConnectionClosed {
                connection_id: id,
                reason: Some("server shutdown".to_string()),
            },
            harness.context(),
        )
        .await;

        assert!(harness.connection_id.read().await.is_none());
        let last_event = events.lock().unwrap().last().cloned().unwrap();
        assert!(matches!(
            last_event,
            ConnectionEvent::Disconnected { reason: Some(reason) } if reason == "server shutdown"
        ));
    }

    #[tokio::test]
    async fn incoming_rpc_request_sends_handler_response_or_method_not_found() {
        let harness = IncomingHarness::new();
        let (tx, mut rx) = mpsc::channel(4);
        *harness.message_tx.write().await = Some(tx);

        let handler: RpcRequestHandler = std::sync::Arc::new(|request| {
            Box::pin(async move {
                JsonRpcResponse::success(
                    serde_json::json!({ "handled": request.method }),
                    request.id.clone(),
                )
            })
        });
        harness
            .rpc_request_handlers
            .insert("client.echo".to_string(), handler);

        Client::handle_incoming_message(
            BidirectionalMessage::Request(JsonRpcRequest::new(
                "client.echo".to_string(),
                None,
                Some(serde_json::json!("known")),
            )),
            harness.context(),
        )
        .await;

        let response = rx.recv().await.expect("handler response sent");
        match response {
            BidirectionalMessage::Response(response) => {
                assert_eq!(response.id, Some(serde_json::json!("known")));
                assert_eq!(
                    response.result,
                    Some(serde_json::json!({"handled": "client.echo"}))
                );
            }
            other => panic!("unexpected outgoing message: {other:?}"),
        }

        Client::handle_incoming_message(
            BidirectionalMessage::Request(JsonRpcRequest::new(
                "client.missing".to_string(),
                None,
                Some(serde_json::json!("missing")),
            )),
            harness.context(),
        )
        .await;

        let response = rx.recv().await.expect("method-not-found response sent");
        match response {
            BidirectionalMessage::Response(response) => {
                assert_eq!(response.id, Some(serde_json::json!("missing")));
                let error = response.error.expect("error response");
                assert_eq!(error.code, ras_jsonrpc_types::error_codes::METHOD_NOT_FOUND);
                assert_eq!(error.message, "Method not found");
            }
            other => panic!("unexpected outgoing message: {other:?}"),
        }

        Client::handle_incoming_message(
            BidirectionalMessage::Request(JsonRpcRequest::new(
                "client.echo".to_string(),
                None,
                None,
            )),
            harness.context(),
        )
        .await;

        assert!(rx.try_recv().is_err());
    }
}
