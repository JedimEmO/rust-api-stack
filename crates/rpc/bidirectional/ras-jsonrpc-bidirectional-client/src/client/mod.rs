//! Main client implementation for bidirectional JSON-RPC communication

use crate::{
    ClientState, ConnectionEvent, ConnectionEventHandler, NotificationHandler, PendingRequest,
    RpcRequestHandler, Subscription, WebSocketTransport,
    config::ClientConfig,
    error::{ClientError, ClientResult},
};
use dashmap::DashMap;
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionId};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{debug, info};

#[cfg(not(target_arch = "wasm32"))]
use crate::native::NativeWebSocketTransport;

#[cfg(target_arch = "wasm32")]
use crate::wasm::WasmWebSocketTransport;

mod builder;
mod driver;
pub use builder::ClientBuilder;

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
    /// Signaled when the server's ConnectionEstablished message arrives
    connected_notify: Arc<tokio::sync::Notify>,
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
            connected_notify: Arc::new(tokio::sync::Notify::new()),
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

        let mut transport = self.transport.write().await;
        transport
            .connect()
            .await
            .map_err(|e| ClientError::connection(format!("Failed to connect: {}", e)))?;
        drop(transport);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (message_tx, message_rx) = mpsc::channel(self.config.message_buffer_size);

        *self.shutdown_tx.write().await = Some(shutdown_tx);
        *self.message_tx.write().await = Some(message_tx);

        self.start_message_handler(message_rx, shutdown_rx).await?;

        // Wait for the server's ConnectionEstablished message before
        // reporting the client as connected. The notify is signaled by the
        // message handler; bound the wait so a silent server cannot hang us.
        let handshake = async {
            loop {
                if self.connection_id.read().await.is_some() {
                    break;
                }
                self.connected_notify.notified().await;
            }
        };
        if tokio::time::timeout(self.config.connection_timeout, handshake)
            .await
            .is_err()
        {
            // Tear down the half-open connection
            let _ = self.disconnect().await;
            return Err(ClientError::timeout(
                self.config.connection_timeout.as_secs(),
            ));
        }

        *self.state.write().await = ClientState::Connected;

        // Start heartbeat once connected (its loop exits when state leaves
        // Connected, so starting earlier would race it to an immediate stop)
        if let Some(interval) = self.config.heartbeat_interval {
            self.start_heartbeat(interval).await;
        }

        info!("Client connected to {}", self.config.url);

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

        if let Some(shutdown_tx) = self.shutdown_tx.write().await.take() {
            let _ = shutdown_tx.send(());
        }

        let mut transport = self.transport.write().await;
        transport
            .disconnect()
            .await
            .map_err(|e| ClientError::connection(format!("Failed to disconnect: {}", e)))?;

        *self.connection_id.write().await = None;
        *self.message_tx.write().await = None;

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

        if self.pending_requests.len() >= self.config.max_pending_requests {
            return Err(ClientError::internal("Too many pending requests"));
        }

        self.pending_requests.insert(request_id.clone(), pending);

        // Send the request; on failure, drop our pending entry so the map
        // cannot fill up with waiters that will never be answered.
        let message = BidirectionalMessage::Request(request);
        if let Err(e) = self.send_message(message).await {
            self.pending_requests.remove(&request_id);
            return Err(e);
        }

        // Wait for response with timeout; every failure path removes our
        // entry for the same reason (the success path is removed by the
        // message handler when the response arrives).
        match tokio::time::timeout(self.config.request_timeout, response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                self.pending_requests.remove(&request_id);
                Err(ClientError::internal("Response channel closed"))
            }
            Err(_) => {
                self.pending_requests.remove(&request_id);
                Err(ClientError::timeout(self.config.request_timeout.as_secs()))
            }
        }
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

#[cfg(test)]
mod tests;
